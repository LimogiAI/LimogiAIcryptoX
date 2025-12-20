"""
Live Trading Scanner

Independent scanner for live trading opportunities.
Runs separately from paper trading scanner with its own settings.
"""
import asyncio
from typing import Optional, List, Dict, Any
from datetime import datetime
from loguru import logger

from .manager import get_live_trading_manager


class LiveTradingScanner:
    """
    Dedicated scanner for live trading.

    Features:
    - Independent scan interval
    - Uses live trading config settings
    - Updates scanner status in database
    - Triggers live trades when opportunities found
    """

    def __init__(self, engine, kraken_client):
        self.engine = engine
        self.kraken_client = kraken_client
        self._running = False
        self._task: Optional[asyncio.Task] = None
        self._scan_interval = 10.0  # Default 10 seconds

        # Stats
        self._last_scan_at: Optional[datetime] = None
        self._scans_completed = 0

    @property
    def is_running(self) -> bool:
        return self._running

    def start(self):
        """Start the scanner background task"""
        if self._running:
            logger.warning("Live scanner already running")
            return

        self._running = True
        self._task = asyncio.create_task(self._scan_loop())
        logger.info("🔍 Live Trading Scanner started")

        # Update status in database
        self._update_status(is_running=True)

    def stop(self):
        """Stop the scanner"""
        self._running = False
        if self._task:
            self._task.cancel()
            self._task = None
        logger.info("🔍 Live Trading Scanner stopped")

        # Update status in database
        self._update_status(is_running=False)

    async def _scan_loop(self):
        """Main scan loop"""
        logger.info(
            f"Live scanner loop starting: {self._scan_interval}s interval")

        while self._running:
            try:
                await self._run_scan_cycle()
            except asyncio.CancelledError:
                logger.info("Live scanner cancelled")
                break
            except Exception as e:
                logger.error(f"Live scanner error: {e}")
                self._update_status(last_error=str(e))

            await asyncio.sleep(self._scan_interval)

    async def _run_scan_cycle(self):
        """Run a single scan cycle"""
        start_time = datetime.utcnow()

        manager = get_live_trading_manager()
        if not manager:
            logger.warning("Live trading manager not available")
            return

        # Get live trading config
        config = manager.get_config()

        # Determine base currencies to scan
        if config.base_currency == 'ALL':
            base_currencies = ['USD', 'USDT', 'EUR', 'BTC', 'ETH']
        elif config.base_currency == 'CUSTOM':
            base_currencies = config.custom_currencies if config.custom_currencies else [
                'USD']
        else:
            base_currencies = [config.base_currency]

        # Run scan using the Rust engine (ALWAYS scan, even if trading disabled)
        try:
            opportunities = self.engine.scan(base_currencies)
            logger.info(
                f"🔍 Live scan returned {len(opportunities) if opportunities else 0} opportunities for bases: {base_currencies}")
        except Exception as e:
            logger.error(f"Engine scan error: {e}")
            self._update_status(last_error=str(e))
            return

        # Get engine stats
        stats = self.engine.get_stats()
        pairs_scanned = stats.pairs_monitored
        paths_found = len(opportunities) if opportunities else 0

        # Filter by profit threshold
        min_threshold_pct = config.min_profit_threshold * 100  # Convert to percentage
        profitable = [
            o for o in opportunities if o.net_profit_pct >= min_threshold_pct]
        profitable_count = len(profitable)

        # Calculate scan duration
        scan_duration_ms = (datetime.utcnow() -
                            start_time).total_seconds() * 1000

        # Update scanner status
        self._update_status(
            is_running=self._running,
            pairs_scanned=pairs_scanned,
            paths_found=paths_found,
            opportunities_found=len(opportunities) if opportunities else 0,
            profitable_count=profitable_count,
            scan_duration_ms=scan_duration_ms,
        )

        self._last_scan_at = datetime.utcnow()
        self._scans_completed += 1

        # Log scan results
        if self._scans_completed % 6 == 1:  # Log every ~60 seconds
            logger.info(
                f"🔍 Live scan: {pairs_scanned} pairs, {paths_found} paths, "
                f"{profitable_count} above {min_threshold_pct:.2f}% threshold"
            )

        # Skip execution if live trading not enabled
        if not config.is_enabled:
            # Still save top opportunities as SKIPPED so user can see them
            for opp in profitable[:5]:  # Top 5 only
                manager.save_opportunity(
                    path=opp.path,
                    expected_profit_pct=opp.net_profit_pct,
                    status='SKIPPED',
                    status_reason='Live trading disabled',
                    pairs_scanned=pairs_scanned,
                    paths_found=paths_found,
                )
            return

        # Try to execute profitable opportunities
        for opp in profitable:
            try:
                result = await manager.try_execute_opportunity(
                    path=opp.path,
                    profit_pct=opp.net_profit_pct,
                    pairs_scanned=pairs_scanned,
                    paths_found=paths_found,
                )

                if result:
                    logger.info(
                        f"💰 Live trade executed: {opp.path} | "
                        f"Expected: {opp.net_profit_pct:.3f}% | "
                        f"Status: {result.status}"
                    )

                    # Only execute one trade per cycle in sequential mode
                    if config.execution_mode == 'sequential':
                        break

            except Exception as e:
                logger.error(f"Error executing live trade: {e}")

    def _update_status(
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

    def get_status(self) -> Dict[str, Any]:
        """Get current scanner status"""
        return {
            'is_running': self._running,
            'scan_interval': self._scan_interval,
            'scans_completed': self._scans_completed,
            'last_scan_at': self._last_scan_at.isoformat() if self._last_scan_at else None,
        }


# Singleton instance
_live_scanner: Optional[LiveTradingScanner] = None


def get_live_scanner() -> Optional[LiveTradingScanner]:
    """Get the global live scanner instance"""
    return _live_scanner


def initialize_live_scanner(engine, kraken_client) -> LiveTradingScanner:
    """Initialize the global live scanner"""
    global _live_scanner

    _live_scanner = LiveTradingScanner(engine, kraken_client)

    return _live_scanner


def start_live_scanner():
    """Start the live scanner if initialized"""
    if _live_scanner:
        _live_scanner.start()
    else:
        logger.warning("Live scanner not initialized")


def stop_live_scanner():
    """Stop the live scanner"""
    if _live_scanner:
        _live_scanner.stop()
