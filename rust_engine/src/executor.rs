//! Order Execution Engine
//!
//! Executes arbitrage trades via Kraken WebSocket v2 private channels.
//!
//! Features:
//! - WebSocket-based order placement (~50-100ms vs ~500ms REST)
//! - Real-time fill tracking via executions channel
//! - Sequential leg execution with amount propagation
//! - Parallel leg execution with pre-positioned funds (Phase 5)
//! - Automatic retry and error handling
//!
//! Execution Modes:
//! - Sequential: Each leg waits for the previous (safest, default)
//! - Parallel: Execute independent legs simultaneously (faster, requires pre-positioned funds)

use crate::auth::KrakenAuth;
use crate::order_book::OrderBookCache;
use crate::types::Opportunity;
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

const KRAKEN_WS_V2_PRIVATE: &str = "wss://ws-auth.kraken.com/v2";
const ORDER_TIMEOUT_MS: u64 = 30000; // 30 seconds to fill
const EXECUTION_TIMEOUT_MS: u64 = 60000; // 60 seconds for full trade

/// Execution errors
#[derive(Debug, Error, Clone)]
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

/// Order side
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

/// Order type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OrderType {
    Market,
    /// Limit order with price and time-in-force
    Limit { price: f64, time_in_force: TimeInForce },
}

/// Time in force for limit orders
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeInForce {
    /// Good Till Cancelled
    GTC,
    /// Immediate Or Cancel - fill what you can immediately, cancel the rest
    IOC,
    /// Fill Or Kill - fill completely or cancel entirely
    FOK,
}

impl std::fmt::Display for TimeInForce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TimeInForce::GTC => write!(f, "GTC"),
            TimeInForce::IOC => write!(f, "IOC"),
            TimeInForce::FOK => write!(f, "FOK"),
        }
    }
}

/// Fee configuration for dynamic fee optimization
#[derive(Debug, Clone, Copy)]
pub struct FeeConfig {
    /// Maker fee rate (e.g., 0.0016 for 0.16%)
    pub maker_fee: f64,
    /// Taker fee rate (e.g., 0.0026 for 0.26%)
    pub taker_fee: f64,
    /// Minimum profit margin required to attempt maker order (percentage)
    pub min_profit_for_maker: f64,
    /// Maximum spread percentage to attempt maker order
    pub max_spread_for_maker: f64,
    /// Whether to use maker orders for non-final legs
    pub use_maker_for_intermediate: bool,
}

impl Default for FeeConfig {
    fn default() -> Self {
        Self {
            maker_fee: 0.0016,       // 0.16%
            taker_fee: 0.0026,       // 0.26%
            min_profit_for_maker: 0.5, // Need 0.5% profit to try maker
            max_spread_for_maker: 0.1, // Spread must be < 0.1%
            use_maker_for_intermediate: false, // Default: taker for all legs
        }
    }
}

/// Order selection result with reasoning
#[derive(Debug, Clone)]
pub struct OrderSelection {
    pub order_type: OrderType,
    pub reason: String,
    pub estimated_savings: f64,
}

/// Execution mode for arbitrage trades
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Execute legs sequentially (safest, default)
    Sequential,
    /// Execute independent legs in parallel (faster, requires pre-positioned funds)
    Parallel,
}

impl Default for ExecutionMode {
    fn default() -> Self {
        ExecutionMode::Sequential
    }
}

/// Pre-positioned balances for parallel execution
#[derive(Debug, Clone, Default)]
pub struct PrePositionedBalances {
    /// Available balance per currency
    pub balances: HashMap<String, f64>,
}

impl PrePositionedBalances {
    pub fn new() -> Self {
        Self {
            balances: HashMap::new(),
        }
    }

    pub fn with_balance(mut self, currency: &str, amount: f64) -> Self {
        self.balances.insert(currency.to_string(), amount);
        self
    }

    pub fn get(&self, currency: &str) -> f64 {
        self.balances.get(currency).copied().unwrap_or(0.0)
    }

    pub fn has_sufficient(&self, currency: &str, amount: f64) -> bool {
        self.get(currency) >= amount
    }
}

/// Parallel execution plan - identifies which legs can run in parallel
#[derive(Debug, Clone)]
pub struct ParallelExecutionPlan {
    /// Groups of legs that can be executed in parallel
    /// Each group is executed after the previous group completes
    pub groups: Vec<LegGroup>,
    /// Whether full parallel execution is possible
    pub can_fully_parallelize: bool,
    /// Estimated time savings (percentage)
    pub estimated_speedup: f64,
}

/// A group of legs that can be executed in parallel
#[derive(Debug, Clone)]
pub struct LegGroup {
    /// Indices of legs in this group
    pub leg_indices: Vec<usize>,
    /// Whether this group can start immediately (has pre-positioned funds)
    pub can_start_immediately: bool,
}

/// A single leg of an arbitrage trade
#[derive(Debug, Clone)]
pub struct TradeLeg {
    pub pair: String,
    pub side: OrderSide,
    pub input_currency: String,  // Currency being spent
    pub output_currency: String, // Currency being received
    pub amount: f64,             // Input amount
    pub expected_output: f64,    // Expected output
}

/// Result of a single leg execution
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

/// Result of full arbitrage execution
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

/// Pending order awaiting fill confirmation
struct PendingOrder {
    order_id: String,
    client_id: String,
    response_tx: oneshot::Sender<OrderResponse>,
    created_at: Instant,
}

/// Order response from WebSocket
#[derive(Debug, Clone)]
pub struct OrderResponse {
    pub order_id: String,
    pub status: String,
    pub filled_qty: f64,
    pub avg_price: f64,
    pub fee: f64,
    pub error: Option<String>,
}

/// Execution statistics
#[derive(Debug, Clone)]
pub struct ExecutionStats {
    pub orders_placed: u64,
    pub orders_filled: u64,
    pub orders_failed: u64,
    pub total_volume: f64,
}

/// Execution engine for WebSocket-based trading
pub struct ExecutionEngine {
    auth: Arc<KrakenAuth>,
    cache: Arc<OrderBookCache>,

    // WebSocket state
    is_connected: Arc<AtomicBool>,
    ws_tx: Arc<RwLock<Option<mpsc::UnboundedSender<String>>>>,

    // Pending orders
    pending_orders: Arc<RwLock<HashMap<String, PendingOrder>>>,

    // Request ID counter
    req_id_counter: AtomicU64,

    // Statistics
    orders_sent: AtomicU64,
    orders_filled: AtomicU64,
    orders_failed: AtomicU64,
    orders_timed_out: AtomicU64,
    total_volume: RwLock<f64>,

    // Phase 6: Dynamic fee optimization
    fee_config: RwLock<FeeConfig>,
    maker_orders_attempted: AtomicU64,
    maker_orders_filled: AtomicU64,
    total_fee_savings: RwLock<f64>,

    // Reconnection control
    should_reconnect: Arc<AtomicBool>,
    reconnect_attempts: AtomicU64,
    max_reconnect_attempts: u64,

