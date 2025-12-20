"""
Live Executor - Executes Real Trades on Kraken

This is where the actual trading happens.
Handles sequential leg execution with fill verification.
"""
import asyncio
import uuid
from typing import Optional, Dict, Any, List, Tuple
from dataclasses import dataclass, field
from datetime import datetime
from loguru import logger


# Currency mapping for Kraken
CURRENCY_MAP = {
    "BTC": "XBT",
    "DOGE": "XDG",
}

# Quote currencies (used to determine buy vs sell direction)
QUOTE_CURRENCIES = {"USD", "USDT", "EUR", "ZUSD", "ZEUR", "GBP", "ZGBP", "CAD", "ZCAD", "JPY", "ZJPY"}


@dataclass
class LegExecution:
    """Result of executing a single leg"""
    leg_number: int
    pair: str
    side: str  # 'buy' or 'sell'
    
    # What we tried to do
    input_currency: str
    input_amount: float
    output_currency: str
    
    # What actually happened
    success: bool
    order_id: Optional[str] = None
    executed_price: Optional[float] = None
    executed_amount: Optional[float] = None
    output_amount: Optional[float] = None
    fee: Optional[float] = None
    fee_currency: Optional[str] = None
    
    # Slippage tracking (comparing expected vs actual price)
    expected_price: Optional[float] = None  # Best bid/ask before order
    slippage_pct: Optional[float] = None    # (actual - expected) / expected * 100
    slippage_usd: Optional[float] = None    # Slippage in USD terms
    
    # Timing
    started_at: Optional[datetime] = None
    completed_at: Optional[datetime] = None
    execution_ms: Optional[float] = None
    
    # Error info
    error: Optional[str] = None
    retries: int = 0
    
    def to_dict(self) -> Dict[str, Any]:
        return {
            'leg': self.leg_number,
            'pair': self.pair,
            'side': self.side,
            'input_currency': self.input_currency,
            'input_amount': self.input_amount,
            'output_currency': self.output_currency,
            'success': self.success,
            'order_id': self.order_id,
            'executed_price': self.executed_price,
            'executed_amount': self.executed_amount,
            'output_amount': self.output_amount,
            'fee': self.fee,
            'fee_currency': self.fee_currency,
            'expected_price': self.expected_price,
            'slippage_pct': self.slippage_pct,
            'slippage_usd': self.slippage_usd,
            'execution_ms': self.execution_ms,
            'error': self.error,
            'retries': self.retries,
        }


@dataclass
class TradeExecution:
    """Result of executing a complete arbitrage trade"""
    trade_id: str
    path: str
    legs: int
    
    # Amounts
    amount_in: float
    amount_out: Optional[float] = None
    profit_loss: Optional[float] = None
    profit_loss_pct: Optional[float] = None
    
    # Status
    status: str = 'PENDING'  # PENDING, EXECUTING, COMPLETED, PARTIAL, FAILED
    current_leg: int = 0
    error_message: Optional[str] = None
    
    # Held position if partial
    held_currency: Optional[str] = None
    held_amount: Optional[float] = None
    
    # Leg details
    leg_executions: List[LegExecution] = field(default_factory=list)
    order_ids: List[str] = field(default_factory=list)
    
    # Timing
    started_at: Optional[datetime] = None
    completed_at: Optional[datetime] = None
    total_execution_ms: Optional[float] = None
    
    def to_dict(self) -> Dict[str, Any]:
        return {
            'trade_id': self.trade_id,
            'path': self.path,
            'legs': self.legs,
            'amount_in': self.amount_in,
            'amount_out': self.amount_out,
            'profit_loss': self.profit_loss,
            'profit_loss_pct': self.profit_loss_pct,
            'status': self.status,
            'current_leg': self.current_leg,
            'error_message': self.error_message,
            'held_currency': self.held_currency,
            'held_amount': self.held_amount,
            'leg_executions': [leg.to_dict() for leg in self.leg_executions],
            'order_ids': self.order_ids,
            'started_at': self.started_at.isoformat() if self.started_at else None,
            'completed_at': self.completed_at.isoformat() if self.completed_at else None,
            'total_execution_ms': self.total_execution_ms,
        }


