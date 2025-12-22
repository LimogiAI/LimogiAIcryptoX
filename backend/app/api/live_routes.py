"""
Live Trading API Routes

Endpoints for controlling and monitoring live trading.

SECURITY: All state-changing endpoints require API key authentication.
Read-only endpoints (status, config GET) are public.
"""
from fastapi import APIRouter, HTTPException, Query, Depends
from typing import Optional, List, Dict, Any
from pydantic import BaseModel
from loguru import logger

from app.core.auth import require_api_key


router = APIRouter(prefix="/live", tags=["Live Trading"])


def get_manager():
    """Get the live trading manager instance"""
    from app.core.live_trading import get_live_trading_manager
    manager = get_live_trading_manager()
    if not manager:
        raise HTTPException(status_code=503, detail="Live trading not initialized")
    return manager


# ==========================================
# Request/Response Models
# ==========================================

class ConfigUpdateRequest(BaseModel):
    """Request to update live trading configuration"""
    trade_amount: Optional[float] = None
    min_profit_threshold: Optional[float] = None
    max_daily_loss: Optional[float] = None
    max_total_loss: Optional[float] = None
    max_retries_per_leg: Optional[int] = None
    order_timeout_seconds: Optional[int] = None
    base_currency: Optional[str] = None
    custom_currencies: Optional[List[str]] = None


class EnableRequest(BaseModel):
    """Request to enable live trading"""
    confirm: bool = False
    confirm_text: str = ""


class ExecuteTradeRequest(BaseModel):
    """Request to manually execute a trade"""
    path: str
    amount: Optional[float] = None


# ==========================================
# Configuration Endpoints
# ==========================================

@router.get("/config")
async def get_config():
    """Get current live trading configuration"""
    manager = get_manager()
    config = manager.get_config()
    options = manager.get_config_options()
    
    return {
        "config": config.to_dict(),
        "options": options,
    }


@router.put("/config", dependencies=[Depends(require_api_key)])
async def update_config(request: ConfigUpdateRequest):
    """Update live trading configuration (syncs to Rust engine). Requires API key."""
    manager = get_manager()

    updates = {k: v for k, v in request.dict().items() if v is not None}

    if not updates:
        raise HTTPException(status_code=400, detail="No updates provided")

    try:
        config = manager.update_config(updates)

        # Sync config to Rust engine
        try:
            engine = get_engine()
            engine.update_trading_config(
                enabled=config.is_enabled,
                trade_amount=config.trade_amount,
                min_profit_threshold=config.min_profit_threshold * 100,  # Convert to percentage
                max_daily_loss=config.max_daily_loss,
                max_total_loss=config.max_total_loss,
                base_currency=config.base_currency,
            )
            logger.info("Trading config synced to Rust engine")
        except Exception as e:
            logger.warning(f"Failed to sync config to Rust engine: {e}")

        return {
            "success": True,
            "message": "Configuration updated",
            "config": config.to_dict(),
        }
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))


# ==========================================
# Enable/Disable Endpoints
# ==========================================

@router.post("/enable", dependencies=[Depends(require_api_key)])
async def enable_live_trading(request: EnableRequest):
    """
    Enable live trading (syncs to Rust engine). Requires API key.

    Requires:
    - confirm: true
    - confirm_text: "I understand the risks"
    """
    manager = get_manager()

    result = await manager.enable(
        confirm=request.confirm,
        confirm_text=request.confirm_text
    )

    if not result.get('success'):
        raise HTTPException(status_code=400, detail=result.get('error'))

    # Sync to Rust engine and enable auto-execution
    try:
        engine = get_engine()
        engine.enable_trading()
        # Enable auto-execution so Rust handles the full pipeline
        engine.enable_auto_execution()
        logger.info("Live trading + auto-execution enabled in Rust engine")
    except Exception as e:
        logger.warning(f"Failed to enable trading in Rust engine: {e}")

    return result


