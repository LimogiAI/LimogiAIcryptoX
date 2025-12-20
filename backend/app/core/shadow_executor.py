"""
Shadow Executor - Compares paper trading against REAL Kraken market data
Fetches live order books and calculates what actual execution would look like
"""
import asyncio
from typing import Optional, Dict, Any, List, Tuple
from datetime import datetime
from dataclasses import dataclass
import json

from app.core.kraken_client import (
    KrakenClient, 
    get_kraken_client,
    OrderRequest,
    ShadowTradeLog,
)


# Mapping from our currency names to Kraken format
CURRENCY_MAP = {
    "BTC": "XBT",
    "DOGE": "XDG",
}

# Common quote currencies (used to determine buy vs sell)
QUOTE_CURRENCIES = {"USD", "USDT", "EUR", "ZUSD", "ZEUR", "GBP", "ZGBP", "CAD", "ZCAD", "JPY", "ZJPY"}


@dataclass
class ShadowExecutionResult:
    """Result of a shadow execution attempt"""
    success: bool
    path: str
    legs: int
    trade_amount: float
    paper_profit_pct: float       # What paper trading calculated
    shadow_profit_pct: float      # What live market data shows
    difference_pct: float         # Gap between paper and reality
    latency_ms: float             # Time to fetch live data
    leg_details: List[Dict]       # Details for each leg
    market_snapshot: Dict[str, Any]
    would_have_profited: bool
    timestamp: datetime = None
    reason: Optional[str] = None
    
    def __post_init__(self):
        if self.timestamp is None:
            self.timestamp = datetime.utcnow()


