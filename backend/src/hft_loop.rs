//! HFT Trading Loop - Unified Scan + Execute Architecture
//!
//! Design Principles:
//! 1. SEQUENTIAL: Scan → Find First Opportunity → Execute → Complete
//! 2. NO INTERRUPTIONS: Hot path is locked, events are ignored during execution
//! 3. SPEED: No sorting, no extra checks in hot path - first profitable = go
//! 4. COLD PATH: All validation happens AFTER trade completes
//!
//! State Machine:
//! IDLE → [event trigger] → HOT_PATH → [complete/fail] → COLD_PATH → IDLE
//!                                                              ↓
//!                                                          [circuit break]
//!                                                              ↓
//!                                                           STOPPED
#![allow(dead_code)]

use crate::config_manager::ConfigManager;
use crate::db::{Database, NewLiveTrade};
use crate::executor::ExecutionEngine;
use crate::order_book::OrderBookCache;
use crate::scanner::Scanner;
use crate::types::Opportunity;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};

/// HFT Loop State
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HftState {
    /// Waiting for order book update event
    Idle,
    /// Scanning and executing (locked, no interruptions)
    HotPath,
    /// Validating trade result, updating stats
    ColdPath,
    /// Circuit breaker tripped, trading stopped
    Stopped,
}

/// Per-leg timing data for database storage
#[derive(Debug, Clone, serde::Serialize)]
pub struct LegTiming {
    pub leg: usize,
    pub pair: String,
    pub side: String,
    pub duration_ms: u64,
    pub success: bool,
    pub error: Option<String>,
}

/// Result of a single trading cycle
#[derive(Debug, Clone)]
pub enum CycleResult {
    /// No opportunity found, back to IDLE
    NoOpportunity,
    /// Trade executed successfully
    TradeSuccess {
        path: String,
        profit_pct: f64,
        profit_amount: f64,
        duration_ms: u64,
        leg_timings: Vec<LegTiming>,
    },
    /// Trade failed (partial or error)
    TradeFailed {
        path: String,
        error: String,
        is_partial: bool,
        leg_timings: Vec<LegTiming>,
    },
    /// Circuit breaker tripped
    CircuitBroken {
        reason: String,
    },
}

/// Cold path decision after trade
#[derive(Debug, Clone)]
pub enum ColdPathDecision {
    /// Continue trading
    Continue,
    /// Stop trading (circuit breaker)
    Stop { reason: String },
}

/// HFT Loop Statistics
#[derive(Debug, Clone, Default)]
pub struct HftStats {
    pub cycles_completed: u64,
    pub opportunities_found: u64,
    pub trades_executed: u64,
    pub trades_successful: u64,
    pub trades_failed: u64,
    pub trades_partial: u64,
    pub total_profit: f64,
    pub total_loss: f64,
    pub daily_profit: f64,
    pub daily_loss: f64,
    pub events_received: u64,
    pub events_ignored_in_hot_path: u64,
}

/// Configuration for HFT Loop
#[derive(Debug, Clone)]
pub struct HftConfig {
    /// Minimum profit threshold (from user, can be negative for test mode)
    pub min_profit_threshold: f64,
    /// Trade amount in USD
    pub trade_amount: f64,
    /// Maximum daily loss before circuit break
    pub max_daily_loss: f64,
    /// Maximum total loss before circuit break
    pub max_total_loss: f64,
    /// Base currencies to scan (USD, EUR, etc.)
    pub base_currencies: Vec<String>,
}

/// Unified HFT Trading Loop
pub struct HftLoop {
    // State
    state: Arc<RwLock<HftState>>,
    stats: Arc<RwLock<HftStats>>,
    config: Arc<RwLock<HftConfig>>,

    // Core components
    cache: Arc<OrderBookCache>,
    config_manager: Arc<ConfigManager>,
    execution_engine: Arc<RwLock<Option<ExecutionEngine>>>,
    db: Database,

    // Control flags
    is_running: Arc<AtomicBool>,

    // Counters
    cycle_count: Arc<AtomicU64>,
}

