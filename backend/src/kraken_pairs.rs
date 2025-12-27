//! Kraken Pair Selector Module
//!
//! Intelligently selects high-liquidity trading pairs for HFT triangular arbitrage.
//! Optimized for Ontario, Canada regulatory requirements.
//!
//! Selection Criteria:
//! 1. Status: "online" (actively trading)
//! 2. Quote currencies: USD, EUR only
//! 3. No dark pools (.d suffix)
//! 4. Cost minimum <= $20 (for small trades)
//! 5. Sorted by 24h USD volume
//! 6. Validated for triangular arbitrage paths
#![allow(dead_code)]

use crate::restrictions::RestrictionsManager;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, info, warn};

/// Get Kraken REST API URL from environment or use default
fn get_kraken_rest_url() -> String {
    std::env::var("KRAKEN_REST_URL")
        .unwrap_or_else(|_| "https://api.kraken.com".to_string())
}

/// Get restrictions config path from environment or use default
fn get_restrictions_config_path() -> String {
    std::env::var("RESTRICTIONS_CONFIG_PATH")
        .unwrap_or_else(|_| "config/canada_restrictions.json".to_string())
}

/// Get AssetPairs API path from environment or use default
fn get_asset_pairs_path() -> String {
    std::env::var("KRAKEN_ASSET_PAIRS_PATH")
        .unwrap_or_else(|_| "/0/public/AssetPairs".to_string())
}

/// Get Ticker API path from environment or use default
fn get_ticker_path() -> String {
    std::env::var("KRAKEN_TICKER_PATH")
        .unwrap_or_else(|_| "/0/public/Ticker".to_string())
}

/// Errors that can occur during pair selection
#[derive(Debug, Error)]
pub enum PairSelectionError {
    #[error("HTTP request failed: {0}")]
    RequestError(#[from] reqwest::Error),
    #[error("Failed to parse API response: {0}")]
    ParseError(String),
    #[error("Kraken API error: {0}")]
    ApiError(String),
    #[error("No valid pairs found after filtering")]
    NoPairsFound,
}

/// Configuration for pair selection
/// NOTE: All values MUST be explicitly set - no hardcoded defaults
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairSelectionConfig {
    /// Maximum number of pairs to select - REQUIRED, user must configure
    pub max_pairs: Option<usize>,
    /// Minimum 24h USD volume required - REQUIRED, user must configure
    pub min_volume_24h_usd: Option<f64>,
    /// Maximum cost minimum for a pair in USD - REQUIRED
    pub max_cost_min: Option<f64>,
    /// Allowed quote currencies (from user's start currency selection on dashboard)
    /// This is the currency user starts/ends with in triangular arbitrage
    /// Empty = not configured, user MUST call set_start_currency() before selecting pairs
    pub allowed_quote_currencies: Vec<String>,
    /// Blocked base currencies (loaded from config/canada_restrictions.json)
    pub blocked_base_currencies: Vec<String>,
}

impl Default for PairSelectionConfig {
    fn default() -> Self {
        // Try to load blocked currencies from JSON config
        let blocked_currencies = Self::load_blocked_currencies_from_config();

        Self {
            // All None = not configured, user MUST set these values
            max_pairs: None,
            min_volume_24h_usd: None,
            max_cost_min: None,
            // Empty = not configured. User MUST call set_start_currency() before engine start.
            allowed_quote_currencies: vec![],
            blocked_base_currencies: blocked_currencies,
        }
    }
}

impl PairSelectionConfig {
    /// Check if the config is valid and ready for pair selection
    pub fn is_configured(&self) -> bool {
        self.max_pairs.is_some()
            && self.min_volume_24h_usd.is_some()
            && self.max_cost_min.is_some()
            && !self.allowed_quote_currencies.is_empty()
    }

