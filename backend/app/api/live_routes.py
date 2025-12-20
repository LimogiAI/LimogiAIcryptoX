"""
Live Trading API Routes

Endpoints for controlling and monitoring live trading.
"""
from fastapi import APIRouter, HTTPException, Query
from typing import Optional, List, Dict, Any
from pydantic import BaseModel
from loguru import logger


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
    execution_mode: Optional[str] = None
    max_parallel_trades: Optional[int] = None
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


@router.put("/config")
async def update_config(request: ConfigUpdateRequest):
    """Update live trading configuration"""
    manager = get_manager()
    
    updates = {k: v for k, v in request.dict().items() if v is not None}
    
    if not updates:
        raise HTTPException(status_code=400, detail="No updates provided")
    
    try:
        config = manager.update_config(updates)
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

@router.post("/enable")
async def enable_live_trading(request: EnableRequest):
    """
    Enable live trading.
    
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
    
    return result


@router.post("/disable")
async def disable_live_trading(reason: str = "Manual disable"):
    """Disable live trading"""
    manager = get_manager()
    return manager.disable(reason)


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


@router.post("/circuit-breaker/reset")
async def reset_circuit_breaker():
    """Reset circuit breaker (does not reset loss counters)"""
    manager = get_manager()
    state = manager.reset_circuit_breaker()
    return {
        "success": True,
        "message": "Circuit breaker reset",
        "state": state.to_dict(),
    }


@router.post("/circuit-breaker/trigger")
async def trigger_circuit_breaker(reason: str = "Manual trigger"):
    """Manually trigger circuit breaker"""
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

@router.post("/reset-daily")
async def reset_daily_stats():
    """Reset daily statistics"""
    manager = get_manager()
    state = manager.reset_daily_stats()
    return {
        "success": True,
        "message": "Daily statistics reset",
        "state": state.to_dict(),
    }


@router.post("/reset-all")
async def reset_all_stats(
    confirm: bool = Query(default=False),
    confirm_text: str = Query(default=""),
):
    """
    Reset ALL statistics (use with caution!).
    
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


@router.post("/reset-partial")
async def reset_partial_stats(
    confirm: bool = Query(default=False),
):
    """
    Reset partial trade statistics only.
    
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

@router.post("/execute")
async def execute_trade(request: ExecuteTradeRequest):
    """
    Manually execute a live trade.
    
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

@router.post("/trades/{trade_id}/resolve")
async def resolve_partial_trade(trade_id: str):
    """
    Resolve a PARTIAL trade by selling the held currency back to USD.
    
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

@router.post("/quick-disable")
async def quick_disable():
    """Quick disable for emergency stop button"""
    manager = get_manager()
    manager.disable("Emergency stop")
    manager.trigger_circuit_breaker("Emergency stop")
    
    return {
        "success": True,
        "message": "Live trading disabled and circuit breaker triggered",
    }


# ==========================================
# Opportunities Endpoints
# ==========================================

@router.get("/opportunities")
async def get_opportunities(
    limit: int = Query(default=50, le=500),
    status: Optional[str] = Query(default=None),
    hours: int = Query(default=24, le=168),
):
    """
    Get recent live opportunities.
    
    Status can be: PENDING, EXECUTED, SKIPPED, MISSED, EXPIRED
    """
    manager = get_manager()
    
    opportunities = manager.get_opportunities(
        limit=limit,
        status=status,
        hours=hours,
    )
    
    return {
        "count": len(opportunities),
        "opportunities": opportunities,
    }


# ==========================================
# Scanner Status Endpoint
# ==========================================

@router.get("/scanner/status")
async def get_scanner_status():
    """Get current scanner status"""
    manager = get_manager()
    return manager.get_scanner_status()


@router.post("/scanner/start")
async def start_scanner():
    """Start the live trading scanner"""
    from app.core.live_trading import get_live_scanner
    
    scanner = get_live_scanner()
    if not scanner:
        raise HTTPException(status_code=503, detail="Live scanner not initialized")
    
    scanner.start()
    
    manager = get_manager()
    return {
        "success": True,
        "message": "Scanner started",
        "status": manager.get_scanner_status(),
    }


@router.post("/scanner/stop")
async def stop_scanner():
    """Stop the live trading scanner"""
    from app.core.live_trading import get_live_scanner
    
    scanner = get_live_scanner()
    if not scanner:
        raise HTTPException(status_code=503, detail="Live scanner not initialized")
    
    scanner.stop()
    
    manager = get_manager()
    return {
        "success": True,
        "message": "Scanner stopped",
        "status": manager.get_scanner_status(),
    }
