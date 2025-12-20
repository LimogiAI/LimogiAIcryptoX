"""
Application configuration using Pydantic Settings

CONFIGURATION HIERARCHY (highest to lowest priority):
1. Environment variables (from .env or docker)
2. Default values in this file

NOTE: docker-compose.yml should only pass-through variables, not define defaults.
      All defaults should be defined HERE as the single source of truth.
"""
from pydantic_settings import BaseSettings
from typing import Optional, List
from functools import lru_cache
import os


class Settings(BaseSettings):
    """Application settings loaded from environment variables"""

    # ===========================================
    # Database
    # ===========================================
    database_url: str = "postgresql://krakencryptox:krakencryptox123@localhost:5432/krakencryptox"

    # ===========================================
    # Kraken API - Live Trading
    # Loaded from .env.live
    # ===========================================
    kraken_live_api_key: Optional[str] = None
    kraken_live_private_key: Optional[str] = None
    kraken_max_loss_usd: float = 30.0     # Hard limit for live trading

    # Kraken URLs
    kraken_ws_url: str = "wss://ws.kraken.com"
    kraken_rest_url: str = "https://api.kraken.com"

    # ===========================================
    # Trading Engine Settings
    # ===========================================

    # Capital Management
    total_capital: float = 100.0          # Starting capital (USD)

    # Timing (milliseconds)
    cooldown_ms: int = 0                  # Cooldown between trades (0 = no cooldown)
    path_cooldown_ms: int = 3000          # Cooldown per path after trade (3 seconds)
    scan_interval_ms: int = 10000         # Scan every 10 seconds

    # Order Book
    orderbook_depth: int = 25             # Levels to fetch (25 for better depth)
    max_pairs: int = 300                  # Top 300 pairs by volume to monitor

    # Staleness Thresholds (milliseconds)
    staleness_warn_ms: int = 500          # Warn if data older than 500ms
    staleness_buffer_ms: int = 1000       # Add 1% buffer if older than 1s
    staleness_reject_ms: int = 2000       # Reject if older than 2s

    # ===========================================
    # Trading Parameters
    # ===========================================
    fee_rate_taker: float = 0.0026        # 0.26% Kraken taker fee
    fee_rate_maker: float = 0.0016        # 0.16% Kraken maker fee
    min_profit_threshold: float = 0.0005  # 0.05% minimum profit to consider
    trade_amount: float = 10.0            # Default trade amount (USD)
    max_trades_per_cycle: int = 5         # Maximum trades per scan cycle
    max_path_legs: int = 4                # Maximum legs in arbitrage path

    # Base currencies to scan for arbitrage cycles
    base_currencies: List[str] = ["USD", "USDT", "EUR", "BTC", "ETH"]

    # ===========================================
    # Circuit Breaker / Kill Switch Settings
    # ===========================================
    kill_switch_enabled: bool = True      # Enable kill switch by default
    max_loss_pct: float = 0.30            # Stop at 30% loss from peak
    max_consecutive_losses: int = 10      # Stop after 10 consecutive losses
    max_daily_loss_pct: float = 0.30      # Stop at 30% daily loss from peak

    # ===========================================
    # Application Settings
    # ===========================================
    log_level: str = "INFO"
    app_name: str = "LimogiAICryptoX"
    debug: bool = False

    class Config:
        env_file = ".env"
        extra = "ignore"
        case_sensitive = False


@lru_cache()
def get_settings() -> Settings:
    """Get cached settings instance, loading from multiple env files"""

    # Load env files in order of priority (later files override earlier)
    env_files = [
        ".env.live",             # Kraken API keys (live trading)
        "/app/.env.live",        # Docker path for Kraken keys (live trading)
        ".env.runtime",          # Runtime settings saved from UI
        "/app/.env.runtime",     # Docker path for runtime settings
    ]

    for env_path in env_files:
        if os.path.exists(env_path):
            with open(env_path) as f:
                for line in f:
                    line = line.strip()
                    if line and not line.startswith("#") and "=" in line:
                        key, value = line.split("=", 1)
                        os.environ[key.strip()] = value.strip()

    return Settings()


settings = get_settings()
