//! Kraken API Authentication Module
//!
//! Provides authentication for:
//! - REST API private endpoints (API-Key + API-Sign headers)
//! - WebSocket v2 private channels (token-based)
//!
//! Authentication algorithm:
//! 1. Create SHA256 hash of (nonce + POST data)
//! 2. Decode API secret from base64
//! 3. Create HMAC-SHA512 of (URI path + SHA256 hash) using decoded secret
//! 4. Base64 encode the HMAC result for API-Sign header

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use parking_lot::RwLock;
use thiserror::Error;
use tracing::{debug, error, info, warn};

type HmacSha512 = Hmac<Sha512>;

const KRAKEN_API_URL: &str = "https://api.kraken.com";
const TOKEN_REFRESH_BUFFER_SECS: u64 = 60; // Refresh 1 minute before expiry
const TOKEN_VALIDITY_SECS: u64 = 900; // 15 minutes

/// Authentication errors
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("API credentials not configured")]
    NotConfigured,
    #[error("Invalid API secret: {0}")]
    InvalidSecret(String),
    #[error("Failed to get WebSocket token: {0}")]
    TokenError(String),
    #[error("HTTP request failed: {0}")]
    RequestError(#[from] reqwest::Error),
    #[error("API error: {0}")]
    ApiError(String),
}

/// Response from GetWebSocketsToken endpoint
#[derive(Debug, Deserialize)]
struct TokenResponse {
    error: Vec<String>,
    result: Option<TokenResult>,
}

#[derive(Debug, Deserialize)]
struct TokenResult {
    token: String,
    expires: Option<u64>,
}

/// Kraken API authenticator
pub struct KrakenAuth {
    api_key: String,
    api_secret: Vec<u8>, // Decoded from base64
    client: Client,

    // Cached WebSocket token
    ws_token: RwLock<Option<CachedToken>>,

    // Nonce counter (must be increasing)
    nonce_counter: AtomicU64,
}

/// Cached WebSocket token with expiry
#[derive(Clone)]
struct CachedToken {
    token: String,
    obtained_at: Instant,
    expires_at: Instant,
}

impl KrakenAuth {
    /// Create a new authenticator with API credentials
    pub fn new(api_key: String, api_secret: String) -> Result<Self, AuthError> {
        // Decode the base64-encoded API secret
        let decoded_secret = BASE64
            .decode(&api_secret)
            .map_err(|e| AuthError::InvalidSecret(e.to_string()))?;

        // Initialize nonce from current timestamp in milliseconds
        let initial_nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Ok(Self {
            api_key,
            api_secret: decoded_secret,
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            ws_token: RwLock::new(None),
            nonce_counter: AtomicU64::new(initial_nonce),
        })
    }

    /// Create an authenticator without credentials (for public-only mode)
    pub fn new_public_only() -> Self {
        Self {
            api_key: String::new(),
            api_secret: Vec::new(),
            client: Client::new(),
            ws_token: RwLock::new(None),
            nonce_counter: AtomicU64::new(0),
        }
    }

    /// Check if credentials are configured
    pub fn is_configured(&self) -> bool {
        !self.api_key.is_empty() && !self.api_secret.is_empty()
    }

    /// Get next nonce value (must be increasing for each request)
    fn next_nonce(&self) -> u64 {
        self.nonce_counter.fetch_add(1, Ordering::SeqCst)
    }

    /// Generate API-Sign header for a private endpoint
    ///
    /// Algorithm:
    /// 1. SHA256(nonce + POST_data)
    /// 2. HMAC-SHA512(uri_path + sha256_hash, base64_decode(api_secret))
    /// 3. base64_encode(hmac_result)
    pub fn sign_request(&self, uri_path: &str, nonce: u64, post_data: &str) -> Result<String, AuthError> {
        if !self.is_configured() {
            return Err(AuthError::NotConfigured);
        }

        // Step 1: SHA256(nonce + POST_data)
        let nonce_str = nonce.to_string();
        let mut sha256 = Sha256::new();
        sha256.update(nonce_str.as_bytes());
        sha256.update(post_data.as_bytes());
        let sha256_hash = sha256.finalize();

        // Step 2: HMAC-SHA512(uri_path + sha256_hash, api_secret)
        let mut hmac = HmacSha512::new_from_slice(&self.api_secret)
            .map_err(|e| AuthError::InvalidSecret(e.to_string()))?;
        hmac.update(uri_path.as_bytes());
        hmac.update(&sha256_hash);
        let hmac_result = hmac.finalize().into_bytes();

        // Step 3: Base64 encode
        Ok(BASE64.encode(hmac_result))
    }

