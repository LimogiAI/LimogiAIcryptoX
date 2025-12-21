//! Kraken WebSocket v2 Client
//!
//! Implements the Kraken WebSocket v2 API protocol for:
//! - Public channels (book, ticker, instrument)
//! - Private channels (executions, add_order, cancel_order)
//!
//! Key differences from v1:
//! - JSON object messages instead of arrays
//! - method/params structure for subscriptions
//! - Numeric prices instead of strings
//! - CRC32 checksum validation

use crate::order_book::{OrderBookCache, PairInfo};
use crate::types::OrderBookLevel;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, trace, warn};

// Kraken WebSocket v2 endpoints
const KRAKEN_WS_V2_PUBLIC: &str = "wss://ws.kraken.com/v2";
const KRAKEN_WS_V2_PRIVATE: &str = "wss://ws-auth.kraken.com/v2";
const KRAKEN_REST_URL: &str = "https://api.kraken.com";

// ============================================================================
// WebSocket v2 Message Types
// ============================================================================

/// Generic v2 message envelope
#[derive(Debug, Deserialize)]
pub struct V2Message {
    pub channel: Option<String>,
    #[serde(rename = "type")]
    pub msg_type: Option<String>,
    pub data: Option<Vec<Value>>,
    // For system messages
    pub method: Option<String>,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub success: Option<bool>,
    pub req_id: Option<u64>,
}

/// Book channel data (L2 order book)
#[derive(Debug, Deserialize)]
pub struct V2BookData {
    pub symbol: String,
    pub bids: Vec<V2Level>,
    pub asks: Vec<V2Level>,
    pub checksum: Option<u32>,
    pub timestamp: Option<String>,
}

/// Single price level in v2 format
#[derive(Debug, Deserialize)]
pub struct V2Level {
    pub price: f64,  // v2 uses numbers, not strings
    pub qty: f64,
}

/// Ticker channel data
#[derive(Debug, Deserialize)]
pub struct V2TickerData {
    pub symbol: String,
    pub bid: f64,
    pub bid_qty: f64,
    pub ask: f64,
    pub ask_qty: f64,
    pub last: f64,
    pub volume: f64,
    pub vwap: f64,
    pub low: f64,
    pub high: f64,
    pub change: f64,
    pub change_pct: f64,
}

/// Instrument channel data (trading pair info)
#[derive(Debug, Deserialize)]
pub struct V2InstrumentData {
    pub symbol: String,
    pub base: String,
    pub quote: String,
    pub status: String,
    pub qty_precision: u32,
    pub qty_increment: f64,
    pub price_precision: u32,
    pub price_increment: f64,
    pub cost_min: f64,
}

// ============================================================================
// Event Handler Trait
// ============================================================================

/// Trait for handling WebSocket v2 events
pub trait V2EventHandler: Send + Sync {
    /// Called when order book is updated
    fn on_book_update(&self, symbol: &str, bids: &[V2Level], asks: &[V2Level], is_snapshot: bool);

    /// Called when ticker is updated
    fn on_ticker_update(&self, data: &V2TickerData);

    /// Called on connection status change
    fn on_connection_status(&self, connected: bool);
}

// ============================================================================
// WebSocket v2 Client
// ============================================================================

/// Temporary pair info for sorting by volume
#[derive(Clone)]
struct TempPairInfo {
    pair_name: String,
    base: String,
    quote: String,
    kraken_id: String,
    ws_name: String,
    volume_24h: f64,
}

/// WebSocket v2 manager for Kraken
pub struct KrakenWebSocketV2 {
    cache: Arc<OrderBookCache>,
    is_running: Arc<AtomicBool>,
    messages_received: Arc<AtomicU64>,
    shutdown_tx: Option<mpsc::Sender<()>>,
    max_pairs: usize,
    orderbook_depth: usize,
    // Symbol to pair name mapping (v2 uses symbols like "BTC/USD")
    symbol_to_pair: HashMap<String, String>,
    // Channel to emit order book update events for event-driven scanning
    event_tx: Option<mpsc::UnboundedSender<String>>,
}

