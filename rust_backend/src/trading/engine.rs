//! Trading Engine - Async-first design
//!
//! Integrates all trading components with proper async support.

use crate::auth::KrakenAuth;
use crate::config_manager::ConfigManager;
use crate::db::{Database, LiveTradingConfig, LiveTrade};
use crate::event_system::EventDrivenScanner;
use crate::executor::{ExecutionEngine, TradeResult, FeeConfig};
use crate::order_book::OrderBookCache;
use crate::trading_config::TradingGuard;
use crate::types::{EngineConfig, EngineStats, Opportunity, OrderBookHealth, EngineSettings};
use crate::ws_v2::KrakenWebSocketV2;

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

#[derive(Error, Debug)]
pub enum EngineError {
    #[error("Not initialized")]
    NotInitialized,
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
pub struct EventScannerStatsApi {
    pub scans_triggered: u64,
    pub opportunities_detected: u64,
    pub event_count: u64,
    pub pending_pairs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceInfo {
    pub pair: String,
    pub bid: f64,
    pub ask: f64,
    pub mid: f64,
    pub spread_pct: f64,
}

// ==========================================
// Internal Settings
// ==========================================

#[derive(Debug, Clone)]
struct InternalSettings {
    scan_interval_ms: u64,
    max_pairs: usize,
    orderbook_depth: usize,
    scanner_enabled: bool,
}

impl Default for InternalSettings {
    fn default() -> Self {
        Self {
            scan_interval_ms: 5000,
            max_pairs: 300,
            orderbook_depth: 25,
            scanner_enabled: true,
        }
    }
}

// ==========================================
// Trading Engine
// ==========================================

pub struct TradingEngine {
    // Core components
    cache: Arc<OrderBookCache>,
    websocket: RwLock<Option<KrakenWebSocketV2>>,
    event_scanner: Arc<EventDrivenScanner>,
    execution_engine: RwLock<Option<Arc<ExecutionEngine>>>,
    trading_guard: Arc<TradingGuard>,
    config_manager: Arc<ConfigManager>,
    
    // Database
    #[allow(dead_code)]
    db: Database,
    
    // Settings - using tokio RwLock for async
    settings: RwLock<InternalSettings>,
    
    // State
    is_running: AtomicBool,
    start_time: RwLock<Option<Instant>>,
    auto_execution_enabled: AtomicBool,
    
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
        let engine_config = EngineConfig::default();
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
        
        // Create event scanner
        let event_scanner = Arc::new(EventDrivenScanner::new(
            Arc::clone(&cache),
            Arc::clone(&config_manager),
        ));
        
        // Create trading guard
        let trading_guard = Arc::new(TradingGuard::new());
        
        Ok(Self {
            cache,
            websocket: RwLock::new(None),
            event_scanner,
            execution_engine: RwLock::new(None),
            trading_guard,
            config_manager,
            db,
            settings: RwLock::new(InternalSettings::default()),
            is_running: AtomicBool::new(false),
            start_time: RwLock::new(None),
            auto_execution_enabled: AtomicBool::new(false),
            auth,
        })
    }

