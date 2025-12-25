//! API module - Axum HTTP server and routes
//!
//! Native Rust Axum web server.
//! All API endpoints for the trading platform.

mod handlers;
mod websocket;

use crate::AppState;
use axum::{
    routing::{get, post, put},
    Router,
};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

/// Create the main application router with all endpoints
pub fn create_router(state: Arc<AppState>) -> Router {
    // CORS configuration
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // ==========================================
        // Status & Health
        // ==========================================
        .route("/api/health", get(handlers::health_check))
        .route("/api/status", get(handlers::get_status))
        .route("/api/engine-settings", get(handlers::get_engine_settings))
        .route("/api/engine-settings", put(handlers::update_engine_settings))
        .route("/api/engine/restart", post(handlers::restart_engine))
        
        // ==========================================
        // Live Trading Config
        // ==========================================
        .route("/api/live/config", get(handlers::get_config))
        .route("/api/live/config", put(handlers::update_config))
        
        // ==========================================
        // Enable/Disable Trading
        // ==========================================
        .route("/api/live/enable", post(handlers::enable_trading))
        .route("/api/live/disable", post(handlers::disable_trading))
        .route("/api/live/quick-disable", post(handlers::quick_disable))
        
        // ==========================================
        // Trading Status & State
        // ==========================================
        .route("/api/live/status", get(handlers::get_live_status))
        .route("/api/live/state", get(handlers::get_state))
        .route("/api/live/circuit-breaker", get(handlers::get_circuit_breaker))
        .route("/api/live/circuit-breaker/reset", post(handlers::reset_circuit_breaker))
        .route("/api/live/circuit-breaker/trigger", post(handlers::trigger_circuit_breaker))
        
        // ==========================================
        // Stats Reset
        // ==========================================
        .route("/api/live/reset-daily", post(handlers::reset_daily_stats))
        .route("/api/live/reset-all", post(handlers::reset_all_stats))
        
        // ==========================================
        // Trade Execution
        // ==========================================
        .route("/api/live/execute", post(handlers::execute_trade))
        
        // ==========================================
        // Trade History
        // ==========================================
        .route("/api/live/trades", get(handlers::get_trades))
        .route("/api/live/trades/partial", get(handlers::get_partial_trades))
        .route("/api/live/trades/:trade_id", get(handlers::get_trade))
        .route("/api/live/trades/:trade_id/resolve-preview", get(handlers::preview_resolve_partial))
        .route("/api/live/trades/:trade_id/resolve", post(handlers::resolve_partial_trade))
        
        // ==========================================
        // Positions
        // ==========================================
        .route("/api/live/positions", get(handlers::get_positions))
        
        // ==========================================
        // Scanner Control
        // ==========================================
        .route("/api/live/scanner/status", get(handlers::get_scanner_status))
        .route("/api/live/scanner/start", post(handlers::start_scanner))
        .route("/api/live/scanner/stop", post(handlers::stop_scanner))
        
        // ==========================================
        // Opportunities
        // ==========================================
        .route("/api/opportunities", get(handlers::get_opportunities))
        .route("/api/opportunities/past", get(handlers::get_past_opportunities))
        .route("/api/scan", post(handlers::trigger_scan))
        
        // ==========================================
        // Order Book Health
        // ==========================================
        .route("/api/orderbook-health", get(handlers::get_orderbook_health))
        
        // ==========================================
        // Market Data (prices, currencies, pairs)
        // ==========================================
        .route("/api/prices/live", get(handlers::get_prices))
        .route("/api/currencies", get(handlers::get_currencies))
        .route("/api/pairs", get(handlers::get_pairs))
        
        // ==========================================
        // Event Scanner Stats
        // ==========================================
        .route("/api/event-scanner-stats", get(handlers::get_event_scanner_stats))
        
        // ==========================================
        // Fee Configuration
        // ==========================================
        .route("/api/live/fee-config", get(handlers::get_fee_config))
        .route("/api/live/fee-config", put(handlers::update_fee_config))
        .route("/api/live/fee-stats", get(handlers::get_fee_stats))
        .route("/api/live/kraken-fees", get(handlers::get_kraken_fees))
        
        // ==========================================
        // WebSocket for real-time updates
        // ==========================================
        .route("/ws", get(websocket::ws_handler))
        
        // Apply middleware
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}