//! Type definitions for the trading engine
//! Pure Rust - No Python bindings

use chrono::{DateTime, Utc};
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

/// Detail for a single leg in an arbitrage path
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegDetail {
    pub pair: String,
    pub action: String,
    pub rate: f64,
}

/// Arbitrage opportunity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Opportunity {
    pub id: String,
    pub path: String,
    pub legs: usize,
    pub gross_profit_pct: f64,
    pub fees_pct: f64,
    pub net_profit_pct: f64,
    pub is_profitable: bool,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub detected_at: DateTime<Utc>,
    pub fee_rate: f64,
    pub fee_source: String,
    pub legs_detail: Vec<LegDetail>,
}

impl Opportunity {
    pub fn get_price_snapshot_json(&self) -> String {
        let snapshot = serde_json::json!({
            "fee_rate": self.fee_rate,
            "fee_source": self.fee_source,
            "legs": self.legs_detail.iter().map(|leg| {
                serde_json::json!({
                    "pair": leg.pair,
                    "action": leg.action,
                    "rate": leg.rate
                })
            }).collect::<Vec<_>>()
        });
        serde_json::to_string(&snapshot).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Slippage calculation result for a single leg
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlippageLeg {
    pub pair: String,
    pub side: String,
    pub best_price: f64,
    pub actual_price: f64,
    pub slippage_pct: f64,
    pub can_fill: bool,
    pub depth_used: usize,
    pub reason: Option<String>,
}

/// Slippage calculation result for entire path
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlippageResult {
    pub total_slippage_pct: f64,
    pub can_execute: bool,
    pub reason: Option<String>,
    pub legs: Vec<SlippageLeg>,
}

/// Engine statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineStats {
    pub is_running: bool,
    pub pairs_monitored: usize,
    pub currencies_tracked: usize,
    pub orderbooks_cached: usize,
    pub avg_orderbook_staleness_ms: f64,
    pub opportunities_found: u64,
    pub opportunities_per_second: f64,
    pub uptime_seconds: u64,
    pub scan_cycle_ms: f64,
    pub last_scan_at: String,
}

/// Engine configuration
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub min_profit_threshold: f64,
    pub fee_rate: f64,
    pub fee_source: String,
    pub scan_interval_ms: u64,
    pub orderbook_depth: usize,
    pub max_pairs: usize,
    pub scanner_enabled: bool,
    pub staleness_warn_ms: i64,
    pub staleness_buffer_ms: i64,
    pub staleness_reject_ms: i64,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            min_profit_threshold: 0.0005,
            fee_rate: 0.0026,
            fee_source: "default".to_string(),
            scan_interval_ms: 10000,
            orderbook_depth: 25,
            max_pairs: 300,
            scanner_enabled: true,
            staleness_warn_ms: 100,
            staleness_buffer_ms: 250,
            staleness_reject_ms: 1000,
        }
    }
}

/// Engine settings that can be changed at runtime
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineSettings {
    pub scan_interval_ms: u64,
    pub max_pairs: usize,
    pub orderbook_depth: usize,
    pub scanner_enabled: bool,
}

/// Dispatcher statistics
#[derive(Debug, Default)]
pub struct DispatcherStats {
    pub opportunities_found: u64,
    pub last_cycle_duration_ms: f64,
    pub last_cycle_at: String,
}

/// Order Book Health Statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrderBookHealth {
    pub total_pairs: u32,
    pub valid_pairs: u32,
    pub skipped_no_orderbook: u32,
    pub skipped_thin_depth: u32,
    pub skipped_stale: u32,
    pub skipped_bad_spread: u32,
    pub skipped_no_price: u32,
    pub avg_freshness_ms: f64,
    pub avg_spread_pct: f64,
    pub avg_depth: f64,
    pub rejected_opportunities: u32,
    pub last_update: String,
}

/// Price info for API responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceInfo {
    pub pair: String,
    pub bid: f64,
    pub ask: f64,
}
