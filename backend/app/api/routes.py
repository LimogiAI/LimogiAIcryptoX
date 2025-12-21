"""
API Routes for LimogiAICryptoX v2.0 - Live Trading Platform
"""
from fastapi import APIRouter, Depends, HTTPException, Query
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select, desc
from typing import List, Optional
from datetime import datetime, timedelta

from app.core.database import get_db

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
    "min_profit_threshold": 0.0005,
    "fee_rate": 0.0026,
    "base_currency": "ALL",
    "custom_currencies": [],
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
                min_profit_threshold=_runtime_settings.get("min_profit_threshold"),
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
        return {
            "is_running": stats.is_running,
            "engine": "rust_v2",
            "pairs_monitored": stats.pairs_monitored,
            "currencies_tracked": stats.currencies_tracked,
            "orderbooks_cached": stats.orderbooks_cached,
            "avg_staleness_ms": stats.avg_orderbook_staleness_ms,
            "opportunities_found": stats.opportunities_found,
            "opportunities_per_second": stats.opportunities_per_second,
            "uptime_seconds": stats.uptime_seconds,
            "scan_cycle_ms": stats.scan_cycle_ms,
            "scan_interval_ms": settings.scan_interval_ms,
            "max_pairs": settings.max_pairs,
            "orderbook_depth": settings.orderbook_depth,
            "last_scan_at": stats.last_scan_at,
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
            "options": {
                "scan_interval_ms": [100, 250, 500, 1000, 2000, 5000, 7000, 10000],
                "max_pairs": [100, 200, 300, 400],
                "orderbook_depth": [10, 25, 100, 500, 1000],
            },
        }
    else:
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
    """Update engine settings at runtime."""
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

    if all(v is None for v in [scan_interval_ms, max_pairs, orderbook_depth, scanner_enabled]):
        raise HTTPException(status_code=400, detail="No settings provided to update")

    try:
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
    """Hot-reload the engine with new settings."""
    engine = get_engine()

    if not engine:
        raise HTTPException(status_code=503, detail="Engine not available")

    try:
        engine.reconnect_websocket()
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


@router.get("/event-scanner-stats")
async def get_event_scanner_stats():
    """Get event scanner statistics (debug endpoint)"""
    engine = get_engine()
    if not engine:
        return {"error": "Engine not available"}

    try:
        # Get detailed stats
        stats = engine.get_event_scanner_stats_detailed()
        auto_stats = engine.get_auto_execution_stats()
        graph_stats = engine.get_graph_detailed_stats()

        # Debug: count paths
        path_counts = engine.debug_count_paths()
        usd_connections = engine.debug_get_usd_connections()

        # Get cached opportunities (all, not just profitable)
        cached_opps, age_ms = engine.get_cached_opportunities_with_age()

        # Find best opportunities (even if negative profit)
        best_opps = sorted(cached_opps, key=lambda o: o.net_profit_pct, reverse=True)[:5]
        best_formatted = [
            {
                "path": o.path,
                "legs": o.legs,
                "gross_profit_pct": round(o.gross_profit_pct, 4),
                "fees_pct": round(o.fees_pct, 4),
                "net_profit_pct": round(o.net_profit_pct, 4),
                "is_profitable": o.is_profitable,
            }
            for o in best_opps
        ]

        return {
            "event_count": stats[0],
            "scan_count": stats[1],
            "incremental_updates": stats[2],
            "full_rebuilds": stats[3],
            "opportunities_found": stats[4],
            "graph_nodes": stats[5],
            "graph_edges": stats[6],
            "incremental_enabled": stats[7],
            "auto_executions": auto_stats[0],
            "auto_execution_successes": auto_stats[1],
            "graph_details": {
                "nodes": graph_stats[0],
                "pairs": graph_stats[1],
                "total_edges": graph_stats[2],
                "valid_edges": graph_stats[3],
            },
            "debug": {
                "usd_paths": path_counts[0],
                "eur_paths": path_counts[1],
                "usd_connections": usd_connections[:10],
                "cached_opps_count": len(cached_opps),
                "cache_age_ms": age_ms,
                "best_opportunities": best_formatted,
            }
        }
    except Exception as e:
        return {"error": str(e)}


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
    hours: int = Query(default=24, le=720),
    db: AsyncSession = Depends(get_db),
):
    """Get order book health history for trend charts"""
    from app.models.models import OrderBookHealthHistory

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
    """Get historical opportunities"""
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

    total_query = select(func.count(OpportunityHistory.id)).where(
        OpportunityHistory.timestamp >= cutoff
    )
    total_result = await db.execute(total_query)
    total_count = total_result.scalar() or 0

    profitable_query = select(func.count(OpportunityHistory.id)).where(
        OpportunityHistory.timestamp >= cutoff,
        OpportunityHistory.is_profitable == True
    )
    profitable_result = await db.execute(profitable_query)
    profitable_count = profitable_result.scalar() or 0

    traded_query = select(func.count(OpportunityHistory.id)).where(
        OpportunityHistory.timestamp >= cutoff,
        OpportunityHistory.was_traded == True
    )
    traded_result = await db.execute(traded_query)
    traded_count = traded_result.scalar() or 0

    avg_profit_query = select(func.avg(OpportunityHistory.expected_profit_pct)).where(
        OpportunityHistory.timestamp >= cutoff,
        OpportunityHistory.is_profitable == True
    )
    avg_profit_result = await db.execute(avg_profit_query)
    avg_profit = avg_profit_result.scalar() or 0

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

    if base_currency and base_currency != "ALL":
        opportunities = [o for o in opportunities if o.path.startswith(base_currency)]

    if profitable_only:
        opportunities = [o for o in opportunities if o.is_profitable]

    if min_profit_pct is not None:
        opportunities = [o for o in opportunities if o.net_profit_pct >= min_profit_pct]

    if sort_by == "profit":
        opportunities = sorted(opportunities, key=lambda o: o.net_profit_pct, reverse=True)

    opportunities = opportunities[:limit]

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
# Trade Controls (Settings)
# ============================================

@router.get("/trade-controls")
async def get_trade_controls():
    """Get trade control settings"""
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
# Stats Summary
# ============================================

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
        }

    stats = engine.get_stats()
    profitable_count = len([o for o in cached_opps if o.is_profitable])

    return {
        "total_opportunities": len(cached_opps),
        "profitable_opportunities": profitable_count,
        "avg_profit_pct": sum(o.net_profit_pct for o in cached_opps) / len(cached_opps) if cached_opps else 0,
        "best_profit_pct": best_profit,
        "pairs_monitored": stats.pairs_monitored,
        "uptime_seconds": stats.uptime_seconds,
    }
