"""
API Routes for KrakenCryptoX v2.0 - Single Balance Pool
"""
from fastapi import APIRouter, Depends, HTTPException, Query
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select, desc
from typing import List, Optional
from datetime import datetime, timedelta

from app.core.database import get_db
from app.models.models import PaperTrade

# Import live trading routes
from app.api.live_routes import router as live_router

router = APIRouter()

# Include live trading routes
router.include_router(live_router)


def get_engine():
    """Get the global engine instance"""
    from app.main import get_engine as _get_engine
    return _get_engine()


def get_cached_opportunities():
    """Get cached opportunities from main"""
    from app.main import get_cached_opportunities as _get_cached
    return _get_cached()


def get_best_profit():
    """Get best profit today from main"""
    from app.main import get_best_profit_today as _get_best
    return _get_best()


# Runtime settings (synced with Rust engine)
_runtime_settings = {
    "is_active": True,
    "trade_amount": 10.0,
    "min_profit_threshold": 0.0005,
    "cooldown_seconds": 0,
    "max_trades_per_cycle": 5,
    "latency_penalty_pct": 0.001,  # 0.10% per leg default
    "fee_rate": 0.0026,            # 0.26% taker fee default
    "base_currency": "ALL",        # ALL, USD, EUR, BTC, ETH, USDT, CUSTOM
    "custom_currencies": [],       # For CUSTOM: ["USD", "EUR", "USDT"]
}


def get_runtime_settings():
    """Get current runtime settings"""
    return _runtime_settings.copy()


def update_runtime_settings(updates: dict):
    """Update runtime settings and sync with Rust engine"""
    global _runtime_settings
    
    engine = get_engine()
    
    for key, value in updates.items():
        if key in _runtime_settings:
            _runtime_settings[key] = value
    
    # Sync with Rust engine
    if engine:
        try:
            engine.update_config(
                trade_amount=_runtime_settings.get("trade_amount"),
                min_profit_threshold=_runtime_settings.get("min_profit_threshold"),
                cooldown_ms=int(_runtime_settings.get("cooldown_seconds", 5) * 1000),
                max_trades_per_cycle=_runtime_settings.get("max_trades_per_cycle"),
                latency_penalty_pct=_runtime_settings.get("latency_penalty_pct"),
                fee_rate=_runtime_settings.get("fee_rate"),
            )
        except Exception as e:
            print(f"Warning: Could not sync settings with Rust engine: {e}")
    
    return _runtime_settings.copy()


# ============================================
# Engine Status & Control
# ============================================

@router.get("/status")
async def get_scanner_status():
    """Get current engine status"""
    from app.core.config import settings
    
    engine = get_engine()
    
    if engine:
        stats = engine.get_stats()
        state = engine.get_trading_state()
        return {
            "is_running": stats.is_running,
            "engine": "rust_v2",
            "pairs_monitored": stats.pairs_monitored,
            "currencies_tracked": stats.currencies_tracked,
            "orderbooks_cached": stats.orderbooks_cached,
            "avg_staleness_ms": stats.avg_orderbook_staleness_ms,
            "opportunities_found": stats.opportunities_found,
            "opportunities_per_second": stats.opportunities_per_second,
            "trades_executed": stats.trades_executed,
            "total_profit": stats.total_profit,
            "win_rate": stats.win_rate,
            "uptime_seconds": stats.uptime_seconds,
            "scan_cycle_ms": stats.scan_cycle_ms,
            "scan_interval_ms": settings.scan_interval_ms,
            "max_pairs": settings.max_pairs,
            "orderbook_depth": settings.orderbook_depth,
            "last_scan_at": stats.last_scan_at,
            "balance": state.balance,
            "is_in_cooldown": state.is_in_cooldown,
        }
    else:
        return {
            "is_running": False,
            "engine": "none",
            "scan_interval_ms": settings.scan_interval_ms,
            "max_pairs": settings.max_pairs,
            "orderbook_depth": settings.orderbook_depth,
            "error": "Engine not initialized",
        }


@router.get("/engine-settings")
async def get_engine_settings():
    """Get current engine settings from Rust engine"""
    engine = get_engine()
    
    if engine:
        settings = engine.get_engine_settings()
        return {
            "scan_interval_ms": settings.scan_interval_ms,
            "max_pairs": settings.max_pairs,
            "orderbook_depth": settings.orderbook_depth,
            "scanner_enabled": settings.scanner_enabled,
            # Available options for dropdowns
            "options": {
                "scan_interval_ms": [100, 250, 500, 1000, 2000, 5000, 7000, 10000],
                "max_pairs": [100, 200, 300, 400],
                "orderbook_depth": [10, 25, 100, 500, 1000],
            },
        }
    else:
        # Fallback to config file
        from app.core.config import settings as config_settings
        return {
            "scan_interval_ms": config_settings.scan_interval_ms,
            "max_pairs": config_settings.max_pairs,
            "orderbook_depth": config_settings.orderbook_depth,
            "scanner_enabled": True,
            "options": {
                "scan_interval_ms": [100, 250, 500, 1000, 2000, 5000, 7000, 10000],
                "max_pairs": [100, 200, 300, 400],
                "orderbook_depth": [10, 25, 100, 500, 1000],
            },
            "error": "Engine not initialized",
        }


