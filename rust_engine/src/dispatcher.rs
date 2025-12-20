//! Dispatcher - coordinates scanning and trade execution with single balance pool

use crate::order_book::OrderBookCache;
use crate::scanner::Scanner;
use crate::slippage::SlippageCalculator;
use crate::balance_manager::BalanceManager;
use crate::types::{DispatcherStats, EngineConfig, Opportunity, OrderBookHealth, TradeResult};

use chrono::Utc;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info};

/// Dispatcher coordinates the trading cycle
pub struct Dispatcher {
    cache: Arc<OrderBookCache>,
    balance_manager: Arc<BalanceManager>,
    slippage_calc: SlippageCalculator,
    
    // Scanner storage for health stats (read-only access)
    last_scanner: RwLock<Option<Scanner>>,
    
    // Path cooldowns (prevent trading same path too frequently)
    path_cooldowns: RwLock<HashMap<String, chrono::DateTime<Utc>>>,
    
    // Trade counter for unique IDs
    trade_counter: AtomicU64,
    
    // Statistics
    opportunities_found: AtomicU64,
    last_cycle_duration_ms: RwLock<f64>,
    last_cycle_at: RwLock<String>,
}

impl Dispatcher {
    pub fn new(
        cache: Arc<OrderBookCache>,
        balance_manager: Arc<BalanceManager>,
    ) -> Self {
        let config = balance_manager.get_config();
        let slippage_calc = SlippageCalculator::new(
            Arc::clone(&cache),
            config.staleness_warn_ms,
            config.staleness_buffer_ms,
            config.staleness_reject_ms,
        );
        
        Self {
            cache,
            balance_manager,
            slippage_calc,
            last_scanner: RwLock::new(None),
            path_cooldowns: RwLock::new(HashMap::new()),
            trade_counter: AtomicU64::new(0),
            opportunities_found: AtomicU64::new(0),
            last_cycle_duration_ms: RwLock::new(0.0),
            last_cycle_at: RwLock::new(String::new()),
        }
    }

    /// Get order book health stats from last scan
    pub fn get_orderbook_health(&self) -> OrderBookHealth {
        let guard = self.last_scanner.read();
        match &*guard {
            Some(scanner) => scanner.get_health(),
            None => OrderBookHealth::default(),
        }
    }

    /// Run a complete trading cycle
    pub fn run_cycle(&self, base_currencies: &[String]) -> Vec<TradeResult> {
        let start = Instant::now();
        let mut results = Vec::new();
        
        // Get current config from balance manager
        let config = self.balance_manager.get_config();
        let max_trades = config.max_trades_per_cycle;
        let min_profit = config.min_profit_threshold;
        
        // 1. Scan for opportunities
        let scanner = Scanner::new(Arc::clone(&self.cache), config.clone());
        let opportunities = scanner.scan(base_currencies);
        
        // Store scanner for health stats (non-blocking, after scan completes)
        *self.last_scanner.write() = Some(scanner);
        
        self.opportunities_found.fetch_add(opportunities.len() as u64, Ordering::Relaxed);
        
        // 2. Filter profitable opportunities above threshold
        let profitable: Vec<&Opportunity> = opportunities
            .iter()
            .filter(|o| o.is_profitable && o.net_profit_pct >= min_profit * 100.0)
            .collect();
        
        if profitable.is_empty() {
            self.update_stats(start);
            return results;
        }
        
        // 3. Clean up expired path cooldowns
        self.cleanup_path_cooldowns();
        
        // 4. Check cooldown and clear if expired
        self.balance_manager.check_cooldown();
        
        // 5. Execute trades up to max_trades_per_cycle
        let mut trades_executed = 0;
        
        for opportunity in profitable.iter() {
            // Check if we've hit max trades limit
            if trades_executed >= max_trades {
                debug!("Reached max trades per cycle: {}", max_trades);
                break;
            }
            
            // Check if we can trade (balance + cooldown)
            if !self.balance_manager.can_trade() {
                debug!("Cannot trade: insufficient balance or in cooldown");
                break;
            }
            
            // Check path cooldown
            if self.is_path_locked(&opportunity.path) {
                continue;
            }
            
            // Get actual trade amount
            let trade_amount = self.balance_manager.reserve_for_trade();
            if trade_amount <= 0.0 {
                break;
            }
            
            // Calculate slippage
            let slippage = self.slippage_calc.calculate_path(&opportunity.path, trade_amount);
            
            if !slippage.can_execute {
                continue;
            }
            
            // Execute trade
            let result = self.execute_trade(opportunity, &slippage, trade_amount);
            
            // Lock the path
            self.lock_path(&opportunity.path);
            
            results.push(result);
            trades_executed += 1;
        }
        
        self.update_stats(start);
        results
    }

