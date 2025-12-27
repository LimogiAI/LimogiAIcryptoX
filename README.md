# LimogiAICryptoX - HFT Triangular Arbitrage Trading System

High-frequency triangular arbitrage trading system for Kraken cryptocurrency exchange.

## Architecture

- **Backend**: Rust (Axum + Tokio) - High-performance trading engine
- **Frontend**: React + TypeScript + Tailwind CSS - Trading dashboard
- **Database**: PostgreSQL 15 - Trade history and configuration
- **Deployment**: Docker Compose

## Quick Start

### Prerequisites

- Docker and Docker Compose (v2)
- Kraken API credentials (with trading permissions)
- Git

### 1. Clone the Repository

```bash
git clone <repository-url>
cd LimogiAICryptoX
```

### 2. Configure Environment

Create backend environment file:

```bash
cp backend/.env.example backend/.env
```

Edit `backend/.env` with your Kraken API credentials:

```env
# Kraken API Credentials (REQUIRED)
KRAKEN_API_KEY=your_api_key_here
KRAKEN_API_SECRET=your_api_secret_here

# API Security (REQUIRED for production)
API_KEY=your_secure_api_key_for_dashboard

# Database (uses defaults from docker-compose if not set)
DATABASE_URL=postgresql://krakencryptox:krakencryptox123@db:5432/krakencryptox
```

Create frontend environment file:

```bash
cp frontend/.env.example frontend/.env
```

Edit `frontend/.env`:

```env
VITE_API_URL=http://localhost:8000
VITE_API_KEY=your_secure_api_key_for_dashboard
```

### 3. Start the Application

```bash
docker compose up -d --build
```

### 4. Access the Dashboard

- **Frontend**: http://localhost:3000
- **Backend API**: http://localhost:8000

## Production Deployment (AWS EC2)

### 1. Server Setup

```bash
# Update system
sudo apt update && sudo apt upgrade -y

# Install Docker
curl -fsSL https://get.docker.com | sudo sh
sudo usermod -aG docker $USER

# Install Docker Compose plugin
sudo apt install docker-compose-plugin -y

# Log out and back in for group changes
exit
```

### 2. Deploy Application

```bash
# Clone repository
git clone <repository-url>
cd LimogiAICryptoX

# Configure environment files (see section 2 above)
nano backend/.env
nano frontend/.env

# Update frontend API URL for your server IP
# In frontend/.env:
# VITE_API_URL=http://YOUR_EC2_PUBLIC_IP:8000

# Build and start
docker compose up -d --build
```

### 3. Verify Deployment

```bash
# Check containers are running
docker compose ps

# Check backend logs
docker compose logs backend --tail 50

# Check frontend logs
docker compose logs frontend --tail 50
```

## Upgrading Existing Installations

If you're upgrading from a previous version, run the database upgrade script:

```bash
# Make script executable (first time only)
chmod +x scripts/upgrade-db.sh

# Run upgrade
./scripts/upgrade-db.sh
```

This script will:
- Add any missing database columns
- Fix column types (e.g., TIMESTAMP -> TIMESTAMPTZ)
- Restart the backend to apply changes

## Configuration

### Trading Configuration (via Dashboard)

All trading parameters are configured through the web dashboard:

| Parameter | Description | Recommended |
|-----------|-------------|-------------|
| Start Currency | USD, EUR, or both | USD,EUR |
| Trade Amount | Amount per trade in start currency | $10-50 |
| Min Profit Threshold | Minimum profit % to execute | 0.0001% |
| Max Daily Loss | Stop trading if daily loss exceeds | $10 |
| Max Total Loss | Stop trading if total loss exceeds | $20 |
| Max Pairs | Maximum trading pairs to monitor | 50 |
| Min 24h Volume | Minimum pair volume in USD | $50,000 |
| Max Cost Min | Maximum order minimum | $10-20 |

### Fee Configuration

Fees are fetched automatically from Kraken API based on your account's 30-day trading volume.

## Troubleshooting

### Session Tracking Not Working

If session tracking shows as `null`, the database schema needs updating:

```bash
./scripts/upgrade-db.sh
```

Or manually fix:

```bash
docker compose exec db psql -U krakencryptox -d krakencryptox -c "
ALTER TABLE live_trading_config
ALTER COLUMN enabled_at TYPE TIMESTAMPTZ,
ALTER COLUMN disabled_at TYPE TIMESTAMPTZ;
"
docker compose restart backend
```

### docker-compose vs docker compose

Use `docker compose` (with space, v2) instead of `docker-compose` (with hyphen, v1):

```bash
# Correct (v2)
docker compose up -d

# May have issues (v1)
docker-compose up -d
```

### Check Database Connection

```bash
# List databases
docker compose exec db psql -U krakencryptox -c "\l"

# Check tables
docker compose exec db psql -U krakencryptox -d krakencryptox -c "\dt"

# Check live_trading_config schema
docker compose exec db psql -U krakencryptox -d krakencryptox -c "\d live_trading_config"
```

### View Logs

```bash
# All services
docker compose logs -f

# Backend only
docker compose logs backend -f

# Last 100 lines
docker compose logs backend --tail 100
```

### Rebuild After Code Changes

```bash
# Rebuild specific service
docker compose build backend --no-cache
docker compose up -d backend

# Rebuild all
docker compose down
docker compose up -d --build
```

### Reset Database (CAUTION: Deletes all data!)

```bash
docker compose down -v
docker compose up -d --build
```

## Security Considerations

1. **Never commit `.env` files** - They contain API keys
2. **Use strong API keys** - For both Kraken and dashboard access
3. **Firewall** - Only expose necessary ports (3000, 8000)
4. **HTTPS** - Use a reverse proxy (nginx) with SSL in production
5. **Rate limits** - The system respects Kraken's rate limits

## Project Structure

```
LimogiAICryptoX/
├── backend/                 # Rust trading engine
│   ├── src/
│   │   ├── main.rs         # Entry point
│   │   ├── api/            # REST API handlers
│   │   ├── executor.rs     # Trade execution
│   │   ├── hft_loop.rs     # HFT scanning loop
│   │   └── graph_manager.rs # Arbitrage detection
│   ├── Cargo.toml
│   ├── Dockerfile
│   └── .env
├── frontend/               # React dashboard
│   ├── src/
│   │   ├── components/     # UI components
│   │   ├── services/       # API client
│   │   └── types/          # TypeScript types
│   ├── package.json
│   ├── Dockerfile
│   └── .env
├── db/
│   ├── init.sql           # Base schema
│   └── migrations/        # Schema updates
├── scripts/
│   └── upgrade-db.sh      # Database upgrade script
├── docker-compose.yml
└── README.md
```

## License

Proprietary - All rights reserved.
