//! Slot manager for parallel paper trading

use crate::types::{EngineConfig, Slot};
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::info;

/// Internal slot state
struct SlotState {
    id: usize,
    balance: f64,
    initial_balance: f64,
    status: SlotStatus,
    cooldown_until: Option<DateTime<Utc>>,
    trades_count: u64,
    wins_count: u64,
    total_profit: f64,
    current_opportunity_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SlotStatus {
    Ready,
    Executing,
    Cooldown,
}

impl SlotStatus {
    fn as_str(&self) -> &'static str {
        match self {
            SlotStatus::Ready => "READY",
            SlotStatus::Executing => "EXECUTING",
            SlotStatus::Cooldown => "COOLDOWN",
        }
    }
}

/// Slot manager statistics
pub struct SlotStats {
    pub total_trades: u64,
    pub total_profit: f64,
    pub win_rate: f64,
}

/// Manages multiple trading slots
pub struct SlotManager {
    slots: RwLock<Vec<SlotState>>,
    config: RwLock<EngineConfig>,
    total_trades: AtomicU64,
    total_wins: AtomicU64,
}

impl SlotManager {
    pub fn new(config: EngineConfig) -> Self {
        let slot_amount = config.slot_amount;
        let slot_count = config.slot_count;
        
        let slots: Vec<SlotState> = (0..slot_count)
            .map(|id| SlotState {
                id,
                balance: slot_amount,
                initial_balance: slot_amount,
                status: SlotStatus::Ready,
                cooldown_until: None,
                trades_count: 0,
                wins_count: 0,
                total_profit: 0.0,
                current_opportunity_id: None,
            })
            .collect();
        
        info!("Initialized {} slots with ${:.2} each", slot_count, slot_amount);
        
        Self {
            slots: RwLock::new(slots),
            config: RwLock::new(config),
            total_trades: AtomicU64::new(0),
            total_wins: AtomicU64::new(0),
        }
    }

    /// Update the trade amount for all slots
    pub fn update_slot_amount(&self, amount: f64) {
        let mut config = self.config.write();
        config.slot_amount = amount;
        info!("Updated slot trade amount to ${:.2}", amount);
    }

    /// Get current trade amount
    pub fn get_trade_amount(&self) -> f64 {
        self.config.read().slot_amount
    }

    /// Get slot cooldown in ms
    pub fn get_cooldown_ms(&self) -> u64 {
        self.config.read().slot_cooldown_ms
    }

    /// Get all slots as API response
    pub fn get_slots(&self) -> Vec<Slot> {
        let slots = self.slots.read();
        slots
            .iter()
            .map(|s| Slot {
                id: s.id,
                balance: s.balance,
                initial_balance: s.initial_balance,
                status: s.status.as_str().to_string(),
                cooldown_until: s.cooldown_until.map(|t| t.to_rfc3339()),
                trades_count: s.trades_count,
                wins_count: s.wins_count,
                win_rate: if s.trades_count > 0 {
                    s.wins_count as f64 / s.trades_count as f64 * 100.0
                } else {
                    0.0
                },
                total_profit: s.total_profit,
                current_opportunity_id: s.current_opportunity_id.clone(),
            })
            .collect()
    }

    /// Get IDs of slots that are ready to trade
    pub fn get_ready_slots(&self) -> Vec<usize> {
        let mut slots = self.slots.write();
        let now = Utc::now();
        let trade_amount = self.get_trade_amount();
        
        // Update cooldown status
        for slot in slots.iter_mut() {
            if slot.status == SlotStatus::Cooldown {
                if let Some(until) = slot.cooldown_until {
                    if now >= until {
                        slot.status = SlotStatus::Ready;
                        slot.cooldown_until = None;
                    }
                }
            }
        }
        
        // Return ready slots that have sufficient balance
        slots
            .iter()
            .filter(|s| s.status == SlotStatus::Ready && s.balance >= trade_amount)
            .map(|s| s.id)
            .collect()
    }