@router.post("/disable", dependencies=[Depends(require_api_key)])
async def disable_live_trading(reason: str = "Manual disable"):
    """Disable live trading (syncs to Rust engine). Requires API key."""
    manager = get_manager()
    result = manager.disable(reason)

    # Sync to Rust engine and disable auto-execution
    try:
        engine = get_engine()
        engine.disable_auto_execution()
        engine.disable_trading(reason)
        logger.info("Live trading + auto-execution disabled in Rust engine")
    except Exception as e:
        logger.warning(f"Failed to disable trading in Rust engine: {e}")

    return result


# ==========================================
# Status Endpoints
# ==========================================

@router.get("/status")
async def get_status():
    """Get complete live trading status"""
    manager = get_manager()
    return await manager.get_status()


@router.get("/state")
async def get_state():
    """Get circuit breaker state (includes partial trade stats)"""
    manager = get_manager()
    state = manager.get_circuit_breaker_state()
    return state.to_dict()


@router.get("/circuit-breaker")
async def get_circuit_breaker():
    """Get circuit breaker state"""
    manager = get_manager()
    state = manager.get_circuit_breaker_state()
    return state.to_dict()


@router.post("/circuit-breaker/reset", dependencies=[Depends(require_api_key)])
async def reset_circuit_breaker():
    """Reset circuit breaker (does not reset loss counters). Requires API key."""
    manager = get_manager()
    state = manager.reset_circuit_breaker()
    return {
        "success": True,
        "message": "Circuit breaker reset",
        "state": state.to_dict(),
    }


@router.post("/circuit-breaker/trigger", dependencies=[Depends(require_api_key)])
async def trigger_circuit_breaker(reason: str = "Manual trigger"):
    """Manually trigger circuit breaker. Requires API key."""
    manager = get_manager()
    manager.trigger_circuit_breaker(reason)
    state = manager.get_circuit_breaker_state()
    return {
        "success": True,
        "message": f"Circuit breaker triggered: {reason}",
        "state": state.to_dict(),
    }


# ==========================================
# Stats Reset Endpoints
# ==========================================

@router.post("/reset-daily", dependencies=[Depends(require_api_key)])
async def reset_daily_stats():
    """Reset daily statistics. Requires API key."""
    manager = get_manager()
    state = manager.reset_daily_stats()
    return {
        "success": True,
        "message": "Daily statistics reset",
        "state": state.to_dict(),
    }


@router.post("/reset-all", dependencies=[Depends(require_api_key)])
async def reset_all_stats(
    confirm: bool = Query(default=False),
    confirm_text: str = Query(default=""),
):
    """
    Reset ALL statistics (use with caution!). Requires API key.

    Requires:
    - confirm: true
    - confirm_text: "reset all stats"
    """
    if not confirm or confirm_text != "reset all stats":
        raise HTTPException(
            status_code=400,
            detail='Must confirm with confirm=true AND confirm_text="reset all stats"'
        )
    
    manager = get_manager()
    state = manager.reset_all_stats()
    
    logger.warning("All live trading stats were manually reset")
    
    return {
        "success": True,
        "message": "All statistics reset",
        "state": state.to_dict(),
    }


@router.post("/reset-partial", dependencies=[Depends(require_api_key)])
async def reset_partial_stats(
    confirm: bool = Query(default=False),
):
    """
    Reset partial trade statistics only. Requires API key.

    Use when you've manually resolved partial trades outside the system.
    """
    if not confirm:
        raise HTTPException(
            status_code=400,
            detail='Must confirm with confirm=true'
        )
    
    manager = get_manager()
    state = manager.circuit_breaker.reset_partial_stats()
    
    logger.warning("Partial trade stats were manually reset")
    
    return {
        "success": True,
        "message": "Partial trade statistics reset",
        "state": state.to_dict(),
    }


# ==========================================
# Trade Execution Endpoints
# ==========================================

@router.post("/execute", dependencies=[Depends(require_api_key)])
async def execute_trade(request: ExecuteTradeRequest):
    """
    Manually execute a live trade. Requires API key.

    Use this for testing or manual arbitrage execution.
    """
    manager = get_manager()
    
    result = await manager.execute_trade(
        path=request.path,
        amount=request.amount,
        opportunity_profit_pct=0.0,  # Manual trades don't have expected profit
    )
    
    return result.to_dict()


