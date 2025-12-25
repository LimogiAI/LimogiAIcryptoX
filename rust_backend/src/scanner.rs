//! Arbitrage scanner using graph-based pathfinding

use crate::order_book::OrderBookCache;
use crate::types::{EngineConfig, LegDetail, Opportunity, OrderBookHealth, PriceEdge};
use chrono::Utc;
use parking_lot::RwLock;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use uuid::Uuid;

/// Arbitrage scanner using directed graph
pub struct Scanner {
    cache: Arc<OrderBookCache>,
    config: EngineConfig,
    health: Arc<RwLock<OrderBookHealth>>,
}

/// Internal representation of an arbitrage path
#[derive(Debug, Clone)]
struct ArbitragePath {
    currencies: Vec<String>,
    pairs: Vec<String>,
    actions: Vec<String>,  // "buy" or "sell" for each pair
    rates: Vec<f64>,       // Exchange rates used
}

impl Scanner {
    pub fn new(cache: Arc<OrderBookCache>, config: EngineConfig) -> Self {
        Self { 
            cache, 
            config,
            health: Arc::new(RwLock::new(OrderBookHealth::default())),
        }
    }

    /// Get current order book health stats
    pub fn get_health(&self) -> OrderBookHealth {
        self.health.read().clone()
    }

    /// Scan for all arbitrage opportunities
    pub fn scan(&self, base_currencies: &[String]) -> Vec<Opportunity> {
        let prices = self.cache.get_all_prices();
        
        if prices.is_empty() {
            return vec![];
        }
        
        // Build graph
        let (graph, node_map) = self.build_graph(&prices);
        
        // Find opportunities from each base currency in parallel
        let opportunities: Vec<Opportunity> = base_currencies
            .par_iter()
            .flat_map(|base| {
                self.find_opportunities_from(&graph, &node_map, base, &prices)
            })
            .collect();
        
        // Sort by profit and deduplicate
        let mut unique: HashMap<String, Opportunity> = HashMap::new();
        for opp in opportunities {
            let key = opp.path.clone();
            if !unique.contains_key(&key) || unique[&key].net_profit_pct < opp.net_profit_pct {
                unique.insert(key, opp);
            }
        }
        
        let mut result: Vec<Opportunity> = unique.into_values().collect();
        result.sort_by(|a, b| b.net_profit_pct.partial_cmp(&a.net_profit_pct).unwrap());
        
        result
    }

