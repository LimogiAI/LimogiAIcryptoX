//! Slippage calculator using order book depth

use crate::order_book::OrderBookCache;
use crate::types::{OrderBook, OrderBookLevel, SlippageLeg, SlippageResult};
use std::sync::Arc;
use tracing::debug;

/// Slippage calculator
pub struct SlippageCalculator {
    cache: Arc<OrderBookCache>,
    staleness_warn_ms: i64,
    staleness_buffer_ms: i64,
    staleness_reject_ms: i64,
}

impl SlippageCalculator {
    pub fn new(
        cache: Arc<OrderBookCache>,
        staleness_warn_ms: i64,
        staleness_buffer_ms: i64,
        staleness_reject_ms: i64,
    ) -> Self {
        Self {
            cache,
            staleness_warn_ms,
            staleness_buffer_ms,
            staleness_reject_ms,
        }
    }

    /// Calculate slippage for a single trade
    pub fn calculate_single(
        &self,
        order_book: &OrderBook,
        side: &str,
        trade_amount_usd: f64,
    ) -> SlippageLeg {
        let pair = order_book.pair.clone();
        
        // Select which side of the book to use
        let levels = if side == "buy" {
            &order_book.asks
        } else {
            &order_book.bids
        };
        
        if levels.is_empty() {
            return SlippageLeg {
                pair,
                side: side.to_string(),
                best_price: 0.0,
                actual_price: 0.0,
                slippage_pct: 0.0,
                can_fill: false,
                depth_used: 0,
                reason: Some("No order book data available".to_string()),
            };
        }
        
        let best_price = levels[0].price;
        let mut remaining_usd = trade_amount_usd;
        let mut total_qty = 0.0;
        let mut total_cost = 0.0;
        let mut depth_used = 0;
        
        for level in levels {
            let price = level.price;
            let qty = level.qty;
            let level_value_usd = qty * price;
            
            depth_used += 1;
            
            if remaining_usd <= level_value_usd {
                // Can fill remaining at this level
                let qty_needed = remaining_usd / price;
                total_qty += qty_needed;
                total_cost += remaining_usd;
                remaining_usd = 0.0;
                break;
            } else {
                // Take all at this level, continue to next
                total_qty += qty;
                total_cost += level_value_usd;
                remaining_usd -= level_value_usd;
            }
        }
        
        if remaining_usd > 0.0 {
            // Not enough liquidity
            return SlippageLeg {
                pair,
                side: side.to_string(),
                best_price,
                actual_price: 0.0,
                slippage_pct: 100.0,
                can_fill: false,
                depth_used,
                reason: Some(format!(
                    "Insufficient liquidity. Needed ${:.2}, available ${:.2}",
                    trade_amount_usd, total_cost
                )),
            };
        }
        
        // Calculate actual average price
        let actual_price = if total_qty > 0.0 {
            total_cost / total_qty
        } else {
            best_price
        };
        
        // Calculate slippage percentage
        let slippage_pct = if side == "buy" {
            // For buys, slippage is positive if we pay MORE than best price
            ((actual_price - best_price) / best_price * 100.0).max(0.0)
        } else {
            // For sells, slippage is positive if we receive LESS than best price
            ((best_price - actual_price) / best_price * 100.0).max(0.0)
        };
        
        SlippageLeg {
            pair,
            side: side.to_string(),
            best_price,
            actual_price,
            slippage_pct,
            can_fill: true,
            depth_used,
            reason: None,
        }
    }

    /// Calculate total slippage for an arbitrage path
    pub fn calculate_path(
        &self,
        path: &str,
        trade_amount_usd: f64,
    ) -> SlippageResult {
        // Parse path: "USD → BTC → ETH → USD"
        let currencies: Vec<&str> = path.split('→')
            .map(|s| s.trim())
            .collect();
        
        if currencies.len() < 3 {
            return SlippageResult {
                total_slippage_pct: 0.0,
                can_execute: false,
                reason: Some("Invalid path format".to_string()),
                legs: vec![],
            };
        }
        
        let mut legs = Vec::new();
        let mut current_amount = trade_amount_usd;
        let mut total_slippage_pct = 0.0;
        
        // Process each leg
        for i in 0..(currencies.len() - 1) {
            let from_currency = currencies[i];
            let to_currency = currencies[i + 1];
            
            // Find the pair and determine buy/sell
            let (pair, side, order_book) = match self.find_pair_and_side(from_currency, to_currency) {
                Some(result) => result,
                None => {
                    return SlippageResult {
                        total_slippage_pct,
                        can_execute: false,
                        reason: Some(format!(
                            "Order book not found for {}/{}",
                            from_currency, to_currency
                        )),
                        legs,
                    };
                }
            };
            
            // Check order book staleness - critical for realistic trading!
            let staleness = order_book.staleness_ms();
            if staleness > self.staleness_reject_ms {
                return SlippageResult {
                    total_slippage_pct,
                    can_execute: false,
                    reason: Some(format!(
                        "Order book for {} too stale: {}ms > {}ms limit",
                        pair, staleness, self.staleness_reject_ms
                    )),
                    legs,
                };
            }
            
            // Calculate slippage for this leg
            let mut leg = self.calculate_single(&order_book, &side, current_amount);
            
            // Add safety buffer for slightly stale data
            if staleness > self.staleness_buffer_ms {
                debug!("Adding 1% staleness buffer for {} ({}ms old)", pair, staleness);
                leg.slippage_pct += 1.0;  // Add 1% buffer for stale data
            } else if staleness > self.staleness_warn_ms {
                debug!("Order book for {} is {}ms old (warning threshold)", pair, staleness);
            }
            
            if !leg.can_fill {
                return SlippageResult {
                    total_slippage_pct,
                    can_execute: false,
                    reason: Some(format!("Cannot fill leg {}: {}", i + 1, leg.reason.as_deref().unwrap_or("unknown"))),
                    legs: {
                        legs.push(leg);
                        legs
                    },
                };
            }
            
            total_slippage_pct += leg.slippage_pct;
            
            // Update amount for next leg (accounting for slippage)
            let slippage_factor = 1.0 - (leg.slippage_pct / 100.0);
            current_amount *= slippage_factor;
            
            legs.push(leg);
        }
        
        SlippageResult {
            total_slippage_pct,
            can_execute: true,
            reason: None,
            legs,
        }
    }

