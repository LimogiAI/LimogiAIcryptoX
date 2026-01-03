# Beeks Server Deployment Guide

Complete guide to deploy LimogiAICryptoX on Beeks Exchange Cloud for ultra-low-latency Kraken trading.

## Prerequisites

- [x] SSH access to Beeks server
- [x] Docker installed
- [x] docker-compose installed
- [ ] Kraken API credentials (API key + secret)

---

## Step 1: Clone the Repository

```bash
# Navigate to home directory
cd ~

# Clone the main branch
git clone -b main https://github.com/YOUR_USERNAME/LimogiAIcryptoX.git

# Enter project directory
cd LimogiAIcryptoX
```

---

## Step 2: Configure Environment Variables

### 2.1 Create Backend Environment File

```bash
cat > backend/.env << 'EOF'
# ============================================
# DATABASE CONFIGURATION
# ============================================
DATABASE_URL=postgresql://krakencryptox:krakencryptox123@db:5432/krakencryptox

# ============================================
# KRAKEN API CREDENTIALS (REQUIRED)
# Get from: https://www.kraken.com/u/security/api
# ============================================
KRAKEN_API_KEY=your_kraken_api_key_here
KRAKEN_API_SECRET=your_kraken_api_secret_here

# ============================================
# KRAKEN COLOCATION ENDPOINTS (BEEKS)
# These bypass public internet for sub-ms latency
# Source: https://docs.kraken.com/api/docs/guides/global-intro/
# ============================================
KRAKEN_WS_V2_PUBLIC=wss://colo-london.vip-ws.kraken.com/v2
KRAKEN_WS_V2_PRIVATE=wss://colo-london.vip-ws-auth.kraken.com/v2

# REST API (no colocation endpoint documented for spot)
KRAKEN_REST_URL=https://api.kraken.com

# ============================================
# SERVER CONFIGURATION
# ============================================
PORT=8000
RUST_LOG=info
EOF
```

### 2.2 Edit with Your Actual Kraken Credentials

```bash
nano backend/.env
```

Replace these placeholder values:
- `KRAKEN_API_KEY` → Your actual Kraken API key
- `KRAKEN_API_SECRET` → Your actual Kraken API secret

### 2.3 Create Frontend Environment File

```bash
cat > frontend/.env << 'EOF'
VITE_API_URL=http://localhost:8000
EOF
```

> **Remote Access:** If accessing dashboard from another machine, replace `localhost` with the Beeks server IP.

---

## Step 3: Build and Start Application

### 3.1 Build All Services

```bash
# Build Docker images (takes 5-10 minutes first time)
docker-compose build --no-cache
```

### 3.2 Start All Services

```bash
# Start in detached mode
docker-compose up -d
```

### 3.3 Verify Services Are Running

```bash
docker-compose ps
```

Expected output:
```
NAME                      STATUS    PORTS
limogiaicryptox-db        Up        5432/tcp
limogiaicryptox-backend   Up        0.0.0.0:8000->8000/tcp
limogiaicryptox-frontend  Up        0.0.0.0:3000->3000/tcp
```

### 3.4 Check Backend Logs

```bash
# View logs
docker-compose logs backend

# Follow logs in real-time
docker-compose logs -f backend
```

---

## Step 4: Verify Application

### 4.1 Test Backend Health

```bash
curl http://localhost:8000/api/health
```

Expected:
```json
{"status":"success","message":"OK"}
```

### 4.2 Verify Kraken Connection (Tests API Credentials)

```bash
curl http://localhost:8000/api/fees
```

Expected (if credentials correct):
```json
{
  "status": "success",
  "data": {
    "maker_fee": 0.0016,
    "taker_fee": 0.0026,
    "source": "live"
  }
}
```

**If you see an error, check your API credentials in backend/.env**

### 4.3 Access Dashboard

Open in browser:
```
http://YOUR_BEEKS_SERVER_IP:3000
```

---

## Step 5: Configure Trading Settings

Via the Dashboard, configure:

| Setting | Recommended Value | Description |
|---------|-------------------|-------------|
| Start Currency | USD | Currency to start/end triangular arbitrage |
| Trade Amount | $25 | Amount per trade (start small) |
| Min Profit Threshold | 0.1% (0.001) | Minimum profit to execute |
| Max Daily Loss | $50 | Circuit breaker - daily limit |
| Max Total Loss | $100 | Circuit breaker - total limit |

Or via API:
```bash
curl -X POST http://localhost:8000/api/config \
  -H "Content-Type: application/json" \
  -d '{
    "start_currency": "USD",
    "trade_amount": 25.0,
    "min_profit_threshold": 0.001,
    "max_daily_loss": 50.0,
    "max_total_loss": 100.0
  }'
```

