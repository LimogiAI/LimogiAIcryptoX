//! Config manager - stores engine configuration settings
//!
//! This module manages runtime-configurable settings for the HFT scanning engine.
//! Primary settings: fee rates and min profit threshold.

use crate::types::EngineConfig;
use parking_lot::RwLock;
use tracing::info;

/// Manages engine configuration
pub struct ConfigManager {
    config: RwLock<EngineConfig>,
}

impl ConfigManager {
    pub fn new(config: EngineConfig) -> Self {
        info!(
            "ConfigManager initialized: fee_rate={:.2}% ({}), min_profit={:.4}%",
            config.fee_rate * 100.0,
            config.fee_source,
            config.min_profit_threshold * 100.0
        );

        Self {
            config: RwLock::new(config),
        }
    }

    /// Update min profit threshold
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
            config.fee_source = "live".to_string();
            info!("Updated fee rate to {:.2}% (source: live)", fee * 100.0);
        }
    }

    /// Update fee rate with explicit source tracking
    pub fn update_fee_rate(&self, fee_rate: f64, source: &str) {
        let mut config = self.config.write();
        config.fee_rate = fee_rate;
        config.fee_source = source.to_string();
        info!("Updated fee rate to {:.2}% (source: {})", fee_rate * 100.0, source);
    }

    /// Get current configuration
    pub fn get_config(&self) -> EngineConfig {
        self.config.read().clone()
    }
}
