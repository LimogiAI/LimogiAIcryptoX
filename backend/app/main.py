"""
LimogiAICryptoX v2.0 - Live Trading Platform
Main FastAPI Application
"""
import asyncio
from contextlib import asynccontextmanager
from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from loguru import logger
import sys

from app.core.config import settings
from app.core.database import init_db, close_db, SessionLocal

# Import Rust engine
try:
    from trading_engine import TradingEngine
    RUST_ENGINE_AVAILABLE = True
    logger.info("Rust trading engine loaded")
except ImportError as e:
    RUST_ENGINE_AVAILABLE = False
    logger.warning(f"Rust engine not available: {e}")
    TradingEngine = None

# Configure logging
logger.remove()
logger.add(
    sys.stdout,
    format="<green>{time:YYYY-MM-DD HH:mm:ss}</green> | <level>{level: <8}</level> | <cyan>{name}</cyan>:<cyan>{function}</cyan>:<cyan>{line}</cyan> - <level>{message}</level>",
    level=settings.log_level,
)

# Global engine instance
engine = None

# Global scanner instance (for accessing cached data)
scanner = None


def get_engine():
    """Get the global engine instance"""
    global engine
    return engine


def get_cached_opportunities():
    """Get cached opportunities from the unified scanner"""
    global scanner
    if scanner:
        return scanner.get_cached_opportunities()
    return []


def get_best_profit_today():
    """Get best profit today from the unified scanner"""
    global scanner
    if scanner:
        return scanner.get_best_profit_today()
    return 0.0


@asynccontextmanager
async def lifespan(app: FastAPI):
    """Application lifespan manager"""
    global engine, scanner

    logger.info("Starting LimogiAICryptoX v2.0...")

    # Initialize database
    await init_db()
    logger.info("Database connected")

    # Initialize Kraken client for live trading
    live_kraken_client = None
    try:
        from app.core.kraken_client import KrakenClient, TradingMode

        # Live trading client (with order permissions)
        # Use effective keys (supports both KRAKEN_API_KEY and KRAKEN_LIVE_API_KEY)
        api_key = settings.effective_api_key
        api_secret = settings.effective_api_secret

        if api_key and api_secret:
            live_kraken_client = KrakenClient(
                api_key=api_key,
                private_key=api_secret,
                mode=TradingMode.LIVE,
                max_loss_usd=settings.kraken_max_loss_usd,
            )
            logger.info(f"Using Kraken API key: {api_key[:8]}...")

            # Initialize live trading manager
            try:
                from app.core.live_trading import initialize_live_trading
                live_trading_manager = initialize_live_trading(live_kraken_client, SessionLocal)
                logger.info("Live Trading Manager initialized")

                # Test live connection
                try:
                    balance = await live_kraken_client.get_balance()
                    usd_balance = balance.get("ZUSD", 0) + balance.get("USD", 0)
                    logger.info(f"Kraken Live API connected (Balance: ${usd_balance:.2f})")
                    logger.info(f"Max loss limit: ${settings.kraken_max_loss_usd}")
                except Exception as e:
                    logger.warning(f"Kraken Live API connection test failed: {e}")
            except Exception as e:
                logger.error(f"Failed to initialize Live Trading Manager: {e}")
        else:
            logger.warning("Kraken Live API credentials not configured (.env.live) - live trading unavailable")

    except Exception as e:
        logger.error(f"Failed to initialize Kraken client: {e}")

    # Initialize Rust engine
    if RUST_ENGINE_AVAILABLE:
        try:
            engine = TradingEngine(
                min_profit_threshold=settings.min_profit_threshold,
                fee_rate=settings.fee_rate_taker,
                max_pairs=settings.max_pairs,
            )

            # Initialize (fetch pairs and prices)
            engine.initialize()
            logger.info("Rust engine initialized")

            # Start WebSocket streaming
            engine.start_websocket()
            logger.info("WebSocket streaming started")

            # Auto-initialize Rust execution engine if API credentials available
            if api_key and api_secret:
                try:
                    engine.init_execution_engine(api_key, api_secret)
                    logger.info("Rust execution engine initialized with API credentials")

                    # Auto-connect to private WebSocket
                    engine.connect_execution_engine()
                    if engine.is_execution_engine_connected():
                        logger.info("Rust execution engine connected to Kraken private WebSocket")

                        # Setup auto-execution pipeline (wires trading guard + execution engine to scanner)
                        engine.setup_auto_execution_pipeline()
                        logger.info("Auto-execution pipeline configured (will activate when trading enabled)")
                    else:
                        logger.warning("Rust execution engine failed to connect to private WebSocket")
                except Exception as e:
                    logger.warning(f"Failed to auto-initialize Rust execution engine: {e}")

            # Initialize and start the UI Cache Manager (fetches from Rust for UI display)
            # NOTE: All scanning happens in Rust - this just fetches cached data for UI
            if live_kraken_client:
                try:
                    from app.core.live_trading.ui_cache import initialize_ui_cache, start_ui_cache
                    from app.core.live_trading import get_live_trading_manager

                    ui_cache = initialize_ui_cache(engine, live_kraken_client, SessionLocal)

                    # Sync trading config from Python to Rust
                    # IMPORTANT: Always start with trading DISABLED for safety
                    # User must explicitly click "Start Trading" each session
                    manager = get_live_trading_manager()
                    if manager:
                        py_config = manager.get_config()
                        engine.update_trading_config(
                            enabled=False,  # ALWAYS start disabled - user must click "Start Trading"
                            trade_amount=py_config.trade_amount,
                            min_profit_threshold=py_config.min_profit_threshold * 100,  # Convert to percentage
                            max_daily_loss=py_config.max_daily_loss,
                            max_total_loss=py_config.max_total_loss,
                            base_currency=py_config.base_currency,
                            execution_mode=py_config.execution_mode,
                        )
                        # Also ensure Python config is disabled on startup
                        if py_config.is_enabled:
                            manager.disable("Server restart - requires re-enable")
                        logger.info("Trading config synced to Rust (DISABLED on startup for safety)")

                    start_ui_cache()
                    logger.info("UI Cache Manager started (fetches from Rust engine for UI)")
                except Exception as e:
                    logger.error(f"Failed to start UI Cache Manager: {e}")

        except Exception as e:
            logger.error(f"Failed to initialize Rust engine: {e}")
            engine = None
    else:
        logger.warning("Running without Rust engine")

    # Log startup info
    logger.info("=" * 50)
    logger.info("LimogiAICryptoX v2.0 is ready!")
    if engine:
        stats = engine.get_stats()
        logger.info(f"Monitoring {stats.pairs_monitored} pairs")
    logger.info(f"Fee rate: {settings.fee_rate_taker * 100:.2f}%")
    logger.info(f"Min profit threshold: {settings.min_profit_threshold * 100:.3f}%")
    logger.info("=" * 50)

    yield

    # Shutdown
    logger.info("Shutting down LimogiAICryptoX v2.0...")

    # Stop UI cache manager
    if ui_cache:
        try:
            from app.core.live_trading.ui_cache import stop_ui_cache
            stop_ui_cache()
        except Exception as e:
            logger.error(f"Error stopping UI cache manager: {e}")

    if engine:
        try:
            engine.stop_websocket()
        except Exception as e:
            logger.error(f"Error stopping WebSocket: {e}")

    # Close Kraken client
    if live_kraken_client:
        try:
            await live_kraken_client.close()
        except Exception as e:
            logger.error(f"Error closing Kraken client: {e}")

    await close_db()

    logger.info("LimogiAICryptoX v2.0 stopped")