    /// Find the trading pair and determine if we need to buy or sell
    fn find_pair_and_side(&self, from: &str, to: &str) -> Option<(String, String, OrderBook)> {
        // Try direct pair: FROM/TO (we're selling FROM to get TO)
        let direct_pair = format!("{}/{}", from, to);
        if let Some(book) = self.cache.get_order_book(&direct_pair) {
            return Some((direct_pair, "sell".to_string(), book));
        }
        
        // Try reverse pair: TO/FROM (we're buying TO with FROM)
        let reverse_pair = format!("{}/{}", to, from);
        if let Some(book) = self.cache.get_order_book(&reverse_pair) {
            return Some((reverse_pair, "buy".to_string(), book));
        }
        
        // Try without slash
        for pair in self.cache.get_all_pairs() {
            let pair_no_slash = pair.replace("/", "");
            if pair_no_slash == format!("{}{}", from, to) {
                if let Some(book) = self.cache.get_order_book(&pair) {
                    return Some((pair, "sell".to_string(), book));
                }
            } else if pair_no_slash == format!("{}{}", to, from) {
                if let Some(book) = self.cache.get_order_book(&pair) {
                    return Some((pair, "buy".to_string(), book));
                }
            }
        }
        
        None
    }

    /// Calculate slippage with explicit order books (for parallel processing)
    pub fn calculate_path_with_books(
        &self,
        path: &str,
        order_books: &std::collections::HashMap<String, OrderBook>,
        trade_amount_usd: f64,
    ) -> SlippageResult {
        let currencies: Vec<&str> = path.split('→')
            .map(|s| s.trim())
            .collect();
        
        if currencies.len() < 3 {
            return SlippageResult {
                total_slippage_pct: 0.0,
                can_execute: false,
                reason: Some("Invalid path format".to_string()),
                legs: vec![],
            };
        }
        
        let mut legs = Vec::new();
        let mut current_amount = trade_amount_usd;
        let mut total_slippage_pct = 0.0;
        
        for i in 0..(currencies.len() - 1) {
            let from_currency = currencies[i];
            let to_currency = currencies[i + 1];
            
            // Try both orderings
            let direct_pair = format!("{}/{}", from_currency, to_currency);
            let reverse_pair = format!("{}/{}", to_currency, from_currency);
            
            let (pair, side, order_book) = if let Some(book) = order_books.get(&direct_pair) {
                (direct_pair, "sell".to_string(), book)
            } else if let Some(book) = order_books.get(&reverse_pair) {
                (reverse_pair, "buy".to_string(), book)
            } else {
                return SlippageResult {
                    total_slippage_pct,
                    can_execute: false,
                    reason: Some(format!("Order book not found for {}/{}", from_currency, to_currency)),
                    legs,
                };
            };
            
            let leg = self.calculate_single(order_book, &side, current_amount);
            
            if !leg.can_fill {
                return SlippageResult {
                    total_slippage_pct,
                    can_execute: false,
                    reason: Some(format!("Cannot fill leg {}: {}", i + 1, leg.reason.as_deref().unwrap_or("unknown"))),
                    legs: {
                        legs.push(leg);
                        legs
                    },
                };
            }
            
            total_slippage_pct += leg.slippage_pct;
            let slippage_factor = 1.0 - (leg.slippage_pct / 100.0);
            current_amount *= slippage_factor;
            
            legs.push(leg);
        }
        
        SlippageResult {
            total_slippage_pct,
            can_execute: true,
            reason: None,
            legs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slippage_calculation() {
        let mut book = OrderBook::new("BTC/USD".to_string());
        
        // Add asks (we want to buy)
        book.asks = vec![
            OrderBookLevel { price: 100000.0, qty: 0.1 },  // $10,000 worth
            OrderBookLevel { price: 100100.0, qty: 0.1 },  // $10,010 worth
            OrderBookLevel { price: 100200.0, qty: 0.1 },  // $10,020 worth
        ];
        
        let cache = Arc::new(OrderBookCache::new());
        let calc = SlippageCalculator::new(cache, 500, 1000, 2000);
        
        // Buy $5,000 worth - should fill from first level only
        let result = calc.calculate_single(&book, "buy", 5000.0);
        assert!(result.can_fill);
        assert_eq!(result.depth_used, 1);
        assert!(result.slippage_pct < 0.01);  // Essentially 0
        
        // Buy $15,000 worth - should use first two levels
        let result = calc.calculate_single(&book, "buy", 15000.0);
        assert!(result.can_fill);
        assert_eq!(result.depth_used, 2);
        assert!(result.slippage_pct > 0.0);  // Some slippage
    }
}