    /// Execute a paper trade
    fn execute_trade(
        &self,
        opportunity: &Opportunity,
        slippage: &crate::types::SlippageResult,
        trade_amount: f64,
    ) -> TradeResult {
        let balance_before = self.balance_manager.get_balance();
        let config = self.balance_manager.get_config();
        
        // Calculate latency penalty (per leg)
        let num_legs = opportunity.legs as f64;
        let latency_penalty_pct = config.latency_penalty_pct * 100.0 * num_legs; // Convert to percentage
        
        // Calculate actual profit after slippage AND latency penalty
        let expected_profit_pct = opportunity.net_profit_pct;
        let slippage_pct = slippage.total_slippage_pct;
        let total_penalty_pct = slippage_pct + latency_penalty_pct;
        let actual_profit_pct = expected_profit_pct - total_penalty_pct;
        
        // Calculate profit amount
        let profit_amount = trade_amount * (actual_profit_pct / 100.0);
        let is_win = profit_amount > 0.0;
        
        // Update balance
        self.balance_manager.complete_trade(profit_amount, is_win);
        
        let balance_after = self.balance_manager.get_balance();
        
        // Build slippage details string (include latency penalty)
        let mut slippage_details = slippage
            .legs
            .iter()
            .map(|l| format!("{}:{:.4}%", l.pair, l.slippage_pct))
            .collect::<Vec<_>>()
            .join(", ");
        
        if latency_penalty_pct > 0.0 {
            slippage_details = format!("{} + latency:{:.4}%", slippage_details, latency_penalty_pct);
        }
        
        // Get unique trade ID
        let trade_id = self.trade_counter.fetch_add(1, Ordering::Relaxed);
        
        TradeResult {
            trade_id,
            path: opportunity.path.clone(),
            trade_amount,
            expected_profit_pct,
            slippage_pct: total_penalty_pct, // Include latency in total slippage
            slippage_details,
            actual_profit_pct,
            profit_amount,
            balance_before,
            balance_after,
            status: if is_win { "WIN".to_string() } else { "LOSS".to_string() },
        }
    }

    /// Check if a path is currently locked
    fn is_path_locked(&self, path: &str) -> bool {
        let cooldowns = self.path_cooldowns.read();
        if let Some(until) = cooldowns.get(path) {
            return Utc::now() < *until;
        }
        false
    }

    /// Lock a path for cooldown period
    fn lock_path(&self, path: &str) {
        let config = self.balance_manager.get_config();
        let cooldown_ms = config.path_cooldown_ms;
        
        let until = Utc::now() + chrono::Duration::milliseconds(cooldown_ms as i64);
        let mut cooldowns = self.path_cooldowns.write();
        cooldowns.insert(path.to_string(), until);
    }

    /// Clean up expired path cooldowns
    fn cleanup_path_cooldowns(&self) {
        let now = Utc::now();
        let mut cooldowns = self.path_cooldowns.write();
        cooldowns.retain(|_, until| *until > now);
    }

    /// Get locked paths
    pub fn get_locked_paths(&self) -> Vec<String> {
        let now = Utc::now();
        let cooldowns = self.path_cooldowns.read();
        cooldowns
            .iter()
            .filter(|(_, until)| **until > now)
            .map(|(path, _)| path.clone())
            .collect()
    }

    /// Update cycle statistics
    fn update_stats(&self, start: Instant) {
        let duration_ms = start.elapsed().as_secs_f64() * 1000.0;
        *self.last_cycle_duration_ms.write() = duration_ms;
        *self.last_cycle_at.write() = Utc::now().to_rfc3339();
    }

    /// Get dispatcher statistics
    pub fn get_stats(&self) -> DispatcherStats {
        DispatcherStats {
            opportunities_found: self.opportunities_found.load(Ordering::Relaxed),
            last_cycle_duration_ms: *self.last_cycle_duration_ms.read(),
            last_cycle_at: self.last_cycle_at.read().clone(),
        }
    }
}