    /// Start the trading engine
    pub async fn start(&self) -> Result<(), EngineError> {
        info!("Starting trading engine...");
        
        let settings = self.settings.read().await.clone();
        
        // Initialize WebSocket
        let mut ws = KrakenWebSocketV2::new(Arc::clone(&self.cache));
        ws.set_max_pairs(settings.max_pairs);
        
        // Create event channel and connect to scanner
        let (mut event_rx, _event_stats) = ws.create_event_channel();
        let event_scanner = Arc::clone(&self.event_scanner);
        
        // Spawn task to receive orderbook events and trigger scans
        tokio::spawn(async move {
            while let Some(pair) = event_rx.recv().await {
                event_scanner.on_orderbook_update(&pair);
            }
            info!("Event channel closed");
        });
        
        // Initialize (fetch pairs)
        ws.initialize().await
            .map_err(|e| EngineError::WebSocket(e.to_string()))?;
        
        // Fetch initial prices
        ws.fetch_initial_prices().await
            .map_err(|e| EngineError::WebSocket(e.to_string()))?;
        
        // Start WebSocket streaming
        ws.start(settings.max_pairs, settings.orderbook_depth).await
            .map_err(|e| EngineError::WebSocket(e.to_string()))?;
        
        *self.websocket.write().await = Some(ws);
        
        // Initialize execution engine if authenticated
        if let Some(ref auth) = self.auth {
            let exec_engine = ExecutionEngine::new(
                Arc::clone(auth),
                Arc::clone(&self.cache),
            );
            
            // Connect to private WebSocket
            if let Err(e) = exec_engine.connect().await {
                warn!("Failed to connect execution engine: {}", e);
            }
            
            *self.execution_engine.write().await = Some(Arc::new(exec_engine));
            info!("Execution engine initialized");
        }
        
        // Initialize event scanner graph
        self.event_scanner.initialize_graph();
        
        // Auto-fetch fees from Kraken API and apply them
        if self.auth.is_some() {
            match self.fetch_kraken_fees().await {
                Ok(fee_data) => {
                    if let (Some(taker), Some(maker)) = (
                        fee_data.get("taker_fee").and_then(|v| v.as_f64()),
                        fee_data.get("maker_fee").and_then(|v| v.as_f64())
                    ) {
                        // Update the scanner's fee rate (uses taker fee for market orders)
                        self.config_manager.update_fee_rate(taker, "kraken_api");
                        
                        // Update the executor's fee config
                        if let Some(ref exec_engine) = *self.execution_engine.read().await {
                            exec_engine.update_fee_config(maker, taker);
                        }
                        
                        let volume = fee_data.get("volume_30d")
                            .and_then(|v| v.as_str())
                            .unwrap_or("0");
                        info!(
                            "✅ Fees loaded from Kraken API: taker={:.2}%, maker={:.2}%, 30d_volume={}",
                            taker * 100.0, maker * 100.0, volume
                        );
                    }
                }
                Err(e) => {
                    warn!("⚠️ Failed to fetch fees from Kraken, using defaults (0.26%): {}", e);
                }
            }
        }
        
        // Create opportunity save channel and spawn saver task
        let mut opp_rx = self.event_scanner.create_opportunity_save_channel();
        let db = self.db.clone();
        tokio::spawn(async move {
            use crate::db::NewLiveOpportunity;
            
            while let Some(opp) = opp_rx.recv().await {
                let new_opp = NewLiveOpportunity {
                    path: opp.path.clone(),
                    legs: opp.legs as i32,
                    expected_profit_pct: opp.net_profit_pct,
                    expected_profit_usd: None,
                    trade_amount: None,
                    status: "DETECTED".to_string(),
                    status_reason: None,
                    pairs_scanned: None,
                    paths_found: None,
                };
                
                if let Err(e) = db.save_opportunity(&new_opp).await {
                    debug!("Failed to save opportunity to DB: {}", e);
                }
            }
            info!("Opportunity save channel closed");
        });
        
        // Create auto-execution channel and spawn executor task
        let mut auto_exec_rx = self.event_scanner.create_auto_exec_channel();
        let execution_engine_lock = self.execution_engine.read().await;
        let execution_engine = execution_engine_lock.as_ref().map(Arc::clone);
        drop(execution_engine_lock);
        let trading_guard = Arc::clone(&self.trading_guard);
        let event_scanner = Arc::clone(&self.event_scanner);  // Use event_scanner's flag
        let db_for_trades = self.db.clone();
        
        tokio::spawn(async move {
            // Track last execution time and path to prevent rapid retries
            let mut last_execution_time: Option<std::time::Instant> = None;
            let mut last_executed_path: Option<String> = None;
            const MIN_EXECUTION_INTERVAL_MS: u64 = 2000; // 2 second cooldown between executions
            const PATH_COOLDOWN_MS: u64 = 10000; // 10 second cooldown for same path
            
            while let Some(opp) = auto_exec_rx.recv().await {
                // Check if auto-execution is still enabled (via event_scanner's flag)
                if !event_scanner.is_auto_execution_enabled() {
                    continue;
                }
                
                // Check cooldown - prevent rapid-fire executions
                if let Some(last_time) = last_execution_time {
                    let elapsed = last_time.elapsed().as_millis() as u64;
                    if elapsed < MIN_EXECUTION_INTERVAL_MS {
                        debug!("Cooldown active: {}ms remaining", MIN_EXECUTION_INTERVAL_MS - elapsed);
                        continue;
                    }
                    
                    // Extra cooldown for same path
                    if let Some(ref last_path) = last_executed_path {
                        if last_path == &opp.path && elapsed < PATH_COOLDOWN_MS {
                            debug!("Path cooldown active for {}: {}ms remaining", opp.path, PATH_COOLDOWN_MS - elapsed);
                            continue;
                        }
                    }
                }
                
                // Check trading guard
                if !trading_guard.is_enabled() {
                    debug!("Trading not enabled, skipping auto-execution");
                    continue;
                }
                
                // Check profit threshold
                let config = trading_guard.get_config();
                if opp.net_profit_pct < config.min_profit_threshold {
                    debug!("Below profit threshold: {:.3}% < {:.3}%", opp.net_profit_pct, config.min_profit_threshold);
                    continue;
                }
                
                // Check all guards
                let guard_result = trading_guard.check_opportunity(&opp.path, opp.net_profit_pct);
                if !guard_result.can_trade {
                    debug!("Guard blocked: {:?}", guard_result.reason);
                    continue;
                }
                
                // Try to start execution (prevents concurrent executions)
                if !trading_guard.try_start_execution() {
                    debug!("Another trade already executing");
                    continue;
                }
                
                // Update execution tracking BEFORE executing
                last_execution_time = Some(std::time::Instant::now());
                last_executed_path = Some(opp.path.clone());
                
                info!("🚀 Auto-executing: {} | Expected profit: {:.3}%", opp.path, opp.net_profit_pct);
                
                // Get execution engine
                let exec_result = match &execution_engine {
                    Some(engine) => {
                        engine.execute_opportunity(&opp, config.trade_amount).await
                    }
                    None => {
                        warn!("Execution engine not available");
                        trading_guard.finish_execution();
                        continue;
                    }
                };
                
                // Finish execution
                trading_guard.finish_execution();
                
                // Process result
                match exec_result {
                    Ok(trade_result) => {
                        if trade_result.success {
                            info!(
                                "💰 Auto-execution SUCCESS: {} | Profit: ${:.4} ({:.3}%)",
                                opp.path, trade_result.profit_amount, trade_result.profit_pct
                            );
                        } else {
                            warn!(
                                "❌ Auto-execution FAILED: {} | Error: {:?}",
                                opp.path, trade_result.error
                            );
                        }
                        
                        // Save trade to database
                        let new_trade = crate::db::NewLiveTrade {
                            trade_id: trade_result.id.clone(),
                            path: trade_result.path.clone(),
                            legs: trade_result.legs.len() as i32,
                            amount_in: trade_result.start_amount,
                            amount_out: Some(trade_result.end_amount),
                            profit_loss: Some(trade_result.profit_amount),
                            profit_loss_pct: Some(trade_result.profit_pct),
                            status: if trade_result.success { "COMPLETED".to_string() } else { "FAILED".to_string() },
                            current_leg: None,
                            error_message: trade_result.error.clone(),
                            held_currency: None,
                            held_amount: None,
                            held_value_usd: None,
                            order_ids: Some(serde_json::json!(trade_result.legs.iter().map(|l| &l.order_id).collect::<Vec<_>>())),
                            leg_fills: Some(serde_json::to_value(&trade_result.legs).unwrap_or_default()),
                            started_at: Some(trade_result.executed_at),
                            completed_at: Some(chrono::Utc::now()),
                            total_execution_ms: Some(trade_result.total_duration_ms as f64),
                            opportunity_profit_pct: Some(opp.net_profit_pct),
                        };
                        
                        if let Err(e) = db_for_trades.save_trade(&new_trade).await {
                            warn!("Failed to save trade to DB: {}", e);
                        }
                    }
                    Err(e) => {
                        warn!("❌ Auto-execution ERROR: {} | {}", opp.path, e);
                    }
                }
            }
            info!("Auto-execution channel closed");
        });
        
        self.is_running.store(true, Ordering::SeqCst);
        *self.start_time.write().await = Some(Instant::now());
        
        info!("Trading engine started successfully");
        Ok(())
    }

