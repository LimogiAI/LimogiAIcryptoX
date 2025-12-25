//! In-memory order book cache with lock-free reads

use crate::types::{OrderBook, OrderBookLevel, PriceEdge};
use chrono::Utc;
use dashmap::DashMap;
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tracing::{debug, warn};

/// Thread-safe order book cache
pub struct OrderBookCache {
    /// Order books by pair name (e.g., "BTC/USD")
    order_books: DashMap<String, Arc<RwLock<OrderBook>>>,
    
    /// Price edges for graph (bid/ask only, no depth)
    prices: DashMap<String, PriceEdge>,
    
    /// All known currencies
    currencies: DashMap<String, bool>,
    
    /// Pair info mapping
    pair_info: DashMap<String, PairInfo>,
    
    /// Statistics
    stats: Arc<RwLock<CacheStats>>,
}

#[derive(Debug, Clone)]
pub struct PairInfo {
    pub pair_name: String,
    pub base: String,
    pub quote: String,
    pub kraken_id: String,
    pub ws_name: String,
    pub volume_24h: f64,
}

#[derive(Debug, Default)]
pub struct CacheStats {
    pub updates_received: u64,
    pub snapshots_received: u64,
    pub last_update: Option<chrono::DateTime<Utc>>,
}

impl OrderBookCache {
    pub fn new() -> Self {
        Self {
            order_books: DashMap::new(),
            prices: DashMap::new(),
            currencies: DashMap::new(),
            pair_info: DashMap::new(),
            stats: Arc::new(RwLock::new(CacheStats::default())),
        }
    }

    /// Register a trading pair
    pub fn register_pair(&self, info: PairInfo) {
        // Add currencies
        self.currencies.insert(info.base.clone(), true);
        self.currencies.insert(info.quote.clone(), true);
        
        // Create empty order book
        let order_book = OrderBook::new(info.pair_name.clone());
        self.order_books.insert(
            info.pair_name.clone(),
            Arc::new(RwLock::new(order_book)),
        );
        
        // Store pair info
        self.pair_info.insert(info.pair_name.clone(), info);
    }

    /// Update order book from WebSocket snapshot
    pub fn update_snapshot(
        &self,
        pair: &str,
        bids: Vec<OrderBookLevel>,
        asks: Vec<OrderBookLevel>,
        sequence: u64,
    ) {
        if let Some(book_ref) = self.order_books.get(pair) {
            let mut book = book_ref.write();
            book.bids = bids;
            book.asks = asks;
            book.sequence = sequence;
            book.last_update = Utc::now();
            
            // Update price edge
            self.update_price_from_book(pair, &book);
        }
        
        let mut stats = self.stats.write();
        stats.snapshots_received += 1;
        stats.last_update = Some(Utc::now());
    }

    /// Update order book from WebSocket incremental update
    pub fn update_incremental(
        &self,
        pair: &str,
        bid_updates: Vec<OrderBookLevel>,
        ask_updates: Vec<OrderBookLevel>,
        sequence: u64,
    ) {
        if let Some(book_ref) = self.order_books.get(pair) {
            let mut book = book_ref.write();
            
            // Skip if out of sequence (but allow sequence=0 to always update)
            if sequence != 0 && sequence <= book.sequence {
                return;
            }
            
            // Apply bid updates
            for update in bid_updates {
                Self::apply_level_update(&mut book.bids, update, true);
            }
            
            // Apply ask updates
            for update in ask_updates {
                Self::apply_level_update(&mut book.asks, update, false);
            }
            
            book.sequence = sequence;
            book.last_update = Utc::now();
            
            // Update price edge
            self.update_price_from_book(pair, &book);
        }
        
        let mut stats = self.stats.write();
        stats.updates_received += 1;
        stats.last_update = Some(Utc::now());
    }

    /// Apply a single level update to bids or asks
    fn apply_level_update(levels: &mut Vec<OrderBookLevel>, update: OrderBookLevel, is_bid: bool) {
        // Find existing level at this price using relative comparison
        // Uses relative epsilon for better handling across different price ranges
        // (e.g., BTC at 50000 vs SHIB at 0.00001)
        let pos = levels.iter().position(|l| {
            let diff = (l.price - update.price).abs();
            let max_price = l.price.abs().max(update.price.abs());
            if max_price < 1e-10 {
                // Both prices near zero - use absolute comparison
                diff < 1e-15
            } else {
                // Use relative comparison: difference should be < 1e-9 relative to price
                diff / max_price < 1e-9
            }
        });
        
        if update.qty == 0.0 {
            // Remove level
            if let Some(idx) = pos {
                levels.remove(idx);
            }
        } else if let Some(idx) = pos {
            // Update existing level
            levels[idx].qty = update.qty;
        } else {
            // Insert new level in sorted order
            let insert_pos = if is_bid {
                // Bids: highest price first
                levels.iter().position(|l| l.price < update.price).unwrap_or(levels.len())
            } else {
                // Asks: lowest price first
                levels.iter().position(|l| l.price > update.price).unwrap_or(levels.len())
            };
            levels.insert(insert_pos, update);
        }
    }

    /// Update price edge from order book
    fn update_price_from_book(&self, pair: &str, book: &OrderBook) {
        if let Some(info) = self.pair_info.get(pair) {
            let bid = book.best_bid().unwrap_or(0.0);
            let ask = book.best_ask().unwrap_or(0.0);
            
            let edge = PriceEdge {
                pair: pair.to_string(),
                base: info.base.clone(),
                quote: info.quote.clone(),
                bid,
                ask,
                volume_24h: info.volume_24h,
                last_update: book.last_update,
            };
            
            self.prices.insert(pair.to_string(), edge);
        }
    }

