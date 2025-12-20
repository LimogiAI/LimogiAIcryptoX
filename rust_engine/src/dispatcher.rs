//! Dispatcher - coordinates scanning for opportunities
//! 
//! This module scans for arbitrage opportunities and tracks health stats.
//! All trade execution is handled by the Python live trading system.

use crate::order_book::OrderBookCache;
use crate::scanner::Scanner;
use crate::config_manager::ConfigManager;
use crate::types::{DispatcherStats, Opportunity, OrderBookHealth};

use chrono::Utc;
use parking_lot::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Dispatcher coordinates opportunity scanning
pub struct Dispatcher {
    cache: Arc<OrderBookCache>,
    config_manager: Arc<ConfigManager>,
    
    // Scanner storage for health stats
    last_scanner: RwLock<Option<Scanner>>,
    
    // Statistics
    opportunities_found: AtomicU64,
    last_cycle_duration_ms: RwLock<f64>,
    last_cycle_at: RwLock<String>,
}

impl Dispatcher {
    pub fn new(
        cache: Arc<OrderBookCache>,
        config_manager: Arc<ConfigManager>,
    ) -> Self {
        Self {
            cache,
            config_manager,
            last_scanner: RwLock::new(None),
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

    /// Run a scan cycle - returns opportunities found
    pub fn run_cycle(&self, base_currencies: &[String]) -> Vec<Opportunity> {
        let start = Instant::now();
        
        // Get current config
        let config = self.config_manager.get_config();
        let min_profit = config.min_profit_threshold;
        
        // Scan for opportunities
        let scanner = Scanner::new(Arc::clone(&self.cache), config);
        let opportunities = scanner.scan(base_currencies);
        
        // Store scanner for health stats
        *self.last_scanner.write() = Some(scanner);
        
        self.opportunities_found.fetch_add(opportunities.len() as u64, Ordering::Relaxed);
        
        // Filter profitable opportunities above threshold
        let profitable: Vec<Opportunity> = opportunities
            .into_iter()
            .filter(|o| o.is_profitable && o.net_profit_pct >= min_profit * 100.0)
            .collect();
        
        self.update_stats(start);
        
        profitable
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