impl HftLoop {
    pub fn new(
        cache: Arc<OrderBookCache>,
        config_manager: Arc<ConfigManager>,
        db: Database,
    ) -> Self {
        Self {
            state: Arc::new(RwLock::new(HftState::Idle)),
            stats: Arc::new(RwLock::new(HftStats::default())),
            config: Arc::new(RwLock::new(HftConfig {
                min_profit_threshold: 0.0,
                trade_amount: 10.0,
                max_daily_loss: 100.0,
                max_total_loss: 500.0,
                base_currencies: vec!["USD".to_string()],
            })),
            cache,
            config_manager,
            execution_engine: Arc::new(RwLock::new(None)),
            db,
            is_running: Arc::new(AtomicBool::new(false)),
            cycle_count: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Update configuration from database
    pub async fn update_config(&self, config: HftConfig) {
        *self.config.write().await = config;
    }

    /// Set execution engine
    pub async fn set_execution_engine(&self, engine: ExecutionEngine) {
        *self.execution_engine.write().await = Some(engine);
    }

    /// Get current state
    pub async fn get_state(&self) -> HftState {
        *self.state.read().await
    }

    /// Get statistics
    pub async fn get_stats(&self) -> HftStats {
        self.stats.read().await.clone()
    }

    /// Create event channel for order book updates
    pub fn create_event_channel(&mut self) -> mpsc::Sender<String> {
        let (tx, rx) = mpsc::channel(1000);

        // Spawn the main loop
        let state = Arc::clone(&self.state);
        let stats = Arc::clone(&self.stats);
        let config = Arc::clone(&self.config);
        let cache = Arc::clone(&self.cache);
        let config_manager = Arc::clone(&self.config_manager);
        let execution_engine = Arc::clone(&self.execution_engine);
        let is_running = Arc::clone(&self.is_running);
        let cycle_count = Arc::clone(&self.cycle_count);
        let db = self.db.clone();

        tokio::spawn(async move {
            Self::run_loop(
                rx,
                state,
                stats,
                config,
                cache,
                config_manager,
                execution_engine,
                is_running,
                cycle_count,
                db,
            ).await;
        });

        tx
    }

    /// Main HFT loop - processes events and executes trades
    async fn run_loop(
        mut event_rx: mpsc::Receiver<String>,
        state: Arc<RwLock<HftState>>,
        stats: Arc<RwLock<HftStats>>,
        config: Arc<RwLock<HftConfig>>,
        cache: Arc<OrderBookCache>,
        config_manager: Arc<ConfigManager>,
        execution_engine: Arc<RwLock<Option<ExecutionEngine>>>,
        is_running: Arc<AtomicBool>,
        cycle_count: Arc<AtomicU64>,
        db: Database,
    ) {
        info!("HFT Loop started");
        is_running.store(true, Ordering::SeqCst);

        while is_running.load(Ordering::SeqCst) {
            // Wait for event (only when IDLE)
            let current_state = *state.read().await;

            match current_state {
                HftState::Stopped => {
                    // Circuit breaker tripped - wait for manual reset
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    continue;
                }
                HftState::Idle => {
                    // Wait for order book update event
                    match event_rx.recv().await {
                        Some(_pair) => {
                            stats.write().await.events_received += 1;

                            // Transition to HOT_PATH
                            *state.write().await = HftState::HotPath;
                        }
                        None => {
                            // Channel closed
                            info!("Event channel closed, stopping HFT loop");
                            break;
                        }
                    }
                }
                HftState::HotPath => {
                    // Should not happen - we handle hot path immediately below
                    warn!("Unexpected state: HotPath without processing");
                }
                HftState::ColdPath => {
                    // Should not happen - we handle cold path immediately below
                    warn!("Unexpected state: ColdPath without processing");
                }
            }

            // Check if we're in HOT_PATH
            if *state.read().await != HftState::HotPath {
                continue;
            }

            // ============================================
            // HOT PATH - No interruptions, no extra checks
            // ============================================

            let cycle_result = Self::execute_hot_path(
                &cache,
                &config_manager,
                &execution_engine,
                &config,
            ).await;

            cycle_count.fetch_add(1, Ordering::Relaxed);

            // ============================================
            // COLD PATH - Validation and decision
            // ============================================

            *state.write().await = HftState::ColdPath;

            let decision = Self::execute_cold_path(
                &cycle_result,
                &stats,
                &config,
                &db,
            ).await;

            // Update state based on decision
            match decision {
                ColdPathDecision::Continue => {
                    *state.write().await = HftState::Idle;
                }
                ColdPathDecision::Stop { reason } => {
                    warn!("Circuit breaker tripped: {}", reason);
                    *state.write().await = HftState::Stopped;
                }
            }
        }

        info!("HFT Loop stopped");
        is_running.store(false, Ordering::SeqCst);
    }

    /// HOT PATH: Scan → Find First → Execute
    /// SPEED CRITICAL - No extra checks, no delays
    async fn execute_hot_path(
        cache: &Arc<OrderBookCache>,
        config_manager: &Arc<ConfigManager>,
        execution_engine: &Arc<RwLock<Option<ExecutionEngine>>>,
        hft_config: &Arc<RwLock<HftConfig>>,
    ) -> CycleResult {
        let hot_path_start = std::time::Instant::now();

        let config = hft_config.read().await;
        let engine_config = config_manager.get_config();

        // Step 1: Create scanner and find FIRST profitable opportunity
        let scan_start = std::time::Instant::now();
        let scanner = Scanner::new(Arc::clone(cache), engine_config);

        // Scan - but we only care about the FIRST opportunity that meets threshold
        let opportunity = Self::find_first_opportunity(
            &scanner,
            &config.base_currencies,
            config.min_profit_threshold,
        );
        let scan_ms = scan_start.elapsed().as_micros() as f64 / 1000.0;

        let opp = match opportunity {
            Some(o) => o,
            None => {
                // Log every 100th scan to avoid spam
                static SCAN_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                let count = SCAN_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if count % 100 == 0 {
                    info!("📊 Scanned {} times, no opportunity above threshold (scan: {:.2}ms)", count + 1, scan_ms);
                }
                return CycleResult::NoOpportunity;
            }
        };

        info!("🎯 Found opportunity: {} | {:.3}% | scan: {:.2}ms", opp.path, opp.net_profit_pct, scan_ms);

        // Step 2: Execute immediately - no more checks
        let engine_guard = execution_engine.read().await;
        let engine = match engine_guard.as_ref() {
            Some(e) => e,
            None => {
                warn!("Execution engine not available");
                return CycleResult::TradeFailed {
                    path: opp.path,
                    error: "Execution engine not available".to_string(),
                    is_partial: false,
                    leg_timings: vec![],
                };
            }
        };

        let trade_amount = config.trade_amount;
        drop(config); // Release lock before async call

        // Execute the trade
        let start = std::time::Instant::now();
        let result = engine.execute_opportunity(&opp, trade_amount).await;
        let duration_ms = start.elapsed().as_millis() as u64;

        let total_hot_path_ms = hot_path_start.elapsed().as_millis() as u64;

        match result {
            Ok(trade_result) => {
                // Build leg timings and log string in single pass (post-execution, not time-critical)
                let mut leg_timings = Vec::with_capacity(trade_result.legs.len());
                let mut leg_times_parts = Vec::with_capacity(trade_result.legs.len());
                let mut completed_legs = 0usize;

                for l in &trade_result.legs {
                    leg_timings.push(LegTiming {
                        leg: l.leg_index + 1,
                        pair: l.pair.clone(),
                        side: l.side.clone(),
                        duration_ms: l.duration_ms,
                        success: l.success,
                        error: l.error.clone(),
                    });
                    if l.success {
                        completed_legs += 1;
                        leg_times_parts.push(format!("L{}:{}ms", l.leg_index + 1, l.duration_ms));
                    } else {
                        leg_times_parts.push(format!("L{}:{}ms✗", l.leg_index + 1, l.duration_ms));
                    }
                }
                let leg_times_str = leg_times_parts.join(", ");

                if trade_result.success {
                    info!(
                        "💰 Trade SUCCESS: {} | ${:.4} ({:.3}%) | scan: {:.2}ms | legs: [{}] | exec: {}ms | total: {}ms",
                        trade_result.path, trade_result.profit_amount, trade_result.profit_pct,
                        scan_ms, leg_times_str, duration_ms, total_hot_path_ms
                    );
                    CycleResult::TradeSuccess {
                        path: trade_result.path,
                        profit_pct: trade_result.profit_pct,
                        profit_amount: trade_result.profit_amount,
                        duration_ms,
                        leg_timings,
                    }
                } else {
                    let is_partial = completed_legs > 0 && completed_legs < trade_result.legs.len();

                    warn!(
                        "❌ Trade FAILED: {} | {} | scan: {:.2}ms | legs: [{}] | exec: {}ms | total: {}ms",
                        trade_result.path,
                        trade_result.error.as_deref().unwrap_or("Unknown error"),
                        scan_ms, leg_times_str, duration_ms, total_hot_path_ms
                    );

                    CycleResult::TradeFailed {
                        path: trade_result.path,
                        error: trade_result.error.unwrap_or_else(|| "Unknown error".to_string()),
                        is_partial,
                        leg_timings,
                    }
                }
            }
            Err(e) => {
                warn!("❌ Execution error: {} | {} | exec: {}ms | total: {}ms (scan: {:.2}ms)",
                    opp.path, e, duration_ms, total_hot_path_ms, scan_ms);
                CycleResult::TradeFailed {
                    path: opp.path,
                    error: e.to_string(),
                    is_partial: false,
                    leg_timings: vec![],
                }
            }
        }
    }

    /// Find the FIRST opportunity that meets threshold
    /// Uses HFT-optimized scan_first() - stops DFS at first profitable path
    fn find_first_opportunity(
        scanner: &Scanner,
        base_currencies: &[String],
        min_threshold: f64,
    ) -> Option<Opportunity> {
        // HFT-optimized: stops immediately on first profitable opportunity
        // No sorting, no collecting all paths - pure speed
        scanner.scan_first(base_currencies, min_threshold)
    }

    /// COLD PATH: Validate results, update stats, check circuit breakers
    async fn execute_cold_path(
        cycle_result: &CycleResult,
        stats: &Arc<RwLock<HftStats>>,
        config: &Arc<RwLock<HftConfig>>,
        db: &Database,
    ) -> ColdPathDecision {
        // Read config once at the start (before acquiring stats lock)
        let config_snapshot = config.read().await.clone();

        // Update stats (short critical section)
        let (daily_loss, total_loss) = {
            let mut stats_guard = stats.write().await;
            stats_guard.cycles_completed += 1;

            match cycle_result {
                CycleResult::NoOpportunity => {
                    return ColdPathDecision::Continue;
                }
                CycleResult::TradeSuccess { profit_amount, .. } => {
                    stats_guard.opportunities_found += 1;
                    stats_guard.trades_executed += 1;
                    stats_guard.trades_successful += 1;

                    if *profit_amount >= 0.0 {
                        stats_guard.total_profit += profit_amount;
                        stats_guard.daily_profit += profit_amount;
                    } else {
                        stats_guard.total_loss += profit_amount.abs();
                        stats_guard.daily_loss += profit_amount.abs();
                    }
                }
                CycleResult::TradeFailed { is_partial, .. } => {
                    stats_guard.opportunities_found += 1;
                    stats_guard.trades_executed += 1;
                    stats_guard.trades_failed += 1;
                    if *is_partial {
                        stats_guard.trades_partial += 1;
                    }
                }
                CycleResult::CircuitBroken { reason } => {
                    return ColdPathDecision::Stop { reason: reason.clone() };
                }
            }

            (stats_guard.daily_loss, stats_guard.total_loss)
        }; // Stats lock released here

        // Save to database (no locks held)
        match cycle_result {
            CycleResult::TradeSuccess { path, profit_pct, profit_amount, duration_ms, leg_timings } => {
                // Serialize leg timings to JSON
                let leg_fills_json = serde_json::to_value(leg_timings).ok();

                let new_trade = NewLiveTrade {
                    trade_id: uuid::Uuid::new_v4().to_string(),
                    path: path.clone(),
                    legs: path.matches(" → ").count() as i32 + 1,
                    amount_in: config_snapshot.trade_amount,
                    amount_out: Some(config_snapshot.trade_amount + profit_amount),
                    profit_loss: Some(*profit_amount),
                    profit_loss_pct: Some(*profit_pct),
                    status: "COMPLETED".to_string(),
                    current_leg: None,
                    error_message: None,
                    held_currency: None,
                    held_amount: None,
                    held_value_usd: None,
                    order_ids: None,
                    leg_fills: leg_fills_json,
                    started_at: Some(chrono::Utc::now()),
                    completed_at: Some(chrono::Utc::now()),
                    total_execution_ms: Some(*duration_ms as f64),
                    opportunity_profit_pct: Some(*profit_pct),
                };

                if let Err(e) = db.save_trade(&new_trade).await {
                    warn!("Failed to save trade to DB: {}", e);
                }

                // Update trading state with trade result
                let is_win = *profit_amount > 0.0;
                if let Err(e) = db.record_trade_result(*profit_amount, config_snapshot.trade_amount, is_win).await {
                    warn!("Failed to update trading state: {}", e);
                }

                // Check circuit breakers (using snapshot values)
                if daily_loss > config_snapshot.max_daily_loss {
                    return ColdPathDecision::Stop {
                        reason: format!(
                            "Daily loss limit exceeded: ${:.2} > ${:.2}",
                            daily_loss, config_snapshot.max_daily_loss
                        ),
                    };
                }
                if total_loss > config_snapshot.max_total_loss {
                    return ColdPathDecision::Stop {
                        reason: format!(
                            "Total loss limit exceeded: ${:.2} > ${:.2}",
                            total_loss, config_snapshot.max_total_loss
                        ),
                    };
                }
            }

            CycleResult::TradeFailed { path, error, is_partial, leg_timings } => {
                // Serialize leg timings to JSON (even partial data is useful)
                let leg_fills_json = if leg_timings.is_empty() {
                    None
                } else {
                    serde_json::to_value(leg_timings).ok()
                };

                let new_trade = NewLiveTrade {
                    trade_id: uuid::Uuid::new_v4().to_string(),
                    path: path.clone(),
                    legs: path.matches(" → ").count() as i32 + 1,
                    amount_in: config_snapshot.trade_amount,
                    amount_out: None,
                    profit_loss: None,
                    profit_loss_pct: None,
                    status: if *is_partial { "PARTIAL".to_string() } else { "FAILED".to_string() },
                    current_leg: None,
                    error_message: Some(error.clone()),
                    held_currency: None,
                    held_amount: None,
                    held_value_usd: None,
                    order_ids: None,
                    leg_fills: leg_fills_json,
                    started_at: Some(chrono::Utc::now()),
                    completed_at: Some(chrono::Utc::now()),
                    total_execution_ms: None,
                    opportunity_profit_pct: None,
                };

                if let Err(e) = db.save_trade(&new_trade).await {
                    warn!("Failed to save failed trade to DB: {}", e);
                }
            }

            // NoOpportunity and CircuitBroken are handled in stats update block above
            _ => {}
        }

        ColdPathDecision::Continue
    }

    /// Stop the HFT loop
    pub fn stop(&self) {
        self.is_running.store(false, Ordering::SeqCst);
        info!("HFT Loop stop requested");
    }

    /// Reset circuit breaker and resume trading
    pub async fn reset_circuit_breaker(&self) {
        let mut state = self.state.write().await;
        if *state == HftState::Stopped {
            *state = HftState::Idle;
            info!("Circuit breaker reset, resuming trading");
        }
    }

    /// Reset daily statistics
    pub async fn reset_daily_stats(&self) {
        let mut stats = self.stats.write().await;
        stats.daily_profit = 0.0;
        stats.daily_loss = 0.0;
        info!("Daily stats reset");
    }

    /// Check if loop is running
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::Relaxed)
    }
}
