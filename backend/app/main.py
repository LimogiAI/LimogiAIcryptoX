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
from app.core.database import init_db, close_db, AsyncSessionLocal
from app.api.routes import router as api_router
from app.api.websocket import router as ws_router

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

# Cached opportunities (updated by scan loop)
cached_opportunities = []
best_profit_today = 0.0


def get_engine():
    """Get the global engine instance"""
    global engine
    return engine


def get_cached_opportunities():
    """Get cached opportunities"""
    global cached_opportunities
    return cached_opportunities


def get_best_profit_today():
    """Get best profit today"""
    global best_profit_today
    return best_profit_today


@asynccontextmanager
async def lifespan(app: FastAPI):
    """Application lifespan manager"""
    global engine

    logger.info("Starting LimogiAICryptoX v2.0...")

    # Initialize database
    await init_db()
    logger.info("Database connected")

    # Initialize Kraken client for live trading
    live_kraken_client = None
    try:
        from app.core.kraken_client import KrakenClient, TradingMode
        from app.core.database import SessionLocal

        # Live trading client (with order permissions)
        if settings.kraken_live_api_key and settings.kraken_live_private_key:
            live_kraken_client = KrakenClient(
                api_key=settings.kraken_live_api_key,
                private_key=settings.kraken_live_private_key,
                mode=TradingMode.LIVE,
                max_loss_usd=settings.kraken_max_loss_usd,
            )

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

            # Start background scan loop (for opportunities detection only)
            scan_task = asyncio.create_task(run_scan_loop())
            logger.info("Scan loop started")

            # Start health snapshot loop (every 5 minutes)
            health_task = asyncio.create_task(run_health_snapshot_loop())
            logger.info("Health snapshot loop started")

            # Initialize and start live trading scanner
            if live_kraken_client:
                try:
                    from app.core.live_trading import initialize_live_scanner, start_live_scanner
                    live_scanner = initialize_live_scanner(engine, live_kraken_client)
                    start_live_scanner()
                    logger.info("Live Trading Scanner started")
                except Exception as e:
                    logger.error(f"Failed to start Live Trading Scanner: {e}")

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


async def run_scan_loop():
    """Background scan loop using Rust engine - detects opportunities only"""
    global engine, cached_opportunities, best_profit_today

    # Import runtime settings from routes
    from app.api.routes import get_runtime_settings

    if not engine:
        return

    default_base_currencies = settings.base_currencies
    interval_seconds = settings.scan_interval_ms / 1000.0

    # Separate counter for cache updates (less frequent)
    cache_update_counter = 0

    logger.info(f"Starting scan loop: {interval_seconds}s interval, default bases: {default_base_currencies}")

    while True:
        try:
            # Get runtime settings (can be changed via API)
            runtime = get_runtime_settings()

            # Determine which base currencies to use
            base_currency_setting = runtime.get("base_currency", "ALL")
            custom_currencies = runtime.get("custom_currencies", [])

            # Build the list of allowed base currencies
            if base_currency_setting == "ALL":
                trading_base_currencies = default_base_currencies
            elif base_currency_setting == "CUSTOM":
                trading_base_currencies = custom_currencies if custom_currencies else default_base_currencies
            else:
                trading_base_currencies = [base_currency_setting]

            # Check if scanner is enabled
            if not engine.is_scanner_enabled():
                await asyncio.sleep(interval_seconds)
                continue

            # Run scan cycle (detect opportunities)
            results = engine.run_cycle(trading_base_currencies)

            # Get min profit threshold from runtime settings
            min_threshold = runtime.get("min_profit_threshold", 0.0005)

            # Update cache every 5 cycles
            cache_update_counter += 1
            if cache_update_counter >= 5:
                cache_update_counter = 0
                try:
                    opportunities = engine.scan(default_base_currencies)
                    cached_opportunities = [
                        o for o in opportunities[:1000]
                        if o.net_profit_pct >= min_threshold * 100
                    ]

                    if opportunities:
                        max_profit = max(o.net_profit_pct for o in opportunities)
                        if max_profit > best_profit_today:
                            best_profit_today = max_profit

                        # Save top opportunities to history
                        async with AsyncSessionLocal() as db:
                            await save_opportunities_to_history(db, opportunities[:50])

                except Exception as e:
                    logger.error(f"Cache update error: {e}")

        except Exception as e:
            logger.error(f"Scan loop error: {e}")

        await asyncio.sleep(interval_seconds)


