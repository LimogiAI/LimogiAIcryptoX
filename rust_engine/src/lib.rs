//! Trading Engine - High-performance Rust extension for LimogiAICryptoX
//!
//! This module provides Python bindings via PyO3 for:
//! - Real-time order book streaming via WebSocket
//! - In-memory order book cache
//! - Parallel arbitrage opportunity scanning
//! - Slippage calculation
//! - Order book health monitoring
//!
//! All trade execution is handled by the Python live trading system.

mod config_manager;
mod dispatcher;
mod order_book;
mod scanner;
mod slippage;
mod types;
mod websocket;

use crate::config_manager::ConfigManager;
use crate::dispatcher::Dispatcher;
use crate::order_book::OrderBookCache;
use crate::types::{EngineConfig, EngineStats, EngineSettings, Opportunity, OrderBookHealth, SlippageResult};
use crate::websocket::KrakenWebSocket;

use parking_lot::RwLock;
use pyo3::prelude::*;
use pyo3::exceptions::PyRuntimeError;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::runtime::Runtime;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

/// Main trading engine exposed to Python
#[pyclass]
pub struct TradingEngine {
    cache: Arc<OrderBookCache>,
    websocket: Arc<RwLock<Option<KrakenWebSocket>>>,
    config_manager: Arc<ConfigManager>,
    dispatcher: Arc<Dispatcher>,
    runtime: Arc<Runtime>,
    
    // State
    is_running: Arc<AtomicBool>,
    start_time: Option<Instant>,
    
    // Statistics
    scan_count: Arc<AtomicU64>,
    total_opportunities: Arc<AtomicU64>,
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
        
        info!("Initializing TradingEngine (Live Trading Mode)...");
        
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
        
        let runtime = Runtime::new()
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create runtime: {}", e)))?;
        
        info!(
            "TradingEngine created: {} max pairs, {:.2}% fee rate",
            max_pairs,
            fee_rate * 100.0
        );
        
        Ok(Self {
            cache,
            websocket: Arc::new(RwLock::new(None)),
            config_manager,
            dispatcher,
            runtime: Arc::new(runtime),
            is_running: Arc::new(AtomicBool::new(false)),
            start_time: None,
            scan_count: Arc::new(AtomicU64::new(0)),
            total_opportunities: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Initialize the engine (fetch pairs, initial prices)
    fn initialize(&mut self) -> PyResult<()> {
        info!("Initializing engine...");
        
        let cache = Arc::clone(&self.cache);
        let config = self.config_manager.get_config();
        let max_pairs = config.max_pairs;
        
        self.runtime.block_on(async {
            let mut ws = KrakenWebSocket::new(cache);
            ws.set_max_pairs(max_pairs);
            
            // Fetch trading pairs
            ws.initialize().await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to initialize: {}", e)))?;
            
            // Fetch initial prices
            ws.fetch_initial_prices().await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to fetch prices: {}", e)))?;
            
            *self.websocket.write() = Some(ws);
            
            Ok::<(), PyErr>(())
        })?;
        
        let (pairs, currencies, _) = self.cache.get_stats();
        info!("Initialized with {} pairs and {} currencies", pairs, currencies);
        
        Ok(())
    }

    /// Start WebSocket streaming
    fn start_websocket(&mut self) -> PyResult<()> {
        let config = self.config_manager.get_config();
        let max_pairs = config.max_pairs;
        let depth = config.orderbook_depth;
        
        self.runtime.block_on(async {
            let mut ws_guard = self.websocket.write();
            if let Some(ws) = ws_guard.as_mut() {
                ws.start(max_pairs, depth).await
                    .map_err(|e| PyRuntimeError::new_err(format!("Failed to start WebSocket: {}", e)))?;
            }
            Ok::<(), PyErr>(())
        })?;
        
        self.is_running.store(true, Ordering::SeqCst);
        self.start_time = Some(Instant::now());
        
        info!("WebSocket streaming started (pairs={}, depth={})", max_pairs, depth);
        Ok(())
    }

    /// Stop WebSocket streaming
    fn stop_websocket(&mut self) -> PyResult<()> {
        self.runtime.block_on(async {
            let mut ws_guard = self.websocket.write();
            if let Some(ws) = ws_guard.as_mut() {
                ws.stop().await;
            }
            Ok::<(), PyErr>(())
        })?;
        
        self.is_running.store(false, Ordering::SeqCst);
        info!("WebSocket streaming stopped");
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

    /// Reconnect WebSocket with new settings (max_pairs and/or depth changed)
    fn reconnect_websocket(&mut self) -> PyResult<()> {
        info!("Reconnecting WebSocket with new settings...");
        
        // Stop current connection
        self.runtime.block_on(async {
            let mut ws_guard = self.websocket.write();
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
        
        self.runtime.block_on(async {
            let mut ws = KrakenWebSocket::new(Arc::clone(&self.cache));
            ws.set_max_pairs(max_pairs);
            ws.set_orderbook_depth(depth);
            
            // Fetch trading pairs
            ws.initialize().await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to initialize: {}", e)))?;
            
            // Fetch initial prices
            ws.fetch_initial_prices().await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to fetch prices: {}", e)))?;
            
            *self.websocket.write() = Some(ws);
            
            Ok::<(), PyErr>(())
        })?;
        
        // Start streaming with new settings
        self.runtime.block_on(async {
            let mut ws_guard = self.websocket.write();
            if let Some(ws) = ws_guard.as_mut() {
                ws.start(max_pairs, depth).await
                    .map_err(|e| PyRuntimeError::new_err(format!("Failed to start WebSocket: {}", e)))?;
            }
            Ok::<(), PyErr>(())
        })?;
        
        self.is_running.store(true, Ordering::SeqCst);
        
        let (pairs, currencies, _) = self.cache.get_stats();
        info!("WebSocket reconnected with {} pairs, {} currencies, depth={}", pairs, currencies, depth);
        
        Ok(())
    }

    /// Get order book health statistics
    fn get_orderbook_health(&self) -> OrderBookHealth {
        self.dispatcher.get_orderbook_health()
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

    /// String representation
    fn __repr__(&self) -> String {
        format!(
            "TradingEngine(pairs={}, running={})",
            self.cache.get_all_pairs().len(),
            self.is_running.load(Ordering::SeqCst)
        )
    }
}

/// Python module definition
#[pymodule]
fn trading_engine(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<TradingEngine>()?;
    m.add_class::<Opportunity>()?;
    m.add_class::<SlippageResult>()?;
    m.add_class::<crate::types::SlippageLeg>()?;
    m.add_class::<EngineStats>()?;
    m.add_class::<EngineSettings>()?;
    m.add_class::<OrderBookHealth>()?;
    
    Ok(())
}
