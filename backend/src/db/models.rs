//! Database models matching PostgreSQL schema
#![allow(dead_code)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgRow;
use sqlx::{FromRow, Row};

/// Live trading configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveTradingConfig {
    pub id: i32,
    pub is_enabled: bool,
    pub trade_amount: Option<f64>,
    pub min_profit_threshold: Option<f64>,
    pub max_daily_loss: Option<f64>,
    pub max_total_loss: Option<f64>,
    pub base_currency: Option<String>,
    pub custom_currencies: Option<serde_json::Value>,
    // Pair Selection Filters (REQUIRED)
    pub max_pairs: Option<i32>,
    pub min_volume_24h_usd: Option<f64>,
    pub max_cost_min: Option<f64>,
    // Timestamps
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub enabled_at: Option<DateTime<Utc>>,
    pub disabled_at: Option<DateTime<Utc>>,
}

impl Default for LiveTradingConfig {
    fn default() -> Self {
        Self {
            id: 1,
            is_enabled: false,
            // NOTE: All values are None by default - user MUST configure from dashboard
            // The API will reject enabling trading if these haven't been explicitly set
            trade_amount: None,
            min_profit_threshold: None,
            max_daily_loss: None,
            max_total_loss: None,
            base_currency: None,
            custom_currencies: Some(serde_json::json!([])),
            // Pair Selection Filters - user MUST configure
            max_pairs: None,
            min_volume_24h_usd: None,
            max_cost_min: None,
            created_at: None,
            updated_at: None,
            enabled_at: None,
            disabled_at: None,
        }
    }
}

impl<'r> FromRow<'r, PgRow> for LiveTradingConfig {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            is_enabled: row.try_get("is_enabled")?,
            trade_amount: row.try_get("trade_amount").ok(),
            min_profit_threshold: row.try_get("min_profit_threshold").ok(),
            max_daily_loss: row.try_get("max_daily_loss").ok(),
            max_total_loss: row.try_get("max_total_loss").ok(),
            base_currency: row.try_get("base_currency").ok(),
            custom_currencies: row.try_get("custom_currencies").ok(),
            max_pairs: row.try_get("max_pairs").ok(),
            min_volume_24h_usd: row.try_get("min_volume_24h_usd").ok(),
            max_cost_min: row.try_get("max_cost_min").ok(),
            created_at: row.try_get("created_at").ok(),
            updated_at: row.try_get("updated_at").ok(),
            enabled_at: row.try_get("enabled_at").ok(),
            disabled_at: row.try_get("disabled_at").ok(),
        })
    }
}

/// Config update request (all fields optional)
#[derive(Debug, Clone, Deserialize)]
pub struct ConfigUpdate {
    pub trade_amount: Option<f64>,
    pub min_profit_threshold: Option<f64>,
    pub max_daily_loss: Option<f64>,
    pub max_total_loss: Option<f64>,
    #[serde(alias = "start_currency")]
    pub base_currency: Option<String>,
    // Pair Selection Filters
    pub max_pairs: Option<i32>,
    pub min_volume_24h_usd: Option<f64>,
    pub max_cost_min: Option<f64>,
}

/// Live trading state (circuit breaker, stats)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveTradingState {
    pub id: i32,
    pub daily_loss: f64,
    pub daily_profit: f64,
    pub daily_trades: i32,
    pub daily_wins: i32,
    pub total_loss: f64,
    pub total_profit: f64,
    pub total_trades: i32,
    pub total_wins: i32,
    pub total_trade_amount: f64,
    pub partial_trades: i32,
    pub partial_estimated_loss: f64,
    pub partial_estimated_profit: f64,
    pub partial_trade_amount: f64,
    pub is_circuit_broken: bool,
    pub circuit_broken_at: Option<DateTime<Utc>>,
    pub circuit_broken_reason: Option<String>,
    pub last_trade_at: Option<DateTime<Utc>>,
    pub last_daily_reset: Option<DateTime<Utc>>,
    pub is_executing: bool,
    pub current_trade_id: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl Default for LiveTradingState {
    fn default() -> Self {
        Self {
            id: 1,
            daily_loss: 0.0,
            daily_profit: 0.0,
            daily_trades: 0,
            daily_wins: 0,
            total_loss: 0.0,
            total_profit: 0.0,
            total_trades: 0,
            total_wins: 0,
            total_trade_amount: 0.0,
            partial_trades: 0,
            partial_estimated_loss: 0.0,
            partial_estimated_profit: 0.0,
            partial_trade_amount: 0.0,
            is_circuit_broken: false,
            circuit_broken_at: None,
            circuit_broken_reason: None,
            last_trade_at: None,
            last_daily_reset: None,
            is_executing: false,
            current_trade_id: None,
            created_at: None,
            updated_at: None,
        }
    }
}

