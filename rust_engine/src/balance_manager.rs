//! Balance manager - single balance pool with kill switch protection
//! Loss calculations are based on PEAK balance (high water mark), not initial balance

use crate::types::{EngineConfig, TradingState};
use chrono::{DateTime, Datelike, Utc};
use parking_lot::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{info, warn, error};

/// Internal trading state
struct InternalState {
    balance: f64,
    initial_balance: f64,
    peak_balance: f64,           // High water mark - highest balance ever reached
    total_trades: u64,
    total_wins: u64,
    total_profit: f64,
    cooldown_until: Option<DateTime<Utc>>,
    
    // Kill switch state
    is_killed: bool,
    kill_reason: Option<String>,
    consecutive_losses: u32,
    daily_profit: f64,
    daily_peak_balance: f64,     // Peak balance at start of day for daily loss calc
    daily_reset_date: Option<u32>,  // Day of year for daily reset
}

/// Manages single balance pool for trading with kill switch protection
/// All loss calculations are from PEAK balance (high water mark)
pub struct BalanceManager {
    state: RwLock<InternalState>,
    config: RwLock<EngineConfig>,
}

impl BalanceManager {
    pub fn new(config: EngineConfig) -> Self {
        let initial_balance = config.initial_balance;
        
        info!("Initialized balance manager with ${:.2}", initial_balance);
        info!("Kill switch: {} (max_loss: {:.0}% from peak, max_consecutive: {}, daily_max: {:.0}% from peak)",
            if config.kill_switch_enabled { "ENABLED" } else { "DISABLED" },
            config.max_loss_pct * 100.0,
            config.max_consecutive_losses,
            config.max_daily_loss_pct * 100.0
        );
        info!("⚠️  All loss calculations are from PEAK balance (high water mark)");
        info!("⚠️  NO auto-reset - manual intervention required when kill switch triggers");
        
        Self {
            state: RwLock::new(InternalState {
                balance: initial_balance,
                initial_balance,
                peak_balance: initial_balance,
                total_trades: 0,
                total_wins: 0,
                total_profit: 0.0,
                cooldown_until: None,
                is_killed: false,
                kill_reason: None,
                consecutive_losses: 0,
                daily_profit: 0.0,
                daily_peak_balance: initial_balance,
                daily_reset_date: Some(Utc::now().ordinal()),
            }),
            config: RwLock::new(config),
        }
    }

    /// Update configuration from UI
    pub fn update_config(
        &self,
        trade_amount: Option<f64>,
        min_profit_threshold: Option<f64>,
        cooldown_ms: Option<u64>,
        max_trades_per_cycle: Option<usize>,
        latency_penalty_pct: Option<f64>,
        fee_rate: Option<f64>,
    ) {
        let mut config = self.config.write();
        
        if let Some(amount) = trade_amount {
            config.trade_amount = amount;
            info!("Updated trade amount to ${:.2}", amount);
        }
        if let Some(threshold) = min_profit_threshold {
            config.min_profit_threshold = threshold;
            info!("Updated min profit threshold to {:.4}%", threshold * 100.0);
        }
        if let Some(cooldown) = cooldown_ms {
            config.cooldown_ms = cooldown;
            info!("Updated cooldown to {}ms", cooldown);
        }
        if let Some(max_trades) = max_trades_per_cycle {
            config.max_trades_per_cycle = max_trades;
            info!("Updated max trades per cycle to {}", max_trades);
        }
        if let Some(penalty) = latency_penalty_pct {
            config.latency_penalty_pct = penalty;
            info!("Updated latency penalty to {:.2}% per leg", penalty * 100.0);
        }
        if let Some(fee) = fee_rate {
            config.fee_rate = fee;
            info!("Updated fee rate to {:.2}%", fee * 100.0);
        }
    }

