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

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, info, warn};

const KRAKEN_REST_URL: &str = "https://api.kraken.com";

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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairSelectionConfig {
    /// Maximum number of pairs to select (default: 30)
    pub max_pairs: usize,
    /// Minimum 24h USD volume required (default: 50,000)
    pub min_volume_24h_usd: f64,
    /// Maximum cost minimum for a pair (default: 20.0 USD)
    pub max_cost_min: f64,
    /// Allowed quote currencies (default: ["USD", "EUR"])
    pub allowed_quote_currencies: Vec<String>,
    /// Blocked base currencies (default: ["USDT", "USDC"] - restricted in Canada)
    pub blocked_base_currencies: Vec<String>,
}

impl Default for PairSelectionConfig {
    fn default() -> Self {
        Self {
            max_pairs: 30,
            min_volume_24h_usd: 50_000.0,
            max_cost_min: 20.0,
            allowed_quote_currencies: vec!["USD".to_string(), "EUR".to_string()],
            // USDT/USDC restricted in Canada (CA:BC)
            blocked_base_currencies: vec!["USDT".to_string(), "USDC".to_string()],
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
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();

        Self { config, client }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(PairSelectionConfig::default())
    }

    /// Select the best trading pairs for arbitrage
    pub async fn select_pairs(&self) -> Result<Vec<SelectedPair>, PairSelectionError> {
        info!("Starting pair selection (max: {}, quote currencies: {:?})",
              self.config.max_pairs, self.config.allowed_quote_currencies);

        // Step 1: Fetch all asset pairs from Kraken
        let all_pairs = self.fetch_asset_pairs().await?;
        info!("Fetched {} total pairs from Kraken", all_pairs.len());

        // Step 2: Apply initial filters
        let filtered_pairs = self.apply_filters(all_pairs);
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
            .take(self.config.max_pairs)
            .collect();

        info!("Selected {} pairs for trading:", final_pairs.len());
        for (i, pair) in final_pairs.iter().enumerate() {
            info!("  {}. {} - ${:.0} 24h volume", i + 1, pair.pair_name, pair.volume_24h_usd);
        }

        Ok(final_pairs)
    }

    /// Fetch all asset pairs from Kraken REST API
    async fn fetch_asset_pairs(&self) -> Result<Vec<RawPairInfo>, PairSelectionError> {
        let url = format!("{}/0/public/AssetPairs", KRAKEN_REST_URL);
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
    fn apply_filters(&self, pairs: Vec<RawPairInfo>) -> Vec<RawPairInfo> {
        pairs
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
                if p.costmin > self.config.max_cost_min {
                    debug!("Filtered out {} - costmin {} > {}", p.altname, p.costmin, self.config.max_cost_min);
                    return false;
                }

                // Filter 5: Base currency must not be in blocked list (e.g., USDT/USDC restricted in Canada)
                if self.config.blocked_base_currencies.contains(&p.base) {
                    debug!("Filtered out {} - base {} is blocked", p.altname, p.base);
                    return false;
                }

                true
            })
            .collect()
    }

    /// Fetch 24h volumes for pairs
    async fn fetch_volumes(&self, pairs: Vec<RawPairInfo>) -> Result<Vec<SelectedPair>, PairSelectionError> {
        let mut result = Vec::new();

        // Get EUR/USD rate for volume conversion
        let eur_usd_rate = self.fetch_eur_usd_rate().await.unwrap_or(1.05);
        info!("Using EUR/USD rate: {:.4}", eur_usd_rate);

        // Fetch in chunks of 100 (Kraken rate limit)
        let kraken_ids: Vec<String> = pairs.iter().map(|p| p.kraken_id.clone()).collect();

        for chunk in kraken_ids.chunks(100) {
            let pair_param = chunk.join(",");
            let url = format!("{}/0/public/Ticker?pair={}", KRAKEN_REST_URL, pair_param);

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
                        if volume_usd >= self.config.min_volume_24h_usd {
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

    /// Fetch EUR/USD exchange rate
    async fn fetch_eur_usd_rate(&self) -> Result<f64, PairSelectionError> {
        let url = format!("{}/0/public/Ticker?pair=EURUSD", KRAKEN_REST_URL);
        let response = self.client.get(&url).send().await?;
        let data: Value = response.json().await?;

        let rate = data.get("result")
            .and_then(|r| r.get("ZEURZUSD").or_else(|| r.get("EURUSD")))
            .and_then(|t| t.get("c"))
            .and_then(|c| c.get(0))
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(1.05);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_currency() {
        let selector = KrakenPairSelector::with_defaults();
        assert_eq!(selector.normalize_currency("XXBT"), "BTC");
        assert_eq!(selector.normalize_currency("XBT"), "BTC");
        assert_eq!(selector.normalize_currency("ZUSD"), "USD");
        assert_eq!(selector.normalize_currency("ZEUR"), "EUR");
        assert_eq!(selector.normalize_currency("SOL"), "SOL");
        assert_eq!(selector.normalize_currency("XETH"), "ETH");
    }

    #[test]
    fn test_default_config() {
        let config = PairSelectionConfig::default();
        assert_eq!(config.max_pairs, 30);
        assert_eq!(config.allowed_quote_currencies, vec!["USD", "EUR"]);
        assert_eq!(config.max_cost_min, 20.0);
    }
}
