# KrakenCryptoX 🦑

Multi-Pair Arbitrage Opportunity Scanner for Kraken Exchange

## Overview

KrakenCryptoX monitors cryptocurrency trading pairs on Kraken exchange in real-time, detecting arbitrage opportunities by analyzing price discrepancies across multiple trading paths.

### Key Features

- **Real-time Price Monitoring** - WebSocket connection to Kraken for live price updates
- **Multi-Path Arbitrage Detection** - Finds profitable paths with 2-4+ legs
- **Automatic Fee Calculation** - Accounts for Kraken's trading fees (0.26% taker)
- **Graph-Based Analysis** - Models all currencies and pairs as a directed graph
- **Live Dashboard** - React frontend for monitoring opportunities
- **Historical Tracking** - PostgreSQL database stores all detected opportunities

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    KrakenCryptoX                            │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────┐     ┌─────────────┐     ┌─────────────┐   │
│  │   Kraken    │────▶│   Backend   │────▶│  Frontend   │   │
│  │  WebSocket  │     │   FastAPI   │     │    React    │   │
│  └─────────────┘     └─────────────┘     └─────────────┘   │
│                            │                               │
│                            ▼                               │
│                      ┌─────────────┐                       │
│                      │ PostgreSQL  │                       │
│                      └─────────────┘                       │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## Quick Start

### Prerequisites

- Docker & Docker Compose
- Git

### Installation

1. Clone and navigate to the project:
```bash
cd krakencryptox
```

2. Copy environment file:
```bash
cp .env.example .env
```

3. Start all services:
```bash
docker-compose up -d
```

4. Access the dashboard:
- **Frontend**: http://localhost:3000
- **API Docs**: http://localhost:8000/docs
- **API Health**: http://localhost:8000/health

### Stopping

```bash
docker-compose down
```

## Configuration

Edit `.env` file to customize:

| Variable | Default | Description |
|----------|---------|-------------|
| `POSTGRES_USER` | krakencryptox | Database user |
| `POSTGRES_PASSWORD` | krakencryptox123 | Database password |
| `KRAKEN_API_KEY` | (empty) | Optional - for private endpoints |
| `LOG_LEVEL` | INFO | Logging level |

### Trading Parameters

In `backend/app/core/config.py`:

| Parameter | Default | Description |
|-----------|---------|-------------|
| `fee_rate_taker` | 0.0026 | Kraken taker fee (0.26%) |
| `min_profit_threshold` | 0.001 | Min profit % to log (0.1%) |
| `alert_profit_threshold` | 0.003 | Profit % to alert (0.3%) |
| `max_path_legs` | 4 | Maximum trades in path |
| `min_pair_volume_24h` | 500000 | Min 24h volume filter |
| `base_trade_amount` | 10000 | Base amount for calculations |

## API Endpoints

### Scanner
- `GET /api/status` - Scanner status
- `POST /api/scan` - Trigger manual scan

### Opportunities
- `GET /api/opportunities` - List opportunities
- `GET /api/opportunities/best` - Best opportunities
- `GET /api/opportunities/{id}` - Opportunity details

### Graph
- `GET /api/graph/info` - Graph statistics
- `GET /api/graph/paths?start=USDT&end=USDT` - Find paths

### Prices
- `GET /api/prices/matrix` - Price matrix
- `GET /api/prices/live` - Live prices

### Statistics
- `GET /api/stats/summary` - Summary stats

### WebSocket
- `ws://localhost:8000/ws` - Real-time updates

## How It Works

### 1. Data Collection
- Fetches all trading pairs from Kraken REST API
- Subscribes to real-time price updates via WebSocket
- Updates internal graph structure on each price change

### 2. Graph Model
```
Currencies are NODES: BTC, ETH, SOL, USDT, USD, EUR...
Trading pairs are EDGES: BTC/USDT, ETH/BTC, SOL/ETH...

Each pair creates two directed edges:
- BTC → USDT (sell BTC at bid price)
- USDT → BTC (buy BTC at ask price)
```

### 3. Path Finding
- Uses NetworkX for graph algorithms
- Finds all simple paths/cycles up to N legs
- Calculates profit for each path including fees

### 4. Profit Calculation
```
For path: USDT → BTC → ETH → USDT

Start: $10,000 USDT

Leg 1: USDT → BTC
  Buy BTC at ask price
  Apply 0.26% fee
  
Leg 2: BTC → ETH  
  Sell BTC for ETH
  Apply 0.26% fee
  
Leg 3: ETH → USDT
  Sell ETH for USDT
  Apply 0.26% fee

End: $X USDT

Profit = (X - 10,000) / 10,000 * 100%
```

## Project Structure

```
krakencryptox/
├── docker-compose.yml
├── .env.example
├── README.md
├── backend/
│   ├── Dockerfile
│   ├── requirements.txt
│   └── app/
│       ├── main.py           # FastAPI app
│       ├── api/
│       │   ├── routes.py     # REST endpoints
│       │   └── websocket.py  # WebSocket handler
│       ├── core/
│       │   ├── config.py     # Settings
│       │   └── database.py   # DB connection
│       ├── models/
│       │   ├── models.py     # SQLAlchemy models
│       │   └── schemas.py    # Pydantic schemas
│       └── services/
│           ├── kraken_api.py # Kraken REST/WS
│           ├── graph_service.py  # Graph logic
│           └── scanner.py    # Main scanner
├── frontend/
│   ├── Dockerfile
│   ├── package.json
│   ├── public/
│   └── src/
│       ├── App.js
│       ├── components/
│       ├── hooks/
│       ├── services/
│       └── styles/
└── db/
    └── init.sql              # Database schema
```

## Understanding Results

### Opportunity Status

| Net Profit % | Status | Meaning |
|--------------|--------|---------|
| > 0.3% | 🟢 High Alert | Worth investigating |
| 0.1% - 0.3% | 🟡 Marginal | Covers fees, small profit |
| < 0.1% | 🔴 Below Threshold | Not logged |
| < 0% | ⚫ Loss | Fees exceed profit |

### Why Most Opportunities Show Losses

This is expected! Kraken markets are highly efficient:
- Professional traders and bots constantly arbitrage
- Fees (0.78% for 3 legs) eat most discrepancies
- The scanner shows market reality

### What We're Looking For

Rare moments when:
- Spread > 0.8% for 3-leg paths
- Usually during high volatility
- Or for less liquid pairs

## Development

### Run Backend Only
```bash
cd backend
pip install -r requirements.txt
uvicorn app.main:app --reload
```

### Run Frontend Only
```bash
cd frontend
npm install
npm start
```

### View Logs
```bash
docker-compose logs -f backend
```

## Disclaimer

This tool is for **educational and monitoring purposes only**.

- Not financial advice
- Past opportunities don't guarantee future profits
- Actual execution may differ due to slippage and timing
- Always do your own research

## License

MIT License

---

Built with ❤️ for crypto research