    /// Get engine statistics
    pub async fn get_stats(&self) -> EngineStats {
        let (pairs, currencies, _avg_staleness) = self.cache.get_stats();
        let uptime = self.start_time.read().await
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0);
        
        let scanner_stats = self.event_scanner.get_stats();
        
        EngineStats {
            is_running: self.is_running.load(Ordering::Relaxed),
            pairs_monitored: pairs,
            currencies_tracked: currencies,
            orderbooks_cached: pairs,
            avg_orderbook_staleness_ms: 0.0,
            opportunities_found: scanner_stats.opportunities_found,
            opportunities_per_second: 0.0,
            uptime_seconds: uptime,
            scan_cycle_ms: 0.0,
            last_scan_at: String::new(),
        }
    }

    /// Get engine settings
    pub async fn get_settings(&self) -> EngineSettings {
        let s = self.settings.read().await;
        EngineSettings {
            scan_interval_ms: s.scan_interval_ms,
            max_pairs: s.max_pairs,
            orderbook_depth: s.orderbook_depth,
            scanner_enabled: s.scanner_enabled,
        }
    }

    /// Update engine settings
    pub async fn update_settings(
        &self,
        scan_interval_ms: Option<u64>,
        max_pairs: Option<usize>,
        orderbook_depth: Option<usize>,
        scanner_enabled: Option<bool>,
    ) {
        let mut settings = self.settings.write().await;
        if let Some(v) = scan_interval_ms { settings.scan_interval_ms = v; }
        if let Some(v) = max_pairs { settings.max_pairs = v; }
        if let Some(v) = orderbook_depth { settings.orderbook_depth = v; }
        if let Some(v) = scanner_enabled { settings.scanner_enabled = v; }
    }

