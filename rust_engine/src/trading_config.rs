//! Trading Configuration and Guard Logic
//!
//! Provides configuration and guard checks for automated trading:
//! - Trading enabled/disabled state
//! - Circuit breaker (daily/total loss limits)
//! - Profit threshold filtering
//! - Base currency filtering
//! - Execution mode (sequential/parallel)
//!
//! This moves guard logic from Python to Rust for faster execution.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

/// Trading configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingConfig {
    /// Is live trading enabled
    pub enabled: bool,
    /// Trade amount in base currency
    pub trade_amount: f64,
    /// Minimum profit threshold (as percentage, e.g., 0.3 = 0.3%)
    pub min_profit_threshold: f64,
    /// Maximum daily loss allowed
    pub max_daily_loss: f64,
    /// Maximum total loss allowed
    pub max_total_loss: f64,
    /// Base currency filter (e.g., "USD", "ALL", or comma-separated list)
    pub base_currency: String,
    /// Execution mode: "sequential" or "parallel"
    pub execution_mode: String,
}

impl Default for TradingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            trade_amount: 10.0,
            min_profit_threshold: 0.3, // 0.3%
            max_daily_loss: 30.0,
            max_total_loss: 30.0,
            base_currency: "USD".to_string(),
            execution_mode: "sequential".to_string(),
        }
    }
}

/// Circuit breaker state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerState {
    /// Is circuit broken (trading halted)
    pub is_broken: bool,
    /// Reason for break
    pub broken_reason: Option<String>,
    /// Daily P&L
    pub daily_pnl: f64,
    /// Total P&L
    pub total_pnl: f64,
    /// Number of trades today
    pub daily_trades: u32,
    /// Total number of trades
    pub total_trades: u32,
    /// Is currently executing a trade
    pub is_executing: bool,
}

impl Default for CircuitBreakerState {
    fn default() -> Self {
        Self {
            is_broken: false,
            broken_reason: None,
            daily_pnl: 0.0,
            total_pnl: 0.0,
            daily_trades: 0,
            total_trades: 0,
            is_executing: false,
        }
    }
}

/// Guard check result
#[derive(Debug, Clone)]
pub struct GuardCheckResult {
    pub can_trade: bool,
    pub reason: Option<String>,
}

/// Trade execution result for callbacks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeResult {
    pub trade_id: String,
    pub path: String,
    pub status: String, // "COMPLETED", "PARTIAL", "FAILED"
    pub legs_completed: usize,
    pub total_legs: usize,
    pub amount_in: f64,
    pub amount_out: f64,
    pub profit_pct: f64,
    pub profit_amount: f64,
    pub execution_time_ms: u64,
    pub error: Option<String>,
    pub leg_details: Vec<LegResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegResult {
    pub leg_index: usize,
    pub pair: String,
    pub side: String,
    pub amount_in: f64,
    pub amount_out: f64,
    pub price: f64,
    pub fee: f64,
    pub status: String,
}

/// Trading guard that manages config and circuit breaker
pub struct TradingGuard {
    config: RwLock<TradingConfig>,
    state: RwLock<CircuitBreakerState>,

    // Atomic flags for fast checks
    enabled: AtomicBool,
    is_broken: AtomicBool,
    is_executing: AtomicBool,

    // Statistics
    trades_executed: AtomicU64,
    trades_successful: AtomicU64,
    opportunities_seen: AtomicU64,
    opportunities_executed: AtomicU64,

    // Last reset time for daily stats
    last_daily_reset: RwLock<Instant>,
}

impl TradingGuard {
    pub fn new() -> Self {
        Self {
            config: RwLock::new(TradingConfig::default()),
            state: RwLock::new(CircuitBreakerState::default()),
            enabled: AtomicBool::new(false),
            is_broken: AtomicBool::new(false),
            is_executing: AtomicBool::new(false),
            trades_executed: AtomicU64::new(0),
            trades_successful: AtomicU64::new(0),
            opportunities_seen: AtomicU64::new(0),
            opportunities_executed: AtomicU64::new(0),
            last_daily_reset: RwLock::new(Instant::now()),
        }
    }

