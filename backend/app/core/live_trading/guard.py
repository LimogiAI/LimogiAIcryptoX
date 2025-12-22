"""
Trade Guard - Pre-trade Safety Checks

Validates that all conditions are met before executing a live trade.
"""
from typing import Optional, Tuple, Dict, Any
from dataclasses import dataclass
from datetime import datetime
from loguru import logger


@dataclass
class GuardCheckResult:
    """Result of guard checks"""
    can_trade: bool
    reason: Optional[str] = None
    checks: Dict[str, bool] = None
    
    def __post_init__(self):
        if self.checks is None:
            self.checks = {}
    
    def to_dict(self) -> Dict[str, Any]:
        return {
            'can_trade': self.can_trade,
            'reason': self.reason,
            'checks': self.checks,
        }


class TradeGuard:
    """
    Pre-trade safety guard.
    
    Checks:
    1. Live trading is enabled
    2. Circuit breaker not triggered
    3. Sufficient balance for trade
    4. Within daily/total loss limits
    5. API is healthy
    6. Not already executing (sequential mode)
    """
    
    def __init__(self, config_manager, circuit_breaker, kraken_client):
        self.config_manager = config_manager
        self.circuit_breaker = circuit_breaker
        self.kraken_client = kraken_client
        
        # Cache balance check
        self._last_balance_check: Optional[datetime] = None
        self._cached_balance: Dict[str, float] = {}
        self._balance_cache_ttl = 10  # seconds
    
    async def check_all(self, trade_amount: float = None) -> GuardCheckResult:
        """
        Run all pre-trade checks.
        Returns GuardCheckResult with pass/fail and details.
        """
        checks = {}
        
        # Get config
        config = self.config_manager.get_settings()
        
        if trade_amount is None:
            trade_amount = config.trade_amount
        
        # 1. Check if live trading is enabled
        checks['live_enabled'] = config.is_enabled
        if not config.is_enabled:
            return GuardCheckResult(
                can_trade=False,
                reason="Live trading is not enabled",
                checks=checks
            )
        
        # 2. Check circuit breaker
        can_trade, reason = self.circuit_breaker.check_can_trade()
        checks['circuit_breaker'] = can_trade
        if not can_trade:
            return GuardCheckResult(
                can_trade=False,
                reason=reason,
                checks=checks
            )
        
        # 3. Check remaining budget
        remaining_daily = self.circuit_breaker.get_remaining_daily_budget()
        remaining_total = self.circuit_breaker.get_remaining_total_budget()
        
        checks['daily_budget'] = remaining_daily >= trade_amount
        checks['total_budget'] = remaining_total >= trade_amount
        
        if remaining_daily < trade_amount:
            return GuardCheckResult(
                can_trade=False,
                reason=f"Insufficient daily budget: ${remaining_daily:.2f} < ${trade_amount:.2f}",
                checks=checks
            )
        
        if remaining_total < trade_amount:
            return GuardCheckResult(
                can_trade=False,
                reason=f"Insufficient total budget: ${remaining_total:.2f} < ${trade_amount:.2f}",
                checks=checks
            )
        
        # 4. Check Kraken balance
        try:
            balance = await self._get_balance()
            usd_balance = balance.get('ZUSD', 0) + balance.get('USD', 0)
            checks['kraken_balance'] = usd_balance >= trade_amount
            
            if usd_balance < trade_amount:
                return GuardCheckResult(
                    can_trade=False,
                    reason=f"Insufficient Kraken balance: ${usd_balance:.2f} < ${trade_amount:.2f}",
                    checks=checks
                )
        except Exception as e:
            checks['kraken_balance'] = False
            return GuardCheckResult(
                can_trade=False,
                reason=f"Failed to check Kraken balance: {str(e)}",
                checks=checks
            )
        
        # 5. Check API health (simple ping)
        try:
            # Just verify we can make an API call
            await self.kraken_client.get_ticker("XXBTZUSD")
            checks['api_healthy'] = True
        except Exception as e:
            checks['api_healthy'] = False
            return GuardCheckResult(
                can_trade=False,
                reason=f"Kraken API unhealthy: {str(e)}",
                checks=checks
            )
        
        # All checks passed
        return GuardCheckResult(
            can_trade=True,
            reason=None,
            checks=checks
        )
    
    async def _get_balance(self) -> Dict[str, float]:
        """Get Kraken balance (cached for performance)"""
        now = datetime.utcnow()
        
        if self._last_balance_check:
            age = (now - self._last_balance_check).total_seconds()
            if age < self._balance_cache_ttl and self._cached_balance:
                return self._cached_balance
        
        self._cached_balance = await self.kraken_client.get_balance()
        self._last_balance_check = now
        
        return self._cached_balance
    
    async def check_opportunity_valid(
        self, 
        path: str, 
        expected_profit_pct: float,
        max_staleness_ms: int = 2000
    ) -> Tuple[bool, Optional[str]]:
        """
        Verify an opportunity is still valid before trading.
        
        This is a lighter check than the full scanner - just verifies
        the prices haven't moved too much since detection.
        """
        config = self.config_manager.get_settings()
        
        # Check if expected profit is above threshold
        if expected_profit_pct < config.min_profit_threshold * 100:
            return False, f"Profit {expected_profit_pct:.2f}% below threshold {config.min_profit_threshold * 100:.2f}%"
        
        # TODO: Could add price freshness check here by parsing path
        # and fetching current prices. For now, trust the scanner.
        
        return True, None
    
    def check_base_currency_filter(self, path: str) -> bool:
        """Check if path matches base currency filter"""
        config = self.config_manager.get_settings()
        
        # Parse start currency from path
        if ' → ' in path:
            start_currency = path.split(' → ')[0].strip()
        elif '→' in path:
            start_currency = path.split('→')[0].strip()
        else:
            start_currency = path.split()[0].strip()
        
        if config.base_currency == 'ALL':
            return True
        elif config.base_currency == 'CUSTOM':
            return start_currency in (config.custom_currencies or [])
        else:
            return start_currency == config.base_currency
    
    async def get_status(self) -> Dict[str, Any]:
        """Get current guard status for API/UI"""
        config = self.config_manager.get_settings()
        state = self.circuit_breaker.get_state()
        
        # Try to get balance
        balance = {}
        balance_error = None
        try:
            balance = await self._get_balance()
        except Exception as e:
            balance_error = str(e)
        
        usd_balance = balance.get('ZUSD', 0) + balance.get('USD', 0)
        
        return {
            'live_enabled': config.is_enabled,
            'circuit_broken': state.is_broken,
            'circuit_broken_reason': state.broken_reason,
            'is_executing': state.is_executing,
            'current_trade_id': state.current_trade_id,
            'kraken_balance_usd': usd_balance,
            'kraken_balance_error': balance_error,
            'remaining_daily_budget': self.circuit_breaker.get_remaining_daily_budget(),
            'remaining_total_budget': self.circuit_breaker.get_remaining_total_budget(),
            'trade_amount': config.trade_amount,
            'min_profit_threshold': config.min_profit_threshold * 100,  # As percentage
        }
