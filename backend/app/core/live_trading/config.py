"""
Live Trading Configuration Manager

Handles loading/saving live trading settings from database.
All settings are user-configurable, no hardcoded values.
"""
from typing import Optional, List, Dict, Any
from dataclasses import dataclass, asdict
from datetime import datetime
from loguru import logger


@dataclass
class LiveTradingSettings:
    """Live trading configuration settings"""
    # Enable/disable
    is_enabled: bool = False
    
    # Trade parameters
    trade_amount: float = 10.0
    min_profit_threshold: float = 0.003  # 0.3%
    
    # Loss limits
    max_daily_loss: float = 30.0
    max_total_loss: float = 30.0
    
    # Order execution
    max_retries_per_leg: int = 2
    order_timeout_seconds: int = 15
    
    # Base currency filter
    base_currency: str = 'USD'
    custom_currencies: List[str] = None
    
    # Timestamps
    enabled_at: Optional[datetime] = None
    disabled_at: Optional[datetime] = None
    
    def __post_init__(self):
        if self.custom_currencies is None:
            self.custom_currencies = []
    
    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary for API response"""
        return {
            'is_enabled': self.is_enabled,
            'trade_amount': self.trade_amount,
            'min_profit_threshold': self.min_profit_threshold,
            'max_daily_loss': self.max_daily_loss,
            'max_total_loss': self.max_total_loss,
            'max_retries_per_leg': self.max_retries_per_leg,
            'order_timeout_seconds': self.order_timeout_seconds,
            'base_currency': self.base_currency,
            'custom_currencies': self.custom_currencies,
            'enabled_at': self.enabled_at.isoformat() if self.enabled_at else None,
            'disabled_at': self.disabled_at.isoformat() if self.disabled_at else None,
        }


# Valid options for settings (for UI dropdowns)
TRADE_AMOUNT_OPTIONS = [5.0, 10.0, 15.0, 20.0, 25.0, 50.0, 75.0, 100.0]
TRADE_AMOUNT_MIN = 5.0
TRADE_AMOUNT_MAX = 100.0

MIN_PROFIT_OPTIONS = [0.0, 0.10, 0.20, 0.30, 0.40, 0.50, 0.60, 0.70, 0.80, 0.90]  # Percentages
MIN_PROFIT_MIN = 0.0
MIN_PROFIT_MAX = 0.90

MAX_LOSS_MIN = 10.0
MAX_LOSS_MAX = 200.0

BASE_CURRENCY_OPTIONS = ['ALL', 'USD', 'EUR', 'USDT', 'BTC', 'ETH', 'CUSTOM']


class ConfigManager:
    """Manages live trading configuration from database"""
    
    def __init__(self, db_session_factory):
        self.db_session_factory = db_session_factory
        self._cached_settings: Optional[LiveTradingSettings] = None
        self._cache_time: Optional[datetime] = None
        self._cache_ttl_seconds = 5  # Refresh cache every 5 seconds
    
    def _get_db(self):
        """Get a database session"""
        return self.db_session_factory()
    
    def get_settings(self, force_refresh: bool = False) -> LiveTradingSettings:
        """Get current settings (cached)"""
        now = datetime.utcnow()
        
        # Return cached if still valid
        if not force_refresh and self._cached_settings and self._cache_time:
            age = (now - self._cache_time).total_seconds()
            if age < self._cache_ttl_seconds:
                return self._cached_settings
        
        # Load from database
        db = self._get_db()
        try:
            from app.models.live_trading import LiveTradingConfig
            
            config = db.query(LiveTradingConfig).filter(LiveTradingConfig.id == 1).first()
            
            if not config:
                # Create default config
                config = LiveTradingConfig(id=1)
                db.add(config)
                db.commit()
                db.refresh(config)
            
            self._cached_settings = LiveTradingSettings(
                is_enabled=config.is_enabled,
                trade_amount=config.trade_amount,
                min_profit_threshold=config.min_profit_threshold,
                max_daily_loss=config.max_daily_loss,
                max_total_loss=config.max_total_loss,
                max_retries_per_leg=config.max_retries_per_leg,
                order_timeout_seconds=config.order_timeout_seconds,
                base_currency=config.base_currency,
                custom_currencies=config.custom_currencies or [],
                enabled_at=config.enabled_at,
                disabled_at=config.disabled_at,
            )
            self._cache_time = now
            
            return self._cached_settings
            
        except Exception as e:
            logger.error(f"Error loading live trading config: {e}")
            # Return default settings on error
            return LiveTradingSettings()
        finally:
            db.close()
    
    def update_settings(self, updates: Dict[str, Any]) -> LiveTradingSettings:
        """Update settings in database"""
        db = self._get_db()
        try:
            from app.models.live_trading import LiveTradingConfig
            
            config = db.query(LiveTradingConfig).filter(LiveTradingConfig.id == 1).first()
            
            if not config:
                config = LiveTradingConfig(id=1)
                db.add(config)
            
            # Validate and apply updates
            if 'trade_amount' in updates:
                val = float(updates['trade_amount'])
                if val > 0:
                    config.trade_amount = val
                else:
                    raise ValueError("trade_amount must be greater than 0")
            
            if 'min_profit_threshold' in updates:
                # Accept as percentage (0-0.9) or as decimal (0-0.009)
                val = float(updates['min_profit_threshold'])
                # If value is > 0.01, assume it's a percentage like 0.3 meaning 0.3%
                if val > 0.01:
                    val = val / 100  # Convert 0.3 -> 0.003
                if 0 <= val <= 0.009:  # 0% to 0.9%
                    config.min_profit_threshold = val
                else:
                    raise ValueError(f"min_profit_threshold must be between 0 and 0.9%")
            
            if 'max_daily_loss' in updates:
                val = float(updates['max_daily_loss'])
                if MAX_LOSS_MIN <= val <= MAX_LOSS_MAX:
                    config.max_daily_loss = val
                else:
                    raise ValueError(f"max_daily_loss must be between {MAX_LOSS_MIN} and {MAX_LOSS_MAX}")
            
            if 'max_total_loss' in updates:
                val = float(updates['max_total_loss'])
                if MAX_LOSS_MIN <= val <= MAX_LOSS_MAX:
                    config.max_total_loss = val
                else:
                    raise ValueError(f"max_total_loss must be between {MAX_LOSS_MIN} and {MAX_LOSS_MAX}")
            
            if 'max_retries_per_leg' in updates:
                val = int(updates['max_retries_per_leg'])
                if 0 <= val <= 5:
                    config.max_retries_per_leg = val
            
            if 'order_timeout_seconds' in updates:
                val = int(updates['order_timeout_seconds'])
                if 10 <= val <= 120:
                    config.order_timeout_seconds = val
            
            if 'base_currency' in updates:
                val = updates['base_currency']
                if val in BASE_CURRENCY_OPTIONS:
                    config.base_currency = val
            
            if 'custom_currencies' in updates:
                config.custom_currencies = updates['custom_currencies']
            
            db.commit()
            
            # Invalidate cache
            self._cached_settings = None
            self._cache_time = None
            
            logger.info(f"Updated live trading config: {updates}")
            
            return self.get_settings(force_refresh=True)
            
        except Exception as e:
            db.rollback()
            logger.error(f"Error updating live trading config: {e}")
            raise
        finally:
            db.close()
    
    def enable(self) -> LiveTradingSettings:
        """Enable live trading"""
        db = self._get_db()
        try:
            from app.models.live_trading import LiveTradingConfig
            
            config = db.query(LiveTradingConfig).filter(LiveTradingConfig.id == 1).first()
            if config:
                config.is_enabled = True
                config.enabled_at = datetime.utcnow()
                config.disabled_at = None
                db.commit()
            
            self._cached_settings = None
            logger.warning("⚠️ LIVE TRADING ENABLED - Real money will be used!")
            
            return self.get_settings(force_refresh=True)
            
        finally:
            db.close()
    
    def disable(self, reason: str = "Manual disable") -> LiveTradingSettings:
        """Disable live trading"""
        db = self._get_db()
        try:
            from app.models.live_trading import LiveTradingConfig
            
            config = db.query(LiveTradingConfig).filter(LiveTradingConfig.id == 1).first()
            if config:
                config.is_enabled = False
                config.disabled_at = datetime.utcnow()
                db.commit()
            
            self._cached_settings = None
            logger.info(f"Live trading disabled: {reason}")
            
            return self.get_settings(force_refresh=True)
            
        finally:
            db.close()
    
    def get_options(self) -> Dict[str, Any]:
        """Get valid options for UI dropdowns"""
        return {
            'trade_amount': {
                'presets': TRADE_AMOUNT_OPTIONS,
                'min': TRADE_AMOUNT_MIN,
                'max': TRADE_AMOUNT_MAX,
                'allow_custom': True,
            },
            'min_profit_threshold': {
                'options': MIN_PROFIT_OPTIONS,
                'min': MIN_PROFIT_MIN,
                'max': MIN_PROFIT_MAX,
                'step': 0.05,
                'unit': '%',
            },
            'max_daily_loss': {
                'min': MAX_LOSS_MIN,
                'max': MAX_LOSS_MAX,
                'unit': 'USD',
            },
            'max_total_loss': {
                'min': MAX_LOSS_MIN,
                'max': MAX_LOSS_MAX,
                'unit': 'USD',
            },
            'base_currency': {
                'options': BASE_CURRENCY_OPTIONS,
            },
        }