@router.put("/engine-settings")
async def update_engine_settings(
    scan_interval_ms: Optional[int] = None,
    max_pairs: Optional[int] = None,
    orderbook_depth: Optional[int] = None,
    scanner_enabled: Optional[bool] = None,
):
    """
    Update engine settings at runtime.
    
    - scan_interval_ms: Changes immediately (no reconnection)
    - max_pairs / orderbook_depth: Requires WebSocket reconnection (~5-10 sec)
    - scanner_enabled: Enables/disables scanner (affects auto-trading)
    
    Valid values:
    - scan_interval_ms: 100, 250, 500, 1000, 2000, 5000, 7000, 10000
    - max_pairs: 100, 200, 300, 400
    - orderbook_depth: 10, 25, 100, 500, 1000
    """
    engine = get_engine()
    
    if not engine:
        raise HTTPException(status_code=503, detail="Engine not available")
    
    valid_scan_intervals = [100, 250, 500, 1000, 2000, 5000, 7000, 10000]
    valid_max_pairs = [100, 200, 300, 400]
    valid_depths = [10, 25, 100, 500, 1000]
    
    errors = []
    
    if scan_interval_ms is not None and scan_interval_ms not in valid_scan_intervals:
        errors.append(f"Invalid scan_interval_ms: {scan_interval_ms}. Valid: {valid_scan_intervals}")
    
    if max_pairs is not None and max_pairs not in valid_max_pairs:
        errors.append(f"Invalid max_pairs: {max_pairs}. Valid: {valid_max_pairs}")
    
    if orderbook_depth is not None and orderbook_depth not in valid_depths:
        errors.append(f"Invalid orderbook_depth: {orderbook_depth}. Valid: {valid_depths}")
    
    if errors:
        raise HTTPException(status_code=400, detail="; ".join(errors))
    
    # Check if any setting provided
    if all(v is None for v in [scan_interval_ms, max_pairs, orderbook_depth, scanner_enabled]):
        raise HTTPException(status_code=400, detail="No settings provided to update")
    
    try:
        # Update settings in Rust engine (returns True if reconnection needed)
        needs_reconnect = engine.update_engine_settings(
            scan_interval_ms=scan_interval_ms,
            max_pairs=max_pairs,
            orderbook_depth=orderbook_depth,
            scanner_enabled=scanner_enabled,
        )
        
        return {
            "success": True,
            "updated": {
                k: v for k, v in {
                    "scan_interval_ms": scan_interval_ms,
                    "max_pairs": max_pairs,
                    "orderbook_depth": orderbook_depth,
                    "scanner_enabled": scanner_enabled,
                }.items() if v is not None
            },
            "needs_reconnect": needs_reconnect,
            "message": "Settings updated" + (" (reconnection required)" if needs_reconnect else ""),
        }
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Failed to update settings: {str(e)}")


@router.post("/engine/restart")
async def restart_engine():
    """
    Hot-reload the engine with new settings.
    This reconnects WebSocket with new pairs/depth settings.
    Takes about 5-10 seconds.
    """
    engine = get_engine()
    
    if not engine:
        raise HTTPException(status_code=503, detail="Engine not available")
    
    try:
        # Hot-reload: reconnect WebSocket with current settings
        engine.reconnect_websocket()
        
        # Get updated stats
        settings = engine.get_engine_settings()
        
        return {
            "success": True,
            "message": "Engine restarted successfully",
            "settings": {
                "scan_interval_ms": settings.scan_interval_ms,
                "max_pairs": settings.max_pairs,
                "orderbook_depth": settings.orderbook_depth,
                "scanner_enabled": settings.scanner_enabled,
            },
        }
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Failed to restart engine: {str(e)}")


@router.get("/orderbook-health")
async def get_orderbook_health():
    """Get order book health statistics"""
    engine = get_engine()
    
    if not engine:
        raise HTTPException(status_code=503, detail="Engine not available")
    
    health = engine.get_orderbook_health()
    
    return {
        "total_pairs": health.total_pairs,
        "valid_pairs": health.valid_pairs,
        "valid_pct": round(health.valid_pairs / max(health.total_pairs, 1) * 100, 1),
        "skipped": {
            "no_orderbook": health.skipped_no_orderbook,
            "thin_depth": health.skipped_thin_depth,
            "stale": health.skipped_stale,
            "bad_spread": health.skipped_bad_spread,
            "no_price": health.skipped_no_price,
            "total": health.skipped_no_orderbook + health.skipped_thin_depth + health.skipped_stale + health.skipped_bad_spread + health.skipped_no_price,
        },
        "averages": {
            "freshness_ms": round(health.avg_freshness_ms, 1),
            "spread_pct": round(health.avg_spread_pct, 3),
            "depth": round(health.avg_depth, 1),
        },
        "rejected_opportunities": health.rejected_opportunities,
        "thresholds": {
            "min_depth": 3,
            "max_staleness_ms": 5000,
            "max_spread_pct": 10.0,
            "max_profit_pct": 5.0,
        },
        "last_update": health.last_update,
    }


@router.get("/orderbook-health/history")
async def get_orderbook_health_history(
    hours: int = Query(default=24, le=720),  # Max 30 days
    db: AsyncSession = Depends(get_db),
):
    """Get order book health history for trend charts"""
    from app.models.models import OrderBookHealthHistory
    from datetime import datetime, timedelta
    
    cutoff = datetime.utcnow() - timedelta(hours=hours)
    
    query = select(OrderBookHealthHistory).where(
        OrderBookHealthHistory.timestamp >= cutoff
    ).order_by(OrderBookHealthHistory.timestamp.asc())
    
    result = await db.execute(query)
    records = result.scalars().all()
    
    return {
        "count": len(records),
        "hours": hours,
        "history": [
            {
                "timestamp": r.timestamp.isoformat(),
                "total_pairs": r.total_pairs,
                "valid_pairs": r.valid_pairs,
                "valid_pct": r.valid_pct,
                "skipped": {
                    "no_orderbook": r.skipped_no_orderbook,
                    "thin_depth": r.skipped_thin_depth,
                    "stale": r.skipped_stale,
                    "bad_spread": r.skipped_bad_spread,
                    "no_price": r.skipped_no_price,
                    "total": r.skipped_total,
                },
                "averages": {
                    "freshness_ms": r.avg_freshness_ms,
                    "spread_pct": r.avg_spread_pct,
                    "depth": r.avg_depth,
                },
                "rejected_opportunities": r.rejected_opportunities,
            }
            for r in records
        ],
    }