    /// Get validation error message if config is not ready
    pub fn validate(&self) -> Result<(), String> {
        if self.max_pairs.is_none() {
            return Err("max_pairs not configured. Please set maximum pairs to monitor.".to_string());
        }
        if self.min_volume_24h_usd.is_none() {
            return Err("min_volume_24h_usd not configured. Please set minimum 24h volume.".to_string());
        }
        if self.max_cost_min.is_none() {
            return Err("max_cost_min not configured. Please set maximum cost minimum.".to_string());
        }
        if self.allowed_quote_currencies.is_empty() {
            return Err("Start currency not configured. Please select at least one currency from the dashboard.".to_string());
        }
        Ok(())
    }

    /// Get max_pairs or return error
    pub fn get_max_pairs(&self) -> Result<usize, String> {
        self.max_pairs.ok_or_else(|| "max_pairs not configured".to_string())
    }

    /// Get min_volume_24h_usd or return error
    pub fn get_min_volume(&self) -> Result<f64, String> {
        self.min_volume_24h_usd.ok_or_else(|| "min_volume_24h_usd not configured".to_string())
    }

    /// Get max_cost_min or return error
    pub fn get_max_cost_min(&self) -> Result<f64, String> {
        self.max_cost_min.ok_or_else(|| "max_cost_min not configured".to_string())
    }
}

impl PairSelectionConfig {
    /// Load blocked currencies from the restrictions JSON config file
    /// Returns empty list if no blocked currencies are configured (no fallbacks)
    fn load_blocked_currencies_from_config() -> Vec<String> {
        let config_path = get_restrictions_config_path();
        match RestrictionsManager::load_or_error(&config_path) {
            Ok(manager) => {
                let blocked = manager.get_blocked_currencies();
                if blocked.is_empty() {
                    info!("No blocked currencies configured - all currencies allowed");
                } else {
                    info!("Loaded {} blocked currencies from config: {:?}", blocked.len(), blocked);
                }
                blocked
            }
            Err(e) => {
                panic!("Failed to load restrictions config from {}: {}. Please ensure the config file exists.",
                       config_path, e);
            }
        }
    }

    /// Create config with a specific RestrictionsManager
    /// NOTE: Still requires user to set max_pairs, min_volume, max_cost_min, and start_currency
    pub fn with_restrictions_manager(restrictions_manager: &RestrictionsManager) -> Self {
        let blocked = restrictions_manager.get_blocked_currencies();
        info!("Using {} blocked currencies from RestrictionsManager", blocked.len());

        Self {
            // All None - user MUST configure these
            max_pairs: None,
            min_volume_24h_usd: None,
            max_cost_min: None,
            // Empty - user MUST set start currency
            allowed_quote_currencies: vec![],
            blocked_base_currencies: blocked,
        }
    }

    /// Set pair selection parameters
    /// These can be exposed to user via dashboard or set to reasonable system limits
    pub fn set_pair_selection_params(&mut self, max_pairs: usize, min_volume: f64, max_cost: f64) {
        self.max_pairs = Some(max_pairs);
        self.min_volume_24h_usd = Some(min_volume);
        self.max_cost_min = Some(max_cost);
        info!("Pair selection params set: max_pairs={}, min_volume=${}, max_cost=${}",
              max_pairs, min_volume, max_cost);
    }

    /// Update blocked currencies (for runtime updates)
    pub fn update_blocked_currencies(&mut self, blocked: Vec<String>) {
        info!("Updating blocked currencies: {:?}", blocked);
        self.blocked_base_currencies = blocked;
    }

