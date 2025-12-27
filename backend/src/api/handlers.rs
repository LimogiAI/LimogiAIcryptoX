//! API request handlers
//!
//! All endpoint handlers for the trading API.

use crate::db::{ConfigUpdate, NewLiveTrade};
use crate::restrictions::{AddRemoveRequest, UpdateRequest};
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Datelike, FixedOffset, TimeZone, Utc, Weekday};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, error};

/// Check if a date is in US Eastern Daylight Time (EDT)
/// DST starts 2nd Sunday of March, ends 1st Sunday of November
fn is_dst(dt: &DateTime<Utc>) -> bool {
    let year = dt.year();
    let month = dt.month();
    let day = dt.day();

    // March: DST starts on 2nd Sunday
    if month == 3 {
        let first_day = Utc.with_ymd_and_hms(year, 3, 1, 0, 0, 0).unwrap();
        let first_sunday = match first_day.weekday() {
            Weekday::Sun => 1,
            Weekday::Mon => 7,
            Weekday::Tue => 6,
            Weekday::Wed => 5,
            Weekday::Thu => 4,
            Weekday::Fri => 3,
            Weekday::Sat => 2,
        };
        let second_sunday = first_sunday + 7;
        return day >= second_sunday;
    }

    // November: DST ends on 1st Sunday
    if month == 11 {
        let first_day = Utc.with_ymd_and_hms(year, 11, 1, 0, 0, 0).unwrap();
        let first_sunday = match first_day.weekday() {
            Weekday::Sun => 1,
            Weekday::Mon => 7,
            Weekday::Tue => 6,
            Weekday::Wed => 5,
            Weekday::Thu => 4,
            Weekday::Fri => 3,
            Weekday::Sat => 2,
        };
        return day < first_sunday;
    }

    // April-October: DST active
    // December-February: Standard time
    month >= 4 && month <= 10
}

/// Format a UTC datetime in Eastern Time (ET/EDT)
fn format_datetime_et(dt: DateTime<Utc>) -> String {
    let (offset_hours, suffix) = if is_dst(&dt) {
        (-4, "EDT")
    } else {
        (-5, "EST")
    };

    let offset = FixedOffset::east_opt(offset_hours * 3600).unwrap();
    let et_time = dt.with_timezone(&offset);
    format!("{} {}", et_time.format("%Y-%m-%d %H:%M:%S"), suffix)
}

// ==========================================
// Response Helpers
// ==========================================

pub fn error_response(error: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({
            "success": false,
            "error": error
        }))
    ).into_response()
}

pub fn bad_request(error: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({
            "success": false,
            "error": error
        }))
    ).into_response()
}

// ==========================================
// Request Types
// ==========================================

#[derive(Debug, Deserialize)]
pub struct EnableRequest {
    pub confirm: bool,
    pub confirm_text: String,
}

#[derive(Debug, Deserialize)]
pub struct DisableRequest {
    #[serde(default = "default_disable_reason")]
    pub reason: String,
}

fn default_disable_reason() -> String {
    "Manual disable".to_string()
}

#[derive(Debug, Deserialize)]
pub struct ExecuteTradeRequest {
    pub path: String,
    pub amount: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct TradesQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    pub status: Option<String>,
    #[serde(default = "default_hours")]
    pub hours: i32,
}

fn default_limit() -> i64 { 20 }
fn default_hours() -> i32 { 24 }

#[derive(Debug, Deserialize)]
pub struct ResetAllQuery {
    #[serde(default)]
    pub confirm: bool,
    #[serde(default)]
    pub confirm_text: String,
}

#[derive(Debug, Deserialize)]
pub struct FeeConfigUpdate {
    pub maker_fee: Option<f64>,
    pub taker_fee: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct LimitQuery {
    pub limit: Option<usize>,
}

// ==========================================
// Health & Status Handlers
// ==========================================

pub async fn health_check() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "rust_backend",
        "version": "1.0.0"
    }))
}

pub async fn get_status(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let stats = state.engine.get_stats().await;
    
    Json(serde_json::json!({
        "is_running": stats.is_running,
        "engine": "rust_v2",
        "pairs_monitored": stats.pairs_monitored,
        "currencies_tracked": stats.currencies_tracked,
        "orderbooks_cached": stats.orderbooks_cached,
        "avg_staleness_ms": stats.avg_orderbook_staleness_ms,
        "opportunities_found": stats.opportunities_found,
        "opportunities_per_second": stats.opportunities_per_second,
        "uptime_seconds": stats.uptime_seconds,
        "scan_cycle_ms": stats.scan_cycle_ms,
        "last_scan_at": stats.last_scan_at,
    }))
}

pub async fn restart_engine(
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("Engine restart requested");

    match state.engine.restart_websocket().await {
        Ok(()) => {
            Json(serde_json::json!({
                "success": true,
                "message": "Engine restarted successfully"
            })).into_response()
        }
        Err(e) => {
            error!("Failed to restart engine: {}", e);
            Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to restart: {}", e)
            })).into_response()
        }
    }
}

// ==========================================
// Config Handlers
// ==========================================