class ShadowExecutor:
    """
    Executes trades in shadow mode by fetching LIVE order books
    from Kraken and calculating real execution profit.
    """
    
    def __init__(self, kraken_client: Optional[KrakenClient] = None, db_session_factory=None):
        self.client = kraken_client or get_kraken_client()
        self.db_session_factory = db_session_factory
        self.execution_history: List[ShadowExecutionResult] = []
        self.total_paper_profit: float = 0.0
        self.total_shadow_profit: float = 0.0
        self.total_executions: int = 0
        self.profitable_count: int = 0
        
        # Detailed trade history
        self.detailed_trades: List[Dict] = []
        
    async def _get_taker_fee(self, pair: str = None) -> float:
        """Fetch actual taker fee from Kraken API"""
        if not self.client:
            return 0.26  # Default fallback
        
        try:
            fee_info = await self.client.get_trade_fees(pair)
            
            # If we got specific pair fee
            if pair and fee_info.get("fees", {}).get(pair):
                return fee_info["fees"][pair]["fee"] / 100  # Convert to decimal
            
            # Otherwise try to get any fee from the response
            for pair_name, fee_data in fee_info.get("fees", {}).items():
                return fee_data["fee"] / 100  # Convert to decimal
            
            # Fallback
            return fee_info.get("default_taker_fee", 0.26) / 100
            
        except Exception as e:
            print(f"Error fetching fee: {e}")
            return 0.0026  # Default 0.26%
        
    def _convert_to_kraken_pair(self, base: str, quote: str) -> str:
        """Convert currency pair to Kraken format"""
        # Apply currency mapping
        kraken_base = CURRENCY_MAP.get(base, base)
        kraken_quote = CURRENCY_MAP.get(quote, quote)
        
        # Try different formats Kraken uses
        # Most pairs are just concatenated: XBTUSD, ETHUSD
        return f"{kraken_base}{kraken_quote}"
    
    def _parse_path(self, path: str) -> List[str]:
        """Parse path string into list of currencies"""
        # Path format: "USD → BTC → ETH → USD" or "USD->BTC->ETH->USD"
        if "→" in path:
            return [c.strip() for c in path.split("→")]
        elif "->" in path:
            return [c.strip() for c in path.split("->")]
        else:
            return [c.strip() for c in path.split()]
    
    def _determine_pair_and_side(self, from_currency: str, to_currency: str) -> Tuple[str, str, bool]:
        """
        Determine the Kraken pair name and whether to buy or sell.
        Returns: (kraken_pair, side, is_reversed)
        
        Example:
        - USD -> BTC: pair=XBTUSD, side=buy (buying BTC with USD)
        - BTC -> ETH: pair=ETHXBT, side=buy (buying ETH with BTC) or XBTETH, side=sell
        - ETH -> USD: pair=ETHUSD, side=sell (selling ETH for USD)
        """
        # Normalize currencies
        from_norm = CURRENCY_MAP.get(from_currency, from_currency)
        to_norm = CURRENCY_MAP.get(to_currency, to_currency)
        
        # If from_currency is a quote currency, we're buying
        if from_currency in QUOTE_CURRENCIES or from_currency.startswith("Z"):
            # Buying to_currency with from_currency
            # Pair is usually TO/FROM (e.g., XBTUSD for buying BTC with USD)
            pair = f"{to_norm}{from_norm}"
            return pair, "buy", False
        else:
            # Selling from_currency for to_currency
            # Pair is FROM/TO if to_currency is quote
            if to_currency in QUOTE_CURRENCIES or to_currency.startswith("Z"):
                pair = f"{from_norm}{to_norm}"
                return pair, "sell", False
            else:
                # Both are base currencies - need to figure out which pair exists
                # Try FROM/TO first (selling FROM for TO)
                pair = f"{from_norm}{to_norm}"
                return pair, "sell", False
    
    async def _calculate_leg_execution(
        self,
        from_currency: str,
        to_currency: str,
        amount: float,
        fee_rate: float,
        is_amount_in_from: bool = True,
    ) -> Dict[str, Any]:
        """
        Calculate what executing this leg would look like with live order book.
        
        Args:
            from_currency: Currency we're converting from
            to_currency: Currency we're converting to
            amount: Amount (in from_currency if is_amount_in_from, else in to_currency)
            fee_rate: Taker fee rate from Kraken API
            is_amount_in_from: Whether amount is in from_currency
            
        Returns:
            Dict with execution details including slippage
        """
        pair, side, is_reversed = self._determine_pair_and_side(from_currency, to_currency)
        
        try:
            # Fetch live order book from Kraken
            order_book = await self.client.get_order_book(pair, depth=25)
            
            if not order_book or (not order_book.get("bids") and not order_book.get("asks")):
                # Try alternative pair format
                alt_pair = pair.replace("XBT", "BTC").replace("XDG", "DOGE")
                if alt_pair != pair:
                    order_book = await self.client.get_order_book(alt_pair, depth=25)
                    if order_book and (order_book.get("bids") or order_book.get("asks")):
                        pair = alt_pair
            
            if not order_book or (not order_book.get("bids") and not order_book.get("asks")):
                return {
                    "pair": pair,
                    "side": side,
                    "success": False,
                    "error": f"No order book data for {pair}",
                    "slippage_pct": 0,
                    "executed_amount": 0,
                    "received_amount": 0,
                }
            
            # Use asks for buys, bids for sells
            book_side = order_book["asks"] if side == "buy" else order_book["bids"]
            
            if not book_side:
                return {
                    "pair": pair,
                    "side": side,
                    "success": False,
                    "error": f"Empty {side} side of order book",
                    "slippage_pct": 0,
                    "executed_amount": 0,
                    "received_amount": 0,
                }
            
            # Get best price (top of book)
            best_price = book_side[0][0]
            
            # Calculate how much we need to execute
            if side == "buy":
                # We have quote currency (e.g., USD), buying base (e.g., BTC)
                # amount is in USD, we need to buy amount_usd worth of BTC
                amount_to_fill = amount / best_price if is_amount_in_from else amount
            else:
                # We have base currency (e.g., BTC), selling for quote (e.g., USD)
                # amount is the base currency amount we're selling
                amount_to_fill = amount if is_amount_in_from else amount / best_price
            
            # Walk through order book and calculate average execution price
            remaining = amount_to_fill
            total_cost = 0.0
            total_filled = 0.0
            levels_used = 0
            
            for price, volume in book_side:
                if remaining <= 0:
                    break
                    
                fill_amount = min(remaining, volume)
                total_cost += fill_amount * price
                total_filled += fill_amount
                remaining -= fill_amount
                levels_used += 1
            
            if total_filled == 0:
                return {
                    "pair": pair,
                    "side": side,
                    "success": False,
                    "error": "Insufficient liquidity",
                    "slippage_pct": 0,
                    "executed_amount": 0,
                    "received_amount": 0,
                }
            
            # Calculate average execution price
            avg_price = total_cost / total_filled
            
            # Calculate slippage (difference from best price)
            if side == "buy":
                slippage_pct = ((avg_price - best_price) / best_price) * 100
            else:
                slippage_pct = ((best_price - avg_price) / best_price) * 100
            
            # Calculate fee for this leg
            fee_pct = fee_rate * 100  # Convert to percentage
            
            # Calculate what we receive after execution (slippage already factored in avg_price)
            if side == "buy":
                # Bought base currency
                received_before_fee = total_filled
                fee_amount = total_filled * fee_rate
                received = total_filled * (1 - fee_rate)
            else:
                # Sold for quote currency
                received_before_fee = total_cost
                fee_amount = total_cost * fee_rate
                received = total_cost * (1 - fee_rate)
            
            return {
                "pair": pair,
                "side": side,
                "success": True,
                "best_price": best_price,
                "avg_price": avg_price,
                "slippage_pct": slippage_pct,
                "fee_pct": fee_pct,
                "fee_amount": fee_amount,
                "executed_amount": total_filled,
                "received_before_fee": received_before_fee,
                "received_amount": received,
                "levels_used": levels_used,
                "book_depth": len(book_side),
            }
            
        except Exception as e:
            return {
                "pair": pair,
                "side": side,
                "success": False,
                "error": str(e),
                "slippage_pct": 0,
                "executed_amount": 0,
                "received_amount": 0,
            }
    
    async def execute_shadow(
        self,
        path: str,
        trade_amount_usd: float,
        paper_expected_profit_pct: float,
        paper_slippage_pct: float,
    ) -> ShadowExecutionResult:
        """
        Execute a shadow trade by fetching LIVE order books from Kraken
        and calculating what actual execution would look like.
        
        This is the key function that validates paper trading accuracy.
        """
        start_time = datetime.utcnow()
        
        if not self.client:
            return ShadowExecutionResult(
                success=False,
                path=path,
                legs=0,
                trade_amount=trade_amount_usd,
                paper_profit_pct=paper_expected_profit_pct,
                shadow_profit_pct=0.0,
                difference_pct=0.0,
                latency_ms=0.0,
                leg_details=[],
                market_snapshot={},
                would_have_profited=False,
                reason="Kraken client not initialized",
            )
        
        try:
            # Parse path into currencies
            currencies = self._parse_path(path)
            num_legs = len(currencies) - 1
            
            if num_legs < 2:
                return ShadowExecutionResult(
                    success=False,
                    path=path,
                    legs=num_legs,
                    trade_amount=trade_amount_usd,
                    paper_profit_pct=paper_expected_profit_pct,
                    shadow_profit_pct=0.0,
                    difference_pct=0.0,
                    latency_ms=0.0,
                    leg_details=[],
                    market_snapshot={},
                    would_have_profited=False,
                    reason=f"Invalid path: {path}",
                )
            
            # Fetch real taker fee from Kraken API
            taker_fee_rate = await self._get_taker_fee()
            
            # Execute each leg and track cumulative result
            current_amount = trade_amount_usd
            leg_details = []
            total_slippage_pct = 0.0
            total_slippage_usd = 0.0
            total_fee_pct = 0.0
            total_fee_usd = 0.0
            all_successful = True
            market_snapshot = {}
            
            for i in range(num_legs):
                from_curr = currencies[i]
                to_curr = currencies[i + 1]
                
                leg_result = await self._calculate_leg_execution(
                    from_currency=from_curr,
                    to_currency=to_curr,
                    amount=current_amount,
                    fee_rate=taker_fee_rate,
                    is_amount_in_from=True,
                )
                
                leg_details.append({
                    "leg": i + 1,
                    "from": from_curr,
                    "to": to_curr,
                    "input_amount": current_amount,
                    **leg_result,
                })
                
                # Store market data and accumulate totals
                if leg_result.get("success"):
                    market_snapshot[leg_result["pair"]] = {
                        "best_price": leg_result.get("best_price"),
                        "avg_price": leg_result.get("avg_price"),
                        "slippage_pct": leg_result.get("slippage_pct"),
                        "fee_pct": leg_result.get("fee_pct"),
                        "book_depth": leg_result.get("book_depth"),
                    }
                    
                    # Accumulate slippage and fees
                    total_slippage_pct += leg_result.get("slippage_pct", 0)
                    total_fee_pct += leg_result.get("fee_pct", 0)
                    total_fee_usd += leg_result.get("fee_amount", 0)
                    
                    # Update running total
                    current_amount = leg_result.get("received_amount", 0)
                else:
                    all_successful = False
                    break
            
            end_time = datetime.utcnow()
            latency_ms = (end_time - start_time).total_seconds() * 1000
            
            # Calculate slippage in USD
            total_slippage_usd = trade_amount_usd * (total_slippage_pct / 100)
            
            if not all_successful:
                # Some leg failed - can't complete arbitrage
                failed_leg = next((l for l in leg_details if not l.get("success")), None)
                
                return ShadowExecutionResult(
                    success=False,
                    path=path,
                    legs=num_legs,
                    trade_amount=trade_amount_usd,
                    paper_profit_pct=paper_expected_profit_pct,
                    shadow_profit_pct=0.0,
                    difference_pct=paper_expected_profit_pct,  # Full gap
                    latency_ms=latency_ms,
                    leg_details=leg_details,
                    market_snapshot=market_snapshot,
                    would_have_profited=False,
                    reason=failed_leg.get("error", "Unknown leg failure") if failed_leg else "Unknown",
                )
            
            # Calculate final profit
            # We started with trade_amount_usd, ended with current_amount
            shadow_profit_pct = ((current_amount - trade_amount_usd) / trade_amount_usd) * 100
            shadow_profit_usd = current_amount - trade_amount_usd
            gross_profit_pct = shadow_profit_pct + total_fee_pct + total_slippage_pct  # Before deductions
            difference_pct = paper_expected_profit_pct - shadow_profit_pct
            would_profit = shadow_profit_pct > 0
            
            result = ShadowExecutionResult(
                success=True,
                path=path,
                legs=num_legs,
                trade_amount=trade_amount_usd,
                paper_profit_pct=paper_expected_profit_pct,
                shadow_profit_pct=shadow_profit_pct,
                difference_pct=difference_pct,
                latency_ms=latency_ms,
                leg_details=leg_details,
                market_snapshot=market_snapshot,
                would_have_profited=would_profit,
                timestamp=end_time,
            )
            
            # Update stats
            self.execution_history.append(result)
            self.total_executions += 1
            self.total_paper_profit += trade_amount_usd * (paper_expected_profit_pct / 100)
            self.total_shadow_profit += trade_amount_usd * (shadow_profit_pct / 100)
            if would_profit:
                self.profitable_count += 1
            
            # Save to database (both tables)
            await self._save_to_database(result)
            
            # Save detailed trade to new table
            await self._save_detailed_trade(
                timestamp=end_time,
                path=path,
                legs=num_legs,
                amount=trade_amount_usd,
                taker_fee_pct=total_fee_pct,
                taker_fee_usd=total_fee_usd,
                total_slippage_pct=total_slippage_pct,
                total_slippage_usd=total_slippage_usd,
                gross_profit_pct=gross_profit_pct,
                net_profit_pct=shadow_profit_pct,
                net_profit_usd=shadow_profit_usd,
                status="WIN" if would_profit else "LOSS",
                leg_details=leg_details,
            )
            
            # Keep only last 500 executions in memory
            if len(self.execution_history) > 500:
                self.execution_history = self.execution_history[-500:]
            
            return result
            
        except Exception as e:
            end_time = datetime.utcnow()
            latency_ms = (end_time - start_time).total_seconds() * 1000
            
            result = ShadowExecutionResult(
                success=False,
                path=path,
                legs=0,
                trade_amount=trade_amount_usd,
                paper_profit_pct=paper_expected_profit_pct,
                shadow_profit_pct=0.0,
                difference_pct=0.0,
                latency_ms=latency_ms,
                leg_details=[],
                market_snapshot={},
                would_have_profited=False,
                timestamp=end_time,
                reason=str(e),
            )
            
            await self._save_to_database(result)
            return result
    
    async def _save_to_database(self, result: ShadowExecutionResult):
        """Save shadow execution result to database"""
        if not self.db_session_factory:
            return
            
        try:
            from app.models.models import ShadowTrade
            
            db = self.db_session_factory()
            try:
                shadow_trade = ShadowTrade(
                    timestamp=result.timestamp,
                    path=result.path,
                    trade_amount=result.trade_amount,
                    paper_profit_pct=result.paper_profit_pct,
                    shadow_profit_pct=result.shadow_profit_pct,
                    difference_pct=result.difference_pct,
                    would_have_profited=result.would_have_profited,
                    latency_ms=result.latency_ms,
                    success=result.success,
                    reason=result.reason,
                    market_snapshot=result.market_snapshot if result.market_snapshot else None,
                )
                db.add(shadow_trade)
                db.commit()
            finally:
                db.close()
        except Exception as e:
            print(f"Failed to save shadow trade to database: {e}")
    
    async def _save_detailed_trade(
        self,
        timestamp: datetime,
        path: str,
        legs: int,
        amount: float,
        taker_fee_pct: float,
        taker_fee_usd: float,
        total_slippage_pct: float,
        total_slippage_usd: float,
        gross_profit_pct: float,
        net_profit_pct: float,
        net_profit_usd: float,
        status: str,
        leg_details: List[Dict],
    ):
        """Save detailed shadow trade to new table"""
        if not self.db_session_factory:
            return
            
        try:
            from app.models.models import ShadowTradeDetailed
            
            db = self.db_session_factory()
            try:
                detailed_trade = ShadowTradeDetailed(
                    timestamp=timestamp,
                    path=path,
                    legs=legs,
                    amount=amount,
                    taker_fee_pct=taker_fee_pct,
                    taker_fee_usd=taker_fee_usd,
                    total_slippage_pct=total_slippage_pct,
                    total_slippage_usd=total_slippage_usd,
                    gross_profit_pct=gross_profit_pct,
                    net_profit_pct=net_profit_pct,
                    net_profit_usd=net_profit_usd,
                    status=status,
                    leg_details=leg_details,
                )
                db.add(detailed_trade)
                db.commit()
                
                # Also keep in memory for quick access
                self.detailed_trades.append({
                    "timestamp": timestamp.isoformat(),
                    "path": path,
                    "legs": legs,
                    "amount": amount,
                    "taker_fee_pct": taker_fee_pct,
                    "taker_fee_usd": taker_fee_usd,
                    "total_slippage_pct": total_slippage_pct,
                    "total_slippage_usd": total_slippage_usd,
                    "gross_profit_pct": gross_profit_pct,
                    "net_profit_pct": net_profit_pct,
                    "net_profit_usd": net_profit_usd,
                    "status": status,
                    "leg_details": leg_details,
                })
                
                # Keep only last 500 in memory
                if len(self.detailed_trades) > 500:
                    self.detailed_trades = self.detailed_trades[-500:]
                    
            finally:
                db.close()
        except Exception as e:
            print(f"Failed to save detailed shadow trade: {e}")
    
    def get_detailed_trades(self, limit: int = 50) -> List[Dict]:
        """Get recent detailed shadow trades"""
        return self.detailed_trades[-limit:][::-1]  # Newest first
    
    def get_stats(self) -> Dict[str, Any]:
        """Get shadow execution statistics"""
        if self.total_executions == 0:
            return {
                "total_executions": 0,
                "profitable_count": 0,
                "win_rate": 0,
                "total_paper_profit": 0,
                "total_shadow_profit": 0,
                "profit_gap": 0,
                "avg_latency_ms": 0,
                "avg_difference_pct": 0,
            }
        
        win_rate = (self.profitable_count / self.total_executions * 100)
        avg_latency = sum(r.latency_ms for r in self.execution_history) / len(self.execution_history) if self.execution_history else 0
        avg_difference = sum(r.difference_pct for r in self.execution_history) / len(self.execution_history) if self.execution_history else 0
        
        return {
            "total_executions": self.total_executions,
            "profitable_count": self.profitable_count,
            "win_rate": win_rate,
            "total_paper_profit": self.total_paper_profit,
            "total_shadow_profit": self.total_shadow_profit,
            "profit_gap": self.total_paper_profit - self.total_shadow_profit,
            "avg_latency_ms": avg_latency,
            "avg_difference_pct": avg_difference,
        }
    
    def get_recent_executions(self, limit: int = 50) -> List[Dict[str, Any]]:
        """Get recent shadow executions"""
        executions = self.execution_history[-limit:]
        return [
            {
                "timestamp": e.timestamp.isoformat() if e.timestamp else None,
                "path": e.path,
                "legs": e.legs,
                "trade_amount": e.trade_amount,
                "paper_profit_pct": e.paper_profit_pct,
                "shadow_profit_pct": e.shadow_profit_pct,
                "difference_pct": e.difference_pct,
                "latency_ms": e.latency_ms,
                "would_have_profited": e.would_have_profited,
                "success": e.success,
                "reason": e.reason,
                "leg_details": e.leg_details,
            }
            for e in reversed(executions)
        ]
    
    def get_accuracy_report(self) -> Dict[str, Any]:
        """
        Generate accuracy report comparing paper vs shadow.
        This shows how accurate paper trading predictions are.
        """
        if not self.execution_history:
            return {"message": "No executions yet"}
        
        successful = [e for e in self.execution_history if e.success]
        
        if not successful:
            return {"message": "No successful shadow executions yet"}
        
        profitable_in_paper = sum(1 for e in successful if e.paper_profit_pct > 0)
        profitable_in_shadow = sum(1 for e in successful if e.would_have_profited)
        
        # False positives: paper said profitable, shadow said not
        false_positives = sum(
            1 for e in successful 
            if e.paper_profit_pct > 0 and not e.would_have_profited
        )
        
        # Calculate average and max differences
        differences = [e.difference_pct for e in successful]
        avg_diff = sum(differences) / len(differences)
        max_diff = max(differences)
        min_diff = min(differences)
        
        return {
            "total_samples": len(successful),
            "profitable_in_paper": profitable_in_paper,
            "profitable_in_shadow": profitable_in_shadow,
            "false_positive_count": false_positives,
            "false_positive_rate": (false_positives / profitable_in_paper * 100) if profitable_in_paper > 0 else 0,
            "avg_difference_pct": avg_diff,
            "max_difference_pct": max_diff,
            "min_difference_pct": min_diff,
            "avg_latency_ms": sum(e.latency_ms for e in successful) / len(successful),
            "paper_vs_reality_gap": self.total_paper_profit - self.total_shadow_profit,
            "paper_win_rate": (profitable_in_paper / len(successful) * 100) if successful else 0,
            "shadow_win_rate": (profitable_in_shadow / len(successful) * 100) if successful else 0,
        }


# Singleton instance
_shadow_executor: Optional[ShadowExecutor] = None

def get_shadow_executor() -> Optional[ShadowExecutor]:
    """Get the global shadow executor instance"""
    return _shadow_executor
    
def initialize_shadow_executor(kraken_client: Optional[KrakenClient] = None, db_session_factory=None) -> ShadowExecutor:
    """Initialize the global shadow executor"""
    global _shadow_executor
    _shadow_executor = ShadowExecutor(kraken_client, db_session_factory)
    return _shadow_executor