    /// Update price from ticker (when no order book subscription)
    pub fn update_price_ticker(
        &self,
        pair: &str,
        bid: f64,
        ask: f64,
        volume_24h: f64,
    ) {
        if let Some(info) = self.pair_info.get(pair) {
            let edge = PriceEdge {
                pair: pair.to_string(),
                base: info.base.clone(),
                quote: info.quote.clone(),
                bid,
                ask,
                volume_24h,
                last_update: Utc::now(),
            };
            
            self.prices.insert(pair.to_string(), edge);
        }
    }

    /// Get order book for a pair (lock-free read)
    /// Returns None if order book has no real data - NO FAKE LIQUIDITY
    pub fn get_order_book(&self, pair: &str) -> Option<OrderBook> {
        self.order_books.get(pair).and_then(|r| {
            let book = r.read().clone();
            
            // Return None if order book is empty - don't fake it!
            // This ensures slippage calculations use real market depth only
            if book.bids.is_empty() || book.asks.is_empty() {
                tracing::debug!("Order book for {} has no real data, skipping", pair);
                return None;
            }
            
            Some(book)
        })
    }

    /// Get multiple order books
    pub fn get_order_books(&self, pairs: &[String]) -> HashMap<String, OrderBook> {
        pairs
            .iter()
            .filter_map(|pair| {
                self.get_order_book(pair).map(|book| (pair.clone(), book))
            })
            .collect()
    }

    /// Get price edge for a pair
    pub fn get_price(&self, pair: &str) -> Option<PriceEdge> {
        self.prices.get(pair).map(|r| r.clone())
    }

    /// Get all prices
    pub fn get_all_prices(&self) -> HashMap<String, PriceEdge> {
        self.prices
            .iter()
            .map(|r| (r.key().clone(), r.value().clone()))
            .collect()
    }

    /// Get all currencies
    pub fn get_currencies(&self) -> HashSet<String> {
        self.currencies
            .iter()
            .map(|r| r.key().clone())
            .collect()
    }

    /// Get pair info
    pub fn get_pair_info(&self, pair: &str) -> Option<PairInfo> {
        self.pair_info.get(pair).map(|r| r.clone())
    }

    /// Get all pairs
    pub fn get_all_pairs(&self) -> Vec<String> {
        self.pair_info.iter().map(|r| r.key().clone()).collect()
    }

    /// Get pairs sorted by volume
    pub fn get_pairs_by_volume(&self, limit: usize) -> Vec<String> {
        let mut pairs: Vec<_> = self.pair_info
            .iter()
            .map(|r| (r.key().clone(), r.value().volume_24h))
            .collect();
        
        pairs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        
        pairs.into_iter()
            .take(limit)
            .map(|(pair, _)| pair)
            .collect()
    }

    /// Get cache statistics
    pub fn get_stats(&self) -> (usize, usize, f64) {
        let pairs = self.order_books.len();
        let currencies = self.currencies.len();
        
        // Calculate average staleness
        let mut total_staleness: i64 = 0;
        let mut count = 0;
        
        for entry in self.order_books.iter() {
            let book = entry.read();
            total_staleness += book.staleness_ms();
            count += 1;
        }
        
        let avg_staleness = if count > 0 {
            total_staleness as f64 / count as f64
        } else {
            0.0
        };
        
        (pairs, currencies, avg_staleness)
    }

    /// Check if order book is fresh enough
    pub fn is_fresh(&self, pair: &str, max_staleness_ms: i64) -> bool {
        self.order_books
            .get(pair)
            .map(|r| r.read().staleness_ms() < max_staleness_ms)
            .unwrap_or(false)
    }

    /// Get staleness for a pair
    pub fn get_staleness(&self, pair: &str) -> Option<i64> {
        self.order_books
            .get(pair)
            .map(|r| r.read().staleness_ms())
    }

    /// Clear all data (for reconnection with new settings)
    pub fn clear(&self) {
        self.order_books.clear();
        self.prices.clear();
        self.currencies.clear();
        self.pair_info.clear();
        
        // Reset stats
        let mut stats = self.stats.write();
        stats.updates_received = 0;
        stats.snapshots_received = 0;
        stats.last_update = None;
        
        tracing::info!("Order book cache cleared");
    }
}

impl Default for OrderBookCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_book_cache() {
        let cache = OrderBookCache::new();
        
        // Register a pair
        cache.register_pair(PairInfo {
            pair_name: "BTC/USD".to_string(),
            base: "BTC".to_string(),
            quote: "USD".to_string(),
            kraken_id: "XBTUSD".to_string(),
            ws_name: "XBT/USD".to_string(),
            volume_24h: 1000000.0,
        });
        
        // Update with snapshot
        cache.update_snapshot(
            "BTC/USD",
            vec![
                OrderBookLevel { price: 100000.0, qty: 1.0 },
                OrderBookLevel { price: 99999.0, qty: 2.0 },
            ],
            vec![
                OrderBookLevel { price: 100001.0, qty: 1.5 },
                OrderBookLevel { price: 100002.0, qty: 2.5 },
            ],
            1,
        );
        
        // Verify
        let book = cache.get_order_book("BTC/USD").unwrap();
        assert_eq!(book.best_bid(), Some(100000.0));
        assert_eq!(book.best_ask(), Some(100001.0));
        
        let price = cache.get_price("BTC/USD").unwrap();
        assert_eq!(price.bid, 100000.0);
        assert_eq!(price.ask, 100001.0);
    }
}