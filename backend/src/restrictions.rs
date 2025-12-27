//! Geographic Restrictions Module
//!
//! Manages trading restrictions based on jurisdiction (e.g., Canada).
//! Loads from JSON config file and provides API endpoints for management.
#![allow(dead_code)]

use chrono::Utc;
use parking_lot::RwLock;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
use thiserror::Error;
use tracing::{info, warn};

const CONFIG_FILE_PATH: &str = "config/canada_restrictions.json";

#[derive(Debug, Error)]
pub enum RestrictionsError {
    #[error("Failed to read config file: {0}")]
    FileReadError(String),
    #[error("Failed to parse config: {0}")]
    ParseError(String),
    #[error("Failed to write config file: {0}")]
    FileWriteError(String),
    #[error("Failed to fetch from source: {0}")]
    FetchError(String),
    #[error("API error: {0}")]
    ApiError(String),
}

/// Restrictions configuration loaded from JSON
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestrictionsConfig {
    pub version: String,
    pub jurisdiction: String,
    pub jurisdiction_name: String,
    pub last_updated: String,
    pub update_source: String,
    pub regulatory_body: String,
    pub blocked_base_currencies: Vec<String>,
    pub allowed_specified_assets: Vec<String>,
    #[serde(default)]
    pub blocked_pairs: Vec<String>,
    pub notes: String,
    #[serde(default)]
    pub sources: Vec<String>,
}

impl Default for RestrictionsConfig {
    fn default() -> Self {
        // Default is empty - no hardcoded values
        // All restrictions must come from the JSON config file
        Self {
            version: "1.0".to_string(),
            jurisdiction: "CA".to_string(),
            jurisdiction_name: "Canada".to_string(),
            last_updated: Utc::now().to_rfc3339(),
            update_source: "none".to_string(),
            regulatory_body: "".to_string(),
            blocked_base_currencies: vec![], // Empty - no hardcoded restrictions
            allowed_specified_assets: vec![], // Empty - no hardcoded allowlist
            blocked_pairs: vec![],
            notes: "".to_string(),
            sources: vec![],
        }
    }
}

/// Manager for geographic restrictions
pub struct RestrictionsManager {
    config: RwLock<RestrictionsConfig>,
    config_path: String,
    client: Client,
    kraken_api_key: Option<String>,
    kraken_api_secret: Option<String>,
}

impl RestrictionsManager {
    /// Create a new restrictions manager
    /// Attempts to load from file, logs warning if file doesn't exist
    pub fn new(config_path: Option<&str>) -> Self {
        let path = config_path.unwrap_or(CONFIG_FILE_PATH).to_string();
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
            .build()
            .unwrap_or_default();

        // Load API keys from environment
        let kraken_api_key = std::env::var("KRAKEN_API_KEY").ok();
        let kraken_api_secret = std::env::var("KRAKEN_API_SECRET").ok();

        let manager = Self {
            config: RwLock::new(RestrictionsConfig::default()),
            config_path: path.clone(),
            client,
            kraken_api_key,
            kraken_api_secret,
        };

        // Try to load from file - warn if not found (no fallback to hardcoded values)
        if let Err(e) = manager.load_from_file() {
            warn!("Failed to load restrictions from {}: {} - no currencies will be blocked", path, e);
        }

        manager
    }

    /// Create a new restrictions manager, failing if config file doesn't exist
    /// This is the strict version that should be used in production
    pub fn load_or_error(config_path: &str) -> Result<Self, RestrictionsError> {
        let path = Path::new(config_path);

        if !path.exists() {
            return Err(RestrictionsError::FileReadError(
                format!("Config file not found: {}", config_path)
            ));
        }

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
            .build()
            .unwrap_or_default();

        let kraken_api_key = std::env::var("KRAKEN_API_KEY").ok();
        let kraken_api_secret = std::env::var("KRAKEN_API_SECRET").ok();

        let manager = Self {
            config: RwLock::new(RestrictionsConfig::default()),
            config_path: config_path.to_string(),
            client,
            kraken_api_key,
            kraken_api_secret,
        };

        // Load from file - this must succeed
        manager.load_from_file()?;

        Ok(manager)
    }

    /// Load restrictions from JSON config file
    /// Returns error if file doesn't exist (no auto-creation with defaults)
    pub fn load_from_file(&self) -> Result<(), RestrictionsError> {
        let path = Path::new(&self.config_path);

        if !path.exists() {
            return Err(RestrictionsError::FileReadError(
                format!("Restrictions config file not found: {}", self.config_path)
            ));
        }

        let content = std::fs::read_to_string(path)
            .map_err(|e| RestrictionsError::FileReadError(e.to_string()))?;

        let config: RestrictionsConfig = serde_json::from_str(&content)
            .map_err(|e| RestrictionsError::ParseError(e.to_string()))?;

        info!(
            "Loaded restrictions for {} - {} blocked currencies, last updated: {}",
            config.jurisdiction_name,
            config.blocked_base_currencies.len(),
            config.last_updated
        );

        *self.config.write() = config;
        Ok(())
    }

