"""
Live Trading Manager - Main Orchestrator

Coordinates all live trading components:
- ConfigManager: Settings from database
- CircuitBreaker: Safety limits
- TradeGuard: Pre-trade checks
- LiveExecutor: Order execution
"""
import asyncio
from typing import Optional, Dict, Any, List
from datetime import datetime, timedelta
from loguru import logger

from .config import ConfigManager, LiveTradingSettings
from .circuit_breaker import CircuitBreaker, CircuitBreakerState
from .guard import TradeGuard, GuardCheckResult
from .executor import LiveExecutor, TradeExecution


class LiveTradingManager:
    """
    Main orchestrator for live trading.
    
    This is the primary interface for:
    - Enabling/disabling live trading
    - Configuring settings
    - Executing trades (manual or automatic)
    - Monitoring status
    """
    
    def __init__(
        self,
        kraken_client,
        db_session_factory,
    ):
        self.kraken_client = kraken_client
        self.db_session_factory = db_session_factory
        
        # Initialize components
        self.config = ConfigManager(db_session_factory)
        self.circuit_breaker = CircuitBreaker(db_session_factory, self.config)
        self.guard = TradeGuard(self.config, self.circuit_breaker, kraken_client)
        self.executor = LiveExecutor(kraken_client, db_session_factory, self.config, self.circuit_breaker)
        
        logger.info("Live Trading Manager initialized")
    
    # ==========================================
    # Configuration
    # ==========================================
    
    def get_config(self) -> LiveTradingSettings:
        """Get current configuration"""
        return self.config.get_settings()
    
    def update_config(self, updates: Dict[str, Any]) -> LiveTradingSettings:
        """Update configuration"""
        return self.config.update_settings(updates)
    
    def get_config_options(self) -> Dict[str, Any]:
        """Get valid options for configuration (for UI dropdowns)"""
        return self.config.get_options()
    
    # ==========================================
    # Enable/Disable
    # ==========================================
    
    async def enable(self, confirm: bool = False, confirm_text: str = "") -> Dict[str, Any]:
        """
        Enable live trading.
        
        Requires explicit confirmation for safety.
        """
        if not confirm or confirm_text != "I understand the risks":
            return {
                'success': False,
                'error': 'Must confirm with confirm=true AND confirm_text="I understand the risks"',
            }
        
        # Run pre-flight checks
        check_result = await self.guard.check_all()
        
        if not check_result.can_trade:
            # Don't enable if checks fail (except for the "not enabled" check)
            checks_without_enabled = {k: v for k, v in check_result.checks.items() if k != 'live_enabled'}
            if not all(checks_without_enabled.values()):
                return {
                    'success': False,
                    'error': f'Pre-flight checks failed: {check_result.reason}',
                    'checks': check_result.checks,
                }
        
        # Enable
        settings = self.config.enable()
        
        logger.warning("⚠️ LIVE TRADING ENABLED - Real money will be used!")
        
        return {
            'success': True,
            'message': '⚠️ LIVE TRADING ENABLED - Real money will be used!',
            'config': settings.to_dict(),
        }
    
    def disable(self, reason: str = "Manual disable") -> Dict[str, Any]:
        """Disable live trading"""
        settings = self.config.disable(reason)
        
        return {
            'success': True,
            'message': 'Live trading disabled',
            'reason': reason,
            'config': settings.to_dict(),
        }
    
    # ==========================================
    # Status
    # ==========================================
    
    async def get_status(self) -> Dict[str, Any]:
        """Get complete live trading status"""
        config = self.config.get_settings()
        state = self.circuit_breaker.get_state()
        guard_status = await self.guard.get_status()
        
        return {
            'enabled': config.is_enabled,
            'config': config.to_dict(),
            'state': state.to_dict(),
            'guard': guard_status,
            'can_trade': config.is_enabled and not state.is_broken and not state.is_executing,
        }
    
    def get_circuit_breaker_state(self) -> CircuitBreakerState:
        """Get circuit breaker state"""
        return self.circuit_breaker.get_state()
    
    # ==========================================
    # Trade Execution
    # ==========================================
    
    async def execute_trade(
        self,
        path: str,
        amount: Optional[float] = None,
        opportunity_profit_pct: float = 0.0,
    ) -> TradeExecution:
        """
        Execute a live trade.
        
        Args:
            path: Arbitrage path (e.g., "USD → BTC → ETH → USD")
            amount: Trade amount (uses config default if not specified)
            opportunity_profit_pct: Expected profit from scanner
            
        Returns:
            TradeExecution result
        """
        config = self.config.get_settings()
        
        if amount is None:
            amount = config.trade_amount
        
        # Run guard checks
        check_result = await self.guard.check_all(amount)
        
        if not check_result.can_trade:
            logger.warning(f"Trade blocked by guard: {check_result.reason}")
            return TradeExecution(
                trade_id=f"BLOCKED-{datetime.utcnow().strftime('%Y%m%d%H%M%S')}",
                path=path,
                legs=len(path.split('→')) - 1,
                amount_in=amount,
                status='FAILED',
                error_message=f"Guard check failed: {check_result.reason}",
            )
        
        # Check base currency filter
        if not self.guard.check_base_currency_filter(path):
            logger.info(f"Trade skipped: path {path} doesn't match base currency filter")
            return TradeExecution(
                trade_id=f"FILTERED-{datetime.utcnow().strftime('%Y%m%d%H%M%S')}",
                path=path,
                legs=len(path.split('→')) - 1,
                amount_in=amount,
                status='FAILED',
                error_message="Path doesn't match base currency filter",
            )
        
        # Check min profit threshold
        if opportunity_profit_pct < config.min_profit_threshold * 100:
            logger.info(f"Trade skipped: profit {opportunity_profit_pct:.2f}% below threshold {config.min_profit_threshold * 100:.2f}%")
            return TradeExecution(
                trade_id=f"LOW-PROFIT-{datetime.utcnow().strftime('%Y%m%d%H%M%S')}",
                path=path,
                legs=len(path.split('→')) - 1,
                amount_in=amount,
                status='FAILED',
                error_message=f"Profit {opportunity_profit_pct:.2f}% below threshold",
            )
        
        # Execute!
        return await self.executor.execute_arbitrage(path, amount, opportunity_profit_pct)
    
    async def try_execute_opportunity(
        self,
        path: str,
        profit_pct: float,
        pairs_scanned: int = None,
        paths_found: int = None,
    ) -> Optional[TradeExecution]:
        """
        Try to execute an opportunity from the scanner.
        
        This is called by the scan loop when an opportunity is found.
        Returns None if trade was not executed (filtered, blocked, etc.)
        """
        config = self.config.get_settings()
        
        # Quick checks before full guard check
        if not config.is_enabled:
            # Save as skipped - trading disabled
            self.save_opportunity(
                path=path,
                expected_profit_pct=profit_pct,
                status='SKIPPED',
                status_reason='Live trading disabled',
                pairs_scanned=pairs_scanned,
                paths_found=paths_found,
            )
            return None
        
        state = self.circuit_breaker.get_state()
        if state.is_broken:
            # Save as missed - circuit breaker
            self.save_opportunity(
                path=path,
                expected_profit_pct=profit_pct,
                status='MISSED',
                status_reason='Circuit breaker triggered',
                pairs_scanned=pairs_scanned,
                paths_found=paths_found,
            )
            return None
        
        if config.execution_mode == 'sequential' and state.is_executing:
            # Save as skipped - already executing
            self.save_opportunity(
                path=path,
                expected_profit_pct=profit_pct,
                status='SKIPPED',
                status_reason='Trade already executing',
                pairs_scanned=pairs_scanned,
                paths_found=paths_found,
            )
            return None
        
        # Check base currency filter
        if not self.guard.check_base_currency_filter(path):
            # Save as skipped - wrong currency
            self.save_opportunity(
                path=path,
                expected_profit_pct=profit_pct,
                status='SKIPPED',
                status_reason='Base currency filter',
                pairs_scanned=pairs_scanned,
                paths_found=paths_found,
            )
            return None
        
        # Check profit threshold
        if profit_pct < config.min_profit_threshold * 100:
            # Save as skipped - below threshold
            self.save_opportunity(
                path=path,
                expected_profit_pct=profit_pct,
                status='SKIPPED',
                status_reason=f'Below threshold ({profit_pct:.2f}% < {config.min_profit_threshold * 100:.2f}%)',
                pairs_scanned=pairs_scanned,
                paths_found=paths_found,
            )
            return None
        
        # Save as pending before execution
        opp = self.save_opportunity(
            path=path,
            expected_profit_pct=profit_pct,
            status='PENDING',
            pairs_scanned=pairs_scanned,
            paths_found=paths_found,
        )
        
        # Execute
        result = await self.execute_trade(path, config.trade_amount, profit_pct)
        
        # Update opportunity status based on result
        if result.status in ['COMPLETED', 'PARTIAL']:
            self.update_opportunity_status(
                path=path,
                status='EXECUTED',
                status_reason=f'Trade {result.status}',
                trade_id=result.trade_id,
            )
            return result
        elif result.status == 'FAILED':
            self.update_opportunity_status(
                path=path,
                status='MISSED',
                status_reason=result.error_message or 'Trade failed',
            )
            return result
        
        return None
    
    # ==========================================
    # Trade History
    # ==========================================
    
    def get_trades(
        self,
        limit: int = 50,
        status: Optional[str] = None,
        hours: int = 24,
    ) -> List[Dict[str, Any]]:
        """Get recent live trades"""
        db = self.db_session_factory()
        try:
            from app.models.live_trading import LiveTrade
            
            query = db.query(LiveTrade)
            
            if status:
                query = query.filter(LiveTrade.status == status)
            
            if hours:
                cutoff = datetime.utcnow() - timedelta(hours=hours)
                query = query.filter(LiveTrade.created_at >= cutoff)
            
            query = query.order_by(LiveTrade.created_at.desc()).limit(limit)
            
            trades = query.all()
            
            return [
                {
                    'id': t.id,
                    'trade_id': t.trade_id,
                    'path': t.path,
                    'legs': t.legs,
                    'amount_in': t.amount_in,
                    'amount_out': t.amount_out,
                    'profit_loss': t.profit_loss,
                    'profit_loss_pct': t.profit_loss_pct,
                    'status': t.status,
                    'error_message': t.error_message,
                    'held_currency': t.held_currency,
                    'held_amount': t.held_amount,
                    'order_ids': t.order_ids,
                    'leg_fills': t.leg_fills,
                    'started_at': t.started_at.isoformat() if t.started_at else None,
                    'completed_at': t.completed_at.isoformat() if t.completed_at else None,
                    'total_execution_ms': t.total_execution_ms,
                    'opportunity_profit_pct': t.opportunity_profit_pct,
                }
                for t in trades
            ]
            
        finally:
            db.close()
    
    def get_trade_by_id(self, trade_id: str) -> Optional[Dict[str, Any]]:
        """Get a specific trade by ID"""
        db = self.db_session_factory()
        try:
            from app.models.live_trading import LiveTrade
            
            trade = db.query(LiveTrade).filter(LiveTrade.trade_id == trade_id).first()
            
            if not trade:
                return None
            
            return {
                'id': trade.id,
                'trade_id': trade.trade_id,
                'path': trade.path,
                'legs': trade.legs,
                'amount_in': trade.amount_in,
                'amount_out': trade.amount_out,
                'profit_loss': trade.profit_loss,
                'profit_loss_pct': trade.profit_loss_pct,
                'status': trade.status,
                'current_leg': trade.current_leg,
                'error_message': trade.error_message,
                'held_currency': trade.held_currency,
                'held_amount': trade.held_amount,
                'order_ids': trade.order_ids,
                'leg_fills': trade.leg_fills,
                'started_at': trade.started_at.isoformat() if trade.started_at else None,
                'completed_at': trade.completed_at.isoformat() if trade.completed_at else None,
                'total_execution_ms': trade.total_execution_ms,
                'opportunity_profit_pct': trade.opportunity_profit_pct,
            }
            
        finally:
            db.close()
    
    # ==========================================
    # Positions
    # ==========================================
    
    async def get_positions(self) -> Dict[str, Any]:
        """Get current positions from Kraken with USD values for all assets"""
        try:
            balances = await self.kraken_client.get_balance()

            # Currency code mapping (Kraken uses X/Z prefixes)
            currency_map = {
                'XXBT': 'BTC',
                'XETH': 'ETH',
                'XLTC': 'LTC',
                'XXRP': 'XRP',
                'XXLM': 'XLM',
                'XDOGE': 'DOGE',
                'XXMR': 'XMR',
                'XZEC': 'ZEC',
                'ZUSD': 'USD',
                'ZEUR': 'EUR',
                'ZGBP': 'GBP',
                'ZCAD': 'CAD',
                'ZJPY': 'JPY',
                'ZAUD': 'AUD',
            }

            # Multiple pair formats to try for price lookups (Kraken is inconsistent)
            pair_variants = {
                'XXBT': ['XXBTZUSD', 'XBTUSD'],
                'XETH': ['XETHZUSD', 'ETHUSD'],
                'XLTC': ['XLTCZUSD', 'LTCUSD'],
                'XXRP': ['XXRPZUSD', 'XRPUSD'],
                'XXLM': ['XXLMZUSD', 'XLMUSD'],
                'XDOGE': ['XDOGEZUSD', 'DOGEUSD'],
                'XXMR': ['XXMRZUSD', 'XMRUSD'],
                'XZEC': ['XZECZUSD', 'ZECUSD'],
                'ZEUR': ['EURUSD', 'ZEURZUSD'],
                'ZGBP': ['GBPUSD', 'ZGBPZUSD'],
                'BTC': ['XBTUSD', 'XXBTZUSD'],
                'ETH': ['ETHUSD', 'XETHZUSD'],
                'LTC': ['LTCUSD', 'XLTCZUSD'],
                'XRP': ['XRPUSD', 'XXRPZUSD'],
                'USDT': ['USDTZUSD'],
                'USDC': ['USDCUSD'],
                'DOT': ['DOTUSD'],
                'SOL': ['SOLUSD'],
                'ADA': ['ADAUSD'],
                'LINK': ['LINKUSD'],
                'MATIC': ['MATICUSD'],
                'AVAX': ['AVAXUSD'],
                'ATOM': ['ATOMUSD'],
                'UNI': ['UNIUSD'],
                'TRX': ['TRXUSD'],
                'DOGE': ['DOGEUSD', 'XDOGEZUSD'],
                'XLM': ['XLMUSD', 'XXLMZUSD'],
            }

            positions = []
            total_usd = 0.0
            balances_dict = {}

            for currency, balance in balances.items():
                bal = float(balance)
                if bal <= 0.00001:  # Skip dust amounts
                    continue

                # Get display name
                display_name = currency_map.get(currency, currency)

                # Calculate USD value
                usd_value = 0.0

                # USD and stablecoins are 1:1
                if currency in ['ZUSD', 'USD']:
                    usd_value = bal
                elif currency in ['USDT', 'USDC', 'DAI', 'USDT.M']:
                    usd_value = bal  # Stablecoins ~= 1 USD
                else:
                    # Try multiple pair formats
                    pairs_to_try = pair_variants.get(currency, [])
                    # Add dynamic patterns as fallback
                    if not pairs_to_try:
                        pairs_to_try = [
                            f"{currency}USD",
                            f"{currency}ZUSD",
                            f"X{currency}ZUSD",
                        ]

                    price = 0.0
                    for pair in pairs_to_try:
                        try:
                            ticker = await self.kraken_client.get_ticker(pair)
                            if ticker and len(ticker) > 0:
                                pair_data = list(ticker.values())[0]
                                # 'c' is the last trade closed [price, lot volume]
                                price = float(pair_data.get('c', [0])[0])
                                if price > 0:
                                    usd_value = bal * price
                                    break
                        except Exception:
                            continue

                    if usd_value == 0 and bal > 0:
                        logger.debug(f"Could not get USD price for {currency}, tried: {pairs_to_try}")

                positions.append({
                    'currency': display_name,
                    'raw_currency': currency,
                    'balance': bal,
                    'usd_value': round(usd_value, 2),
                })

                balances_dict[display_name] = bal
                total_usd += usd_value

            # Sort positions by USD value (highest first)
            positions.sort(key=lambda x: x['usd_value'], reverse=True)

            return {
                'connected': True,
                'positions': positions,
                'balances': balances_dict,
                'total_usd': round(total_usd, 2),
                'synced_at': datetime.utcnow().isoformat(),
            }

        except Exception as e:
            logger.error(f"Error fetching positions: {e}")
            return {
                'connected': False,
                'positions': [],
                'balances': {},
                'total_usd': 0,
                'error': str(e),
            }
    
    # ==========================================
    # Circuit Breaker Controls
    # ==========================================
    
    def reset_circuit_breaker(self) -> CircuitBreakerState:
        """Reset circuit breaker (does not reset loss counters)"""
        return self.circuit_breaker.reset()
    
    def reset_daily_stats(self) -> CircuitBreakerState:
        """Reset daily statistics"""
        return self.circuit_breaker.reset_daily_stats()
    
    def reset_all_stats(self) -> CircuitBreakerState:
        """Reset all statistics (use with caution!)"""
        return self.circuit_breaker.reset_total_stats()
    
    def trigger_circuit_breaker(self, reason: str = "Manual trigger"):
        """Manually trigger circuit breaker"""
        self.circuit_breaker.trigger_manual(reason)
    
    # ==========================================
    # Opportunity Tracking
    # ==========================================
    
    def save_opportunity(
        self,
        path: str,
        expected_profit_pct: float,
        status: str = 'PENDING',
        status_reason: str = None,
        trade_id: str = None,
        pairs_scanned: int = None,
        paths_found: int = None,
    ) -> Dict[str, Any]:
        """Save an opportunity to the database"""
        db = self.db_session_factory()
        try:
            from app.models.live_trading import LiveOpportunity
            
            config = self.config.get_settings()
            
            opportunity = LiveOpportunity(
                path=path,
                legs=path.count('→'),
                expected_profit_pct=expected_profit_pct,
                expected_profit_usd=(expected_profit_pct / 100) * config.trade_amount if config.trade_amount else None,
                trade_amount=config.trade_amount,
                status=status,
                status_reason=status_reason,
                trade_id=trade_id,
                pairs_scanned=pairs_scanned,
                paths_found=paths_found,
            )
            
            db.add(opportunity)
            db.commit()
            db.refresh(opportunity)
            
            return {
                'id': opportunity.id,
                'path': opportunity.path,
                'expected_profit_pct': opportunity.expected_profit_pct,
                'status': opportunity.status,
            }
            
        except Exception as e:
            logger.error(f"Error saving opportunity: {e}")
            db.rollback()
            return None
        finally:
            db.close()
    
    def update_opportunity_status(
        self,
        opportunity_id: int = None,
        path: str = None,
        status: str = None,
        status_reason: str = None,
        trade_id: str = None,
    ):
        """Update opportunity status (by id or most recent with path)"""
        db = self.db_session_factory()
        try:
            from app.models.live_trading import LiveOpportunity
            
            if opportunity_id:
                opportunity = db.query(LiveOpportunity).filter(
                    LiveOpportunity.id == opportunity_id
                ).first()
            elif path:
                opportunity = db.query(LiveOpportunity).filter(
                    LiveOpportunity.path == path,
                    LiveOpportunity.status == 'PENDING'
                ).order_by(LiveOpportunity.found_at.desc()).first()
            else:
                return
            
            if opportunity:
                if status:
                    opportunity.status = status
                if status_reason:
                    opportunity.status_reason = status_reason
                if trade_id:
                    opportunity.trade_id = trade_id
                
                db.commit()
                
        except Exception as e:
            logger.error(f"Error updating opportunity: {e}")
            db.rollback()
        finally:
            db.close()
    
    def get_opportunities(
        self,
        limit: int = 50,
        status: str = None,
        hours: int = 24,
    ) -> List[Dict[str, Any]]:
        """Get recent opportunities"""
        db = self.db_session_factory()
        try:
            from app.models.live_trading import LiveOpportunity
            
            query = db.query(LiveOpportunity)
            
            if status:
                query = query.filter(LiveOpportunity.status == status)
            
            if hours:
                cutoff = datetime.utcnow() - timedelta(hours=hours)
                query = query.filter(LiveOpportunity.found_at >= cutoff)
            
            query = query.order_by(LiveOpportunity.found_at.desc()).limit(limit)
            
            opportunities = query.all()
            
            return [
                {
                    'id': opp.id,
                    'found_at': opp.found_at.isoformat() if opp.found_at else None,
                    'path': opp.path,
                    'legs': opp.legs,
                    'expected_profit_pct': opp.expected_profit_pct,
                    'expected_profit_usd': opp.expected_profit_usd,
                    'trade_amount': opp.trade_amount,
                    'status': opp.status,
                    'status_reason': opp.status_reason,
                    'trade_id': opp.trade_id,
                    'pairs_scanned': opp.pairs_scanned,
                    'paths_found': opp.paths_found,
                }
                for opp in opportunities
            ]
            
        finally:
            db.close()
    
    # ==========================================
    # Scanner Status
    # ==========================================
    
    def update_scanner_status(
        self,
        is_running: bool = None,
        pairs_scanned: int = None,
        paths_found: int = None,
        opportunities_found: int = None,
        profitable_count: int = None,
        scan_duration_ms: float = None,
        last_error: str = None,
    ):
        """Update scanner status"""
        db = self.db_session_factory()
        try:
            from app.models.live_trading import LiveScannerStatus
            
            status = db.query(LiveScannerStatus).filter(LiveScannerStatus.id == 1).first()
            
            if not status:
                status = LiveScannerStatus(id=1)
                db.add(status)
            
            if is_running is not None:
                status.is_running = is_running
            if pairs_scanned is not None:
                status.pairs_scanned = pairs_scanned
            if paths_found is not None:
                status.paths_found = paths_found
            if opportunities_found is not None:
                status.opportunities_found = opportunities_found
            if profitable_count is not None:
                status.profitable_count = profitable_count
            if scan_duration_ms is not None:
                status.scan_duration_ms = scan_duration_ms
            if last_error is not None:
                status.last_error = last_error
                status.last_error_at = datetime.utcnow()
            
            status.last_scan_at = datetime.utcnow()
            
            db.commit()
            
        except Exception as e:
            logger.error(f"Error updating scanner status: {e}")
            db.rollback()
        finally:
            db.close()
    
    def get_scanner_status(self) -> Dict[str, Any]:
        """Get current scanner status"""
        db = self.db_session_factory()
        try:
            from app.models.live_trading import LiveScannerStatus
            
            status = db.query(LiveScannerStatus).filter(LiveScannerStatus.id == 1).first()
            
            if not status:
                return {
                    'is_running': False,
                    'last_scan_at': None,
                    'pairs_scanned': 0,
                    'paths_found': 0,
                    'opportunities_found': 0,
                    'profitable_count': 0,
                    'scan_duration_ms': None,
                    'last_error': None,
                }
            
            # Calculate seconds since last scan
            seconds_ago = None
            if status.last_scan_at:
                seconds_ago = (datetime.utcnow() - status.last_scan_at).total_seconds()
            
            return {
                'is_running': status.is_running,
                'last_scan_at': status.last_scan_at.isoformat() if status.last_scan_at else None,
                'seconds_ago': seconds_ago,
                'pairs_scanned': status.pairs_scanned,
                'paths_found': status.paths_found,
                'opportunities_found': status.opportunities_found,
                'profitable_count': status.profitable_count,
                'scan_duration_ms': status.scan_duration_ms,
                'last_error': status.last_error,
                'last_error_at': status.last_error_at.isoformat() if status.last_error_at else None,
            }
            
        finally:
            db.close()


# Singleton instance
_live_trading_manager: Optional[LiveTradingManager] = None


def get_live_trading_manager() -> Optional[LiveTradingManager]:
    """Get the global live trading manager instance"""
    return _live_trading_manager


def initialize_live_trading(kraken_client, db_session_factory) -> LiveTradingManager:
    """Initialize the global live trading manager"""
    global _live_trading_manager
    
    _live_trading_manager = LiveTradingManager(kraken_client, db_session_factory)
    
    logger.info("✅ Live Trading Manager initialized")
    
    return _live_trading_manager