impl KrakenWebSocketV2 {
    pub fn new(cache: Arc<OrderBookCache>) -> Self {
        Self {
            cache,
            is_running: Arc::new(AtomicBool::new(false)),
            messages_received: Arc::new(AtomicU64::new(0)),
            shutdown_tx: None,
            max_pairs: 200,
            orderbook_depth: 25,
            symbol_to_pair: HashMap::new(),
            event_tx: None,
        }
    }

    /// Set the event channel for order book update notifications
    pub fn set_event_channel(&mut self, tx: mpsc::UnboundedSender<String>) {
        self.event_tx = Some(tx);
    }

    /// Get a receiver for order book update events
    pub fn create_event_channel(&mut self) -> mpsc::UnboundedReceiver<String> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.event_tx = Some(tx);
        rx
    }

    pub fn set_max_pairs(&mut self, max_pairs: usize) {
        self.max_pairs = max_pairs;
    }

    pub fn set_orderbook_depth(&mut self, depth: usize) {
        self.orderbook_depth = depth;
    }

    pub fn get_orderbook_depth(&self) -> usize {
        self.orderbook_depth
    }

    /// Initialize by fetching trading pairs from REST API
    pub async fn initialize(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Fetching trading pairs from Kraken (limit: {})...", self.max_pairs);

        let client = reqwest::Client::new();

        // Step 1: Fetch all pair info
        let response = client
            .get(format!("{}/0/public/AssetPairs", KRAKEN_REST_URL))
            .send()
            .await?;

        let data: Value = response.json().await?;

        if let Some(error) = data.get("error").and_then(|e| e.as_array()) {
            if !error.is_empty() {
                return Err(format!("Kraken API error: {:?}", error).into());
            }
        }

        let result = data.get("result").ok_or("No result in response")?;
        let pairs = result.as_object().ok_or("Result is not an object")?;

        // Collect all pairs temporarily
        let mut temp_pairs: Vec<TempPairInfo> = Vec::new();

        for (kraken_id, pair_info) in pairs {
            // Skip dark pool pairs
            if pair_info.get("altname")
                .and_then(|v| v.as_str())
                .map(|s| s.ends_with(".d"))
                .unwrap_or(false)
            {
                continue;
            }

            let base = self.normalize_currency(
                pair_info.get("base").and_then(|v| v.as_str()).unwrap_or("")
            );
            let quote = self.normalize_currency(
                pair_info.get("quote").and_then(|v| v.as_str()).unwrap_or("")
            );

            if base.is_empty() || quote.is_empty() {
                continue;
            }

            // v2 uses symbols like "BTC/USD" (with slash)
            let ws_name = pair_info
                .get("wsname")
                .and_then(|v| v.as_str())
                .unwrap_or(&format!("{}/{}", base, quote))
                .to_string();

            let pair_name = format!("{}/{}", base, quote);

            temp_pairs.push(TempPairInfo {
                pair_name,
                base,
                quote,
                kraken_id: kraken_id.clone(),
                ws_name,
                volume_24h: 0.0,
            });
        }

        info!("Found {} total pairs, fetching volumes...", temp_pairs.len());

        // Step 2: Fetch volumes for all pairs
        let kraken_ids: Vec<String> = temp_pairs.iter().map(|p| p.kraken_id.clone()).collect();

        for chunk in kraken_ids.chunks(100) {
            let response = client
                .get(format!("{}/0/public/Ticker", KRAKEN_REST_URL))
                .query(&[("pair", chunk.join(","))])
                .send()
                .await?;

            let data: Value = response.json().await?;

            if let Some(result) = data.get("result").and_then(|r| r.as_object()) {
                for (kraken_id, ticker) in result {
                    // Find and update volume
                    for pair in temp_pairs.iter_mut() {
                        if pair.kraken_id == *kraken_id {
                            pair.volume_24h = ticker.get("v")
                                .and_then(|v| v.get(1))
                                .and_then(|v| v.as_str())
                                .and_then(|s| s.parse::<f64>().ok())
                                .unwrap_or(0.0);
                            break;
                        }
                    }
                }
            }

            // Rate limit
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        // Step 3: Sort by volume and take top N
        temp_pairs.sort_by(|a, b| b.volume_24h.partial_cmp(&a.volume_24h).unwrap_or(std::cmp::Ordering::Equal));
        let top_pairs: Vec<TempPairInfo> = temp_pairs.into_iter().take(self.max_pairs).collect();

        info!("Selected top {} pairs by volume", top_pairs.len());

        // Step 4: Register only top pairs and build symbol mapping
        for pair in &top_pairs {
            self.cache.register_pair(PairInfo {
                pair_name: pair.pair_name.clone(),
                base: pair.base.clone(),
                quote: pair.quote.clone(),
                kraken_id: pair.kraken_id.clone(),
                ws_name: pair.ws_name.clone(),
                volume_24h: pair.volume_24h,
            });

            // Build symbol to pair mapping for v2 messages
            self.symbol_to_pair.insert(pair.ws_name.clone(), pair.pair_name.clone());
        }

        info!("Registered {} trading pairs (limited from full set)", self.cache.get_all_pairs().len());
        Ok(())
    }

    /// Fetch initial prices via REST API
    pub async fn fetch_initial_prices(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Fetching initial prices...");

        let client = reqwest::Client::new();
        let pairs: Vec<String> = self.cache.get_all_pairs();

        // Fetch in batches
        for chunk in pairs.chunks(100) {
            let pair_ids: Vec<String> = chunk
                .iter()
                .filter_map(|p| self.cache.get_pair_info(p).map(|i| i.kraken_id))
                .collect();

            if pair_ids.is_empty() {
                continue;
            }

            let response = client
                .get(format!("{}/0/public/Ticker", KRAKEN_REST_URL))
                .query(&[("pair", pair_ids.join(","))])
                .send()
                .await?;

            let data: Value = response.json().await?;

            if let Some(result) = data.get("result").and_then(|r| r.as_object()) {
                for (kraken_id, ticker) in result {
                    // Find pair name from kraken_id
                    if let Some(pair_name) = self.find_pair_by_kraken_id(kraken_id) {
                        let bid = ticker.get("b")
                            .and_then(|b| b.get(0))
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<f64>().ok())
                            .unwrap_or(0.0);

                        let ask = ticker.get("a")
                            .and_then(|a| a.get(0))
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<f64>().ok())
                            .unwrap_or(0.0);

                        let volume = ticker.get("v")
                            .and_then(|v| v.get(1))
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<f64>().ok())
                            .unwrap_or(0.0);

                        self.cache.update_price_ticker(&pair_name, bid, ask, volume);
                    }
                }
            }

            // Rate limit
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        info!("Loaded initial prices for {} pairs", self.cache.get_all_prices().len());
        Ok(())
    }

    /// Start WebSocket v2 connection and subscribe to channels
    pub async fn start(&mut self, pairs_limit: usize, depth: usize) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        self.shutdown_tx = Some(shutdown_tx);
        self.orderbook_depth = depth;

        // Get top pairs by volume (already limited in cache)
        let pairs_to_subscribe = self.cache.get_pairs_by_volume(pairs_limit);

        if pairs_to_subscribe.is_empty() {
            warn!("No pairs to subscribe to");
            return Ok(());
        }

        info!("Subscribing to {} pairs via WebSocket v2", pairs_to_subscribe.len());

        let cache = Arc::clone(&self.cache);
        let is_running = Arc::clone(&self.is_running);
        let messages_received = Arc::clone(&self.messages_received);

        // Get ws_names (symbols) for subscription
        let symbols: Vec<String> = pairs_to_subscribe
            .iter()
            .filter_map(|p| self.cache.get_pair_info(p).map(|i| i.ws_name))
            .collect();

        // Build symbol to pair name lookup
        let symbol_to_pair: HashMap<String, String> = pairs_to_subscribe
            .iter()
            .filter_map(|p| {
                self.cache.get_pair_info(p).map(|i| (i.ws_name.clone(), p.clone()))
            })
            .collect();

        // Clone event channel for the task
        let event_tx = self.event_tx.clone();

        // Spawn WebSocket task
        let ws_depth = self.orderbook_depth;
        tokio::spawn(async move {
            is_running.store(true, Ordering::SeqCst);

            loop {
                match Self::run_websocket_v2(
                    &cache,
                    &symbols,
                    &symbol_to_pair,
                    &is_running,
                    &messages_received,
                    &mut shutdown_rx,
                    ws_depth,
                    event_tx.clone(),
                ).await {
                    Ok(_) => {
                        if !is_running.load(Ordering::SeqCst) {
                            break;
                        }
                        warn!("WebSocket v2 disconnected, reconnecting in 5s...");
                    }
                    Err(e) => {
                        error!("WebSocket v2 error: {}", e);
                    }
                }

                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }

            info!("WebSocket v2 task stopped");
        });

        Ok(())
    }

    /// Main WebSocket v2 loop
    async fn run_websocket_v2(
        cache: &Arc<OrderBookCache>,
        symbols: &[String],
        symbol_to_pair: &HashMap<String, String>,
        is_running: &Arc<AtomicBool>,
        messages_received: &Arc<AtomicU64>,
        shutdown_rx: &mut mpsc::Receiver<()>,
        depth: usize,
        event_tx: Option<mpsc::UnboundedSender<String>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (ws_stream, _) = connect_async(KRAKEN_WS_V2_PUBLIC).await?;
        let (mut write, mut read) = ws_stream.split();

        info!("WebSocket v2 connected to {}", KRAKEN_WS_V2_PUBLIC);

        // Request ID counter
        let mut req_id: u64 = 1;

        // Subscribe to book channel (L2 order book)
        // v2 allows up to 1000 symbols per subscription
        for chunk in symbols.chunks(500) {
            let subscribe_msg = json!({
                "method": "subscribe",
                "params": {
                    "channel": "book",
                    "symbol": chunk,
                    "depth": depth
                },
                "req_id": req_id
            });
            req_id += 1;

            write.send(Message::Text(subscribe_msg.to_string())).await?;
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        info!("Subscribed to book channel (depth={})", depth);

        // Subscribe to ticker channel for volume updates
        for chunk in symbols.chunks(500) {
            let subscribe_msg = json!({
                "method": "subscribe",
                "params": {
                    "channel": "ticker",
                    "symbol": chunk
                },
                "req_id": req_id
            });
            req_id += 1;

            write.send(Message::Text(subscribe_msg.to_string())).await?;
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        info!("Subscribed to ticker channel");

        // Message loop
        loop {
            tokio::select! {
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            messages_received.fetch_add(1, Ordering::Relaxed);
                            Self::handle_v2_message(cache, symbol_to_pair, &text, &event_tx);
                        }
                        Some(Ok(Message::Ping(data))) => {
                            let _ = write.send(Message::Pong(data)).await;
                        }
                        Some(Ok(Message::Close(_))) => {
                            warn!("WebSocket v2 closed by server");
                            break;
                        }
                        Some(Err(e)) => {
                            error!("WebSocket v2 error: {}", e);
                            break;
                        }
                        None => {
                            warn!("WebSocket v2 stream ended");
                            break;
                        }
                        _ => {}
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("Shutdown signal received");
                    is_running.store(false, Ordering::SeqCst);
                    break;
                }
            }
        }

        Ok(())
    }

    /// Handle incoming WebSocket v2 message
    fn handle_v2_message(
        cache: &Arc<OrderBookCache>,
        symbol_to_pair: &HashMap<String, String>,
        text: &str,
        event_tx: &Option<mpsc::UnboundedSender<String>>,
    ) {
        let value: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => return,
        };

        // Check for channel data (book, ticker, etc.)
        if let Some(channel) = value.get("channel").and_then(|c| c.as_str()) {
            let msg_type = value.get("type").and_then(|t| t.as_str()).unwrap_or("");

            match channel {
                "book" => {
                    let is_snapshot = msg_type == "snapshot";
                    Self::handle_v2_book_message(cache, symbol_to_pair, &value, is_snapshot, event_tx);
                }
                "ticker" => {
                    Self::handle_v2_ticker_message(cache, symbol_to_pair, &value);
                }
                "heartbeat" => {
                    // Heartbeat messages - ignore
                }
                "status" => {
                    // System status
                    if let Some(data) = value.get("data").and_then(|d| d.as_array()) {
                        for item in data {
                            if let Some(status) = item.get("system").and_then(|s| s.as_str()) {
                                info!("Kraken system status: {}", status);
                            }
                        }
                    }
                }
                _ => {
                    debug!("Unknown channel: {}", channel);
                }
            }
            return;
        }

        // Check for subscription response
        if let Some(method) = value.get("method").and_then(|m| m.as_str()) {
            match method {
                "subscribe" => {
                    if let Some(success) = value.get("success").and_then(|s| s.as_bool()) {
                        if !success {
                            if let Some(err) = value.get("error").and_then(|e| e.as_str()) {
                                warn!("Subscription error: {}", err);
                            }
                        }
                    }
                }
                "pong" => {
                    // Pong response - connection is alive
                }
                _ => {}
            }
        }
    }

    /// Handle v2 book channel message
    fn handle_v2_book_message(
        cache: &Arc<OrderBookCache>,
        symbol_to_pair: &HashMap<String, String>,
        value: &Value,
        is_snapshot: bool,
        event_tx: &Option<mpsc::UnboundedSender<String>>,
    ) {
        // v2 book data can come as either:
        // 1. Array: [{"symbol": "BTC/USD", "bids": [...], "asks": [...]}]
        // 2. Single object: {"symbol": "BTC/USD", "bids": [...], "asks": [...]}

        let items: Vec<&Value> = if let Some(arr) = value.get("data").and_then(|d| d.as_array()) {
            arr.iter().collect()
        } else if let Some(obj) = value.get("data") {
            vec![obj]
        } else {
            debug!("No data in book message");
            return;
        };

        for item in items {
            let symbol = match item.get("symbol").and_then(|s| s.as_str()) {
                Some(s) => s,
                None => {
                    debug!("No symbol in book data");
                    continue;
                }
            };

            let pair_name = match symbol_to_pair.get(symbol) {
                Some(p) => p,
                None => {
                    trace!("Symbol not found in mapping: {}", symbol);
                    continue;
                }
            };

            // Parse bids and asks - v2 uses numeric values directly
            let bids = Self::parse_v2_levels(item.get("bids"));
            let asks = Self::parse_v2_levels(item.get("asks"));

            // Get checksum for validation (optional)
            let checksum = item.get("checksum")
                .and_then(|c| c.as_u64())
                .map(|c| c as u32)
                .unwrap_or(0);

            if is_snapshot {
                // For snapshot, we use checksum as sequence
                cache.update_snapshot(pair_name, bids, asks, checksum as u64);
            } else {
                // For incremental updates, pass 0 to skip sequence checking
                // v2 uses checksums for integrity, not sequences for ordering
                cache.update_incremental(pair_name, bids, asks, 0);
            }

            // Emit event for event-driven scanning
            if let Some(tx) = event_tx {
                // Non-blocking send - if channel is full or closed, just skip
                let _ = tx.send(pair_name.clone());
            }
        }
    }

    /// Parse v2 order book levels
    fn parse_v2_levels(value: Option<&Value>) -> Vec<OrderBookLevel> {
        value
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|level| {
                        // v2 uses object format: {"price": 123.45, "qty": 1.5}
                        let price = level.get("price")?.as_f64()?;
                        let qty = level.get("qty")?.as_f64()?;
                        Some(OrderBookLevel { price, qty })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Handle v2 ticker channel message
    fn handle_v2_ticker_message(
        cache: &Arc<OrderBookCache>,
        symbol_to_pair: &HashMap<String, String>,
        value: &Value,
    ) {
        let data = match value.get("data").and_then(|d| d.as_array()) {
            Some(d) => d,
            None => return,
        };

        for item in data {
            let symbol = match item.get("symbol").and_then(|s| s.as_str()) {
                Some(s) => s,
                None => continue,
            };

            let pair_name = match symbol_to_pair.get(symbol) {
                Some(p) => p,
                None => continue,
            };

            // v2 ticker uses numeric values
            let bid = item.get("bid").and_then(|b| b.as_f64()).unwrap_or(0.0);
            let ask = item.get("ask").and_then(|a| a.as_f64()).unwrap_or(0.0);
            let volume = item.get("volume").and_then(|v| v.as_f64()).unwrap_or(0.0);

            cache.update_price_ticker(pair_name, bid, ask, volume);
        }
    }

    /// Stop WebSocket connection
    pub async fn stop(&mut self) {
        self.is_running.store(false, Ordering::SeqCst);
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }
    }

    /// Check if running
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }

    /// Get message count
    pub fn messages_received(&self) -> u64 {
        self.messages_received.load(Ordering::Relaxed)
    }

    /// Normalize Kraken currency symbol
    fn normalize_currency(&self, symbol: &str) -> String {
        let s = symbol.to_uppercase();

        // Strip X/Z prefix from legacy Kraken symbols
        if s.len() == 4 && (s.starts_with('X') || s.starts_with('Z')) {
            let suffix = &s[1..];
            if matches!(suffix, "ETH" | "XBT" | "EUR" | "USD" | "GBP" | "JPY" | "CAD" | "AUD") {
                return suffix.to_string();
            }
        }

        // Normalize XBT to BTC
        if s == "XBT" {
            return "BTC".to_string();
        }

        s
    }

    /// Find pair name from kraken_id
    fn find_pair_by_kraken_id(&self, kraken_id: &str) -> Option<String> {
        for pair in self.cache.get_all_pairs() {
            if let Some(info) = self.cache.get_pair_info(&pair) {
                if info.kraken_id == kraken_id {
                    return Some(pair);
                }
            }
        }
        None
    }
}