# ==========================================
# Trade History Endpoints
# ==========================================

@router.get("/trades")
async def get_trades(
    limit: int = Query(default=50, le=500),
    status: Optional[str] = Query(default=None),
    hours: int = Query(default=24, le=720),
):
    """Get recent live trades"""
    manager = get_manager()
    
    trades = manager.get_trades(
        limit=limit,
        status=status,
        hours=hours,
    )
    
    return {
        "count": len(trades),
        "trades": trades,
    }


@router.get("/trades/partial")
async def get_partial_trades():
    """Get all unresolved partial trades"""
    manager = get_manager()
    
    trades = manager.get_trades(
        limit=100,
        status='PARTIAL',
        hours=720,  # 30 days
    )
    
    return {
        "count": len(trades),
        "trades": trades,
    }


@router.get("/trades/{trade_id}")
async def get_trade(trade_id: str):
    """Get a specific trade by ID"""
    manager = get_manager()
    
    trade = manager.get_trade_by_id(trade_id)
    
    if not trade:
        raise HTTPException(status_code=404, detail="Trade not found")
    
    return trade


# ==========================================
# Partial Trade Resolution Endpoints
# ==========================================

@router.post("/trades/{trade_id}/resolve", dependencies=[Depends(require_api_key)])
async def resolve_partial_trade(trade_id: str):
    """
    Resolve a PARTIAL trade by selling the held currency back to USD. Requires API key.

    This will:
    1. Sell the held crypto for USD
    2. Calculate actual P/L
    3. Update the trade status to RESOLVED
    4. Move from partial tracking to completed totals
    """
    manager = get_manager()
    
    # Get the trade first to validate
    trade = manager.get_trade_by_id(trade_id)
    
    if not trade:
        raise HTTPException(status_code=404, detail="Trade not found")
    
    if trade.get('status') != 'PARTIAL':
        raise HTTPException(
            status_code=400, 
            detail=f"Trade is not PARTIAL (current status: {trade.get('status')})"
        )
    
    if not trade.get('held_currency') or not trade.get('held_amount'):
        raise HTTPException(
            status_code=400,
            detail="Trade has no held position to resolve"
        )
    
    # Execute the resolution
    result = await manager.executor.resolve_partial_trade(trade_id)
    
    if not result:
        raise HTTPException(
            status_code=500,
            detail="Failed to resolve partial trade - check logs for details"
        )
    
    # Get updated state
    state = manager.get_circuit_breaker_state()
    
    return {
        "success": True,
        "message": f"Partial trade resolved: {trade_id}",
        "resolution": result.to_dict(),
        "state": state.to_dict(),
    }


@router.get("/trades/{trade_id}/resolve-preview")
async def preview_resolve_partial_trade(trade_id: str):
    """
    Preview what would happen if we resolve a partial trade.
    
    Shows current held position and estimated USD value.
    Does NOT execute any trades.
    """
    manager = get_manager()
    
    # Get the trade
    trade = manager.get_trade_by_id(trade_id)
    
    if not trade:
        raise HTTPException(status_code=404, detail="Trade not found")
    
    if trade.get('status') != 'PARTIAL':
        raise HTTPException(
            status_code=400, 
            detail=f"Trade is not PARTIAL (current status: {trade.get('status')})"
        )
    
    held_currency = trade.get('held_currency')
    held_amount = trade.get('held_amount')
    
    if not held_currency or not held_amount:
        raise HTTPException(
            status_code=400,
            detail="Trade has no held position"
        )
    
    # Get current USD value
    current_value = await manager.executor._get_usd_value(held_currency, held_amount)
    
    original_amount = trade.get('amount_in', 0)
    snapshot_value = trade.get('held_value_usd')
    
    estimated_pl = (current_value or 0) - original_amount
    snapshot_pl = (snapshot_value or 0) - original_amount if snapshot_value else None
    
    return {
        "trade_id": trade_id,
        "held_currency": held_currency,
        "held_amount": held_amount,
        "original_amount_usd": original_amount,
        "snapshot_value_usd": snapshot_value,
        "snapshot_pl": snapshot_pl,
        "current_value_usd": current_value,
        "estimated_pl": estimated_pl,
        "estimated_pl_pct": (estimated_pl / original_amount * 100) if original_amount > 0 else 0,
        "note": "These are estimates. Actual P/L will be determined after selling.",
    }


