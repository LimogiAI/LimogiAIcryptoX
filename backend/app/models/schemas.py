"""
Pydantic Schemas for API request/response models v2.0
"""
from pydantic import BaseModel, Field
from typing import Optional, List, Dict, Any
from datetime import datetime
from decimal import Decimal
import uuid


# ============================================
# Currency Schemas
# ============================================
class CurrencyBase(BaseModel):
    symbol: str
    name: Optional[str] = None
    currency_type: str


class CurrencyResponse(CurrencyBase):
    id: int
    is_active: bool

    class Config:
        from_attributes = True


# ============================================
# Trading Pair Schemas
# ============================================
class TradingPairBase(BaseModel):
    pair_name: str
    base_currency: str
    quote_currency: str
    kraken_symbol: str


class TradingPairResponse(TradingPairBase):
    id: int
    is_active: bool
    min_volume: Optional[Decimal] = None
    price_decimals: Optional[int] = None
    volume_decimals: Optional[int] = None

    class Config:
        from_attributes = True


# ============================================
# Arbitrage Opportunity Schemas
# ============================================
class ArbitrageOpportunityBase(BaseModel):
    path: str
    path_pairs: List[str]
    legs: int
    start_currency: str
    start_amount: Decimal
    end_amount: Decimal
    gross_profit_pct: Decimal
    total_fees_pct: Decimal
    net_profit_pct: Decimal
    net_profit_amount: Decimal
    is_profitable: bool


class ArbitrageOpportunityResponse(ArbitrageOpportunityBase):
    id: int
    opportunity_id: uuid.UUID
    detected_at: datetime

    class Config:
        from_attributes = True


# ============================================
# Scanner Status
# ============================================
class ScannerStatus(BaseModel):
    is_running: bool
    pairs_monitored: int
    currencies_tracked: int
    last_scan_at: Optional[datetime] = None
    opportunities_last_hour: int
    profitable_last_hour: int
    best_opportunity_today: Optional[Decimal] = None
    uptime_seconds: int


# ============================================
# Slot Schemas
# ============================================
class SlotBase(BaseModel):
    id: int
    balance: float
    status: str
    cooldown_until: Optional[str] = None
    trades_count: int
    wins_count: int
    total_profit: float


class SlotResponse(SlotBase):
    win_rate: float
    current_opportunity_id: Optional[str] = None

    class Config:
        from_attributes = True


class SlotsResponse(BaseModel):
    slots: List[SlotResponse]
    total_balance: float
    total_profit: float
    win_rate: float
    ready_slots: int


# ============================================
# Trade Schemas
# ============================================
class TradeBase(BaseModel):
    slot_id: int
    path: str
    trade_amount: float
    expected_profit_pct: float
    slippage_pct: float
    actual_profit_pct: float
    profit_amount: float
    status: str


class TradeResponse(TradeBase):
    id: int
    legs: int
    balance_before: float
    balance_after: float
    executed_at: datetime

    class Config:
        from_attributes = True


# ============================================
# Slippage Schemas
# ============================================
class SlippageLeg(BaseModel):
    pair: str
    side: str
    best_price: float
    actual_price: float
    slippage_pct: float
    can_fill: bool
    depth_used: int


class SlippageResult(BaseModel):
    path: str
    trade_amount: float
    total_slippage_pct: float
    can_execute: bool
    reason: Optional[str] = None
    legs: List[SlippageLeg]


# ============================================
# Price Schemas
# ============================================
class PriceResponse(BaseModel):
    pair: str
    bid: float
    ask: float
    volume_24h: float
    spread_pct: float


# ============================================
# Stats Schemas
# ============================================
class EngineStats(BaseModel):
    is_running: bool
    pairs_monitored: int
    currencies_tracked: int
    orderbooks_cached: int
    avg_staleness_ms: float
    opportunities_found: int
    opportunities_per_second: float
    trades_executed: int
    total_profit: float
    win_rate: float
    uptime_seconds: int
    scan_cycle_ms: float
    last_scan_at: Optional[str] = None


class TradeStats(BaseModel):
    period_hours: int
    total_trades: int
    wins: int
    losses: int
    win_rate: float
    total_profit: float
    avg_profit_per_trade: float
    best_trade: Optional[float] = None
    worst_trade: Optional[float] = None


# ============================================
# API Response Wrappers
# ============================================
class APIResponse(BaseModel):
    success: bool
    message: Optional[str] = None
    data: Optional[Any] = None


class PaginatedResponse(BaseModel):
    items: List[Any]
    total: int
    page: int
    page_size: int
    total_pages: int
