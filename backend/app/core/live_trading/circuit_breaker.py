"""
Circuit Breaker for Live Trading

Monitors losses and automatically stops trading when limits are exceeded.
This is a critical safety component.
"""
from typing import Optional, Dict, Any
from dataclasses import dataclass
from datetime import datetime, date
from loguru import logger


@dataclass
class CircuitBreakerState:
    """Current circuit breaker state"""
    is_broken: bool = False
    broken_at: Optional[datetime] = None
    broken_reason: Optional[str] = None
    
    daily_loss: float = 0.0
    daily_profit: float = 0.0
    daily_trades: int = 0
    daily_wins: int = 0
    
    total_loss: float = 0.0
    total_profit: float = 0.0
    total_trades: int = 0
    total_wins: int = 0
    total_trade_amount: float = 0.0  # Sum of all trade amounts
    
    last_trade_at: Optional[datetime] = None
    last_daily_reset: Optional[datetime] = None
    
    # Execution lock (for sequential mode)
    is_executing: bool = False
    current_trade_id: Optional[str] = None
    
    def to_dict(self) -> Dict[str, Any]:
        return {
            'is_broken': self.is_broken,
            'broken_at': self.broken_at.isoformat() if self.broken_at else None,
            'broken_reason': self.broken_reason,
            'daily_loss': self.daily_loss,
            'daily_profit': self.daily_profit,
            'daily_net': self.daily_profit - self.daily_loss,
            'daily_trades': self.daily_trades,
            'daily_wins': self.daily_wins,
            'daily_win_rate': (self.daily_wins / self.daily_trades * 100) if self.daily_trades > 0 else 0,
            'total_loss': self.total_loss,
            'total_profit': self.total_profit,
            'total_net': self.total_profit - self.total_loss,
            'total_trades': self.total_trades,
            'total_wins': self.total_wins,
            'total_trade_amount': self.total_trade_amount,
            'total_win_rate': (self.total_wins / self.total_trades * 100) if self.total_trades > 0 else 0,
            'last_trade_at': self.last_trade_at.isoformat() if self.last_trade_at else None,
            'is_executing': self.is_executing,
            'current_trade_id': self.current_trade_id,
        }