# ==========================================
# Positions Endpoint
# ==========================================

@router.get("/positions")
async def get_positions():
    """Get current positions from Kraken"""
    manager = get_manager()
    return await manager.get_positions()


# ==========================================
# Quick Actions for UI
# ==========================================

@router.post("/quick-disable", dependencies=[Depends(require_api_key)])
async def quick_disable():
    """Quick disable for emergency stop button (syncs to Rust). Requires API key."""
    manager = get_manager()
    manager.disable("Emergency stop")
    manager.trigger_circuit_breaker("Emergency stop")

    # CRITICAL: Sync to Rust engine immediately (disable auto-execution first)
    try:
        engine = get_engine()
        engine.disable_auto_execution()  # Stop auto-execution immediately
        engine.disable_trading("Emergency stop")
        engine.trip_circuit_breaker("Emergency stop")
        logger.info("Emergency stop: Auto-execution + trading disabled, circuit breaker tripped")
    except Exception as e:
        logger.error(f"CRITICAL: Failed to sync emergency stop to Rust engine: {e}")

    return {
        "success": True,
        "message": "Live trading disabled and circuit breaker triggered",
    }


# NOTE: /opportunities endpoint removed - LiveOpportunity was dead code
# All scanning happens in Rust, trade results saved to live_trades table
# Use /trades endpoint to see executed trades


# ==========================================
# Scanner Status Endpoint
# ==========================================

@router.get("/scanner/status")
async def get_scanner_status():
    """Get current scanner status"""
    manager = get_manager()
    return manager.get_scanner_status()


@router.post("/scanner/start", dependencies=[Depends(require_api_key)])
async def start_scanner():
    """Start the UI cache manager (fetches from Rust engine for UI). Requires API key."""
    from app.core.live_trading import get_ui_cache

    ui_cache = get_ui_cache()
    if not ui_cache:
        raise HTTPException(status_code=503, detail="UI cache manager not initialized")

    ui_cache.start()

    manager = get_manager()
    return {
        "success": True,
        "message": "UI cache manager started",
        "status": manager.get_scanner_status(),
    }


@router.post("/scanner/stop", dependencies=[Depends(require_api_key)])
async def stop_scanner():
    """Stop the UI cache manager. Requires API key."""
    from app.core.live_trading import get_ui_cache

    ui_cache = get_ui_cache()
    if not ui_cache:
        raise HTTPException(status_code=503, detail="UI cache manager not initialized")

    ui_cache.stop()

    manager = get_manager()
    return {
        "success": True,
        "message": "UI cache manager stopped",
        "status": manager.get_scanner_status(),
    }


# ==========================================
# Rust Execution Engine Endpoints (Phase 4-6)
# ==========================================

def get_engine():
    """Get the Rust trading engine instance"""
    from app.main import engine
    if not engine:
        raise HTTPException(status_code=503, detail="Trading engine not initialized")
    return engine


class RustExecutionConfigRequest(BaseModel):
    """Request to configure Rust execution engine"""
    api_key: str
    api_secret: str


class FeeConfigRequest(BaseModel):
    """Request to configure fee optimization"""
    maker_fee: float = 0.0016  # 0.16%
    taker_fee: float = 0.0026  # 0.26%
    min_profit_for_maker: float = 0.5  # Minimum profit % to try maker orders
    max_spread_for_maker: float = 0.1  # Maximum spread % for maker orders
    use_maker_for_intermediate: bool = False  # Enable maker for non-final legs


class RustExecuteRequest(BaseModel):
    """Request to execute via Rust engine"""
    path: str
    amount: float