    /// Sync config from database
    pub fn sync_config(&self, config: &LiveTradingConfig) {
        // Note: Fee rate is now loaded from Kraken API at startup
        // Only update profit threshold here
        self.config_manager.update_config(
            Some(config.min_profit_threshold),
            None,
        );
        info!("Config synced: enabled={}, amount={}", config.is_enabled, config.trade_amount);
    }

    /// Enable trading
    pub fn enable_trading(&self) {
        self.trading_guard.enable();
        info!("Trading ENABLED");
    }

    /// Disable trading
    pub fn disable_trading(&self) {
        self.trading_guard.disable("Manual disable");
        info!("Trading DISABLED");
    }

    /// Enable auto-execution
    pub fn enable_auto_execution(&self) {
        self.auto_execution_enabled.store(true, Ordering::SeqCst);
        self.event_scanner.enable_auto_execution();
        info!("Auto-execution ENABLED");
    }

    /// Disable auto-execution
    pub fn disable_auto_execution(&self) {
        self.auto_execution_enabled.store(false, Ordering::SeqCst);
        self.event_scanner.disable_auto_execution();
        info!("Auto-execution DISABLED");
    }

    /// Check if auto-execution is enabled
    pub fn is_auto_execution_enabled(&self) -> bool {
        self.auto_execution_enabled.load(Ordering::Relaxed)
    }

    /// Trip circuit breaker
    pub fn trip_circuit_breaker(&self, reason: &str) {
        self.trading_guard.trip_circuit_breaker(reason);
        warn!("Circuit breaker tripped: {}", reason);
    }

    /// Reset circuit breaker
    pub fn reset_circuit_breaker(&self) {
        self.trading_guard.reset_circuit_breaker();
        info!("Circuit breaker reset");
    }

    /// Reset daily stats
    pub fn reset_daily_stats(&self) {
        self.trading_guard.reset_daily();
        info!("Daily stats reset");
    }

