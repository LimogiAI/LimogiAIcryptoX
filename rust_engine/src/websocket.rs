//! Kraken WebSocket client for real-time order book streaming

use crate::order_book::{OrderBookCache, PairInfo};
use crate::types::OrderBookLevel;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, warn};

const KRAKEN_WS_URL: &str = "wss://ws.kraken.com";
const KRAKEN_REST_URL: &str = "https://api.kraken.com";

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

/// WebSocket manager for Kraken
pub struct KrakenWebSocket {
    cache: Arc<OrderBookCache>,
    is_running: Arc<AtomicBool>,
    messages_received: Arc<AtomicU64>,
    shutdown_tx: Option<mpsc::Sender<()>>,
    max_pairs: usize,
    orderbook_depth: usize,
}

impl KrakenWebSocket {
    pub fn new(cache: Arc<OrderBookCache>) -> Self {
        Self {
            cache,
            is_running: Arc::new(AtomicBool::new(false)),
            messages_received: Arc::new(AtomicU64::new(0)),
            shutdown_tx: None,
            max_pairs: 200, // Default
            orderbook_depth: 25, // Default
        }
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

    /// Initialize by fetching trading pairs from REST API - LIMITED BY max_pairs
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
        
        // Step 4: Register only top pairs
        for pair in &top_pairs {
            self.cache.register_pair(PairInfo {
                pair_name: pair.pair_name.clone(),
                base: pair.base.clone(),
                quote: pair.quote.clone(),
                kraken_id: pair.kraken_id.clone(),
                ws_name: pair.ws_name.clone(),
                volume_24h: pair.volume_24h,
            });
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

    /// Start WebSocket connection and subscribe to order books
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
        
        info!("Subscribing to {} pairs via WebSocket", pairs_to_subscribe.len());
        
        let cache = Arc::clone(&self.cache);
        let is_running = Arc::clone(&self.is_running);
        let messages_received = Arc::clone(&self.messages_received);
        
        // Get ws_names for subscription
        let ws_names: Vec<String> = pairs_to_subscribe
            .iter()
            .filter_map(|p| self.cache.get_pair_info(p).map(|i| i.ws_name))
            .collect();
        
        // Build pair name lookup from ws_name
        let ws_to_pair: std::collections::HashMap<String, String> = pairs_to_subscribe
            .iter()
            .filter_map(|p| {
                self.cache.get_pair_info(p).map(|i| (i.ws_name.clone(), p.clone()))
            })
            .collect();
        
        // Spawn WebSocket task
        let ws_depth = self.orderbook_depth;
        tokio::spawn(async move {
            is_running.store(true, Ordering::SeqCst);
            
            loop {
                match Self::run_websocket(
                    &cache,
                    &ws_names,
                    &ws_to_pair,
                    &is_running,
                    &messages_received,
                    &mut shutdown_rx,
                    ws_depth,
                ).await {
                    Ok(_) => {
                        if !is_running.load(Ordering::SeqCst) {
                            break;
                        }
                        warn!("WebSocket disconnected, reconnecting in 5s...");
                    }
                    Err(e) => {
                        error!("WebSocket error: {}", e);
                    }
                }
                
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
            
            info!("WebSocket task stopped");
        });
        
        Ok(())
    }

    /// Main WebSocket loop
    async fn run_websocket(
        cache: &Arc<OrderBookCache>,
        ws_names: &[String],
        ws_to_pair: &std::collections::HashMap<String, String>,
        is_running: &Arc<AtomicBool>,
        messages_received: &Arc<AtomicU64>,
        shutdown_rx: &mut mpsc::Receiver<()>,
        depth: usize,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (ws_stream, _) = connect_async(KRAKEN_WS_URL).await?;
        let (mut write, mut read) = ws_stream.split();
        
        info!("WebSocket connected to Kraken");
        
        // Subscribe to order book depth (configurable)
        let depth_name = format!("book-{}", depth);
        for chunk in ws_names.chunks(50) {
            let subscribe_msg = json!({
                "event": "subscribe",
                "pair": chunk,
                "subscription": {
                    "name": "book",
                    "depth": depth
                }
            });
            
            write.send(Message::Text(subscribe_msg.to_string())).await?;
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
        
        info!("Subscribed to order books with depth={}", depth);
        
        // Also subscribe to ticker for volume updates
        for chunk in ws_names.chunks(50) {
            let subscribe_msg = json!({
                "event": "subscribe",
                "pair": chunk,
                "subscription": {
                    "name": "ticker"
                }
            });
            
            write.send(Message::Text(subscribe_msg.to_string())).await?;
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
        
        info!("Subscribed to order books and tickers");
        
        // Message loop
        loop {
            tokio::select! {
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            messages_received.fetch_add(1, Ordering::Relaxed);
                            Self::handle_message(cache, ws_to_pair, &text);
                        }
                        Some(Ok(Message::Ping(data))) => {
                            let _ = write.send(Message::Pong(data)).await;
                        }
                        Some(Ok(Message::Close(_))) => {
                            warn!("WebSocket closed by server");
                            break;
                        }
                        Some(Err(e)) => {
                            error!("WebSocket error: {}", e);
                            break;
                        }
                        None => {
                            warn!("WebSocket stream ended");
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

    /// Handle incoming WebSocket message
    fn handle_message(
        cache: &Arc<OrderBookCache>,
        ws_to_pair: &std::collections::HashMap<String, String>,
        text: &str,
    ) {
        let value: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => return,
        };
        
        // Handle system messages
        if let Some(event) = value.get("event").and_then(|e| e.as_str()) {
            match event {
                "heartbeat" => return,
                "systemStatus" => {
                    debug!("System status: {:?}", value.get("status"));
                    return;
                }
                "subscriptionStatus" => {
                    if let Some(status) = value.get("status").and_then(|s| s.as_str()) {
                        if status == "error" {
                            warn!("Subscription error: {:?}", value.get("errorMessage"));
                        }
                    }
                    return;
                }
                _ => return,
            }
        }
        
        // Handle data messages (array format)
        if let Some(arr) = value.as_array() {
            if arr.len() < 4 {
                return;
            }
            
            let ws_pair = arr.last().and_then(|v| v.as_str()).unwrap_or("");
            let pair_name = match ws_to_pair.get(ws_pair) {
                Some(p) => p,
                None => return,
            };
            
            // Check message type - support all depth formats
            let msg_type = arr.get(arr.len() - 2).and_then(|v| v.as_str()).unwrap_or("");
            
            // Match book-N for any depth (book-10, book-25, book-100, book-500, book-1000)
            if msg_type.starts_with("book-") {
                Self::handle_book_message(cache, pair_name, &arr[1]);
            } else if msg_type == "ticker" {
                Self::handle_ticker_message(cache, pair_name, &arr[1]);
            }
        }
    }

    /// Handle order book message
    fn handle_book_message(cache: &Arc<OrderBookCache>, pair: &str, data: &Value) {
        let is_snapshot = data.get("bs").is_some() || data.get("as").is_some();
        
        if is_snapshot {
            let bids = Self::parse_levels(data.get("bs"));
            let asks = Self::parse_levels(data.get("as"));
            cache.update_snapshot(pair, bids, asks, 0);
        } else {
            let bids = Self::parse_levels(data.get("b"));
            let asks = Self::parse_levels(data.get("a"));
            
            if let Some(arr) = data.as_array() {
                for item in arr {
                    let bids = Self::parse_levels(item.get("b"));
                    let asks = Self::parse_levels(item.get("a"));
                    cache.update_incremental(pair, bids, asks, 0);
                }
            } else {
                cache.update_incremental(pair, bids, asks, 0);
            }
        }
    }

    /// Parse order book levels from JSON
    fn parse_levels(value: Option<&Value>) -> Vec<OrderBookLevel> {
        value
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|level| {
                        let parts = level.as_array()?;
                        let price = parts.get(0)?.as_str()?.parse::<f64>().ok()?;
                        let qty = parts.get(1)?.as_str()?.parse::<f64>().ok()?;
                        Some(OrderBookLevel { price, qty })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Handle ticker message
    fn handle_ticker_message(cache: &Arc<OrderBookCache>, pair: &str, data: &Value) {
        let bid = data.get("b")
            .and_then(|b| b.get(0))
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        
        let ask = data.get("a")
            .and_then(|a| a.get(0))
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        
        let volume = data.get("v")
            .and_then(|v| v.get(1))
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        
        cache.update_price_ticker(pair, bid, ask, volume);
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
        
        if s.len() == 4 && (s.starts_with('X') || s.starts_with('Z')) {
            let suffix = &s[1..];
            if matches!(suffix, "ETH" | "XBT" | "EUR" | "USD" | "GBP" | "JPY" | "CAD" | "AUD") {
                return suffix.to_string();
            }
        }
        
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