    /// Save current config to JSON file
    pub fn save_to_file(&self) -> Result<(), RestrictionsError> {
        let config = self.config.read().clone();
        let content = serde_json::to_string_pretty(&config)
            .map_err(|e| RestrictionsError::ParseError(e.to_string()))?;

        // Ensure config directory exists
        if let Some(parent) = Path::new(&self.config_path).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| RestrictionsError::FileWriteError(e.to_string()))?;
        }

        std::fs::write(&self.config_path, content)
            .map_err(|e| RestrictionsError::FileWriteError(e.to_string()))?;

        info!("Saved restrictions config to {}", self.config_path);
        Ok(())
    }

    /// Get current restrictions config
    pub fn get_config(&self) -> RestrictionsConfig {
        self.config.read().clone()
    }

    /// Get blocked base currencies list
    pub fn get_blocked_currencies(&self) -> Vec<String> {
        self.config.read().blocked_base_currencies.clone()
    }

    /// Get allowed specified assets list
    pub fn get_allowed_assets(&self) -> Vec<String> {
        self.config.read().allowed_specified_assets.clone()
    }

    /// Check if a currency is blocked
    pub fn is_currency_blocked(&self, currency: &str) -> bool {
        self.config.read().blocked_base_currencies.contains(&currency.to_uppercase())
    }

    /// Check if a currency is an allowed specified asset
    pub fn is_allowed_specified_asset(&self, currency: &str) -> bool {
        self.config.read().allowed_specified_assets.contains(&currency.to_uppercase())
    }

    /// Update restrictions manually
    pub fn update_restrictions(
        &self,
        blocked_currencies: Vec<String>,
        allowed_assets: Option<Vec<String>>,
        source: &str,
    ) -> Result<(), RestrictionsError> {
        {
            let mut config = self.config.write();
            config.blocked_base_currencies = blocked_currencies;
            if let Some(assets) = allowed_assets {
                config.allowed_specified_assets = assets;
            }
            config.last_updated = Utc::now().to_rfc3339();
            config.update_source = source.to_string();
        }
        self.save_to_file()
    }

    /// Try to refresh restrictions from Kraken API
    /// This attempts to fetch asset info and determine which are tradeable in Canada
    pub async fn refresh_from_kraken(&self) -> Result<RefreshResult, RestrictionsError> {
        info!("Attempting to refresh restrictions from Kraken API...");

        // Kraken's public API endpoint for asset pairs
        let url = "https://api.kraken.com/0/public/AssetPairs";

        let response = self.client
            .get(url)
            .send()
            .await
            .map_err(|e| RestrictionsError::FetchError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(RestrictionsError::FetchError(
                format!("HTTP {}", response.status())
            ));
        }

        let data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| RestrictionsError::ParseError(e.to_string()))?;

        // Check for API errors
        if let Some(errors) = data.get("error").and_then(|e| e.as_array()) {
            if !errors.is_empty() {
                let error_msg: Vec<String> = errors
                    .iter()
                    .filter_map(|e| e.as_str().map(String::from))
                    .collect();
                return Err(RestrictionsError::ApiError(error_msg.join(", ")));
            }
        }

        // Extract pairs and look for patterns
        let result = data.get("result").and_then(|r| r.as_object());

        if let Some(pairs) = result {
            let total_pairs = pairs.len();

            // Use current config's blocked/allowed currencies (from JSON file, no hardcoding)
            let current_config = self.config.read().clone();
            let configured_blocked = &current_config.blocked_base_currencies;
            let configured_allowed = &current_config.allowed_specified_assets;

            // Check which of our configured currencies exist on Kraken
            let mut found_blocked: Vec<String> = Vec::new();
            let mut found_allowed: Vec<String> = Vec::new();
            let mut not_found_blocked: Vec<String> = Vec::new();
            let mut not_found_allowed: Vec<String> = Vec::new();

            for (pair_id, _pair_info) in pairs {
                for blocked in configured_blocked {
                    if pair_id.contains(blocked) && !found_blocked.contains(blocked) {
                        found_blocked.push(blocked.clone());
                    }
                }
                for allowed in configured_allowed {
                    if pair_id.contains(allowed) && !found_allowed.contains(allowed) {
                        found_allowed.push(allowed.clone());
                    }
                }
            }

            // Check which configured currencies were NOT found on Kraken
            for blocked in configured_blocked {
                if !found_blocked.contains(blocked) {
                    not_found_blocked.push(blocked.clone());
                }
            }
            for allowed in configured_allowed {
                if !found_allowed.contains(allowed) {
                    not_found_allowed.push(allowed.clone());
                }
            }

            info!(
                "Kraken API scan: {} total pairs. Blocked: {}/{} found. Allowed: {}/{} found.",
                total_pairs,
                found_blocked.len(), configured_blocked.len(),
                found_allowed.len(), configured_allowed.len()
            );

            if !not_found_blocked.is_empty() {
                warn!("Blocked currencies not found on Kraken: {:?}", not_found_blocked);
            }
            if !not_found_allowed.is_empty() {
                warn!("Allowed currencies not found on Kraken: {:?}", not_found_allowed);
            }

            // Update last_updated timestamp but don't change the lists
            // (Lists should only be changed via add/remove or manual JSON edit)
            {
                let mut config = self.config.write();
                config.last_updated = Utc::now().to_rfc3339();
                config.update_source = "kraken_api_verified".to_string();
            }

            self.save_to_file()?;

            Ok(RefreshResult {
                success: true,
                source: "kraken_api".to_string(),
                blocked_currencies: found_blocked.clone(),
                allowed_assets: found_allowed.clone(),
                message: format!(
                    "Verified against Kraken API ({} pairs). Blocked: {}/{} exist. Allowed: {}/{} exist.{}{}",
                    total_pairs,
                    found_blocked.len(), configured_blocked.len(),
                    found_allowed.len(), configured_allowed.len(),
                    if !not_found_blocked.is_empty() { format!(" Not found blocked: {:?}.", not_found_blocked) } else { String::new() },
                    if !not_found_allowed.is_empty() { format!(" Not found allowed: {:?}.", not_found_allowed) } else { String::new() }
                ),
            })
        } else {
            Err(RestrictionsError::ParseError("No result in API response".to_string()))
        }
    }

    /// Add a currency to blocked list
    pub fn add_blocked_currency(&self, currency: &str) -> Result<(), RestrictionsError> {
        let currency = currency.to_uppercase();
        {
            let mut config = self.config.write();
            if !config.blocked_base_currencies.contains(&currency) {
                config.blocked_base_currencies.push(currency.clone());
                config.last_updated = Utc::now().to_rfc3339();
                config.update_source = "manual_add".to_string();
            }
        }
        self.save_to_file()?;
        info!("Added {} to blocked currencies", currency);
        Ok(())
    }

    /// Remove a currency from blocked list
    pub fn remove_blocked_currency(&self, currency: &str) -> Result<(), RestrictionsError> {
        let currency = currency.to_uppercase();
        {
            let mut config = self.config.write();
            config.blocked_base_currencies.retain(|c| c != &currency);
            config.last_updated = Utc::now().to_rfc3339();
            config.update_source = "manual_remove".to_string();
        }
        self.save_to_file()?;
        info!("Removed {} from blocked currencies", currency);
        Ok(())
    }
}