    /// Update kill switch settings
    pub fn update_kill_switch(
        &self,
        enabled: Option<bool>,
        max_loss_pct: Option<f64>,
        max_consecutive_losses: Option<u32>,
        max_daily_loss_pct: Option<f64>,
    ) {
        let mut config = self.config.write();
        
        if let Some(e) = enabled {
            config.kill_switch_enabled = e;
            info!("Kill switch {}", if e { "ENABLED" } else { "DISABLED" });
        }
        if let Some(pct) = max_loss_pct {
            config.max_loss_pct = pct;
            info!("Updated max loss to {:.0}% from peak", pct * 100.0);
        }
        if let Some(n) = max_consecutive_losses {
            config.max_consecutive_losses = n;
            info!("Updated max consecutive losses to {}", n);
        }
        if let Some(pct) = max_daily_loss_pct {
            config.max_daily_loss_pct = pct;
            info!("Updated max daily loss to {:.0}% from peak", pct * 100.0);
        }
    }

    /// Update engine settings (scan interval, max pairs, depth, scanner on/off)
    /// Returns true if WebSocket reconnection is needed (pairs or depth changed)
    pub fn update_engine_settings(
        &self,
        scan_interval_ms: Option<u64>,
        max_pairs: Option<usize>,
        orderbook_depth: Option<usize>,
        scanner_enabled: Option<bool>,
    ) -> bool {
        let mut config = self.config.write();
        let mut needs_reconnect = false;
        
        if let Some(interval) = scan_interval_ms {
            config.scan_interval_ms = interval;
            info!("Updated scan interval to {}ms", interval);
        }
        
        if let Some(pairs) = max_pairs {
            if pairs != config.max_pairs {
                config.max_pairs = pairs;
                needs_reconnect = true;
                info!("Updated max pairs to {} (reconnection required)", pairs);
            }
        }
        
        if let Some(depth) = orderbook_depth {
            if depth != config.orderbook_depth {
                config.orderbook_depth = depth;
                needs_reconnect = true;
                info!("Updated orderbook depth to {} (reconnection required)", depth);
            }
        }
        
        if let Some(enabled) = scanner_enabled {
            config.scanner_enabled = enabled;
            info!("Scanner {}", if enabled { "ENABLED" } else { "DISABLED" });
        }
        
        needs_reconnect
    }

    /// Get current engine settings
    pub fn get_engine_settings(&self) -> (u64, usize, usize, bool) {
        let config = self.config.read();
        (
            config.scan_interval_ms,
            config.max_pairs,
            config.orderbook_depth,
            config.scanner_enabled,
        )
    }

    /// Check if scanner is enabled
    pub fn is_scanner_enabled(&self) -> bool {
        self.config.read().scanner_enabled
    }

    /// Get current configuration
    pub fn get_config(&self) -> EngineConfig {
        self.config.read().clone()
    }

    /// Get current trade amount setting
    pub fn get_trade_amount(&self) -> f64 {
        self.config.read().trade_amount
    }

    /// Get min profit threshold
    pub fn get_min_profit_threshold(&self) -> f64 {
        self.config.read().min_profit_threshold
    }

    /// Get cooldown in ms
    pub fn get_cooldown_ms(&self) -> u64 {
        self.config.read().cooldown_ms
    }

    /// Get max trades per cycle
    pub fn get_max_trades_per_cycle(&self) -> usize {
        self.config.read().max_trades_per_cycle
    }

    /// Check if killed
    pub fn is_killed(&self) -> bool {
        self.state.read().is_killed
    }

    /// Get kill reason
    pub fn get_kill_reason(&self) -> Option<String> {
        self.state.read().kill_reason.clone()
    }

    /// Manually trigger kill switch
    pub fn trigger_kill(&self, reason: &str) {
        let mut state = self.state.write();
        state.is_killed = true;
        state.kill_reason = Some(reason.to_string());
        error!("🛑 KILL SWITCH TRIGGERED: {}", reason);
    }

    /// Reset kill switch (allows trading again) - does NOT reset daily counters
    pub fn reset_kill_switch(&self) {
        let mut state = self.state.write();
        state.is_killed = false;
        state.kill_reason = None;
        state.consecutive_losses = 0;
        info!("Kill switch reset - trading enabled");
    }

    /// Check and reset daily profit counter if new day (but NOT the kill switch)
    fn check_daily_reset(&self) {
        let mut state = self.state.write();
        let today = Utc::now().ordinal();
        
        if let Some(last_day) = state.daily_reset_date {
            if last_day != today {
                info!("New day detected - resetting daily profit counter (kill switch NOT auto-reset)");
                state.daily_profit = 0.0;
                state.daily_peak_balance = state.peak_balance;  // Reset daily peak to current peak
                state.daily_reset_date = Some(today);
                // NOTE: Kill switch is NOT auto-reset - requires manual intervention
            }
        } else {
            state.daily_reset_date = Some(today);
            state.daily_peak_balance = state.peak_balance;
        }
    }

