//! Config manager - stores engine configuration settings
//! 
//! This module manages runtime-configurable settings for the scanning engine.
//! All trading execution is handled by the Python live trading system.

use crate::types::EngineConfig;
use parking_lot::RwLock;
use tracing::info;

/// Manages engine configuration
pub struct ConfigManager {
    config: RwLock<EngineConfig>,
}

impl ConfigManager {
    pub fn new(config: EngineConfig) -> Self {
        info!("Initialized config manager");
        info!("Scan interval: {}ms, Max pairs: {}, Orderbook depth: {}", 
            config.scan_interval_ms, config.max_pairs, config.orderbook_depth);
        
        Self {
            config: RwLock::new(config),
        }
    }

    /// Update scanning configuration from UI
    pub fn update_config(
        &self,
        min_profit_threshold: Option<f64>,
        fee_rate: Option<f64>,
    ) {
        let mut config = self.config.write();
        
        if let Some(threshold) = min_profit_threshold {
            config.min_profit_threshold = threshold;
            info!("Updated min profit threshold to {:.4}%", threshold * 100.0);
        }
        if let Some(fee) = fee_rate {
            config.fee_rate = fee;
            info!("Updated fee rate to {:.2}%", fee * 100.0);
        }
    }

    /// Update engine settings (scan interval, max pairs, depth, scanner on/off)
    /// Returns true if WebSocket reconnection is needed (pairs or depth changed)
    pub fn update_engine_settings(
        &self,
        scan_interval_ms: Option<u64>,
        max_pairs: Option<usize>,
        orderbook_depth: Option<usize>,
        scanner_enabled: Option<bool>,
    ) -> bool {
        let mut config = self.config.write();
        let mut needs_reconnect = false;
        
        if let Some(interval) = scan_interval_ms {
            config.scan_interval_ms = interval;
            info!("Updated scan interval to {}ms", interval);
        }
        
        if let Some(pairs) = max_pairs {
            if pairs != config.max_pairs {
                config.max_pairs = pairs;
                needs_reconnect = true;
                info!("Updated max pairs to {} (reconnection required)", pairs);
            }
        }
        
        if let Some(depth) = orderbook_depth {
            if depth != config.orderbook_depth {
                config.orderbook_depth = depth;
                needs_reconnect = true;
                info!("Updated orderbook depth to {} (reconnection required)", depth);
            }
        }
        
        if let Some(enabled) = scanner_enabled {
            config.scanner_enabled = enabled;
            info!("Scanner {}", if enabled { "ENABLED" } else { "DISABLED" });
        }
        
        needs_reconnect
    }

    /// Get current engine settings
    pub fn get_engine_settings(&self) -> (u64, usize, usize, bool) {
        let config = self.config.read();
        (
            config.scan_interval_ms,
            config.max_pairs,
            config.orderbook_depth,
            config.scanner_enabled,
        )
    }

    /// Check if scanner is enabled
    pub fn is_scanner_enabled(&self) -> bool {
        self.config.read().scanner_enabled
    }

    /// Get current configuration
    pub fn get_config(&self) -> EngineConfig {
        self.config.read().clone()
    }

    /// Get min profit threshold
    pub fn get_min_profit_threshold(&self) -> f64 {
        self.config.read().min_profit_threshold
    }

    /// Get fee rate
    pub fn get_fee_rate(&self) -> f64 {
        self.config.read().fee_rate
    }
}