@router.post("/scan")
async def trigger_scan(
    base_currencies: Optional[List[str]] = Query(default=None),
):
    """Manually trigger a scan"""
    engine = get_engine()
    
    if not engine:
        raise HTTPException(status_code=503, detail="Engine not available")
    
    if base_currencies is None:
        base_currencies = ["USD", "USDT", "EUR", "BTC", "ETH"]
    
    try:
        opportunities = engine.scan(base_currencies)
        
        profitable = [o for o in opportunities if o.is_profitable]
        best_profit = max((o.net_profit_pct for o in opportunities), default=0.0)
        
        return {
            "success": True,
            "total_opportunities": len(opportunities),
            "profitable": len(profitable),
            "best_profit_pct": best_profit,
            "opportunities": [
                {
                    "path": o.path,
                    "legs": o.legs,
                    "net_profit_pct": o.net_profit_pct,
                    "is_profitable": o.is_profitable,
                }
                for o in opportunities[:20]
            ],
        }
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))


# ============================================
# Opportunity History
# ============================================

@router.get("/opportunities/history")
async def get_opportunity_history(
    limit: int = Query(default=100, le=500),
    hours: int = Query(default=24, le=720),
    start_currency: Optional[str] = Query(default=None),
    profitable_only: bool = Query(default=False),
    db: AsyncSession = Depends(get_db),
):
    """Get historical opportunities (not just traded ones)"""
    from app.models.models import OpportunityHistory
    
    cutoff = datetime.utcnow() - timedelta(hours=hours)
    
    query = select(OpportunityHistory).where(
        OpportunityHistory.timestamp >= cutoff
    )
    
    if start_currency:
        query = query.where(OpportunityHistory.start_currency == start_currency)
    
    if profitable_only:
        query = query.where(OpportunityHistory.is_profitable == True)
    
    query = query.order_by(desc(OpportunityHistory.timestamp)).limit(limit)
    
    result = await db.execute(query)
    records = result.scalars().all()
    
    return {
        "count": len(records),
        "hours": hours,
        "opportunities": [
            {
                "id": r.id,
                "timestamp": r.timestamp.isoformat(),
                "path": r.path,
                "legs": r.legs,
                "start_currency": r.start_currency,
                "expected_profit_pct": r.expected_profit_pct,
                "is_profitable": r.is_profitable,
                "was_traded": r.was_traded,
                "actual_profit_pct": r.actual_profit_pct,
                "slippage_pct": r.slippage_pct,
            }
            for r in records
        ],
    }


@router.get("/opportunities/history/stats")
async def get_opportunity_history_stats(
    hours: int = Query(default=24, le=720),
    db: AsyncSession = Depends(get_db),
):
    """Get statistics about historical opportunities"""
    from app.models.models import OpportunityHistory
    from sqlalchemy import func
    
    cutoff = datetime.utcnow() - timedelta(hours=hours)
    
    # Total count
    total_query = select(func.count(OpportunityHistory.id)).where(
        OpportunityHistory.timestamp >= cutoff
    )
    total_result = await db.execute(total_query)
    total_count = total_result.scalar() or 0
    
    # Profitable count
    profitable_query = select(func.count(OpportunityHistory.id)).where(
        OpportunityHistory.timestamp >= cutoff,
        OpportunityHistory.is_profitable == True
    )
    profitable_result = await db.execute(profitable_query)
    profitable_count = profitable_result.scalar() or 0
    
    # Traded count
    traded_query = select(func.count(OpportunityHistory.id)).where(
        OpportunityHistory.timestamp >= cutoff,
        OpportunityHistory.was_traded == True
    )
    traded_result = await db.execute(traded_query)
    traded_count = traded_result.scalar() or 0
    
    # Average expected profit
    avg_profit_query = select(func.avg(OpportunityHistory.expected_profit_pct)).where(
        OpportunityHistory.timestamp >= cutoff,
        OpportunityHistory.is_profitable == True
    )
    avg_profit_result = await db.execute(avg_profit_query)
    avg_profit = avg_profit_result.scalar() or 0
    
    # Top paths
    top_paths_query = select(
        OpportunityHistory.path,
        func.count(OpportunityHistory.id).label('count')
    ).where(
        OpportunityHistory.timestamp >= cutoff
    ).group_by(
        OpportunityHistory.path
    ).order_by(
        desc('count')
    ).limit(10)
    
    top_paths_result = await db.execute(top_paths_query)
    top_paths = [{"path": row[0], "count": row[1]} for row in top_paths_result.fetchall()]
    
    return {
        "hours": hours,
        "total_opportunities": total_count,
        "profitable_opportunities": profitable_count,
        "traded_opportunities": traded_count,
        "trade_rate_pct": (traded_count / total_count * 100) if total_count > 0 else 0,
        "avg_expected_profit_pct": avg_profit,
        "top_paths": top_paths,
    }


# ============================================
# Trading State (replaces slots)
# ============================================

@router.get("/trading-state")
async def get_trading_state():
    """Get current trading state (single balance pool)"""
    engine = get_engine()
    
    if not engine:
        raise HTTPException(status_code=503, detail="Engine not available")
    
    state = engine.get_trading_state()
    
    return {
        "balance": state.balance,
        "initial_balance": state.initial_balance,
        "peak_balance": state.peak_balance,
        "total_trades": state.total_trades,
        "total_wins": state.total_wins,
        "total_profit": state.total_profit,
        "win_rate": state.win_rate,
        "is_in_cooldown": state.is_in_cooldown,
        "cooldown_until": state.cooldown_until,
        "can_trade": engine.can_trade(),
        # Kill switch status
        "is_killed": state.is_killed,
        "kill_reason": state.kill_reason,
        "consecutive_losses": state.consecutive_losses,
        "daily_profit": state.daily_profit,
        "loss_from_peak_pct": state.loss_from_peak_pct,
    }


