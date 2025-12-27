//! Order Execution Engine - Async-first design
//!
//! Executes arbitrage trades via Kraken WebSocket v2 private channels.
//! Designed for async Rust web servers (Axum), not Python bindings.

use crate::auth::KrakenAuth;
use crate::order_book::OrderBookCache;
use crate::types::Opportunity;
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot, RwLock};
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Get Kraken WebSocket v2 private URL from environment or use default
fn get_kraken_ws_private_url() -> String {
    std::env::var("KRAKEN_WS_V2_PRIVATE")
        .unwrap_or_else(|_| "wss://ws-auth.kraken.com/v2".to_string())
}

const ORDER_TIMEOUT_MS: u64 = 5000;  // 5 seconds for HFT (was 30s)

// ==========================================
// Error Types
// ==========================================

#[derive(Debug, Error, Clone)]
#[allow(dead_code)]
pub enum ExecutionError {
    #[error("Not authenticated")]
    NotAuthenticated,
    #[error("WebSocket not connected")]
    NotConnected,
    #[error("Order rejected: {0}")]
    OrderRejected(String),
    #[error("Order timeout after {0}ms")]
    Timeout(u64),
    #[error("Partial fill: got {filled} of {expected}")]
    PartialFill { filled: f64, expected: f64 },
    #[error("Execution failed at leg {leg}: {reason}")]
    LegFailed { leg: usize, reason: String },
    #[error("WebSocket error: {0}")]
    WebSocketError(String),
    #[error("Invalid path format: {0}")]
    InvalidPath(String),
}

// ==========================================
// Order Types
// ==========================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderSide {
    Buy,
    Sell,
}

impl std::fmt::Display for OrderSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrderSide::Buy => write!(f, "buy"),
            OrderSide::Sell => write!(f, "sell"),
        }
    }
}


