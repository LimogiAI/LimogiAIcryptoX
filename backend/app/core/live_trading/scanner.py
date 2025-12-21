"""
Unified Scanner for LimogiAICryptoX

Single scanner that handles:
- Fetching cached opportunities from Rust event-driven scanner
- Order book health tracking
- Opportunity caching for UI
- Opportunity history saving

NOTE: All scanning AND execution happens in Rust (auto-execution).
Python only handles:
- Database logging (receives trade results via callback)
- UI caching (fetches opportunities from Rust cache)
- Health snapshots

When user clicks "Start Trading":
1. Python enables trading in Rust
2. Rust auto-executes profitable opportunities immediately on detection
3. Rust sends trade results back to Python via callback for DB logging
"""
import asyncio
from typing import Optional, List, Dict, Any
from datetime import datetime, timedelta
from loguru import logger

from .manager import get_live_trading_manager


class UnifiedScanner:
    """
    Single scanner for all scanning operations.

    Features:
    - Fetches cached opportunities from Rust for UI display
    - Records order book health snapshots
    - Saves opportunity history to database

    NOTE: Execution is handled entirely by Rust auto-execution.
    Python does NOT poll for execution - it only:
    1. Caches opportunities for UI
    2. Saves health snapshots
    3. Saves opportunity history
    """

    def __init__(self, engine, kraken_client, db_session_factory):
        self.engine = engine
        self.kraken_client = kraken_client
        self.db_session_factory = db_session_factory
        self._running = False
        self._task: Optional[asyncio.Task] = None
        # Rust engine does event-driven scanning AND auto-execution
        # Python just fetches cached results for UI display
        self._check_interval = 1.0  # Check for UI cache updates every 1s (not critical)

        # Cached data for UI
        self.cached_opportunities: List[Any] = []
        self.best_profit_today: float = 0.0

        # Stats
        self._last_scan_at: Optional[datetime] = None
        self._scans_completed = 0
        self._last_health_snapshot_at: Optional[datetime] = None

    @property
    def is_running(self) -> bool:
        return self._running

    def start(self):
        """Start the scanner background task"""
        if self._running:
            logger.warning("Scanner already running")
            return

        self._running = True
        self._task = asyncio.create_task(self._scan_loop())
        logger.info("🔍 Unified Scanner started")

        self._update_scanner_status(is_running=True)

    def stop(self):
        """Stop the scanner"""
        self._running = False
        if self._task:
            self._task.cancel()
            self._task = None
        logger.info("🔍 Unified Scanner stopped")

        self._update_scanner_status(is_running=False)

    async def _scan_loop(self):
        """Main loop - fetches cached opportunities from Rust event-driven scanner"""
        logger.info(f"Scanner loop starting: {self._check_interval}s interval (Rust does event-driven scanning)")

        # Wait for initial data to load
        await asyncio.sleep(5)

        while self._running:
            try:
                await self._process_opportunities()
            except asyncio.CancelledError:
                logger.info("Scanner cancelled")
                break
            except Exception as e:
                logger.error(f"Scanner error: {e}")
                self._update_scanner_status(last_error=str(e))

            await asyncio.sleep(self._check_interval)

    async def _process_opportunities(self):
        """
        Process cached opportunities from Rust event-driven scanner.

        NOTE: This does NOT execute trades - Rust handles auto-execution.
        This only:
        1. Caches opportunities for UI display
        2. Updates scanner status
        3. Saves opportunity history periodically
        4. Saves health snapshots periodically
        """
        start_time = datetime.utcnow()

        manager = get_live_trading_manager()
        if not manager:
            return

        # Get live trading config for settings
        config = manager.get_config()

        # Check if scanner is enabled in engine
        if not self.engine.is_scanner_enabled():
            return

        # Get cached opportunities from Rust (no new scan triggered)
        # Rust scanner updates cache on every WebSocket order book update
        try:
            opportunities, cache_age_ms = self.engine.get_cached_opportunities_with_age()
        except Exception as e:
            logger.error(f"Error getting cached opportunities: {e}")
            self._update_scanner_status(last_error=str(e))
            return

        # Skip if cache is too old (> 5 seconds means something is wrong)
        if cache_age_ms > 5000:
            return

        # Get engine stats
        stats = self.engine.get_stats()
        pairs_scanned = stats.pairs_monitored
        total_paths = len(opportunities) if opportunities else 0

        # Filter by profit threshold
        min_threshold_pct = config.min_profit_threshold * 100
        profitable = [o for o in opportunities if o.net_profit_pct >= min_threshold_pct]
        profitable_count = len(profitable)

        # Calculate scan duration
        scan_duration_ms = (datetime.utcnow() - start_time).total_seconds() * 1000

        # Update scanner status in database
        # paths_found = total cyclic paths discovered by DFS
        # opportunities_found = paths above profit threshold (potentially tradeable)
        self._update_scanner_status(
            is_running=self._running,
            pairs_scanned=pairs_scanned,
            paths_found=total_paths,
            opportunities_found=profitable_count,  # Only count profitable ones
            profitable_count=profitable_count,
            scan_duration_ms=scan_duration_ms,
        )

        self._last_scan_at = datetime.utcnow()
        self._scans_completed += 1

        # === CACHE OPPORTUNITIES FOR UI (every cycle) ===
        if opportunities:
            self.cached_opportunities = [
                o for o in opportunities[:1000]
                if o.net_profit_pct >= min_threshold_pct
            ]
            max_profit = max(o.net_profit_pct for o in opportunities)
            if max_profit > self.best_profit_today:
                self.best_profit_today = max_profit

        # === SAVE OPPORTUNITY HISTORY (every 30 cycles = ~30 seconds at 1s interval) ===
        if self._scans_completed % 30 == 0 and opportunities:
            await self._save_opportunities_to_history(opportunities[:50])

        # === HEALTH SNAPSHOT (every 300 cycles = ~5 minutes at 1s interval) ===
        if self._scans_completed % 300 == 0:
            await self._save_health_snapshot()

        # Log results periodically (every 60 cycles = ~1 minute at 1s interval)
        if self._scans_completed % 60 == 1:
            # Get auto-execution stats from Rust
            try:
                auto_execs, auto_successes = self.engine.get_auto_execution_stats()
                auto_exec_info = f" | Auto-exec: {auto_execs} ({auto_successes} success)"
            except Exception:
                auto_exec_info = ""

            logger.info(
                f"🔍 Scan: {total_paths} paths, {profitable_count} profitable (>{min_threshold_pct:.2f}%) "
                f"(cache age: {cache_age_ms}ms){auto_exec_info}"
            )

        # NOTE: Execution is handled by Rust auto-execution
        # No Python execution loop needed anymore!

    def _save_rust_trade_result(
        self,
        trade_id: str,
        path: str,
        profit_amount: float,
        profit_pct: float,
    ):
        """Save a Rust-executed trade to the database"""
        from app.models.live_trading import LiveTrade

        try:
            db = self.db_session_factory()

            # Get trade config from Rust
            rust_config = self.engine.get_trading_config()
            trade_amount = rust_config[1]  # trade_amount is index 1

            trade = LiveTrade(
                trade_id=trade_id,
                path=path,
                legs=path.count('→'),
                amount_in=trade_amount,
                amount_out=trade_amount + profit_amount,
                profit_loss=profit_amount,
                profit_loss_pct=profit_pct,
                status='COMPLETED',
                started_at=datetime.utcnow(),
                completed_at=datetime.utcnow(),
            )

            db.add(trade)
            db.commit()

            logger.debug(f"Saved Rust trade to database: {trade_id}")

        except Exception as e:
            logger.error(f"Error saving Rust trade result: {e}")
            db.rollback()
        finally:
            db.close()

    async def _save_opportunities_to_history(self, opportunities):
        """Save detected opportunities to history"""
        from app.models.models import OpportunityHistory
        from sqlalchemy import delete

        try:
            db = self.db_session_factory()
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

            db.commit()

            # Cleanup old records (30 days)
            cutoff = datetime.utcnow() - timedelta(days=30)
            db.execute(
                delete(OpportunityHistory).where(OpportunityHistory.timestamp < cutoff)
            )
            db.commit()

        except Exception as e:
            logger.error(f"Failed to save opportunities: {e}")
            db.rollback()
        finally:
            db.close()

    async def _save_health_snapshot(self):
        """Save order book health snapshot"""
        from app.models.models import OrderBookHealthHistory
        from sqlalchemy import delete

        try:
            health = self.engine.get_orderbook_health()
            db = self.db_session_factory()

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
            db.commit()

            # Cleanup old records (30 days)
            cutoff = datetime.utcnow() - timedelta(days=30)
            result = db.execute(
                delete(OrderBookHealthHistory).where(OrderBookHealthHistory.timestamp < cutoff)
            )
            if result.rowcount > 0:
                db.commit()
                logger.debug(f"Cleaned up {result.rowcount} old health records")

            self._last_health_snapshot_at = datetime.utcnow()
            logger.debug(f"Health snapshot: {health.valid_pairs}/{health.total_pairs} valid pairs")

        except Exception as e:
            logger.error(f"Failed to save health snapshot: {e}")
            db.rollback()
        finally:
            db.close()

    def _update_scanner_status(
        self,
        is_running: bool = None,
        pairs_scanned: int = None,
        paths_found: int = None,
        opportunities_found: int = None,
        profitable_count: int = None,
        scan_duration_ms: float = None,
        last_error: str = None,
    ):
        """Update scanner status in database"""
        manager = get_live_trading_manager()
        if manager:
            manager.update_scanner_status(
                is_running=is_running if is_running is not None else self._running,
                pairs_scanned=pairs_scanned,
                paths_found=paths_found,
                opportunities_found=opportunities_found,
                profitable_count=profitable_count,
                scan_duration_ms=scan_duration_ms,
                last_error=last_error,
            )

    def get_cached_opportunities(self) -> List[Any]:
        """Get cached opportunities for UI"""
        return self.cached_opportunities

    def get_best_profit_today(self) -> float:
        """Get best profit seen today"""
        return self.best_profit_today

    def reset_daily_stats(self):
        """Reset daily statistics"""
        self.best_profit_today = 0.0

    def get_status(self) -> Dict[str, Any]:
        """Get current scanner status"""
        # Get auto-execution stats from Rust if available
        auto_execs, auto_successes = 0, 0
        try:
            auto_execs, auto_successes = self.engine.get_auto_execution_stats()
        except Exception:
            pass

        return {
            'is_running': self._running,
            'check_interval': self._check_interval,
            'scans_completed': self._scans_completed,
            'last_scan_at': self._last_scan_at.isoformat() if self._last_scan_at else None,
            'last_health_snapshot_at': self._last_health_snapshot_at.isoformat() if self._last_health_snapshot_at else None,
            'cached_opportunities_count': len(self.cached_opportunities),
            'best_profit_today': self.best_profit_today,
            'auto_executions': auto_execs,
            'auto_execution_successes': auto_successes,
        }


# Singleton instance
_scanner: Optional[UnifiedScanner] = None


def get_scanner() -> Optional[UnifiedScanner]:
    """Get the global scanner instance"""
    return _scanner


def initialize_scanner(engine, kraken_client, db_session_factory) -> UnifiedScanner:
    """Initialize the global scanner"""
    global _scanner
    _scanner = UnifiedScanner(engine, kraken_client, db_session_factory)
    return _scanner


def start_scanner():
    """Start the scanner if initialized"""
    if _scanner:
        _scanner.start()
    else:
        logger.warning("Scanner not initialized")


def stop_scanner():
    """Stop the scanner"""
    if _scanner:
        _scanner.stop()


# Keep old names for backwards compatibility
LiveTradingScanner = UnifiedScanner
get_live_scanner = get_scanner
initialize_live_scanner = lambda engine, kraken_client: initialize_scanner(engine, kraken_client, None)
start_live_scanner = start_scanner
stop_live_scanner = stop_scanner
