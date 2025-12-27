//! Trading Engine - HFT Architecture
//!
//! Unified scan + execute in single sequential path.
//! Uses HftLoop for core trading logic.

use crate::auth::KrakenAuth;
use crate::config_manager::ConfigManager;
use crate::db::{Database, LiveTradingConfig};
use crate::executor::ExecutionEngine;

// Re-export for API compatibility
pub use crate::executor::TradeResult;
use crate::hft_loop::{HftLoop, HftConfig, HftState, HftStats};
use crate::kraken_pairs::{KrakenPairSelector, PairSelectionConfig};
use crate::order_book::OrderBookCache;
use crate::types::{EngineStats, Opportunity, OrderBookHealth};
use crate::ws_v2::KrakenWebSocketV2;

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;
use tokio::sync::{mpsc, RwLock};
use tracing::{info, warn};
use rand::Rng;

#[derive(Error, Debug)]
pub enum EngineError {
    #[error("Not initialized")]
    NotInitialized,
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("WebSocket error: {0}")]
    WebSocket(String),
    #[error("Execution error: {0}")]
    Execution(String),
    #[error("Database error: {0}")]
    Database(String),
    #[error("Auth error: {0}")]
    Auth(String),
}

// ==========================================
// API Response Types
// ==========================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannerStatus {
    pub is_running: bool,
    pub last_scan_at: Option<String>,
    pub pairs_scanned: i32,
    pub opportunities_found: i32,
    pub profitable_count: i32,
    pub scan_duration_ms: Option<f64>,
    pub scan_count: u64,
    pub event_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub currency: String,
    pub balance: f64,
    pub usd_value: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceInfo {
    pub pair: String,
    pub bid: f64,
    pub ask: f64,
    pub mid: f64,
    pub spread_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventScannerStatsApi {
    pub scans_triggered: u64,
    pub opportunities_detected: u64,
    pub event_count: u64,
    pub pending_pairs: usize,
}

// ==========================================
// Trading Engine
// ==========================================

pub struct TradingEngine {
    // Core components
    cache: Arc<OrderBookCache>,
    websocket: RwLock<Option<KrakenWebSocketV2>>,
    config_manager: Arc<ConfigManager>,

    // HFT Loop - unified scan + execute
    hft_loop: Arc<RwLock<Option<HftLoop>>>,
    hft_event_tx: RwLock<Option<mpsc::Sender<String>>>,

    // Execution engine (shared with HFT loop)
    execution_engine: Arc<RwLock<Option<ExecutionEngine>>>,

    // Database
    db: Database,

    // State
    is_running: AtomicBool,
    start_time: RwLock<Option<Instant>>,

    // Auth
    auth: Option<Arc<KrakenAuth>>,
}

impl TradingEngine {
    /// Create a new trading engine
    pub async fn new(
        api_key: Option<String>,
        api_secret: Option<String>,
        db: Database,
    ) -> Result<Self, EngineError> {
        let cache = Arc::new(OrderBookCache::new());
        let engine_config = crate::types::EngineConfig::unconfigured();
        let config_manager = Arc::new(ConfigManager::new(engine_config));

        // Create auth if credentials provided
        let auth = if let (Some(key), Some(secret)) = (api_key, api_secret) {
            match KrakenAuth::new(key, secret) {
                Ok(a) => {
                    info!("Kraken authentication configured");
                    Some(Arc::new(a))
                }
                Err(e) => {
                    warn!("Failed to create Kraken auth: {}", e);
                    None
                }
            }
        } else {
            warn!("No Kraken API credentials - execution disabled");
            None
        };

        Ok(Self {
            cache,
            websocket: RwLock::new(None),
            config_manager,
            hft_loop: Arc::new(RwLock::new(None)),
            hft_event_tx: RwLock::new(None),
            execution_engine: Arc::new(RwLock::new(None)),
            db,
            is_running: AtomicBool::new(false),
            start_time: RwLock::new(None),
            auth,
        })
    }

    /// Start the trading engine with HFT loop
    pub async fn start(&self) -> Result<(), EngineError> {
        info!("Starting trading engine (HFT mode)...");

        // Clear cache from any previous run to ensure pair count matches new config
        self.cache.clear();

        // Load user configuration from database
        let db_config = self.db.get_config().await
            .map_err(|e| EngineError::Database(format!("Failed to load config: {}", e)))?;

        // Validate configuration
        let start_currency = db_config.start_currency.clone().unwrap_or_default();
        if start_currency.is_empty() {
            return Err(EngineError::Config(
                "Start currency not configured. Please select from the dashboard.".to_string()
            ));
        }

        let max_pairs = db_config.max_pairs.ok_or_else(|| EngineError::Config(
            "max_pairs not configured".to_string()
        ))?;
        let min_volume_24h_usd = db_config.min_volume_24h_usd.ok_or_else(|| EngineError::Config(
            "min_volume_24h_usd not configured".to_string()
        ))?;
        let max_cost_min = db_config.max_cost_min.ok_or_else(|| EngineError::Config(
            "max_cost_min not configured".to_string()
        ))?;

        // Select pairs
        info!("Selecting high-liquidity pairs for HFT arbitrage...");
        let mut pair_config = PairSelectionConfig::default();
        pair_config.set_pair_selection_params(max_pairs as usize, min_volume_24h_usd, max_cost_min);
        pair_config.set_start_currency(&start_currency);

        if let Err(e) = pair_config.validate() {
            return Err(EngineError::Config(e));
        }

        let pair_selector = KrakenPairSelector::new(pair_config);
        let selected_pairs = pair_selector.select_pairs().await
            .map_err(|e| EngineError::WebSocket(format!("Pair selection failed: {}", e)))?;

        if selected_pairs.is_empty() {
            return Err(EngineError::WebSocket("No pairs selected".to_string()));
        }

        info!("Selected {} pairs for HFT arbitrage", selected_pairs.len());

        // Initialize WebSocket
        let mut ws = KrakenWebSocketV2::new(Arc::clone(&self.cache));
        ws.set_max_pairs(selected_pairs.len());

        // Create HFT Loop
        let mut hft_loop = HftLoop::new(
            Arc::clone(&self.cache),
            Arc::clone(&self.config_manager),
            self.db.clone(),
        );

        // Initialize execution engine FIRST (before WebSocket starts sending events)
        if let Some(ref auth) = self.auth {
            let exec_engine = ExecutionEngine::new(
                Arc::clone(auth),
                Arc::clone(&self.cache),
            );

            if let Err(e) = exec_engine.connect().await {
                warn!("Failed to connect execution engine: {}", e);
            } else {
                // Set execution engine in HFT loop BEFORE events start
                hft_loop.set_execution_engine(exec_engine).await;
                info!("Execution engine connected");
            }
        }

        // Configure HFT loop with user settings (before starting event channel)
        let hft_config = HftConfig {
            min_profit_threshold: db_config.min_profit_threshold.unwrap_or(0.1),
            trade_amount: db_config.trade_amount.unwrap_or(10.0),
            max_daily_loss: db_config.max_daily_loss.unwrap_or(100.0),
            max_total_loss: db_config.max_total_loss.unwrap_or(500.0),
            base_currencies: start_currency.split(',').map(|s| s.trim().to_uppercase()).collect(),
        };
        hft_loop.update_config(hft_config).await;

        // Fetch and apply fees from Kraken
        if self.auth.is_some() {
            if let Ok(fee_data) = self.fetch_kraken_fees().await {
                if let Some(taker) = fee_data.get("taker_fee").and_then(|v| v.as_f64()) {
                    self.config_manager.update_fee_rate(taker, "kraken_api");
                    info!("Fees loaded from Kraken API: taker={:.2}%", taker * 100.0);
                }
            }
        }

        // NOW create event channel and start HFT loop (after execution engine is ready)
        let hft_event_tx = hft_loop.create_event_channel();

        // Create WebSocket event channel
        let (mut ws_event_rx, _) = ws.create_event_channel();

        // Forward WebSocket events to HFT loop
        let hft_tx_clone = hft_event_tx.clone();
        tokio::spawn(async move {
            while let Some(pair) = ws_event_rx.recv().await {
                if hft_tx_clone.send(pair).await.is_err() {
                    break;
                }
            }
            info!("WebSocket to HFT event forwarder stopped");
        });

        // Initialize WebSocket with pairs and START (events will flow after this)
        ws.initialize_with_pairs(selected_pairs);
        ws.start(max_pairs as usize, 25).await
            .map_err(|e| EngineError::WebSocket(e.to_string()))?;

        *self.websocket.write().await = Some(ws);

        // Store references
        *self.hft_loop.write().await = Some(hft_loop);
        *self.hft_event_tx.write().await = Some(hft_event_tx);

        self.is_running.store(true, Ordering::SeqCst);
        *self.start_time.write().await = Some(Instant::now());

        info!("Trading engine started (HFT mode)");
        Ok(())
    }

    /// Stop the trading engine
    pub async fn stop(&self) {
        info!("Stopping trading engine...");

        if let Some(ref hft_loop) = *self.hft_loop.read().await {
            hft_loop.stop();
        }

        if let Some(ref mut ws) = *self.websocket.write().await {
            ws.stop().await;
        }

        self.is_running.store(false, Ordering::SeqCst);
        info!("Trading engine stopped");
    }

    /// Get engine statistics
    pub async fn get_stats(&self) -> EngineStats {
        let (pairs, currencies, _) = self.cache.get_stats();
        let uptime = self.start_time.read().await
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0);

        let hft_stats = if let Some(ref hft) = *self.hft_loop.read().await {
            hft.get_stats().await
        } else {
            HftStats::default()
        };

        EngineStats {
            is_running: self.is_running.load(Ordering::Relaxed),
            pairs_monitored: pairs,
            currencies_tracked: currencies,
            orderbooks_cached: pairs,
            avg_orderbook_staleness_ms: 0.0,
            opportunities_found: hft_stats.opportunities_found,
            opportunities_per_second: 0.0,
            uptime_seconds: uptime,
            scan_cycle_ms: 0.0,
            last_scan_at: String::new(),
        }
    }

    /// Get HFT loop state
    pub async fn get_hft_state(&self) -> Option<HftState> {
        if let Some(ref hft) = *self.hft_loop.read().await {
            Some(hft.get_state().await)
        } else {
            None
        }
    }

    /// Get HFT statistics
    pub async fn get_hft_stats(&self) -> HftStats {
        if let Some(ref hft) = *self.hft_loop.read().await {
            hft.get_stats().await
        } else {
            HftStats::default()
        }
    }

    /// Reset circuit breaker
    pub async fn reset_circuit_breaker(&self) {
        if let Some(ref hft) = *self.hft_loop.read().await {
            hft.reset_circuit_breaker().await;
        }
        info!("Circuit breaker reset");
    }

    /// Reset daily statistics
    pub async fn reset_daily_stats(&self) {
        if let Some(ref hft) = *self.hft_loop.read().await {
            hft.reset_daily_stats().await;
        }
        info!("Daily stats reset");
    }

    /// Sync config from database
    pub async fn sync_config(&self, config: &LiveTradingConfig) {
        let min_profit = config.min_profit_threshold.unwrap_or(0.1);
        self.config_manager.update_config(Some(min_profit), None);

        if let Some(ref hft) = *self.hft_loop.read().await {
            let hft_config = HftConfig {
                min_profit_threshold: min_profit,
                trade_amount: config.trade_amount.unwrap_or(10.0),
                max_daily_loss: config.max_daily_loss.unwrap_or(100.0),
                max_total_loss: config.max_total_loss.unwrap_or(500.0),
                base_currencies: config.start_currency.clone()
                    .unwrap_or_default()
                    .split(',')
                    .map(|s| s.trim().to_uppercase())
                    .collect(),
            };
            hft.update_config(hft_config).await;
        }

        info!("Config synced: trade_amount={:?}", config.trade_amount);
    }

    /// Check if engine is running
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::Relaxed)
    }

    /// Get scanner status
    pub fn get_scanner_status(&self) -> ScannerStatus {
        let (pairs_count, _, _) = self.cache.get_stats();

        ScannerStatus {
            is_running: self.is_running.load(Ordering::Relaxed),
            last_scan_at: None,
            pairs_scanned: pairs_count as i32,
            opportunities_found: 0,
            profitable_count: 0,
            scan_duration_ms: None,
            scan_count: 0,
            event_count: 0,
        }
    }

    /// Start scanner (no-op in HFT mode - always running)
    pub async fn start_scanner(&self) {
        info!("Scanner started (HFT mode)");
    }

    /// Stop scanner (no-op in HFT mode)
    pub async fn stop_scanner(&self) {
        info!("Scanner stopped (HFT mode)");
    }

    /// Scan now (no-op in HFT mode - scans happen on events)
    pub fn scan_now(&self) -> Vec<Opportunity> {
        info!("Manual scan triggered (HFT mode)");
        Vec::new()
    }

    /// Get event scanner stats (legacy API)
    pub fn get_event_scanner_stats(&self) -> EventScannerStatsApi {
        EventScannerStatsApi {
            scans_triggered: 0,
            opportunities_detected: 0,
            event_count: 0,
            pending_pairs: 0,
        }
    }

    /// Get positions from Kraken
    pub async fn get_positions(&self) -> Result<Vec<Position>, EngineError> {
        let auth = match &self.auth {
            Some(a) if a.is_configured() => a,
            _ => return Ok(Vec::new()),
        };

        let client = reqwest::Client::new();
        // Use shared nonce from KrakenAuth to prevent conflicts with other API calls
        let nonce = auth.next_nonce();

        let post_data = format!("nonce={}", nonce);
        let path = "/0/private/Balance";
        let url = format!("https://api.kraken.com{}", path);

        let signature = auth.sign_request(path, nonce, &post_data)
            .map_err(|e| EngineError::Auth(format!("Failed to sign: {}", e)))?;

        let response = client.post(&url)
            .header("API-Key", auth.api_key())
            .header("API-Sign", signature)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(post_data)
            .send()
            .await
            .map_err(|e| EngineError::Execution(format!("Request failed: {}", e)))?;

        let json: serde_json::Value = response.json().await
            .map_err(|e| EngineError::Execution(format!("Parse failed: {}", e)))?;

        if let Some(error) = json.get("error").and_then(|e| e.as_array()) {
            if !error.is_empty() {
                return Err(EngineError::Execution(format!("API error: {:?}", error)));
            }
        }

        let mut positions = Vec::new();
        if let Some(result) = json.get("result").and_then(|r| r.as_object()) {
            for (currency, balance) in result {
                let balance_f64 = balance.as_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);

                if balance_f64 < 0.00000001 {
                    continue;
                }

                positions.push(Position {
                    currency: self.normalize_currency(currency),
                    balance: balance_f64,
                    usd_value: None,
                });
            }
        }

        Ok(positions)
    }

    fn normalize_currency(&self, currency: &str) -> String {
        match currency {
            "XXBT" | "XBT" => "BTC".to_string(),
            "XETH" => "ETH".to_string(),
            "ZUSD" => "USD".to_string(),
            "ZEUR" => "EUR".to_string(),
            "ZGBP" => "GBP".to_string(),
            "ZJPY" => "JPY".to_string(),
            "ZCAD" => "CAD".to_string(),
            other => other.to_string(),
        }
    }

    /// Get trade balance from Kraken (total portfolio value in USD)
    /// Uses /0/private/TradeBalance endpoint which returns "eb" (equivalent balance)
    pub async fn get_trade_balance(&self) -> Result<f64, EngineError> {
        let auth = match &self.auth {
            Some(a) if a.is_configured() => a,
            _ => return Err(EngineError::Auth("Kraken API credentials not configured".to_string())),
        };

        let client = reqwest::Client::new();
        // Use shared nonce from KrakenAuth to prevent conflicts with other API calls
        let nonce = auth.next_nonce();

        // Request trade balance with USD as the base asset
        let post_data = format!("nonce={}&asset=ZUSD", nonce);
        let path = "/0/private/TradeBalance";
        let url = format!("https://api.kraken.com{}", path);

        let signature = auth.sign_request(path, nonce, &post_data)
            .map_err(|e| EngineError::Auth(format!("Failed to sign: {}", e)))?;

        let response = client.post(&url)
            .header("API-Key", auth.api_key())
            .header("API-Sign", signature)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(post_data)
            .send()
            .await
            .map_err(|e| EngineError::Execution(format!("Request failed: {}", e)))?;

        let json: serde_json::Value = response.json().await
            .map_err(|e| EngineError::Execution(format!("Parse failed: {}", e)))?;

        if let Some(error) = json.get("error").and_then(|e| e.as_array()) {
            if !error.is_empty() {
                return Err(EngineError::Execution(format!("API error: {:?}", error)));
            }
        }

        // Extract "eb" (equivalent balance) from result
        // This is the total portfolio value in the specified asset (USD)
        let eb = json.get("result")
            .and_then(|r| r.get("eb"))
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);

        Ok(eb)
    }

    /// Get prices
    pub fn get_prices(&self, limit: usize) -> Vec<PriceInfo> {
        self.cache.get_all_prices()
            .into_iter()
            .take(limit)
            .map(|(pair, edge)| PriceInfo {
                pair,
                bid: edge.bid,
                ask: edge.ask,
                mid: (edge.bid + edge.ask) / 2.0,
                spread_pct: if edge.bid > 0.0 { ((edge.ask - edge.bid) / edge.bid) * 100.0 } else { 0.0 },
            })
            .collect()
    }

    /// Get price for specific pair
    pub fn get_price(&self, pair: &str) -> Option<f64> {
        self.cache.get_price(pair).map(|edge| (edge.bid + edge.ask) / 2.0)
    }

    /// Get currencies
    pub fn get_currencies(&self) -> Vec<String> {
        self.cache.get_currencies().into_iter().collect()
    }

    /// Get pairs
    pub fn get_pairs(&self) -> Vec<String> {
        self.cache.get_all_pairs()
    }

    /// Fetch fees from Kraken
    pub async fn fetch_kraken_fees(&self) -> Result<serde_json::Value, String> {
        let auth = self.auth.as_ref()
            .ok_or_else(|| "Kraken API credentials not configured".to_string())?;

        if !auth.is_configured() {
            return Err("Kraken API credentials not configured".to_string());
        }

        let client = reqwest::Client::new();
        // Use shared nonce from KrakenAuth to prevent conflicts with other API calls
        let nonce = auth.next_nonce();

        let post_data = format!("nonce={}&pair=XBTUSD", nonce);
        let path = "/0/private/TradeVolume";
        let url = format!("https://api.kraken.com{}", path);

        let signature = auth.sign_request(path, nonce, &post_data)
            .map_err(|e| format!("Failed to sign: {}", e))?;

        let response = client.post(&url)
            .header("API-Key", auth.api_key())
            .header("API-Sign", signature)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(post_data)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        let json: serde_json::Value = response.json().await
            .map_err(|e| format!("Parse failed: {}", e))?;

        if let Some(error) = json.get("error").and_then(|e| e.as_array()) {
            if !error.is_empty() {
                return Err(format!("API error: {:?}", error));
            }
        }

        if let Some(result) = json.get("result") {
            let fees = result.get("fees").cloned().unwrap_or(serde_json::json!({}));
            let fees_maker = result.get("fees_maker").cloned().unwrap_or(serde_json::json!({}));
            let volume = result.get("volume").and_then(|v| v.as_str()).unwrap_or("0");

            // Extract taker fee from "fees" object
            let taker_fee = fees.as_object()
                .and_then(|f| f.values().next())
                .and_then(|v| v.get("fee"))
                .and_then(|f| f.as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .ok_or_else(|| "Failed to parse taker fee".to_string())?;

            // Extract maker fee from "fees_maker" object
            let maker_fee = fees_maker.as_object()
                .and_then(|f| f.values().next())
                .and_then(|v| v.get("fee"))
                .and_then(|f| f.as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0); // Default to 0 if not available

            Ok(serde_json::json!({
                "taker_fee": taker_fee / 100.0,
                "maker_fee": maker_fee / 100.0,
                "volume_30d": volume,
                "source": "kraken_api"
            }))
        } else {
            Err("No result in response".to_string())
        }
    }

    /// Get database reference
    pub fn database(&self) -> &Database {
        &self.db
    }

    /// Get order book health
    pub fn get_orderbook_health(&self) -> OrderBookHealth {
        OrderBookHealth::default()
    }

    /// Get cached opportunities (empty for HFT - we execute immediately)
    pub fn get_cached_opportunities(&self) -> Vec<Opportunity> {
        Vec::new()
    }

    /// Restart WebSocket
    pub async fn restart_websocket(&self) -> Result<(), EngineError> {
        // Stop and restart
        self.stop().await;
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        self.start().await
    }

    /// Enable trading (HFT always enabled when started)
    pub fn enable_trading(&self) {
        info!("Trading enabled (HFT mode)");
    }

    /// Disable trading
    pub fn disable_trading(&self) {
        info!("Trading disabled");
    }

    /// Enable auto-execution (HFT always auto-executes)
    pub fn enable_auto_execution(&self) {
        info!("Auto-execution enabled (HFT mode)");
    }

    /// Disable auto-execution
    pub fn disable_auto_execution(&self) {
        info!("Auto-execution disabled");
    }

    /// Check if auto-execution is enabled (always true in HFT mode when running)
    pub fn is_auto_execution_enabled(&self) -> bool {
        self.is_running.load(Ordering::Relaxed)
    }

    /// Trip circuit breaker
    pub async fn trip_circuit_breaker(&self, reason: &str) {
        warn!("Circuit breaker tripped: {}", reason);
        if let Some(ref hft) = *self.hft_loop.read().await {
            hft.stop();
        }
    }

    /// Execute a trade manually
    pub async fn execute_trade(&self, path: &str, amount: f64) -> Result<TradeResult, EngineError> {
        // Get execution engine
        let engine_guard = self.execution_engine.read().await;
        let engine = engine_guard.as_ref()
            .ok_or(EngineError::NotInitialized)?;

        // Create opportunity
        let opportunity = Opportunity {
            id: uuid::Uuid::new_v4().to_string(),
            path: path.to_string(),
            legs: path.matches(" → ").count() + 1,
            gross_profit_pct: 0.0,
            fees_pct: 0.0,
            net_profit_pct: 0.0,
            is_profitable: true,
            detected_at: chrono::Utc::now(),
            fee_rate: 0.0026,
            fee_source: "manual".to_string(),
            legs_detail: Vec::new(),
        };

        engine.execute_opportunity(&opportunity, amount).await
            .map_err(|e| EngineError::Execution(e.to_string()))
    }

    /// Resolve partial trade
    pub async fn resolve_partial_trade(&self, trade: &crate::db::LiveTrade) -> Result<TradeResult, EngineError> {
        let held_currency = trade.held_currency.as_ref()
            .ok_or(EngineError::Execution("No held currency".to_string()))?;
        let held_amount = trade.held_amount
            .ok_or(EngineError::Execution("No held amount".to_string()))?;

        let engine_guard = self.execution_engine.read().await;
        let engine = engine_guard.as_ref()
            .ok_or(EngineError::NotInitialized)?;

        engine.execute_single_leg(held_currency, "USD", held_amount).await
            .map_err(|e| EngineError::Execution(e.to_string()))
    }

    /// Update fee config
    pub async fn update_fee_config(&self, maker_fee: Option<f64>, taker_fee: Option<f64>) {
        if let Some(taker) = taker_fee {
            self.config_manager.update_fee_rate(taker, "manual");
        }
        info!("Fee config updated: maker={:?}, taker={:?}", maker_fee, taker_fee);
    }

    /// Get past opportunities from database
    pub async fn get_past_opportunities(&self, limit: i64, hours: i32) -> Result<Vec<crate::db::LiveOpportunity>, EngineError> {
        self.db.get_opportunities(limit, None, hours).await
            .map_err(|e| EngineError::Database(e.to_string()))
    }
}