    /// Execute a trade
    pub async fn execute_trade(&self, path: &str, amount: f64) -> Result<TradeResult, EngineError> {
        // Get engine - clone Arc to avoid holding lock across await
        let engine = {
            let guard = self.execution_engine.read().await;
            guard.as_ref()
                .ok_or(EngineError::NotInitialized)?
                .clone()
        };
        
        // Get current fee rate from config manager
        let config = self.config_manager.get_config();
        
        // Create opportunity from path
        let opportunity = Opportunity {
            id: uuid::Uuid::new_v4().to_string(),
            path: path.to_string(),
            legs: path.matches(" → ").count() + 1,
            gross_profit_pct: 0.0,
            fees_pct: 0.0,
            net_profit_pct: 0.0,
            is_profitable: true,
            detected_at: chrono::Utc::now(),
            fee_rate: config.fee_rate,
            fee_source: config.fee_source.clone(),
            legs_detail: Vec::new(),
        };
        
        engine.execute_opportunity(&opportunity, amount).await
            .map_err(|e| EngineError::Execution(e.to_string()))
    }

    /// Resolve a partial trade (sell held currency back to USD)
    pub async fn resolve_partial_trade(&self, trade: &LiveTrade) -> Result<TradeResult, EngineError> {
        let held_currency = trade.held_currency.as_ref()
            .ok_or(EngineError::Execution("No held currency".to_string()))?;
        let held_amount = trade.held_amount
            .ok_or(EngineError::Execution("No held amount".to_string()))?;
        
        info!("Resolving partial trade: selling {:.6} {} to USD", held_amount, held_currency);
        
        // Get execution engine
        let engine = {
            let guard = self.execution_engine.read().await;
            guard.as_ref()
                .ok_or(EngineError::NotInitialized)?
                .clone()
        };
        
        // Execute single leg trade to convert held currency back to USD
        engine.execute_single_leg(held_currency, "USD", held_amount).await
            .map_err(|e| EngineError::Execution(e.to_string()))
    }

    /// Get cached opportunities
    pub fn get_cached_opportunities(&self) -> Vec<Opportunity> {
        self.event_scanner.get_cached_opportunities()
    }

    /// Scan now (manual trigger)
    pub fn scan_now(&self) -> Vec<Opportunity> {
        self.event_scanner.trigger_scan()
    }

    /// Get scanner status
    pub fn get_scanner_status(&self) -> ScannerStatus {
        let stats = self.event_scanner.get_stats();
        let opportunities = self.get_cached_opportunities();
        let profitable_count = opportunities.iter().filter(|o| o.is_profitable).count();
        
        // Get actual pair count from order book cache
        let (pairs_count, _, _) = self.cache.get_stats();
        
        ScannerStatus {
            is_running: self.is_running.load(Ordering::Relaxed),
            last_scan_at: if stats.scan_count > 0 { 
                Some(chrono::Utc::now().to_rfc3339()) 
            } else { 
                None 
            },
            pairs_scanned: pairs_count as i32,
            opportunities_found: stats.opportunities_found as i32,
            profitable_count: profitable_count as i32,
            scan_duration_ms: None,
            scan_count: stats.scan_count,
            event_count: stats.event_count,
        }
    }

    /// Start scanner
    pub async fn start_scanner(&self) {
        self.settings.write().await.scanner_enabled = true;
        info!("Scanner started");
    }

    /// Stop scanner
    pub async fn stop_scanner(&self) {
        self.settings.write().await.scanner_enabled = false;
        info!("Scanner stopped");
    }

    /// Restart WebSocket with current settings
    pub async fn restart_websocket(&self) -> Result<(), EngineError> {
        let settings = self.settings.read().await;
        let max_pairs = settings.max_pairs;
        let depth = settings.orderbook_depth;
        drop(settings);
        
        info!("Restarting WebSocket with max_pairs={}, depth={}", max_pairs, depth);
        
        // Clear existing cache data for fresh start
        self.cache.clear();
        
        // Stop existing WebSocket
        {
            let mut ws_guard = self.websocket.write().await;
            if let Some(ref mut ws) = *ws_guard {
                ws.stop().await;
            }
        }
        
        // Small delay to allow clean disconnect
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        
        // Create and start new WebSocket with updated settings
        {
            let mut ws_guard = self.websocket.write().await;
            let mut ws = KrakenWebSocketV2::new(Arc::clone(&self.cache));
            
            // Set max_pairs BEFORE initialize so it fetches correct number
            ws.set_max_pairs(max_pairs);
            ws.set_orderbook_depth(depth);
            
            // Initialize and start with new settings
            ws.initialize().await
                .map_err(|e| EngineError::WebSocket(e.to_string()))?;
            ws.fetch_initial_prices().await
                .map_err(|e| EngineError::WebSocket(e.to_string()))?;
            ws.start(max_pairs, depth).await
                .map_err(|e| EngineError::WebSocket(e.to_string()))?;
            
            *ws_guard = Some(ws);
        }
        
        // Reinitialize the graph with new pairs from cache
        self.event_scanner.initialize_graph();
        
        info!("WebSocket restarted successfully with {} pairs", max_pairs);
        Ok(())
    }