@router.post("/execution-engine/init", dependencies=[Depends(require_api_key)])
async def init_execution_engine(request: RustExecutionConfigRequest):
    """
    Initialize the Rust execution engine with Kraken API credentials. Requires API key.

    This must be called before using any Rust-based execution features.
    The engine will use WebSocket v2 for faster order placement (~50ms vs ~500ms REST).
    """
    try:
        engine = get_engine()
        engine.init_execution_engine(request.api_key, request.api_secret)

        return {
            "success": True,
            "message": "Rust execution engine initialized",
            "features": {
                "websocket_execution": True,
                "fee_optimization": True,
            }
        }
    except Exception as e:
        logger.error(f"Failed to initialize execution engine: {e}")
        raise HTTPException(status_code=500, detail=str(e))


@router.post("/execution-engine/connect", dependencies=[Depends(require_api_key)])
async def connect_execution_engine():
    """
    Connect the Rust execution engine to Kraken's private WebSocket. Requires API key.

    Must call /init first with API credentials.
    """
    try:
        engine = get_engine()
        engine.connect_execution_engine()

        return {
            "success": True,
            "message": "Connected to Kraken private WebSocket",
            "connected": engine.is_execution_engine_connected(),
        }
    except Exception as e:
        logger.error(f"Failed to connect execution engine: {e}")
        raise HTTPException(status_code=500, detail=str(e))


@router.post("/execution-engine/disconnect", dependencies=[Depends(require_api_key)])
async def disconnect_execution_engine():
    """Disconnect the Rust execution engine from Kraken's private WebSocket. Requires API key."""
    try:
        engine = get_engine()
        engine.disconnect_execution_engine()

        return {
            "success": True,
            "message": "Disconnected from Kraken private WebSocket",
        }
    except Exception as e:
        logger.error(f"Failed to disconnect execution engine: {e}")
        raise HTTPException(status_code=500, detail=str(e))


@router.get("/execution-engine/status")
async def get_execution_engine_status():
    """Get Rust execution engine status and statistics."""
    try:
        engine = get_engine()

        is_connected = engine.is_execution_engine_connected()
        orders_placed, orders_filled, orders_failed, total_volume, _ = engine.get_execution_stats()

        # Get trading stats from Rust guard
        trades_executed, trades_successful, opps_seen, opps_executed, daily_pnl, total_pnl = \
            engine.get_trading_stats()

        # Get trading config from Rust
        enabled, trade_amount, min_profit, max_daily, max_total, base_currency = \
            engine.get_trading_config()

        # Get circuit breaker state
        is_broken, broken_reason, cb_daily_pnl, cb_total_pnl, daily_trades, total_trades, is_executing = \
            engine.get_circuit_breaker_state()

        return {
            "connected": is_connected,
            "trading_enabled": enabled,
            "stats": {
                "orders_placed": orders_placed,
                "orders_filled": orders_filled,
                "orders_failed": orders_failed,
                "total_volume_usd": total_volume,
                "fill_rate": (orders_filled / orders_placed * 100) if orders_placed > 0 else 0,
                "trades_executed": trades_executed,
                "trades_successful": trades_successful,
                "success_rate": (trades_successful / trades_executed) if trades_executed > 0 else 0,
                "daily_pnl": daily_pnl,
                "total_pnl": total_pnl,
                "total_profit": total_pnl,  # Alias for UI compatibility
            },
            "config": {
                "trade_amount": trade_amount,
                "min_profit_threshold": min_profit,
                "max_daily_loss": max_daily,
                "max_total_loss": max_total,
                "base_currency": base_currency,
            },
            "circuit_breaker": {
                "is_broken": is_broken,
                "reason": broken_reason,
                "daily_trades": daily_trades,
                "total_trades": total_trades,
                "is_executing": is_executing,
            }
        }
    except Exception as e:
        logger.error(f"Failed to get execution engine status: {e}")
        raise HTTPException(status_code=500, detail=str(e))


