//! WebSocket handler for real-time updates to frontend
//!
//! Pushes:
//! - Opportunity updates
//! - Trade executions
//! - Scanner status
//! - Order book health

use crate::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tracing::{debug, error, info, warn};

/// WebSocket upgrade handler
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// Handle WebSocket connection
async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    
    info!("WebSocket client connected");

    // Send initial status
    let initial_status = get_status_update(&state).await;
    if let Ok(json) = serde_json::to_string(&initial_status) {
        let _ = sender.send(Message::Text(json)).await;
    }

    // Spawn task to send periodic updates
    let state_clone = Arc::clone(&state);
    let mut send_task = tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(1));
        
        loop {
            ticker.tick().await;
            
            let update = get_status_update(&state_clone).await;
            
            match serde_json::to_string(&update) {
                Ok(json) => {
                    if sender.send(Message::Text(json)).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    error!("Failed to serialize WebSocket update: {}", e);
                }
            }
        }
    });

    // Handle incoming messages (pings, commands)
    let mut recv_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    debug!("Received WebSocket message: {}", text);
                    // Handle commands if needed
                }
                Ok(Message::Ping(_data)) => {
                    debug!("Received ping");
                    // Pong is handled automatically by axum
                }
                Ok(Message::Close(_)) => {
                    info!("WebSocket client requested close");
                    break;
                }
                Err(e) => {
                    warn!("WebSocket error: {}", e);
                    break;
                }
                _ => {}
            }
        }
    });

    // Wait for either task to complete
    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }

    info!("WebSocket client disconnected");
}

/// WebSocket update payload
#[derive(Debug, Serialize)]
struct WebSocketUpdate {
    #[serde(rename = "type")]
    update_type: String,
    timestamp: String,
    data: WebSocketData,
}

#[derive(Debug, Serialize)]
struct WebSocketData {
    // Engine status
    is_running: bool,
    pairs_monitored: i32,
    currencies_tracked: i32,
    
    // Trading status
    trading_enabled: bool,
    auto_execution_enabled: bool,
    is_circuit_broken: bool,
    
    // Scanner stats
    opportunities_found: u64,
    profitable_count: usize,
    best_profit_pct: f64,
    
    // Recent opportunities
    opportunities: Vec<OpportunityBrief>,
    
    // P&L
    daily_pnl: f64,
    total_pnl: f64,
}

#[derive(Debug, Serialize)]
struct OpportunityBrief {
    path: String,
    profit_pct: f64,
    is_profitable: bool,
}

async fn get_status_update(state: &Arc<AppState>) -> WebSocketUpdate {
    let engine_stats = state.engine.get_stats().await;
    let opportunities = state.engine.get_cached_opportunities();
    
    let profitable: Vec<_> = opportunities.iter()
        .filter(|o| o.is_profitable)
        .collect();
    
    let best_profit = profitable.iter()
        .map(|o| o.net_profit_pct)
        .max_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap_or(0.0);

    // Get trading state from database
    let db_state = state.db.get_state().await.unwrap_or_default();
    let config = state.db.get_config().await.unwrap_or_default();
    
    WebSocketUpdate {
        update_type: "status".to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        data: WebSocketData {
            is_running: engine_stats.is_running,
            pairs_monitored: engine_stats.pairs_monitored as i32,
            currencies_tracked: engine_stats.currencies_tracked as i32,
            trading_enabled: config.is_enabled,
            auto_execution_enabled: state.engine.is_auto_execution_enabled(),
            is_circuit_broken: db_state.is_circuit_broken,
            opportunities_found: engine_stats.opportunities_found,
            profitable_count: profitable.len(),
            best_profit_pct: best_profit,
            opportunities: opportunities.iter()
                .take(10)
                .map(|o| OpportunityBrief {
                    path: o.path.clone(),
                    profit_pct: o.net_profit_pct,
                    is_profitable: o.is_profitable,
                })
                .collect(),
            daily_pnl: db_state.daily_profit - db_state.daily_loss,
            total_pnl: db_state.total_profit - db_state.total_loss,
        },
    }
}