    /// Update trading configuration
    pub fn update_config(&self, config: TradingConfig) {
        self.enabled.store(config.enabled, Ordering::SeqCst);
        *self.config.write() = config;
        info!("Trading config updated");
    }

    /// Get current configuration
    pub fn get_config(&self) -> TradingConfig {
        self.config.read().clone()
    }

    /// Enable trading
    pub fn enable(&self) {
        self.enabled.store(true, Ordering::SeqCst);
        self.config.write().enabled = true;
        info!("Trading ENABLED");
    }

    /// Disable trading
    pub fn disable(&self, reason: &str) {
        self.enabled.store(false, Ordering::SeqCst);
        self.config.write().enabled = false;
        info!("Trading DISABLED: {}", reason);
    }

    /// Check if trading is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Trip the circuit breaker
    pub fn trip_circuit_breaker(&self, reason: &str) {
        self.is_broken.store(true, Ordering::SeqCst);
        let mut state = self.state.write();
        state.is_broken = true;
        state.broken_reason = Some(reason.to_string());
        warn!("Circuit breaker TRIPPED: {}", reason);
    }

    /// Reset circuit breaker
    pub fn reset_circuit_breaker(&self) {
        self.is_broken.store(false, Ordering::SeqCst);
        let mut state = self.state.write();
        state.is_broken = false;
        state.broken_reason = None;
        info!("Circuit breaker RESET");
    }

    /// Check if circuit is broken
    pub fn is_circuit_broken(&self) -> bool {
        self.is_broken.load(Ordering::Relaxed)
    }

    /// Get circuit breaker state
    pub fn get_state(&self) -> CircuitBreakerState {
        self.state.read().clone()
    }

    /// Set executing flag (returns false if already executing in sequential mode)
    pub fn try_start_execution(&self) -> bool {
        let config = self.config.read();
        if config.execution_mode == "sequential" {
            // In sequential mode, only one execution at a time
            if self.is_executing.compare_exchange(
                false, true, Ordering::SeqCst, Ordering::SeqCst
            ).is_err() {
                return false;
            }
        }
        self.state.write().is_executing = true;
        true
    }

    /// Clear executing flag
    pub fn finish_execution(&self) {
        self.is_executing.store(false, Ordering::SeqCst);
        self.state.write().is_executing = false;
    }

    /// Check if an opportunity passes all guards
    pub fn check_opportunity(&self, path: &str, profit_pct: f64) -> GuardCheckResult {
        // Fast atomic checks first
        if !self.enabled.load(Ordering::Relaxed) {
            return GuardCheckResult {
                can_trade: false,
                reason: Some("Trading disabled".to_string()),
            };
        }

        if self.is_broken.load(Ordering::Relaxed) {
            return GuardCheckResult {
                can_trade: false,
                reason: Some("Circuit breaker tripped".to_string()),
            };
        }

        let config = self.config.read();
        let state = self.state.read();

        // Check if already executing (in sequential mode)
        if config.execution_mode == "sequential" && state.is_executing {
            return GuardCheckResult {
                can_trade: false,
                reason: Some("Trade already executing".to_string()),
            };
        }

        // Check profit threshold
        if profit_pct < config.min_profit_threshold {
            return GuardCheckResult {
                can_trade: false,
                reason: Some(format!(
                    "Below threshold: {:.3}% < {:.3}%",
                    profit_pct, config.min_profit_threshold
                )),
            };
        }

        // Check base currency filter
        if !self.check_base_currency(path, &config.base_currency) {
            return GuardCheckResult {
                can_trade: false,
                reason: Some("Base currency filter".to_string()),
            };
        }

        // Check daily loss limit
        if state.daily_pnl < 0.0 && state.daily_pnl.abs() >= config.max_daily_loss {
            return GuardCheckResult {
                can_trade: false,
                reason: Some(format!(
                    "Daily loss limit: ${:.2} >= ${:.2}",
                    state.daily_pnl.abs(), config.max_daily_loss
                )),
            };
        }

        // Check total loss limit
        if state.total_pnl < 0.0 && state.total_pnl.abs() >= config.max_total_loss {
            return GuardCheckResult {
                can_trade: false,
                reason: Some(format!(
                    "Total loss limit: ${:.2} >= ${:.2}",
                    state.total_pnl.abs(), config.max_total_loss
                )),
            };
        }

        GuardCheckResult {
            can_trade: true,
            reason: None,
        }
    }