@router.post("/execution-engine/execute", dependencies=[Depends(require_api_key)])
async def execute_via_rust(request: RustExecuteRequest):
    """
    Execute an arbitrage opportunity via the Rust execution engine. Requires API key.

    Executes legs sequentially, waiting for each to complete before the next.
    This is the safest approach for triangular arbitrage.
    """
    try:
        engine = get_engine()

        if not engine.is_execution_engine_connected():
            raise HTTPException(
                status_code=400,
                detail="Execution engine not connected. Call /execution-engine/connect first."
            )

        success, legs_completed, total_input, total_output, profit_pct, error = \
            engine.execute_opportunity(request.path, request.amount)

        return {
            "success": success,
            "legs_completed": legs_completed,
            "total_input": total_input,
            "total_output": total_output,
            "profit_pct": profit_pct,
            "profit_amount": total_output - total_input,
            "error": error if error else None,
        }
    except Exception as e:
        logger.error(f"Rust execution failed: {e}")
        raise HTTPException(status_code=500, detail=str(e))


# ==========================================
# Fee Optimization Endpoints (Phase 6)
# ==========================================

@router.get("/fee-config")
async def get_fee_config():
    """Get current fee optimization configuration."""
    try:
        engine = get_engine()
        maker_fee, taker_fee, min_profit, max_spread, use_maker = engine.get_fee_config()

        return {
            "maker_fee": maker_fee,
            "maker_fee_pct": maker_fee * 100,
            "taker_fee": taker_fee,
            "taker_fee_pct": taker_fee * 100,
            "fee_savings_potential_pct": (taker_fee - maker_fee) * 100,
            "min_profit_for_maker": min_profit,
            "max_spread_for_maker": max_spread,
            "use_maker_for_intermediate": use_maker,
        }
    except Exception as e:
        logger.error(f"Failed to get fee config: {e}")
        raise HTTPException(status_code=500, detail=str(e))


@router.get("/kraken-fees")
async def get_kraken_account_fees():
    """
    Fetch REAL fee tier from Kraken based on your 30-day trading volume.

    This calls Kraken's TradeVolume API to get your actual maker/taker fees,
    which depend on your trading volume tier.

    Fee tiers (as of 2024):
    - $0-$50K: 0.26% taker, 0.16% maker
    - $50K-$100K: 0.24% taker, 0.14% maker
    - $100K-$250K: 0.22% taker, 0.12% maker
    - And continues down with higher volume
    """
    try:
        manager = get_manager()
        kraken_client = manager.kraken_client

        if not kraken_client:
            raise HTTPException(status_code=503, detail="Kraken client not available")

        # Fetch real fees from Kraken
        fees_info = await kraken_client.get_trade_fees()

        # Check for errors
        if "error" in fees_info:
            return {
                "success": False,
                "error": fees_info["error"],
                "using_defaults": True,
                "default_taker_fee": 0.26,
                "default_maker_fee": 0.16,
            }

        # Calculate average fee from returned pairs
        taker_fees = [f["fee"] for f in fees_info.get("fees", {}).values()]
        maker_fees = [f["fee"] for f in fees_info.get("fees_maker", {}).values()]

        avg_taker = sum(taker_fees) / len(taker_fees) if taker_fees else 0.26
        avg_maker = sum(maker_fees) / len(maker_fees) if maker_fees else 0.16

        return {
            "success": True,
            "30_day_volume_usd": fees_info.get("volume", 0),
            "currency": fees_info.get("currency", "ZUSD"),
            "taker_fee_pct": avg_taker,
            "maker_fee_pct": avg_maker,
            "fee_savings_pct": avg_taker - avg_maker,
            "pairs_queried": len(taker_fees),
            "raw_fees": fees_info.get("fees", {}),
            "raw_fees_maker": fees_info.get("fees_maker", {}),
        }
    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Failed to fetch Kraken fees: {e}")
        raise HTTPException(status_code=500, detail=str(e))


