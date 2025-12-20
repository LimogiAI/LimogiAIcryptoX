//! Type definitions for the trading engine

use chrono::{DateTime, Utc};
use pyo3::prelude::*;
use serde::{Deserialize, Serialize};

/// Order book level (price + quantity)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookLevel {
    pub price: f64,
    pub qty: f64,
}

/// Complete order book
#[derive(Debug, Clone)]
pub struct OrderBook {
    pub pair: String,
    pub bids: Vec<OrderBookLevel>,
    pub asks: Vec<OrderBookLevel>,
    pub sequence: u64,
    pub last_update: DateTime<Utc>,
}

impl OrderBook {
    pub fn new(pair: String) -> Self {
        Self {
            pair,
            bids: Vec::new(),
            asks: Vec::new(),
            sequence: 0,
            last_update: Utc::now(),
        }
    }

    pub fn best_bid(&self) -> Option<f64> {
        self.bids.first().map(|l| l.price)
    }

    pub fn best_ask(&self) -> Option<f64> {
        self.asks.first().map(|l| l.price)
    }

    pub fn spread(&self) -> Option<f64> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(ask - bid),
            _ => None,
        }
    }

    pub fn spread_pct(&self) -> Option<f64> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) if bid > 0.0 => Some((ask - bid) / bid * 100.0),
            _ => None,
        }
    }

    pub fn staleness_ms(&self) -> i64 {
        (Utc::now() - self.last_update).num_milliseconds()
    }
}

/// Price edge for graph (simplified order book for fast lookup)
#[derive(Debug, Clone)]
pub struct PriceEdge {
    pub pair: String,
    pub base: String,
    pub quote: String,
    pub bid: f64,
    pub ask: f64,
    pub volume_24h: f64,
    pub last_update: DateTime<Utc>,
}

/// Arbitrage opportunity
#[pyclass]
#[derive(Debug, Clone)]
pub struct Opportunity {
    #[pyo3(get)]
    pub id: String,
    #[pyo3(get)]
    pub path: String,
    #[pyo3(get)]
    pub legs: usize,
    #[pyo3(get)]
    pub gross_profit_pct: f64,
    #[pyo3(get)]
    pub fees_pct: f64,
    #[pyo3(get)]
    pub net_profit_pct: f64,
    #[pyo3(get)]
    pub is_profitable: bool,
    pub detected_at: DateTime<Utc>,
}

#[pymethods]
impl Opportunity {
    fn __repr__(&self) -> String {
        format!(
            "Opportunity(path='{}', net_profit={:.4}%, profitable={})",
            self.path, self.net_profit_pct, self.is_profitable
        )
    }
}

/// Slippage calculation result for a single leg
#[pyclass]
#[derive(Debug, Clone)]
pub struct SlippageLeg {
    #[pyo3(get)]
    pub pair: String,
    #[pyo3(get)]
    pub side: String,
    #[pyo3(get)]
    pub best_price: f64,
    #[pyo3(get)]
    pub actual_price: f64,
    #[pyo3(get)]
    pub slippage_pct: f64,
    #[pyo3(get)]
    pub can_fill: bool,
    #[pyo3(get)]
    pub depth_used: usize,
    #[pyo3(get)]
    pub reason: Option<String>,
}

/// Slippage calculation result for entire path
#[pyclass]
#[derive(Debug, Clone)]
pub struct SlippageResult {
    #[pyo3(get)]
    pub total_slippage_pct: f64,
    #[pyo3(get)]
    pub can_execute: bool,
    #[pyo3(get)]
    pub reason: Option<String>,
    #[pyo3(get)]
    pub legs: Vec<SlippageLeg>,
}

#[pymethods]
impl SlippageResult {
    fn __repr__(&self) -> String {
        format!(
            "SlippageResult(slippage={:.4}%, can_execute={})",
            self.total_slippage_pct, self.can_execute
        )
    }
}

/// Engine statistics
#[pyclass]
#[derive(Debug, Clone)]
pub struct EngineStats {
    #[pyo3(get)]
    pub is_running: bool,
    #[pyo3(get)]
    pub pairs_monitored: usize,
    #[pyo3(get)]
    pub currencies_tracked: usize,
    #[pyo3(get)]
    pub orderbooks_cached: usize,
    #[pyo3(get)]
    pub avg_orderbook_staleness_ms: f64,
    #[pyo3(get)]
    pub opportunities_found: u64,
    #[pyo3(get)]
    pub opportunities_per_second: f64,
    #[pyo3(get)]
    pub uptime_seconds: u64,
    #[pyo3(get)]
    pub scan_cycle_ms: f64,
    #[pyo3(get)]
    pub last_scan_at: String,
}