    /// Get positions/balances from Kraken
    pub async fn get_positions(&self) -> Result<Vec<Position>, EngineError> {
        // Check if auth is configured
        let auth = match &self.auth {
            Some(a) if a.is_configured() => a,
            _ => return Ok(Vec::new()), // No auth, return empty
        };
        
        let client = reqwest::Client::new();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        
        let post_data = format!("nonce={}", nonce);
        let path = "/0/private/Balance";
        let url = format!("https://api.kraken.com{}", path);
        
        // Generate signature
        let signature = auth.sign_request(path, nonce, &post_data)
            .map_err(|e| EngineError::Auth(format!("Failed to sign request: {}", e)))?;
        
        let response = client.post(&url)
            .header("API-Key", auth.api_key())
            .header("API-Sign", signature)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(post_data)
            .send()
            .await
            .map_err(|e| EngineError::Execution(format!("Request failed: {}", e)))?;
        
        let json: serde_json::Value = response.json::<serde_json::Value>().await
            .map_err(|e| EngineError::Execution(format!("Failed to parse response: {}", e)))?;
        
        if let Some(error) = json.get("error").and_then(|e: &serde_json::Value| e.as_array()) {
            if !error.is_empty() {
                return Err(EngineError::Execution(format!("Kraken API error: {:?}", error)));
            }
        }
        
        // Parse balances
        let mut positions = Vec::new();
        if let Some(result) = json.get("result").and_then(|r: &serde_json::Value| r.as_object()) {
            for (currency, balance) in result {
                let balance_f64 = balance.as_str()
                    .and_then(|s: &str| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                
                // Skip zero balances
                if balance_f64 < 0.00000001 {
                    continue;
                }
                
                // Normalize currency name (XXBT -> BTC, ZUSD -> USD, etc.)
                let normalized = self.normalize_currency(currency);
                
                positions.push(Position {
                    currency: normalized,
                    balance: balance_f64,
                    usd_value: None, // Could calculate from prices if needed
                });
            }
        }
        
        Ok(positions)
    }
    
    /// Normalize Kraken currency names
    fn normalize_currency(&self, currency: &str) -> String {
        match currency {
            "XXBT" | "XBT" => "BTC".to_string(),
            "XETH" => "ETH".to_string(),
            "ZUSD" => "USD".to_string(),
            "ZEUR" => "EUR".to_string(),
            "ZGBP" => "GBP".to_string(),
            "ZJPY" => "JPY".to_string(),
            "ZCAD" => "CAD".to_string(),
            "ZAUD" => "AUD".to_string(),
            "XXRP" => "XRP".to_string(),
            "XXLM" => "XLM".to_string(),
            "XLTC" => "LTC".to_string(),
            "XDOGE" | "XXDG" => "DOGE".to_string(),
            other => other.to_string(),
        }
    }

    /// Get order book health
    pub fn get_orderbook_health(&self) -> OrderBookHealth {
        self.event_scanner.get_orderbook_health()
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

    /// Get price for a specific pair
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

    /// Get event scanner stats
    pub fn get_event_scanner_stats(&self) -> EventScannerStatsApi {
        let stats = self.event_scanner.get_stats();
        
        EventScannerStatsApi {
            scans_triggered: stats.scan_count,
            opportunities_detected: stats.opportunities_found,
            event_count: stats.event_count,
            pending_pairs: stats.pending_pairs,
        }
    }

    /// Get fee config
    pub async fn get_fee_config(&self) -> FeeConfig {
        let guard = self.execution_engine.read().await;
        if let Some(engine) = guard.as_ref() {
            engine.get_fee_config().await
        } else {
            FeeConfig::default()
        }
    }

    /// Update fee config
    pub async fn update_fee_config(&self, maker_fee: Option<f64>, taker_fee: Option<f64>) {
        let guard = self.execution_engine.read().await;
        if let Some(engine) = guard.as_ref() {
            let mut config = engine.get_fee_config().await;
            if let Some(f) = maker_fee { config.maker_fee = f; }
            if let Some(f) = taker_fee { config.taker_fee = f; }
            engine.set_fee_config(config).await;
        }
        info!("Fee config updated");
    }

    /// Fetch real fees from Kraken API
    pub async fn fetch_kraken_fees(&self) -> Result<serde_json::Value, String> {
        // Check if auth is configured
        let auth = self.auth.as_ref()
            .ok_or_else(|| "Kraken API credentials not configured".to_string())?;
        
        if !auth.is_configured() {
            return Err("Kraken API credentials not configured".to_string());
        }
        
        let client = reqwest::Client::new();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        
        let post_data = format!("nonce={}", nonce);
        let path = "/0/private/TradeVolume";
        let url = format!("https://api.kraken.com{}", path);
        
        // Generate signature
        let signature = auth.sign_request(path, nonce, &post_data)
            .map_err(|e| format!("Failed to sign request: {}", e))?;
        
        let response = client.post(&url)
            .header("API-Key", auth.api_key())
            .header("API-Sign", signature)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(post_data)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;
        
        let json: serde_json::Value = response.json().await
            .map_err(|e| format!("Failed to parse response: {}", e))?;
        
        if let Some(error) = json.get("error").and_then(|e| e.as_array()) {
            if !error.is_empty() {
                return Err(format!("Kraken API error: {:?}", error));
            }
        }
        
        // Extract fee info from response
        if let Some(result) = json.get("result") {
            let fees = result.get("fees").cloned().unwrap_or(serde_json::json!({}));
            let fees_maker = result.get("fees_maker").cloned().unwrap_or(serde_json::json!({}));
            let volume = result.get("volume").and_then(|v| v.as_str()).unwrap_or("0");
            
            // Get the first fee tier (usually the user's current tier)
            let taker_fee = fees.as_object()
                .and_then(|f| f.values().next())
                .and_then(|v| v.get("fee"))
                .and_then(|f| f.as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.26) / 100.0;
            
            let maker_fee = fees_maker.as_object()
                .and_then(|f| f.values().next())
                .and_then(|v| v.get("fee"))
                .and_then(|f| f.as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.16) / 100.0;
            
            Ok(serde_json::json!({
                "maker_fee": maker_fee,
                "taker_fee": taker_fee,
                "volume_30d": volume,
                "source": "kraken_api"
            }))
        } else {
            Err("No result in Kraken response".to_string())
        }
    }

    /// Save a profitable opportunity to database
    pub async fn save_opportunity(&self, opportunity: &Opportunity) -> Result<(), EngineError> {
        use crate::db::NewLiveOpportunity;
        
        let new_opp = NewLiveOpportunity {
            path: opportunity.path.clone(),
            legs: opportunity.legs as i32,
            expected_profit_pct: opportunity.net_profit_pct,
            expected_profit_usd: None, // Could calculate based on trade amount
            trade_amount: None,
            status: "DETECTED".to_string(),
            status_reason: None,
            pairs_scanned: None,
            paths_found: None,
        };
        
        self.db.save_opportunity(&new_opp).await
            .map_err(|e| EngineError::Database(e.to_string()))?;
        
        Ok(())
    }

    /// Save multiple profitable opportunities to database
    pub async fn save_profitable_opportunities(&self, opportunities: &[Opportunity]) {
        for opp in opportunities.iter().filter(|o| o.is_profitable) {
            if let Err(e) = self.save_opportunity(opp).await {
                warn!("Failed to save opportunity: {}", e);
            }
        }
    }

    /// Get past opportunities from database
    pub async fn get_past_opportunities(&self, limit: i64, hours: i32) -> Result<Vec<crate::db::LiveOpportunity>, EngineError> {
        self.db.get_opportunities(limit, None, hours).await
            .map_err(|e| EngineError::Database(e.to_string()))
    }

    /// Get database reference (for handlers that need direct access)
    pub fn database(&self) -> &Database {
        &self.db
    }
}