---

## Step 6: Start Trading Engine

### Via Dashboard
Click **"Start Engine"** button.

### Via API
```bash
curl -X POST http://localhost:8000/api/engine/start
```

### Verify Running
```bash
curl http://localhost:8000/api/engine/status
```

---

## Step 7: Monitor & Verify Latency

### 7.1 Watch Logs

```bash
docker-compose logs -f backend
```

### 7.2 What to Look For

**WebSocket Connection (should show colocation endpoint):**
```
INFO  WebSocket v2 connected to wss://colo-london.vip-ws.kraken.com/v2
INFO  Subscribed to book channel (depth=10)
```

**Scan Performance:**
```
INFO  Scanned 100 times, no opportunity above threshold (scan: 0.25ms)
                                                              ^^^^^^
                                                    Should be <0.5ms
```

**Trade Execution (when opportunity found):**
```
⚡ Leg 1 completed: buy ETH/USD | ... | 45ms
⚡ Leg 2 completed: sell ETH/BTC | ... | 42ms
⚡ Leg 3 completed: sell BTC/USD | ... | 48ms
💰 Trade SUCCESS: USD → ETH → BTC → USD | $0.05 | total: 142ms
                                                        ^^^^^
                                              Should be ~100-200ms
```

### 7.3 Expected Latency Comparison

| Metric | AWS London | Beeks Colocation |
|--------|------------|------------------|
| Scan time | ~0.5ms | **~0.2-0.3ms** |
| Per-leg execution | ~100-150ms | **~40-60ms** |
| Total 3-leg trade | ~300-450ms | **~120-180ms** |
| WebSocket latency | ~1-5ms | **<1ms** |

---

## Troubleshooting

### Backend Won't Start

```bash
# Check logs
docker-compose logs backend

# Common fixes:
# 1. Wait 30s for database to initialize
# 2. Verify backend/.env has correct credentials
# 3. Rebuild: docker-compose up -d --build backend
```

### Cannot Connect to Kraken

```bash
# Test colocation WebSocket endpoint
curl -I https://colo-london.vip-ws.kraken.com

# Test standard REST API
curl https://api.kraken.com/0/public/Time

# If colocation fails, temporarily use standard endpoints:
# KRAKEN_WS_V2_PUBLIC=wss://ws.kraken.com/v2
# KRAKEN_WS_V2_PRIVATE=wss://ws-auth.kraken.com/v2
```

### Database Issues

```bash
# Check database status
docker-compose ps db
docker-compose logs db

# Restart database
docker-compose restart db

# Full reset (DELETES ALL DATA)
docker-compose down -v
docker-compose up -d
```

### Frontend Can't Reach Backend

```bash
# Verify backend responds
curl http://localhost:8000/api/health

# Check frontend .env
cat frontend/.env

# Should show: VITE_API_URL=http://localhost:8000
```

---

## Useful Commands

```bash
# Stop all services
docker-compose down

# Restart specific service
docker-compose restart backend

# View resource usage
docker stats

# Rebuild and restart
docker-compose up -d --build

# View all logs
docker-compose logs

# Enter backend container shell
docker-compose exec backend /bin/sh

# Access database directly
docker-compose exec db psql -U krakencryptox -d krakencryptox
```

---

## Colocation Endpoints Reference

From [Kraken API Documentation](https://docs.kraken.com/api/docs/guides/global-intro/):

| Service | Standard Endpoint | Colocation Endpoint |
|---------|-------------------|---------------------|
| WS Public | `wss://ws.kraken.com/v2` | `wss://colo-london.vip-ws.kraken.com/v2` |
| WS Private | `wss://ws-auth.kraken.com/v2` | `wss://colo-london.vip-ws-auth.kraken.com/v2` |
| REST API | `https://api.kraken.com` | (use standard) |

---

## Quick Reference

```bash
# === DEPLOYMENT ===
git clone -b main https://github.com/YOUR_USERNAME/LimogiAIcryptoX.git
cd LimogiAIcryptoX
# Edit backend/.env with your Kraken credentials
docker-compose up -d --build

# === MONITORING ===
docker-compose logs -f backend

# === CONTROL ===
curl -X POST http://localhost:8000/api/engine/start   # Start
curl -X POST http://localhost:8000/api/engine/stop    # Stop
curl http://localhost:8000/api/engine/status          # Status

# === DASHBOARD ===
# Open: http://YOUR_SERVER_IP:3000
```