#[pymethods]
impl EngineStats {
    fn __repr__(&self) -> String {
        format!(
            "EngineStats(running={}, pairs={}, opportunities={})",
            self.is_running, self.pairs_monitored, self.opportunities_found
        )
    }
}

/// Engine configuration
#[derive(Debug, Clone)]
pub struct EngineConfig {
    // Scanning settings
    pub min_profit_threshold: f64,   // Min profit to consider opportunity
    pub fee_rate: f64,               // Taker fee rate (e.g., 0.0026 = 0.26%)
    
    // Runtime-changeable engine settings
    pub scan_interval_ms: u64,       // Scan interval
    pub orderbook_depth: usize,      // Order book depth
    pub max_pairs: usize,            // Max pairs to monitor
    pub scanner_enabled: bool,       // Scanner ON/OFF toggle
    
    // Fixed settings
    pub staleness_warn_ms: i64,
    pub staleness_buffer_ms: i64,
    pub staleness_reject_ms: i64,
}

/// Engine settings that can be changed at runtime (returned to Python)
#[pyclass]
#[derive(Debug, Clone)]
pub struct EngineSettings {
    #[pyo3(get)]
    pub scan_interval_ms: u64,
    #[pyo3(get)]
    pub max_pairs: usize,
    #[pyo3(get)]
    pub orderbook_depth: usize,
    #[pyo3(get)]
    pub scanner_enabled: bool,
}

#[pymethods]
impl EngineSettings {
    fn __repr__(&self) -> String {
        format!(
            "EngineSettings(scan_interval={}ms, max_pairs={}, depth={}, scanner={})",
            self.scan_interval_ms, self.max_pairs, self.orderbook_depth, self.scanner_enabled
        )
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            // Scanning settings
            min_profit_threshold: 0.0005,  // 0.05%
            fee_rate: 0.0026,              // 0.26% taker fee default
            
            // Runtime-changeable engine settings
            scan_interval_ms: 10000,       // 10 seconds default
            orderbook_depth: 25,           // 25 levels default
            max_pairs: 300,                // 300 pairs default
            scanner_enabled: true,         // Scanner ON by default
            
            // Fixed settings
            staleness_warn_ms: 100,        // Warn if > 100ms old
            staleness_buffer_ms: 250,      // Add 1% buffer if > 250ms old
            staleness_reject_ms: 1000,     // Reject if > 1 second old
        }
    }
}

/// Dispatcher statistics
#[derive(Debug, Default)]
pub struct DispatcherStats {
    pub opportunities_found: u64,
    pub last_cycle_duration_ms: f64,
    pub last_cycle_at: String,
}

/// Order Book Health Statistics
#[pyclass]
#[derive(Debug, Clone, Default)]
pub struct OrderBookHealth {
    #[pyo3(get)]
    pub total_pairs: u32,
    #[pyo3(get)]
    pub valid_pairs: u32,
    #[pyo3(get)]
    pub skipped_no_orderbook: u32,
    #[pyo3(get)]
    pub skipped_thin_depth: u32,
    #[pyo3(get)]
    pub skipped_stale: u32,
    #[pyo3(get)]
    pub skipped_bad_spread: u32,
    #[pyo3(get)]
    pub skipped_no_price: u32,
    #[pyo3(get)]
    pub avg_freshness_ms: f64,
    #[pyo3(get)]
    pub avg_spread_pct: f64,
    #[pyo3(get)]
    pub avg_depth: f64,
    #[pyo3(get)]
    pub rejected_opportunities: u32,
    #[pyo3(get)]
    pub last_update: String,
}

#[pymethods]
impl OrderBookHealth {
    fn __repr__(&self) -> String {
        format!(
            "OrderBookHealth(valid={}/{}, no_ob={}, thin={}, stale={}, bad_spread={}, avg_fresh={:.1}ms, avg_spread={:.2}%)",
            self.valid_pairs, self.total_pairs,
            self.skipped_no_orderbook, self.skipped_thin_depth,
            self.skipped_stale, self.skipped_bad_spread,
            self.avg_freshness_ms, self.avg_spread_pct
        )
    }
}