    /// Reserve a slot for trading
    pub fn reserve_slot(&self, slot_id: usize, opportunity_id: &str) -> bool {
        let mut slots = self.slots.write();
        let trade_amount = self.get_trade_amount();
        
        if let Some(slot) = slots.get_mut(slot_id) {
            if slot.status == SlotStatus::Ready && slot.balance >= trade_amount {
                slot.status = SlotStatus::Executing;
                slot.current_opportunity_id = Some(opportunity_id.to_string());
                return true;
            }
        }
        false
    }

    /// Complete a trade and update slot
    pub fn complete_trade(&self, slot_id: usize, profit_amount: f64, is_win: bool) {
        let mut slots = self.slots.write();
        let cooldown_ms = self.get_cooldown_ms();
        
        if let Some(slot) = slots.get_mut(slot_id) {
            slot.balance += profit_amount;
            slot.trades_count += 1;
            slot.total_profit += profit_amount;
            
            if is_win {
                slot.wins_count += 1;
                self.total_wins.fetch_add(1, Ordering::Relaxed);
            }
            
            // Set cooldown
            slot.status = SlotStatus::Cooldown;
            slot.cooldown_until = Some(Utc::now() + chrono::Duration::milliseconds(cooldown_ms as i64));
            slot.current_opportunity_id = None;
            
            self.total_trades.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Release a slot without completing trade (error case)
    pub fn release_slot(&self, slot_id: usize) {
        let mut slots = self.slots.write();
        if let Some(slot) = slots.get_mut(slot_id) {
            slot.status = SlotStatus::Ready;
            slot.current_opportunity_id = None;
        }
    }

    /// Get slot balance
    pub fn get_slot_balance(&self, slot_id: usize) -> Option<f64> {
        let slots = self.slots.read();
        slots.get(slot_id).map(|s| s.balance)
    }

    /// Get total balance across all slots
    pub fn get_total_balance(&self) -> f64 {
        let slots = self.slots.read();
        slots.iter().map(|s| s.balance).sum()
    }

    /// Get total profit across all slots
    pub fn get_total_profit(&self) -> f64 {
        let slots = self.slots.read();
        slots.iter().map(|s| s.total_profit).sum()
    }

    /// Get overall win rate
    pub fn get_win_rate(&self) -> f64 {
        let total = self.total_trades.load(Ordering::Relaxed);
        if total == 0 {
            return 0.0;
        }
        let wins = self.total_wins.load(Ordering::Relaxed);
        wins as f64 / total as f64 * 100.0
    }

    /// Get statistics
    pub fn get_stats(&self) -> SlotStats {
        SlotStats {
            total_trades: self.total_trades.load(Ordering::Relaxed),
            total_profit: self.get_total_profit(),
            win_rate: self.get_win_rate(),
        }
    }

    /// Reset all slots to initial state
    pub fn reset(&self, total_capital: f64) {
        let mut slots = self.slots.write();
        let slot_count = slots.len();
        let slot_amount = total_capital / slot_count as f64;
        
        for slot in slots.iter_mut() {
            slot.balance = slot_amount;
            slot.initial_balance = slot_amount;
            slot.status = SlotStatus::Ready;
            slot.cooldown_until = None;
            slot.trades_count = 0;
            slot.wins_count = 0;
            slot.total_profit = 0.0;
            slot.current_opportunity_id = None;
        }
        
        self.total_trades.store(0, Ordering::Relaxed);
        self.total_wins.store(0, Ordering::Relaxed);
        
        info!("Reset {} slots with ${:.2} each", slot_count, slot_amount);
    }

    /// Rebalance slots to equal amounts
    pub fn rebalance(&self) {
        let mut slots = self.slots.write();
        let total: f64 = slots.iter().map(|s| s.balance).sum();
        let per_slot = total / slots.len() as f64;
        
        for slot in slots.iter_mut() {
            slot.balance = per_slot;
        }
        
        info!("Rebalanced {} slots to ${:.2} each", slots.len(), per_slot);
    }
}
