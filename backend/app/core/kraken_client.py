"""
Kraken Authenticated API Client
For account information and fee queries only.
All trade execution is handled by the Rust WebSocket engine.
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


@dataclass
class OrderResult:
    """Result of an order attempt"""
    success: bool
    order_id: Optional[str] = None
    executed_price: Optional[float] = None
    executed_volume: Optional[float] = None
    fees: Optional[float] = None
    error: Optional[str] = None
    timestamp: str = ""


class KrakenClient:
    """
    Authenticated Kraken API client for account info and fee queries.

    NOTE: All trade execution is handled by the Rust WebSocket engine
    in rust_engine/src/executor.rs for maximum speed (~50ms vs ~500ms REST).
    This client is only used for:
    - Account balance queries
    - Fee tier queries
    - Order book queries (backup)
    """

    BASE_URL = "https://api.kraken.com"

    def __init__(
        self,
        api_key: str,
        private_key: str,
        max_loss_usd: float = 30.0,
    ):
        self.api_key = api_key
        self.private_key = private_key
        self.max_loss_usd = max_loss_usd

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
    # Account Information
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

        NOTE: Kraken requires pair parameter to return fee info.
        We use common pairs to sample fee tier.
        """
        try:
            # Kraken requires a pair to return fee info
            # Use common high-volume pairs to get fee tier
            sample_pairs = pair or "XXBTZUSD,XETHZUSD,XXBTZEUR"
            data = {"pair": sample_pairs}

            result = await self._private_request("TradeVolume", data)

            if result is None:
                return {
                    "currency": "ZUSD",
                    "volume": 0,
                    "fees": {},
                    "fees_maker": {},
                    "error": "No response from Kraken API",
                    "default_taker_fee": 0.26,
                }

            # Extract fee info
            fees_info = {
                "currency": result.get("currency", "ZUSD"),
                "volume": float(result.get("volume", 0)),
                "fees": {},
                "fees_maker": {},
            }

            # Parse per-pair fees (may be empty or dict)
            fees_dict = result.get("fees") or {}
            if isinstance(fees_dict, dict):
                for pair_name, fee_data in fees_dict.items():
                    if isinstance(fee_data, dict):
                        fees_info["fees"][pair_name] = {
                            "fee": float(fee_data.get("fee", 0.26)),
                            "min_fee": float(fee_data.get("minfee", 0.10)),
                            "max_fee": float(fee_data.get("maxfee", 0.26)),
                        }

            fees_maker_dict = result.get("fees_maker") or {}
            if isinstance(fees_maker_dict, dict):
                for pair_name, fee_data in fees_maker_dict.items():
                    if isinstance(fee_data, dict):
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


# Singleton instance
_kraken_client: Optional[KrakenClient] = None

def get_kraken_client() -> Optional[KrakenClient]:
    """Get the global Kraken client instance"""
    return _kraken_client

def initialize_kraken_client(
    api_key: str,
    private_key: str,
    max_loss_usd: float = 30.0,
) -> KrakenClient:
    """Initialize the global Kraken client"""
    global _kraken_client

    _kraken_client = KrakenClient(
        api_key=api_key,
        private_key=private_key,
        max_loss_usd=max_loss_usd,
    )

    return _kraken_client