class CircuitBreaker:
    """
    Circuit breaker for live trading safety.
    
    Triggers when:
    - Daily loss exceeds max_daily_loss
    - Total loss exceeds max_total_loss
    - Manual trigger
    """
    
    def __init__(self, db_session_factory, config_manager):
        self.db_session_factory = db_session_factory
        self.config_manager = config_manager
        self._cached_state: Optional[CircuitBreakerState] = None
    
    def _get_db(self):
        return self.db_session_factory()
    
    def get_state(self, force_refresh: bool = False) -> CircuitBreakerState:
        """Get current circuit breaker state"""
        db = self._get_db()
        try:
            from app.models.live_trading import LiveTradingState
            
            state = db.query(LiveTradingState).filter(LiveTradingState.id == 1).first()
            
            if not state:
                state = LiveTradingState(id=1)
                db.add(state)
                db.commit()
                db.refresh(state)
            
            # Check if daily reset needed
            self._check_daily_reset(db, state)
            
            return CircuitBreakerState(
                is_broken=state.is_circuit_broken,
                broken_at=state.circuit_broken_at,
                broken_reason=state.circuit_broken_reason,
                daily_loss=state.daily_loss,
                daily_profit=state.daily_profit,
                daily_trades=state.daily_trades,
                daily_wins=state.daily_wins,
                total_loss=state.total_loss,
                total_profit=state.total_profit,
                total_trades=state.total_trades,
                total_wins=state.total_wins,
                total_trade_amount=getattr(state, 'total_trade_amount', 0.0),
                last_trade_at=state.last_trade_at,
                last_daily_reset=state.last_daily_reset,
                is_executing=state.is_executing,
                current_trade_id=state.current_trade_id,
            )
            
        finally:
            db.close()
    
    def _check_daily_reset(self, db, state):
        """Check if daily stats should be reset"""
        from app.models.live_trading import LiveTradingState
        
        if state.last_daily_reset:
            last_reset_date = state.last_daily_reset.date()
            today = datetime.utcnow().date()
            
            if last_reset_date < today:
                # Reset daily stats
                state.daily_loss = 0.0
                state.daily_profit = 0.0
                state.daily_trades = 0
                state.daily_wins = 0
                state.last_daily_reset = datetime.utcnow()
                
                # Also reset circuit breaker if it was daily-loss triggered
                if state.is_circuit_broken and state.circuit_broken_reason and 'daily' in state.circuit_broken_reason.lower():
                    state.is_circuit_broken = False
                    state.circuit_broken_at = None
                    state.circuit_broken_reason = None
                    logger.info("Circuit breaker auto-reset on new day")
                
                db.commit()
                logger.info("Daily live trading stats reset")
    
    def check_can_trade(self) -> tuple[bool, Optional[str]]:
        """
        Check if trading is allowed.
        Returns (can_trade, reason_if_not)
        """
        state = self.get_state()
        config = self.config_manager.get_settings()
        
        # Check circuit breaker
        if state.is_broken:
            return False, f"Circuit breaker triggered: {state.broken_reason}"
        
        # Check daily loss limit
        if state.daily_loss >= config.max_daily_loss:
            self._trigger("Daily loss limit reached: ${:.2f} >= ${:.2f}".format(
                state.daily_loss, config.max_daily_loss
            ))
            return False, f"Daily loss limit reached (${state.daily_loss:.2f})"
        
        # Check total loss limit
        if state.total_loss >= config.max_total_loss:
            self._trigger("Total loss limit reached: ${:.2f} >= ${:.2f}".format(
                state.total_loss, config.max_total_loss
            ))
            return False, f"Total loss limit reached (${state.total_loss:.2f})"
        
        # Check if already executing (sequential mode)
        if config.execution_mode == 'sequential' and state.is_executing:
            return False, f"Trade already in progress: {state.current_trade_id}"
        
        return True, None
    
    def record_trade_result(self, profit_loss: float, is_win: bool, trade_id: str, trade_amount: float = 0.0):
        """Record the result of a completed trade"""
        db = self._get_db()
        try:
            from app.models.live_trading import LiveTradingState
            
            state = db.query(LiveTradingState).filter(LiveTradingState.id == 1).first()
            if not state:
                return
            
            # Update stats
            if profit_loss >= 0:
                state.daily_profit += profit_loss
                state.total_profit += profit_loss
            else:
                state.daily_loss += abs(profit_loss)
                state.total_loss += abs(profit_loss)
            
            state.daily_trades += 1
            state.total_trades += 1
            
            # Track total trade amount (persisted in DB)
            if hasattr(state, 'total_trade_amount'):
                state.total_trade_amount = (state.total_trade_amount or 0.0) + trade_amount
            
            if is_win:
                state.daily_wins += 1
                state.total_wins += 1
            
            state.last_trade_at = datetime.utcnow()
            state.is_executing = False
            state.current_trade_id = None
            
            db.commit()
            
            logger.info(f"Recorded trade result: {'+' if profit_loss >= 0 else ''}${profit_loss:.2f} ({'WIN' if is_win else 'LOSS'}), amount: ${trade_amount:.2f}")
            
            # Check limits after recording
            config = self.config_manager.get_settings()
            
            if state.daily_loss >= config.max_daily_loss:
                self._trigger(f"Daily loss limit reached: ${state.daily_loss:.2f}")
            elif state.total_loss >= config.max_total_loss:
                self._trigger(f"Total loss limit reached: ${state.total_loss:.2f}")
            
        finally:
            db.close()
    
    def mark_executing(self, trade_id: str) -> bool:
        """Mark that a trade is starting execution"""
        db = self._get_db()
        try:
            from app.models.live_trading import LiveTradingState
            
            state = db.query(LiveTradingState).filter(LiveTradingState.id == 1).first()
            if not state:
                return False
            
            config = self.config_manager.get_settings()
            
            # For sequential mode, check if already executing
            if config.execution_mode == 'sequential' and state.is_executing:
                return False
            
            state.is_executing = True
            state.current_trade_id = trade_id
            db.commit()
            
            return True
            
        finally:
            db.close()
    
    def mark_execution_complete(self, trade_id: str):
        """Mark that trade execution is complete (success or failure)"""
        db = self._get_db()
        try:
            from app.models.live_trading import LiveTradingState
            
            state = db.query(LiveTradingState).filter(LiveTradingState.id == 1).first()
            if state and state.current_trade_id == trade_id:
                state.is_executing = False
                state.current_trade_id = None
                db.commit()
            
        finally:
            db.close()
    
    def _trigger(self, reason: str):
        """Trigger the circuit breaker"""
        db = self._get_db()
        try:
            from app.models.live_trading import LiveTradingState, LiveTradingConfig
            
            state = db.query(LiveTradingState).filter(LiveTradingState.id == 1).first()
            config = db.query(LiveTradingConfig).filter(LiveTradingConfig.id == 1).first()
            
            if state and not state.is_circuit_broken:
                state.is_circuit_broken = True
                state.circuit_broken_at = datetime.utcnow()
                state.circuit_broken_reason = reason
                state.is_executing = False
                state.current_trade_id = None
            
            # Also disable live trading
            if config and config.is_enabled:
                config.is_enabled = False
                config.disabled_at = datetime.utcnow()
            
            db.commit()
            
            logger.error(f"🛑 CIRCUIT BREAKER TRIGGERED: {reason}")
            
        finally:
            db.close()
    
    def trigger_manual(self, reason: str = "Manual trigger"):
        """Manually trigger circuit breaker"""
        self._trigger(f"Manual: {reason}")
    
    def reset(self) -> CircuitBreakerState:
        """Reset the circuit breaker (does NOT reset loss counters)"""
        db = self._get_db()
        try:
            from app.models.live_trading import LiveTradingState
            
            state = db.query(LiveTradingState).filter(LiveTradingState.id == 1).first()
            if state:
                state.is_circuit_broken = False
                state.circuit_broken_at = None
                state.circuit_broken_reason = None
                state.is_executing = False
                state.current_trade_id = None
                db.commit()
            
            logger.info("Circuit breaker reset")
            
            return self.get_state(force_refresh=True)
            
        finally:
            db.close()
    
    def reset_daily_stats(self) -> CircuitBreakerState:
        """Manually reset daily statistics"""
        db = self._get_db()
        try:
            from app.models.live_trading import LiveTradingState
            
            state = db.query(LiveTradingState).filter(LiveTradingState.id == 1).first()
            if state:
                state.daily_loss = 0.0
                state.daily_profit = 0.0
                state.daily_trades = 0
                state.daily_wins = 0
                state.last_daily_reset = datetime.utcnow()
                db.commit()
            
            logger.info("Daily live trading stats manually reset")
            
            return self.get_state(force_refresh=True)
            
        finally:
            db.close()
    
    def reset_total_stats(self) -> CircuitBreakerState:
        """Manually reset ALL statistics (use with caution)"""
        db = self._get_db()
        try:
            from app.models.live_trading import LiveTradingState
            
            state = db.query(LiveTradingState).filter(LiveTradingState.id == 1).first()
            if state:
                state.daily_loss = 0.0
                state.daily_profit = 0.0
                state.daily_trades = 0
                state.daily_wins = 0
                state.total_loss = 0.0
                state.total_profit = 0.0
                state.total_trades = 0
                state.total_wins = 0
                if hasattr(state, 'total_trade_amount'):
                    state.total_trade_amount = 0.0
                state.last_daily_reset = datetime.utcnow()
                state.is_circuit_broken = False
                state.circuit_broken_at = None
                state.circuit_broken_reason = None
                db.commit()
            
            logger.warning("All live trading stats manually reset")
            
            return self.get_state(force_refresh=True)
            
        finally:
            db.close()
    
    def get_remaining_daily_budget(self) -> float:
        """Get remaining daily loss budget"""
        state = self.get_state()
        config = self.config_manager.get_settings()
        return max(0, config.max_daily_loss - state.daily_loss)
    
    def get_remaining_total_budget(self) -> float:
        """Get remaining total loss budget"""
        state = self.get_state()
        config = self.config_manager.get_settings()
        return max(0, config.max_total_loss - state.total_loss)
