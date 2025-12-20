"""
Kraken Authenticated API Client
Supports both shadow mode (logging only) and live trading
"""
import hashlib
import hmac
import base64
import time
import urllib.parse
from typing import Optional, Dict, Any, List
from dataclasses import dataclass
from datetime import datetime
import httpx
import asyncio
from enum import Enum

class TradingMode(Enum):
    SHADOW = "shadow"  # Log trades but don't execute
    LIVE = "live"      # Actually execute trades

@dataclass
class OrderRequest:
    """Represents an order to be placed"""
    pair: str           # e.g., "XBTUSD"
    side: str           # "buy" or "sell"
    order_type: str     # "market" or "limit"
    volume: float       # Amount in base currency
    price: Optional[float] = None  # Required for limit orders
    
@dataclass
class OrderResult:
    """Result of an order attempt"""
    success: bool
    order_id: Optional[str] = None
    executed_price: Optional[float] = None
    executed_volume: Optional[float] = None
    fees: Optional[float] = None
    error: Optional[str] = None
    was_shadow: bool = True  # True if this was shadow mode (not actually executed)
    timestamp: str = ""
    
@dataclass
class ShadowTradeLog:
    """Log entry for shadow mode trades"""
    timestamp: str
    path: str
    orders: List[Dict[str, Any]]
    expected_profit_pct: float
    slippage_pct: float
    would_have_executed: bool
    reason_if_not: Optional[str] = None
    market_conditions: Optional[Dict[str, Any]] = None