@router.post("/reset")
async def reset_balance(initial_balance: float = 100.0):
    """Reset balance to initial amount"""
    engine = get_engine()
    
    if not engine:
        raise HTTPException(status_code=503, detail="Engine not available")
    
    engine.reset(initial_balance)
    
    return {
        "success": True,
        "message": f"Reset balance to ${initial_balance}",
        "balance": engine.get_balance(),
    }


# ============================================
# Locked Paths
# ============================================

@router.get("/locked-paths")
async def get_locked_paths():
    """Get currently locked paths (in cooldown)"""
    engine = get_engine()
    
    if not engine:
        raise HTTPException(status_code=503, detail="Engine not available")
    
    paths = engine.get_locked_paths()
    
    return {
        "count": len(paths),
        "paths": paths,
    }


# ============================================
# Prices
# ============================================

@router.get("/prices")
async def get_prices(limit: int = Query(default=50, le=500)):
    """Get live prices from Rust engine"""
    engine = get_engine()
    
    if not engine:
        raise HTTPException(status_code=503, detail="Engine not available")
    
    prices = engine.get_all_prices()[:limit]
    
    return {
        "count": len(prices),
        "prices": [
            {
                "pair": p[0],
                "bid": p[1],
                "ask": p[2],
                "volume_24h": p[3],
                "spread_pct": ((p[2] - p[1]) / p[1] * 100) if p[1] > 0 else 0,
            }
            for p in prices
        ],
    }


@router.get("/prices/live")
async def get_live_prices_endpoint(limit: int = Query(default=50, le=500)):
    """Get live prices for frontend"""
    return await get_prices(limit)


@router.get("/prices/matrix")
async def get_price_matrix_endpoint(currencies: Optional[str] = None):
    """Get price matrix for frontend"""
    engine = get_engine()
    
    if not engine:
        return {"matrix": {}, "currencies": []}
    
    all_prices = engine.get_all_prices()
    currency_list = sorted(engine.get_currencies())
    
    if currencies:
        filter_list = [c.strip().upper() for c in currencies.split(",")]
        currency_list = [c for c in currency_list if c in filter_list]
    
    matrix = {}
    for curr in currency_list[:20]:
        matrix[curr] = {}
        for pair, bid, ask, vol in all_prices:
            parts = pair.split("/")
            if len(parts) == 2:
                base, quote = parts
                if base == curr:
                    matrix[curr][quote] = {"bid": bid, "ask": ask}
                elif quote == curr:
                    matrix[curr][base] = {"bid": 1/ask if ask > 0 else 0, "ask": 1/bid if bid > 0 else 0}
    
    return {"matrix": matrix, "currencies": currency_list[:20]}


# ============================================
# Currencies & Pairs
# ============================================

@router.get("/currencies")
async def get_currencies():
    """Get all currencies"""
    engine = get_engine()
    
    if not engine:
        raise HTTPException(status_code=503, detail="Engine not available")
    
    currencies = engine.get_currencies()
    
    return {
        "count": len(currencies),
        "currencies": sorted(currencies),
    }


@router.get("/pairs")
async def get_pairs():
    """Get all trading pairs"""
    engine = get_engine()
    
    if not engine:
        raise HTTPException(status_code=503, detail="Engine not available")
    
    pairs = engine.get_pairs()
    
    return {
        "count": len(pairs),
        "pairs": sorted(pairs),
    }


# ============================================
# Slippage Calculator
# ============================================

@router.post("/slippage")
async def calculate_slippage(
    path: str,
    trade_amount: float = Query(default=10.0),
):
    """Calculate slippage for a path"""
    engine = get_engine()
    
    if not engine:
        raise HTTPException(status_code=503, detail="Engine not available")
    
    result = engine.calculate_slippage(path, trade_amount)
    
    return {
        "path": path,
        "trade_amount": trade_amount,
        "total_slippage_pct": result.total_slippage_pct,
        "can_execute": result.can_execute,
        "reason": result.reason,
        "legs": [
            {
                "pair": leg.pair,
                "side": leg.side,
                "best_price": leg.best_price,
                "actual_price": leg.actual_price,
                "slippage_pct": leg.slippage_pct,
                "can_fill": leg.can_fill,
                "depth_used": leg.depth_used,
            }
            for leg in result.legs
        ],
    }


# ============================================
# Opportunities
# ============================================

@router.get("/opportunities")
async def get_opportunities(
    profitable_only: bool = True,
    min_profit_pct: Optional[float] = None,
    limit: int = Query(default=50, le=200),
    sort_by: str = Query(default="profit"),
    base_currency: Optional[str] = None,
):
    """Get cached arbitrage opportunities"""
    
    opportunities = get_cached_opportunities()
    
    if not opportunities:
        return {"count": 0, "opportunities": []}
    
    # Filter by base currency if specified
    if base_currency and base_currency != "ALL":
        opportunities = [o for o in opportunities if o.path.startswith(base_currency)]
    
    # Filter profitable only
    if profitable_only:
        opportunities = [o for o in opportunities if o.is_profitable]
    
    # Filter by min profit
    if min_profit_pct is not None:
        opportunities = [o for o in opportunities if o.net_profit_pct >= min_profit_pct]
    
    # Sort
    if sort_by == "profit":
        opportunities = sorted(opportunities, key=lambda o: o.net_profit_pct, reverse=True)
    
    # Limit
    opportunities = opportunities[:limit]
    
    # Fee rate per leg (0.26%)
    fee_rate = 0.26
    
    return {
        "count": len(opportunities),
        "opportunities": [
            {
                "id": o.id,
                "path": o.path,
                "legs": o.legs,
                "gross_profit_pct": o.net_profit_pct + (o.legs * fee_rate),
                "fees_pct": o.legs * fee_rate,
                "net_profit_pct": o.net_profit_pct,
                "profit_amount": (o.net_profit_pct / 100) * 10000,
                "is_profitable": o.is_profitable,
                "detected_at": datetime.utcnow().isoformat(),
            }
            for o in opportunities
        ],
    }


# ============================================
# Trades (from database)
# ============================================

