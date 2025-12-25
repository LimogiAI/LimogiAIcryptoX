//! API request handlers
//!
//! All endpoint handlers for the trading API.

use crate::db::{ConfigUpdate, NewLiveTrade};
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, error};

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
    pub status: Option<String>,
    #[serde(default = "default_hours")]
    pub hours: i32,
}

fn default_limit() -> i64 { 50 }
fn default_hours() -> i32 { 24 }

#[derive(Debug, Deserialize)]
pub struct ResetAllQuery {
    #[serde(default)]
    pub confirm: bool,
    #[serde(default)]
    pub confirm_text: String,
}

#[derive(Debug, Deserialize)]
pub struct EngineSettingsUpdate {
    pub scan_interval_ms: Option<u64>,
    pub max_pairs: Option<usize>,
    pub orderbook_depth: Option<usize>,
    pub scanner_enabled: Option<bool>,
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

pub async fn get_engine_settings(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let settings = state.engine.get_settings().await;
    
    Json(serde_json::json!({
        "scan_interval_ms": settings.scan_interval_ms,
        "max_pairs": settings.max_pairs,
        "orderbook_depth": settings.orderbook_depth,
        "scanner_enabled": settings.scanner_enabled,
        "options": {
            "scan_interval_ms": [100, 250, 500, 1000, 2000, 5000, 7000, 10000],
            "max_pairs": [100, 200, 300, 400],
            "orderbook_depth": [10, 25, 100, 500, 1000]
        }
    }))
}

pub async fn update_engine_settings(
    State(state): State<Arc<AppState>>,
    Json(updates): Json<EngineSettingsUpdate>,
) -> impl IntoResponse {
    state.engine.update_settings(
        updates.scan_interval_ms,
        updates.max_pairs,
        updates.orderbook_depth,
        updates.scanner_enabled,
    ).await;
    
    let settings = state.engine.get_settings().await;
    Json(serde_json::json!({
        "success": true,
        "message": "Settings updated",
        "data": settings
    }))
}

pub async fn restart_engine(
    State(state): State<Arc<AppState>>,
) -> Response {
    let settings = state.engine.get_settings().await;
    
    info!("Engine restart requested - restarting WebSocket with max_pairs={}, depth={}", 
          settings.max_pairs, settings.orderbook_depth);
    
    // Actually restart the WebSocket connection with new settings
    match state.engine.restart_websocket().await {
        Ok(()) => {
            let new_settings = state.engine.get_settings().await;
            Json(serde_json::json!({
                "success": true,
                "message": "Engine restarted with new settings",
                "data": new_settings
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
        Ok(config) => Json(serde_json::json!({
            "config": config,
            "options": {
                "base_currency": ["USD", "USDT", "EUR", "ALL"],
                "trade_amounts": [5.0, 10.0, 25.0, 50.0, 100.0],
                "profit_thresholds": [0.001, 0.002, 0.003, 0.005, 0.01]
            }
        })).into_response(),
        Err(e) => error_response(&e.to_string()),
    }
}

pub async fn update_config(
    State(state): State<Arc<AppState>>,
    Json(updates): Json<ConfigUpdate>,
) -> Response {
    match state.db.update_config(updates).await {
        Ok(config) => {
            state.engine.sync_config(&config);
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

    match state.db.enable_trading().await {
        Ok(config) => {
            state.engine.enable_trading();
            state.engine.enable_auto_execution();
            info!("Live trading ENABLED");
            Json(serde_json::json!({
                "success": true,
                "message": "Live trading enabled",
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
            state.engine.disable_auto_execution();
            state.engine.disable_trading();
            info!("Live trading DISABLED: {}", req.reason);
            Json(serde_json::json!({
                "success": true,
                "message": "Live trading disabled",
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
            state.engine.reset_circuit_breaker();
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
            state.engine.trip_circuit_breaker(&params.reason);
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
            state.engine.reset_daily_stats();
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
    let amount = req.amount.unwrap_or(config.trade_amount);
    
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
    match state.db.get_trades(params.limit, params.status.as_deref(), params.hours).await {
        Ok(trades) => Json(serde_json::json!({
            "count": trades.len(),
            "trades": trades,
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
    match state.engine.get_positions().await {
        Ok(positions) => {
            // Calculate total USD value if we have price data
            let mut total_usd = 0.0;
            for pos in &positions {
                if pos.currency == "USD" || pos.currency == "USDT" || pos.currency == "USDC" {
                    total_usd += pos.balance;
                } else {
                    // Try to get USD price for this currency
                    let pair = format!("{}/USD", pos.currency);
                    if let Some(price) = state.engine.get_price(&pair) {
                        total_usd += pos.balance * price;
                    }
                }
            }
            
            Json(serde_json::json!({
                "success": true,
                "connected": true,
                "total_usd": total_usd,
                "positions": positions
            })).into_response()
        },
        Err(e) => {
            // Return disconnected status on error
            Json(serde_json::json!({
                "success": false,
                "connected": false,
                "error": e.to_string(),
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

pub async fn get_fee_config(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let fee_config = state.engine.get_fee_config().await;
    Json(serde_json::json!({
        "success": true,
        "data": fee_config
    }))
}

pub async fn update_fee_config(
    State(state): State<Arc<AppState>>,
    Json(updates): Json<FeeConfigUpdate>,
) -> impl IntoResponse {
    state.engine.update_fee_config(updates.maker_fee, updates.taker_fee).await;
    let fee_config = state.engine.get_fee_config().await;
    Json(serde_json::json!({
        "success": true,
        "message": "Fee configuration updated",
        "data": fee_config
    }))
}

pub async fn get_fee_stats(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let fee_config = state.engine.get_fee_config().await;
    let stats = state.engine.get_stats().await;
    
    Json(serde_json::json!({
        "success": true,
        "data": {
            "fee_config": fee_config,
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
            state.engine.trip_circuit_breaker("Emergency quick disable");
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
            // Fallback to configured fees
            let fee_config = state.engine.get_fee_config().await;
            Json(serde_json::json!({
                "success": false,
                "error": e.to_string(),
                "data": {
                    "maker_fee": fee_config.maker_fee,
                    "taker_fee": fee_config.taker_fee,
                    "source": "config_fallback"
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