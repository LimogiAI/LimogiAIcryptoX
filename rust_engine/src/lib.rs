//! Trading Engine - High-performance Rust extension for KrakenCryptoX
//!
//! This module provides Python bindings via PyO3 for:
//! - Real-time order book streaming via WebSocket
//! - In-memory order book cache
//! - Parallel arbitrage scanning
//! - Slippage calculation
//! - Single balance pool paper trading

mod balance_manager;
mod dispatcher;
mod order_book;
mod scanner;
mod slippage;
mod types;
mod websocket;

use crate::balance_manager::BalanceManager;
use crate::dispatcher::Dispatcher;
use crate::order_book::OrderBookCache;
use crate::types::{EngineConfig, EngineStats, EngineSettings, Opportunity, OrderBookHealth, TradingState, SlippageResult, TradeResult};
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
    balance_manager: Arc<BalanceManager>,
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
        initial_balance=100.0,
        trade_amount=10.0,
        min_profit_threshold=0.0005,
        cooldown_ms=5000,
        max_trades_per_cycle=5,
        fee_rate=0.0026,
        max_pairs=200
    ))]
    fn new(
        initial_balance: f64,
        trade_amount: f64,
        min_profit_threshold: f64,
        cooldown_ms: u64,
        max_trades_per_cycle: usize,
        fee_rate: f64,
        max_pairs: usize,
    ) -> PyResult<Self> {
        // Initialize logging
        let subscriber = FmtSubscriber::builder()
            .with_max_level(Level::INFO)
            .with_target(false)
            .finish();
        let _ = tracing::subscriber::set_global_default(subscriber);
        
        info!("Initializing TradingEngine v2.0...");
        
        let config = EngineConfig {
            trade_amount,
            min_profit_threshold,
            cooldown_ms,
            max_trades_per_cycle,
            // Kill switch defaults - 30% from PEAK balance, NO auto-reset
            kill_switch_enabled: true,
            max_loss_pct: 0.30,            // Stop at 30% loss from peak
            max_consecutive_losses: 10,     // Stop after 10 consecutive losses
            max_daily_loss_pct: 0.30,       // Stop at 30% daily loss from peak
            // Runtime-changeable engine settings
            scan_interval_ms: 10000,        // 10 seconds default
            orderbook_depth: 25,
            max_pairs,
            scanner_enabled: true,          // Scanner ON by default
            // Fixed settings
            initial_balance,
            path_cooldown_ms: 3000,
            fee_rate,
            latency_penalty_pct: 0.001,     // 0.10% per leg default
            staleness_warn_ms: 100,
            staleness_buffer_ms: 250,
            staleness_reject_ms: 1000,
        };
        
        let cache = Arc::new(OrderBookCache::new());
        let balance_manager = Arc::new(BalanceManager::new(config.clone()));
        let dispatcher = Arc::new(Dispatcher::new(
            Arc::clone(&cache),
            Arc::clone(&balance_manager),
        ));
        
        let runtime = Runtime::new()
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create runtime: {}", e)))?;
        
        info!(
            "TradingEngine created: ${:.2} balance, ${:.2} per trade, {} max trades/cycle",
            initial_balance,
            trade_amount,
            max_trades_per_cycle
        );
        
        Ok(Self {
            cache,
            websocket: Arc::new(RwLock::new(None)),
            balance_manager,
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
        let config = self.balance_manager.get_config();
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
        let config = self.balance_manager.get_config();
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

    /// Run a single scan cycle (returns opportunities, doesn't execute)
    fn scan(&self, base_currencies: Vec<String>) -> Vec<Opportunity> {
        let config = self.balance_manager.get_config();
        let scanner = scanner::Scanner::new(Arc::clone(&self.cache), config);
        let opportunities = scanner.scan(&base_currencies);
        
        self.scan_count.fetch_add(1, Ordering::Relaxed);
        self.total_opportunities.fetch_add(opportunities.len() as u64, Ordering::Relaxed);
        
        opportunities
    }

    /// Run a dispatch cycle (scan + execute trades)
    fn run_cycle(&self, base_currencies: Vec<String>) -> Vec<TradeResult> {
        self.scan_count.fetch_add(1, Ordering::Relaxed);
        self.dispatcher.run_cycle(&base_currencies)
    }

    /// Calculate slippage for a path
    fn calculate_slippage(&self, path: String, trade_amount: f64) -> SlippageResult {
        let config = self.balance_manager.get_config();
        let calc = slippage::SlippageCalculator::new(
            Arc::clone(&self.cache),
            config.staleness_warn_ms,
            config.staleness_buffer_ms,
            config.staleness_reject_ms,
        );
        calc.calculate_path(&path, trade_amount)
    }

    /// Update runtime configuration from UI dropdowns
    #[pyo3(signature = (trade_amount=None, min_profit_threshold=None, cooldown_ms=None, max_trades_per_cycle=None, latency_penalty_pct=None, fee_rate=None))]
    fn update_config(
        &self,
        trade_amount: Option<f64>,
        min_profit_threshold: Option<f64>,
        cooldown_ms: Option<u64>,
        max_trades_per_cycle: Option<usize>,
        latency_penalty_pct: Option<f64>,
        fee_rate: Option<f64>,
    ) {
        self.balance_manager.update_config(
            trade_amount,
            min_profit_threshold,
            cooldown_ms,
            max_trades_per_cycle,
            latency_penalty_pct,
            fee_rate,
        );
    }

    /// Get current config values
    fn get_config(&self) -> (f64, f64, u64, usize, f64, f64) {
        let config = self.balance_manager.get_config();
        (
            config.trade_amount,
            config.min_profit_threshold,
            config.cooldown_ms,
            config.max_trades_per_cycle,
            config.latency_penalty_pct,
            config.fee_rate,
        )
    }

    /// Get trading state (balance, trades, win rate)
    fn get_trading_state(&self) -> TradingState {
        self.balance_manager.get_state()
    }

    /// Check if can trade
    fn can_trade(&self) -> bool {
        self.balance_manager.can_trade()
    }

    /// Get current balance
    fn get_balance(&self) -> f64 {
        self.balance_manager.get_balance()
    }

    /// Get total profit
    fn get_total_profit(&self) -> f64 {
        self.balance_manager.get_total_profit()
    }

    /// Get win rate
    fn get_win_rate(&self) -> f64 {
        self.balance_manager.get_win_rate()
    }

    /// Get total trades count
    fn get_total_trades(&self) -> u64 {
        self.balance_manager.get_total_trades()
    }

    /// Reset balance to initial
    fn reset(&self, initial_balance: f64) {
        self.balance_manager.reset(initial_balance);
    }

    // ========== KILL SWITCH METHODS ==========

    /// Check if trading is killed
    fn is_killed(&self) -> bool {
        self.balance_manager.is_killed()
    }

    /// Get kill reason (if killed)
    fn get_kill_reason(&self) -> Option<String> {
        self.balance_manager.get_kill_reason()
    }

    /// Manually trigger kill switch
    fn trigger_kill(&self, reason: String) {
        self.balance_manager.trigger_kill(&reason);
    }

    /// Reset kill switch (allows trading again)
    fn reset_kill_switch(&self) {
        self.balance_manager.reset_kill_switch();
    }

    /// Update kill switch settings
    #[pyo3(signature = (enabled=None, max_loss_pct=None, max_consecutive_losses=None, max_daily_loss_pct=None))]
    fn update_kill_switch(
        &self,
        enabled: Option<bool>,
        max_loss_pct: Option<f64>,
        max_consecutive_losses: Option<u32>,
        max_daily_loss_pct: Option<f64>,
    ) {
        self.balance_manager.update_kill_switch(
            enabled,
            max_loss_pct,
            max_consecutive_losses,
            max_daily_loss_pct,
        );
    }

    /// Get kill switch settings
    fn get_kill_switch_settings(&self) -> (bool, f64, u32, f64) {
        let config = self.balance_manager.get_config();
        (
            config.kill_switch_enabled,
            config.max_loss_pct,
            config.max_consecutive_losses,
            config.max_daily_loss_pct,
        )
    }

    // ========== ENGINE SETTINGS METHODS (Runtime Changeable) ==========

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
        self.balance_manager.update_engine_settings(
            scan_interval_ms,
            max_pairs,
            orderbook_depth,
            scanner_enabled,
        )
    }

    /// Get current engine settings
    fn get_engine_settings(&self) -> EngineSettings {
        let (scan_interval_ms, max_pairs, orderbook_depth, scanner_enabled) = 
            self.balance_manager.get_engine_settings();
        EngineSettings {
            scan_interval_ms,
            max_pairs,
            orderbook_depth,
            scanner_enabled,
        }
    }

    /// Check if scanner is enabled
    fn is_scanner_enabled(&self) -> bool {
        self.balance_manager.is_scanner_enabled()
    }

    /// Reconnect WebSocket with new settings (max_pairs and/or depth changed)
    /// This stops the current connection and starts a new one
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
        
        // Re-initialize with new pair count
        let config = self.balance_manager.get_config();
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

    /// Get locked paths
    fn get_locked_paths(&self) -> Vec<String> {
        self.dispatcher.get_locked_paths()
    }

    /// Get engine statistics
    fn get_stats(&self) -> EngineStats {
        let (pairs, currencies, avg_staleness) = self.cache.get_stats();
        let state = self.balance_manager.get_state();
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
            trades_executed: state.total_trades,
            total_profit: state.total_profit,
            win_rate: state.win_rate,
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
        let state = self.balance_manager.get_state();
        format!(
            "TradingEngine(balance=${:.2}, trades={}, running={})",
            state.balance,
            state.total_trades,
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
    m.add_class::<TradingState>()?;
    m.add_class::<TradeResult>()?;
    m.add_class::<EngineStats>()?;
    m.add_class::<EngineSettings>()?;
    m.add_class::<OrderBookHealth>()?;
    
    Ok(())
}