// ==========================================
// Result Types
// ==========================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegResult {
    pub leg_index: usize,
    pub pair: String,
    pub side: String,
    pub order_id: String,
    pub input_amount: f64,
    pub output_amount: f64,
    pub avg_price: f64,
    pub fee: f64,
    pub duration_ms: u64,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeResult {
    pub id: String,
    pub path: String,
    pub legs: Vec<LegResult>,
    pub start_amount: f64,
    pub end_amount: f64,
    pub profit_amount: f64,
    pub profit_pct: f64,
    pub total_fees: f64,
    pub total_duration_ms: u64,
    pub success: bool,
    pub error: Option<String>,
    pub executed_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct OrderResponse {
    pub order_id: String,
    #[allow(dead_code)]
    pub status: String,
    pub filled_qty: f64,
    pub avg_price: f64,
    pub cum_cost: f64,  // Cumulative cost (quote currency spent for BUY orders)
    pub fee: f64,
    #[allow(dead_code)]
    pub error: Option<String>,
}

// ==========================================
// Internal Types
// ==========================================

#[allow(dead_code)]
struct PendingOrder {
    order_id: String,
    client_id: String,
    response_tx: oneshot::Sender<OrderResponse>,
    created_at: Instant,
}

// ==========================================
// Execution Engine
// ==========================================

pub struct ExecutionEngine {
    auth: Arc<KrakenAuth>,
    cache: Arc<OrderBookCache>,
    
    // WebSocket state - using tokio async locks
    is_connected: Arc<AtomicBool>,
    ws_tx: Arc<RwLock<Option<mpsc::UnboundedSender<String>>>>,
    
    // Pending orders - using tokio async locks
    pending_orders: Arc<RwLock<HashMap<String, PendingOrder>>>,
    
    // Request ID counter (atomic - no lock needed)
    req_id_counter: AtomicU64,
    
    // Statistics (wrapped in Arc for sharing across tasks)
    orders_sent: Arc<AtomicU64>,
    orders_filled: Arc<AtomicU64>,
    orders_failed: Arc<AtomicU64>,
    orders_timed_out: Arc<AtomicU64>,
}

// Ensure ExecutionEngine is Send + Sync for async handlers
unsafe impl Send for ExecutionEngine {}
unsafe impl Sync for ExecutionEngine {}

impl ExecutionEngine {
    /// Create a new execution engine
    pub fn new(auth: Arc<KrakenAuth>, cache: Arc<OrderBookCache>) -> Self {
        Self {
            auth,
            cache,
            is_connected: Arc::new(AtomicBool::new(false)),
            ws_tx: Arc::new(RwLock::new(None)),
            pending_orders: Arc::new(RwLock::new(HashMap::new())),
            req_id_counter: AtomicU64::new(1),
            orders_sent: Arc::new(AtomicU64::new(0)),
            orders_filled: Arc::new(AtomicU64::new(0)),
            orders_failed: Arc::new(AtomicU64::new(0)),
            orders_timed_out: Arc::new(AtomicU64::new(0)),
        }
    }
    
    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }
    
    /// Get next request ID
    fn next_req_id(&self) -> u64 {
        self.req_id_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Connect to Kraken WebSocket
    pub async fn connect(&self) -> Result<(), ExecutionError> {
        info!("Connecting to Kraken private WebSocket...");
        
        let token = self.auth
            .get_ws_token()
            .await
            .map_err(|e| ExecutionError::WebSocketError(e.to_string()))?;
        
        let (ws_stream, _) = connect_async(get_kraken_ws_private_url())
            .await
            .map_err(|e| ExecutionError::WebSocketError(e.to_string()))?;
        
        let (mut write, mut read) = ws_stream.split();
        
        // Create channel for sending messages
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        
        // Store sender
        *self.ws_tx.write().await = Some(tx);
        
        // Authenticate
        let auth_msg = json!({
            "method": "subscribe",
            "params": {
                "channel": "executions",
                "token": token,
                "snap_trades": false
            }
        });
        
        write.send(Message::Text(auth_msg.to_string()))
            .await
            .map_err(|e| ExecutionError::WebSocketError(e.to_string()))?;
        
        self.is_connected.store(true, Ordering::SeqCst);
        info!("Connected to Kraken private WebSocket");
        
        // Spawn message handler
        let pending_orders = Arc::clone(&self.pending_orders);
        let is_connected = Arc::clone(&self.is_connected);
        let orders_filled = Arc::clone(&self.orders_filled);
        let orders_failed = Arc::clone(&self.orders_failed);
        
        tokio::spawn(async move {
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        // Log all private WS messages for debugging
                        debug!("Private WS received: {}", text);

                        if let Ok(json) = serde_json::from_str::<Value>(&text) {
                            // Log important messages
                            if let Some(method) = json.get("method").and_then(|m| m.as_str()) {
                                if method == "subscribe" {
                                    if json.get("success").and_then(|s| s.as_bool()) == Some(true) {
                                        info!("Subscribed to executions channel");
                                    } else {
                                        warn!("Failed to subscribe to executions: {:?}", json);
                                    }
                                }
                            }

                            // Handle add_order responses
                            if json.get("method").and_then(|m| m.as_str()) == Some("add_order") {
                                if json.get("success").and_then(|s| s.as_bool()) == Some(true) {
                                    info!("Order placed: {:?}", json.get("result"));
                                } else {
                                    // Order rejected - complete pending order immediately
                                    let error_msg = json.get("error")
                                        .and_then(|e| e.as_str())
                                        .unwrap_or("Order rejected");
                                    warn!("Order rejected: {}", error_msg);

                                    // Find the pending order by req_id and complete it with error
                                    if let Some(req_id) = json.get("req_id").and_then(|r| r.as_u64()) {
                                        let client_id = format!("arb_{}", req_id);
                                        let mut orders = pending_orders.write().await;
                                        if let Some(pending) = orders.remove(&client_id) {
                                            orders_failed.fetch_add(1, Ordering::Relaxed);
                                            let response = OrderResponse {
                                                order_id: String::new(),
                                                status: "rejected".to_string(),
                                                filled_qty: 0.0,
                                                avg_price: 0.0,
                                                cum_cost: 0.0,
                                                fee: 0.0,
                                                error: Some(error_msg.to_string()),
                                            };
                                            let _ = pending.response_tx.send(response);
                                        }
                                    }
                                }
                            }

                            // Handle execution updates
                            if json.get("channel").and_then(|c| c.as_str()) == Some("executions") {
                                info!("Raw execution data: {}", serde_json::to_string(&json).unwrap_or_default());
                                if let Some(data) = json.get("data").and_then(|d| d.as_array()) {
                                    for exec in data {
                                        let order_id = exec.get("order_id")
                                            .and_then(|o| o.as_str())
                                            .unwrap_or("");
                                        let cl_ord_id = exec.get("cl_ord_id")
                                            .and_then(|o| o.as_str())
                                            .unwrap_or("");
                                        let status = exec.get("order_status")
                                            .and_then(|s| s.as_str())
                                            .unwrap_or("");
                                        let exec_type = exec.get("exec_type")
                                            .and_then(|e| e.as_str())
                                            .unwrap_or("");

                                        // Helper to parse value as f64 (handles both string and number)
                                        fn parse_f64(v: &serde_json::Value) -> f64 {
                                            v.as_f64()
                                                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                                                .unwrap_or(0.0)
                                        }

                                        // Parse quantity - cum_qty is cumulative filled quantity
                                        let cum_qty = exec.get("cum_qty")
                                            .map(parse_f64)
                                            .unwrap_or(0.0);

                                        // Parse avg_price for overall order
                                        let avg_price = exec.get("avg_price")
                                            .map(parse_f64)
                                            .unwrap_or(0.0);

                                        // Parse cumulative cost (quote currency spent for BUY orders)
                                        let cum_cost = exec.get("cum_cost")
                                            .map(parse_f64)
                                            .unwrap_or(0.0);

                                        // Parse fees - Kraken v2 uses fee_usd_equiv for total USD fees
                                        let fee = exec.get("fee_usd_equiv")
                                            .map(parse_f64)
                                            .or_else(|| {
                                                // Fallback: sum fees from the fees array
                                                exec.get("fees")
                                                    .and_then(|f| f.as_array())
                                                    .map(|fees| {
                                                        fees.iter()
                                                            .filter_map(|fee_item| {
                                                                fee_item.get("qty").map(parse_f64)
                                                            })
                                                            .sum()
                                                    })
                                            })
                                            .unwrap_or(0.0);

                                        // For individual trade events, also track last fill
                                        let last_qty = exec.get("last_qty")
                                            .map(parse_f64)
                                            .unwrap_or(0.0);
                                        let last_price = exec.get("last_price")
                                            .map(parse_f64)
                                            .unwrap_or(0.0);

                                        info!("Execution update: order={}, cl_ord={}, status={}, exec_type={}, cum_qty={}, cum_cost={}, avg_price={}, fee={}, last_qty={}, last_price={}",
                                              order_id, cl_ord_id, status, exec_type, cum_qty, cum_cost, avg_price, fee, last_qty, last_price);

                                        // Check if order is complete (filled, canceled, or expired)
                                        if status == "filled" || status == "canceled" || status == "expired" {
                                            let mut orders = pending_orders.write().await;
                                            if let Some(pending) = orders.remove(cl_ord_id) {
                                                let response = OrderResponse {
                                                    order_id: order_id.to_string(),
                                                    status: status.to_string(),
                                                    filled_qty: cum_qty,
                                                    avg_price,
                                                    cum_cost,
                                                    fee,
                                                    error: if status != "filled" {
                                                        Some(format!("Order {}", status))
                                                    } else {
                                                        None
                                                    },
                                                };

                                                if status == "filled" {
                                                    orders_filled.fetch_add(1, Ordering::Relaxed);
                                                } else {
                                                    orders_failed.fetch_add(1, Ordering::Relaxed);
                                                }

                                                let _ = pending.response_tx.send(response);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Ok(Message::Ping(_data)) => {
                        // Pong is handled automatically by tungstenite
                    }
                    Ok(Message::Close(_)) => {
                        info!("WebSocket closed");
                        is_connected.store(false, Ordering::SeqCst);
                        break;
                    }
                    Err(e) => {
                        error!("WebSocket error: {}", e);
                        is_connected.store(false, Ordering::SeqCst);
                        break;
                    }
                    _ => {}
                }
            }
        });
        
        // Spawn sender task
        let is_connected_sender = Arc::clone(&self.is_connected);
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if write.send(Message::Text(msg)).await.is_err() {
                    is_connected_sender.store(false, Ordering::SeqCst);
                    break;
                }
            }
        });
        
        Ok(())
    }
    
    /// Place a market order
    pub async fn place_order(
        &self,
        pair: &str,
        side: OrderSide,
        quantity: f64,
    ) -> Result<OrderResponse, ExecutionError> {
        if !self.is_connected() {
            return Err(ExecutionError::NotConnected);
        }
        
        let token = self.auth
            .get_ws_token()
            .await
            .map_err(|e| ExecutionError::WebSocketError(e.to_string()))?;
        
        let req_id = self.next_req_id();
        let client_id = format!("arb_{}", req_id);
        
        // Create response channel
        let (tx, rx) = oneshot::channel();
        
        // Register pending order
        {
            let mut orders = self.pending_orders.write().await;
            orders.insert(client_id.clone(), PendingOrder {
                order_id: String::new(),
                client_id: client_id.clone(),
                response_tx: tx,
                created_at: Instant::now(),
            });
        }
        
        // Build order message
        // For BUY orders: use cash_order_qty (quote currency amount, e.g., USD)
        // For SELL orders: use order_qty (base currency amount, e.g., ETH)
        let order_msg = match side {
            OrderSide::Buy => json!({
                "method": "add_order",
                "params": {
                    "order_type": "market",
                    "side": "buy",
                    "symbol": pair,
                    "cash_order_qty": quantity,  // Spend this much quote currency (e.g., $10 USD)
                    "cl_ord_id": client_id,
                    "token": token
                },
                "req_id": req_id
            }),
            OrderSide::Sell => json!({
                "method": "add_order",
                "params": {
                    "order_type": "market",
                    "side": "sell",
                    "symbol": pair,
                    "order_qty": quantity,  // Sell this much base currency (e.g., 0.003 ETH)
                    "cl_ord_id": client_id,
                    "token": token
                },
                "req_id": req_id
            }),
        };
        
        // Send order
        {
            let ws_tx = self.ws_tx.read().await;
            if let Some(tx) = ws_tx.as_ref() {
                tx.send(order_msg.to_string())
                    .map_err(|_| ExecutionError::NotConnected)?;
                self.orders_sent.fetch_add(1, Ordering::Relaxed);
            } else {
                return Err(ExecutionError::NotConnected);
            }
        }
        
        // Wait for response with timeout
        match timeout(Duration::from_millis(ORDER_TIMEOUT_MS), rx).await {
            Ok(Ok(response)) => {
                // Check if the response contains an error (order rejected)
                if let Some(error) = &response.error {
                    Err(ExecutionError::OrderRejected(error.clone()))
                } else {
                    Ok(response)
                }
            }
            Ok(Err(_)) => Err(ExecutionError::WebSocketError("Channel closed".to_string())),
            Err(_) => {
                // Remove from pending
                self.pending_orders.write().await.remove(&client_id);
                self.orders_timed_out.fetch_add(1, Ordering::Relaxed);
                Err(ExecutionError::Timeout(ORDER_TIMEOUT_MS))
            }
        }
    }
    
    /// Execute an arbitrage opportunity
    pub async fn execute_opportunity(
        &self,
        opportunity: &Opportunity,
        start_amount: f64,
    ) -> Result<TradeResult, ExecutionError> {
        let trade_id = Uuid::new_v4().to_string();
        let start_time = Instant::now();
        let executed_at = Utc::now();
        
        info!("Executing trade {}: {} with ${:.2}", trade_id, opportunity.path, start_amount);
        
        // Parse path into legs
        let currencies: Vec<&str> = opportunity.path.split(" → ").collect();
        if currencies.len() < 3 {
            return Err(ExecutionError::InvalidPath(opportunity.path.clone()));
        }
        
        let mut current_amount = start_amount;
        let mut leg_results = Vec::new();
        let mut total_fees = 0.0;
        
        // Execute each leg
        for i in 0..currencies.len() - 1 {
            let from_currency = currencies[i];
            let to_currency = currencies[i + 1];
            
            let leg_start = Instant::now();
            
            // Determine pair and side
            let (pair, side) = self.determine_pair_and_side(from_currency, to_currency)?;
            
            info!("Leg {}: {} {} {} (amount: {:.6})", 
                i + 1, side, pair, from_currency, current_amount);
            
            // Place order
            let result = self.place_order(&pair, side, current_amount).await;
            
            let leg_duration = leg_start.elapsed().as_millis() as u64;
            
            match result {
                Ok(response) => {
                    // Calculate output amount based on order side
                    // BUY: We spend quote currency, receive base currency (filled_qty)
                    // SELL: We sell base currency, receive quote currency (cum_cost or filled_qty * avg_price)
                    let output_amount = match side {
                        OrderSide::Buy => response.filled_qty,
                        OrderSide::Sell => {
                            // Use cum_cost if available (more accurate), otherwise calculate
                            if response.cum_cost > 0.0 {
                                response.cum_cost
                            } else {
                                response.filled_qty * response.avg_price
                            }
                        }
                    };

                    info!("⚡ Leg {} completed: {} {} | in={:.8} out={:.8} | price={:.6} fee={:.6} | {}ms",
                          i + 1, side, pair, current_amount, output_amount, response.avg_price, response.fee, leg_duration);

                    total_fees += response.fee;

                    leg_results.push(LegResult {
                        leg_index: i,
                        pair: pair.clone(),
                        side: side.to_string(),
                        order_id: response.order_id,
                        input_amount: current_amount,
                        output_amount,
                        avg_price: response.avg_price,
                        fee: response.fee,
                        duration_ms: leg_duration,
                        success: true,
                        error: None,
                    });

                    current_amount = output_amount;
                }
                Err(e) => {
                    leg_results.push(LegResult {
                        leg_index: i,
                        pair: pair.clone(),
                        side: side.to_string(),
                        order_id: String::new(),
                        input_amount: current_amount,
                        output_amount: 0.0,
                        avg_price: 0.0,
                        fee: 0.0,
                        duration_ms: leg_duration,
                        success: false,
                        error: Some(e.to_string()),
                    });
                    
                    let total_duration = start_time.elapsed().as_millis() as u64;
                    
                    return Ok(TradeResult {
                        id: trade_id,
                        path: opportunity.path.clone(),
                        legs: leg_results,
                        start_amount,
                        end_amount: current_amount,
                        profit_amount: current_amount - start_amount,
                        profit_pct: ((current_amount - start_amount) / start_amount) * 100.0,
                        total_fees,
                        total_duration_ms: total_duration,
                        success: false,
                        error: Some(format!("Leg {} failed: {}", i + 1, e)),
                        executed_at,
                    });
                }
            }
        }
        
        let total_duration = start_time.elapsed().as_millis() as u64;
        // Deduct total fees (in USD equivalent) from profit calculation
        // This gives us the NET profit after all trading fees
        let profit_amount = current_amount - start_amount - total_fees;
        let profit_pct = (profit_amount / start_amount) * 100.0;

        info!("Trade {} completed: ${:.2} -> ${:.2} (fees: ${:.4}) = net {:+.4}% in {}ms",
            trade_id, start_amount, current_amount, total_fees, profit_pct, total_duration);
        
        Ok(TradeResult {
            id: trade_id,
            path: opportunity.path.clone(),
            legs: leg_results,
            start_amount,
            end_amount: current_amount,
            profit_amount,
            profit_pct,
            total_fees,
            total_duration_ms: total_duration,
            success: true,
            error: None,
            executed_at,
        })
    }
    
    /// Determine trading pair and side from currencies
    fn determine_pair_and_side(
        &self,
        from: &str,
        to: &str,
    ) -> Result<(String, OrderSide), ExecutionError> {
        // Common quote currencies
        let quote_currencies = ["USD", "USDT", "EUR", "BTC", "ETH"];
        
        // Check if direct pair exists (from/to)
        let direct_pair = format!("{}/{}", from, to);
        let reverse_pair = format!("{}/{}", to, from);
        
        // Try to get price to see which pair exists
        if self.cache.get_price(&direct_pair).is_some() {
            // from/to exists - we're selling from to get to
            return Ok((direct_pair, OrderSide::Sell));
        }
        
        if self.cache.get_price(&reverse_pair).is_some() {
            // to/from exists - we're buying to with from
            return Ok((reverse_pair, OrderSide::Buy));
        }
        
        // Fallback: guess based on quote currency conventions
        if quote_currencies.contains(&to) {
            Ok((format!("{}/{}", from, to), OrderSide::Sell))
        } else if quote_currencies.contains(&from) {
            Ok((format!("{}/{}", to, from), OrderSide::Buy))
        } else {
            Err(ExecutionError::InvalidPath(format!("Cannot determine pair for {} -> {}", from, to)))
        }
    }
    
    /// Execute a single leg trade (for resolving partial trades)
    pub async fn execute_single_leg(
        &self,
        from_currency: &str,
        to_currency: &str,
        amount: f64,
    ) -> Result<TradeResult, ExecutionError> {
        let trade_id = Uuid::new_v4().to_string();
        let start_time = Instant::now();
        let executed_at = Utc::now();
        
        info!("Executing single leg trade {}: {} -> {} amount {:.6}", 
            trade_id, from_currency, to_currency, amount);
        
        // Determine pair and side
        let (pair, side) = self.determine_pair_and_side(from_currency, to_currency)?;
        
        info!("Single leg: {} {} {} (amount: {:.6})", side, pair, from_currency, amount);
        
        // Place order
        let result = self.place_order(&pair, side, amount).await;
        let total_duration = start_time.elapsed().as_millis() as u64;
        
        match result {
            Ok(response) => {
                // Calculate output amount based on order side
                // BUY: We spend quote currency, receive base currency (filled_qty)
                // SELL: We sell base currency, receive quote currency (cum_cost or filled_qty * avg_price)
                let output_amount = match side {
                    OrderSide::Buy => response.filled_qty,
                    OrderSide::Sell => {
                        if response.cum_cost > 0.0 {
                            response.cum_cost
                        } else {
                            response.filled_qty * response.avg_price
                        }
                    }
                };

                info!("Single leg completed: {} {} | in={:.8} out={:.8} | price={:.6} fee={:.6}",
                      side, pair, amount, output_amount, response.avg_price, response.fee);

                // Deduct fee from profit calculation
                let profit_amount = output_amount - amount - response.fee;
                let profit_pct = if amount > 0.0 { (profit_amount / amount) * 100.0 } else { 0.0 };

                let leg_result = LegResult {
                    leg_index: 0,
                    pair: pair.clone(),
                    side: side.to_string(),
                    order_id: response.order_id,
                    input_amount: amount,
                    output_amount,
                    avg_price: response.avg_price,
                    fee: response.fee,
                    duration_ms: total_duration,
                    success: true,
                    error: None,
                };

                Ok(TradeResult {
                    id: trade_id,
                    path: format!("{} → {}", from_currency, to_currency),
                    legs: vec![leg_result],
                    start_amount: amount,
                    end_amount: output_amount,
                    profit_amount,
                    profit_pct,
                    total_fees: response.fee,
                    total_duration_ms: total_duration,
                    success: true,
                    error: None,
                    executed_at,
                })
            }
            Err(e) => {
                let leg_result = LegResult {
                    leg_index: 0,
                    pair: pair.clone(),
                    side: side.to_string(),
                    order_id: String::new(),
                    input_amount: amount,
                    output_amount: 0.0,
                    avg_price: 0.0,
                    fee: 0.0,
                    duration_ms: total_duration,
                    success: false,
                    error: Some(e.to_string()),
                };
                
                Ok(TradeResult {
                    id: trade_id,
                    path: format!("{} → {}", from_currency, to_currency),
                    legs: vec![leg_result],
                    start_amount: amount,
                    end_amount: 0.0,
                    profit_amount: 0.0,
                    profit_pct: 0.0,
                    total_fees: 0.0,
                    total_duration_ms: total_duration,
                    success: false,
                    error: Some(e.to_string()),
                    executed_at,
                })
            }
        }
    }
}