class KrakenClient:
    """
    Authenticated Kraken API client with shadow mode support
    """
    
    BASE_URL = "https://api.kraken.com"
    
    def __init__(
        self,
        api_key: str,
        private_key: str,
        mode: TradingMode = TradingMode.SHADOW,
        max_loss_usd: float = 30.0,
    ):
        self.api_key = api_key
        self.private_key = private_key
        self.mode = mode
        self.max_loss_usd = max_loss_usd
        
        # Track shadow trading stats
        self.shadow_trades: List[ShadowTradeLog] = []
        self.total_shadow_profit: float = 0.0
        self.total_shadow_trades: int = 0
        
        # Live trading safety
        self.total_live_loss: float = 0.0
        self.live_trading_enabled: bool = False
        
        # HTTP client
        self._client: Optional[httpx.AsyncClient] = None
        
    async def _get_client(self) -> httpx.AsyncClient:
        """Get or create async HTTP client"""
        if self._client is None or self._client.is_closed:
            self._client = httpx.AsyncClient(timeout=30.0)
        return self._client
        
    async def close(self):
        """Close HTTP client"""
        if self._client and not self._client.is_closed:
            await self._client.aclose()
            
    def _generate_signature(self, url_path: str, data: Dict[str, Any], nonce: str) -> str:
        """Generate Kraken API signature"""
        post_data = urllib.parse.urlencode(data)
        encoded = (nonce + post_data).encode()
        message = url_path.encode() + hashlib.sha256(encoded).digest()
        
        mac = hmac.new(
            base64.b64decode(self.private_key),
            message,
            hashlib.sha512
        )
        return base64.b64encode(mac.digest()).decode()
        
    async def _private_request(
        self, 
        endpoint: str, 
        data: Optional[Dict[str, Any]] = None
    ) -> Dict[str, Any]:
        """Make authenticated request to Kraken API"""
        if data is None:
            data = {}
            
        url_path = f"/0/private/{endpoint}"
        url = f"{self.BASE_URL}{url_path}"
        
        nonce = str(int(time.time() * 1000))
        data["nonce"] = nonce
        
        signature = self._generate_signature(url_path, data, nonce)
        
        headers = {
            "API-Key": self.api_key,
            "API-Sign": signature,
            "Content-Type": "application/x-www-form-urlencoded",
        }
        
        client = await self._get_client()
        response = await client.post(url, data=data, headers=headers)
        result = response.json()
        
        if result.get("error"):
            raise Exception(f"Kraken API error: {result['error']}")
            
        return result.get("result", {})
        
    async def _public_request(self, endpoint: str, params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
        """Make public request to Kraken API"""
        url = f"{self.BASE_URL}/0/public/{endpoint}"
        
        client = await self._get_client()
        response = await client.get(url, params=params)
        result = response.json()
        
        if result.get("error"):
            raise Exception(f"Kraken API error: {result['error']}")
            
        return result.get("result", {})
        
    # ========================================
    # Account Information (works in both modes)
    # ========================================
    
    async def get_balance(self) -> Dict[str, float]:
        """Get account balances"""
        result = await self._private_request("Balance")
        return {k: float(v) for k, v in result.items()}
        
    async def get_trade_balance(self, asset: str = "ZUSD") -> Dict[str, float]:
        """Get trade balance info"""
        result = await self._private_request("TradeBalance", {"asset": asset})
        return {k: float(v) if v else 0.0 for k, v in result.items()}
        
    async def get_open_orders(self) -> Dict[str, Any]:
        """Get open orders"""
        return await self._private_request("OpenOrders")
        
    async def get_closed_orders(self) -> Dict[str, Any]:
        """Get closed orders"""
        return await self._private_request("ClosedOrders")
        
    async def get_ticker(self, pair: str) -> Dict[str, Any]:
        """Get current ticker for a pair"""
        result = await self._public_request("Ticker", {"pair": pair})
        return result
    
    async def get_order_book(self, pair: str, depth: int = 25) -> Dict[str, Any]:
        """
        Get order book for a pair.
        Returns bids and asks with price and volume.
        """
        result = await self._public_request("Depth", {"pair": pair, "count": depth})
        # Result format: {pair_name: {asks: [[price, volume, timestamp], ...], bids: [...]}}
        if result:
            pair_data = list(result.values())[0] if result else {}
            return {
                "bids": [[float(b[0]), float(b[1])] for b in pair_data.get("bids", [])],
                "asks": [[float(a[0]), float(a[1])] for a in pair_data.get("asks", [])],
            }
        return {"bids": [], "asks": []}
    
    async def get_trade_fees(self, pair: str = None) -> Dict[str, Any]:
        """
        Get actual taker/maker fees from Kraken account.
        Returns your fee tier based on 30-day volume.
        """
        try:
            data = {}
            if pair:
                data["pair"] = pair
            
            result = await self._private_request("TradeVolume", data)
            
            # Extract fee info
            fees_info = {
                "currency": result.get("currency", "ZUSD"),
                "volume": float(result.get("volume", 0)),
                "fees": {},
                "fees_maker": {},
            }
            
            # Parse per-pair fees
            for pair_name, fee_data in result.get("fees", {}).items():
                fees_info["fees"][pair_name] = {
                    "fee": float(fee_data.get("fee", 0.26)),
                    "min_fee": float(fee_data.get("minfee", 0.10)),
                    "max_fee": float(fee_data.get("maxfee", 0.26)),
                }
            
            for pair_name, fee_data in result.get("fees_maker", {}).items():
                fees_info["fees_maker"][pair_name] = {
                    "fee": float(fee_data.get("fee", 0.16)),
                    "min_fee": float(fee_data.get("minfee", 0.0)),
                    "max_fee": float(fee_data.get("maxfee", 0.16)),
                }
            
            return fees_info
            
        except Exception as e:
            # Fallback to default fees if API fails
            return {
                "currency": "ZUSD",
                "volume": 0,
                "fees": {},
                "fees_maker": {},
                "error": str(e),
                "default_taker_fee": 0.26,
            }
        
    # ========================================
    # Order Execution
    # ========================================
    
    async def place_order(self, order: OrderRequest) -> OrderResult:
        """
        Place an order - behavior depends on mode:
        - SHADOW: Log the order but don't execute
        - LIVE: Actually place the order (with safety checks)
        """
        timestamp = datetime.utcnow().isoformat()
        
        if self.mode == TradingMode.SHADOW:
            return await self._shadow_order(order, timestamp)
        else:
            return await self._live_order(order, timestamp)
            
    async def _shadow_order(self, order: OrderRequest, timestamp: str) -> OrderResult:
        """Log what would have been traded without executing"""
        # Get current market price for logging
        try:
            ticker = await self.get_ticker(order.pair)
            pair_data = list(ticker.values())[0] if ticker else {}
            current_bid = float(pair_data.get("b", [0])[0]) if pair_data else 0
            current_ask = float(pair_data.get("a", [0])[0]) if pair_data else 0
            
            # Simulate execution price
            simulated_price = current_ask if order.side == "buy" else current_bid
            simulated_fees = order.volume * simulated_price * 0.0026  # 0.26% taker fee
            
        except Exception as e:
            simulated_price = 0
            simulated_fees = 0
            
        return OrderResult(
            success=True,
            order_id=f"SHADOW-{int(time.time()*1000)}",
            executed_price=simulated_price,
            executed_volume=order.volume,
            fees=simulated_fees,
            error=None,
            was_shadow=True,
            timestamp=timestamp,
        )
        
    async def _live_order(self, order: OrderRequest, timestamp: str) -> OrderResult:
        """Actually execute an order with safety checks"""
        # Safety check: max loss limit
        if self.total_live_loss >= self.max_loss_usd:
            return OrderResult(
                success=False,
                error=f"Max loss limit reached (${self.max_loss_usd}). Trading disabled.",
                was_shadow=False,
                timestamp=timestamp,
            )
            
        # Safety check: live trading must be explicitly enabled
        if not self.live_trading_enabled:
            return OrderResult(
                success=False,
                error="Live trading not enabled. Call enable_live_trading() first.",
                was_shadow=False,
                timestamp=timestamp,
            )
            
        try:
            data = {
                "pair": order.pair,
                "type": order.side,
                "ordertype": order.order_type,
                "volume": str(order.volume),
            }
            
            if order.price and order.order_type == "limit":
                data["price"] = str(order.price)
                
            result = await self._private_request("AddOrder", data)
            
            order_id = result.get("txid", [None])[0]
            
            return OrderResult(
                success=True,
                order_id=order_id,
                executed_volume=order.volume,
                was_shadow=False,
                timestamp=timestamp,
            )
            
        except Exception as e:
            return OrderResult(
                success=False,
                error=str(e),
                was_shadow=False,
                timestamp=timestamp,
            )
            
    # ========================================
    # Shadow Mode Trading
    # ========================================
    
    async def execute_shadow_arbitrage(
        self,
        path: str,
        orders: List[OrderRequest],
        expected_profit_pct: float,
        slippage_pct: float,
        trade_amount_usd: float,
    ) -> ShadowTradeLog:
        """
        Execute a complete arbitrage cycle in shadow mode.
        Logs all orders that would have been placed.
        """
        timestamp = datetime.utcnow().isoformat()
        
        # Get current market conditions for each pair
        market_conditions = {}
        order_details = []
        
        for order in orders:
            try:
                ticker = await self.get_ticker(order.pair)
                pair_data = list(ticker.values())[0] if ticker else {}
                market_conditions[order.pair] = {
                    "bid": float(pair_data.get("b", [0])[0]),
                    "ask": float(pair_data.get("a", [0])[0]),
                    "volume_24h": float(pair_data.get("v", [0, 0])[1]),
                }
            except:
                market_conditions[order.pair] = {"error": "Failed to fetch"}
                
            order_details.append({
                "pair": order.pair,
                "side": order.side,
                "type": order.order_type,
                "volume": order.volume,
                "price": order.price,
            })
            
        # Determine if trade would have been profitable
        actual_profit_pct = expected_profit_pct - slippage_pct
        would_execute = actual_profit_pct > 0
        
        log_entry = ShadowTradeLog(
            timestamp=timestamp,
            path=path,
            orders=order_details,
            expected_profit_pct=expected_profit_pct,
            slippage_pct=slippage_pct,
            would_have_executed=would_execute,
            reason_if_not="Negative profit after slippage" if not would_execute else None,
            market_conditions=market_conditions,
        )
        
        # Update shadow stats
        self.shadow_trades.append(log_entry)
        self.total_shadow_trades += 1
        if would_execute:
            self.total_shadow_profit += trade_amount_usd * (actual_profit_pct / 100)
            
        # Keep only last 1000 shadow trades
        if len(self.shadow_trades) > 1000:
            self.shadow_trades = self.shadow_trades[-1000:]
            
        return log_entry
        
    # ========================================
    # Mode Control
    # ========================================
    
    def enable_live_trading(self, confirm: bool = False):
        """
        Enable live trading. Requires explicit confirmation.
        """
        if not confirm:
            raise ValueError(
                "Live trading requires explicit confirmation. "
                "Call enable_live_trading(confirm=True) to enable."
            )
        self.live_trading_enabled = True
        self.mode = TradingMode.LIVE
        
    def disable_live_trading(self):
        """Disable live trading and switch to shadow mode"""
        self.live_trading_enabled = False
        self.mode = TradingMode.SHADOW
        
    def get_mode(self) -> str:
        """Get current trading mode"""
        return self.mode.value
        
    def get_shadow_stats(self) -> Dict[str, Any]:
        """Get shadow trading statistics"""
        return {
            "mode": self.mode.value,
            "total_shadow_trades": self.total_shadow_trades,
            "total_shadow_profit": self.total_shadow_profit,
            "recent_trades": len(self.shadow_trades),
            "max_loss_usd": self.max_loss_usd,
            "live_trading_enabled": self.live_trading_enabled,
            "total_live_loss": self.total_live_loss,
        }
        
    def get_recent_shadow_trades(self, limit: int = 50) -> List[Dict[str, Any]]:
        """Get recent shadow trades"""
        trades = self.shadow_trades[-limit:]
        return [
            {
                "timestamp": t.timestamp,
                "path": t.path,
                "orders": t.orders,
                "expected_profit_pct": t.expected_profit_pct,
                "slippage_pct": t.slippage_pct,
                "would_have_executed": t.would_have_executed,
                "reason_if_not": t.reason_if_not,
            }
            for t in reversed(trades)
        ]


# Singleton instance
_kraken_client: Optional[KrakenClient] = None

def get_kraken_client() -> Optional[KrakenClient]:
    """Get the global Kraken client instance"""
    return _kraken_client

def initialize_kraken_client(
    api_key: str,
    private_key: str,
    shadow_mode: bool = True,
    max_loss_usd: float = 30.0,
) -> KrakenClient:
    """Initialize the global Kraken client"""
    global _kraken_client
    
    mode = TradingMode.SHADOW if shadow_mode else TradingMode.LIVE
    _kraken_client = KrakenClient(
        api_key=api_key,
        private_key=private_key,
        mode=mode,
        max_loss_usd=max_loss_usd,
    )
    
    return _kraken_client
