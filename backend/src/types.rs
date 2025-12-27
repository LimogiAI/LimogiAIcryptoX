//! Type definitions for the trading engine
//! Pure Rust - No Python bindings
#![allow(dead_code)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ============================================================================
// HFT Configuration Constants
// ============================================================================

/// Maximum order book staleness in milliseconds for HFT
/// Order books older than this are considered stale and excluded from arbitrage paths
/// Lower = more aggressive (fewer valid paths, but more accurate pricing)
pub const MAX_ORDERBOOK_STALENESS_MS: i64 = 2000; // 2 seconds for HFT

/// Minimum order book depth (number of levels) required for trading
/// Books with fewer levels are considered too thin for reliable execution
pub const MIN_ORDERBOOK_DEPTH: usize = 3;

/// Maximum bid-ask spread percentage allowed
/// Pairs with wider spreads are excluded from arbitrage paths
pub const MAX_SPREAD_PCT: f64 = 5.0;

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

/// Default opportunity TTL in milliseconds for HFT
/// Opportunities older than this are considered stale and should not be executed
pub const OPPORTUNITY_TTL_MS: i64 = 500; // 500ms - very aggressive for HFT

impl Opportunity {
    /// Get the age of this opportunity in milliseconds
    pub fn age_ms(&self) -> i64 {
        let now = Utc::now();
        (now - self.detected_at).num_milliseconds()
    }

    /// Check if this opportunity has expired (too old to execute safely)
    /// For HFT, opportunities older than OPPORTUNITY_TTL_MS are considered stale
    pub fn is_expired(&self) -> bool {
        self.age_ms() > OPPORTUNITY_TTL_MS
    }

    /// Check if this opportunity is still fresh enough to execute
    /// Returns (is_fresh, age_ms) for logging
    pub fn freshness_check(&self) -> (bool, i64) {
        let age = self.age_ms();
        (!self.is_expired(), age)
    }

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
/// NOTE: All values MUST be provided - no defaults allowed
/// Configuration comes from:
/// - User input via dashboard (min_profit_threshold)
/// - Kraken API (fee_rate from fee_configuration table)
/// - User settings (min_profit_threshold from live_trading_config table)
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Minimum profit threshold (as decimal, e.g., 0.003 = 0.3%) - from user config
    pub min_profit_threshold: f64,
    /// Fee rate (taker fee as decimal, e.g., 0.0026 = 0.26%) - from Kraken API or manual
    pub fee_rate: f64,
    /// Source of fee data: "kraken_api", "manual", "pending"
    pub fee_source: String,
}

impl EngineConfig {
    /// Create a new EngineConfig with required values
    pub fn new(
        min_profit_threshold: Option<f64>,
        fee_rate: Option<f64>,
        fee_source: String,
    ) -> Result<Self, String> {
        // min_profit_threshold can be negative (user may want to test with losses)
        let min_profit = min_profit_threshold
            .ok_or("min_profit_threshold is required - user must configure")?;

        let fee = fee_rate
            .ok_or("fee_rate is required - must fetch from Kraken or enter manually")?;
        if fee <= 0.0 {
            return Err("fee_rate must be greater than 0 - fees not configured".to_string());
        }

        if fee_source == "pending" {
            return Err("Fees not configured - must fetch from Kraken or enter manually".to_string());
        }

        Ok(Self {
            min_profit_threshold: min_profit,
            fee_rate: fee,
            fee_source,
        })
    }

    /// Create an unconfigured/invalid config (for initialization only)
    /// Engine MUST NOT start with this config
    pub fn unconfigured() -> Self {
        Self {
            min_profit_threshold: 0.0,
            fee_rate: 0.0,
            fee_source: "pending".to_string(),
        }
    }

    /// Check if this config is valid for starting the engine
    /// Note: min_profit_threshold can be negative (user may want to execute losing trades for testing)
    pub fn is_valid(&self) -> bool {
        self.fee_rate > 0.0
            && self.fee_source != "pending"
    }

    /// Get validation error message
    pub fn validate(&self) -> Result<(), String> {
        // Note: min_profit_threshold can be any value including negative
        // (user may want to test execution with intentional losses)
        if self.fee_rate <= 0.0 {
            return Err("fee_rate not configured".to_string());
        }
        if self.fee_source == "pending" {
            return Err("fees pending - must fetch from Kraken or enter manually".to_string());
        }
        Ok(())
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
