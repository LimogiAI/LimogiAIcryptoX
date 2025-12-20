"""
Live Trading Module for KrakenCryptoX

This module handles real money trading on Kraken exchange.
It is completely independent from paper trading and shadow mode.

Components:
- manager.py: Main orchestrator (LiveTradingManager)
- executor.py: Order execution (LiveExecutor)
- guard.py: Pre-trade safety checks (TradeGuard)
- circuit_breaker.py: Loss limits and emergency stop (CircuitBreaker)
- config.py: Settings management (ConfigManager)
- scanner.py: Independent scanner for live trading opportunities
"""

from .manager import LiveTradingManager, get_live_trading_manager, initialize_live_trading
from .executor import LiveExecutor
from .guard import TradeGuard
from .circuit_breaker import CircuitBreaker
from .config import ConfigManager
from .scanner import (
    LiveTradingScanner, 
    get_live_scanner, 
    initialize_live_scanner,
    start_live_scanner,
    stop_live_scanner,
)

__all__ = [
    'LiveTradingManager',
    'get_live_trading_manager',
    'initialize_live_trading',
    'LiveExecutor',
    'TradeGuard',
    'CircuitBreaker',
    'ConfigManager',
    'LiveTradingScanner',
    'get_live_scanner',
    'initialize_live_scanner',
    'start_live_scanner',
    'stop_live_scanner',
]
