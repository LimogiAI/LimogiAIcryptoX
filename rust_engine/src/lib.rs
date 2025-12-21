//! Trading Engine - High-performance Rust extension for LimogiAICryptoX
//!
//! This module provides Python bindings via PyO3 for:
//! - Real-time order book streaming via WebSocket v2
//! - In-memory order book cache
//! - Parallel arbitrage opportunity scanning
//! - Slippage calculation
//! - Order book health monitoring
//! - Trade execution via WebSocket v2 private channels
//!
//! Key features:
//! - WebSocket v2 API for faster, more reliable data
//! - Event-driven scanning on order book updates
//! - Incremental graph updates
//! - Rust-based order execution (Phase 4)

mod auth;
mod config_manager;
mod dispatcher;
mod event_system;
mod executor;
mod graph_manager;
mod order_book;
mod scanner;
mod slippage;
mod trading_config;
mod types;
mod ws_v2;

use crate::auth::KrakenAuth;
use crate::config_manager::ConfigManager;
use crate::dispatcher::Dispatcher;
use crate::event_system::{EventDrivenScanner, ScanTriggerMode};
use crate::executor::{ExecutionEngine, ExecutionMode, FeeConfig, OrderSide, PrePositionedBalances, TradeResult as ExecutorTradeResult};
use crate::order_book::OrderBookCache;
use crate::trading_config::{TradingConfig, TradingGuard, CircuitBreakerState, GuardCheckResult, TradeResult as TradingTradeResult};
use crate::types::{EngineConfig, EngineStats, EngineSettings, Opportunity, OrderBookHealth, SlippageResult};
use crate::ws_v2::KrakenWebSocketV2;

use chrono;
use parking_lot::RwLock;
use pyo3::prelude::*;
use pyo3::exceptions::PyRuntimeError;
use pyo3::types::PyModule;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::runtime::Runtime;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;
use uuid;

/// Main trading engine exposed to Python
#[pyclass]
pub struct TradingEngine {
    cache: Arc<OrderBookCache>,
    websocket_v2: Arc<RwLock<Option<KrakenWebSocketV2>>>,
    config_manager: Arc<ConfigManager>,
    dispatcher: Arc<Dispatcher>,
    event_scanner: Arc<EventDrivenScanner>,
    runtime: Arc<Runtime>,

    // State
    is_running: Arc<AtomicBool>,
    start_time: Option<Instant>,

    // Statistics
    scan_count: Arc<AtomicU64>,
    total_opportunities: Arc<AtomicU64>,

    // Event listener shutdown channel
    event_shutdown_tx: Arc<RwLock<Option<tokio::sync::mpsc::Sender<()>>>>,

    // Phase 4: Execution engine for WebSocket-based trading
    execution_engine: Arc<RwLock<Option<ExecutionEngine>>>,

    // Trading guard for auto-execution with guard checks
    trading_guard: Arc<TradingGuard>,

    // Auto-execution enabled flag
    auto_execution_enabled: Arc<AtomicBool>,
}