async def save_opportunities_to_history(db, opportunities):
    """Save detected opportunities to history for later review"""
    from app.models.models import OpportunityHistory
    from sqlalchemy import delete
    from datetime import datetime, timedelta

    saved_count = 0
    for opp in opportunities:
        try:
            path = opp.path
            legs = len(path.split('→')) - 1 if '→' in path else len(path.split(' → ')) - 1
            start_currency = path.split('→')[0].strip() if '→' in path else path.split(' → ')[0].strip()

            history_entry = OpportunityHistory(
                path=path,
                legs=legs,
                start_currency=start_currency,
                expected_profit_pct=opp.net_profit_pct,
                is_profitable=opp.is_profitable,
                was_traded=False,
            )
            db.add(history_entry)
            saved_count += 1
        except Exception as e:
            logger.error(f"Failed to save opportunity: {e}")

    try:
        await db.commit()
    except Exception as e:
        await db.rollback()
        logger.error(f"Failed to commit opportunities: {e}")

    # Cleanup: Delete opportunities older than 30 days
    try:
        cutoff = datetime.utcnow() - timedelta(days=30)
        result = await db.execute(
            delete(OpportunityHistory).where(OpportunityHistory.timestamp < cutoff)
        )
        if result.rowcount > 0:
            await db.commit()
            logger.info(f"Cleaned up {result.rowcount} old opportunities")
    except Exception as e:
        await db.rollback()
        logger.error(f"Failed to cleanup old opportunities: {e}")


async def run_health_snapshot_loop():
    """Background loop to save order book health snapshots every 5 minutes"""
    global engine

    if not engine:
        return

    # Wait 30 seconds for initial data to load
    await asyncio.sleep(30)

    logger.info("Starting health snapshot loop: 5 minute interval")

    while True:
        try:
            # Get current health stats
            health = engine.get_orderbook_health()

            # Save to database
            async with AsyncSessionLocal() as db:
                await save_health_snapshot(db, health)
                await cleanup_old_health_records(db)

        except Exception as e:
            logger.error(f"Health snapshot error: {e}")

        # Wait 5 minutes
        await asyncio.sleep(300)


async def save_health_snapshot(db, health):
    """Save health snapshot to database"""
    from app.models.models import OrderBookHealthHistory

    try:
        snapshot = OrderBookHealthHistory(
            total_pairs=health.total_pairs,
            valid_pairs=health.valid_pairs,
            valid_pct=round(health.valid_pairs / max(health.total_pairs, 1) * 100, 1),
            skipped_no_orderbook=health.skipped_no_orderbook,
            skipped_thin_depth=health.skipped_thin_depth,
            skipped_stale=health.skipped_stale,
            skipped_bad_spread=health.skipped_bad_spread,
            skipped_no_price=health.skipped_no_price,
            skipped_total=health.skipped_no_orderbook + health.skipped_thin_depth + health.skipped_stale + health.skipped_bad_spread + health.skipped_no_price,
            avg_freshness_ms=health.avg_freshness_ms,
            avg_spread_pct=health.avg_spread_pct,
            avg_depth=health.avg_depth,
            rejected_opportunities=health.rejected_opportunities,
        )
        db.add(snapshot)
        await db.commit()
        logger.debug(f"Saved health snapshot: {health.valid_pairs}/{health.total_pairs} valid pairs")
    except Exception as e:
        await db.rollback()
        logger.error(f"Failed to save health snapshot: {e}")


async def cleanup_old_health_records(db):
    """Delete health records older than 30 days"""
    from app.models.models import OrderBookHealthHistory
    from sqlalchemy import delete
    from datetime import datetime, timedelta

    try:
        cutoff = datetime.utcnow() - timedelta(days=30)
        result = await db.execute(
            delete(OrderBookHealthHistory).where(OrderBookHealthHistory.timestamp < cutoff)
        )
        if result.rowcount > 0:
            await db.commit()
            logger.info(f"Cleaned up {result.rowcount} old health records")
    except Exception as e:
        await db.rollback()
        logger.error(f"Failed to cleanup old health records: {e}")


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

# Include routers
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
    global engine

    if engine:
        stats = engine.get_stats()
        return {
            "status": "healthy",
            "engine": "rust_v2",
            "is_running": stats.is_running,
            "pairs_loaded": stats.pairs_monitored,
            "currencies": stats.currencies_tracked,
            "uptime_seconds": stats.uptime_seconds,
            "avg_staleness_ms": f"{stats.avg_orderbook_staleness_ms:.1f}",
        }
    else:
        return {
            "status": "degraded",
            "engine": "none",
            "reason": "Rust engine not available",
        }