    /// Get a valid WebSocket token, refreshing if necessary
    pub async fn get_ws_token(&self) -> Result<String, AuthError> {
        if !self.is_configured() {
            return Err(AuthError::NotConfigured);
        }

        // Check if we have a valid cached token
        {
            let cached = self.ws_token.read();
            if let Some(ref token) = *cached {
                if token.expires_at > Instant::now() + Duration::from_secs(TOKEN_REFRESH_BUFFER_SECS) {
                    debug!("Using cached WebSocket token");
                    return Ok(token.token.clone());
                }
            }
        }

        // Need to refresh token
        info!("Fetching new WebSocket token from Kraken API");
        let token = self.fetch_ws_token().await?;

        // Cache the token
        {
            let mut cached = self.ws_token.write();
            *cached = Some(CachedToken {
                token: token.clone(),
                obtained_at: Instant::now(),
                expires_at: Instant::now() + Duration::from_secs(TOKEN_VALIDITY_SECS),
            });
        }

        Ok(token)
    }

    /// Fetch a new WebSocket token from the REST API
    async fn fetch_ws_token(&self) -> Result<String, AuthError> {
        let uri_path = "/0/private/GetWebSocketsToken";
        let nonce = self.next_nonce();
        let post_data = format!("nonce={}", nonce);

        let signature = self.sign_request(uri_path, nonce, &post_data)?;

        let response = self.client
            .post(format!("{}{}", KRAKEN_API_URL, uri_path))
            .header("API-Key", &self.api_key)
            .header("API-Sign", &signature)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(post_data)
            .send()
            .await?;

        let status = response.status();
        let body = response.text().await?;

        debug!("GetWebSocketsToken response ({}): {}", status, body);

        let token_response: TokenResponse = serde_json::from_str(&body)
            .map_err(|e| AuthError::TokenError(format!("Failed to parse response: {}", e)))?;

        if !token_response.error.is_empty() {
            return Err(AuthError::ApiError(token_response.error.join(", ")));
        }

        token_response
            .result
            .map(|r| r.token)
            .ok_or_else(|| AuthError::TokenError("No token in response".to_string()))
    }

    /// Make an authenticated POST request to a private endpoint
    pub async fn post_private(
        &self,
        endpoint: &str,
        params: &HashMap<String, String>,
    ) -> Result<serde_json::Value, AuthError> {
        if !self.is_configured() {
            return Err(AuthError::NotConfigured);
        }

        let uri_path = format!("/0/private/{}", endpoint);
        let nonce = self.next_nonce();

        // Build POST data
        let mut post_data = format!("nonce={}", nonce);
        for (key, value) in params {
            post_data.push_str(&format!("&{}={}", key, value));
        }

        let signature = self.sign_request(&uri_path, nonce, &post_data)?;

        let response = self.client
            .post(format!("{}{}", KRAKEN_API_URL, uri_path))
            .header("API-Key", &self.api_key)
            .header("API-Sign", &signature)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(post_data)
            .send()
            .await?;

        let body: serde_json::Value = response.json().await?;

        // Check for API errors
        if let Some(errors) = body.get("error").and_then(|e| e.as_array()) {
            if !errors.is_empty() {
                let error_msg: Vec<String> = errors
                    .iter()
                    .filter_map(|e| e.as_str().map(String::from))
                    .collect();
                return Err(AuthError::ApiError(error_msg.join(", ")));
            }
        }

        Ok(body)
    }

    /// Get API key (for display/logging - redacted)
    pub fn get_api_key_redacted(&self) -> String {
        if self.api_key.len() > 8 {
            format!("{}...{}", &self.api_key[..4], &self.api_key[self.api_key.len()-4..])
        } else {
            "****".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nonce_increasing() {
        let auth = KrakenAuth::new_public_only();
        let n1 = auth.next_nonce();
        let n2 = auth.next_nonce();
        let n3 = auth.next_nonce();
        assert!(n2 > n1);
        assert!(n3 > n2);
    }

    #[test]
    fn test_not_configured() {
        let auth = KrakenAuth::new_public_only();
        assert!(!auth.is_configured());
    }

    // Note: Full authentication tests require valid API credentials
}