@router.get("/trades")
async def get_trades(
    limit: int = Query(default=50, le=500),
    db: AsyncSession = Depends(get_db),
):
    """Get recent paper trades"""
    query = select(PaperTrade).order_by(desc(PaperTrade.executed_at)).limit(limit)
    
    result = await db.execute(query)
    trades = result.scalars().all()
    
    return {
        "count": len(trades),
        "trades": [
            {
                "id": t.id,
                "path": t.path,
                "legs": t.legs,
                "trade_amount": t.trade_amount,
                "expected_net_profit_pct": t.expected_net_profit_pct,
                "slippage_pct": t.slippage_pct,
                "actual_net_profit_pct": t.actual_net_profit_pct,
                "actual_profit_amount": t.actual_profit_amount,
                "status": t.status,
                "executed_at": t.executed_at.isoformat(),
            }
            for t in trades
        ],
    }


# ============================================
# Trade Controls (Settings)
# ============================================

@router.get("/trade-controls")
async def get_trade_controls():
    """Get trade control settings"""
    engine = get_engine()
    runtime = get_runtime_settings()
    
    return {
        "is_active": runtime["is_active"],
        "trade_amount": runtime["trade_amount"],
        "min_profit_threshold": runtime["min_profit_threshold"],
        "cooldown_seconds": runtime["cooldown_seconds"],
        "max_trades_per_cycle": runtime["max_trades_per_cycle"],
        "latency_penalty_pct": runtime["latency_penalty_pct"],
        "fee_rate": runtime["fee_rate"],
        "base_currency": runtime["base_currency"],
        "custom_currencies": runtime["custom_currencies"],
    }


@router.put("/trade-controls")
async def update_trade_controls(updates: dict):
    """Update trade control settings"""
    key_mapping = {
        "trade_amount": "trade_amount",
        "min_profit_threshold": "min_profit_threshold",
        "cooldown_seconds": "cooldown_seconds",
        "max_trades_per_cycle": "max_trades_per_cycle",
        "is_active": "is_active",
        "latency_penalty_pct": "latency_penalty_pct",
        "fee_rate": "fee_rate",
        "base_currency": "base_currency",
        "custom_currencies": "custom_currencies",
    }
    
    mapped_updates = {}
    for key, value in updates.items():
        if key in key_mapping:
            mapped_updates[key_mapping[key]] = value
    
    if mapped_updates:
        updated = update_runtime_settings(mapped_updates)
        return {
            "success": True,
            "message": "Settings updated",
            "settings": updated,
        }
    
    return {
        "success": False,
        "message": "No valid settings to update",
    }


@router.post("/trade-controls/toggle")
async def toggle_trading(is_active: bool):
    """Toggle auto-trading on/off"""
    update_runtime_settings({"is_active": is_active})
    return {
        "success": True,
        "is_active": is_active,
    }


# ============================================
# Kill Switch Controls
# ============================================

@router.get("/kill-switch")
async def get_kill_switch_status():
    """Get kill switch status and settings"""
    engine = get_engine()
    
    if not engine:
        raise HTTPException(status_code=503, detail="Engine not available")
    
    state = engine.get_trading_state()
    settings = engine.get_kill_switch_settings()
    
    return {
        "is_killed": state.is_killed,
        "kill_reason": state.kill_reason,
        "consecutive_losses": state.consecutive_losses,
        "daily_profit": state.daily_profit,
        "loss_from_peak_pct": state.loss_from_peak_pct,
        "current_balance": state.balance,
        "peak_balance": state.peak_balance,
        "settings": {
            "enabled": settings[0],
            "max_loss_pct": settings[1] * 100,  # Convert to percentage
            "max_consecutive_losses": settings[2],
            "max_daily_loss_pct": settings[3] * 100,  # Convert to percentage
        }
    }


@router.put("/kill-switch/settings")
async def update_kill_switch_settings(updates: dict):
    """Update kill switch settings"""
    engine = get_engine()
    
    if not engine:
        raise HTTPException(status_code=503, detail="Engine not available")
    
    # Convert percentages to decimals
    enabled = updates.get("enabled")
    max_loss_pct = updates.get("max_loss_pct")
    max_consecutive = updates.get("max_consecutive_losses")
    max_daily = updates.get("max_daily_loss_pct")
    
    if max_loss_pct is not None:
        max_loss_pct = max_loss_pct / 100  # Convert from percentage
    if max_daily is not None:
        max_daily = max_daily / 100  # Convert from percentage
    
    engine.update_kill_switch(
        enabled=enabled,
        max_loss_pct=max_loss_pct,
        max_consecutive_losses=max_consecutive,
        max_daily_loss_pct=max_daily,
    )
    
    return {
        "success": True,
        "message": "Kill switch settings updated",
    }


@router.post("/kill-switch/trigger")
async def trigger_kill_switch(reason: str = "Manual trigger"):
    """Manually trigger the kill switch"""
    engine = get_engine()
    
    if not engine:
        raise HTTPException(status_code=503, detail="Engine not available")
    
    engine.trigger_kill(reason)
    
    return {
        "success": True,
        "message": f"Kill switch triggered: {reason}",
    }


@router.post("/kill-switch/reset")
async def reset_kill_switch():
    """Reset the kill switch to allow trading again"""
    engine = get_engine()
    
    if not engine:
        raise HTTPException(status_code=503, detail="Engine not available")
    
    engine.reset_kill_switch()
    
    return {
        "success": True,
        "message": "Kill switch reset - trading enabled",
    }


# ============================================
# Legacy endpoints (backwards compatibility)
# ============================================

@router.get("/paper-trading/settings")
async def get_paper_trading_settings():
    """Get paper trading settings (legacy)"""
    return await get_trade_controls()


# ============================================
# Shadow Mode / Kraken Integration
# ============================================

