"""
Live Trading Module for LimogiAICryptoX

This module handles real money trading on Kraken exchange.

Components:
- manager.py: Main orchestrator (LiveTradingManager)
- executor.py: Order execution (LiveExecutor)
- guard.py: Pre-trade safety checks (TradeGuard)
- circuit_breaker.py: Loss limits and emergency stop (CircuitBreaker)
- config.py: Settings management (ConfigManager)
- ui_cache.py: UI cache manager (fetches from Rust, NOT a scanner)

NOTE: All scanning happens in Rust engine (rust_engine/src/).
The ui_cache.py just fetches cached data from Rust for UI display.
"""

from .manager import LiveTradingManager, get_live_trading_manager, initialize_live_trading
from .executor import LiveExecutor
from .guard import TradeGuard
from .circuit_breaker import CircuitBreaker
from .config import ConfigManager
from .ui_cache import (
    UICacheManager,
    get_ui_cache,
    initialize_ui_cache,
    start_ui_cache,
    stop_ui_cache,
)

__all__ = [
    'LiveTradingManager',
    'get_live_trading_manager',
    'initialize_live_trading',
    'LiveExecutor',
    'TradeGuard',
    'CircuitBreaker',
    'ConfigManager',
    'UICacheManager',
    'get_ui_cache',
    'initialize_ui_cache',
    'start_ui_cache',
    'stop_ui_cache',
]
