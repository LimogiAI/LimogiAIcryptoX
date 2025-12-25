//! Persistent Graph Manager for Incremental Updates
//!
//! Maintains a persistent directed graph for arbitrage scanning:
//! - Graph structure is built once during initialization
//! - Only edge weights are updated when order books change
//! - Tracks which pairs have changed for targeted scanning
//!
//! Performance benefits:
//! - Full rebuild: ~50ms for 300 pairs
//! - Incremental update: ~2ms for single pair update

use crate::order_book::OrderBookCache;
use crate::types::{EngineConfig, LegDetail, Opportunity, OrderBookHealth, PriceEdge};
use chrono::Utc;
use parking_lot::RwLock;
use petgraph::graph::{DiGraph, EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info};
use uuid::Uuid;

/// Edge data in the graph
#[derive(Clone, Debug)]
pub struct EdgeData {
    pub pair: String,
    pub rate: f64,
    pub side: String, // "buy" or "sell"
    pub valid: bool,  // Is this edge currently valid for trading?
}

/// Persistent graph structure for incremental updates
pub struct PersistentGraph {
    /// The actual graph structure
    graph: DiGraph<String, EdgeData>,

    /// Currency name to node index mapping
    node_map: HashMap<String, NodeIndex>,

    /// Pair name to edge indices mapping (each pair has 2 edges: buy and sell)
    edge_map: HashMap<String, Vec<EdgeIndex>>,

    /// Track last update time for each pair
    last_update: HashMap<String, Instant>,

    /// Set of pairs that have been updated since last scan
    dirty_pairs: RwLock<HashSet<String>>,

    /// Graph build counter
    build_count: AtomicU64,

    /// Incremental update counter
    update_count: AtomicU64,

    /// Order book health stats
    health: RwLock<OrderBookHealth>,
}

