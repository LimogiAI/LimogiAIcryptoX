//! Trading module - Unified trading engine
//!
//! This module wraps the existing Rust trading components
//! (scanner, executor, order book, etc.) into a single
//! cohesive interface for the API layer.

mod engine;

pub use engine::TradingEngine;
