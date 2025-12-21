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

    Messages to client:
    - {"type": "status", "data": {...}}
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
                    "currencies_tracked": stats.currencies_tracked,
                    "opportunities_found": stats.opportunities_found,
                    "uptime_seconds": stats.uptime_seconds,
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
                                "opportunities_found": stats.opportunities_found,
                                "uptime_seconds": stats.uptime_seconds,
                                "scan_cycle_ms": stats.scan_cycle_ms,
                                "avg_orderbook_staleness_ms": stats.avg_orderbook_staleness_ms,
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
            "trade_id": trade_result.trade_id,
            "path": trade_result.path,
            "profit_loss": trade_result.profit_loss,
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
                "pairs_monitored": stats.pairs_monitored,
                "currencies_tracked": stats.currencies_tracked,
                "opportunities_found": stats.opportunities_found,
            },
            "timestamp": datetime.utcnow().isoformat()
        })
