"""
KrakenCryptoX v2.0 - Single Balance Pool Paper Trading
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
    logger.info("✅ Rust trading engine loaded")
except ImportError as e:
    RUST_ENGINE_AVAILABLE = False
    logger.warning(f"⚠️ Rust engine not available: {e}")
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
    
    logger.info("🚀 Starting KrakenCryptoX v2.0...")
    
    # Initialize database
    await init_db()
    logger.info("✅ Database connected")
    
    # Initialize Kraken client for shadow mode
    try:
        from app.core.kraken_client import initialize_kraken_client, get_kraken_client, KrakenClient
        from app.core.shadow_executor import initialize_shadow_executor
        from app.core.database import SessionLocal
        
        # Shadow mode client (read-only, for paper trading)
        if settings.kraken_api_key and settings.kraken_private_key:
            kraken_client = initialize_kraken_client(
                api_key=settings.kraken_api_key,
                private_key=settings.kraken_private_key,
                shadow_mode=True,  # Always shadow mode for this client
                max_loss_usd=settings.kraken_max_loss_usd,
            )
            
            # Initialize shadow executor with database session factory
            shadow_executor = initialize_shadow_executor(kraken_client, SessionLocal)
            
            # Test connection
            try:
                balance = await kraken_client.get_balance()
                usd_balance = balance.get("ZUSD", 0) + balance.get("USD", 0)
                logger.info(f"✅ Kraken Shadow API connected (Balance: ${usd_balance:.2f})")
            except Exception as e:
                logger.warning(f"⚠️ Kraken Shadow API connection test failed: {e}")
        else:
            logger.warning("⚠️ Kraken Shadow API credentials not configured (.env.kraken)")
        
        # Live trading client (with order permissions)
        if settings.kraken_live_api_key and settings.kraken_live_private_key:
            from app.core.kraken_client import TradingMode
            live_kraken_client = KrakenClient(
                api_key=settings.kraken_live_api_key,
                private_key=settings.kraken_live_private_key,
                mode=TradingMode.LIVE,  # Real trading
                max_loss_usd=settings.kraken_max_loss_usd,
            )
            
            # Initialize live trading manager with live client
            try:
                from app.core.live_trading import initialize_live_trading
                live_trading_manager = initialize_live_trading(live_kraken_client, SessionLocal)
                logger.info("✅ Live Trading Manager initialized with LIVE API keys")
                
                # Test live connection
                try:
                    balance = await live_kraken_client.get_balance()
                    usd_balance = balance.get("ZUSD", 0) + balance.get("USD", 0)
                    logger.info(f"✅ Kraken Live API connected (Balance: ${usd_balance:.2f})")
                    logger.info(f"💰 Max loss limit: ${settings.kraken_max_loss_usd}")
                except Exception as e:
                    logger.warning(f"⚠️ Kraken Live API connection test failed: {e}")
            except Exception as e:
                logger.error(f"❌ Failed to initialize Live Trading Manager: {e}")
        else:
            logger.warning("⚠️ Kraken Live API credentials not configured (.env.live) - live trading unavailable")
            # Initialize live trading with shadow client as fallback (won't work for real trades)
            if settings.kraken_api_key and settings.kraken_private_key:
                try:
                    from app.core.live_trading import initialize_live_trading
                    live_trading_manager = initialize_live_trading(kraken_client, SessionLocal)
                    logger.info("⚠️ Live Trading Manager initialized with SHADOW API keys (live trading will fail)")
                except Exception as e:
                    logger.error(f"❌ Failed to initialize Live Trading Manager: {e}")
            
    except Exception as e:
        logger.error(f"❌ Failed to initialize Kraken client: {e}")
    
    # Initialize Rust engine
    if RUST_ENGINE_AVAILABLE:
        try:
            engine = TradingEngine(
                initial_balance=settings.total_capital,
                trade_amount=10.0,  # Default, will be updated from UI
                min_profit_threshold=settings.min_profit_threshold,
                cooldown_ms=0,   # No cooldown by default
                max_trades_per_cycle=5,  # Default
                fee_rate=settings.fee_rate_taker,
                max_pairs=settings.max_pairs,
            )
            
            # Initialize (fetch pairs and prices)
            engine.initialize()
            logger.info("✅ Rust engine initialized")
            
            # Start WebSocket streaming
            engine.start_websocket()
            logger.info("✅ WebSocket streaming started")
            
            # Start background scan loop
            scan_task = asyncio.create_task(run_scan_loop())
            logger.info("✅ Scan loop started")
            
            # Start health snapshot loop (every 5 minutes)
            health_task = asyncio.create_task(run_health_snapshot_loop())
            logger.info("✅ Health snapshot loop started")
            
            # Initialize and start live trading scanner (independent from paper trading)
            try:
                from app.core.live_trading import initialize_live_scanner, start_live_scanner
                live_scanner = initialize_live_scanner(engine, live_kraken_client if 'live_kraken_client' in dir() else kraken_client)
                start_live_scanner()
                logger.info("✅ Live Trading Scanner started")
            except Exception as e:
                logger.error(f"❌ Failed to start Live Trading Scanner: {e}")
            
        except Exception as e:
            logger.error(f"❌ Failed to initialize Rust engine: {e}")
            engine = None
    else:
        logger.warning("⚠️ Running without Rust engine")
    
    # Log startup info
    logger.info("=" * 50)
    logger.info("🎯 KrakenCryptoX v2.0 is ready!")
    if engine:
        stats = engine.get_stats()
        state = engine.get_trading_state()
        logger.info(f"📊 Monitoring {stats.pairs_monitored} pairs")
        logger.info(f"💰 Balance: ${state.balance:.2f}")
        logger.info(f"⚡ Single balance pool (no slots)")
    logger.info(f"💰 Fee rate: {settings.fee_rate_taker * 100:.2f}%")
    logger.info(f"🎚️ Min profit threshold: {settings.min_profit_threshold * 100:.3f}%")
    logger.info("=" * 50)
    
    yield
    
    # Shutdown
    logger.info("Shutting down KrakenCryptoX v2.0...")
    
    if engine:
        try:
            engine.stop_websocket()
        except Exception as e:
            logger.error(f"Error stopping WebSocket: {e}")
    
    # Close Kraken client
    try:
        from app.core.kraken_client import get_kraken_client
        kraken_client = get_kraken_client()
        if kraken_client:
            await kraken_client.close()
    except Exception as e:
        logger.error(f"Error closing Kraken client: {e}")
    
    await close_db()
    
    logger.info("👋 KrakenCryptoX v2.0 stopped")


async def run_scan_loop():
    """Background scan loop using Rust engine"""
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
            
            # Determine which base currencies to use for trading
            base_currency_setting = runtime.get("base_currency", "ALL")
            custom_currencies = runtime.get("custom_currencies", [])
            
            # Build the list of allowed base currencies for trading
            if base_currency_setting == "ALL":
                trading_base_currencies = default_base_currencies
            elif base_currency_setting == "CUSTOM":
                trading_base_currencies = custom_currencies if custom_currencies else default_base_currencies
            else:
                # Single currency selected (USD, EUR, BTC, etc.)
                trading_base_currencies = [base_currency_setting]
            
            # Check if scanner is enabled (from Rust engine)
            if not engine.is_scanner_enabled():
                await asyncio.sleep(interval_seconds)
                continue
            
            # Skip if trading is disabled
            if not runtime.get("is_active", True):
                await asyncio.sleep(interval_seconds)
                continue
            
            # Run dispatch cycle (scan + execute) with filtered base currencies
            results = engine.run_cycle(trading_base_currencies)
            
            # Filter results by min profit threshold from runtime settings
            min_threshold = runtime.get("min_profit_threshold", 0.0005)
            
            # Update cache every 5 cycles - scan ALL paths for Opportunities tab
            cache_update_counter += 1
            if cache_update_counter >= 5:
                cache_update_counter = 0
                try:
                    # Scan with default_base_currencies (ALL) for Opportunities display
                    opportunities = engine.scan(default_base_currencies)
                    # Filter by runtime min profit threshold
                    cached_opportunities = [
                        o for o in opportunities[:1000] 
                        if o.net_profit_pct >= min_threshold * 100
                    ]
                    
                    if opportunities:
                        max_profit = max(o.net_profit_pct for o in opportunities)
                        if max_profit > best_profit_today:
                            best_profit_today = max_profit
                        
                        # Save top opportunities to history (for review later)
                        async with AsyncSessionLocal() as db:
                            await save_opportunities_to_history(db, opportunities[:50])
                            
                except Exception as e:
                    logger.error(f"Cache update error: {e}")
            
            if results:
                # Log trades
                for trade in results:
                    logger.info(
                        f"📍 Trade #{trade.trade_id} | {trade.path} | "
                        f"${trade.trade_amount:.2f} | "
                        f"Expected: {trade.expected_profit_pct:.4f}% | "
                        f"Slippage: {trade.slippage_pct:.4f}% | "
                        f"Actual: {trade.actual_profit_pct:.4f}% | "
                        f"${trade.profit_amount:.4f} | {trade.status}"
                    )
                
                # Execute shadow trades to compare with live Kraken prices
                try:
                    from app.core.shadow_executor import get_shadow_executor
                    shadow_executor = get_shadow_executor()
                    if shadow_executor:
                        for trade in results:
                            shadow_result = await shadow_executor.execute_shadow(
                                path=trade.path,
                                trade_amount_usd=trade.trade_amount,
                                paper_expected_profit_pct=trade.expected_profit_pct,
                                paper_slippage_pct=trade.slippage_pct,
                            )
                            logger.info(
                                f"🔍 Shadow #{trade.trade_id} | {trade.path} | "
                                f"Paper: {trade.actual_profit_pct:.4f}% | "
                                f"Shadow: {shadow_result.shadow_profit_pct:.4f}% | "
                                f"Latency: {shadow_result.latency_ms:.0f}ms | "
                                f"Would profit: {shadow_result.would_have_profited}"
                            )
                except Exception as e:
                    logger.error(f"Shadow execution error: {e}")
                
                # Save to database
                async with AsyncSessionLocal() as db:
                    await save_trades_to_db(db, results)
            
        except Exception as e:
            logger.error(f"Scan loop error: {e}")
        
        await asyncio.sleep(interval_seconds)


async def save_trades_to_db(db, trades):
    """Save trade results to database"""
    from app.models.models import PaperTrade, OpportunityHistory
    from sqlalchemy import select, delete, update
    from datetime import datetime, timedelta
    
    for trade in trades:
        try:
            paper_trade = PaperTrade(
                slot_id=0,  # Single pool, always slot 0
                opportunity_id=None,
                path=trade.path,
                legs=len(trade.path.split('→')) - 1,
                trade_amount=trade.trade_amount,
                gross_profit_pct=trade.expected_profit_pct + trade.slippage_pct,
                fees_pct=0.26 * (len(trade.path.split('→')) - 1),
                expected_net_profit_pct=trade.expected_profit_pct,
                slippage_pct=trade.slippage_pct,
                slippage_details=trade.slippage_details,
                actual_net_profit_pct=trade.actual_profit_pct,
                actual_profit_amount=trade.profit_amount,
                balance_before=trade.balance_before,
                balance_after=trade.balance_after,
                status=trade.status,
            )
            db.add(paper_trade)
            await db.commit()
            
            # Mark matching opportunity as traded (most recent one with same path)
            try:
                # Find the most recent opportunity with this path from last 5 minutes
                cutoff = datetime.utcnow() - timedelta(minutes=5)
                result = await db.execute(
                    select(OpportunityHistory)
                    .where(OpportunityHistory.path == trade.path)
                    .where(OpportunityHistory.timestamp >= cutoff)
                    .where(OpportunityHistory.was_traded == False)
                    .order_by(OpportunityHistory.timestamp.desc())
                    .limit(1)
                )
                opp_to_update = result.scalar_one_or_none()
                
                if opp_to_update:
                    opp_to_update.was_traded = True
                    opp_to_update.trade_id = paper_trade.id
                    opp_to_update.actual_profit_pct = trade.actual_profit_pct
                    opp_to_update.slippage_pct = trade.slippage_pct
                    await db.commit()
            except Exception as e:
                logger.error(f"Failed to link opportunity to trade: {e}")
                
        except Exception as e:
            await db.rollback()
            logger.error(f"Failed to save trade: {e}")
    
    # Keep only last 500 trades
    try:
        result = await db.execute(
            select(PaperTrade.id).order_by(PaperTrade.id.desc()).offset(500)
        )
        old_ids = [row[0] for row in result.fetchall()]
        if old_ids:
            await db.execute(
                delete(PaperTrade).where(PaperTrade.id.in_(old_ids))
            )
            await db.commit()
    except Exception as e:
        await db.rollback()
        logger.error(f"Failed to cleanup old trades: {e}")


async def save_opportunities_to_history(db, opportunities):
    """Save detected opportunities to history for later review"""
    from app.models.models import OpportunityHistory
    from sqlalchemy import delete
    from datetime import datetime, timedelta
    
    saved_count = 0
    for opp in opportunities:
        try:
            # Extract path info
            path = opp.path
            legs = len(path.split('→')) - 1 if '→' in path else len(path.split(' → ')) - 1
            start_currency = path.split('→')[0].strip() if '→' in path else path.split(' → ')[0].strip()
            
            history_entry = OpportunityHistory(
                path=path,
                legs=legs,
                start_currency=start_currency,
                expected_profit_pct=opp.net_profit_pct,
                is_profitable=opp.is_profitable,
                was_traded=False,  # Will be updated if traded
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
                
                # Cleanup old records (keep 30 days)
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
    title="KrakenCryptoX v2.0",
    description="Multi-Pair Arbitrage Scanner with Single Balance Pool",
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
        state = engine.get_trading_state()
        return {
            "name": "KrakenCryptoX",
            "version": "2.0.0",
            "engine": "Rust (Single Balance Pool)",
            "status": "running" if stats.is_running else "stopped",
            "pairs_monitored": stats.pairs_monitored,
            "balance": f"${state.balance:.2f}",
            "trades_executed": stats.trades_executed,
            "total_profit": f"${stats.total_profit:.2f}",
            "win_rate": f"{stats.win_rate:.1f}%",
            "docs": "/docs",
        }
    else:
        return {
            "name": "KrakenCryptoX",
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
        state = engine.get_trading_state()
        return {
            "status": "healthy",
            "engine": "rust_v2",
            "is_running": stats.is_running,
            "pairs_loaded": stats.pairs_monitored,
            "currencies": stats.currencies_tracked,
            "uptime_seconds": stats.uptime_seconds,
            "balance": state.balance,
            "avg_staleness_ms": f"{stats.avg_orderbook_staleness_ms:.1f}",
        }
    else:
        return {
            "status": "degraded",
            "engine": "none",
            "reason": "Rust engine not available",
        }