    /// Set the start currency for triangular arbitrage
    /// This is the currency user starts and ends with (their capital currency)
    /// In trading pair terms, this becomes the QUOTE currency filter
    ///
    /// Parses the base_currency string from trading config:
    /// - "USD" -> trades BTC/USD, ETH/USD, etc.
    /// - "USD,EUR,GBP" -> trades pairs ending in any of these currencies
    /// - "" (empty) -> not configured, will fail validation
    ///
    /// NOTE: No hardcoded defaults - user must explicitly select currencies from dashboard
    pub fn set_start_currency(&mut self, start_currency: &str) {
        let currency = start_currency.trim().to_uppercase();

        if currency.is_empty() {
            // Not configured - leave empty, will fail validation
            self.allowed_quote_currencies = vec![];
            warn!("Start currency not set - user must configure before starting engine");
            return;
        }

        // Parse comma-separated list or single currency
        // No "ALL" shortcut - user must explicitly select each currency they want
        self.allowed_quote_currencies = currency
            .split(',')
            .map(|s| s.trim().to_uppercase())
            .filter(|s| !s.is_empty())
            .collect();

        if self.allowed_quote_currencies.is_empty() {
            warn!("No valid currencies parsed from '{}' - user must configure", start_currency);
        } else {
            info!("Start currencies set: {:?} -> trading pairs with these quote currencies",
                  self.allowed_quote_currencies);
        }
    }
}

/// A selected trading pair with all relevant metadata
#[derive(Debug, Clone)]
pub struct SelectedPair {
    /// Normalized pair name (e.g., "BTC/USD")
    pub pair_name: String,
    /// Base currency (e.g., "BTC")
    pub base: String,
    /// Quote currency (e.g., "USD")
    pub quote: String,
    /// Kraken's internal ID (e.g., "XXBTZUSD")
    pub kraken_id: String,
    /// WebSocket pair name (e.g., "XBT/USD")
    pub ws_name: String,
    /// 24-hour volume in USD equivalent
    pub volume_24h_usd: f64,
    /// Minimum order size
    pub ordermin: f64,
    /// Minimum order cost in quote currency
    pub costmin: f64,
}

/// Kraken pair selector for HFT arbitrage
pub struct KrakenPairSelector {
    config: PairSelectionConfig,
    client: Client,
}

impl KrakenPairSelector {
    /// Create a new pair selector with the given configuration
    pub fn new(config: PairSelectionConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap_or_default();

        Self { config, client }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(PairSelectionConfig::default())
    }

    /// Select the best trading pairs for arbitrage
    /// FAILS if configuration is incomplete
    pub async fn select_pairs(&self) -> Result<Vec<SelectedPair>, PairSelectionError> {
        // Validate configuration first - fail early if not configured
        self.config.validate().map_err(|e| PairSelectionError::ApiError(e))?;

        let max_pairs = self.config.get_max_pairs()
            .map_err(|e| PairSelectionError::ApiError(e))?;

        info!("Starting pair selection (max: {}, quote currencies: {:?})",
              max_pairs, self.config.allowed_quote_currencies);

        // Step 1: Fetch all asset pairs from Kraken
        let all_pairs = self.fetch_asset_pairs().await?;
        info!("Fetched {} total pairs from Kraken", all_pairs.len());

        // Step 2: Apply initial filters
        let filtered_pairs = self.apply_filters(all_pairs)?;
        info!("After filtering: {} pairs remain", filtered_pairs.len());

        if filtered_pairs.is_empty() {
            return Err(PairSelectionError::NoPairsFound);
        }

        // Step 3: Fetch 24h volumes for filtered pairs
        let pairs_with_volume = self.fetch_volumes(filtered_pairs).await?;
        info!("Fetched volumes for {} pairs", pairs_with_volume.len());

        // Step 4: Sort by volume and take top N
        let mut sorted_pairs = pairs_with_volume;
        sorted_pairs.sort_by(|a, b| b.volume_24h_usd.partial_cmp(&a.volume_24h_usd).unwrap());

        // Step 5: Validate triangular paths and select final pairs
        let validated_pairs = self.validate_triangular_paths(sorted_pairs);
        info!("After triangular validation: {} pairs", validated_pairs.len());

        // Step 6: Take top N pairs
        let final_pairs: Vec<SelectedPair> = validated_pairs
            .into_iter()
            .take(max_pairs)
            .collect();

        info!("Selected {} pairs for trading:", final_pairs.len());
        for (i, pair) in final_pairs.iter().enumerate() {
            info!("  {}. {} - ${:.0} 24h volume", i + 1, pair.pair_name, pair.volume_24h_usd);
        }

        Ok(final_pairs)
    }

