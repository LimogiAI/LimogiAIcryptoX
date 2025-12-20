"""
SQLAlchemy ORM Models for LimogiAICryptoX v2.0

This module contains models for:
- Trading pairs and currencies
- Price data
- Arbitrage opportunities (live and historical)
- Order book health monitoring
- Engine metrics

All live trading models are in live_trading.py
"""
from datetime import datetime
from sqlalchemy import (
    Column, Integer, BigInteger, String, Boolean,
    Numeric, DateTime, Text, ForeignKey, ARRAY, JSON, Float
)
from sqlalchemy.dialects.postgresql import UUID, JSONB
from sqlalchemy.sql import func
from sqlalchemy.orm import relationship
from app.core.database import Base
import uuid


class Currency(Base):
    """Currency/Token model"""
    __tablename__ = "currencies"

    id = Column(Integer, primary_key=True)
    symbol = Column(String(10), unique=True, nullable=False)
    name = Column(String(100))
    currency_type = Column(String(20), nullable=False)
    is_active = Column(Boolean, default=True)
    created_at = Column(DateTime, server_default=func.current_timestamp())


class TradingPair(Base):
    """Trading pair model"""
    __tablename__ = "trading_pairs"

    id = Column(Integer, primary_key=True)
    pair_name = Column(String(20), unique=True, nullable=False)
    base_currency = Column(String(10), nullable=False)
    quote_currency = Column(String(10), nullable=False)
    kraken_symbol = Column(String(20), unique=True, nullable=False)
    is_active = Column(Boolean, default=True)
    min_volume = Column(Numeric(20, 10))
    price_decimals = Column(Integer)
    volume_decimals = Column(Integer)
    created_at = Column(DateTime, server_default=func.current_timestamp())
    updated_at = Column(DateTime, server_default=func.current_timestamp(),
                        onupdate=func.current_timestamp())

    price_ticks = relationship("PriceTick", back_populates="trading_pair")


class PriceTick(Base):
    """Real-time price tick model"""
    __tablename__ = "price_ticks"

    id = Column(BigInteger, primary_key=True)
    pair_id = Column(Integer, ForeignKey("trading_pairs.id"), nullable=False)
    bid_price = Column(Numeric(20, 10), nullable=False)
    ask_price = Column(Numeric(20, 10), nullable=False)
    bid_volume = Column(Numeric(20, 10))
    ask_volume = Column(Numeric(20, 10))
    last_price = Column(Numeric(20, 10))
    volume_24h = Column(Numeric(20, 10))
    timestamp = Column(DateTime, server_default=func.current_timestamp())

    trading_pair = relationship("TradingPair", back_populates="price_ticks")


class PriceMatrix(Base):
    """Current price matrix for quick lookups"""
    __tablename__ = "price_matrix"

    id = Column(Integer, primary_key=True)
    base_currency = Column(String(10), nullable=False)
    quote_currency = Column(String(10), nullable=False)
    bid_price = Column(Numeric(20, 10))
    ask_price = Column(Numeric(20, 10))
    mid_price = Column(Numeric(20, 10))
    spread_pct = Column(Numeric(10, 6))
    volume_24h = Column(Numeric(20, 10))
    updated_at = Column(DateTime, server_default=func.current_timestamp(),
                        onupdate=func.current_timestamp())


class ArbitrageOpportunity(Base):
    """Detected arbitrage opportunity"""
    __tablename__ = "arbitrage_opportunities"

    id = Column(BigInteger, primary_key=True)
    opportunity_id = Column(UUID(as_uuid=True), default=uuid.uuid4)
    path = Column(Text, nullable=False)
    path_pairs = Column(ARRAY(Text), nullable=False)
    legs = Column(Integer, nullable=False)
    start_currency = Column(String(10), nullable=False)
    start_amount = Column(Numeric(20, 10), nullable=False)
    end_amount = Column(Numeric(20, 10), nullable=False)
    gross_profit_pct = Column(Numeric(10, 6), nullable=False)
    total_fees_pct = Column(Numeric(10, 6), nullable=False)
    net_profit_pct = Column(Numeric(10, 6), nullable=False)
    net_profit_amount = Column(Numeric(20, 10), nullable=False)
    is_profitable = Column(Boolean, nullable=False)
    min_volume_available = Column(Numeric(20, 10))
    prices_snapshot = Column(JSONB)
    detected_at = Column(DateTime, server_default=func.current_timestamp())
    expired_at = Column(DateTime)