@router.get("/shadow/status")
async def get_shadow_status():
    """Get shadow mode status and Kraken connection info"""
    from app.core.kraken_client import get_kraken_client
    from app.core.shadow_executor import get_shadow_executor
    
    client = get_kraken_client()
    executor = get_shadow_executor()
    
    if not client:
        return {
            "connected": False,
            "mode": "disconnected",
            "message": "Kraken client not initialized. Check API credentials.",
        }
    
    # Try to get balance to verify connection
    try:
        balance = await client.get_balance()
        trade_balance = await client.get_trade_balance()
        connected = True
        # Use equity balance (eb) which matches Kraken website total value
        balance_usd = float(trade_balance.get("eb", 0))
        # Also get raw balances for display
        raw_balances = {k: v for k, v in balance.items() if v > 0}
    except Exception as e:
        connected = False
        balance_usd = 0
        raw_balances = {}
        
    shadow_stats = client.get_shadow_stats()
    executor_stats = executor.get_stats() if executor else {}
    
    return {
        "connected": connected,
        "mode": shadow_stats.get("mode", "shadow"),
        "live_trading_enabled": shadow_stats.get("live_trading_enabled", False),
        "max_loss_usd": shadow_stats.get("max_loss_usd", 30),
        "kraken_balance_usd": balance_usd,
        "kraken_balances": raw_balances,
        "shadow_stats": {
            "total_trades": shadow_stats.get("total_shadow_trades", 0),
            "total_profit": shadow_stats.get("total_shadow_profit", 0),
        },
        "executor_stats": executor_stats,
    }


@router.get("/shadow/balance")
async def get_kraken_balance():
    """Get real Kraken account balance"""
    from app.core.kraken_client import get_kraken_client
    
    client = get_kraken_client()
    if not client:
        raise HTTPException(status_code=503, detail="Kraken client not initialized")
        
    try:
        balance = await client.get_balance()
        trade_balance = await client.get_trade_balance()
        
        return {
            "balances": balance,
            "trade_balance": trade_balance,
            "total_usd": balance.get("ZUSD", 0) + balance.get("USD", 0),
        }
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))


@router.get("/shadow/trades")
async def get_shadow_trades(limit: int = Query(default=50, le=200)):
    """Get recent shadow trades from memory (for backwards compatibility)"""
    from app.core.kraken_client import get_kraken_client
    from app.core.shadow_executor import get_shadow_executor
    
    client = get_kraken_client()
    executor = get_shadow_executor()
    
    result = {
        "client_trades": [],
        "executor_trades": [],
    }
    
    if client:
        result["client_trades"] = client.get_recent_shadow_trades(limit)
        
    if executor:
        result["executor_trades"] = executor.get_recent_executions(limit)
        
    return result


@router.get("/shadow/trades/history")
async def get_shadow_trades_history(
    limit: int = Query(default=50, le=500),
    offset: int = Query(default=0, ge=0),
    hours: int = Query(default=24, ge=1, le=720),
    result_filter: Optional[str] = Query(default=None, description="Filter by result: 'win', 'loss', or None for all"),
    path_filter: Optional[str] = Query(default=None, description="Filter by path (partial match)"),
):
    """Get shadow trades from database with pagination and filtering"""
    from sqlalchemy import select, func, desc, and_
    from app.models.models import ShadowTrade
    from app.core.database import SessionLocal
    
    db = SessionLocal()
    try:
        # Base query
        since = datetime.utcnow() - timedelta(hours=hours)
        conditions = [ShadowTrade.timestamp >= since]
        
        # Apply filters
        if result_filter == "win":
            conditions.append(ShadowTrade.would_have_profited == True)
        elif result_filter == "loss":
            conditions.append(ShadowTrade.would_have_profited == False)
            
        if path_filter:
            # Filter by paths starting with the specified currency
            conditions.append(ShadowTrade.path.ilike(f"{path_filter}%"))
        
        # Get total count
        count_query = select(func.count(ShadowTrade.id)).where(and_(*conditions))
        total_count = db.execute(count_query).scalar() or 0
        
        # Get trades
        query = (
            select(ShadowTrade)
            .where(and_(*conditions))
            .order_by(desc(ShadowTrade.timestamp))
            .offset(offset)
            .limit(limit)
        )
        
        trades = db.execute(query).scalars().all()
        
        return {
            "total": total_count,
            "limit": limit,
            "offset": offset,
            "has_more": (offset + limit) < total_count,
            "trades": [
                {
                    "id": t.id,
                    "timestamp": t.timestamp.isoformat() + "Z" if t.timestamp else None,
                    "path": t.path,
                    "trade_amount": t.trade_amount,
                    "paper_profit_pct": t.paper_profit_pct,
                    "shadow_profit_pct": t.shadow_profit_pct,
                    "difference_pct": t.difference_pct,
                    "would_have_profited": t.would_have_profited,
                    "latency_ms": t.latency_ms,
                    "success": t.success,
                    "reason": t.reason,
                }
                for t in trades
            ]
        }
    finally:
        db.close()


@router.get("/shadow/trades/stats")
async def get_shadow_trades_stats(hours: int = Query(default=24, ge=1, le=720)):
    """Get shadow trades statistics from database"""
    from sqlalchemy import select, func, and_
    from app.models.models import ShadowTrade
    from app.core.database import SessionLocal
    
    db = SessionLocal()
    try:
        since = datetime.utcnow() - timedelta(hours=hours)
        
        # Total trades
        total_query = select(func.count(ShadowTrade.id)).where(ShadowTrade.timestamp >= since)
        total = db.execute(total_query).scalar() or 0
        
        # Wins
        wins_query = select(func.count(ShadowTrade.id)).where(
            and_(ShadowTrade.timestamp >= since, ShadowTrade.would_have_profited == True)
        )
        wins = db.execute(wins_query).scalar() or 0
        
        # Avg paper profit
        avg_paper_query = select(func.avg(ShadowTrade.paper_profit_pct)).where(ShadowTrade.timestamp >= since)
        avg_paper = db.execute(avg_paper_query).scalar() or 0
        
        # Avg shadow profit
        avg_shadow_query = select(func.avg(ShadowTrade.shadow_profit_pct)).where(ShadowTrade.timestamp >= since)
        avg_shadow = db.execute(avg_shadow_query).scalar() or 0
        
        # Avg latency
        avg_latency_query = select(func.avg(ShadowTrade.latency_ms)).where(ShadowTrade.timestamp >= since)
        avg_latency = db.execute(avg_latency_query).scalar() or 0
        
        return {
            "hours": hours,
            "total_trades": total,
            "wins": wins,
            "losses": total - wins,
            "win_rate": (wins / total * 100) if total > 0 else 0,
            "avg_paper_profit_pct": float(avg_paper) if avg_paper else 0,
            "avg_shadow_profit_pct": float(avg_shadow) if avg_shadow else 0,
            "avg_difference_pct": float(avg_paper - avg_shadow) if avg_paper and avg_shadow else 0,
            "avg_latency_ms": float(avg_latency) if avg_latency else 0,
        }
    finally:
        db.close()