    /// Build directed graph from price data
    /// CRITICAL: Only includes pairs with VALID ORDER BOOK DATA (not just ticker prices)
    fn build_graph(
        &self,
        prices: &HashMap<String, PriceEdge>,
    ) -> (DiGraph<String, (String, f64, String)>, HashMap<String, NodeIndex>) {
        let mut graph = DiGraph::new();
        let mut node_map: HashMap<String, NodeIndex> = HashMap::new();
        
        // Track health stats
        let mut valid_pairs = 0u32;
        let mut skipped_no_orderbook = 0u32;
        let mut skipped_thin_depth = 0u32;
        let mut skipped_stale = 0u32;
        let mut skipped_bad_spread = 0u32;
        let mut skipped_no_price = 0u32;
        let mut total_freshness_ms = 0.0f64;
        let mut total_spread_pct = 0.0f64;
        let mut total_depth = 0.0f64;
        let mut freshness_count = 0u32;
        
        // Add nodes for all currencies
        let currencies = self.cache.get_currencies();
        for currency in currencies {
            let idx = graph.add_node(currency.clone());
            node_map.insert(currency, idx);
        }
        
        let total_pairs = prices.len() as u32;
        
        // Add edges for all pairs (bidirectional)
        for (pair, edge) in prices {
            let base_idx = match node_map.get(&edge.base) {
                Some(idx) => *idx,
                None => continue,
            };
            let quote_idx = match node_map.get(&edge.quote) {
                Some(idx) => *idx,
                None => continue,
            };
            
            // Skip if no valid prices
            if edge.bid <= 0.0 || edge.ask <= 0.0 {
                skipped_no_price += 1;
                continue;
            }
            
            // CRITICAL FIX: Skip pairs WITHOUT valid order book data
            // This prevents using stale ticker prices for illiquid pairs
            let order_book = match self.cache.get_order_book(pair) {
                Some(book) => book,
                None => {
                    skipped_no_orderbook += 1;
                    continue;  // No order book = no trading
                }
            };
            
            // Validate order book has minimum depth (at least 3 levels each side)
            if order_book.bids.len() < 3 || order_book.asks.len() < 3 {
                skipped_thin_depth += 1;
                continue;  // Too thin order book
            }
            
            // Validate order book is fresh (less than 5 seconds old)
            let staleness = order_book.staleness_ms();
            if staleness > 5000 {
                skipped_stale += 1;
                continue;  // Stale order book
            }
            
            // Track freshness for valid books
            total_freshness_ms += staleness as f64;
            total_depth += (order_book.bids.len() + order_book.asks.len()) as f64 / 2.0;
            freshness_count += 1;
            
            // SANITY CHECK: Validate ticker price matches order book
            // If they differ by more than 5%, the ticker is stale - use order book prices
            let book_bid = order_book.bids.first().map(|l| l.price).unwrap_or(0.0);
            let book_ask = order_book.asks.first().map(|l| l.price).unwrap_or(0.0);
            
            let (bid, ask) = if book_bid > 0.0 && book_ask > 0.0 {
                // Use order book prices (more accurate than ticker)
                let ticker_bid_diff = ((edge.bid - book_bid) / book_bid).abs();
                let ticker_ask_diff = ((edge.ask - book_ask) / book_ask).abs();
                
                if ticker_bid_diff > 0.05 || ticker_ask_diff > 0.05 {
                    // Ticker differs significantly from order book - use order book
                    tracing::debug!(
                        "Using order book prices for {} (ticker diff: bid={:.2}%, ask={:.2}%)",
                        pair, ticker_bid_diff * 100.0, ticker_ask_diff * 100.0
                    );
                }
                (book_bid, book_ask)
            } else {
                // Fallback to ticker (should rarely happen with above checks)
                (edge.bid, edge.ask)
            };
            
            // Final sanity check: spread should be reasonable (< 10%)
            let spread_pct = (ask - bid) / bid * 100.0;
            if spread_pct > 10.0 || spread_pct < 0.0 {
                skipped_bad_spread += 1;
                continue;  // Unrealistic spread
            }
            
            // Track spread for valid pairs
            total_spread_pct += spread_pct;
            valid_pairs += 1;
            
            // Edge from base to quote (sell base, get quote)
            // Rate = how much quote you get for 1 base = bid price
            graph.add_edge(
                base_idx,
                quote_idx,
                (pair.clone(), bid, "sell".to_string()),
            );
            
            // Edge from quote to base (sell quote, get base)
            // Rate = how much base you get for 1 quote = 1/ask price
            graph.add_edge(
                quote_idx,
                base_idx,
                (pair.clone(), 1.0 / ask, "buy".to_string()),
            );
        }
        
        // Update health stats
        {
            let mut health = self.health.write();
            health.total_pairs = total_pairs;
            health.valid_pairs = valid_pairs;
            health.skipped_no_orderbook = skipped_no_orderbook;
            health.skipped_thin_depth = skipped_thin_depth;
            health.skipped_stale = skipped_stale;
            health.skipped_bad_spread = skipped_bad_spread;
            health.skipped_no_price = skipped_no_price;
            health.avg_freshness_ms = if freshness_count > 0 { 
                total_freshness_ms / freshness_count as f64 
            } else { 
                0.0 
            };
            health.avg_spread_pct = if valid_pairs > 0 { 
                total_spread_pct / valid_pairs as f64 
            } else { 
                0.0 
            };
            health.avg_depth = if freshness_count > 0 {
                total_depth / freshness_count as f64
            } else {
                0.0
            };
            health.last_update = Utc::now().to_rfc3339();
        }
        
        let total_skipped = skipped_no_orderbook + skipped_thin_depth + skipped_stale + skipped_bad_spread + skipped_no_price;
        tracing::info!(
            "Graph built: {} pairs with valid order books, {} pairs skipped (no/stale/thin order book)",
            valid_pairs, total_skipped
        );
        
        (graph, node_map)
    }

    /// Find opportunities starting from a specific currency
    fn find_opportunities_from(
        &self,
        graph: &DiGraph<String, (String, f64, String)>,
        node_map: &HashMap<String, NodeIndex>,
        start: &str,
        _prices: &HashMap<String, PriceEdge>,
    ) -> Vec<Opportunity> {
        let start_idx = match node_map.get(start) {
            Some(idx) => *idx,
            None => return vec![],
        };
        
        let mut opportunities = Vec::new();
        let max_legs = 4;  // Max 4 legs
        
        // DFS to find cycles
        let mut paths: Vec<ArbitragePath> = Vec::new();
        self.dfs_find_cycles(
            graph,
            start_idx,
            start_idx,
            &mut vec![start.to_string()],
            &mut vec![],
            &mut vec![],
            &mut vec![],
            &mut HashSet::new(),
            max_legs,
            &mut paths,
        );
        
        // Convert paths to opportunities
        for path in paths {
            if let Some(opp) = self.path_to_opportunity(&path, start) {
                if opp.is_profitable {
                    opportunities.push(opp);
                }
            }
        }
        
        opportunities
    }

