"""
WebSocket endpoints for real-time updates to frontend
"""
from fastapi import APIRouter, WebSocket, WebSocketDisconnect
from typing import List, Dict
import asyncio
import json
from datetime import datetime
from loguru import logger

router = APIRouter()


class ConnectionManager:
    """Manage WebSocket connections to frontend"""
    
    def __init__(self):
        self.active_connections: List[WebSocket] = []
    
    async def connect(self, websocket: WebSocket):
        await websocket.accept()
        self.active_connections.append(websocket)
        logger.info(f"WebSocket connected. Total: {len(self.active_connections)}")
    
    def disconnect(self, websocket: WebSocket):
        if websocket in self.active_connections:
            self.active_connections.remove(websocket)
        logger.info(f"WebSocket disconnected. Total: {len(self.active_connections)}")
    
    async def send_personal_message(self, message: Dict, websocket: WebSocket):
        try:
            await websocket.send_json(message)
        except Exception as e:
            logger.error(f"Failed to send message: {e}")
    
    async def broadcast(self, message: Dict):
        for connection in self.active_connections:
            try:
                await connection.send_json(message)
            except Exception:
                pass


manager = ConnectionManager()


def get_engine():
    """Get the global engine instance"""
    from app.main import get_engine as _get_engine
    return _get_engine()


@router.websocket("/ws")
async def websocket_endpoint(websocket: WebSocket):
    """
    Main WebSocket endpoint for frontend
    
    Messages from client:
    - {"type": "ping"}
    - {"type": "get_status"}
    - {"type": "get_slots"}
    
    Messages to client:
    - {"type": "status", "data": {...}}
    - {"type": "slots", "data": {...}}
    - {"type": "trade", "data": {...}}
    - {"type": "pong"}
    """
    await manager.connect(websocket)
    
    try:
        # Send initial status
        engine = get_engine()
        if engine:
            stats = engine.get_stats()
            await manager.send_personal_message({
                "type": "status",
                "data": {
                    "is_running": stats.is_running,
                    "pairs_monitored": stats.pairs_monitored,
                    "trades_executed": stats.trades_executed,
                    "total_profit": stats.total_profit,
                    "win_rate": stats.win_rate,
                },
                "timestamp": datetime.utcnow().isoformat()
            }, websocket)
        
        while True:
            try:
                # Wait for messages with timeout
                data = await asyncio.wait_for(
                    websocket.receive_text(),
                    timeout=30.0
                )
                
                message = json.loads(data)
                msg_type = message.get("type")
                
                if msg_type == "ping":
                    await manager.send_personal_message({
                        "type": "pong",
                        "timestamp": datetime.utcnow().isoformat()
                    }, websocket)
                
                elif msg_type == "get_status":
                    engine = get_engine()
                    if engine:
                        stats = engine.get_stats()
                        await manager.send_personal_message({
                            "type": "status",
                            "data": {
                                "is_running": stats.is_running,
                                "pairs_monitored": stats.pairs_monitored,
                                "currencies_tracked": stats.currencies_tracked,
                                "trades_executed": stats.trades_executed,
                                "total_profit": stats.total_profit,
                                "win_rate": stats.win_rate,
                                "uptime_seconds": stats.uptime_seconds,
                                "scan_cycle_ms": stats.scan_cycle_ms,
                            },
                            "timestamp": datetime.utcnow().isoformat()
                        }, websocket)
                
                elif msg_type == "get_slots":
                    engine = get_engine()
                    if engine:
                        slots = engine.get_slots()
                        await manager.send_personal_message({
                            "type": "slots",
                            "data": {
                                "slots": [
                                    {
                                        "id": s.id,
                                        "balance": s.balance,
                                        "status": s.status,
                                        "trades_count": s.trades_count,
                                        "wins_count": s.wins_count,
                                        "total_profit": s.total_profit,
                                    }
                                    for s in slots
                                ],
                                "total_balance": engine.get_total_balance(),
                                "total_profit": engine.get_total_profit(),
                            },
                            "timestamp": datetime.utcnow().isoformat()
                        }, websocket)
                    
            except asyncio.TimeoutError:
                # Send heartbeat
                await manager.send_personal_message({
                    "type": "heartbeat",
                    "timestamp": datetime.utcnow().isoformat()
                }, websocket)
                
    except WebSocketDisconnect:
        manager.disconnect(websocket)
    except Exception as e:
        logger.error(f"WebSocket error: {e}")
        manager.disconnect(websocket)


async def broadcast_trade(trade_result):
    """Broadcast trade result to all connected clients"""
    await manager.broadcast({
        "type": "trade",
        "data": {
            "slot_id": trade_result.slot_id,
            "path": trade_result.path,
            "profit_amount": trade_result.profit_amount,
            "status": trade_result.status,
        },
        "timestamp": datetime.utcnow().isoformat()
    })


async def broadcast_status():
    """Broadcast current status to all connected clients"""
    engine = get_engine()
    if engine:
        stats = engine.get_stats()
        await manager.broadcast({
            "type": "status",
            "data": {
                "is_running": stats.is_running,
                "trades_executed": stats.trades_executed,
                "total_profit": stats.total_profit,
                "win_rate": stats.win_rate,
            },
            "timestamp": datetime.utcnow().isoformat()
        })