@router.get("/shadow/accuracy")
async def get_shadow_accuracy():
    """Get accuracy report comparing paper vs shadow trading"""
    from app.core.shadow_executor import get_shadow_executor
    
    executor = get_shadow_executor()
    if not executor:
        raise HTTPException(status_code=503, detail="Shadow executor not initialized")
        
    return executor.get_accuracy_report()


@router.get("/shadow/trades/detailed")
async def get_shadow_trades_detailed(
    limit: int = Query(default=50, le=500),
    offset: int = Query(default=0, ge=0),
    hours: int = Query(default=24, ge=1, le=720),
    result_filter: Optional[str] = Query(default=None, description="Filter by result: 'win', 'loss', or None for all"),
    path_filter: Optional[str] = Query(default=None, description="Filter by path (partial match)"),
):
    """Get detailed shadow trades with real Kraken fees and slippage"""
    from sqlalchemy import select, func, desc, and_
    from app.models.models import ShadowTradeDetailed
    from app.core.database import SessionLocal
    
    db = SessionLocal()
    try:
        # Base query
        since = datetime.utcnow() - timedelta(hours=hours)
        conditions = [ShadowTradeDetailed.timestamp >= since]
        
        # Apply filters
        if result_filter == "win":
            conditions.append(ShadowTradeDetailed.status == "WIN")
        elif result_filter == "loss":
            conditions.append(ShadowTradeDetailed.status == "LOSS")
            
        if path_filter:
            # Filter by paths starting with the specified currency
            conditions.append(ShadowTradeDetailed.path.ilike(f"{path_filter}%"))
        
        # Get total count
        count_query = select(func.count(ShadowTradeDetailed.id)).where(and_(*conditions))
        total_count = db.execute(count_query).scalar() or 0
        
        # Get trades
        query = (
            select(ShadowTradeDetailed)
            .where(and_(*conditions))
            .order_by(desc(ShadowTradeDetailed.timestamp))
            .offset(offset)
            .limit(limit)
        )
        
        trades = db.execute(query).scalars().all()
        
        return {
            "total": total_count,
            "limit": limit,
            "offset": offset,
            "has_more": (offset + limit) < total_count,
            "trades": [
                {
                    "id": t.id,
                    "timestamp": t.timestamp.isoformat() + "Z" if t.timestamp else None,
                    "path": t.path,
                    "legs": t.legs,
                    "amount": t.amount,
                    "taker_fee_pct": t.taker_fee_pct,
                    "taker_fee_usd": t.taker_fee_usd,
                    "total_slippage_pct": t.total_slippage_pct,
                    "total_slippage_usd": t.total_slippage_usd,
                    "gross_profit_pct": t.gross_profit_pct,
                    "net_profit_pct": t.net_profit_pct,
                    "net_profit_usd": t.net_profit_usd,
                    "status": t.status,
                    "leg_details": t.leg_details,
                }
                for t in trades
            ]
        }
    finally:
        db.close()


@router.get("/shadow/trades/detailed/{trade_id}")
async def get_shadow_trade_detail(trade_id: int):
    """Get single detailed shadow trade with leg breakdown"""
    from sqlalchemy import select
    from app.models.models import ShadowTradeDetailed
    from app.core.database import SessionLocal
    
    db = SessionLocal()
    try:
        query = select(ShadowTradeDetailed).where(ShadowTradeDetailed.id == trade_id)
        trade = db.execute(query).scalar_one_or_none()
        
        if not trade:
            raise HTTPException(status_code=404, detail="Trade not found")
        
        return {
            "id": trade.id,
            "timestamp": trade.timestamp.isoformat() + "Z" if trade.timestamp else None,
            "path": trade.path,
            "legs": trade.legs,
            "amount": trade.amount,
            "taker_fee_pct": trade.taker_fee_pct,
            "taker_fee_usd": trade.taker_fee_usd,
            "total_slippage_pct": trade.total_slippage_pct,
            "total_slippage_usd": trade.total_slippage_usd,
            "gross_profit_pct": trade.gross_profit_pct,
            "net_profit_pct": trade.net_profit_pct,
            "net_profit_usd": trade.net_profit_usd,
            "status": trade.status,
            "leg_details": trade.leg_details,
        }
    finally:
        db.close()


@router.post("/shadow/execute")
async def execute_shadow_trade(
    path: str,
    trade_amount: float = 10.0,
    expected_profit_pct: float = 0.1,
    slippage_pct: float = 0.05,
):
    """Manually trigger a shadow trade execution"""
    from app.core.shadow_executor import get_shadow_executor
    
    executor = get_shadow_executor()
    if not executor:
        raise HTTPException(status_code=503, detail="Shadow executor not initialized")
        
    result = await executor.execute_shadow(
        path=path,
        trade_amount_usd=trade_amount,
        paper_expected_profit_pct=expected_profit_pct,
        paper_slippage_pct=slippage_pct,
    )
    
    return {
        "success": result.success,
        "path": result.path,
        "paper_profit_pct": result.paper_profit_pct,
        "shadow_profit_pct": result.shadow_profit_pct,
        "difference_pct": result.difference_pct,
        "latency_ms": result.latency_ms,
        "would_have_profited": result.would_have_profited,
        "reason": result.reason,
    }