@router.post("/sync-kraken-fees", dependencies=[Depends(require_api_key)])
async def sync_kraken_fees_to_engine():
    """
    Fetch real fees from Kraken and update the Rust engine to use them. Requires API key.

    This ensures profit calculations use your actual fee tier instead of defaults.
    Call this after connecting to update fees based on your trading volume.
    """
    try:
        manager = get_manager()
        kraken_client = manager.kraken_client
        engine = get_engine()

        if not kraken_client:
            raise HTTPException(status_code=503, detail="Kraken client not available")

        # Fetch real fees from Kraken
        fees_info = await kraken_client.get_trade_fees()

        if "error" in fees_info:
            return {
                "success": False,
                "error": fees_info["error"],
                "message": "Using default fees - could not fetch from Kraken",
            }

        # Calculate average fee from returned pairs
        taker_fees = [f["fee"] for f in fees_info.get("fees", {}).values()]
        maker_fees = [f["fee"] for f in fees_info.get("fees_maker", {}).values()]

        if not taker_fees:
            return {
                "success": False,
                "error": "No fee data returned from Kraken",
                "message": "Using default fees",
            }

        avg_taker = sum(taker_fees) / len(taker_fees)
        avg_maker = sum(maker_fees) / len(maker_fees) if maker_fees else avg_taker - 0.10

        # Convert from percentage (0.26) to decimal (0.0026)
        taker_decimal = avg_taker / 100
        maker_decimal = avg_maker / 100

        # Get current config to preserve other settings
        _, _, min_profit, max_spread, use_maker = engine.get_fee_config()

        # Update Rust engine with real fees
        engine.set_fee_config(
            maker_decimal,
            taker_decimal,
            min_profit,
            max_spread,
            use_maker,
        )

        logger.info(f"Synced Kraken fees: taker={avg_taker:.2f}%, maker={avg_maker:.2f}%")

        return {
            "success": True,
            "message": "Fees synced from Kraken to engine",
            "30_day_volume_usd": fees_info.get("volume", 0),
            "taker_fee_pct": avg_taker,
            "maker_fee_pct": avg_maker,
            "taker_fee_decimal": taker_decimal,
            "maker_fee_decimal": maker_decimal,
            "pairs_sampled": len(taker_fees),
        }
    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Failed to sync Kraken fees: {e}")
        raise HTTPException(status_code=500, detail=str(e))


@router.put("/fee-config", dependencies=[Depends(require_api_key)])
async def update_fee_config(request: FeeConfigRequest):
    """
    Update fee optimization configuration. Requires API key.

    Fee optimization automatically selects maker vs taker orders based on:
    - Opportunity profit margin
    - Order book spread
    - Leg position (final leg always uses taker for certainty)
    """
    try:
        engine = get_engine()
        engine.set_fee_config(
            request.maker_fee,
            request.taker_fee,
            request.min_profit_for_maker,
            request.max_spread_for_maker,
            request.use_maker_for_intermediate,
        )

        return {
            "success": True,
            "message": "Fee configuration updated",
            "config": {
                "maker_fee": request.maker_fee,
                "taker_fee": request.taker_fee,
                "min_profit_for_maker": request.min_profit_for_maker,
                "max_spread_for_maker": request.max_spread_for_maker,
                "use_maker_for_intermediate": request.use_maker_for_intermediate,
            }
        }
    except Exception as e:
        logger.error(f"Failed to update fee config: {e}")
        raise HTTPException(status_code=500, detail=str(e))


@router.get("/fee-stats")
async def get_fee_optimization_stats():
    """Get fee optimization statistics."""
    try:
        engine = get_engine()
        attempted, filled, savings, success_rate = engine.get_fee_optimization_stats()

        return {
            "maker_orders_attempted": attempted,
            "maker_orders_filled": filled,
            "total_savings_usd": savings,
            "success_rate_pct": success_rate,
            "recommendation": "Enable maker orders for intermediate legs if success_rate > 70%"
                if success_rate > 70 else "Keep using taker orders for reliability"
        }
    except Exception as e:
        logger.error(f"Failed to get fee stats: {e}")
        raise HTTPException(status_code=500, detail=str(e))