    /// Fetch all asset pairs from Kraken REST API
    async fn fetch_asset_pairs(&self) -> Result<Vec<RawPairInfo>, PairSelectionError> {
        let url = format!("{}{}", get_kraken_rest_url(), get_asset_pairs_path());
        let response = self.client.get(&url).send().await?;
        let data: Value = response.json().await?;

        // Check for API errors
        if let Some(errors) = data.get("error").and_then(|e| e.as_array()) {
            if !errors.is_empty() {
                let error_msg: Vec<String> = errors
                    .iter()
                    .filter_map(|e| e.as_str().map(String::from))
                    .collect();
                return Err(PairSelectionError::ApiError(error_msg.join(", ")));
            }
        }

        let result = data.get("result")
            .ok_or_else(|| PairSelectionError::ParseError("No result in response".to_string()))?;

        let pairs_obj = result.as_object()
            .ok_or_else(|| PairSelectionError::ParseError("Result is not an object".to_string()))?;

        let mut pairs = Vec::new();
        for (kraken_id, pair_info) in pairs_obj {
            if let Some(raw_pair) = self.parse_pair_info(kraken_id, pair_info) {
                pairs.push(raw_pair);
            }
        }

        Ok(pairs)
    }

    /// Parse pair info from Kraken API response
    fn parse_pair_info(&self, kraken_id: &str, info: &Value) -> Option<RawPairInfo> {
        let altname = info.get("altname")?.as_str()?;
        let wsname = info.get("wsname").and_then(|v| v.as_str())?;
        let status = info.get("status").and_then(|v| v.as_str()).unwrap_or("online");
        let base_raw = info.get("base")?.as_str()?;
        let quote_raw = info.get("quote")?.as_str()?;
        let ordermin = info.get("ordermin")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let costmin = info.get("costmin")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);

        Some(RawPairInfo {
            kraken_id: kraken_id.to_string(),
            altname: altname.to_string(),
            ws_name: wsname.to_string(),
            base: self.normalize_currency(base_raw),
            quote: self.normalize_currency(quote_raw),
            status: status.to_string(),
            ordermin,
            costmin,
        })
    }

    /// Apply all filters to raw pairs
    /// Returns error if configuration is incomplete
    fn apply_filters(&self, pairs: Vec<RawPairInfo>) -> Result<Vec<RawPairInfo>, PairSelectionError> {
        let max_cost_min = self.config.get_max_cost_min()
            .map_err(|e| PairSelectionError::ApiError(e))?;

        let filtered: Vec<RawPairInfo> = pairs
            .into_iter()
            .filter(|p| {
                // Filter 1: Status must be "online"
                if p.status != "online" {
                    debug!("Filtered out {} - status: {}", p.altname, p.status);
                    return false;
                }

                // Filter 2: No dark pools (.d suffix)
                if p.altname.ends_with(".d") {
                    debug!("Filtered out {} - dark pool", p.altname);
                    return false;
                }

                // Filter 3: Quote currency must be in allowed list
                if !self.config.allowed_quote_currencies.contains(&p.quote) {
                    debug!("Filtered out {} - quote {} not in allowed list", p.altname, p.quote);
                    return false;
                }

                // Filter 4: Cost minimum must be <= max_cost_min
                if p.costmin > max_cost_min {
                    debug!("Filtered out {} - costmin {} > {}", p.altname, p.costmin, max_cost_min);
                    return false;
                }

                // Filter 5: Base currency must not be in blocked list (loaded from config/canada_restrictions.json)
                if self.config.blocked_base_currencies.contains(&p.base) {
                    debug!("Filtered out {} - base {} is blocked", p.altname, p.base);
                    return false;
                }

                true
            })
            .collect();

        Ok(filtered)
    }