    /// Check base currency filter
    fn check_base_currency(&self, path: &str, filter: &str) -> bool {
        if filter == "ALL" {
            return true;
        }

        // Get first currency from path
        let start_currency = if path.contains(" → ") {
            path.split(" → ").next()
        } else if path.contains("→") {
            path.split("→").next()
        } else {
            path.split_whitespace().next()
        };

        let start = match start_currency {
            Some(c) => c.trim(),
            None => return false,
        };

        // Check if it matches the filter (single currency or comma-separated list)
        if filter.contains(',') {
            filter.split(',').any(|c| c.trim() == start)
        } else {
            filter.trim() == start
        }
    }

    /// Record a trade result
    pub fn record_trade(&self, result: &TradeResult) {
        let mut state = self.state.write();
        let config = self.config.read();

        state.daily_trades += 1;
        state.total_trades += 1;
        state.daily_pnl += result.profit_amount;
        state.total_pnl += result.profit_amount;

        self.trades_executed.fetch_add(1, Ordering::Relaxed);
        if result.status == "COMPLETED" {
            self.trades_successful.fetch_add(1, Ordering::Relaxed);
        }
        self.opportunities_executed.fetch_add(1, Ordering::Relaxed);

        // Check if we need to trip circuit breaker
        if state.daily_pnl < 0.0 && state.daily_pnl.abs() >= config.max_daily_loss {
            drop(state);
            drop(config);
            self.trip_circuit_breaker(&format!("Daily loss limit reached: ${:.2}", result.profit_amount.abs()));
        } else if state.total_pnl < 0.0 && state.total_pnl.abs() >= config.max_total_loss {
            drop(state);
            drop(config);
            self.trip_circuit_breaker(&format!("Total loss limit reached: ${:.2}", result.profit_amount.abs()));
        }

        info!(
            "Trade recorded: {} | Status: {} | Profit: ${:.4} ({:.3}%)",
            result.path, result.status, result.profit_amount, result.profit_pct
        );
    }

    /// Record an opportunity (for statistics)
    pub fn record_opportunity_seen(&self) {
        self.opportunities_seen.fetch_add(1, Ordering::Relaxed);
    }

    /// Get trading statistics
    pub fn get_stats(&self) -> (u64, u64, u64, u64, f64, f64) {
        let state = self.state.read();
        (
            self.trades_executed.load(Ordering::Relaxed),
            self.trades_successful.load(Ordering::Relaxed),
            self.opportunities_seen.load(Ordering::Relaxed),
            self.opportunities_executed.load(Ordering::Relaxed),
            state.daily_pnl,
            state.total_pnl,
        )
    }

    /// Reset daily statistics
    pub fn reset_daily(&self) {
        let mut state = self.state.write();
        state.daily_pnl = 0.0;
        state.daily_trades = 0;
        *self.last_daily_reset.write() = Instant::now();

        // Reset circuit breaker if it was tripped due to daily loss
        if state.is_broken && state.broken_reason.as_ref().map_or(false, |r| r.contains("Daily")) {
            state.is_broken = false;
            state.broken_reason = None;
            self.is_broken.store(false, Ordering::SeqCst);
        }

        info!("Daily statistics reset");
    }
}

impl Default for TradingGuard {
    fn default() -> Self {
        Self::new()
    }
}