    /// Check if we can trade (has balance, not in cooldown, not killed)
    pub fn can_trade(&self) -> bool {
        // Check daily reset first
        self.check_daily_reset();
        
        let state = self.state.read();
        let config = self.config.read();
        
        // Check kill switch
        if state.is_killed {
            return false;
        }
        
        // Check cooldown
        if let Some(until) = state.cooldown_until {
            if Utc::now() < until {
                return false;
            }
        }
        
        // Check balance
        state.balance >= config.trade_amount
    }

    /// Get actual trade amount (may be less than requested if balance is low)
    pub fn get_actual_trade_amount(&self) -> f64 {
        let state = self.state.read();
        let config = self.config.read();
        
        if state.balance >= config.trade_amount {
            config.trade_amount
        } else if state.balance >= 1.0 {
            // Use available balance if at least $1
            state.balance
        } else {
            0.0
        }
    }

    /// Reserve balance for a trade (returns actual amount reserved, 0 if cannot trade)
    pub fn reserve_for_trade(&self) -> f64 {
        // Check daily reset
        self.check_daily_reset();
        
        let mut state = self.state.write();
        let config = self.config.read();
        
        // Check kill switch
        if state.is_killed {
            return 0.0;
        }
        
        // Check cooldown
        if let Some(until) = state.cooldown_until {
            if Utc::now() < until {
                return 0.0;
            }
            // Clear expired cooldown
            state.cooldown_until = None;
        }
        
        // Determine trade amount
        let trade_amount = if state.balance >= config.trade_amount {
            config.trade_amount
        } else if state.balance >= 1.0 {
            state.balance
        } else {
            return 0.0;
        };
        
        trade_amount
    }

    /// Complete a trade and update state - includes kill switch checks
    pub fn complete_trade(&self, profit_amount: f64, is_win: bool) {
        let mut state = self.state.write();
        let config = self.config.read();
        
        // Update basic stats
        state.balance += profit_amount;
        state.total_trades += 1;
        state.total_profit += profit_amount;
        state.daily_profit += profit_amount;
        
        // Update peak balance (high water mark)
        if state.balance > state.peak_balance {
            state.peak_balance = state.balance;
            info!("📈 New peak balance: ${:.2}", state.peak_balance);
        }
        
        // Update daily peak if applicable
        if state.balance > state.daily_peak_balance {
            state.daily_peak_balance = state.balance;
        }
        
        if is_win {
            state.total_wins += 1;
            state.consecutive_losses = 0;  // Reset consecutive losses on win
        } else {
            state.consecutive_losses += 1;
        }
        
        // Set cooldown
        state.cooldown_until = Some(Utc::now() + chrono::Duration::milliseconds(config.cooldown_ms as i64));
        
        // ========== KILL SWITCH CHECKS (from PEAK balance) ==========
        if config.kill_switch_enabled && !state.is_killed {
            
            // Check 1: Max total loss percentage FROM PEAK
            let loss_from_peak = state.peak_balance - state.balance;
            let loss_from_peak_pct = if state.peak_balance > 0.0 {
                loss_from_peak / state.peak_balance
            } else {
                0.0
            };
            
            if loss_from_peak_pct >= config.max_loss_pct {
                state.is_killed = true;
                state.kill_reason = Some(format!(
                    "Max loss exceeded: {:.2}% from peak ${:.2} (limit: {:.0}%)",
                    loss_from_peak_pct * 100.0,
                    state.peak_balance,
                    config.max_loss_pct * 100.0
                ));
                error!("🛑 KILL SWITCH: {:.2}% loss from peak ${:.2}!", 
                    loss_from_peak_pct * 100.0, state.peak_balance);
            }
            
            // Check 2: Max consecutive losses
            if state.consecutive_losses >= config.max_consecutive_losses {
                state.is_killed = true;
                state.kill_reason = Some(format!(
                    "Max consecutive losses: {} (limit: {})",
                    state.consecutive_losses,
                    config.max_consecutive_losses
                ));
                error!("🛑 KILL SWITCH: {} consecutive losses!", state.consecutive_losses);
            }
            
            // Check 3: Max daily loss percentage FROM DAILY PEAK
            let daily_loss_from_peak = state.daily_peak_balance - state.balance;
            let daily_loss_from_peak_pct = if state.daily_peak_balance > 0.0 && daily_loss_from_peak > 0.0 {
                daily_loss_from_peak / state.daily_peak_balance
            } else {
                0.0
            };
            
            if daily_loss_from_peak_pct >= config.max_daily_loss_pct {
                state.is_killed = true;
                state.kill_reason = Some(format!(
                    "Max daily loss exceeded: {:.2}% from today's peak ${:.2} (limit: {:.0}%)",
                    daily_loss_from_peak_pct * 100.0,
                    state.daily_peak_balance,
                    config.max_daily_loss_pct * 100.0
                ));
                error!("🛑 KILL SWITCH: {:.2}% daily loss from peak ${:.2}!", 
                    daily_loss_from_peak_pct * 100.0, state.daily_peak_balance);
            }
        }
    }

