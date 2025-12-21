//! Event System for Event-Driven Scanning with Auto-Execution
//!
//! Provides callbacks and event handling for:
//! - Order book updates (triggers scan)
//! - Opportunity detection (auto-executes if enabled)
//! - Trade execution results (notifies Python via callback)
//!
//! Features:
//! - Debounced scan triggering (50ms window)
//! - Parallel scanning across base currencies
//! - Auto-execution in Rust (no Python polling)
//! - Callback to Python for DB logging only
//! - Incremental graph updates (Phase 3)

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use chrono;
use uuid;

use crate::graph_manager::PersistentGraph;
use crate::scanner::Scanner;
use crate::order_book::OrderBookCache;
use crate::config_manager::ConfigManager;
use crate::types::Opportunity;
use crate::trading_config::{TradingGuard, TradeResult, LegResult};
use crate::executor::ExecutionEngine;

/// Scan trigger mode
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScanTriggerMode {
    /// Scan on every order book update (high frequency)
    Immediate,
    /// Debounce scans with a time window (default: 50ms)
    Debounced(u64),
    /// Disable event-driven scanning (use polling)
    Disabled,
}

impl Default for ScanTriggerMode {
    fn default() -> Self {
        ScanTriggerMode::Debounced(50) // 50ms debounce by default
    }
}

/// Trade result callback for notifying Python
pub type TradeResultCallback = Box<dyn Fn(TradeResult) + Send + Sync>;

/// Event-driven scanner that triggers on order book updates
/// With auto-execution capability (full Rust pipeline)
pub struct EventDrivenScanner {
    cache: Arc<OrderBookCache>,
    config_manager: Arc<ConfigManager>,

    // Persistent graph for incremental updates (Phase 3)
    persistent_graph: Arc<RwLock<PersistentGraph>>,
    use_incremental: Arc<AtomicBool>,

    // Event state
    trigger_mode: Arc<RwLock<ScanTriggerMode>>,
    pending_pairs: Arc<RwLock<HashSet<String>>>,
    last_scan_time: Arc<RwLock<Instant>>,
    scan_in_progress: Arc<AtomicBool>,

    // Statistics
    event_count: Arc<AtomicU64>,
    scan_count: Arc<AtomicU64>,
    opportunities_found: Arc<AtomicU64>,
    incremental_updates: Arc<AtomicU64>,
    full_rebuilds: Arc<AtomicU64>,
    auto_executions: Arc<AtomicU64>,
    auto_execution_successes: Arc<AtomicU64>,

    // Channel for opportunities
    opportunity_tx: Option<mpsc::UnboundedSender<Opportunity>>,

    // Base currencies to scan
    base_currencies: Arc<RwLock<Vec<String>>>,

    // Cached opportunities (for Python to fetch)
    cached_opportunities: Arc<RwLock<Vec<Opportunity>>>,
    last_scan_result_time: Arc<RwLock<Instant>>,

    // Auto-execution components
    trading_guard: Arc<RwLock<Option<Arc<TradingGuard>>>>,
    execution_engine: Arc<RwLock<Option<Arc<RwLock<Option<ExecutionEngine>>>>>>,
    auto_execution_enabled: Arc<AtomicBool>,

    // Tokio runtime for async execution
    runtime: Arc<RwLock<Option<Arc<tokio::runtime::Runtime>>>>,

    // Callback channel for trade results (to notify Python)
    trade_result_tx: Arc<RwLock<Option<mpsc::UnboundedSender<TradeResult>>>>,
}