    /// Fetch 24h volumes for pairs
    async fn fetch_volumes(&self, pairs: Vec<RawPairInfo>) -> Result<Vec<SelectedPair>, PairSelectionError> {
        let mut result = Vec::new();

        let min_volume = self.config.get_min_volume()
            .map_err(|e| PairSelectionError::ApiError(e))?;

        // Get EUR/USD rate for volume conversion - REQUIRED, no fallback
        let eur_usd_rate = self.fetch_eur_usd_rate().await?;
        info!("Using EUR/USD rate: {:.4}", eur_usd_rate);

        // Fetch in chunks of 100 (Kraken rate limit)
        let kraken_ids: Vec<String> = pairs.iter().map(|p| p.kraken_id.clone()).collect();

        for chunk in kraken_ids.chunks(100) {
            let pair_param = chunk.join(",");
            let url = format!("{}{}?pair={}", get_kraken_rest_url(), get_ticker_path(), pair_param);

            let response = self.client.get(&url).send().await?;
            let data: Value = response.json().await?;

            if let Some(result_obj) = data.get("result").and_then(|r| r.as_object()) {
                for (kraken_id, ticker) in result_obj {
                    // Find the matching pair info
                    if let Some(pair_info) = pairs.iter().find(|p| &p.kraken_id == kraken_id) {
                        // Extract 24h volume: v[1] is 24h volume
                        let volume_base = ticker.get("v")
                            .and_then(|v| v.get(1))
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<f64>().ok())
                            .unwrap_or(0.0);

                        // Get last price for USD conversion
                        let last_price = ticker.get("c")
                            .and_then(|c| c.get(0))
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<f64>().ok())
                            .unwrap_or(0.0);

                        // Calculate USD-equivalent volume
                        let volume_quote = volume_base * last_price;
                        let volume_usd = if pair_info.quote == "EUR" {
                            volume_quote * eur_usd_rate
                        } else {
                            volume_quote
                        };

                        // Only include if volume meets minimum
                        if volume_usd >= min_volume {
                            // Use Kraken's original wsname - it's already correct for WebSocket v2
                            // Just normalize XBT -> BTC in the wsname if needed
                            let ws_name = pair_info.ws_name
                                .replace("XBT", "BTC")
                                .replace("XXBT", "BTC");

                            result.push(SelectedPair {
                                pair_name: format!("{}/{}", pair_info.base, pair_info.quote),
                                base: pair_info.base.clone(),
                                quote: pair_info.quote.clone(),
                                kraken_id: pair_info.kraken_id.clone(),
                                ws_name,
                                volume_24h_usd: volume_usd,
                                ordermin: pair_info.ordermin,
                                costmin: pair_info.costmin,
                            });
                        }
                    }
                }
            }

