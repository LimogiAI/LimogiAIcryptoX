"""
SQLAlchemy ORM Models for Live Trading
"""
from datetime import datetime
from sqlalchemy import (
    Column, Integer, String, Boolean, Float, 
    DateTime, Text, TIMESTAMP
)
from sqlalchemy.dialects.postgresql import JSONB
from app.core.database import Base


class LiveTradingConfig(Base):
    """Live trading configuration - user-configurable settings"""
    __tablename__ = "live_trading_config"

    id = Column(Integer, primary_key=True, default=1)
    
    # Enable/disable
    is_enabled = Column(Boolean, nullable=False, default=False)
    
    # Trade parameters
    trade_amount = Column(Float, nullable=False, default=10.0)
    min_profit_threshold = Column(Float, nullable=False, default=0.003)  # 0.3%
    
    # Loss limits
    max_daily_loss = Column(Float, nullable=False, default=30.0)
    max_total_loss = Column(Float, nullable=False, default=30.0)
    
    # Execution mode
    execution_mode = Column(String(20), nullable=False, default='sequential')
    max_parallel_trades = Column(Integer, nullable=False, default=1)
    
    # Order execution
    max_retries_per_leg = Column(Integer, nullable=False, default=2)
    order_timeout_seconds = Column(Integer, nullable=False, default=30)
    
    # Base currency filter
    base_currency = Column(String(20), nullable=False, default='USD')
    custom_currencies = Column(JSONB, default=[])
    
    # Timestamps
    created_at = Column(DateTime, default=datetime.utcnow)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)
    enabled_at = Column(DateTime, nullable=True)
    disabled_at = Column(DateTime, nullable=True)


class LiveTradingState(Base):
    """Live trading state - system-managed"""
    __tablename__ = "live_trading_state"

    id = Column(Integer, primary_key=True, default=1)
    
    # Daily stats (completed trades only)
    daily_loss = Column(Float, nullable=False, default=0.0)
    daily_profit = Column(Float, nullable=False, default=0.0)
    daily_trades = Column(Integer, nullable=False, default=0)
    daily_wins = Column(Integer, nullable=False, default=0)
    
    # Total stats (completed trades only)
    total_loss = Column(Float, nullable=False, default=0.0)
    total_profit = Column(Float, nullable=False, default=0.0)
    total_trades = Column(Integer, nullable=False, default=0)
    total_wins = Column(Integer, nullable=False, default=0)
    total_trade_amount = Column(Float, nullable=False, default=0.0)
    
    # ====== PARTIAL TRADE TRACKING (Option C) ======
    # Separate tracking for unresolved partial trades
    partial_trades = Column(Integer, nullable=False, default=0)          # Count of unresolved partials
    partial_estimated_loss = Column(Float, nullable=False, default=0.0)  # Snapshot estimate (may be profit or loss)
    partial_estimated_profit = Column(Float, nullable=False, default=0.0)
    partial_trade_amount = Column(Float, nullable=False, default=0.0)    # Total $ stuck in partial trades
    
    # Circuit breaker
    is_circuit_broken = Column(Boolean, nullable=False, default=False)
    circuit_broken_at = Column(DateTime, nullable=True)
    circuit_broken_reason = Column(Text, nullable=True)
    
    # Timing
    last_trade_at = Column(DateTime, nullable=True)
    last_daily_reset = Column(DateTime, default=datetime.utcnow)
    
    # Execution state
    is_executing = Column(Boolean, nullable=False, default=False)
    current_trade_id = Column(String(100), nullable=True)
    
    # Timestamps
    created_at = Column(DateTime, default=datetime.utcnow)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)


class LiveTrade(Base):
    """Record of live trade executions"""
    __tablename__ = "live_trades"

    id = Column(Integer, primary_key=True, index=True)
    trade_id = Column(String(100), unique=True, nullable=False, index=True)
    
    # What was traded
    path = Column(String(500), nullable=False, index=True)
    legs = Column(Integer, nullable=False)
    
    # Money in/out
    amount_in = Column(Float, nullable=False)
    amount_out = Column(Float, nullable=True)
    profit_loss = Column(Float, nullable=True)
    profit_loss_pct = Column(Float, nullable=True)
    
    # Status: PENDING, EXECUTING, COMPLETED, PARTIAL, FAILED, RESOLVED
    status = Column(String(20), nullable=False, default='PENDING')
    current_leg = Column(Integer, default=0)
    error_message = Column(Text, nullable=True)
    
    # Held position (if partial failure)
    held_currency = Column(String(20), nullable=True)
    held_amount = Column(Float, nullable=True)
    held_value_usd = Column(Float, nullable=True)  # Snapshot USD value at time of failure
    
    # Resolution tracking (for PARTIAL trades)
    resolved_at = Column(DateTime, nullable=True)
    resolved_amount_usd = Column(Float, nullable=True)  # Actual USD received when sold
    resolution_trade_id = Column(String(100), nullable=True)  # ID of the sell trade
    
    # Kraken references
    order_ids = Column(JSONB, default=[])
    leg_fills = Column(JSONB, default=[])
    
    # Timing
    started_at = Column(DateTime, default=datetime.utcnow)
    completed_at = Column(DateTime, nullable=True)
    total_execution_ms = Column(Float, nullable=True)
    
    # Trigger info
    opportunity_profit_pct = Column(Float, nullable=True)
    
    # Created at
    created_at = Column(DateTime, default=datetime.utcnow, index=True)


class LivePosition(Base):
    """Current holdings synced from Kraken"""
    __tablename__ = "live_positions"

    id = Column(Integer, primary_key=True)
    currency = Column(String(20), unique=True, nullable=False)
    balance = Column(Float, nullable=False, default=0.0)
    usd_value = Column(Float, nullable=True)
    last_synced_at = Column(DateTime, default=datetime.utcnow)


# NOTE: LiveOpportunity model removed - was never used (dead code from Python scanner era)
# All scanning and execution now happens in Rust engine
# Trade results are saved to LiveTrade table by Rust executor


class LiveScannerStatus(Base):
    """Scanner status - updated every scan cycle"""
    __tablename__ = "live_scanner_status"

    id = Column(Integer, primary_key=True, default=1)
    
    # Status
    is_running = Column(Boolean, nullable=False, default=False)
    
    # Last scan info
    last_scan_at = Column(DateTime, nullable=True)
    pairs_scanned = Column(Integer, nullable=False, default=0)
    paths_found = Column(Integer, nullable=False, default=0)
    opportunities_found = Column(Integer, nullable=False, default=0)
    profitable_count = Column(Integer, nullable=False, default=0)  # Above threshold
    
    # Scan timing
    scan_duration_ms = Column(Float, nullable=True)
    
    # Error tracking
    last_error = Column(Text, nullable=True)
    last_error_at = Column(DateTime, nullable=True)
    
    # Timestamps
    created_at = Column(DateTime, default=datetime.utcnow)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)