    // Cleanup task handle
    cleanup_task_running: Arc<AtomicBool>,
}

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
            orders_sent: AtomicU64::new(0),
            orders_filled: AtomicU64::new(0),
            orders_failed: AtomicU64::new(0),
            orders_timed_out: AtomicU64::new(0),
            total_volume: RwLock::new(0.0),
            fee_config: RwLock::new(FeeConfig::default()),
            maker_orders_attempted: AtomicU64::new(0),
            maker_orders_filled: AtomicU64::new(0),
            total_fee_savings: RwLock::new(0.0),
            should_reconnect: Arc::new(AtomicBool::new(true)),
            reconnect_attempts: AtomicU64::new(0),
            max_reconnect_attempts: 10,
            cleanup_task_running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Start the pending order cleanup task
    /// This runs periodically to clean up orders that have timed out without response
    pub fn start_cleanup_task(&self) {
        if self.cleanup_task_running.swap(true, Ordering::SeqCst) {
            // Already running
            return;
        }

        let pending_orders = Arc::clone(&self.pending_orders);
        let orders_timed_out = Arc::new(AtomicU64::new(0));
        let orders_timed_out_clone = Arc::clone(&orders_timed_out);
        let cleanup_running = Arc::clone(&self.cleanup_task_running);

        // Store reference to our counter
        let self_orders_timed_out = &self.orders_timed_out;
        let timeout_counter = self_orders_timed_out.load(Ordering::Relaxed);

        tokio::spawn(async move {
            info!("Started pending order cleanup task");

            while cleanup_running.load(Ordering::Relaxed) {
                // Run cleanup every 10 seconds
                tokio::time::sleep(Duration::from_secs(10)).await;

                let now = Instant::now();
                let timeout_threshold = Duration::from_millis(ORDER_TIMEOUT_MS + 5000); // Extra 5s buffer

                let mut timed_out_orders: Vec<String> = Vec::new();

                // Find timed out orders
                {
                    let orders = pending_orders.read();
                    for (client_id, pending) in orders.iter() {
                        if now.duration_since(pending.created_at) > timeout_threshold {
                            timed_out_orders.push(client_id.clone());
                        }
                    }
                }

                // Clean up timed out orders
                if !timed_out_orders.is_empty() {
                    let mut orders = pending_orders.write();
                    for client_id in &timed_out_orders {
                        if let Some(pending) = orders.remove(client_id) {
                            warn!(
                                "Cleaning up timed out order: {} (age: {:?})",
                                client_id,
                                now.duration_since(pending.created_at)
                            );
                            // Send timeout error to waiting task
                            let _ = pending.response_tx.send(OrderResponse {
                                order_id: pending.order_id.clone(),
                                status: "timeout".to_string(),
                                filled_qty: 0.0,
                                avg_price: 0.0,
                                fee: 0.0,
                                error: Some("Order timed out during cleanup".to_string()),
                            });
                            orders_timed_out_clone.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    info!("Cleaned up {} timed out pending orders", timed_out_orders.len());
                }
            }

            info!("Pending order cleanup task stopped");
        });
    }

    /// Stop the cleanup task
    pub fn stop_cleanup_task(&self) {
        self.cleanup_task_running.store(false, Ordering::SeqCst);
    }

    /// Update fee configuration
    pub fn set_fee_config(&self, config: FeeConfig) {
        *self.fee_config.write() = config;
        info!("Fee config updated: maker={:.2}%, taker={:.2}%",
            config.maker_fee * 100.0, config.taker_fee * 100.0);
    }

    /// Get current fee configuration
    pub fn get_fee_config(&self) -> FeeConfig {
        *self.fee_config.read()
    }

    // =========================================================================
    // Phase 6: Dynamic Fee Optimization
    // =========================================================================

    /// Select the optimal order type based on opportunity profit and order book conditions
    ///
    /// Decision factors:
    /// 1. Opportunity profit margin (need enough to justify risk)
    /// 2. Order book spread (tight spread = maker order more likely to fill)
    /// 3. Leg position (avoid maker on final leg for certainty)
    /// 4. Configuration settings
    pub fn select_order_type(
        &self,
        pair: &str,
        side: OrderSide,
        opportunity_profit_pct: f64,
        is_final_leg: bool,
    ) -> OrderSelection {
        let config = self.fee_config.read();

        // Get order book data
        let order_book = match self.cache.get_order_book(pair) {
            Some(book) => book,
            None => {
                return OrderSelection {
                    order_type: OrderType::Market,
                    reason: "No order book data available".to_string(),
                    estimated_savings: 0.0,
                };
            }
        };

        // Calculate spread
        let (bid, ask) = match (order_book.best_bid(), order_book.best_ask()) {
            (Some(b), Some(a)) => (b, a),
            _ => {
                return OrderSelection {
                    order_type: OrderType::Market,
                    reason: "Incomplete order book".to_string(),
                    estimated_savings: 0.0,
                };
            }
        };

        let spread = ask - bid;
        let spread_pct = (spread / bid) * 100.0;

        // Fee savings from using maker instead of taker
        let fee_savings_pct = (config.taker_fee - config.maker_fee) * 100.0; // 0.10%

        // Decision logic

        // 1. Never use maker for final leg (need certainty of completion)
        if is_final_leg {
            return OrderSelection {
                order_type: OrderType::Market,
                reason: "Final leg requires certainty".to_string(),
                estimated_savings: 0.0,
            };
        }

        // 2. Check if intermediate maker orders are enabled
        if !config.use_maker_for_intermediate {
            return OrderSelection {
                order_type: OrderType::Market,
                reason: "Maker orders disabled for intermediate legs".to_string(),
                estimated_savings: 0.0,
            };
        }

        // 3. Check profit margin is sufficient
        if opportunity_profit_pct < config.min_profit_for_maker {
            return OrderSelection {
                order_type: OrderType::Market,
                reason: format!("Profit {:.2}% < {:.2}% threshold for maker",
                    opportunity_profit_pct, config.min_profit_for_maker),
                estimated_savings: 0.0,
            };
        }

        // 4. Check spread is tight enough
        if spread_pct > config.max_spread_for_maker {
            return OrderSelection {
                order_type: OrderType::Market,
                reason: format!("Spread {:.3}% > {:.2}% max for maker",
                    spread_pct, config.max_spread_for_maker),
                estimated_savings: 0.0,
            };
        }

        // 5. Calculate maker price (post on the favorable side of the book)
        let maker_price = match side {
            // For buy: post at bid (or slightly above to be first in queue)
            OrderSide::Buy => bid + 0.00000001, // Minimal increment to be at front
            // For sell: post at ask (or slightly below to be first in queue)
            OrderSide::Sell => ask - 0.00000001,
        };

        // Use IOC (Immediate or Cancel) to get quick feedback
        // This acts as a "try maker, fall back to market" approach
        OrderSelection {
            order_type: OrderType::Limit {
                price: maker_price,
                time_in_force: TimeInForce::IOC,
            },
            reason: format!("Optimal: {:.3}% spread, {:.2}% potential fee savings",
                spread_pct, fee_savings_pct),
            estimated_savings: fee_savings_pct,
        }
    }

    /// Place an order with dynamic order type selection
    pub async fn place_order_optimized(
        &self,
        pair: &str,
        side: OrderSide,
        quantity: f64,
        opportunity_profit_pct: f64,
        is_final_leg: bool,
    ) -> Result<OrderResponse, ExecutionError> {
        let selection = self.select_order_type(pair, side, opportunity_profit_pct, is_final_leg);

        debug!("Order type selection for {} {}: {:?} - {}",
            side, pair, selection.order_type, selection.reason);

        match selection.order_type {
            OrderType::Market => {
                self.place_order(pair, side, quantity).await
            }
            OrderType::Limit { price, time_in_force } => {
                self.maker_orders_attempted.fetch_add(1, Ordering::Relaxed);

                // Try limit order first
                match self.place_limit_order(pair, side, quantity, price, time_in_force).await {
                    Ok(response) => {
                        if response.filled_qty >= quantity * 0.99 {
                            // Fully filled as maker
                            self.maker_orders_filled.fetch_add(1, Ordering::Relaxed);
                            let savings = response.filled_qty * response.avg_price * selection.estimated_savings / 100.0;
                            *self.total_fee_savings.write() += savings;
                            info!("Maker order filled! Estimated savings: ${:.4}", savings);
                        }
                        Ok(response)
                    }
                    Err(e) => {
                        // Fall back to market order
                        warn!("Limit order failed ({}), falling back to market", e);
                        self.place_order(pair, side, quantity).await
                    }
                }
            }
        }
    }

    /// Place a limit order
    pub async fn place_limit_order(
        &self,
        pair: &str,
        side: OrderSide,
        quantity: f64,
        price: f64,
        time_in_force: TimeInForce,
    ) -> Result<OrderResponse, ExecutionError> {
        if !self.is_connected() {
            return Err(ExecutionError::NotConnected);
        }

        let token = self.auth
            .get_ws_token()
            .await
            .map_err(|e| ExecutionError::WebSocketError(e.to_string()))?;

        let req_id = self.next_req_id();
        let client_id = format!("req_{}", req_id);

        // Create oneshot channel for response
        let (tx, rx) = oneshot::channel();

        // Register pending order
        {
            let mut orders = self.pending_orders.write();
            orders.insert(client_id.clone(), PendingOrder {
                order_id: String::new(),
                client_id: client_id.clone(),
                response_tx: tx,
                created_at: Instant::now(),
            });
        }

        // Build limit order message
        let order_msg = json!({
            "method": "add_order",
            "params": {
                "order_type": "limit",
                "side": side.to_string(),
                "order_qty": quantity,
                "limit_price": price,
                "symbol": pair,
                "time_in_force": time_in_force.to_string(),
                "token": token
            },
            "req_id": req_id
        });

        // Send order
        {
            let ws_tx = self.ws_tx.read();
            if let Some(ref tx) = *ws_tx {
                tx.send(order_msg.to_string())
                    .map_err(|_| ExecutionError::NotConnected)?;
            } else {
                return Err(ExecutionError::NotConnected);
            }
        }

        self.orders_sent.fetch_add(1, Ordering::Relaxed);
        info!("Limit order sent: {} {} {} @ {} ({}) (req_id={})",
            side, quantity, pair, price, time_in_force, req_id);

        // Wait for response with timeout
        match timeout(Duration::from_millis(ORDER_TIMEOUT_MS), rx).await {
            Ok(Ok(response)) => {
                if response.error.is_some() {
                    self.orders_failed.fetch_add(1, Ordering::Relaxed);
                    Err(ExecutionError::OrderRejected(response.error.unwrap()))
                } else {
                    self.orders_filled.fetch_add(1, Ordering::Relaxed);
                    *self.total_volume.write() += response.filled_qty * response.avg_price;
                    Ok(response)
                }
            }
            Ok(Err(_)) => {
                self.orders_failed.fetch_add(1, Ordering::Relaxed);
                Err(ExecutionError::WebSocketError("Channel closed".to_string()))
            }
            Err(_) => {
                // Clean up pending order
                self.pending_orders.write().remove(&client_id);
                self.orders_failed.fetch_add(1, Ordering::Relaxed);
                Err(ExecutionError::Timeout(ORDER_TIMEOUT_MS))
            }
        }
    }

    /// Get fee optimization statistics
    pub fn get_fee_stats(&self) -> (u64, u64, f64, f64) {
        let attempted = self.maker_orders_attempted.load(Ordering::Relaxed);
        let filled = self.maker_orders_filled.load(Ordering::Relaxed);
        let savings = *self.total_fee_savings.read();
        let success_rate = if attempted > 0 {
            (filled as f64 / attempted as f64) * 100.0
        } else {
            0.0
        };

        (attempted, filled, savings, success_rate)
    }

    /// Check if connected to private WebSocket
    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::SeqCst)
    }

    /// Get next request ID
    fn next_req_id(&self) -> u64 {
        self.req_id_counter.fetch_add(1, Ordering::SeqCst)
    }

    /// Connect to Kraken WebSocket v2 private channels with automatic reconnection
    pub async fn connect(&self) -> Result<(), ExecutionError> {
        self.should_reconnect.store(true, Ordering::SeqCst);
        self.reconnect_attempts.store(0, Ordering::SeqCst);

        // Start cleanup task if not already running
        self.start_cleanup_task();

        // Initial connection
        self.connect_internal().await
    }

    /// Internal connection logic (called by connect and reconnection loop)
    async fn connect_internal(&self) -> Result<(), ExecutionError> {
        if !self.auth.is_configured() {
            return Err(ExecutionError::NotAuthenticated);
        }

        // Get fresh WebSocket token
        let token = self.auth
            .get_ws_token()
            .await
            .map_err(|e| ExecutionError::WebSocketError(e.to_string()))?;

        info!("Connecting to Kraken private WebSocket...");

        // Connect to WebSocket
        let (ws_stream, _) = connect_async(KRAKEN_WS_V2_PRIVATE)
            .await
            .map_err(|e| ExecutionError::WebSocketError(e.to_string()))?;

        let (mut write, mut read) = ws_stream.split();

        // Create channel for sending messages
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        *self.ws_tx.write() = Some(tx.clone());

        // Subscribe to executions channel
        let subscribe_msg = json!({
            "method": "subscribe",
            "params": {
                "channel": "executions",
                "token": token,
                "snap_orders": true,
                "snap_trades": false
            },
            "req_id": self.next_req_id()
        });

        write.send(Message::Text(subscribe_msg.to_string()))
            .await
            .map_err(|e| ExecutionError::WebSocketError(e.to_string()))?;

        self.is_connected.store(true, Ordering::SeqCst);
        self.reconnect_attempts.store(0, Ordering::SeqCst); // Reset on successful connect
        info!("Connected to Kraken private WebSocket");

        // Clone shared state for tasks
        let pending_orders = Arc::clone(&self.pending_orders);
        let is_connected = Arc::clone(&self.is_connected);
        let should_reconnect = Arc::clone(&self.should_reconnect);
        let _reconnect_attempts = self.reconnect_attempts.load(Ordering::Relaxed);
        let max_reconnect = self.max_reconnect_attempts;
        let auth = Arc::clone(&self.auth);
        let ws_tx = Arc::clone(&self.ws_tx);
        let req_id_counter = self.req_id_counter.load(Ordering::Relaxed);

        // Spawn writer task with heartbeat/ping support
        let tx_for_heartbeat = tx.clone();
        tokio::spawn(async move {
            let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(30));
            heartbeat_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    // Handle outgoing messages
                    msg = rx.recv() => {
                        match msg {
                            Some(msg) => {
                                if write.send(Message::Text(msg)).await.is_err() {
                                    warn!("Failed to send message, WebSocket writer closing");
                                    break;
                                }
                            }
                            None => {
                                debug!("Message channel closed, WebSocket writer stopping");
                                break;
                            }
                        }
                    }
                    // Send periodic ping to keep connection alive
                    _ = heartbeat_interval.tick() => {
                        let ping_msg = json!({
                            "method": "ping"
                        });
                        if write.send(Message::Text(ping_msg.to_string())).await.is_err() {
                            warn!("Failed to send ping, WebSocket writer closing");
                            break;
                        }
                        debug!("Sent ping to private WebSocket");
                    }
                }
            }
        });

        // Spawn reader task with reconnection logic
        tokio::spawn(async move {
            let mut last_message_time = Instant::now();
            let heartbeat_timeout = Duration::from_secs(60); // Consider disconnected if no message for 60s

            loop {
                tokio::select! {
                    msg = read.next() => {
                        match msg {
                            Some(Ok(Message::Text(text))) => {
                                last_message_time = Instant::now();
                                Self::handle_message(&pending_orders, &text);
                            }
                            Some(Ok(Message::Ping(data))) => {
                                last_message_time = Instant::now();
                                // Respond with pong via the tx channel
                                // Note: tungstenite handles ping/pong at protocol level
                                debug!("Received ping from server");
                            }
                            Some(Ok(Message::Pong(_))) => {
                                last_message_time = Instant::now();
                                debug!("Received pong from server");
                            }
                            Some(Ok(Message::Close(frame))) => {
                                warn!("Private WebSocket closed by server: {:?}", frame);
                                is_connected.store(false, Ordering::SeqCst);
                                break;
                            }
                            Some(Err(e)) => {
                                error!("Private WebSocket error: {}", e);
                                is_connected.store(false, Ordering::SeqCst);
                                break;
                            }
                            None => {
                                warn!("Private WebSocket stream ended");
                                is_connected.store(false, Ordering::SeqCst);
                                break;
                            }
                            _ => {}
                        }
                    }
                    // Check for heartbeat timeout
                    _ = tokio::time::sleep(Duration::from_secs(10)) => {
                        if last_message_time.elapsed() > heartbeat_timeout {
                            warn!("Private WebSocket heartbeat timeout (no message for {:?})", heartbeat_timeout);
                            is_connected.store(false, Ordering::SeqCst);
                            break;
                        }
                    }
                }
            }

            // Attempt reconnection if enabled
            if should_reconnect.load(Ordering::Relaxed) {
                info!("Will attempt to reconnect private WebSocket...");
                Self::reconnect_loop(
                    auth,
                    ws_tx,
                    is_connected,
                    should_reconnect,
                    max_reconnect,
                ).await;
            }
        });

        Ok(())
    }

    /// Reconnection loop for private WebSocket
    async fn reconnect_loop(
        auth: Arc<KrakenAuth>,
        ws_tx: Arc<RwLock<Option<mpsc::UnboundedSender<String>>>>,
        is_connected: Arc<AtomicBool>,
        should_reconnect: Arc<AtomicBool>,
        max_attempts: u64,
    ) {
        let mut attempt = 0u64;
        let base_delay = Duration::from_secs(5);
        let max_delay = Duration::from_secs(60);

        while should_reconnect.load(Ordering::Relaxed) && attempt < max_attempts {
            attempt += 1;
            let delay = std::cmp::min(base_delay * (1 << attempt.min(4)), max_delay);

            warn!(
                "Reconnecting private WebSocket in {:?} (attempt {}/{})",
                delay, attempt, max_attempts
            );

            tokio::time::sleep(delay).await;

            if !should_reconnect.load(Ordering::Relaxed) {
                info!("Reconnection cancelled");
                break;
            }

            // Get fresh token
            let token = match auth.get_ws_token().await {
                Ok(t) => t,
                Err(e) => {
                    error!("Failed to get WebSocket token for reconnection: {}", e);
                    continue;
                }
            };

            // Attempt reconnection
            match connect_async(KRAKEN_WS_V2_PRIVATE).await {
                Ok((ws_stream, _)) => {
                    let (mut write, mut read) = ws_stream.split();

                    // Create new channel
                    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
                    *ws_tx.write() = Some(tx.clone());

                    // Subscribe to executions
                    let subscribe_msg = json!({
                        "method": "subscribe",
                        "params": {
                            "channel": "executions",
                            "token": token,
                            "snap_orders": true,
                            "snap_trades": false
                        },
                        "req_id": 1
                    });

                    if write.send(Message::Text(subscribe_msg.to_string())).await.is_err() {
                        error!("Failed to subscribe after reconnection");
                        continue;
                    }

                    is_connected.store(true, Ordering::SeqCst);
                    info!("Successfully reconnected to private WebSocket");

                    // Spawn new writer task
                    let should_reconnect_writer = Arc::clone(&should_reconnect);
                    tokio::spawn(async move {
                        let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(30));

                        loop {
                            tokio::select! {
                                msg = rx.recv() => {
                                    match msg {
                                        Some(msg) => {
                                            if write.send(Message::Text(msg)).await.is_err() {
                                                break;
                                            }
                                        }
                                        None => break,
                                    }
                                }
                                _ = heartbeat_interval.tick() => {
                                    let ping_msg = json!({"method": "ping"});
                                    if write.send(Message::Text(ping_msg.to_string())).await.is_err() {
                                        break;
                                    }
                                }
                            }
                        }
                    });

                    // Continue reading in this task
                    let pending_orders: Arc<RwLock<HashMap<String, PendingOrder>>> =
                        Arc::new(RwLock::new(HashMap::new()));
                    let mut last_message_time = Instant::now();

                    loop {
                        tokio::select! {
                            msg = read.next() => {
                                match msg {
                                    Some(Ok(Message::Text(text))) => {
                                        last_message_time = Instant::now();
                                        Self::handle_message(&pending_orders, &text);
                                    }
                                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => {
                                        is_connected.store(false, Ordering::SeqCst);
                                        break;
                                    }
                                    _ => {
                                        last_message_time = Instant::now();
                                    }
                                }
                            }
                            _ = tokio::time::sleep(Duration::from_secs(10)) => {
                                if last_message_time.elapsed() > Duration::from_secs(60) {
                                    is_connected.store(false, Ordering::SeqCst);
                                    break;
                                }
                            }
                        }
                    }

                    // Reset attempt counter on disconnect (will retry from 0)
                    attempt = 0;
                }
                Err(e) => {
                    error!("Reconnection failed: {}", e);
                }
            }
        }

        if attempt >= max_attempts {
            error!("Max reconnection attempts ({}) reached, giving up", max_attempts);
        }
    }

    /// Disconnect from private WebSocket and stop reconnection attempts
    pub fn disconnect(&self) {
        info!("Disconnecting from private WebSocket...");

        // Stop reconnection attempts
        self.should_reconnect.store(false, Ordering::SeqCst);

        // Stop cleanup task
        self.stop_cleanup_task();

        // Mark as disconnected
        self.is_connected.store(false, Ordering::SeqCst);

        // Clear the write channel (this will cause writer task to exit)
        *self.ws_tx.write() = None;

        // Clear any pending orders with error
        let mut orders = self.pending_orders.write();
        for (_, pending) in orders.drain() {
            let _ = pending.response_tx.send(OrderResponse {
                order_id: pending.order_id,
                status: "disconnected".to_string(),
                filled_qty: 0.0,
                avg_price: 0.0,
                fee: 0.0,
                error: Some("WebSocket disconnected".to_string()),
            });
        }

        info!("Disconnected from private WebSocket");
    }

    /// Handle incoming WebSocket message
    fn handle_message(
        pending_orders: &Arc<RwLock<HashMap<String, PendingOrder>>>,
        text: &str,
    ) {
        let value: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => return,
        };

        // Check for add_order response
        if let Some(method) = value.get("method").and_then(|m| m.as_str()) {
            if method == "add_order" {
                Self::handle_add_order_response(pending_orders, &value);
                return;
            }
        }

        // Check for execution updates
        if let Some(channel) = value.get("channel").and_then(|c| c.as_str()) {
            if channel == "executions" {
                Self::handle_execution_update(pending_orders, &value);
            }
        }
    }

    /// Handle add_order response
    fn handle_add_order_response(
        pending_orders: &Arc<RwLock<HashMap<String, PendingOrder>>>,
        value: &Value,
    ) {
        let success = value.get("success").and_then(|s| s.as_bool()).unwrap_or(false);
        let req_id = value.get("req_id").and_then(|r| r.as_u64()).unwrap_or(0);
        let client_id = format!("req_{}", req_id);

        if success {
            if let Some(result) = value.get("result") {
                let order_id = result.get("order_id")
                    .and_then(|o| o.as_str())
                    .unwrap_or("")
                    .to_string();

                debug!("Order placed: {} (req_id={})", order_id, req_id);

                // Update pending order with real order_id
                let mut orders = pending_orders.write();
                if let Some(pending) = orders.remove(&client_id) {
                    orders.insert(order_id.clone(), PendingOrder {
                        order_id: order_id.clone(),
                        client_id: pending.client_id,
                        response_tx: pending.response_tx,
                        created_at: pending.created_at,
                    });
                }
            }
        } else {
            let error = value.get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("Unknown error")
                .to_string();

            warn!("Order rejected: {} (req_id={})", error, req_id);

            // Send error response
            let mut orders = pending_orders.write();
            if let Some(pending) = orders.remove(&client_id) {
                let _ = pending.response_tx.send(OrderResponse {
                    order_id: String::new(),
                    status: "rejected".to_string(),
                    filled_qty: 0.0,
                    avg_price: 0.0,
                    fee: 0.0,
                    error: Some(error),
                });
            }
        }
    }

    /// Handle execution update (fills)
    fn handle_execution_update(
        pending_orders: &Arc<RwLock<HashMap<String, PendingOrder>>>,
        value: &Value,
    ) {
        let data = match value.get("data").and_then(|d| d.as_array()) {
            Some(d) => d,
            None => return,
        };

        for item in data {
            let order_id = match item.get("order_id").and_then(|o| o.as_str()) {
                Some(id) => id.to_string(),
                None => continue,
            };

            let status = item.get("order_status")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown")
                .to_string();

            // Check if order is complete (filled or canceled)
            if status == "filled" || status == "canceled" || status == "expired" {
                let filled_qty = item.get("cum_qty")
                    .and_then(|q| q.as_f64())
                    .or_else(|| item.get("cum_qty").and_then(|q| q.as_str()).and_then(|s| s.parse().ok()))
                    .unwrap_or(0.0);

                let avg_price = item.get("avg_price")
                    .and_then(|p| p.as_f64())
                    .or_else(|| item.get("avg_price").and_then(|p| p.as_str()).and_then(|s| s.parse().ok()))
                    .unwrap_or(0.0);

                let fee = item.get("fee")
                    .and_then(|f| f.as_f64())
                    .or_else(|| item.get("fee").and_then(|f| f.as_str()).and_then(|s| s.parse().ok()))
                    .unwrap_or(0.0);

                debug!("Order {} complete: status={}, filled={}, price={}",
                    order_id, status, filled_qty, avg_price);

                // Send response to waiting task
                let mut orders = pending_orders.write();
                if let Some(pending) = orders.remove(&order_id) {
                    let _ = pending.response_tx.send(OrderResponse {
                        order_id: order_id.clone(),
                        status,
                        filled_qty,
                        avg_price,
                        fee,
                        error: None,
                    });
                }
            }
        }
    }

    /// Place a market order and wait for fill
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
        let client_id = format!("req_{}", req_id);

        // Create oneshot channel for response
        let (tx, rx) = oneshot::channel();

        // Register pending order
        {
            let mut orders = self.pending_orders.write();
            orders.insert(client_id.clone(), PendingOrder {
                order_id: String::new(),
                client_id: client_id.clone(),
                response_tx: tx,
                created_at: Instant::now(),
            });
        }

        // Build order message
        let order_msg = json!({
            "method": "add_order",
            "params": {
                "order_type": "market",
                "side": side.to_string(),
                "order_qty": quantity,
                "symbol": pair,
                "token": token
            },
            "req_id": req_id
        });

        // Send order
        {
            let ws_tx = self.ws_tx.read();
            if let Some(ref tx) = *ws_tx {
                tx.send(order_msg.to_string())
                    .map_err(|_| ExecutionError::NotConnected)?;
            } else {
                return Err(ExecutionError::NotConnected);
            }
        }

        self.orders_sent.fetch_add(1, Ordering::Relaxed);
        info!("Order sent: {} {} {} (req_id={})", side, quantity, pair, req_id);

        // Wait for response with timeout
        match timeout(Duration::from_millis(ORDER_TIMEOUT_MS), rx).await {
            Ok(Ok(response)) => {
                if response.error.is_some() {
                    self.orders_failed.fetch_add(1, Ordering::Relaxed);
                    Err(ExecutionError::OrderRejected(response.error.unwrap()))
                } else {
                    self.orders_filled.fetch_add(1, Ordering::Relaxed);
                    *self.total_volume.write() += response.filled_qty * response.avg_price;
                    Ok(response)
                }
            }
            Ok(Err(_)) => {
                self.orders_failed.fetch_add(1, Ordering::Relaxed);
                Err(ExecutionError::WebSocketError("Channel closed".to_string()))
            }
            Err(_) => {
                // Clean up pending order
                self.pending_orders.write().remove(&client_id);
                self.orders_failed.fetch_add(1, Ordering::Relaxed);
                Err(ExecutionError::Timeout(ORDER_TIMEOUT_MS))
            }
        }
    }

    /// Parse an opportunity path into trade legs
    pub fn parse_path(&self, path: &str, start_amount: f64) -> Result<Vec<TradeLeg>, ExecutionError> {
        // Path format: "USD → BTC → ETH → USD"
        let currencies: Vec<&str> = path.split(" → ").collect();

        if currencies.len() < 3 {
            return Err(ExecutionError::InvalidPath(
                "Path must have at least 3 currencies".to_string()
            ));
        }

        if currencies.first() != currencies.last() {
            return Err(ExecutionError::InvalidPath(
                "Path must start and end with same currency".to_string()
            ));
        }

        let mut legs = Vec::new();
        let mut current_amount = start_amount;

        for i in 0..currencies.len() - 1 {
            let from = currencies[i];
            let to = currencies[i + 1];

            // Find the pair and determine side
            let (pair, side) = self.find_pair_and_side(from, to)?;

            // Get current rate from order book
            let rate = self.get_rate(&pair, side)?;
            let expected_output = if side == OrderSide::Sell {
                current_amount * rate
            } else {
                current_amount / rate
            };

            legs.push(TradeLeg {
                pair,
                side,
                input_currency: from.to_string(),
                output_currency: to.to_string(),
                amount: current_amount,
                expected_output,
            });

            current_amount = expected_output;
        }

        Ok(legs)
    }

    /// Find pair and side for a currency conversion
    fn find_pair_and_side(&self, from: &str, to: &str) -> Result<(String, OrderSide), ExecutionError> {
        // Try direct pair (from/to)
        let direct = format!("{}/{}", from, to);
        if self.cache.get_price(&direct).is_some() {
            // Selling "from" to get "to"
            return Ok((direct, OrderSide::Sell));
        }

        // Try inverse pair (to/from)
        let inverse = format!("{}/{}", to, from);
        if self.cache.get_price(&inverse).is_some() {
            // Buying "from" with "to"
            return Ok((inverse, OrderSide::Buy));
        }

        Err(ExecutionError::InvalidPath(
            format!("No pair found for {} → {}", from, to)
        ))
    }

    /// Get current rate for a pair/side
    fn get_rate(&self, pair: &str, side: OrderSide) -> Result<f64, ExecutionError> {
        let price = self.cache.get_price(pair)
            .ok_or_else(|| ExecutionError::InvalidPath(format!("No price for {}", pair)))?;

        Ok(match side {
            OrderSide::Sell => price.bid,
            OrderSide::Buy => price.ask,
        })
    }

    /// Execute a full arbitrage opportunity
    pub async fn execute_opportunity(
        &self,
        opportunity: &Opportunity,
        amount: f64,
    ) -> Result<TradeResult, ExecutionError> {
        let start_time = Instant::now();
        let trade_id = Uuid::new_v4().to_string();

        info!("Executing arbitrage: {} with {} start amount", opportunity.path, amount);

        // Parse path into legs
        let legs = self.parse_path(&opportunity.path, amount)?;

        let mut leg_results = Vec::new();
        let mut current_amount = amount;
        let mut total_fees = 0.0;
        let mut success = true;
        let mut error_msg = None;

        for (i, leg) in legs.iter().enumerate() {
            let leg_start = Instant::now();

            info!("Executing leg {}: {} {} {} @ ~{:.6}",
                i + 1, leg.side, current_amount, leg.pair, self.get_rate(&leg.pair, leg.side).unwrap_or(0.0));

            match self.place_order(&leg.pair, leg.side, current_amount).await {
                Ok(response) => {
                    let output_amount = if leg.side == OrderSide::Sell {
                        response.filled_qty * response.avg_price
                    } else {
                        response.filled_qty
                    };

                    leg_results.push(LegResult {
                        leg_index: i,
                        pair: leg.pair.clone(),
                        side: leg.side.to_string(),
                        order_id: response.order_id,
                        input_amount: current_amount,
                        output_amount,
                        avg_price: response.avg_price,
                        fee: response.fee,
                        duration_ms: leg_start.elapsed().as_millis() as u64,
                        success: true,
                        error: None,
                    });

                    current_amount = output_amount - response.fee;
                    total_fees += response.fee;

                    info!("Leg {} complete: {} → {}", i + 1, leg_results[i].input_amount, current_amount);
                }
                Err(e) => {
                    error!("Leg {} failed: {}", i + 1, e);

                    leg_results.push(LegResult {
                        leg_index: i,
                        pair: leg.pair.clone(),
                        side: leg.side.to_string(),
                        order_id: String::new(),
                        input_amount: current_amount,
                        output_amount: 0.0,
                        avg_price: 0.0,
                        fee: 0.0,
                        duration_ms: leg_start.elapsed().as_millis() as u64,
                        success: false,
                        error: Some(e.to_string()),
                    });

                    success = false;
                    error_msg = Some(format!("Failed at leg {}: {}", i + 1, e));
                    break;
                }
            }
        }

        let profit_amount = current_amount - amount;
        let profit_pct = (profit_amount / amount) * 100.0;

        let result = TradeResult {
            id: trade_id,
            path: opportunity.path.clone(),
            legs: leg_results,
            start_amount: amount,
            end_amount: current_amount,
            profit_amount,
            profit_pct,
            total_fees,
            total_duration_ms: start_time.elapsed().as_millis() as u64,
            success,
            error: error_msg,
            executed_at: Utc::now(),
        };

        if success {
            info!("Arbitrage complete: {:.4} profit ({:.4}%)", profit_amount, profit_pct);
        } else {
            warn!("Arbitrage failed: {:?}", result.error);
        }

        Ok(result)
    }

    /// Get execution statistics
    pub fn get_stats(&self) -> ExecutionStats {
        ExecutionStats {
            orders_placed: self.orders_sent.load(Ordering::Relaxed),
            orders_filled: self.orders_filled.load(Ordering::Relaxed),
            orders_failed: self.orders_failed.load(Ordering::Relaxed),
            total_volume: *self.total_volume.read(),
        }
    }

    // =========================================================================
    // Phase 5: Parallel Leg Execution
    // =========================================================================

    /// Analyze a path and create an execution plan that identifies parallel opportunities
    ///
    /// With pre-positioned funds, some legs can be executed in parallel:
    /// - Example: USD → BTC → EUR → USD
    ///   - If we have USD and EUR pre-positioned:
    ///     - Leg 1 (USD → BTC) and Leg 3 (EUR → USD) could potentially overlap
    ///     - But Leg 2 (BTC → EUR) must wait for Leg 1
    ///
    /// In practice, true parallelism is limited because:
    /// 1. Most paths have sequential dependencies
    /// 2. We need the output of one leg as input for the next
    /// 3. Pre-positioned funds are typically in base currencies (USD, EUR)
    pub fn analyze_parallel_opportunities(
        &self,
        legs: &[TradeLeg],
        balances: &PrePositionedBalances,
    ) -> ParallelExecutionPlan {
        if legs.is_empty() {
            return ParallelExecutionPlan {
                groups: vec![],
                can_fully_parallelize: false,
                estimated_speedup: 0.0,
            };
        }

        // For a 3-leg path: A → B → C → A
        // - Leg 0: A → B (uses A, produces B)
        // - Leg 1: B → C (uses B, produces C)
        // - Leg 2: C → A (uses C, produces A)
        //
        // Dependencies:
        // - Leg 1 depends on Leg 0 (needs B)
        // - Leg 2 depends on Leg 1 (needs C)
        //
        // Parallel opportunity with pre-positioned funds:
        // - If we have C pre-positioned, Leg 2 could start early
        // - But this creates risk: what if Leg 0 or 1 fails?

        let mut groups: Vec<LegGroup> = Vec::new();
        let mut executed_outputs: HashMap<String, f64> = HashMap::new();

        // Start with pre-positioned balances
        for (currency, &amount) in &balances.balances {
            executed_outputs.insert(currency.clone(), amount);
        }

        let mut remaining_legs: Vec<usize> = (0..legs.len()).collect();

        while !remaining_legs.is_empty() {
            let mut current_group = Vec::new();
            let mut to_remove = Vec::new();

            for &leg_idx in &remaining_legs {
                let leg = &legs[leg_idx];

                // Check if we have the input currency available
                let available = executed_outputs.get(&leg.input_currency).copied().unwrap_or(0.0);

                if available >= leg.amount * 0.99 { // 1% tolerance for rounding
                    current_group.push(leg_idx);
                    to_remove.push(leg_idx);
                }
            }

            if current_group.is_empty() {
                // No progress possible - this shouldn't happen with valid paths
                // Fall back to sequential execution
                warn!("Parallel analysis failed, falling back to sequential");
                return self.create_sequential_plan(legs);
            }

            // Remove executed legs
            for idx in &to_remove {
                remaining_legs.retain(|&i| i != *idx);
            }

            // Mark outputs as available for next iteration
            for &leg_idx in &current_group {
                let leg = &legs[leg_idx];
                // Subtract input
                if let Some(amount) = executed_outputs.get_mut(&leg.input_currency) {
                    *amount -= leg.amount;
                }
                // Add output
                *executed_outputs.entry(leg.output_currency.clone()).or_insert(0.0) += leg.expected_output;
            }

            groups.push(LegGroup {
                leg_indices: current_group.clone(),
                can_start_immediately: groups.is_empty(), // First group can start immediately
            });
        }

        // Calculate potential speedup
        // Best case: all legs in one group = 100% speedup (N times faster)
        // Worst case: all legs sequential = 0% speedup
        let total_legs = legs.len();
        let parallel_legs = groups.iter()
            .map(|g| if g.leg_indices.len() > 1 { g.leg_indices.len() - 1 } else { 0 })
            .sum::<usize>();

        let estimated_speedup = if total_legs > 1 {
            (parallel_legs as f64 / (total_legs - 1) as f64) * 100.0
        } else {
            0.0
        };

        let can_fully_parallelize = groups.len() == 1 && groups[0].leg_indices.len() == legs.len();

        ParallelExecutionPlan {
            groups,
            can_fully_parallelize,
            estimated_speedup,
        }
    }

    /// Create a simple sequential execution plan
    fn create_sequential_plan(&self, legs: &[TradeLeg]) -> ParallelExecutionPlan {
        let groups: Vec<LegGroup> = legs.iter().enumerate().map(|(i, _)| {
            LegGroup {
                leg_indices: vec![i],
                can_start_immediately: i == 0,
            }
        }).collect();

        ParallelExecutionPlan {
            groups,
            can_fully_parallelize: false,
            estimated_speedup: 0.0,
        }
    }

    /// Execute an opportunity with parallel execution where possible
    ///
    /// This method analyzes the path and executes legs in parallel when:
    /// 1. Pre-positioned funds are available
    /// 2. Legs don't have sequential dependencies
    ///
    /// For most triangular arbitrage (A → B → C → A), this results in:
    /// - Group 1: Leg 0 (if A is available)
    /// - Group 2: Leg 1 (needs output of Leg 0)
    /// - Group 3: Leg 2 (needs output of Leg 1)
    ///
    /// True parallelism is rare but possible in specific scenarios.
    pub async fn execute_opportunity_parallel(
        &self,
        opportunity: &Opportunity,
        amount: f64,
        balances: &PrePositionedBalances,
    ) -> Result<TradeResult, ExecutionError> {
        let start_time = Instant::now();
        let trade_id = Uuid::new_v4().to_string();

        info!("Executing arbitrage (parallel mode): {} with {} start amount", opportunity.path, amount);

        // Parse path into legs
        let legs = self.parse_path(&opportunity.path, amount)?;

        // Analyze parallel opportunities
        let plan = self.analyze_parallel_opportunities(&legs, balances);

        info!("Parallel execution plan: {} groups, {:.1}% estimated speedup, can_fully_parallelize={}",
            plan.groups.len(), plan.estimated_speedup, plan.can_fully_parallelize);

        let mut leg_results: Vec<LegResult> = vec![LegResult {
            leg_index: 0,
            pair: String::new(),
            side: String::new(),
            order_id: String::new(),
            input_amount: 0.0,
            output_amount: 0.0,
            avg_price: 0.0,
            fee: 0.0,
            duration_ms: 0,
            success: false,
            error: None,
        }; legs.len()];

        let mut total_fees = 0.0;
        let mut success = true;
        let mut error_msg = None;
        let mut current_amounts: HashMap<String, f64> = balances.balances.clone();

        // Add starting amount to the first currency
        let first_currency = legs.first()
            .map(|l| l.input_currency.clone())
            .unwrap_or_default();
        *current_amounts.entry(first_currency).or_insert(0.0) += amount;

        // Execute groups sequentially, but legs within each group in parallel
        for (group_idx, group) in plan.groups.iter().enumerate() {
            info!("Executing group {} with {} leg(s)", group_idx + 1, group.leg_indices.len());

            if group.leg_indices.len() == 1 {
                // Single leg - execute directly
                let leg_idx = group.leg_indices[0];
                let leg = &legs[leg_idx];
                let leg_start = Instant::now();

                let input_amount = current_amounts.get(&leg.input_currency).copied().unwrap_or(leg.amount);

                info!("Executing leg {}: {} {} {} @ ~{:.6}",
                    leg_idx + 1, leg.side, input_amount, leg.pair,
                    self.get_rate(&leg.pair, leg.side).unwrap_or(0.0));

                match self.place_order(&leg.pair, leg.side, input_amount).await {
                    Ok(response) => {
                        let output_amount = if leg.side == OrderSide::Sell {
                            response.filled_qty * response.avg_price
                        } else {
                            response.filled_qty
                        };

                        leg_results[leg_idx] = LegResult {
                            leg_index: leg_idx,
                            pair: leg.pair.clone(),
                            side: leg.side.to_string(),
                            order_id: response.order_id,
                            input_amount,
                            output_amount,
                            avg_price: response.avg_price,
                            fee: response.fee,
                            duration_ms: leg_start.elapsed().as_millis() as u64,
                            success: true,
                            error: None,
                        };

                        // Update balances
                        if let Some(amt) = current_amounts.get_mut(&leg.input_currency) {
                            *amt -= input_amount;
                        }
                        *current_amounts.entry(leg.output_currency.clone()).or_insert(0.0) +=
                            output_amount - response.fee;
                        total_fees += response.fee;

                        info!("Leg {} complete: {} → {}", leg_idx + 1, input_amount, output_amount);
                    }
                    Err(e) => {
                        error!("Leg {} failed: {}", leg_idx + 1, e);

                        leg_results[leg_idx] = LegResult {
                            leg_index: leg_idx,
                            pair: leg.pair.clone(),
                            side: leg.side.to_string(),
                            order_id: String::new(),
                            input_amount,
                            output_amount: 0.0,
                            avg_price: 0.0,
                            fee: 0.0,
                            duration_ms: leg_start.elapsed().as_millis() as u64,
                            success: false,
                            error: Some(e.to_string()),
                        };

                        success = false;
                        error_msg = Some(format!("Failed at leg {}: {}", leg_idx + 1, e));
                        break;
                    }
                }
            } else {
                // Multiple legs in parallel - use futures::join_all
                let leg_start = Instant::now();

                // Collect leg info first
                let mut leg_info: Vec<(usize, TradeLeg, f64)> = Vec::new();

                for &leg_idx in &group.leg_indices {
                    let leg = &legs[leg_idx];
                    let input_amount = current_amounts.get(&leg.input_currency).copied().unwrap_or(leg.amount);

                    info!("Queueing parallel leg {}: {} {} {} @ ~{:.6}",
                        leg_idx + 1, leg.side, input_amount, leg.pair,
                        self.get_rate(&leg.pair, leg.side).unwrap_or(0.0));

                    leg_info.push((leg_idx, leg.clone(), input_amount));
                }

                // Create and execute futures - must own the data
                let results: Vec<Result<OrderResponse, ExecutionError>> = {
                    let futures: Vec<_> = leg_info.iter()
                        .map(|(_, leg, input_amount)| {
                            let pair = leg.pair.clone();
                            let side = leg.side;
                            let amount = *input_amount;
                            async move {
                                self.place_order(&pair, side, amount).await
                            }
                        })
                        .collect();

                    futures_util::future::join_all(futures).await
                };

                // Process results
                for (result, (leg_idx, leg, input_amount)) in results.into_iter().zip(leg_info.into_iter()) {
                    match result {
                        Ok(response) => {
                            let output_amount = if leg.side == OrderSide::Sell {
                                response.filled_qty * response.avg_price
                            } else {
                                response.filled_qty
                            };

                            leg_results[leg_idx] = LegResult {
                                leg_index: leg_idx,
                                pair: leg.pair.clone(),
                                side: leg.side.to_string(),
                                order_id: response.order_id,
                                input_amount,
                                output_amount,
                                avg_price: response.avg_price,
                                fee: response.fee,
                                duration_ms: leg_start.elapsed().as_millis() as u64,
                                success: true,
                                error: None,
                            };

                            // Update balances
                            if let Some(amt) = current_amounts.get_mut(&leg.input_currency) {
                                *amt -= input_amount;
                            }
                            *current_amounts.entry(leg.output_currency.clone()).or_insert(0.0) +=
                                output_amount - response.fee;
                            total_fees += response.fee;

                            info!("Parallel leg {} complete: {} → {}", leg_idx + 1, input_amount, output_amount);
                        }
                        Err(e) => {
                            error!("Parallel leg {} failed: {}", leg_idx + 1, e);

                            leg_results[leg_idx] = LegResult {
                                leg_index: leg_idx,
                                pair: leg.pair.clone(),
                                side: leg.side.to_string(),
                                order_id: String::new(),
                                input_amount,
                                output_amount: 0.0,
                                avg_price: 0.0,
                                fee: 0.0,
                                duration_ms: leg_start.elapsed().as_millis() as u64,
                                success: false,
                                error: Some(e.to_string()),
                            };

                            success = false;
                            if error_msg.is_none() {
                                error_msg = Some(format!("Failed at leg {}: {}", leg_idx + 1, e));
                            }
                        }
                    }
                }

                if !success {
                    break;
                }
            }

            if !success {
                break;
            }
        }

        // Calculate final amounts
        let final_currency = legs.last()
            .map(|l| l.output_currency.clone())
            .unwrap_or_default();
        let end_amount = current_amounts.get(&final_currency).copied().unwrap_or(0.0);
        let profit_amount = end_amount - amount;
        let profit_pct = (profit_amount / amount) * 100.0;

        let result = TradeResult {
            id: trade_id,
            path: opportunity.path.clone(),
            legs: leg_results,
            start_amount: amount,
            end_amount,
            profit_amount,
            profit_pct,
            total_fees,
            total_duration_ms: start_time.elapsed().as_millis() as u64,
            success,
            error: error_msg,
            executed_at: Utc::now(),
        };

        if success {
            info!("Parallel arbitrage complete: {:.4} profit ({:.4}%) in {}ms",
                profit_amount, profit_pct, result.total_duration_ms);
        } else {
            warn!("Parallel arbitrage failed: {:?}", result.error);
        }

        Ok(result)
    }

    /// Execute opportunity with automatic mode selection
    ///
    /// Analyzes the path and balances to determine if parallel execution would help.
    /// Falls back to sequential if parallel wouldn't provide benefits.
    pub async fn execute_opportunity_auto(
        &self,
        opportunity: &Opportunity,
        amount: f64,
        balances: &PrePositionedBalances,
        mode: ExecutionMode,
    ) -> Result<TradeResult, ExecutionError> {
        match mode {
            ExecutionMode::Sequential => {
                self.execute_opportunity(opportunity, amount).await
            }
            ExecutionMode::Parallel => {
                // Analyze if parallel execution would help
                let legs = self.parse_path(&opportunity.path, amount)?;
                let plan = self.analyze_parallel_opportunities(&legs, balances);

                if plan.estimated_speedup > 10.0 {
                    // Worth doing parallel execution
                    info!("Using parallel execution (estimated {:.1}% speedup)", plan.estimated_speedup);
                    self.execute_opportunity_parallel(opportunity, amount, balances).await
                } else {
                    // Not worth it, use sequential
                    info!("Parallel execution not beneficial, using sequential");
                    self.execute_opportunity(opportunity, amount).await
                }
            }
        }
    }
}