            // Rate limiting - 100ms between requests
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        Ok(result)
    }

    /// Fetch EUR/USD exchange rate from Kraken
    /// FAILS if rate cannot be fetched - no fallback/default values
    async fn fetch_eur_usd_rate(&self) -> Result<f64, PairSelectionError> {
        let url = format!("{}{}?pair=EURUSD", get_kraken_rest_url(), get_ticker_path());
        let response = self.client.get(&url).send().await?;
        let data: Value = response.json().await?;

        // Check for API errors
        if let Some(errors) = data.get("error").and_then(|e| e.as_array()) {
            if !errors.is_empty() {
                let error_msg: Vec<String> = errors
                    .iter()
                    .filter_map(|e| e.as_str().map(String::from))
                    .collect();
                return Err(PairSelectionError::ApiError(
                    format!("Failed to fetch EUR/USD rate: {}", error_msg.join(", "))
                ));
            }
        }

        let rate = data.get("result")
            .and_then(|r| r.get("ZEURZUSD").or_else(|| r.get("EURUSD")))
            .and_then(|t| t.get("c"))
            .and_then(|c| c.get(0))
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .ok_or_else(|| PairSelectionError::ParseError(
                "Failed to parse EUR/USD rate from Kraken API response".to_string()
            ))?;

        if rate <= 0.0 {
            return Err(PairSelectionError::ParseError(
                "Invalid EUR/USD rate from Kraken API".to_string()
            ));
        }

        Ok(rate)
    }

    /// Validate that selected pairs form valid triangular arbitrage paths
    fn validate_triangular_paths(&self, pairs: Vec<SelectedPair>) -> Vec<SelectedPair> {
        // Build a graph of available trading paths
        let mut currency_pairs: HashMap<String, HashSet<String>> = HashMap::new();
        let mut pair_lookup: HashMap<(String, String), SelectedPair> = HashMap::new();

        for pair in &pairs {
            // Add both directions to the graph
            currency_pairs
                .entry(pair.base.clone())
                .or_default()
                .insert(pair.quote.clone());
            currency_pairs
                .entry(pair.quote.clone())
                .or_default()
                .insert(pair.base.clone());

            pair_lookup.insert((pair.base.clone(), pair.quote.clone()), pair.clone());
            pair_lookup.insert((pair.quote.clone(), pair.base.clone()), pair.clone());
        }

        // Find pairs that participate in at least one triangular path
        let mut valid_pairs: HashSet<String> = HashSet::new();

        for quote in &self.config.allowed_quote_currencies {
            // For each quote currency (USD, EUR), find triangles: QUOTE -> A -> B -> QUOTE
            if let Some(first_hops) = currency_pairs.get(quote) {
                for a in first_hops {
                    if a == quote {
                        continue;
                    }
                    if let Some(second_hops) = currency_pairs.get(a) {
                        for b in second_hops {
                            if b == quote || b == a {
                                continue;
                            }
                            // Check if B connects back to QUOTE
                            if let Some(third_hops) = currency_pairs.get(b) {
                                if third_hops.contains(quote) {
                                    // Valid triangle found: QUOTE -> A -> B -> QUOTE
                                    debug!("Found triangle: {} -> {} -> {} -> {}", quote, a, b, quote);

                                    // Mark all pairs in this triangle as valid
                                    if let Some(p) = pair_lookup.get(&(quote.clone(), a.clone())) {
                                        valid_pairs.insert(p.pair_name.clone());
                                    }
                                    if let Some(p) = pair_lookup.get(&(a.clone(), quote.clone())) {
                                        valid_pairs.insert(p.pair_name.clone());
                                    }
                                    if let Some(p) = pair_lookup.get(&(a.clone(), b.clone())) {
                                        valid_pairs.insert(p.pair_name.clone());
                                    }
                                    if let Some(p) = pair_lookup.get(&(b.clone(), a.clone())) {
                                        valid_pairs.insert(p.pair_name.clone());
                                    }
                                    if let Some(p) = pair_lookup.get(&(b.clone(), quote.clone())) {
                                        valid_pairs.insert(p.pair_name.clone());
                                    }
                                    if let Some(p) = pair_lookup.get(&(quote.clone(), b.clone())) {
                                        valid_pairs.insert(p.pair_name.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        info!("Found {} pairs participating in triangular paths", valid_pairs.len());

        // Return only pairs that are in valid triangles, maintaining volume order
        pairs
            .into_iter()
            .filter(|p| valid_pairs.contains(&p.pair_name))
            .collect()
    }

    /// Normalize Kraken currency symbols to standard format
    fn normalize_currency(&self, symbol: &str) -> String {
        match symbol {
            "XXBT" | "XBT" => "BTC".to_string(),
            "XETH" => "ETH".to_string(),
            "ZUSD" => "USD".to_string(),
            "ZEUR" => "EUR".to_string(),
            "ZCAD" => "CAD".to_string(),
            "ZGBP" => "GBP".to_string(),
            "ZJPY" => "JPY".to_string(),
            "XXRP" => "XRP".to_string(),
            "XXLM" => "XLM".to_string(),
            "XLTC" => "LTC".to_string(),
            "XXMR" => "XMR".to_string(),
            "XXDG" | "XDG" => "DOGE".to_string(),
            "XETC" => "ETC".to_string(),
            "XZEC" => "ZEC".to_string(),
            s if s.starts_with('X') || s.starts_with('Z') => s[1..].to_string(),
            s => s.to_string(),
        }
    }

    /// Get the current configuration
    pub fn config(&self) -> &PairSelectionConfig {
        &self.config
    }
}

/// Internal struct for raw pair info before volume filtering
#[derive(Debug)]
struct RawPairInfo {
    kraken_id: String,
    altname: String,
    ws_name: String,
    base: String,
    quote: String,
    status: String,
    ordermin: f64,
    costmin: f64,
}