    /// DFS to find all cycles back to start
    fn dfs_find_cycles(
        &self,
        graph: &DiGraph<String, (String, f64, String)>,
        start: NodeIndex,
        current: NodeIndex,
        currencies: &mut Vec<String>,
        pairs: &mut Vec<String>,
        actions: &mut Vec<String>,
        rates: &mut Vec<f64>,
        visited_pairs: &mut HashSet<String>,
        max_legs: usize,
        results: &mut Vec<ArbitragePath>,
    ) {
        if currencies.len() > max_legs + 1 {
            return;
        }
        
        // Check if we're back at start (and have at least 2 legs)
        if current == start && currencies.len() > 2 {
            results.push(ArbitragePath {
                currencies: currencies.clone(),
                pairs: pairs.clone(),
                actions: actions.clone(),
                rates: rates.clone(),
            });
            return;
        }
        
        // Explore neighbors
        for edge in graph.edges(current) {
            let (pair, rate, action) = edge.weight();
            let target = edge.target();
            let target_currency = &graph[target];
            
            // Don't revisit same pair
            if visited_pairs.contains(pair) {
                continue;
            }
            
            // Don't revisit currencies except start
            if target != start && currencies.contains(target_currency) {
                continue;
            }
            
            // Recurse
            currencies.push(target_currency.clone());
            pairs.push(pair.clone());
            actions.push(action.clone());
            rates.push(*rate);
            visited_pairs.insert(pair.clone());
            
            self.dfs_find_cycles(
                graph,
                start,
                target,
                currencies,
                pairs,
                actions,
                rates,
                visited_pairs,
                max_legs,
                results,
            );
            
            currencies.pop();
            pairs.pop();
            actions.pop();
            rates.pop();
            visited_pairs.remove(pair);
        }
    }

    /// Convert a path to an Opportunity with profit calculations
    fn path_to_opportunity(&self, path: &ArbitragePath, _start: &str) -> Option<Opportunity> {
        if path.rates.is_empty() {
            return None;
        }
        
        let start_amount = 1.0;  // Calculate for 1 unit
        
        // Calculate final amount by multiplying all rates
        let mut amount = start_amount;
        for rate in &path.rates {
            amount *= rate;
        }
        
        // Calculate fees (fee per leg)
        let fee_per_leg = self.config.fee_rate;
        let total_legs = path.pairs.len();
        let fees_pct = fee_per_leg * 100.0 * total_legs as f64;
        
        // Apply fees
        for _ in 0..total_legs {
            amount *= 1.0 - fee_per_leg;
        }
        
        // Calculate profits
        let gross_profit_pct = (path.rates.iter().product::<f64>() - 1.0) * 100.0;
        let net_profit_pct = (amount - start_amount) / start_amount * 100.0;
        
        // SANITY CHECK: Reject unrealistic profits
        // Real arbitrage opportunities are typically 0.01% - 1%
        // Anything above 5% is almost certainly a data error
        const MAX_REALISTIC_PROFIT_PCT: f64 = 5.0;
        if gross_profit_pct.abs() > MAX_REALISTIC_PROFIT_PCT {
            tracing::debug!(
                "Rejecting unrealistic opportunity: {} with {:.2}% gross profit (max: {}%)",
                path.currencies.join(" → "),
                gross_profit_pct,
                MAX_REALISTIC_PROFIT_PCT
            );
            return None;
        }
        
        let is_profitable = net_profit_pct > self.config.min_profit_threshold * 100.0;
        
        // Build path string
        let path_str = path.currencies.join(" → ");

        // Build legs detail for price snapshot
        let legs_detail: Vec<LegDetail> = path.pairs.iter()
            .zip(path.actions.iter())
            .zip(path.rates.iter())
            .map(|((pair, action), rate)| LegDetail {
                pair: pair.clone(),
                action: action.clone(),
                rate: *rate,
            })
            .collect();
        
        Some(Opportunity {
            id: Uuid::new_v4().to_string(),
            path: path_str,
            legs: total_legs,
            gross_profit_pct,
            fees_pct,
            net_profit_pct,
            is_profitable,
            detected_at: Utc::now(),
            fee_rate: self.config.fee_rate,
            fee_source: self.config.fee_source.clone(),
            legs_detail,
        })
    }

    /// Scan for opportunities with specific pairs only
    pub fn scan_filtered(&self, base_currencies: &[String], min_profit_pct: f64) -> Vec<Opportunity> {
        let mut opportunities = self.scan(base_currencies);
        opportunities.retain(|o| o.net_profit_pct >= min_profit_pct);
        opportunities
    }

    /// Get unique paths from opportunities (for dispatcher)
    pub fn get_unique_paths(opportunities: &[Opportunity]) -> Vec<String> {
        let mut seen = HashSet::new();
        opportunities
            .iter()
            .filter_map(|o| {
                if seen.contains(&o.path) {
                    None
                } else {
                    seen.insert(o.path.clone());
                    Some(o.path.clone())
                }
            })
            .collect()
    }
}