pub async fn get_config(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    match state.db.get_config().await {
        Ok(config) => {
            // Calculate session duration if trading is enabled
            let session_info = if config.is_enabled {
                if let Some(enabled_at) = config.enabled_at {
                    let duration_secs = Utc::now().signed_duration_since(enabled_at).num_seconds();
                    let hours = duration_secs / 3600;
                    let minutes = (duration_secs % 3600) / 60;
                    let seconds = duration_secs % 60;

                    Some(serde_json::json!({
                        "started_at": format_datetime_et(enabled_at),
                        "duration_seconds": duration_secs,
                        "duration_formatted": format!("{}h {}m {}s", hours, minutes, seconds)
                    }))
                } else {
                    None
                }
            } else {
                // If stopped, show last session info
                if let (Some(enabled_at), Some(disabled_at)) = (config.enabled_at, config.disabled_at) {
                    let duration_secs = disabled_at.signed_duration_since(enabled_at).num_seconds();
                    if duration_secs > 0 {
                        let hours = duration_secs / 3600;
                        let minutes = (duration_secs % 3600) / 60;
                        let seconds = duration_secs % 60;

                        Some(serde_json::json!({
                            "started_at": format_datetime_et(enabled_at),
                            "stopped_at": format_datetime_et(disabled_at),
                            "duration_seconds": duration_secs,
                            "duration_formatted": format!("{}h {}m {}s", hours, minutes, seconds)
                        }))
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            Json(serde_json::json!({
                "id": config.id,
                "is_enabled": config.is_enabled,
                "trade_amount": config.trade_amount,
                "min_profit_threshold": config.min_profit_threshold,
                "max_daily_loss": config.max_daily_loss,
                "max_total_loss": config.max_total_loss,
                "start_currency": config.start_currency,
                "custom_currencies": config.custom_currencies,
                "max_pairs": config.max_pairs,
                "min_volume_24h_usd": config.min_volume_24h_usd,
                "max_cost_min": config.max_cost_min,
                "session": session_info
            })).into_response()
        },
        Err(e) => error_response(&e.to_string()),
    }
}

/// Configuration status response
#[derive(Debug, Clone, Serialize)]
pub struct ConfigurationStatus {
    pub is_configured: bool,
    pub can_start_engine: bool,
    pub missing_fields: Vec<String>,
    pub warnings: Vec<String>,
    pub config_summary: ConfigSummary,
    pub fee_config: FeeConfigStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigSummary {
    pub trade_amount: Option<f64>,
    pub min_profit_threshold: Option<f64>,
    pub start_currency: Option<String>,
    pub max_daily_loss: Option<f64>,
    pub max_total_loss: Option<f64>,
    // Pair Selection Filters
    pub max_pairs: Option<i32>,
    pub min_volume_24h_usd: Option<f64>,
    pub max_cost_min: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeeConfigStatus {
    pub is_configured: bool,
    pub maker_fee: Option<f64>,
    pub taker_fee: Option<f64>,
    pub fee_source: String,
    pub volume_tier: Option<String>,
    pub thirty_day_volume: Option<f64>,
    pub last_fetched_at: Option<String>,
    pub last_updated_at: Option<String>,
}

/// GET /api/live/configuration-status
/// Returns the current configuration status - what's configured and what's missing
/// User MUST configure all required fields before starting the engine
pub async fn get_configuration_status(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let config = match state.db.get_config().await {
        Ok(c) => c,
        Err(e) => return error_response(&e.to_string()),
    };

    let mut missing_fields = Vec::new();
    let mut warnings = Vec::new();

    // Check required fields - NO DEFAULTS ALLOWED
    // User must explicitly set these values

    // 1. Start currency - REQUIRED
    let start_currency_val = config.start_currency.clone().unwrap_or_default();
    let start_currency = if start_currency_val.is_empty() {
        missing_fields.push("start_currency: Select USD, EUR, or both for triangular arbitrage".to_string());
        None
    } else {
        Some(start_currency_val)
    };

    // 2. Trade amount - REQUIRED (must be set by user, not default)
    let trade_amount_val = config.trade_amount.unwrap_or(0.0);
    let trade_amount = if trade_amount_val <= 0.0 {
        missing_fields.push("trade_amount: Set your trade amount ($20-$100 recommended)".to_string());
        None
    } else {
        // Validate trade amount is reasonable
        if trade_amount_val < 5.0 {
            warnings.push(format!("trade_amount: ${:.2} is very low, may not meet Kraken minimums", trade_amount_val));
        } else if trade_amount_val > 1000.0 {
            warnings.push(format!("trade_amount: ${:.2} is high for HFT testing", trade_amount_val));
        }
        Some(trade_amount_val)
    };

    // 3. Min profit threshold - REQUIRED (can be any value including negative)
    let min_profit_threshold = if let Some(min_profit_val) = config.min_profit_threshold {
        // Accept any value without warnings - user knows what they're doing
        Some(min_profit_val)
    } else {
        missing_fields.push("min_profit_threshold: Set minimum profit % (can be negative for testing)".to_string());
        None
    };

    // 4. Loss limits - REQUIRED
    let max_daily_loss = if config.max_daily_loss.unwrap_or(0.0) <= 0.0 {
        missing_fields.push("max_daily_loss: Set daily loss limit (circuit breaker)".to_string());
        None
    } else {
        config.max_daily_loss
    };

    let max_total_loss = if config.max_total_loss.unwrap_or(0.0) <= 0.0 {
        missing_fields.push("max_total_loss: Set total loss limit (circuit breaker)".to_string());
        None
    } else {
        config.max_total_loss
    };

    // 6. Pair Selection Filters - REQUIRED for pair filtering
    let max_pairs = if config.max_pairs.is_none() || config.max_pairs.unwrap_or(0) <= 0 {
        missing_fields.push("max_pairs: Set maximum trading pairs to monitor (30-100 recommended)".to_string());
        None
    } else {
        config.max_pairs
    };

    let min_volume_24h_usd = if config.min_volume_24h_usd.is_none() || config.min_volume_24h_usd.unwrap_or(0.0) <= 0.0 {
        missing_fields.push("min_volume_24h_usd: Set minimum 24h USD volume filter ($50,000+ recommended)".to_string());
        None
    } else {
        config.min_volume_24h_usd
    };

    let max_cost_min = if config.max_cost_min.is_none() || config.max_cost_min.unwrap_or(0.0) <= 0.0 {
        missing_fields.push("max_cost_min: Set maximum order minimum cost filter ($20+ recommended)".to_string());
        None
    } else {
        config.max_cost_min
    };

    // Get fee configuration
    let fee_config = match state.db.get_fee_configuration().await {
        Ok(fc) => fc,
        Err(_) => crate::db::FeeConfiguration::default(),
    };

    let fees_configured = fee_config.fee_source != "pending";
    if !fees_configured {
        missing_fields.push("fees: Maker/Taker fees not configured. Click 'Fetch Fees' or enter manually.".to_string());
    }

    let fee_config_status = FeeConfigStatus {
        is_configured: fees_configured,
        maker_fee: if fees_configured { Some(fee_config.maker_fee) } else { None },
        taker_fee: if fees_configured { Some(fee_config.taker_fee) } else { None },
        fee_source: fee_config.fee_source.clone(),
        volume_tier: fee_config.volume_tier.clone(),
        thirty_day_volume: fee_config.thirty_day_volume,
        last_fetched_at: fee_config.last_fetched_at.map(|t| t.to_rfc3339()),
        last_updated_at: fee_config.last_updated_at.map(|t| t.to_rfc3339()),
    };

    // Determine if configuration is complete (including fees)
    let is_configured = missing_fields.is_empty();
    let can_start_engine = is_configured; // Can only start if fully configured

    let status = ConfigurationStatus {
        is_configured,
        can_start_engine,
        missing_fields,
        warnings,
        config_summary: ConfigSummary {
            trade_amount,
            min_profit_threshold,
            start_currency,
            max_daily_loss,
            max_total_loss,
            // Pair Selection Filters
            max_pairs,
            min_volume_24h_usd,
            max_cost_min,
        },
        fee_config: fee_config_status,
    };

    Json(serde_json::json!({
        "success": true,
        "status": status
    })).into_response()
}

pub async fn update_config(
    State(state): State<Arc<AppState>>,
    Json(updates): Json<ConfigUpdate>,
) -> Response {
    match state.db.update_config(updates).await {
        Ok(config) => {
            state.engine.sync_config(&config).await;
            Json(serde_json::json!({
                "success": true,
                "message": "Configuration updated",
                "config": config
            })).into_response()
        }
        Err(e) => error_response(&e.to_string()),
    }
}

// ==========================================
// Enable/Disable Handlers
// ==========================================

pub async fn enable_trading(
    State(state): State<Arc<AppState>>,
    Json(req): Json<EnableRequest>,
) -> Response {
    if !req.confirm || req.confirm_text != "I understand the risks" {
        return bad_request("Must confirm with 'I understand the risks'");
    }

    // CRITICAL: Validate configuration BEFORE enabling trading
    // User MUST configure all required fields from the dashboard
    let config = match state.db.get_config().await {
        Ok(c) => c,
        Err(e) => return error_response(&format!("Failed to get config: {}", e)),
    };

    let mut missing_fields = Vec::new();

    // Check all required fields - NO DEFAULTS ALLOWED
    if config.start_currency.clone().unwrap_or_default().is_empty() {
        missing_fields.push("Start Currency (USD/EUR)");
    }
    if config.trade_amount.unwrap_or(0.0) <= 0.0 {
        missing_fields.push("Trade Amount");
    }
    // min_profit_threshold can be any value including 0 or negative (for testing losses)
    if config.min_profit_threshold.is_none() {
        missing_fields.push("Min Profit Threshold");
    }
    if config.max_daily_loss.unwrap_or(0.0) <= 0.0 {
        missing_fields.push("Daily Loss Limit");
    }
    if config.max_total_loss.unwrap_or(0.0) <= 0.0 {
        missing_fields.push("Total Loss Limit");
    }
    // Pair Selection Filters - REQUIRED
    if config.max_pairs.unwrap_or(0) <= 0 {
        missing_fields.push("Max Pairs (pair filter)");
    }
    if config.min_volume_24h_usd.unwrap_or(0.0) <= 0.0 {
        missing_fields.push("Min 24h Volume USD (pair filter)");
    }
    if config.max_cost_min.unwrap_or(0.0) <= 0.0 {
        missing_fields.push("Max Cost Min (pair filter)");
    }

    if !missing_fields.is_empty() {
        return bad_request(&format!(
            "Configuration incomplete. Please configure from the dashboard: {}",
            missing_fields.join(", ")
        ));
    }

    // Configuration is valid - start the HFT engine (WebSocket + Scanner + Executor)
    match state.db.enable_trading().await {
        Ok(config) => {
            // Start the full HFT engine (WebSocket connection, HFT loop, execution engine)
            if let Err(e) = state.engine.start().await {
                return error_response(&format!("Failed to start HFT engine: {}", e));
            }

            info!("HFT Engine STARTED: trade_amount=${}, min_profit={:.2}%, start_currency={}",
                config.trade_amount.unwrap_or(0.0),
                config.min_profit_threshold.unwrap_or(0.0) * 100.0,
                config.start_currency.clone().unwrap_or_default()
            );
            Json(serde_json::json!({
                "success": true,
                "message": "HFT Engine started (WebSocket + Scanner + Executor)",
                "config": config
            })).into_response()
        }
        Err(e) => error_response(&e.to_string()),
    }
}

pub async fn disable_trading(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DisableRequest>,
) -> Response {
    match state.db.disable_trading(&req.reason).await {
        Ok(config) => {
            // Stop the full HFT engine (WebSocket, HFT loop, execution engine)
            state.engine.stop().await;
            info!("HFT Engine STOPPED: {}", req.reason);
            Json(serde_json::json!({
                "success": true,
                "message": "HFT Engine stopped",
                "config": config
            })).into_response()
        }
        Err(e) => error_response(&e.to_string()),
    }
}

// ==========================================
// Status & State Handlers
// ==========================================

pub async fn get_live_status(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let config = state.db.get_config().await.unwrap_or_default();
    let db_state = state.db.get_state().await.unwrap_or_default();
    let engine_stats = state.engine.get_stats().await;
    
    Json(serde_json::json!({
        "config": config,
        "state": db_state,
        "engine": {
            "is_running": engine_stats.is_running,
            "pairs_monitored": engine_stats.pairs_monitored,
            "auto_execution_enabled": state.engine.is_auto_execution_enabled(),
        }
    }))
}

pub async fn get_state(
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.db.get_state().await {
        Ok(s) => Json(serde_json::json!({
            "success": true,
            "data": s
        })).into_response(),
        Err(e) => error_response(&e.to_string()),
    }
}

pub async fn get_circuit_breaker(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    match state.db.get_state().await {
        Ok(s) => Json(serde_json::json!({
            "is_broken": s.is_circuit_broken,
            "broken_at": s.circuit_broken_at,
            "reason": s.circuit_broken_reason,
            "daily_loss": s.daily_loss,
            "daily_profit": s.daily_profit,
            "total_loss": s.total_loss,
            "total_profit": s.total_profit,
        })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

pub async fn reset_circuit_breaker(
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.db.reset_circuit_breaker().await {
        Ok(s) => {
            state.engine.reset_circuit_breaker().await;
            Json(serde_json::json!({
                "success": true,
                "message": "Circuit breaker reset",
                "state": s
            })).into_response()
        }
        Err(e) => error_response(&e.to_string()),
    }
}

pub async fn trigger_circuit_breaker(
    State(state): State<Arc<AppState>>,
    Query(params): Query<DisableRequest>,
) -> Response {
    match state.db.trip_circuit_breaker(&params.reason).await {
        Ok(s) => {
            state.engine.trip_circuit_breaker(&params.reason).await;
            Json(serde_json::json!({
                "success": true,
                "message": format!("Circuit breaker triggered: {}", params.reason),
                "state": s
            })).into_response()
        }
        Err(e) => error_response(&e.to_string()),
    }
}

// ==========================================
// Stats Reset Handlers
// ==========================================

pub async fn reset_daily_stats(
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.db.reset_daily_stats().await {
        Ok(s) => {
            state.engine.reset_daily_stats().await;
            Json(serde_json::json!({
                "success": true,
                "message": "Daily statistics reset",
                "state": s
            })).into_response()
        }
        Err(e) => error_response(&e.to_string()),
    }
}

pub async fn reset_all_stats(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ResetAllQuery>,
) -> Response {
    if !params.confirm || params.confirm_text != "reset all stats" {
        return bad_request("Must confirm with 'reset all stats'");
    }

    match state.db.reset_daily_stats().await {
        Ok(s) => Json(serde_json::json!({
            "success": true,
            "message": "All statistics reset",
            "state": s
        })).into_response(),
        Err(e) => error_response(&e.to_string()),
    }
}

// ==========================================
// Trade Execution Handlers
// ==========================================

pub async fn execute_trade(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ExecuteTradeRequest>,
) -> Response {
    let config = state.db.get_config().await.unwrap_or_default();
    let amount = req.amount.unwrap_or_else(|| config.trade_amount.unwrap_or(0.0));
    if amount <= 0.0 {
        return bad_request("Trade amount not configured. Please set from the dashboard.");
    }
    
    match state.engine.execute_trade(&req.path, amount).await {
        Ok(result) => {
            let trade = NewLiveTrade {
                trade_id: result.id.clone(),
                path: result.path.clone(),
                legs: result.legs.len() as i32,
                amount_in: result.start_amount,
                amount_out: Some(result.end_amount),
                profit_loss: Some(result.profit_amount),
                profit_loss_pct: Some(result.profit_pct),
                status: if result.success { "COMPLETED".to_string() } else { "FAILED".to_string() },
                current_leg: Some(result.legs.len() as i32),
                error_message: result.error.clone(),
                held_currency: None,
                held_amount: None,
                held_value_usd: None,
                order_ids: Some(serde_json::json!(result.legs.iter().map(|l| &l.order_id).collect::<Vec<_>>())),
                leg_fills: Some(serde_json::to_value(&result.legs).unwrap_or_default()),
                started_at: Some(result.executed_at),
                completed_at: Some(chrono::Utc::now()),
                total_execution_ms: Some(result.total_duration_ms as f64),
                opportunity_profit_pct: None,
            };
            
            let _ = state.db.save_trade(&trade).await;
            
            Json(serde_json::json!({
                "success": true,
                "data": result
            })).into_response()
        }
        Err(e) => error_response(&e.to_string()),
    }
}

// ==========================================
// Trade History Handlers
// ==========================================

pub async fn get_trades(
    State(state): State<Arc<AppState>>,
    Query(params): Query<TradesQuery>,
) -> impl IntoResponse {
    // Get total count for pagination
    let total_count = state.db.get_trades_count(params.status.as_deref(), params.hours).await.unwrap_or(0);

    match state.db.get_trades_paginated(params.limit, params.offset, params.status.as_deref(), params.hours).await {
        Ok(trades) => Json(serde_json::json!({
            "trades": trades,
            "pagination": {
                "total": total_count,
                "limit": params.limit,
                "offset": params.offset,
                "has_more": params.offset + (trades.len() as i64) < total_count
            }
        })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

pub async fn get_partial_trades(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    match state.db.get_trades(100, Some("PARTIAL"), 720).await {
        Ok(trades) => Json(serde_json::json!({
            "count": trades.len(),
            "trades": trades,
        })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

pub async fn get_trade(
    State(state): State<Arc<AppState>>,
    Path(trade_id): Path<String>,
) -> Response {
    match state.db.get_trade(&trade_id).await {
        Ok(Some(trade)) => Json(serde_json::json!({
            "success": true,
            "data": trade
        })).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "success": false,
                "error": "Trade not found"
            }))
        ).into_response(),
        Err(e) => error_response(&e.to_string()),
    }
}

pub async fn preview_resolve_partial(
    State(state): State<Arc<AppState>>,
    Path(trade_id): Path<String>,
) -> Response {
    let trade = match state.db.get_trade(&trade_id).await {
        Ok(Some(t)) => t,
        Ok(None) => return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "success": false,
                "error": "Trade not found"
            }))
        ).into_response(),
        Err(e) => return error_response(&e.to_string()),
    };

    if trade.status != "PARTIAL" {
        return bad_request(&format!("Trade is not PARTIAL (current: {})", trade.status));
    }

    let held_currency = trade.held_currency.as_ref().unwrap_or(&"UNKNOWN".to_string()).clone();
    let held_amount = trade.held_amount.unwrap_or(0.0);
    
    // Get current price for the held currency - try both pair formats
    let pair1 = format!("{}/USD", held_currency);
    let pair2 = format!("USD/{}", held_currency);
    
    let (current_price, estimated_usd) = if let Some(price) = state.engine.get_price(&pair1) {
        // Direct pair (e.g., EUR/USD) - multiply to get USD
        (price, held_amount * price)
    } else if let Some(price) = state.engine.get_price(&pair2) {
        // Reverse pair (e.g., USD/EUR) - divide to get USD
        (1.0 / price, held_amount / price)
    } else {
        // No price found
        (0.0, 0.0)
    };
    
    let original_amount = trade.amount_in;
    let estimated_loss = original_amount - estimated_usd;
    
    Json(serde_json::json!({
        "success": true,
        "trade_id": trade_id,
        "held_currency": held_currency,
        "held_amount": held_amount,
        "current_price": current_price,
        "estimated_usd": estimated_usd,
        "original_amount": original_amount,
        "estimated_loss": estimated_loss,
        "path": trade.path,
        "action": format!("Sell {:.6} {} for ~${:.2} USD", held_amount, held_currency, estimated_usd)
    })).into_response()
}

pub async fn resolve_partial_trade(
    State(state): State<Arc<AppState>>,
    Path(trade_id): Path<String>,
) -> Response {
    let trade = match state.db.get_trade(&trade_id).await {
        Ok(Some(t)) => t,
        Ok(None) => return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "success": false,
                "error": "Trade not found"
            }))
        ).into_response(),
        Err(e) => return error_response(&e.to_string()),
    };

    if trade.status != "PARTIAL" {
        return bad_request(&format!("Trade is not PARTIAL (current: {})", trade.status));
    }

    let original_amount = trade.amount_in;

    match state.engine.resolve_partial_trade(&trade).await {
        Ok(result) => {
            // Get the resolved amount from the execution result
            let resolved_amount_usd = result.end_amount;
            let profit_loss = resolved_amount_usd - original_amount;
            
            // Update trade and state in database
            match state.db.resolve_partial_trade(&trade_id, resolved_amount_usd, original_amount).await {
                Ok(updated_trade) => {
                    Json(serde_json::json!({
                        "success": true,
                        "message": "Partial trade resolved",
                        "resolution": {
                            "trade_id": trade_id,
                            "original_amount": original_amount,
                            "resolved_amount": resolved_amount_usd,
                            "profit_loss": profit_loss,
                            "execution": result
                        },
                        "trade": updated_trade
                    })).into_response()
                }
                Err(e) => error_response(&format!("Trade executed but failed to update DB: {}", e)),
            }
        }
        Err(e) => error_response(&e.to_string()),
    }
}

// ==========================================
// Positions Handler
// ==========================================

pub async fn get_positions(
    State(state): State<Arc<AppState>>,
) -> Response {
    // Get total portfolio value directly from Kraken TradeBalance API
    // This is the authoritative source for total USD value
    let total_usd = match state.engine.get_trade_balance().await {
        Ok(eb) => eb,
        Err(e) => {
            // Return disconnected status on error
            return Json(serde_json::json!({
                "success": false,
                "connected": false,
                "error": e.to_string(),
                "balances": {
                    "usd": 0.0,
                    "eur": 0.0,
                    "eur_in_usd": 0.0,
                    "total_usd": 0.0,
                    "eur_usd_rate": 0.0
                },
                "positions": []
            })).into_response();
        }
    };

    match state.engine.get_positions().await {
        Ok(positions) => {
            // Extract quote currency balances (USD, EUR)
            let mut usd_balance = 0.0;
            let mut eur_balance = 0.0;

            // Get EUR/USD rate for conversion (fallback to reasonable default)
            let eur_usd_rate = state.engine.get_price("EUR/USD").unwrap_or(1.04);

            // Build list of positions with USD values
            let mut positions_with_values: Vec<serde_json::Value> = Vec::new();

            for pos in &positions {
                let mut usd_value: Option<f64> = None;

                match pos.currency.as_str() {
                    "USD" | "ZUSD" => {
                        usd_balance += pos.balance;
                        usd_value = Some(pos.balance);
                    },
                    "USDT" | "USDC" => {
                        usd_value = Some(pos.balance);
                    },
                    "EUR" | "ZEUR" => {
                        eur_balance += pos.balance;
                        let value = pos.balance * eur_usd_rate;
                        usd_value = Some(value);
                    },
                    currency => {
                        // Try to get USD price for this currency from cache
                        let pair = format!("{}/USD", currency);
                        if let Some(price) = state.engine.get_price(&pair) {
                            let value = pos.balance * price;
                            usd_value = Some(value);
                        } else {
                            // Try EUR pair and convert
                            let eur_pair = format!("{}/EUR", currency);
                            if let Some(eur_price) = state.engine.get_price(&eur_pair) {
                                let value = pos.balance * eur_price * eur_usd_rate;
                                usd_value = Some(value);
                            }
                            // Individual position USD value is optional - total_usd from TradeBalance is authoritative
                        }
                    }
                }

                positions_with_values.push(serde_json::json!({
                    "currency": pos.currency,
                    "balance": pos.balance,
                    "usd_value": usd_value
                }));
            }

            // Format timestamp in ET
            let fetched_at = format_datetime_et(Utc::now());

            Json(serde_json::json!({
                "success": true,
                "connected": true,
                "balances": {
                    "usd": usd_balance,
                    "eur": eur_balance,
                    "eur_in_usd": eur_balance * eur_usd_rate,
                    "total_usd": total_usd,
                    "eur_usd_rate": eur_usd_rate
                },
                "fetched_at": fetched_at,
                "positions": positions_with_values
            })).into_response()
        },
        Err(e) => {
            // Return disconnected status on error
            Json(serde_json::json!({
                "success": false,
                "connected": false,
                "error": e.to_string(),
                "balances": {
                    "usd": 0.0,
                    "eur": 0.0,
                    "eur_in_usd": 0.0,
                    "total_usd": 0.0,
                    "eur_usd_rate": 0.0
                },
                "positions": []
            })).into_response()
        }
    }
}

// ==========================================
// Scanner Handlers
// ==========================================

pub async fn get_scanner_status(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let stats = state.engine.get_scanner_status();
    Json(serde_json::json!({
        "success": true,
        "data": stats
    }))
}

pub async fn start_scanner(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    state.engine.start_scanner().await;
    Json(serde_json::json!({
        "success": true,
        "message": "Scanner started"
    }))
}

pub async fn stop_scanner(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    state.engine.stop_scanner().await;
    Json(serde_json::json!({
        "success": true,
        "message": "Scanner stopped"
    }))
}

// ==========================================
// Opportunities Handler
// ==========================================

pub async fn get_opportunities(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let opportunities = state.engine.get_cached_opportunities();
    
    Json(serde_json::json!({
        "count": opportunities.len(),
        "opportunities": opportunities,
    }))
}

pub async fn trigger_scan(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let opportunities = state.engine.scan_now();
    
    let profitable: Vec<_> = opportunities.iter()
        .filter(|o| o.is_profitable)
        .collect();
    
    Json(serde_json::json!({
        "success": true,
        "total_opportunities": opportunities.len(),
        "profitable": profitable.len(),
        "best_profit_pct": profitable.iter()
            .map(|o| o.net_profit_pct)
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap_or(0.0),
        "opportunities": opportunities.iter().take(20).collect::<Vec<_>>(),
    }))
}

// ==========================================
// Order Book Health Handler
// ==========================================

pub async fn get_orderbook_health(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let health = state.engine.get_orderbook_health();
    let valid_pct = if health.total_pairs > 0 {
        (health.valid_pairs as f64 / health.total_pairs as f64 * 100.0).round() as u32
    } else {
        0
    };
    let skipped_total = health.skipped_no_orderbook + health.skipped_thin_depth 
        + health.skipped_stale + health.skipped_bad_spread + health.skipped_no_price;
    
    Json(serde_json::json!({
        "total_pairs": health.total_pairs,
        "valid_pairs": health.valid_pairs,
        "valid_pct": valid_pct,
        "averages": {
            "freshness_ms": health.avg_freshness_ms,
            "spread_pct": health.avg_spread_pct,
            "depth": health.avg_depth
        },
        "skipped": {
            "total": skipped_total,
            "no_orderbook": health.skipped_no_orderbook,
            "thin_depth": health.skipped_thin_depth,
            "stale": health.skipped_stale,
            "bad_spread": health.skipped_bad_spread,
            "no_price": health.skipped_no_price
        },
        "thresholds": {
            "min_depth": 3,
            "max_staleness_ms": 5000,
            "max_spread_pct": 5.0
        },
        "rejected_opportunities": health.rejected_opportunities,
        "last_update": health.last_update
    }))
}

// ==========================================
// Prices Handler
// ==========================================

pub async fn get_prices(
    State(state): State<Arc<AppState>>,
    Query(params): Query<LimitQuery>,
) -> impl IntoResponse {
    let prices = state.engine.get_prices(params.limit.unwrap_or(50));
    Json(serde_json::json!({
        "success": true,
        "data": prices
    }))
}

pub async fn get_currencies(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let currencies = state.engine.get_currencies();
    Json(serde_json::json!({
        "success": true,
        "data": currencies
    }))
}

pub async fn get_pairs(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let pairs = state.engine.get_pairs();
    Json(serde_json::json!({
        "success": true,
        "data": pairs
    }))
}

// ==========================================
// Event Scanner Stats Handler
// ==========================================

pub async fn get_event_scanner_stats(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let stats = state.engine.get_event_scanner_stats();
    Json(serde_json::json!({
        "success": true,
        "data": stats
    }))
}

// ==========================================
// Fee Config Handlers
// ==========================================

/// GET /api/fees - Get current fee configuration from database
pub async fn get_fee_config(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    match state.db.get_fee_configuration().await {
        Ok(fee_config) => Json(serde_json::json!({
            "success": true,
            "data": {
                "maker_fee": fee_config.maker_fee,
                "taker_fee": fee_config.taker_fee,
                "fee_source": fee_config.fee_source,
                "volume_tier": fee_config.volume_tier,
                "thirty_day_volume": fee_config.thirty_day_volume,
                "last_fetched_at": fee_config.last_fetched_at,
                "last_updated_at": fee_config.last_updated_at,
                "is_configured": fee_config.fee_source != "pending"
            }
        })).into_response(),
        Err(e) => error_response(&e.to_string()),
    }
}

/// PUT /api/fees - Manually update fee configuration (only when engine stopped)
pub async fn update_fee_config(
    State(state): State<Arc<AppState>>,
    Json(updates): Json<FeeConfigUpdate>,
) -> Response {
    // Check if engine is running - fees can only be updated when stopped
    let stats = state.engine.get_stats().await;
    if stats.is_running {
        return bad_request("Cannot update fees while engine is running. Please stop the engine first.");
    }

    // Validate fees
    let maker_fee = updates.maker_fee.unwrap_or(0.0);
    let taker_fee = updates.taker_fee.unwrap_or(0.0);

    if maker_fee < 0.0 || maker_fee > 0.1 {
        return bad_request("Maker fee must be between 0% and 10%");
    }
    if taker_fee < 0.0 || taker_fee > 0.1 {
        return bad_request("Taker fee must be between 0% and 10%");
    }

    // Update in database
    match state.db.update_fee_manual(maker_fee, taker_fee).await {
        Ok(fee_config) => {
            // Also update the engine's fee config
            state.engine.update_fee_config(Some(maker_fee), Some(taker_fee)).await;
            info!("Fee configuration manually updated: maker={:.4}%, taker={:.4}%",
                maker_fee * 100.0, taker_fee * 100.0);
            Json(serde_json::json!({
                "success": true,
                "message": "Fee configuration updated manually",
                "data": {
                    "maker_fee": fee_config.maker_fee,
                    "taker_fee": fee_config.taker_fee,
                    "fee_source": fee_config.fee_source,
                    "last_updated_at": fee_config.last_updated_at
                }
            })).into_response()
        }
        Err(e) => error_response(&e.to_string()),
    }
}

/// POST /api/fees/fetch - Fetch fees from Kraken API and store in database
pub async fn fetch_fees_from_kraken(
    State(state): State<Arc<AppState>>,
) -> Response {
    // Check if engine is running - fees can only be fetched when stopped (unless initial fetch)
    let stats = state.engine.get_stats().await;
    if stats.is_running {
        return bad_request("Cannot fetch fees while engine is running. Please stop the engine first.");
    }

    info!("Fetching fees from Kraken API...");

    match state.engine.fetch_kraken_fees().await {
        Ok(fee_data) => {
            // Extract fee values
            let taker_fee = fee_data.get("taker_fee").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let maker_fee = fee_data.get("maker_fee").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let volume = fee_data.get("volume_30d").and_then(|v| v.as_str()).unwrap_or("0");
            let volume_f64 = volume.parse::<f64>().ok();

            // Store in database
            match state.db.update_fee_from_kraken(maker_fee, taker_fee, None, volume_f64).await {
                Ok(fee_config) => {
                    // Also update the engine's fee config
                    state.engine.update_fee_config(Some(maker_fee), Some(taker_fee)).await;
                    info!("Fees fetched from Kraken: maker={:.4}%, taker={:.4}%, volume={}",
                        maker_fee * 100.0, taker_fee * 100.0, volume);
                    Json(serde_json::json!({
                        "success": true,
                        "message": "Fees fetched from Kraken API",
                        "data": {
                            "maker_fee": fee_config.maker_fee,
                            "taker_fee": fee_config.taker_fee,
                            "fee_source": fee_config.fee_source,
                            "thirty_day_volume": fee_config.thirty_day_volume,
                            "last_fetched_at": fee_config.last_fetched_at
                        }
                    })).into_response()
                }
                Err(e) => error_response(&format!("Failed to store fees: {}", e)),
            }
        }
        Err(e) => {
            error!("Failed to fetch fees from Kraken: {}", e);
            Json(serde_json::json!({
                "success": false,
                "error": e,
                "message": "Failed to fetch fees from Kraken. Please check your API credentials or enter fees manually."
            })).into_response()
        }
    }
}

pub async fn get_fee_stats(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let fee_config = match state.db.get_fee_configuration().await {
        Ok(fc) => fc,
        Err(_) => crate::db::FeeConfiguration::default(),
    };
    let stats = state.engine.get_stats().await;

    Json(serde_json::json!({
        "success": true,
        "data": {
            "fee_config": {
                "maker_fee": fee_config.maker_fee,
                "taker_fee": fee_config.taker_fee,
                "fee_source": fee_config.fee_source,
                "is_configured": fee_config.fee_source != "pending"
            },
            "orders_sent": 0,
            "orders_filled": 0,
            "maker_orders_attempted": 0,
            "maker_orders_filled": 0,
            "total_fee_savings": 0.0,
            "uptime_seconds": stats.uptime_seconds
        }
    }))
}

// ==========================================
// Quick Disable Handler
// ==========================================

pub async fn quick_disable(
    State(state): State<Arc<AppState>>,
) -> Response {
    // Emergency stop - disable everything immediately
    match state.db.disable_trading("Emergency quick disable").await {
        Ok(config) => {
            state.engine.disable_auto_execution();
            state.engine.disable_trading();
            state.engine.trip_circuit_breaker("Emergency quick disable").await;
            info!("EMERGENCY: Quick disable activated");
            Json(serde_json::json!({
                "success": true,
                "message": "Emergency quick disable activated",
                "config": config
            })).into_response()
        }
        Err(e) => error_response(&e.to_string()),
    }
}

// ==========================================
// Kraken Live Fees Handler
// ==========================================

pub async fn get_kraken_fees(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    // Fetch real fees from Kraken API
    match state.engine.fetch_kraken_fees().await {
        Ok(fees) => Json(serde_json::json!({
            "success": true,
            "data": fees
        })).into_response(),
        Err(e) => {
            // Fallback to DB fee configuration
            let fee_config = state.db.get_fee_configuration().await.unwrap_or_default();
            Json(serde_json::json!({
                "success": false,
                "error": e.to_string(),
                "data": {
                    "maker_fee": fee_config.maker_fee,
                    "taker_fee": fee_config.taker_fee,
                    "source": fee_config.fee_source
                }
            })).into_response()
        }
    }
}

// ==========================================
// Past Opportunities Handler
// ==========================================

#[derive(Debug, Deserialize)]
pub struct PastOpportunitiesQuery {
    pub limit: Option<i64>,
    pub hours: Option<i32>,
}

pub async fn get_past_opportunities(
    State(state): State<Arc<AppState>>,
    Query(query): Query<PastOpportunitiesQuery>,
) -> impl IntoResponse {
    let limit = query.limit.unwrap_or(100);
    let hours = query.hours.unwrap_or(24);
    
    match state.engine.get_past_opportunities(limit, hours).await {
        Ok(opportunities) => Json(serde_json::json!({
            "success": true,
            "count": opportunities.len(),
            "hours": hours,
            "opportunities": opportunities,
        })).into_response(),
        Err(e) => Json(serde_json::json!({
            "success": false,
            "error": e.to_string(),
            "opportunities": [],
        })).into_response(),
    }
}

// ==========================================
// Restrictions Management
// ==========================================

/// GET /api/config/restrictions
/// Get current restrictions configuration
pub async fn get_restrictions(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let config = state.restrictions.get_config();
    Json(serde_json::json!({
        "success": true,
        "data": config,
    }))
}

/// POST /api/config/restrictions/refresh
/// Refresh restrictions from Kraken API
pub async fn refresh_restrictions(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    info!("API: Refreshing restrictions from Kraken API");

    match state.restrictions.refresh_from_kraken().await {
        Ok(result) => {
            info!("Restrictions refreshed successfully: {}", result.message);
            Json(serde_json::json!({
                "success": true,
                "data": result,
            })).into_response()
        }
        Err(e) => {
            error!("Failed to refresh restrictions: {}", e);
            Json(serde_json::json!({
                "success": false,
                "error": e.to_string(),
            })).into_response()
        }
    }
}

/// POST /api/config/restrictions/reload
/// Reload restrictions from JSON config file
pub async fn reload_restrictions(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    info!("API: Reloading restrictions from config file");

    match state.restrictions.load_from_file() {
        Ok(()) => {
            let config = state.restrictions.get_config();
            Json(serde_json::json!({
                "success": true,
                "message": "Restrictions reloaded from config file",
                "data": config,
            })).into_response()
        }
        Err(e) => {
            error!("Failed to reload restrictions: {}", e);
            Json(serde_json::json!({
                "success": false,
                "error": e.to_string(),
            })).into_response()
        }
    }
}

/// POST /api/config/restrictions/add
/// Add a currency to the blocked list
pub async fn add_blocked_currency(
    State(state): State<Arc<AppState>>,
    Json(request): Json<AddRemoveRequest>,
) -> impl IntoResponse {
    info!("API: Adding {} to blocked currencies", request.currency);

    match state.restrictions.add_blocked_currency(&request.currency) {
        Ok(()) => {
            let blocked = state.restrictions.get_blocked_currencies();
            Json(serde_json::json!({
                "success": true,
                "message": format!("Added {} to blocked currencies", request.currency),
                "blocked_currencies": blocked,
            })).into_response()
        }
        Err(e) => {
            error!("Failed to add blocked currency: {}", e);
            Json(serde_json::json!({
                "success": false,
                "error": e.to_string(),
            })).into_response()
        }
    }
}

/// POST /api/config/restrictions/remove
/// Remove a currency from the blocked list
pub async fn remove_blocked_currency(
    State(state): State<Arc<AppState>>,
    Json(request): Json<AddRemoveRequest>,
) -> impl IntoResponse {
    info!("API: Removing {} from blocked currencies", request.currency);

    match state.restrictions.remove_blocked_currency(&request.currency) {
        Ok(()) => {
            let blocked = state.restrictions.get_blocked_currencies();
            Json(serde_json::json!({
                "success": true,
                "message": format!("Removed {} from blocked currencies", request.currency),
                "blocked_currencies": blocked,
            })).into_response()
        }
        Err(e) => {
            error!("Failed to remove blocked currency: {}", e);
            Json(serde_json::json!({
                "success": false,
                "error": e.to_string(),
            })).into_response()
        }
    }
}

/// PUT /api/config/restrictions
/// Update the entire restrictions config
pub async fn update_restrictions(
    State(state): State<Arc<AppState>>,
    Json(request): Json<UpdateRequest>,
) -> impl IntoResponse {
    info!("API: Updating restrictions config");

    if let Some(blocked) = request.blocked_currencies {
        match state.restrictions.update_restrictions(blocked.clone(), request.allowed_assets, "api_update") {
            Ok(()) => {
                let config = state.restrictions.get_config();
                Json(serde_json::json!({
                    "success": true,
                    "message": "Restrictions updated",
                    "data": config,
                })).into_response()
            }
            Err(e) => {
                error!("Failed to update restrictions: {}", e);
                Json(serde_json::json!({
                    "success": false,
                    "error": e.to_string(),
                })).into_response()
            }
        }
    } else {
        bad_request("blocked_currencies is required")
    }
}