    /// Clear cooldown (if expired)
    pub fn check_cooldown(&self) {
        let mut state = self.state.write();
        if let Some(until) = state.cooldown_until {
            if Utc::now() >= until {
                state.cooldown_until = None;
            }
        }
    }

    /// Get current balance
    pub fn get_balance(&self) -> f64 {
        self.state.read().balance
    }

    /// Get peak balance
    pub fn get_peak_balance(&self) -> f64 {
        self.state.read().peak_balance
    }

    /// Get initial balance
    pub fn get_initial_balance(&self) -> f64 {
        self.state.read().initial_balance
    }

    /// Get total profit
    pub fn get_total_profit(&self) -> f64 {
        self.state.read().total_profit
    }

    /// Get total trades count
    pub fn get_total_trades(&self) -> u64 {
        self.state.read().total_trades
    }

    /// Get win rate
    pub fn get_win_rate(&self) -> f64 {
        let state = self.state.read();
        if state.total_trades == 0 {
            return 0.0;
        }
        state.total_wins as f64 / state.total_trades as f64 * 100.0
    }

    /// Get trading state for API
    pub fn get_state(&self) -> TradingState {
        // Check daily reset
        self.check_daily_reset();
        
        let state = self.state.read();
        let now = Utc::now();
        
        let is_in_cooldown = state.cooldown_until
            .map(|until| now < until)
            .unwrap_or(false);
        
        // Calculate loss from peak
        let loss_from_peak = state.peak_balance - state.balance;
        let loss_from_peak_pct = if state.peak_balance > 0.0 && loss_from_peak > 0.0 {
            loss_from_peak / state.peak_balance * 100.0
        } else {
            0.0
        };
        
        TradingState {
            balance: state.balance,
            initial_balance: state.initial_balance,
            peak_balance: state.peak_balance,
            total_trades: state.total_trades,
            total_wins: state.total_wins,
            total_profit: state.total_profit,
            win_rate: if state.total_trades > 0 {
                state.total_wins as f64 / state.total_trades as f64 * 100.0
            } else {
                0.0
            },
            is_in_cooldown,
            cooldown_until: state.cooldown_until.map(|t| t.to_rfc3339()),
            // Kill switch fields
            is_killed: state.is_killed,
            kill_reason: state.kill_reason.clone(),
            consecutive_losses: state.consecutive_losses,
            daily_profit: state.daily_profit,
            loss_from_peak_pct,
        }
    }

    /// Reset to initial state (also resets kill switch and peak)
    pub fn reset(&self, initial_balance: f64) {
        let mut state = self.state.write();
        
        state.balance = initial_balance;
        state.initial_balance = initial_balance;
        state.peak_balance = initial_balance;
        state.total_trades = 0;
        state.total_wins = 0;
        state.total_profit = 0.0;
        state.cooldown_until = None;
        state.is_killed = false;
        state.kill_reason = None;
        state.consecutive_losses = 0;
        state.daily_profit = 0.0;
        state.daily_peak_balance = initial_balance;
        state.daily_reset_date = Some(Utc::now().ordinal());
        
        info!("Full reset: balance=${:.2}, peak=${:.2}, kill switch cleared", initial_balance, initial_balance);
    }
}