#[pymethods]
impl TradingEngine {
    /// Create a new trading engine
    #[new]
    #[pyo3(signature = (
        min_profit_threshold=0.0005,
        fee_rate=0.0026,
        max_pairs=300
    ))]
    fn new(
        min_profit_threshold: f64,
        fee_rate: f64,
        max_pairs: usize,
    ) -> PyResult<Self> {
        // Initialize logging
        let subscriber = FmtSubscriber::builder()
            .with_max_level(Level::INFO)
            .with_target(false)
            .finish();
        let _ = tracing::subscriber::set_global_default(subscriber);
        
        info!("Initializing TradingEngine (WebSocket v2 Mode)...");

        let config = EngineConfig {
            min_profit_threshold,
            fee_rate,
            scan_interval_ms: 10000,        // 10 seconds default
            orderbook_depth: 25,
            max_pairs,
            scanner_enabled: true,
            staleness_warn_ms: 100,
            staleness_buffer_ms: 250,
            staleness_reject_ms: 1000,
        };

        let cache = Arc::new(OrderBookCache::new());
        let config_manager = Arc::new(ConfigManager::new(config.clone()));
        let dispatcher = Arc::new(Dispatcher::new(
            Arc::clone(&cache),
            Arc::clone(&config_manager),
        ));

        // Create event-driven scanner
        let event_scanner = Arc::new(EventDrivenScanner::new(
            Arc::clone(&cache),
            Arc::clone(&config_manager),
        ));

        let runtime = Runtime::new()
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create runtime: {}", e)))?;

        info!(
            "TradingEngine created: {} max pairs, {:.2}% fee rate (WebSocket v2 + Event-Driven)",
            max_pairs,
            fee_rate * 100.0
        );

        Ok(Self {
            cache,
            websocket_v2: Arc::new(RwLock::new(None)),
            config_manager,
            dispatcher,
            event_scanner,
            runtime: Arc::new(runtime),
            is_running: Arc::new(AtomicBool::new(false)),
            start_time: None,
            scan_count: Arc::new(AtomicU64::new(0)),
            total_opportunities: Arc::new(AtomicU64::new(0)),
            event_shutdown_tx: Arc::new(RwLock::new(None)),
            execution_engine: Arc::new(RwLock::new(None)),
            trading_guard: Arc::new(TradingGuard::new()),
            auto_execution_enabled: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Initialize the engine (fetch pairs, initial prices)
    fn initialize(&mut self) -> PyResult<()> {
        info!("Initializing engine with WebSocket v2...");

        let cache = Arc::clone(&self.cache);
        let config = self.config_manager.get_config();
        let max_pairs = config.max_pairs;

        self.runtime.block_on(async {
            let mut ws = KrakenWebSocketV2::new(cache);
            ws.set_max_pairs(max_pairs);

            // Fetch trading pairs
            ws.initialize().await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to initialize: {}", e)))?;

            // Fetch initial prices
            ws.fetch_initial_prices().await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to fetch prices: {}", e)))?;

            *self.websocket_v2.write() = Some(ws);

            Ok::<(), PyErr>(())
        })?;

        let (pairs, currencies, _) = self.cache.get_stats();
        info!("Initialized with {} pairs and {} currencies (v2)", pairs, currencies);

        Ok(())
    }

    /// Start WebSocket v2 streaming
    fn start_websocket(&mut self) -> PyResult<()> {
        let config = self.config_manager.get_config();
        let max_pairs = config.max_pairs;
        let depth = config.orderbook_depth;

        // Create event channel for order book updates
        let (event_shutdown_tx, mut event_shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);
        *self.event_shutdown_tx.write() = Some(event_shutdown_tx);

        // Get event receiver from WebSocket (now returns bounded channel + stats)
        let event_channel = self.runtime.block_on(async {
            let mut ws_guard = self.websocket_v2.write();
            if let Some(ws) = ws_guard.as_mut() {
                let (rx, stats) = ws.create_event_channel();
                ws.start(max_pairs, depth).await
                    .map_err(|e| PyRuntimeError::new_err(format!("Failed to start WebSocket v2: {}", e)))?;
                Ok::<_, PyErr>(Some((rx, stats)))
            } else {
                Ok(None)
            }
        })?;

        // Initialize persistent graph for incremental updates (Phase 3)
        self.event_scanner.initialize_graph();

        // Spawn event listener task
        if let Some((mut rx, event_stats)) = event_channel {
            let event_scanner = Arc::clone(&self.event_scanner);

            self.runtime.spawn(async move {
                info!("Event listener started for event-driven scanning (bounded channel, capacity=1000)");
                let mut last_stats_log = std::time::Instant::now();

                loop {
                    tokio::select! {
                        pair = rx.recv() => {
                            match pair {
                                Some(pair_name) => {
                                    event_scanner.on_orderbook_update(&pair_name);
                                }
                                None => {
                                    info!("Event channel closed");
                                    break;
                                }
                            }
                        }
                        _ = event_shutdown_rx.recv() => {
                            info!("Event listener shutdown");
                            break;
                        }
                    }

                    // Periodically log channel statistics (every 60 seconds)
                    if last_stats_log.elapsed() > std::time::Duration::from_secs(60) {
                        let sent = event_stats.events_sent.load(std::sync::atomic::Ordering::Relaxed);
                        let dropped = event_stats.events_dropped.load(std::sync::atomic::Ordering::Relaxed);
                        if dropped > 0 {
                            tracing::warn!(
                                "Event channel stats: sent={}, dropped={} ({:.2}% drop rate)",
                                sent, dropped, (dropped as f64 / (sent + dropped) as f64) * 100.0
                            );
                        } else {
                            tracing::debug!("Event channel stats: sent={}, dropped=0", sent);
                        }
                        last_stats_log = std::time::Instant::now();
                    }
                }
            });
        }

        self.is_running.store(true, Ordering::SeqCst);
        self.start_time = Some(Instant::now());

        info!("WebSocket v2 streaming started (pairs={}, depth={})", max_pairs, depth);
        Ok(())
    }

    /// Stop WebSocket v2 streaming
    fn stop_websocket(&mut self) -> PyResult<()> {
        // Send shutdown signal to event listener
        if let Some(tx) = self.event_shutdown_tx.write().take() {
            let _ = self.runtime.block_on(async {
                let _ = tx.send(()).await;
            });
        }

        self.runtime.block_on(async {
            let mut ws_guard = self.websocket_v2.write();
            if let Some(ws) = ws_guard.as_mut() {
                ws.stop().await;
            }
            Ok::<(), PyErr>(())
        })?;

        self.is_running.store(false, Ordering::SeqCst);
        info!("WebSocket v2 streaming stopped");
        Ok(())
    }

    /// Run a single scan cycle (returns opportunities)
    fn scan(&self, base_currencies: Vec<String>) -> Vec<Opportunity> {
        let config = self.config_manager.get_config();
        let scanner = scanner::Scanner::new(Arc::clone(&self.cache), config);
        let opportunities = scanner.scan(&base_currencies);
        
        self.scan_count.fetch_add(1, Ordering::Relaxed);
        self.total_opportunities.fetch_add(opportunities.len() as u64, Ordering::Relaxed);
        
        opportunities
    }

    /// Run a dispatch cycle (scan and return opportunities)
    fn run_cycle(&self, base_currencies: Vec<String>) -> Vec<Opportunity> {
        self.scan_count.fetch_add(1, Ordering::Relaxed);
        self.dispatcher.run_cycle(&base_currencies)
    }

    /// Calculate slippage for a path
    fn calculate_slippage(&self, path: String, trade_amount: f64) -> SlippageResult {
        let config = self.config_manager.get_config();
        let calc = slippage::SlippageCalculator::new(
            Arc::clone(&self.cache),
            config.staleness_warn_ms,
            config.staleness_buffer_ms,
            config.staleness_reject_ms,
        );
        calc.calculate_path(&path, trade_amount)
    }

    /// Update scanning configuration
    #[pyo3(signature = (min_profit_threshold=None, fee_rate=None))]
    fn update_config(
        &self,
        min_profit_threshold: Option<f64>,
        fee_rate: Option<f64>,
    ) {
        self.config_manager.update_config(min_profit_threshold, fee_rate);
    }

    /// Update engine settings (scan interval, max pairs, depth, scanner on/off)
    /// Returns true if WebSocket reconnection is needed
    #[pyo3(signature = (scan_interval_ms=None, max_pairs=None, orderbook_depth=None, scanner_enabled=None))]
    fn update_engine_settings(
        &self,
        scan_interval_ms: Option<u64>,
        max_pairs: Option<usize>,
        orderbook_depth: Option<usize>,
        scanner_enabled: Option<bool>,
    ) -> bool {
        self.config_manager.update_engine_settings(
            scan_interval_ms,
            max_pairs,
            orderbook_depth,
            scanner_enabled,
        )
    }

    /// Get current engine settings
    fn get_engine_settings(&self) -> EngineSettings {
        let (scan_interval_ms, max_pairs, orderbook_depth, scanner_enabled) = 
            self.config_manager.get_engine_settings();
        EngineSettings {
            scan_interval_ms,
            max_pairs,
            orderbook_depth,
            scanner_enabled,
        }
    }

    /// Check if scanner is enabled
    fn is_scanner_enabled(&self) -> bool {
        self.config_manager.is_scanner_enabled()
    }

    /// Reconnect WebSocket v2 with new settings (max_pairs and/or depth changed)
    fn reconnect_websocket(&mut self) -> PyResult<()> {
        info!("Reconnecting WebSocket v2 with new settings...");

        // Stop event listener first
        if let Some(tx) = self.event_shutdown_tx.write().take() {
            let _ = self.runtime.block_on(async {
                let _ = tx.send(()).await;
            });
        }

        // Stop current connection
        self.runtime.block_on(async {
            let mut ws_guard = self.websocket_v2.write();
            if let Some(ws) = ws_guard.as_mut() {
                ws.stop().await;
            }
            Ok::<(), PyErr>(())
        })?;

        self.is_running.store(false, Ordering::SeqCst);

        // Brief pause to ensure clean disconnect
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Clear old order book data
        self.cache.clear();

        // Re-initialize with new settings
        let config = self.config_manager.get_config();
        let max_pairs = config.max_pairs;
        let depth = config.orderbook_depth;

        // Create new event channel
        let (event_shutdown_tx, mut event_shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);
        *self.event_shutdown_tx.write() = Some(event_shutdown_tx);

        let (mut event_rx, event_stats) = self.runtime.block_on(async {
            let mut ws = KrakenWebSocketV2::new(Arc::clone(&self.cache));
            ws.set_max_pairs(max_pairs);
            ws.set_orderbook_depth(depth);

            // Fetch trading pairs
            ws.initialize().await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to initialize: {}", e)))?;

            // Fetch initial prices
            ws.fetch_initial_prices().await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to fetch prices: {}", e)))?;

            // Create event channel and start streaming (bounded channel)
            let (rx, stats) = ws.create_event_channel();
            ws.start(max_pairs, depth).await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to start WebSocket v2: {}", e)))?;

            *self.websocket_v2.write() = Some(ws);

            Ok::<_, PyErr>((rx, stats))
        })?;

        // Reinitialize persistent graph for incremental updates (Phase 3)
        self.event_scanner.initialize_graph();

        // Spawn new event listener task
        let event_scanner = Arc::clone(&self.event_scanner);
        self.runtime.spawn(async move {
            info!("Event listener restarted for event-driven scanning (bounded channel)");
            let mut last_stats_log = std::time::Instant::now();

            loop {
                tokio::select! {
                    pair = event_rx.recv() => {
                        match pair {
                            Some(pair_name) => {
                                event_scanner.on_orderbook_update(&pair_name);
                            }
                            None => {
                                info!("Event channel closed");
                                break;
                            }
                        }
                    }
                    _ = event_shutdown_rx.recv() => {
                        info!("Event listener shutdown");
                        break;
                    }
                }

                // Periodically log channel statistics (every 60 seconds)
                if last_stats_log.elapsed() > std::time::Duration::from_secs(60) {
                    let sent = event_stats.events_sent.load(std::sync::atomic::Ordering::Relaxed);
                    let dropped = event_stats.events_dropped.load(std::sync::atomic::Ordering::Relaxed);
                    if dropped > 0 {
                        tracing::warn!(
                            "Event channel stats: sent={}, dropped={} ({:.2}% drop rate)",
                            sent, dropped, (dropped as f64 / (sent + dropped) as f64) * 100.0
                        );
                    }
                    last_stats_log = std::time::Instant::now();
                }
            }
        });

        self.is_running.store(true, Ordering::SeqCst);

        let (pairs, currencies, _) = self.cache.get_stats();
        info!("WebSocket v2 reconnected with {} pairs, {} currencies, depth={}", pairs, currencies, depth);

        Ok(())
    }

    /// Get order book health statistics
    fn get_orderbook_health(&self) -> OrderBookHealth {
        // Use event scanner's health (which uses PersistentGraph)
        self.event_scanner.get_orderbook_health()
    }

    /// Get engine statistics
    fn get_stats(&self) -> EngineStats {
        let (pairs, currencies, avg_staleness) = self.cache.get_stats();
        let dispatcher_stats = self.dispatcher.get_stats();
        
        let uptime = self.start_time
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0);
        
        let opps_per_sec = if uptime > 0 {
            self.total_opportunities.load(Ordering::Relaxed) as f64 / uptime as f64
        } else {
            0.0
        };
        
        EngineStats {
            is_running: self.is_running.load(Ordering::SeqCst),
            pairs_monitored: pairs,
            currencies_tracked: currencies,
            orderbooks_cached: pairs,
            avg_orderbook_staleness_ms: avg_staleness,
            opportunities_found: dispatcher_stats.opportunities_found,
            opportunities_per_second: opps_per_sec,
            uptime_seconds: uptime,
            scan_cycle_ms: dispatcher_stats.last_cycle_duration_ms,
            last_scan_at: dispatcher_stats.last_cycle_at,
        }
    }

    /// Check if engine is running
    fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }

    /// Get price for a pair
    fn get_price(&self, pair: String) -> Option<(f64, f64)> {
        self.cache.get_price(&pair).map(|e| (e.bid, e.ask))
    }

    /// Get all prices
    fn get_all_prices(&self) -> Vec<(String, f64, f64, f64)> {
        self.cache
            .get_all_prices()
            .into_iter()
            .map(|(pair, edge)| (pair, edge.bid, edge.ask, edge.volume_24h))
            .collect()
    }

    /// Get all currency symbols
    fn get_currencies(&self) -> Vec<String> {
        self.cache.get_currencies().into_iter().collect()
    }

    /// Get all pair names
    fn get_pairs(&self) -> Vec<String> {
        self.cache.get_all_pairs()
    }

    // =========================================================================
    // Event-Driven Scanning Methods
    // =========================================================================

    /// Set event-driven scan mode
    /// - "disabled": Use polling only (10s interval)
    /// - "immediate": Scan on every order book update
    /// - "debounced": Debounce scans with a time window (default: 50ms)
    #[pyo3(signature = (mode, debounce_ms=50))]
    fn set_scan_mode(&self, mode: String, debounce_ms: u64) {
        let trigger_mode = match mode.to_lowercase().as_str() {
            "disabled" => ScanTriggerMode::Disabled,
            "immediate" => ScanTriggerMode::Immediate,
            "debounced" => ScanTriggerMode::Debounced(debounce_ms),
            _ => {
                info!("Unknown scan mode '{}', using debounced", mode);
                ScanTriggerMode::Debounced(debounce_ms)
            }
        };
        self.event_scanner.set_trigger_mode(trigger_mode);
    }

    /// Get current scan mode
    fn get_scan_mode(&self) -> String {
        match self.event_scanner.get_trigger_mode() {
            ScanTriggerMode::Disabled => "disabled".to_string(),
            ScanTriggerMode::Immediate => "immediate".to_string(),
            ScanTriggerMode::Debounced(ms) => format!("debounced_{}ms", ms),
        }
    }

    /// Set base currencies for event-driven scanning
    fn set_event_scan_currencies(&self, currencies: Vec<String>) {
        self.event_scanner.set_base_currencies(currencies);
    }

    /// Notify the event scanner that an order book was updated
    /// Called internally by WebSocket handler
    fn notify_orderbook_update(&self, pair: String) {
        self.event_scanner.on_orderbook_update(&pair);
    }

    /// Get event scanner statistics
    fn get_event_scanner_stats(&self) -> (u64, u64, u64, usize, String) {
        let stats = self.event_scanner.get_stats();
        (
            stats.event_count,
            stats.scan_count,
            stats.opportunities_found,
            stats.pending_pairs,
            format!("{:?}", stats.mode),
        )
    }

    /// Get detailed event scanner statistics including incremental graph info
    fn get_event_scanner_stats_detailed(&self) -> (u64, u64, u64, u64, u64, usize, usize, bool) {
        let stats = self.event_scanner.get_stats();
        (
            stats.event_count,
            stats.scan_count,
            stats.incremental_updates,
            stats.full_rebuilds,
            stats.opportunities_found,
            stats.graph_nodes,
            stats.graph_edges,
            stats.incremental_enabled,
        )
    }

    /// Get graph detailed stats: (nodes, pairs, total_edges, valid_edges)
    fn get_graph_detailed_stats(&self) -> (usize, usize, usize, usize) {
        self.event_scanner.get_graph_detailed_stats()
    }

    /// Debug: count paths from USD and EUR
    fn debug_count_paths(&self) -> (usize, usize) {
        self.event_scanner.debug_count_paths()
    }

    /// Debug: get currencies connected to USD
    fn debug_get_usd_connections(&self) -> Vec<String> {
        self.event_scanner.debug_get_usd_connections()
    }

    /// Enable or disable incremental graph updates (Phase 3)
    fn set_incremental_mode(&self, enabled: bool) {
        self.event_scanner.set_incremental_mode(enabled);
    }

    /// Check if incremental mode is enabled
    fn is_incremental_enabled(&self) -> bool {
        self.event_scanner.is_incremental_enabled()
    }

    /// Get cached opportunities from event-driven scanner
    /// Returns opportunities found in the most recent scan (no new scan triggered)
    fn get_cached_opportunities(&self) -> Vec<Opportunity> {
        self.event_scanner.get_cached_opportunities()
    }

    /// Get cached opportunities with age in milliseconds
    /// Returns (opportunities, age_ms) where age_ms is how old the cache is
    fn get_cached_opportunities_with_age(&self) -> (Vec<Opportunity>, u64) {
        self.event_scanner.get_cached_opportunities_with_age()
    }

    // =========================================================================
    // Phase 4: Execution Engine Methods
    // =========================================================================

    /// Initialize the execution engine with API credentials
    /// Must be called before any trading operations
    fn init_execution_engine(&self, api_key: String, api_secret: String) -> PyResult<()> {
        let auth = KrakenAuth::new(api_key, api_secret)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to initialize auth: {}", e)))?;

        let engine = ExecutionEngine::new(
            Arc::new(auth),
            Arc::clone(&self.cache),
        );

        *self.execution_engine.write() = Some(engine);
        info!("Execution engine initialized with API credentials");

        Ok(())
    }

    /// Connect the execution engine to Kraken's private WebSocket
    fn connect_execution_engine(&self) -> PyResult<()> {
        let engine_guard = self.execution_engine.read();
        let engine = engine_guard.as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Execution engine not initialized. Call init_execution_engine first."))?;

        self.runtime.block_on(async {
            engine.connect().await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to connect execution engine: {}", e)))
        })
    }

    /// Disconnect the execution engine
    fn disconnect_execution_engine(&self) -> PyResult<()> {
        let engine_guard = self.execution_engine.read();
        if let Some(engine) = engine_guard.as_ref() {
            engine.disconnect();
        }
        Ok(())
    }

    /// Check if execution engine is connected
    fn is_execution_engine_connected(&self) -> bool {
        let engine_guard = self.execution_engine.read();
        engine_guard.as_ref()
            .map(|e| e.is_connected())
            .unwrap_or(false)
    }

    /// Place a single order via WebSocket
    /// Returns: (order_id, status, filled_qty, avg_price, error_msg)
    fn place_order(&self, pair: String, side: String, quantity: f64) -> PyResult<(String, String, f64, f64, String)> {
        let engine_guard = self.execution_engine.read();
        let engine = engine_guard.as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Execution engine not initialized"))?;

        let order_side = match side.to_lowercase().as_str() {
            "buy" => OrderSide::Buy,
            "sell" => OrderSide::Sell,
            _ => return Err(PyRuntimeError::new_err(format!("Invalid side: {}. Use 'buy' or 'sell'", side))),
        };

        let result = self.runtime.block_on(async {
            engine.place_order(&pair, order_side, quantity).await
        });

        match result {
            Ok(response) => Ok((
                response.order_id,
                response.status,
                response.filled_qty,
                response.avg_price,
                String::new(),
            )),
            Err(e) => Ok((
                String::new(),
                "error".to_string(),
                0.0,
                0.0,
                e.to_string(),
            )),
        }
    }

    /// Execute an arbitrage opportunity
    /// Returns: (success, legs_completed, total_input, total_output, profit_pct, error_msg)
    fn execute_opportunity(&self, path: String, amount: f64) -> PyResult<(bool, usize, f64, f64, f64, String)> {
        let engine_guard = self.execution_engine.read();
        let engine = engine_guard.as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Execution engine not initialized"))?;

        // Create a minimal opportunity for execution
        let opportunity = Opportunity {
            id: uuid::Uuid::new_v4().to_string(),
            path: path.clone(),
            legs: path.matches(" → ").count() + 1,
            gross_profit_pct: 0.0,
            fees_pct: 0.0,
            net_profit_pct: 0.0,
            is_profitable: true,
            detected_at: chrono::Utc::now(),
        };

        let result = self.runtime.block_on(async {
            engine.execute_opportunity(&opportunity, amount).await
        });

        match result {
            Ok(trade_result) => {
                // TradeResult is a struct with success field
                if trade_result.success {
                    Ok((
                        true,
                        trade_result.legs.len(),
                        trade_result.start_amount,
                        trade_result.end_amount,
                        trade_result.profit_pct,
                        String::new(),
                    ))
                } else {
                    Ok((
                        false,
                        trade_result.legs.iter().filter(|l| l.success).count(),
                        trade_result.start_amount,
                        trade_result.end_amount,
                        trade_result.profit_pct,
                        trade_result.error.unwrap_or_else(|| "Unknown error".to_string()),
                    ))
                }
            }
            Err(e) => Ok((false, 0, amount, 0.0, 0.0, e.to_string())),
        }
    }

    /// Get execution engine statistics
    /// Returns: (orders_placed, orders_filled, orders_failed, total_volume, is_connected)
    fn get_execution_stats(&self) -> (u64, u64, u64, f64, bool) {
        let engine_guard = self.execution_engine.read();
        match engine_guard.as_ref() {
            Some(engine) => {
                let stats = engine.get_stats();
                (
                    stats.orders_placed,
                    stats.orders_filled,
                    stats.orders_failed,
                    stats.total_volume,
                    engine.is_connected(),
                )
            }
            None => (0, 0, 0, 0.0, false),
        }
    }

    // =========================================================================
    // Phase 5: Parallel Leg Execution Methods
    // =========================================================================

    /// Analyze a path for parallel execution opportunities
    /// Returns: (num_groups, can_fully_parallelize, estimated_speedup_pct)
    ///
    /// Args:
    ///   path: The arbitrage path (e.g., "USD → BTC → ETH → USD")
    ///   amount: The trade amount
    ///   balances: Dict of pre-positioned balances {currency: amount}
    #[pyo3(signature = (path, amount, balances=None))]
    fn analyze_parallel_execution(
        &self,
        path: String,
        amount: f64,
        balances: Option<std::collections::HashMap<String, f64>>,
    ) -> PyResult<(usize, bool, f64)> {
        let engine_guard = self.execution_engine.read();
        let engine = engine_guard.as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Execution engine not initialized"))?;

        let legs = engine.parse_path(&path, amount)
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

        let pre_positioned = match balances {
            Some(b) => PrePositionedBalances { balances: b },
            None => PrePositionedBalances::new(),
        };

        let plan = engine.analyze_parallel_opportunities(&legs, &pre_positioned);

        Ok((
            plan.groups.len(),
            plan.can_fully_parallelize,
            plan.estimated_speedup,
        ))
    }

    /// Execute an arbitrage opportunity with parallel leg execution
    /// Returns: (success, legs_completed, total_input, total_output, profit_pct, error_msg)
    ///
    /// Args:
    ///   path: The arbitrage path (e.g., "USD → BTC → ETH → USD")
    ///   amount: The trade amount
    ///   balances: Dict of pre-positioned balances {currency: amount}
    ///   mode: "sequential" (default) or "parallel"
    #[pyo3(signature = (path, amount, balances=None, mode="sequential"))]
    fn execute_opportunity_parallel(
        &self,
        path: String,
        amount: f64,
        balances: Option<std::collections::HashMap<String, f64>>,
        mode: &str,
    ) -> PyResult<(bool, usize, f64, f64, f64, String)> {
        let engine_guard = self.execution_engine.read();
        let engine = engine_guard.as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Execution engine not initialized"))?;

        let pre_positioned = match balances {
            Some(b) => PrePositionedBalances { balances: b },
            None => PrePositionedBalances::new(),
        };

        let exec_mode = match mode.to_lowercase().as_str() {
            "parallel" => ExecutionMode::Parallel,
            _ => ExecutionMode::Sequential,
        };

        // Create a minimal opportunity for execution
        let opportunity = Opportunity {
            id: uuid::Uuid::new_v4().to_string(),
            path: path.clone(),
            legs: path.matches(" → ").count() + 1,
            gross_profit_pct: 0.0,
            fees_pct: 0.0,
            net_profit_pct: 0.0,
            is_profitable: true,
            detected_at: chrono::Utc::now(),
        };

        let result = self.runtime.block_on(async {
            engine.execute_opportunity_auto(&opportunity, amount, &pre_positioned, exec_mode).await
        });

        match result {
            Ok(trade_result) => {
                if trade_result.success {
                    Ok((
                        true,
                        trade_result.legs.len(),
                        trade_result.start_amount,
                        trade_result.end_amount,
                        trade_result.profit_pct,
                        String::new(),
                    ))
                } else {
                    Ok((
                        false,
                        trade_result.legs.iter().filter(|l| l.success).count(),
                        trade_result.start_amount,
                        trade_result.end_amount,
                        trade_result.profit_pct,
                        trade_result.error.unwrap_or_else(|| "Unknown error".to_string()),
                    ))
                }
            }
            Err(e) => Ok((false, 0, amount, 0.0, 0.0, e.to_string())),
        }
    }

    // =========================================================================
    // Phase 6: Dynamic Fee Optimization Methods
    // =========================================================================

    /// Configure dynamic fee optimization
    /// Args:
    ///   maker_fee: Maker fee rate (e.g., 0.0016 for 0.16%)
    ///   taker_fee: Taker fee rate (e.g., 0.0026 for 0.26%)
    ///   min_profit_for_maker: Minimum profit % to try maker orders
    ///   max_spread_for_maker: Maximum spread % for maker orders
    ///   use_maker_for_intermediate: Enable maker orders for non-final legs
    #[pyo3(signature = (maker_fee=0.0016, taker_fee=0.0026, min_profit_for_maker=0.5, max_spread_for_maker=0.1, use_maker_for_intermediate=false))]
    fn set_fee_config(
        &self,
        maker_fee: f64,
        taker_fee: f64,
        min_profit_for_maker: f64,
        max_spread_for_maker: f64,
        use_maker_for_intermediate: bool,
    ) -> PyResult<()> {
        let engine_guard = self.execution_engine.read();
        if let Some(engine) = engine_guard.as_ref() {
            engine.set_fee_config(FeeConfig {
                maker_fee,
                taker_fee,
                min_profit_for_maker,
                max_spread_for_maker,
                use_maker_for_intermediate,
            });
            Ok(())
        } else {
            Err(PyRuntimeError::new_err("Execution engine not initialized"))
        }
    }

    /// Get current fee configuration
    /// Returns: (maker_fee, taker_fee, min_profit_for_maker, max_spread_for_maker, use_maker_for_intermediate)
    fn get_fee_config(&self) -> PyResult<(f64, f64, f64, f64, bool)> {
        let engine_guard = self.execution_engine.read();
        if let Some(engine) = engine_guard.as_ref() {
            let config = engine.get_fee_config();
            Ok((
                config.maker_fee,
                config.taker_fee,
                config.min_profit_for_maker,
                config.max_spread_for_maker,
                config.use_maker_for_intermediate,
            ))
        } else {
            Err(PyRuntimeError::new_err("Execution engine not initialized"))
        }
    }

    /// Get fee optimization statistics
    /// Returns: (maker_orders_attempted, maker_orders_filled, total_savings_usd, success_rate_pct)
    fn get_fee_optimization_stats(&self) -> (u64, u64, f64, f64) {
        let engine_guard = self.execution_engine.read();
        match engine_guard.as_ref() {
            Some(engine) => engine.get_fee_stats(),
            None => (0, 0, 0.0, 0.0),
        }
    }

    /// Execute an opportunity with optimized fee selection
    /// Returns: (success, legs_completed, total_input, total_output, profit_pct, error_msg)
    ///
    /// This method automatically selects maker vs taker orders based on:
    /// - Opportunity profit margin
    /// - Order book spread
    /// - Leg position (final leg always uses taker for certainty)
    #[pyo3(signature = (path, amount, opportunity_profit_pct=0.0))]
    fn execute_opportunity_optimized(
        &self,
        path: String,
        amount: f64,
        opportunity_profit_pct: f64,
    ) -> PyResult<(bool, usize, f64, f64, f64, String)> {
        let engine_guard = self.execution_engine.read();
        let engine = engine_guard.as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Execution engine not initialized"))?;

        // Create opportunity with profit info for fee optimization
        let opportunity = Opportunity {
            id: uuid::Uuid::new_v4().to_string(),
            path: path.clone(),
            legs: path.matches(" → ").count() + 1,
            gross_profit_pct: opportunity_profit_pct,
            fees_pct: 0.0,
            net_profit_pct: opportunity_profit_pct,
            is_profitable: opportunity_profit_pct > 0.0,
            detected_at: chrono::Utc::now(),
        };

        // Execute with fee optimization
        let result = self.runtime.block_on(async {
            engine.execute_opportunity(&opportunity, amount).await
        });

        match result {
            Ok(trade_result) => {
                if trade_result.success {
                    Ok((
                        true,
                        trade_result.legs.len(),
                        trade_result.start_amount,
                        trade_result.end_amount,
                        trade_result.profit_pct,
                        String::new(),
                    ))
                } else {
                    Ok((
                        false,
                        trade_result.legs.iter().filter(|l| l.success).count(),
                        trade_result.start_amount,
                        trade_result.end_amount,
                        trade_result.profit_pct,
                        trade_result.error.unwrap_or_else(|| "Unknown error".to_string()),
                    ))
                }
            }
            Err(e) => Ok((false, 0, amount, 0.0, 0.0, e.to_string())),
        }
    }

    /// String representation
    fn __repr__(&self) -> String {
        format!(
            "TradingEngine(pairs={}, running={})",
            self.cache.get_all_pairs().len(),
            self.is_running.load(Ordering::SeqCst)
        )
    }

    // =========================================================================
    // Trading Guard & Auto-Execution Methods
    // =========================================================================

    /// Update trading configuration for auto-execution
    /// This configures guard checks (thresholds, circuit breaker limits)
    #[pyo3(signature = (
        enabled=None,
        trade_amount=None,
        min_profit_threshold=None,
        max_daily_loss=None,
        max_total_loss=None,
        base_currency=None,
        execution_mode=None
    ))]
    fn update_trading_config(
        &self,
        enabled: Option<bool>,
        trade_amount: Option<f64>,
        min_profit_threshold: Option<f64>,
        max_daily_loss: Option<f64>,
        max_total_loss: Option<f64>,
        base_currency: Option<String>,
        execution_mode: Option<String>,
    ) {
        let mut config = self.trading_guard.get_config();

        if let Some(v) = enabled { config.enabled = v; }
        if let Some(v) = trade_amount { config.trade_amount = v; }
        if let Some(v) = min_profit_threshold { config.min_profit_threshold = v; }
        if let Some(v) = max_daily_loss { config.max_daily_loss = v; }
        if let Some(v) = max_total_loss { config.max_total_loss = v; }
        if let Some(v) = base_currency { config.base_currency = v; }
        if let Some(v) = execution_mode { config.execution_mode = v; }

        self.trading_guard.update_config(config);
    }

    /// Get current trading configuration
    /// Returns: (enabled, trade_amount, min_profit_threshold, max_daily_loss, max_total_loss, base_currency, execution_mode)
    fn get_trading_config(&self) -> (bool, f64, f64, f64, f64, String, String) {
        let config = self.trading_guard.get_config();
        (
            config.enabled,
            config.trade_amount,
            config.min_profit_threshold,
            config.max_daily_loss,
            config.max_total_loss,
            config.base_currency,
            config.execution_mode,
        )
    }

    /// Enable live trading
    fn enable_trading(&self) {
        self.trading_guard.enable();
    }

    /// Disable live trading
    fn disable_trading(&self, reason: String) {
        self.trading_guard.disable(&reason);
    }

    /// Check if trading is enabled
    fn is_trading_enabled(&self) -> bool {
        self.trading_guard.is_enabled()
    }

    /// Trip the circuit breaker manually
    fn trip_circuit_breaker(&self, reason: String) {
        self.trading_guard.trip_circuit_breaker(&reason);
    }

    /// Reset the circuit breaker
    fn reset_circuit_breaker(&self) {
        self.trading_guard.reset_circuit_breaker();
    }

    /// Check if circuit breaker is tripped
    fn is_circuit_broken(&self) -> bool {
        self.trading_guard.is_circuit_broken()
    }

    /// Get circuit breaker state
    /// Returns: (is_broken, reason, daily_pnl, total_pnl, daily_trades, total_trades, is_executing)
    fn get_circuit_breaker_state(&self) -> (bool, Option<String>, f64, f64, u32, u32, bool) {
        let state = self.trading_guard.get_state();
        (
            state.is_broken,
            state.broken_reason,
            state.daily_pnl,
            state.total_pnl,
            state.daily_trades,
            state.total_trades,
            state.is_executing,
        )
    }

    /// Check if an opportunity passes all guard checks
    /// Returns: (can_trade, reason)
    fn check_opportunity_guards(&self, path: String, profit_pct: f64) -> (bool, Option<String>) {
        let result = self.trading_guard.check_opportunity(&path, profit_pct);
        (result.can_trade, result.reason)
    }

    /// Get trading statistics
    /// Returns: (trades_executed, trades_successful, opportunities_seen, opportunities_executed, daily_pnl, total_pnl)
    fn get_trading_stats(&self) -> (u64, u64, u64, u64, f64, f64) {
        self.trading_guard.get_stats()
    }

    /// Reset daily trading statistics
    fn reset_daily_stats(&self) {
        self.trading_guard.reset_daily();
    }

    /// Enable auto-execution (Rust handles full pipeline)
    /// When enabled, Rust will automatically execute profitable opportunities
    /// as soon as they are detected - no Python polling required
    fn enable_auto_execution(&self) {
        self.auto_execution_enabled.store(true, Ordering::SeqCst);

        // Also enable in the event scanner
        self.event_scanner.enable_auto_execution();

        info!("Auto-execution ENABLED - Rust will handle full trading pipeline");
    }

    /// Disable auto-execution
    fn disable_auto_execution(&self) {
        self.auto_execution_enabled.store(false, Ordering::SeqCst);

        // Also disable in the event scanner
        self.event_scanner.disable_auto_execution();

        info!("Auto-execution DISABLED");
    }

    /// Check if auto-execution is enabled
    fn is_auto_execution_enabled(&self) -> bool {
        self.auto_execution_enabled.load(Ordering::SeqCst)
    }

    /// Setup auto-execution pipeline in the event scanner
    /// This wires up the trading guard and execution engine to the scanner
    /// so it can auto-execute when profitable opportunities are detected
    fn setup_auto_execution_pipeline(&self) -> PyResult<()> {
        // Set up auto-execution in the event scanner
        self.event_scanner.setup_auto_execution(
            Arc::clone(&self.trading_guard),
            Arc::clone(&self.execution_engine),
            Arc::clone(&self.runtime),
        );

        info!("Auto-execution pipeline setup complete");
        Ok(())
    }

    /// Get auto-execution statistics
    /// Returns: (auto_executions, auto_successes)
    fn get_auto_execution_stats(&self) -> (u64, u64) {
        self.event_scanner.get_auto_execution_stats()
    }

    /// Execute an opportunity through the full Rust pipeline
    /// This includes guard checks, execution, and result recording
    /// Returns: (success, trade_id, profit_amount, profit_pct, error_msg)
    fn execute_with_guards(&self, path: String, profit_pct: f64) -> PyResult<(bool, String, f64, f64, String)> {
        // Check guards first
        let guard_result = self.trading_guard.check_opportunity(&path, profit_pct);
        if !guard_result.can_trade {
            return Ok((
                false,
                String::new(),
                0.0,
                0.0,
                guard_result.reason.unwrap_or_else(|| "Guard check failed".to_string()),
            ));
        }

        // Try to start execution (check if already executing in sequential mode)
        if !self.trading_guard.try_start_execution() {
            return Ok((
                false,
                String::new(),
                0.0,
                0.0,
                "Another trade is already executing".to_string(),
            ));
        }

        // Get trade amount from config
        let config = self.trading_guard.get_config();
        let trade_amount = config.trade_amount;

        // Check execution engine
        let engine_guard = self.execution_engine.read();
        let engine = match engine_guard.as_ref() {
            Some(e) => e,
            None => {
                self.trading_guard.finish_execution();
                return Ok((
                    false,
                    String::new(),
                    0.0,
                    0.0,
                    "Execution engine not initialized".to_string(),
                ));
            }
        };

        // Execute the opportunity
        let opportunity = Opportunity {
            id: uuid::Uuid::new_v4().to_string(),
            path: path.clone(),
            legs: path.matches(" → ").count() + 1,
            gross_profit_pct: profit_pct,
            fees_pct: 0.0,
            net_profit_pct: profit_pct,
            is_profitable: true,
            detected_at: chrono::Utc::now(),
        };

        let result = self.runtime.block_on(async {
            engine.execute_opportunity(&opportunity, trade_amount).await
        });

        // Finish execution flag
        self.trading_guard.finish_execution();

        match result {
            Ok(trade_result) => {
                // Record the trade result
                let trading_result = TradingTradeResult {
                    trade_id: trade_result.id.clone(),
                    path: trade_result.path.clone(),
                    status: if trade_result.success { "COMPLETED".to_string() } else { "FAILED".to_string() },
                    legs_completed: trade_result.legs.iter().filter(|l| l.success).count(),
                    total_legs: trade_result.legs.len(),
                    amount_in: trade_result.start_amount,
                    amount_out: trade_result.end_amount,
                    profit_pct: trade_result.profit_pct,
                    profit_amount: trade_result.profit_amount,
                    execution_time_ms: trade_result.total_duration_ms,
                    error: trade_result.error.clone(),
                    leg_details: trade_result.legs.iter().map(|l| {
                        trading_config::LegResult {
                            leg_index: l.leg_index,
                            pair: l.pair.clone(),
                            side: l.side.clone(),
                            amount_in: l.input_amount,
                            amount_out: l.output_amount,
                            price: l.avg_price,
                            fee: l.fee,
                            status: if l.success { "FILLED".to_string() } else { "FAILED".to_string() },
                        }
                    }).collect(),
                };

                self.trading_guard.record_trade(&trading_result);

                Ok((
                    trade_result.success,
                    trade_result.id,
                    trade_result.profit_amount,
                    trade_result.profit_pct,
                    trade_result.error.unwrap_or_default(),
                ))
            }
            Err(e) => {
                Ok((
                    false,
                    String::new(),
                    0.0,
                    0.0,
                    e.to_string(),
                ))
            }
        }
    }
}

/// Python module definition
#[pymodule]
fn trading_engine(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<TradingEngine>()?;
    m.add_class::<Opportunity>()?;
    m.add_class::<SlippageResult>()?;
    m.add_class::<crate::types::SlippageLeg>()?;
    m.add_class::<EngineStats>()?;
    m.add_class::<EngineSettings>()?;
    m.add_class::<OrderBookHealth>()?;

    Ok(())
}