@router.post("/shadow/mode")
async def set_shadow_mode(enable_shadow: bool = True):
    """Switch between shadow and live mode"""
    from app.core.kraken_client import get_kraken_client
    
    client = get_kraken_client()
    if not client:
        raise HTTPException(status_code=503, detail="Kraken client not initialized")
        
    if enable_shadow:
        client.disable_live_trading()
        return {"mode": "shadow", "message": "Shadow mode enabled. Trades will be logged but not executed."}
    else:
        # Don't auto-enable live trading - require explicit confirmation
        return {
            "mode": "shadow", 
            "message": "Live trading cannot be enabled via API for safety. Use enable_live_trading endpoint with confirmation.",
        }


@router.post("/shadow/enable-live")
async def enable_live_trading(confirm: bool = False, confirm_text: str = ""):
    """
    Enable live trading. Requires explicit confirmation.
    You must pass confirm=true AND confirm_text="I understand the risks"
    """
    from app.core.kraken_client import get_kraken_client
    
    client = get_kraken_client()
    if not client:
        raise HTTPException(status_code=503, detail="Kraken client not initialized")
        
    if not confirm or confirm_text != "I understand the risks":
        raise HTTPException(
            status_code=400, 
            detail="Live trading requires confirm=true AND confirm_text='I understand the risks'"
        )
        
    try:
        client.enable_live_trading(confirm=True)
        return {
            "mode": "live",
            "message": "⚠️ LIVE TRADING ENABLED. Real money will be used!",
            "max_loss_usd": client.max_loss_usd,
        }
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))


@router.put("/paper-trading/settings")
async def update_paper_trading_settings_endpoint(updates: dict):
    """Update paper trading settings (legacy)"""
    return await update_trade_controls(updates)


@router.post("/paper-trading/toggle")
async def toggle_paper_trading(is_active: bool):
    """Toggle paper trading on/off (legacy)"""
    return await toggle_trading(is_active)


@router.get("/paper-trading/wallet")
async def get_paper_wallet():
    """Get paper wallet (legacy)"""
    engine = get_engine()
    
    if not engine:
        raise HTTPException(status_code=503, detail="Engine not available")
    
    state = engine.get_trading_state()
    
    return {
        "currency": "USD",
        "balance": state.balance,
        "initial_balance": state.initial_balance,
        "profit_loss": state.total_profit,
        "profit_loss_pct": (state.total_profit / state.initial_balance * 100) if state.initial_balance > 0 else 0,
    }


@router.get("/paper-trading/stats")
async def get_paper_trading_stats():
    """Get paper trading statistics (legacy)"""
    engine = get_engine()
    
    if not engine:
        raise HTTPException(status_code=503, detail="Engine not available")
    
    state = engine.get_trading_state()
    
    return {
        "total_trades": state.total_trades,
        "winning_trades": state.total_wins,
        "losing_trades": state.total_trades - state.total_wins,
        "win_rate": state.win_rate,
        "total_profit": state.total_profit,
        "current_balance": state.balance,
        "initial_balance": state.initial_balance,
    }


@router.post("/paper-trading/wallet/reset")
async def reset_paper_wallet(initial_balance: float = 100.0):
    """Reset paper wallet (legacy)"""
    return await reset_balance(initial_balance)


@router.get("/paper-trading/trades")
async def get_paper_trading_trades(
    limit: int = Query(default=50, le=500),
    db: AsyncSession = Depends(get_db),
):
    """Get paper trades (legacy)"""
    return await get_trades(limit=limit, db=db)


@router.post("/paper-trading/initialize")
async def initialize_paper_trading():
    """Initialize paper trading (legacy)"""
    engine = get_engine()
    if not engine:
        raise HTTPException(status_code=503, detail="Engine not available")
    
    return {
        "success": True,
        "message": "Paper trading initialized",
        "balance": engine.get_balance(),
    }


@router.get("/slots")
async def get_slots():
    """Get slots (legacy - returns single balance as slot 0)"""
    engine = get_engine()
    
    if not engine:
        raise HTTPException(status_code=503, detail="Engine not available")
    
    state = engine.get_trading_state()
    
    # Return single "slot" for backwards compatibility
    return {
        "slots": [
            {
                "id": 0,
                "balance": state.balance,
                "status": "COOLDOWN" if state.is_in_cooldown else "READY",
                "cooldown_until": state.cooldown_until,
                "trades_count": state.total_trades,
                "wins_count": state.total_wins,
                "win_rate": state.win_rate,
                "total_profit": state.total_profit,
            }
        ],
        "total_balance": state.balance,
        "total_profit": state.total_profit,
        "win_rate": state.win_rate,
        "ready_slots": 0 if state.is_in_cooldown else 1,
    }


@router.get("/stats/summary")
async def get_stats_summary():
    """Get statistics summary for frontend"""
    engine = get_engine()
    cached_opps = get_cached_opportunities()
    best_profit = get_best_profit()
    
    if not engine:
        return {
            "total_opportunities": 0,
            "profitable_opportunities": 0,
            "total_trades": 0,
            "win_rate": 0,
            "total_profit": 0,
        }
    
    stats = engine.get_stats()
    profitable_count = len([o for o in cached_opps if o.is_profitable])
    
    return {
        "total_opportunities": len(cached_opps),
        "profitable_opportunities": profitable_count,
        "total_trades": stats.trades_executed,
        "win_rate": stats.win_rate,
        "total_profit": stats.total_profit,
        "avg_profit_pct": sum(o.net_profit_pct for o in cached_opps) / len(cached_opps) if cached_opps else 0,
        "best_profit_pct": best_profit,
        "pairs_monitored": stats.pairs_monitored,
        "uptime_seconds": stats.uptime_seconds,
    }