class ScannerStats(Base):
    """Scanner statistics"""
    __tablename__ = "scanner_stats"

    id = Column(Integer, primary_key=True)
    period_start = Column(DateTime, nullable=False)
    period_end = Column(DateTime, nullable=False)
    period_type = Column(String(10), nullable=False)
    opportunities_found = Column(Integer, default=0)
    profitable_opportunities = Column(Integer, default=0)
    best_profit_pct = Column(Numeric(10, 6))
    avg_profit_pct = Column(Numeric(10, 6))
    total_volume_scanned = Column(Numeric(20, 2))
    pairs_monitored = Column(Integer)
    created_at = Column(DateTime, server_default=func.current_timestamp())


class SystemConfig(Base):
    """System configuration"""
    __tablename__ = "system_config"

    key = Column(String(100), primary_key=True)
    value = Column(Text, nullable=False)
    description = Column(Text)
    updated_at = Column(DateTime, server_default=func.current_timestamp(),
                        onupdate=func.current_timestamp())


# ============================================
# ENGINE MONITORING MODELS
# ============================================

class EngineMetrics(Base):
    """Engine performance metrics"""
    __tablename__ = "engine_metrics"

    id = Column(Integer, primary_key=True, index=True)
    timestamp = Column(DateTime, default=datetime.utcnow, index=True)
    metric_name = Column(String(100), nullable=False, index=True)
    metric_value = Column(Float, nullable=False)
    extra_data = Column(JSONB, nullable=True)


class OrderBookHealthHistory(Base):
    """Order book health history - snapshots every 5 minutes"""
    __tablename__ = "orderbook_health_history"

    id = Column(Integer, primary_key=True, index=True)
    timestamp = Column(DateTime, default=datetime.utcnow, index=True)
    
    # Pair counts
    total_pairs = Column(Integer, nullable=False)
    valid_pairs = Column(Integer, nullable=False)
    valid_pct = Column(Float, nullable=False)
    
    # Skip reasons
    skipped_no_orderbook = Column(Integer, nullable=False, default=0)
    skipped_thin_depth = Column(Integer, nullable=False, default=0)
    skipped_stale = Column(Integer, nullable=False, default=0)
    skipped_bad_spread = Column(Integer, nullable=False, default=0)
    skipped_no_price = Column(Integer, nullable=False, default=0)
    skipped_total = Column(Integer, nullable=False, default=0)
    
    # Averages
    avg_freshness_ms = Column(Float, nullable=False, default=0.0)
    avg_spread_pct = Column(Float, nullable=False, default=0.0)
    avg_depth = Column(Float, nullable=False, default=0.0)
    
    # Opportunities
    rejected_opportunities = Column(Integer, nullable=False, default=0)


class OpportunityHistory(Base):
    """Historical record of all detected opportunities"""
    __tablename__ = "opportunity_history"

    id = Column(Integer, primary_key=True, index=True)
    timestamp = Column(DateTime, default=datetime.utcnow, index=True)
    
    # Path details
    path = Column(String(500), nullable=False, index=True)
    legs = Column(Integer, nullable=False)
    start_currency = Column(String(10), nullable=False, index=True)
    
    # Profit metrics
    expected_profit_pct = Column(Float, nullable=False)
    is_profitable = Column(Boolean, nullable=False)
    
    # Was this opportunity traded via live trading?
    was_traded = Column(Boolean, default=False)
    live_trade_id = Column(String(100), nullable=True)
    
    # If traded, what was the result?
    actual_profit_pct = Column(Float, nullable=True)
    slippage_pct = Column(Float, nullable=True)
    
    # Market snapshot
    prices_snapshot = Column(JSONB, nullable=True)