impl EventDrivenScanner {
    pub fn new(
        cache: Arc<OrderBookCache>,
        config_manager: Arc<ConfigManager>,
    ) -> Self {
        Self {
            cache,
            config_manager,
            persistent_graph: Arc::new(RwLock::new(PersistentGraph::new())),
            use_incremental: Arc::new(AtomicBool::new(true)), // Enable by default
            trigger_mode: Arc::new(RwLock::new(ScanTriggerMode::default())),
            pending_pairs: Arc::new(RwLock::new(HashSet::new())),
            last_scan_time: Arc::new(RwLock::new(Instant::now())),
            scan_in_progress: Arc::new(AtomicBool::new(false)),
            event_count: Arc::new(AtomicU64::new(0)),
            scan_count: Arc::new(AtomicU64::new(0)),
            opportunities_found: Arc::new(AtomicU64::new(0)),
            incremental_updates: Arc::new(AtomicU64::new(0)),
            full_rebuilds: Arc::new(AtomicU64::new(0)),
            auto_executions: Arc::new(AtomicU64::new(0)),
            auto_execution_successes: Arc::new(AtomicU64::new(0)),
            opportunity_tx: None,
            base_currencies: Arc::new(RwLock::new(vec![
                "USD".to_string(),
                "EUR".to_string(),
            ])),
            cached_opportunities: Arc::new(RwLock::new(Vec::new())),
            last_scan_result_time: Arc::new(RwLock::new(Instant::now())),
            trading_guard: Arc::new(RwLock::new(None)),
            execution_engine: Arc::new(RwLock::new(None)),
            auto_execution_enabled: Arc::new(AtomicBool::new(false)),
            runtime: Arc::new(RwLock::new(None)),
            trade_result_tx: Arc::new(RwLock::new(None)),
        }
    }

    /// Set up auto-execution with trading guard and execution engine
    pub fn setup_auto_execution(
        &self,
        trading_guard: Arc<TradingGuard>,
        execution_engine: Arc<RwLock<Option<ExecutionEngine>>>,
        runtime: Arc<tokio::runtime::Runtime>,
    ) {
        *self.trading_guard.write() = Some(trading_guard);
        *self.execution_engine.write() = Some(execution_engine);
        *self.runtime.write() = Some(runtime);
        info!("Auto-execution setup complete");
    }

    /// Enable auto-execution (Rust handles full pipeline)
    pub fn enable_auto_execution(&self) {
        self.auto_execution_enabled.store(true, Ordering::SeqCst);
        info!("Auto-execution ENABLED in event scanner");
    }

    /// Disable auto-execution
    pub fn disable_auto_execution(&self) {
        self.auto_execution_enabled.store(false, Ordering::SeqCst);
        info!("Auto-execution DISABLED in event scanner");
    }

    /// Check if auto-execution is enabled
    pub fn is_auto_execution_enabled(&self) -> bool {
        self.auto_execution_enabled.load(Ordering::Relaxed)
    }

    /// Create trade result channel for Python to receive results
    pub fn create_trade_result_channel(&self) -> mpsc::UnboundedReceiver<TradeResult> {
        let (tx, rx) = mpsc::unbounded_channel();
        *self.trade_result_tx.write() = Some(tx);
        rx
    }

    /// Get auto-execution statistics
    pub fn get_auto_execution_stats(&self) -> (u64, u64) {
        (
            self.auto_executions.load(Ordering::Relaxed),
            self.auto_execution_successes.load(Ordering::Relaxed),
        )
    }

    /// Initialize the persistent graph with current cache data
    pub fn initialize_graph(&self) {
        let mut graph = self.persistent_graph.write();
        graph.initialize(&self.cache);
        graph.update_all(&self.cache);
        info!("Persistent graph initialized for incremental updates");
    }

    /// Enable or disable incremental graph updates
    pub fn set_incremental_mode(&self, enabled: bool) {
        self.use_incremental.store(enabled, Ordering::SeqCst);
        info!("Incremental graph mode: {}", if enabled { "enabled" } else { "disabled" });
    }

    /// Check if incremental mode is enabled
    pub fn is_incremental_enabled(&self) -> bool {
        self.use_incremental.load(Ordering::SeqCst)
    }

    /// Set the trigger mode
    pub fn set_trigger_mode(&self, mode: ScanTriggerMode) {
        *self.trigger_mode.write() = mode;
        info!("Scan trigger mode set to: {:?}", mode);
    }

    /// Get the trigger mode
    pub fn get_trigger_mode(&self) -> ScanTriggerMode {
        *self.trigger_mode.read()
    }

    /// Set base currencies to scan for opportunities
    pub fn set_base_currencies(&self, currencies: Vec<String>) {
        *self.base_currencies.write() = currencies;
    }