impl<'r> FromRow<'r, PgRow> for LiveTradingState {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            daily_loss: row.try_get("daily_loss")?,
            daily_profit: row.try_get("daily_profit")?,
            daily_trades: row.try_get("daily_trades")?,
            daily_wins: row.try_get("daily_wins")?,
            total_loss: row.try_get("total_loss")?,
            total_profit: row.try_get("total_profit")?,
            total_trades: row.try_get("total_trades")?,
            total_wins: row.try_get("total_wins")?,
            total_trade_amount: row.try_get("total_trade_amount").unwrap_or(0.0),
            partial_trades: row.try_get("partial_trades").unwrap_or(0),
            partial_estimated_loss: row.try_get("partial_estimated_loss").unwrap_or(0.0),
            partial_estimated_profit: row.try_get("partial_estimated_profit").unwrap_or(0.0),
            partial_trade_amount: row.try_get("partial_trade_amount").unwrap_or(0.0),
            is_circuit_broken: row.try_get("is_circuit_broken")?,
            circuit_broken_at: row.try_get("circuit_broken_at").ok(),
            circuit_broken_reason: row.try_get("circuit_broken_reason").ok(),
            last_trade_at: row.try_get("last_trade_at").ok(),
            last_daily_reset: row.try_get("last_daily_reset").ok(),
            is_executing: row.try_get("is_executing")?,
            current_trade_id: row.try_get("current_trade_id").ok(),
            created_at: row.try_get("created_at").ok(),
            updated_at: row.try_get("updated_at").ok(),
        })
    }
}

/// Live trade record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveTrade {
    pub id: i32,
    pub trade_id: String,
    pub path: String,
    pub legs: i32,
    pub amount_in: f64,
    pub amount_out: Option<f64>,
    pub profit_loss: Option<f64>,
    pub profit_loss_pct: Option<f64>,
    pub status: String,
    pub current_leg: Option<i32>,
    pub error_message: Option<String>,
    pub held_currency: Option<String>,
    pub held_amount: Option<f64>,
    pub held_value_usd: Option<f64>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub resolved_amount_usd: Option<f64>,
    pub resolution_trade_id: Option<String>,
    pub order_ids: Option<serde_json::Value>,
    pub leg_fills: Option<serde_json::Value>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub total_execution_ms: Option<f64>,
    pub opportunity_profit_pct: Option<f64>,
    pub created_at: Option<DateTime<Utc>>,
}

impl<'r> FromRow<'r, PgRow> for LiveTrade {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            trade_id: row.try_get("trade_id")?,
            path: row.try_get("path")?,
            legs: row.try_get("legs")?,
            amount_in: row.try_get("amount_in")?,
            amount_out: row.try_get("amount_out").ok(),
            profit_loss: row.try_get("profit_loss").ok(),
            profit_loss_pct: row.try_get("profit_loss_pct").ok(),
            status: row.try_get("status")?,
            current_leg: row.try_get("current_leg").ok(),
            error_message: row.try_get("error_message").ok(),
            held_currency: row.try_get("held_currency").ok(),
            held_amount: row.try_get("held_amount").ok(),
            held_value_usd: row.try_get("held_value_usd").ok(),
            resolved_at: row.try_get("resolved_at").ok(),
            resolved_amount_usd: row.try_get("resolved_amount_usd").ok(),
            resolution_trade_id: row.try_get("resolution_trade_id").ok(),
            order_ids: row.try_get("order_ids").ok(),
            leg_fills: row.try_get("leg_fills").ok(),
            started_at: row.try_get("started_at").ok(),
            completed_at: row.try_get("completed_at").ok(),
            total_execution_ms: row.try_get("total_execution_ms").ok(),
            opportunity_profit_pct: row.try_get("opportunity_profit_pct").ok(),
            created_at: row.try_get("created_at").ok(),
        })
    }
}

