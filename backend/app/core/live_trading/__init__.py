"""
Live Trading Module for LimogiAICryptoX

This module handles real money trading on Kraken exchange.

Components:
- manager.py: Main orchestrator (LiveTradingManager)
- executor.py: Order execution (LiveExecutor)
- guard.py: Pre-trade safety checks (TradeGuard)
- circuit_breaker.py: Loss limits and emergency stop (CircuitBreaker)
- config.py: Settings management (ConfigManager)
- scanner.py: Unified scanner (opportunities, health, trading)
"""

from .manager import LiveTradingManager, get_live_trading_manager, initialize_live_trading
from .executor import LiveExecutor
from .guard import TradeGuard
from .circuit_breaker import CircuitBreaker
from .config import ConfigManager
from .scanner import (
    UnifiedScanner,
    get_scanner,
    initialize_scanner,
    start_scanner,
    stop_scanner,
    # Backwards compatibility aliases
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
    # New unified scanner
    'UnifiedScanner',
    'get_scanner',
    'initialize_scanner',
    'start_scanner',
    'stop_scanner',
    # Backwards compatibility
    'LiveTradingScanner',
    'get_live_scanner',
    'initialize_live_scanner',
    'start_live_scanner',
    'stop_live_scanner',
]