# Create FastAPI app
app = FastAPI(
    title="LimogiAICryptoX v2.0",
    description="Live Crypto Arbitrage Trading Platform",
    version="2.0.0",
    lifespan=lifespan,
)

# CORS middleware
app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

# Import and include routers
from app.api.routes import router as api_router
from app.api.websocket import router as ws_router

app.include_router(api_router, prefix="/api", tags=["API"])
app.include_router(ws_router, tags=["WebSocket"])


@app.get("/")
async def root():
    """Root endpoint"""
    global engine

    if engine:
        stats = engine.get_stats()
        return {
            "name": "LimogiAICryptoX",
            "version": "2.0.0",
            "engine": "Rust",
            "status": "running" if stats.is_running else "stopped",
            "pairs_monitored": stats.pairs_monitored,
            "opportunities_found": stats.opportunities_found,
            "docs": "/docs",
        }
    else:
        return {
            "name": "LimogiAICryptoX",
            "version": "2.0.0",
            "engine": "Not available",
            "status": "limited",
            "docs": "/docs",
        }


@app.get("/health")
async def health_check():
    """Health check endpoint"""
    global engine, scanner

    if engine:
        stats = engine.get_stats()
        scanner_status = scanner.get_status() if scanner else {}
        return {
            "status": "healthy",
            "engine": "rust_v2",
            "is_running": stats.is_running,
            "pairs_loaded": stats.pairs_monitored,
            "currencies": stats.currencies_tracked,
            "uptime_seconds": stats.uptime_seconds,
            "avg_staleness_ms": f"{stats.avg_orderbook_staleness_ms:.1f}",
            "scanner": {
                "is_running": scanner_status.get('is_running', False),
                "scans_completed": scanner_status.get('scans_completed', 0),
                "cached_opportunities": scanner_status.get('cached_opportunities_count', 0),
            }
        }
    else:
        return {
            "status": "degraded",
            "engine": "none",
            "reason": "Rust engine not available",
        }