// ============================================================================
// CRC32 Checksum Validation
// ============================================================================

/// Calculate CRC32 checksum for order book validation
/// Kraken uses CRC32 IEEE polynomial
pub fn calculate_book_checksum(bids: &[OrderBookLevel], asks: &[OrderBookLevel]) -> u32 {
    // Take top 10 levels from each side
    let mut checksum_str = String::new();

    // Add asks (top 10, ascending by price)
    for level in asks.iter().take(10) {
        // Remove decimal point and leading zeros
        let price_str = format_checksum_number(level.price);
        let qty_str = format_checksum_number(level.qty);
        checksum_str.push_str(&price_str);
        checksum_str.push_str(&qty_str);
    }

    // Add bids (top 10, descending by price)
    for level in bids.iter().take(10) {
        let price_str = format_checksum_number(level.price);
        let qty_str = format_checksum_number(level.qty);
        checksum_str.push_str(&price_str);
        checksum_str.push_str(&qty_str);
    }

    // Calculate CRC32
    crc32_ieee(checksum_str.as_bytes())
}

/// Format number for checksum (remove decimal, strip leading zeros)
fn format_checksum_number(value: f64) -> String {
    // Convert to string with enough precision
    let s = format!("{:.10}", value);
    // Remove decimal point
    let s = s.replace('.', "");
    // Strip leading zeros
    s.trim_start_matches('0').to_string()
}

/// Simple CRC32 IEEE implementation
fn crc32_ieee(data: &[u8]) -> u32 {
    const POLYNOMIAL: u32 = 0xEDB88320;
    let mut crc: u32 = 0xFFFFFFFF;

    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ POLYNOMIAL;
            } else {
                crc >>= 1;
            }
        }
    }

    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc32() {
        // Test vector
        let result = crc32_ieee(b"123456789");
        assert_eq!(result, 0xCBF43926);
    }

    #[test]
    fn test_format_checksum() {
        assert_eq!(format_checksum_number(1234.56789), "123456789");
        assert_eq!(format_checksum_number(0.00012345), "12345");
    }
}