impl PersistentGraph {
    /// Create a new persistent graph
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            node_map: HashMap::new(),
            edge_map: HashMap::new(),
            last_update: HashMap::new(),
            dirty_pairs: RwLock::new(HashSet::new()),
            build_count: AtomicU64::new(0),
            update_count: AtomicU64::new(0),
            health: RwLock::new(OrderBookHealth::default()),
        }
    }

    /// Initialize the graph structure from cache
    /// This builds the initial graph with all currencies as nodes
    pub fn initialize(&mut self, cache: &Arc<OrderBookCache>) {
        let start = Instant::now();

        // Clear existing structure
        self.graph.clear();
        self.node_map.clear();
        self.edge_map.clear();
        self.last_update.clear();

        // Add nodes for all currencies
        let currencies = cache.get_currencies();
        for currency in &currencies {
            let idx = self.graph.add_node(currency.clone());
            self.node_map.insert(currency.clone(), idx);
        }

        // Add placeholder edges for all pairs
        let pairs = cache.get_all_pairs();
        let prices = cache.get_all_prices();

        for pair in &pairs {
            if let Some(edge) = prices.get(pair) {
                self.add_pair_edges(pair, &edge.base, &edge.quote);
            }
        }

        self.build_count.fetch_add(1, Ordering::Relaxed);
        info!(
            "PersistentGraph initialized: {} currencies, {} pairs in {:?}",
            currencies.len(),
            pairs.len(),
            start.elapsed()
        );
    }

    /// Add edges for a trading pair (bidirectional)
    fn add_pair_edges(&mut self, pair: &str, base: &str, quote: &str) {
        let base_idx = match self.node_map.get(base) {
            Some(idx) => *idx,
            None => return,
        };
        let quote_idx = match self.node_map.get(quote) {
            Some(idx) => *idx,
            None => return,
        };

        // Edge from base to quote (sell base, get quote)
        let sell_edge = self.graph.add_edge(
            base_idx,
            quote_idx,
            EdgeData {
                pair: pair.to_string(),
                rate: 0.0, // Will be updated
                side: "sell".to_string(),
                valid: false,
            },
        );

        // Edge from quote to base (buy base with quote)
        let buy_edge = self.graph.add_edge(
            quote_idx,
            base_idx,
            EdgeData {
                pair: pair.to_string(),
                rate: 0.0, // Will be updated
                side: "buy".to_string(),
                valid: false,
            },
        );

        self.edge_map.insert(pair.to_string(), vec![sell_edge, buy_edge]);
    }

    /// Update a single pair's edge weights
    /// Returns true if the update was significant (rates changed notably)
    pub fn update_pair(
        &mut self,
        cache: &Arc<OrderBookCache>,
        pair: &str,
    ) -> bool {
        let edge_indices = match self.edge_map.get(pair) {
            Some(indices) => indices.clone(),
            None => return false,
        };

        // Get current price and order book
        let price = cache.get_price(pair);
        let order_book = cache.get_order_book(pair);

        let (bid, ask, valid) = match (&price, &order_book) {
            (Some(p), Some(book)) => {
                // Validate order book
                let has_depth = book.bids.len() >= 3 && book.asks.len() >= 3;
                let is_fresh = book.staleness_ms() < 5000;
                let book_bid = book.bids.first().map(|l| l.price).unwrap_or(0.0);
                let book_ask = book.asks.first().map(|l| l.price).unwrap_or(0.0);
                let spread_pct = if book_bid > 0.0 { (book_ask - book_bid) / book_bid * 100.0 } else { 100.0 };
                let reasonable_spread = spread_pct >= 0.0 && spread_pct < 10.0;

                if has_depth && is_fresh && reasonable_spread && book_bid > 0.0 && book_ask > 0.0 {
                    (book_bid, book_ask, true)
                } else {
                    (p.bid, p.ask, false)
                }
            }
            (Some(p), None) => (p.bid, p.ask, false),
            _ => return false,
        };

        // Update edges
        let mut changed = false;
        for edge_idx in &edge_indices {
            if let Some(edge) = self.graph.edge_weight_mut(*edge_idx) {
                let new_rate = if edge.side == "sell" {
                    bid
                } else {
                    if ask > 0.0 { 1.0 / ask } else { 0.0 }
                };

                // Check if rate changed significantly (>0.001%)
                let old_rate = edge.rate;
                let rate_diff = if old_rate > 0.0 {
                    ((new_rate - old_rate) / old_rate).abs()
                } else {
                    1.0
                };

                if rate_diff > 0.00001 || edge.valid != valid {
                    edge.rate = new_rate;
                    edge.valid = valid;
                    changed = true;
                }
            }
        }

        if changed {
            self.last_update.insert(pair.to_string(), Instant::now());
            self.dirty_pairs.write().insert(pair.to_string());
            self.update_count.fetch_add(1, Ordering::Relaxed);
        }

        changed
    }

    /// Update all pairs from cache (used during initialization or major refresh)
    pub fn update_all(&mut self, cache: &Arc<OrderBookCache>) {
        let start = Instant::now();
        let pairs = cache.get_all_pairs();

        let mut valid_count = 0u32;
        let mut invalid_count = 0u32;

        for pair in &pairs {
            if self.update_pair(cache, pair) {
                // Check if it's now valid
                if let Some(edges) = self.edge_map.get(pair) {
                    if let Some(edge) = edges.first().and_then(|e| self.graph.edge_weight(*e)) {
                        if edge.valid {
                            valid_count += 1;
                        } else {
                            invalid_count += 1;
                        }
                    }
                }
            }
        }

        debug!(
            "PersistentGraph updated all pairs: {} valid, {} invalid in {:?}",
            valid_count, invalid_count, start.elapsed()
        );
    }

    /// Get and clear the set of dirty (changed) pairs since last scan
    pub fn take_dirty_pairs(&self) -> HashSet<String> {
        std::mem::take(&mut *self.dirty_pairs.write())
    }

    /// Check if graph needs a full scan (no prior scans or major changes)
    pub fn needs_full_scan(&self) -> bool {
        self.build_count.load(Ordering::Relaxed) <= 1 || self.dirty_pairs.read().len() > 50
    }

    /// Get connected base currencies for a set of changed pairs
    pub fn get_affected_bases(&self, changed_pairs: &HashSet<String>) -> HashSet<String> {
        let mut bases = HashSet::new();

        for pair in changed_pairs {
            if let Some(edges) = self.edge_map.get(pair) {
                for edge_idx in edges {
                    if let Some((source, target)) = self.graph.edge_endpoints(*edge_idx) {
                        bases.insert(self.graph[source].clone());
                        bases.insert(self.graph[target].clone());
                    }
                }
            }
        }

        bases
    }

    /// Scan for opportunities starting from specific base currencies
    pub fn scan(
        &self,
        base_currencies: &[String],
        config: &EngineConfig,
    ) -> Vec<Opportunity> {
        // Find opportunities from each base currency in parallel
        let opportunities: Vec<Opportunity> = base_currencies
            .par_iter()
            .flat_map(|base| {
                self.find_opportunities_from(base, config)
            })
            .collect();

        // Deduplicate by path
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

    /// Find opportunities starting from a specific currency
    fn find_opportunities_from(
        &self,
        start: &str,
        config: &EngineConfig,
    ) -> Vec<Opportunity> {
        let start_idx = match self.node_map.get(start) {
            Some(idx) => *idx,
            None => return vec![],
        };

        let mut opportunities = Vec::new();
        let max_legs = 4;

        // DFS to find cycles
        let mut paths: Vec<ArbitragePath> = Vec::new();
        self.dfs_find_cycles(
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

        // Convert paths to opportunities - include ALL paths, not just profitable ones
        // Filtering by is_profitable happens in the caching layer
        for path in paths {
            if let Some(opp) = self.path_to_opportunity(&path, config) {
                // Include all opportunities, let the caller filter by profitability
                opportunities.push(opp);
            }
        }

        opportunities
    }

    /// Iterative DFS to find all cycles back to start
    /// Uses explicit stack to avoid stack overflow with large graphs
    fn dfs_find_cycles(
        &self,
        start: NodeIndex,
        _current: NodeIndex,
        currencies: &mut Vec<String>,
        pairs: &mut Vec<String>,
        actions: &mut Vec<String>,
        rates: &mut Vec<f64>,
        visited_pairs: &mut HashSet<String>,
        max_legs: usize,
        results: &mut Vec<ArbitragePath>,
    ) {
        // Stack entry: (node, edge_index, is_backtrack)
        // edge_index tracks which neighbor to explore next
        // is_backtrack indicates we're returning from a subtree
        struct StackFrame {
            node: NodeIndex,
            edge_iter_index: usize,
            edges: Vec<(NodeIndex, String, String, String, f64)>, // (target, pair, side, currency, rate)
        }

        // Collect edges from start node
        let start_edges: Vec<(NodeIndex, String, String, String, f64)> = self.graph.edges(start)
            .filter_map(|edge| {
                let edge_data = edge.weight();
                if !edge_data.valid || edge_data.rate <= 0.0 {
                    return None;
                }
                let target = edge.target();
                let target_currency = self.graph[target].clone();
                Some((target, edge_data.pair.clone(), edge_data.side.clone(), target_currency, edge_data.rate))
            })
            .collect();

        if start_edges.is_empty() {
            return;
        }

        let mut stack: Vec<StackFrame> = vec![StackFrame {
            node: start,
            edge_iter_index: 0,
            edges: start_edges,
        }];

        while let Some(frame) = stack.last_mut() {
            // Check if we've explored all edges at this level
            if frame.edge_iter_index >= frame.edges.len() {
                // Backtrack: pop this frame and undo state
                stack.pop();
                if !pairs.is_empty() {
                    let removed_pair = pairs.pop().unwrap();
                    currencies.pop();
                    actions.pop();
                    rates.pop();
                    visited_pairs.remove(&removed_pair);
                }
                continue;
            }

            // Get next edge to explore
            let (target, pair, side, target_currency, rate) = frame.edges[frame.edge_iter_index].clone();
            frame.edge_iter_index += 1;

            // Check constraints
            if currencies.len() >= max_legs + 1 {
                continue;
            }

            if visited_pairs.contains(&pair) {
                continue;
            }

            // Don't revisit currencies except start
            if target != start && currencies.contains(&target_currency) {
                continue;
            }

            // Check if we found a cycle back to start
            if target == start && !pairs.is_empty() {
                // We have a valid cycle - record it
                let mut final_currencies = currencies.clone();
                final_currencies.push(target_currency.clone());

                let mut final_pairs = pairs.clone();
                final_pairs.push(pair.clone());

                let mut final_actions = actions.clone();
                final_actions.push(side.clone());

                let mut final_rates = rates.clone();
                final_rates.push(rate);

                results.push(ArbitragePath {
                    currencies: final_currencies,
                    pairs: final_pairs,
                    actions: final_actions,
                    rates: final_rates,
                });
                continue;
            }

            // Not back to start - push state and explore further
            currencies.push(target_currency.clone());
            pairs.push(pair.clone());
            actions.push(side.clone());
            rates.push(rate);
            visited_pairs.insert(pair.clone());

            // Collect edges from target node for next frame
            let next_edges: Vec<(NodeIndex, String, String, String, f64)> = self.graph.edges(target)
                .filter_map(|edge| {
                    let edge_data = edge.weight();
                    if !edge_data.valid || edge_data.rate <= 0.0 {
                        return None;
                    }
                    let next_target = edge.target();
                    let next_currency = self.graph[next_target].clone();
                    Some((next_target, edge_data.pair.clone(), edge_data.side.clone(), next_currency, edge_data.rate))
                })
                .collect();

            stack.push(StackFrame {
                node: target,
                edge_iter_index: 0,
                edges: next_edges,
            });
        }
    }

    /// Convert a path to an Opportunity
    fn path_to_opportunity(
        &self,
        path: &ArbitragePath,
        config: &EngineConfig,
    ) -> Option<Opportunity> {
        if path.rates.is_empty() {
            return None;
        }

        let start_amount = 1.0;

        // Calculate final amount by multiplying all rates
        let mut amount = start_amount;
        for rate in &path.rates {
            amount *= rate;
        }

        // Calculate fees
        let fee_per_leg = config.fee_rate;
        let total_legs = path.pairs.len();
        let fees_pct = fee_per_leg * 100.0 * total_legs as f64;

        // Apply fees
        for _ in 0..total_legs {
            amount *= 1.0 - fee_per_leg;
        }

        // Calculate profits
        let gross_profit_pct = (path.rates.iter().product::<f64>() - 1.0) * 100.0;
        let net_profit_pct = (amount - start_amount) / start_amount * 100.0;

        // Reject unrealistic profits
        const MAX_REALISTIC_PROFIT_PCT: f64 = 5.0;
        if gross_profit_pct.abs() > MAX_REALISTIC_PROFIT_PCT {
            return None;
        }

        let is_profitable = net_profit_pct > config.min_profit_threshold * 100.0;
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
            fee_rate: config.fee_rate,
            fee_source: config.fee_source.clone(),
            legs_detail,
        })
    }

    /// Get statistics
    pub fn get_stats(&self) -> (u64, u64, usize, usize) {
        (
            self.build_count.load(Ordering::Relaxed),
            self.update_count.load(Ordering::Relaxed),
            self.node_map.len(),
            self.graph.edge_count(),  // Actual edge count (should be 2x pair count)
        )
    }

    /// Get detailed stats including valid edge count
    pub fn get_detailed_stats(&self) -> (usize, usize, usize, usize) {
        let total_edges = self.graph.edge_count();
        let valid_edges = self.graph.edge_weights()
            .filter(|e| e.valid && e.rate > 0.0)
            .count();
        let nodes = self.node_map.len();
        let pairs = self.edge_map.len();
        (nodes, pairs, total_edges, valid_edges)
    }

    /// Debug: count paths found from a currency (without converting to opportunities)
    pub fn count_paths_from(&self, start: &str) -> usize {
        let start_idx = match self.node_map.get(start) {
            Some(idx) => *idx,
            None => return 0,
        };

        let mut paths: Vec<ArbitragePath> = Vec::new();
        self.dfs_find_cycles(
            start_idx,
            start_idx,
            &mut vec![start.to_string()],
            &mut vec![],
            &mut vec![],
            &mut vec![],
            &mut HashSet::new(),
            4,
            &mut paths,
        );

        paths.len()
    }

    /// Debug: Get connected currencies from a base
    pub fn get_connected_currencies(&self, start: &str) -> Vec<String> {
        let start_idx = match self.node_map.get(start) {
            Some(idx) => *idx,
            None => return vec![],
        };

        self.graph.edges(start_idx)
            .filter(|e| e.weight().valid && e.weight().rate > 0.0)
            .map(|e| self.graph[e.target()].clone())
            .collect()
    }

    /// Get health stats
    pub fn get_health(&self) -> OrderBookHealth {
        self.health.read().clone()
    }

    /// Update health stats from current graph state
    pub fn update_health(&self) {
        let mut valid_pairs = 0u32;
        let mut invalid_pairs = 0u32;

        for edges in self.edge_map.values() {
            if let Some(edge_idx) = edges.first() {
                if let Some(edge) = self.graph.edge_weight(*edge_idx) {
                    if edge.valid {
                        valid_pairs += 1;
                    } else {
                        invalid_pairs += 1;
                    }
                }
            }
        }

        let mut health = self.health.write();
        health.total_pairs = valid_pairs + invalid_pairs;
        health.valid_pairs = valid_pairs;
        health.last_update = Utc::now().to_rfc3339();
    }

    /// Update health stats from cache with detailed skip reasons
    pub fn update_health_from_cache(&self, cache: &OrderBookCache) {
        // Health check thresholds
        let min_depth: usize = 3;
        let max_staleness_ms: i64 = 5000;
        let max_spread_pct: f64 = 10.0;

        let mut valid_pairs = 0u32;
        let mut no_orderbook = 0u32;
        let mut thin_depth = 0u32;
        let mut stale = 0u32;
        let mut bad_spread = 0u32;
        let mut no_price = 0u32;

        let mut total_freshness_ms = 0.0f64;
        let mut total_spread_pct = 0.0f64;
        let mut total_depth = 0.0f64;
        let mut count_for_avg = 0u32;

        for pair in self.edge_map.keys() {
            if let Some(book) = cache.get_order_book(pair) {
                let (best_bid, best_ask) = match (book.best_bid(), book.best_ask()) {
                    (Some(b), Some(a)) => (b, a),
                    _ => {
                        no_price += 1;
                        continue;
                    }
                };

                // Check depth using bids/asks length
                let bid_depth = book.bids.len();
                let ask_depth = book.asks.len();
                if bid_depth < min_depth || ask_depth < min_depth {
                    thin_depth += 1;
                    continue;
                }

                // Check staleness
                let staleness = book.staleness_ms();
                if staleness > max_staleness_ms {
                    stale += 1;
                    continue;
                }

                // Check spread
                let spread_pct = (best_ask - best_bid) / best_bid * 100.0;
                if spread_pct > max_spread_pct {
                    bad_spread += 1;
                    continue;
                }

                // Valid pair
                valid_pairs += 1;
                total_freshness_ms += staleness as f64;
                total_spread_pct += spread_pct;
                total_depth += (bid_depth + ask_depth) as f64 / 2.0;
                count_for_avg += 1;
            } else {
                no_orderbook += 1;
            }
        }

        let mut health = self.health.write();
        health.total_pairs = self.edge_map.len() as u32;
        health.valid_pairs = valid_pairs;
        health.skipped_no_orderbook = no_orderbook;
        health.skipped_thin_depth = thin_depth;
        health.skipped_stale = stale;
        health.skipped_bad_spread = bad_spread;
        health.skipped_no_price = no_price;

        if count_for_avg > 0 {
            health.avg_freshness_ms = total_freshness_ms / count_for_avg as f64;
            health.avg_spread_pct = total_spread_pct / count_for_avg as f64;
            health.avg_depth = total_depth / count_for_avg as f64;
        }

        health.last_update = Utc::now().to_rfc3339();
    }
}

/// Internal representation of an arbitrage path
#[derive(Debug, Clone)]
struct ArbitragePath {
    currencies: Vec<String>,
    pairs: Vec<String>,
    actions: Vec<String>,
    rates: Vec<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_persistent_graph_creation() {
        let graph = PersistentGraph::new();
        assert_eq!(graph.node_map.len(), 0);
        assert_eq!(graph.edge_map.len(), 0);
    }
}