/// New trade to insert
#[derive(Debug, Clone)]
pub struct NewLiveTrade {
    pub trade_id: String,
    pub path: String,
    pub legs: i32,
    pub amount_in: f64,
    pub amount_out: Option<f64>,
    pub profit_loss: Option<f64>,
    pub profit_loss_pct: Option<f64>,
    pub status: String,
    pub current_leg: Option<i32>,
    pub error_message: Option<String>,
    pub held_currency: Option<String>,
    pub held_amount: Option<f64>,
    pub held_value_usd: Option<f64>,
    pub order_ids: Option<serde_json::Value>,
    pub leg_fills: Option<serde_json::Value>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub total_execution_ms: Option<f64>,
    pub opportunity_profit_pct: Option<f64>,
}

/// Live opportunity record (saved to database)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveOpportunity {
    pub id: i32,
    pub found_at: Option<DateTime<Utc>>,
    pub path: String,
    pub legs: i32,
    pub expected_profit_pct: f64,
    pub expected_profit_usd: Option<f64>,
    pub trade_amount: Option<f64>,
    pub status: String,
    pub status_reason: Option<String>,
    pub trade_id: Option<String>,
    pub pairs_scanned: Option<i32>,
    pub paths_found: Option<i32>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl<'r> FromRow<'r, PgRow> for LiveOpportunity {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            found_at: row.try_get("found_at").ok(),
            path: row.try_get("path")?,
            legs: row.try_get("legs")?,
            expected_profit_pct: row.try_get("expected_profit_pct")?,
            expected_profit_usd: row.try_get("expected_profit_usd").ok(),
            trade_amount: row.try_get("trade_amount").ok(),
            status: row.try_get("status")?,
            status_reason: row.try_get("status_reason").ok(),
            trade_id: row.try_get("trade_id").ok(),
            pairs_scanned: row.try_get("pairs_scanned").ok(),
            paths_found: row.try_get("paths_found").ok(),
            created_at: row.try_get("created_at").ok(),
            updated_at: row.try_get("updated_at").ok(),
        })
    }
}

/// New opportunity to insert
#[derive(Debug, Clone)]
pub struct NewLiveOpportunity {
    pub path: String,
    pub legs: i32,
    pub expected_profit_pct: f64,
    pub expected_profit_usd: Option<f64>,
    pub trade_amount: Option<f64>,
    pub status: String,
    pub status_reason: Option<String>,
    pub pairs_scanned: Option<i32>,
    pub paths_found: Option<i32>,
}

/// Fee configuration from Kraken API or manual entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeConfiguration {
    pub id: i32,
    pub maker_fee: f64,
    pub taker_fee: f64,
    pub fee_source: String,  // 'kraken_api', 'manual', 'pending'
    pub volume_tier: Option<String>,
    pub thirty_day_volume: Option<f64>,
    pub last_fetched_at: Option<DateTime<Utc>>,
    pub last_updated_at: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
}

impl Default for FeeConfiguration {
    fn default() -> Self {
        Self {
            id: 1,
            maker_fee: 0.0,
            taker_fee: 0.0,
            fee_source: "pending".to_string(),
            volume_tier: None,
            thirty_day_volume: None,
            last_fetched_at: None,
            last_updated_at: None,
            created_at: None,
        }
    }
}

impl<'r> FromRow<'r, PgRow> for FeeConfiguration {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        // PostgreSQL DECIMAL comes as rust_decimal::Decimal or can be read as f64 directly
        // Using try_get with f64 directly since our precision (10,6) fits well in f64
        Ok(Self {
            id: row.try_get("id")?,
            maker_fee: row.try_get::<f64, _>("maker_fee").unwrap_or(0.0),
            taker_fee: row.try_get::<f64, _>("taker_fee").unwrap_or(0.0),
            fee_source: row.try_get("fee_source")?,
            volume_tier: row.try_get("volume_tier").ok(),
            thirty_day_volume: row.try_get::<f64, _>("thirty_day_volume").ok(),
            last_fetched_at: row.try_get("last_fetched_at").ok(),
            last_updated_at: row.try_get("last_updated_at").ok(),
            created_at: row.try_get("created_at").ok(),
        })
    }
}

/// Fee configuration update request
#[derive(Debug, Clone, Deserialize)]
pub struct FeeConfigurationUpdate {
    pub maker_fee: Option<f64>,
    pub taker_fee: Option<f64>,
    pub fee_source: Option<String>,
    pub volume_tier: Option<String>,
    pub thirty_day_volume: Option<f64>,
}