class LiveExecutor:
    """
    Executes live trades on Kraken.
    
    Flow:
    1. Parse arbitrage path into legs
    2. For each leg:
       a. Determine pair and side (buy/sell)
       b. Place market order
       c. Wait for fill
       d. Verify execution
       e. Pass proceeds to next leg
    3. Record final result
    
    On failure:
    - HOLD position (Option B)
    - Alert user
    - Do NOT attempt reversal
    """
    
    def __init__(self, kraken_client, db_session_factory, config_manager, circuit_breaker):
        self.kraken_client = kraken_client
        self.db_session_factory = db_session_factory
        self.config_manager = config_manager
        self.circuit_breaker = circuit_breaker
    
    def _get_db(self):
        return self.db_session_factory()
    
    def _parse_path(self, path: str) -> List[str]:
        """Parse path string into list of currencies"""
        if ' → ' in path:
            return [c.strip() for c in path.split(' → ')]
        elif '→' in path:
            return [c.strip() for c in path.split('→')]
        else:
            return [c.strip() for c in path.split()]
    
    def _determine_pair_and_side(self, from_currency: str, to_currency: str) -> Tuple[str, str]:
        """
        Determine Kraken pair name and whether to buy or sell.
        
        Returns: (kraken_pair, side)
        
        Examples:
        - USD → BTC: pair=XBTUSD, side=buy (buying BTC with USD)
        - BTC → ETH: pair=ETHXBT, side=buy (buying ETH with BTC)
        - ETH → USD: pair=ETHUSD, side=sell (selling ETH for USD)
        """
        # Normalize currencies
        from_norm = CURRENCY_MAP.get(from_currency, from_currency)
        to_norm = CURRENCY_MAP.get(to_currency, to_currency)
        
        # If from_currency is a quote currency, we're buying the to_currency
        if from_currency in QUOTE_CURRENCIES or from_currency.startswith("Z"):
            # Buying to_currency with from_currency
            pair = f"{to_norm}{from_norm}"
            return pair, "buy"
        else:
            # Selling from_currency for to_currency
            if to_currency in QUOTE_CURRENCIES or to_currency.startswith("Z"):
                pair = f"{from_norm}{to_norm}"
                return pair, "sell"
            else:
                # Both are base currencies - try FROM/TO
                pair = f"{from_norm}{to_norm}"
                return pair, "sell"
    
    async def execute_arbitrage(
        self, 
        path: str, 
        amount: float,
        opportunity_profit_pct: float = 0.0
    ) -> TradeExecution:
        """
        Execute a complete arbitrage trade.
        
        Args:
            path: Arbitrage path (e.g., "USD → BTC → ETH → USD")
            amount: Starting amount in first currency
            opportunity_profit_pct: Expected profit % from scanner
            
        Returns:
            TradeExecution with results
        """
        trade_id = f"LIVE-{uuid.uuid4().hex[:12].upper()}"
        currencies = self._parse_path(path)
        num_legs = len(currencies) - 1
        
        execution = TradeExecution(
            trade_id=trade_id,
            path=path,
            legs=num_legs,
            amount_in=amount,
            status='EXECUTING',
            started_at=datetime.utcnow(),
        )
        
        # Mark as executing in circuit breaker
        if not self.circuit_breaker.mark_executing(trade_id):
            execution.status = 'FAILED'
            execution.error_message = "Another trade is already executing"
            return execution
        
        logger.info(f"🚀 Starting live trade {trade_id}: {path} with ${amount:.2f}")
        
        try:
            current_amount = amount
            current_currency = currencies[0]
            
            for i in range(num_legs):
                from_currency = currencies[i]
                to_currency = currencies[i + 1]
                
                execution.current_leg = i + 1
                
                # Execute this leg
                leg_result = await self._execute_leg(
                    leg_number=i + 1,
                    from_currency=from_currency,
                    to_currency=to_currency,
                    amount=current_amount,
                )
                
                execution.leg_executions.append(leg_result)
                
                if leg_result.order_id:
                    execution.order_ids.append(leg_result.order_id)
                
                if not leg_result.success:
                    # Leg failed - HOLD position (Option B)
                    execution.status = 'PARTIAL' if i > 0 else 'FAILED'
                    execution.error_message = f"Leg {i + 1} failed: {leg_result.error}"
                    execution.held_currency = current_currency
                    execution.held_amount = current_amount
                    
                    logger.error(f"❌ Trade {trade_id} failed at leg {i + 1}: {leg_result.error}")
                    logger.warning(f"⚠️ HOLDING {current_amount:.6f} {current_currency}")
                    
                    break
                
                # Update for next leg
                current_amount = leg_result.output_amount
                current_currency = to_currency
                
                logger.info(f"✅ Leg {i + 1} complete: {leg_result.output_amount:.6f} {to_currency}")
            
            # If all legs completed
            if execution.status == 'EXECUTING':
                execution.status = 'COMPLETED'
                execution.amount_out = current_amount
                execution.profit_loss = current_amount - amount
                execution.profit_loss_pct = (execution.profit_loss / amount) * 100
                
                logger.info(f"✅ Trade {trade_id} COMPLETED: ${amount:.2f} → ${current_amount:.2f} ({'+' if execution.profit_loss >= 0 else ''}{execution.profit_loss_pct:.2f}%)")
            
        except Exception as e:
            execution.status = 'FAILED'
            execution.error_message = str(e)
            logger.error(f"❌ Trade {trade_id} exception: {e}")
        
        finally:
            execution.completed_at = datetime.utcnow()
            execution.total_execution_ms = (execution.completed_at - execution.started_at).total_seconds() * 1000
            
            # Mark execution complete
            self.circuit_breaker.mark_execution_complete(trade_id)
            
            # Record result in circuit breaker
            if execution.profit_loss is not None:
                is_win = execution.profit_loss >= 0
                self.circuit_breaker.record_trade_result(
                    profit_loss=execution.profit_loss,
                    is_win=is_win,
                    trade_id=trade_id,
                    trade_amount=amount
                )
            
            # Save to database
            await self._save_trade(execution, opportunity_profit_pct)
        
        return execution
    
    async def _execute_leg(
        self,
        leg_number: int,
        from_currency: str,
        to_currency: str,
        amount: float,
    ) -> LegExecution:
        """Execute a single leg of the arbitrage"""
        config = self.config_manager.get_settings()
        max_retries = config.max_retries_per_leg
        timeout = config.order_timeout_seconds
        
        pair, side = self._determine_pair_and_side(from_currency, to_currency)
        
        leg = LegExecution(
            leg_number=leg_number,
            pair=pair,
            side=side,
            input_currency=from_currency,
            input_amount=amount,
            output_currency=to_currency,
            success=False,
            started_at=datetime.utcnow(),
        )
        
        logger.info(f"  Leg {leg_number}: {from_currency} → {to_currency} ({side} on {pair})")
        
        for attempt in range(max_retries + 1):
            leg.retries = attempt
            
            try:
                # Get expected price BEFORE placing order (for slippage calculation)
                expected_price = await self._get_expected_price(pair, side)
                leg.expected_price = expected_price
                
                # Calculate volume for order
                volume = await self._calculate_volume(pair, side, amount, from_currency)
                
                if volume <= 0:
                    leg.error = "Could not calculate valid volume"
                    continue
                
                # Place market order
                order_result = await self._place_order(pair, side, volume)
                
                if not order_result.get('success'):
                    leg.error = order_result.get('error', 'Order placement failed')
                    logger.warning(f"    Attempt {attempt + 1}: {leg.error}")
                    continue
                
                leg.order_id = order_result.get('order_id')
                
                # Wait for fill
                fill_result = await self._wait_for_fill(leg.order_id, timeout)
                
                if not fill_result.get('filled'):
                    leg.error = fill_result.get('error', 'Order not filled')
                    logger.warning(f"    Attempt {attempt + 1}: {leg.error}")
                    
                    # Try to cancel unfilled order
                    await self._cancel_order(leg.order_id)
                    continue
                
                # Success!
                leg.success = True
                leg.executed_price = fill_result.get('price')
                leg.executed_amount = fill_result.get('volume')
                leg.fee = fill_result.get('fee')
                leg.fee_currency = fill_result.get('fee_currency')
                
                # Calculate slippage (comparing expected vs actual price)
                if leg.expected_price and leg.executed_price and leg.expected_price > 0:
                    if side == 'buy':
                        # For buy orders, slippage is positive if we paid more than expected
                        leg.slippage_pct = ((leg.executed_price - leg.expected_price) / leg.expected_price) * 100
                    else:
                        # For sell orders, slippage is positive if we received less than expected
                        leg.slippage_pct = ((leg.expected_price - leg.executed_price) / leg.expected_price) * 100
                    
                    # Calculate slippage in USD terms
                    if leg.executed_amount:
                        leg.slippage_usd = abs(leg.slippage_pct / 100) * leg.executed_amount * leg.executed_price
                    
                    logger.info(f"    Slippage: {leg.slippage_pct:.4f}% (${leg.slippage_usd:.4f})")
                
                # Calculate output amount
                if side == 'buy':
                    leg.output_amount = leg.executed_amount - (leg.fee or 0)
                else:
                    leg.output_amount = leg.executed_amount * leg.executed_price - (leg.fee or 0)
                
                break
                
            except Exception as e:
                leg.error = str(e)
                logger.warning(f"    Attempt {attempt + 1} exception: {e}")
                await asyncio.sleep(0.5)  # Brief pause before retry
        
        leg.completed_at = datetime.utcnow()
        leg.execution_ms = (leg.completed_at - leg.started_at).total_seconds() * 1000
        
        return leg
    
    async def _get_expected_price(self, pair: str, side: str) -> Optional[float]:
        """Get the expected price (best bid/ask) before placing order"""
        try:
            ticker = await self.kraken_client.get_ticker(pair)
            pair_data = list(ticker.values())[0] if ticker else {}
            
            if side == 'buy':
                # For buy orders, expected price is the ask (what we'll pay)
                return float(pair_data.get('a', [0])[0])
            else:
                # For sell orders, expected price is the bid (what we'll receive)
                return float(pair_data.get('b', [0])[0])
        except Exception as e:
            logger.warning(f"Failed to get expected price: {e}")
            return None
    
    async def _calculate_volume(
        self, 
        pair: str, 
        side: str, 
        amount: float,
        from_currency: str
    ) -> float:
        """
        Calculate the volume to trade.
        
        For buy orders: volume is in base currency (what we're buying)
        For sell orders: volume is in base currency (what we're selling)
        """
        try:
            ticker = await self.kraken_client.get_ticker(pair)
            pair_data = list(ticker.values())[0] if ticker else {}
            
            if side == 'buy':
                # We're buying base currency with quote currency
                # volume = amount_in_quote / ask_price
                ask_price = float(pair_data.get('a', [0])[0])
                if ask_price <= 0:
                    return 0
                volume = amount / ask_price
            else:
                # We're selling base currency for quote currency
                # volume = amount_in_base
                # If amount is in quote, convert: volume = amount_in_quote / bid_price
                bid_price = float(pair_data.get('b', [0])[0])
                if bid_price <= 0:
                    return 0
                # Assume amount is already in base currency for sells
                volume = amount
            
            return volume
            
        except Exception as e:
            logger.error(f"Error calculating volume: {e}")
            return 0
    
    async def _place_order(self, pair: str, side: str, volume: float) -> Dict[str, Any]:
        """Place a market order on Kraken"""
        try:
            data = {
                "pair": pair,
                "type": side,
                "ordertype": "market",
                "volume": str(volume),
            }
            
            result = await self.kraken_client._private_request("AddOrder", data)
            
            if result and result.get('txid'):
                order_id = result['txid'][0] if isinstance(result['txid'], list) else result['txid']
                logger.info(f"    Order placed: {order_id}")
                return {
                    'success': True,
                    'order_id': order_id,
                }
            else:
                return {
                    'success': False,
                    'error': 'No order ID returned',
                }
                
        except Exception as e:
            return {
                'success': False,
                'error': str(e),
            }
    
    async def _wait_for_fill(self, order_id: str, timeout_seconds: int) -> Dict[str, Any]:
        """Wait for an order to fill"""
        start_time = datetime.utcnow()
        
        while True:
            elapsed = (datetime.utcnow() - start_time).total_seconds()
            if elapsed > timeout_seconds:
                return {
                    'filled': False,
                    'error': f'Timeout after {timeout_seconds}s',
                }
            
            try:
                # Query order status
                result = await self.kraken_client._private_request(
                    "QueryOrders",
                    {"txid": order_id}
                )
                
                if result and order_id in result:
                    order_info = result[order_id]
                    status = order_info.get('status')
                    
                    if status == 'closed':
                        # Order filled
                        return {
                            'filled': True,
                            'price': float(order_info.get('price', 0)),
                            'volume': float(order_info.get('vol_exec', 0)),
                            'fee': float(order_info.get('fee', 0)),
                            'fee_currency': order_info.get('fee_currency', 'USD'),
                        }
                    elif status == 'canceled' or status == 'expired':
                        return {
                            'filled': False,
                            'error': f'Order {status}',
                        }
                
            except Exception as e:
                logger.warning(f"Error checking order status: {e}")
            
            # Wait before next check
            await asyncio.sleep(0.5)
    
    async def _cancel_order(self, order_id: str) -> bool:
        """Cancel an order"""
        try:
            await self.kraken_client._private_request(
                "CancelOrder",
                {"txid": order_id}
            )
            logger.info(f"    Canceled order: {order_id}")
            return True
        except Exception as e:
            logger.warning(f"Failed to cancel order {order_id}: {e}")
            return False
    
    async def _save_trade(self, execution: TradeExecution, opportunity_profit_pct: float):
        """Save trade to database"""
        db = self._get_db()
        try:
            from app.models.live_trading import LiveTrade
            
            trade = LiveTrade(
                trade_id=execution.trade_id,
                path=execution.path,
                legs=execution.legs,
                amount_in=execution.amount_in,
                amount_out=execution.amount_out,
                profit_loss=execution.profit_loss,
                profit_loss_pct=execution.profit_loss_pct,
                status=execution.status,
                current_leg=execution.current_leg,
                error_message=execution.error_message,
                held_currency=execution.held_currency,
                held_amount=execution.held_amount,
                order_ids=execution.order_ids,
                leg_fills=[leg.to_dict() for leg in execution.leg_executions],
                started_at=execution.started_at,
                completed_at=execution.completed_at,
                total_execution_ms=execution.total_execution_ms,
                opportunity_profit_pct=opportunity_profit_pct,
            )
            
            db.add(trade)
            db.commit()
            
            logger.debug(f"Saved trade {execution.trade_id} to database")
            
        except Exception as e:
            db.rollback()
            logger.error(f"Failed to save trade to database: {e}")
        finally:
            db.close()