    /// Get opportunity receiver channel
    pub fn get_opportunity_receiver(&mut self) -> mpsc::UnboundedReceiver<Opportunity> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.opportunity_tx = Some(tx);
        rx
    }

    /// Called when an order book is updated
    /// This is the main entry point for event-driven scanning
    pub fn on_orderbook_update(&self, pair: &str) {
        self.event_count.fetch_add(1, Ordering::Relaxed);

        // If incremental mode is enabled, update the graph edge for this pair
        if self.use_incremental.load(Ordering::Relaxed) {
            let mut graph = self.persistent_graph.write();
            if graph.update_pair(&self.cache, pair) {
                self.incremental_updates.fetch_add(1, Ordering::Relaxed);
            }
        }

        let mode = *self.trigger_mode.read();

        match mode {
            ScanTriggerMode::Disabled => {
                // Do nothing - rely on polling
            }
            ScanTriggerMode::Immediate => {
                // Trigger scan immediately
                self.try_scan();
            }
            ScanTriggerMode::Debounced(ms) => {
                // Add to pending pairs
                self.pending_pairs.write().insert(pair.to_string());

                // Check if we should trigger a scan
                let last = *self.last_scan_time.read();
                if last.elapsed() >= Duration::from_millis(ms) {
                    self.try_scan();
                }
            }
        }
    }

    /// Try to run a scan (if not already running)
    /// If auto-execution is enabled, will also execute profitable opportunities
    fn try_scan(&self) {
        // Check if a scan is already in progress
        if self.scan_in_progress.compare_exchange(
            false,
            true,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ).is_err() {
            debug!("Scan already in progress, skipping");
            return;
        }

        // Clear pending pairs
        let _pending: HashSet<String> = std::mem::take(&mut *self.pending_pairs.write());

        // Update last scan time
        *self.last_scan_time.write() = Instant::now();

        // Run scan
        let opportunities = self.run_scan();

        // Update stats
        self.scan_count.fetch_add(1, Ordering::Relaxed);
        self.opportunities_found.fetch_add(opportunities.len() as u64, Ordering::Relaxed);

        // Cache opportunities for Python to fetch
        {
            let mut cached = self.cached_opportunities.write();
            *cached = opportunities.clone();
            *self.last_scan_result_time.write() = Instant::now();
        }

        // AUTO-EXECUTION: If enabled, execute profitable opportunities immediately
        if self.auto_execution_enabled.load(Ordering::Relaxed) {
            self.try_auto_execute(&opportunities);
        }

        // Send opportunities to channel (if receiver exists)
        if let Some(ref tx) = self.opportunity_tx {
            for opp in opportunities {
                if tx.send(opp).is_err() {
                    warn!("Failed to send opportunity to channel");
                    break;
                }
            }
        }

        // Clear in-progress flag
        self.scan_in_progress.store(false, Ordering::SeqCst);
    }

    /// Try to auto-execute profitable opportunities
    fn try_auto_execute(&self, opportunities: &[Opportunity]) {
        // Get trading guard
        let guard_lock = self.trading_guard.read();
        let trading_guard = match guard_lock.as_ref() {
            Some(g) => g,
            None => {
                debug!("Auto-execution: No trading guard configured");
                return;
            }
        };

        // Check if trading is enabled
        if !trading_guard.is_enabled() {
            return;
        }

        // Get execution engine
        let engine_lock = self.execution_engine.read();
        let execution_engine_arc = match engine_lock.as_ref() {
            Some(e) => e,
            None => {
                debug!("Auto-execution: No execution engine configured");
                return;
            }
        };

        // Get runtime
        let runtime_lock = self.runtime.read();
        let runtime = match runtime_lock.as_ref() {
            Some(r) => r,
            None => {
                debug!("Auto-execution: No runtime configured");
                return;
            }
        };

        // Get config for profit threshold and trade amount
        let config = trading_guard.get_config();

        // Filter profitable opportunities
        let profitable: Vec<&Opportunity> = opportunities
            .iter()
            .filter(|o| o.is_profitable && o.net_profit_pct >= config.min_profit_threshold)
            .collect();

        if profitable.is_empty() {
            return;
        }

        // Try to execute the best opportunity
        for opp in profitable.iter().take(1) {  // Only try best one per scan cycle
            // Check all guards
            let guard_result = trading_guard.check_opportunity(&opp.path, opp.net_profit_pct);
            if !guard_result.can_trade {
                debug!("Guard blocked: {:?}", guard_result.reason);
                continue;
            }

            // Try to start execution (prevents concurrent executions in sequential mode)
            if !trading_guard.try_start_execution() {
                debug!("Another trade already executing");
                continue;
            }

            info!(
                "🚀 Auto-executing: {} | Expected profit: {:.3}%",
                opp.path, opp.net_profit_pct
            );

            self.auto_executions.fetch_add(1, Ordering::Relaxed);

            // Execute via Rust execution engine
            let execution_result = {
                let engine_guard = execution_engine_arc.read();
                match engine_guard.as_ref() {
                    Some(engine) => {
                        // Create opportunity for execution
                        let exec_opp = Opportunity {
                            id: uuid::Uuid::new_v4().to_string(),
                            path: opp.path.clone(),
                            legs: opp.legs,
                            gross_profit_pct: opp.gross_profit_pct,
                            fees_pct: opp.fees_pct,
                            net_profit_pct: opp.net_profit_pct,
                            is_profitable: opp.is_profitable,
                            detected_at: chrono::Utc::now(),
                        };

                        // Execute synchronously (blocking)
                        runtime.block_on(async {
                            engine.execute_opportunity(&exec_opp, config.trade_amount).await
                        })
                    }
                    None => {
                        trading_guard.finish_execution();
                        warn!("Execution engine not available");
                        continue;
                    }
                }
            };

            // Finish execution flag
            trading_guard.finish_execution();

            // Process result
            match execution_result {
                Ok(trade_result) => {
                    // Create TradeResult for recording and callback
                    let result = TradeResult {
                        trade_id: trade_result.id.clone(),
                        path: trade_result.path.clone(),
                        status: if trade_result.success { "COMPLETED".to_string() } else { "FAILED".to_string() },
                        legs_completed: trade_result.legs.iter().filter(|l| l.success).count(),
                        total_legs: trade_result.legs.len(),
                        amount_in: trade_result.start_amount,
                        amount_out: trade_result.end_amount,
                        profit_pct: trade_result.profit_pct,
                        profit_amount: trade_result.profit_amount,
                        execution_time_ms: trade_result.total_duration_ms,
                        error: trade_result.error.clone(),
                        leg_details: trade_result.legs.iter().map(|l| {
                            LegResult {
                                leg_index: l.leg_index,
                                pair: l.pair.clone(),
                                side: l.side.clone(),
                                amount_in: l.input_amount,
                                amount_out: l.output_amount,
                                price: l.avg_price,
                                fee: l.fee,
                                status: if l.success { "FILLED".to_string() } else { "FAILED".to_string() },
                            }
                        }).collect(),
                    };

                    // Record in trading guard
                    trading_guard.record_trade(&result);

                    if trade_result.success {
                        self.auto_execution_successes.fetch_add(1, Ordering::Relaxed);
                        info!(
                            "💰 Auto-execution SUCCESS: {} | Profit: ${:.4} ({:.3}%)",
                            result.path, result.profit_amount, result.profit_pct
                        );
                    } else {
                        warn!(
                            "❌ Auto-execution FAILED: {} | Error: {:?}",
                            result.path, result.error
                        );
                    }

                    // Send result to Python via callback channel
                    if let Some(ref tx) = *self.trade_result_tx.read() {
                        if tx.send(result).is_err() {
                            warn!("Failed to send trade result to Python callback channel");
                        }
                    }
                }
                Err(e) => {
                    warn!("Auto-execution error: {}", e);
                }
            }

            // In sequential mode, only execute one per cycle
            if config.execution_mode == "sequential" {
                break;
            }
        }
    }

    /// Run the actual scan
    fn run_scan(&self) -> Vec<Opportunity> {
        let config = self.config_manager.get_config();
        let base_currencies = self.base_currencies.read().clone();

        if self.use_incremental.load(Ordering::Relaxed) {
            // Use persistent graph for scanning (faster)
            let graph = self.persistent_graph.read();
            graph.scan(&base_currencies, &config)
        } else {
            // Fall back to full rebuild scan
            self.full_rebuilds.fetch_add(1, Ordering::Relaxed);
            let scanner = Scanner::new(Arc::clone(&self.cache), config);
            scanner.scan(&base_currencies)
        }
    }

    /// Manually trigger a scan (for polling mode or on-demand)
    pub fn trigger_scan(&self) -> Vec<Opportunity> {
        self.try_scan();
        // Return empty vec since opportunities go to channel
        // For synchronous result, use run_scan directly
        vec![]
    }

    /// Run a synchronous scan and return results directly
    pub fn scan_sync(&self, base_currencies: &[String]) -> Vec<Opportunity> {
        let config = self.config_manager.get_config();
        let scanner = Scanner::new(Arc::clone(&self.cache), config);
        let opportunities = scanner.scan(base_currencies);

        self.scan_count.fetch_add(1, Ordering::Relaxed);
        self.opportunities_found.fetch_add(opportunities.len() as u64, Ordering::Relaxed);

        opportunities
    }

    /// Get cached opportunities (for Python to fetch without triggering scan)
    pub fn get_cached_opportunities(&self) -> Vec<Opportunity> {
        self.cached_opportunities.read().clone()
    }

    /// Get cached opportunities with age info
    pub fn get_cached_opportunities_with_age(&self) -> (Vec<Opportunity>, u64) {
        let opportunities = self.cached_opportunities.read().clone();
        let age_ms = self.last_scan_result_time.read().elapsed().as_millis() as u64;
        (opportunities, age_ms)
    }

    /// Get statistics
    pub fn get_stats(&self) -> EventScannerStats {
        let (graph_builds, graph_updates, nodes, edges) = self.persistent_graph.read().get_stats();

        EventScannerStats {
            event_count: self.event_count.load(Ordering::Relaxed),
            scan_count: self.scan_count.load(Ordering::Relaxed),
            opportunities_found: self.opportunities_found.load(Ordering::Relaxed),
            pending_pairs: self.pending_pairs.read().len(),
            mode: *self.trigger_mode.read(),
            incremental_updates: self.incremental_updates.load(Ordering::Relaxed),
            full_rebuilds: self.full_rebuilds.load(Ordering::Relaxed),
            graph_builds,
            graph_updates,
            graph_nodes: nodes,
            graph_edges: edges,
            incremental_enabled: self.use_incremental.load(Ordering::Relaxed),
        }
    }

    /// Get detailed graph stats including valid edge count
    pub fn get_graph_detailed_stats(&self) -> (usize, usize, usize, usize) {
        self.persistent_graph.read().get_detailed_stats()
    }

    /// Debug: count paths from USD and EUR
    pub fn debug_count_paths(&self) -> (usize, usize) {
        let graph = self.persistent_graph.read();
        (
            graph.count_paths_from("USD"),
            graph.count_paths_from("EUR"),
        )
    }

    /// Debug: get currencies connected to USD
    pub fn debug_get_usd_connections(&self) -> Vec<String> {
        self.persistent_graph.read().get_connected_currencies("USD")
    }

    /// Get order book health from the persistent graph
    pub fn get_orderbook_health(&self) -> crate::types::OrderBookHealth {
        // First update the health stats
        self.persistent_graph.read().update_health_from_cache(&self.cache);
        // Then return the health
        self.persistent_graph.read().get_health()
    }
}

/// Statistics for the event-driven scanner
#[derive(Debug, Clone)]
pub struct EventScannerStats {
    pub event_count: u64,
    pub scan_count: u64,
    pub opportunities_found: u64,
    pub pending_pairs: usize,
    pub mode: ScanTriggerMode,
    pub incremental_updates: u64,
    pub full_rebuilds: u64,
    pub graph_builds: u64,
    pub graph_updates: u64,
    pub graph_nodes: usize,
    pub graph_edges: usize,
    pub incremental_enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debounce_mode() {
        let mode = ScanTriggerMode::Debounced(50);
        assert_eq!(mode, ScanTriggerMode::Debounced(50));
    }
}