/// Result of a refresh operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshResult {
    pub success: bool,
    pub source: String,
    pub blocked_currencies: Vec<String>,
    pub allowed_assets: Vec<String>,
    pub message: String,
}

/// API response types
#[derive(Debug, Serialize, Deserialize)]
pub struct RestrictionsResponse {
    pub success: bool,
    pub data: RestrictionsConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateRequest {
    pub blocked_currencies: Option<Vec<String>>,
    pub allowed_assets: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AddRemoveRequest {
    pub currency: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_is_empty() {
        // Default config should have empty lists - no hardcoded values
        let config = RestrictionsConfig::default();
        assert_eq!(config.jurisdiction, "CA");
        assert!(config.blocked_base_currencies.is_empty(), "Default should have no blocked currencies");
        assert!(config.allowed_specified_assets.is_empty(), "Default should have no allowed assets");
    }

    #[test]
    fn test_is_currency_blocked_empty_default() {
        // When no config file exists, manager starts with empty blocked list
        let manager = RestrictionsManager::new(None);
        // With empty defaults, nothing should be blocked
        assert!(!manager.is_currency_blocked("USDT"), "No hardcoded blocks");
        assert!(!manager.is_currency_blocked("BTC"), "No hardcoded blocks");
    }

    #[test]
    fn test_is_currency_blocked_case_insensitive() {
        let manager = RestrictionsManager::new(None);
        // Manually add a blocked currency
        let _ = manager.add_blocked_currency("TEST");
        assert!(manager.is_currency_blocked("TEST"));
        assert!(manager.is_currency_blocked("test")); // Case insensitive
        assert!(manager.is_currency_blocked("Test")); // Case insensitive
    }

    #[test]
    fn test_add_remove_blocked_currency() {
        let manager = RestrictionsManager::new(None);
        // Start empty
        assert!(!manager.is_currency_blocked("XYZ"));
        // Add
        let _ = manager.add_blocked_currency("XYZ");
        assert!(manager.is_currency_blocked("XYZ"));
        // Remove
        let _ = manager.remove_blocked_currency("XYZ");
        assert!(!manager.is_currency_blocked("XYZ"));
    }
}
