# Beeks Exchange Cloud Deployment Guide

## Overview

This guide deploys the LimogiAICryptoX HFT backend to Beeks Exchange Cloud for sub-millisecond latency to Kraken.

## Architecture

```
Beeks Gold VM (London)          External Services
┌─────────────────────┐         ┌─────────────────────┐
│  Rust Backend       │◄───────►│  PostgreSQL (AWS)   │
│  - WebSocket        │         │  - Config storage   │
│  - Scanner          │         │  - Trade history    │
│  - Executor         │         └─────────────────────┘
└─────────────────────┘
         │                      ┌─────────────────────┐
         │ <1ms (cross-connect) │  Frontend (Vercel)  │
         ▼                      │  - Dashboard        │
┌─────────────────────┐         └─────────────────────┘
│  Kraken Exchange    │
└─────────────────────┘
```

## Prerequisites

1. **Beeks Account**: Sign up at https://kraken.exchange-cloud.beeksgroup.com/
2. **Order**:
   - Gold VM (£116.40/mo)
   - Cross Connect to Kraken (£550/mo)
3. **AWS RDS PostgreSQL** (or any managed PostgreSQL)
4. **Kraken API Keys** with trading permissions

## Step 1: Provision Beeks Gold VM

1. Go to https://kraken.exchange-cloud.beeksgroup.com/
2. Order "Gold" VM (4 vCPU, 6.6GB RAM, 75GB disk)
3. Order "Cross Connect" (shared fibre to Kraken)
4. Wait for provisioning (typically 1 hour for VM)
5. You'll receive SSH credentials via email

## Step 2: Set Up External PostgreSQL (AWS RDS)

```bash
# Create RDS instance via AWS Console or CLI
aws rds create-db-instance \
  --db-instance-identifier limogiai-db \
  --db-instance-class db.t3.micro \
  --engine postgres \
  --master-username krakencryptox \
  --master-user-password YOUR_SECURE_PASSWORD \
  --allocated-storage 20 \
  --publicly-accessible

# Note: Open port 5432 to Beeks IP in security group
```

## Step 3: Build Release Binary

On your local machine:

```bash
cd backend

# Cross-compile for Linux x86_64 (Beeks runs Linux)
# Option A: If on Linux
cargo build --release

# Option B: If on macOS, use cross-compilation
rustup target add x86_64-unknown-linux-gnu
cargo build --release --target x86_64-unknown-linux-gnu

# Binary location:
# Linux: target/release/trading_server
# Cross: target/x86_64-unknown-linux-gnu/release/trading_server
```

## Step 4: Deploy to Beeks VM

```bash
# SSH into Beeks VM
ssh user@YOUR_BEEKS_IP

# Create app directory
mkdir -p /opt/limogiai
cd /opt/limogiai

# Copy binary from local machine (run on local)
scp target/release/trading_server user@YOUR_BEEKS_IP:/opt/limogiai/

# Copy config files
scp -r config/ user@YOUR_BEEKS_IP:/opt/limogiai/

# Create .env file on Beeks VM
cat > /opt/limogiai/.env << 'EOF'
# Database (AWS RDS)
DATABASE_URL=postgresql://krakencryptox:YOUR_PASSWORD@YOUR_RDS_ENDPOINT:5432/krakencryptox

# Kraken API
KRAKEN_API_KEY=your_api_key_here
KRAKEN_API_SECRET=your_api_secret_here

# Server
PORT=8000
RUST_LOG=info
EOF

# Make binary executable
chmod +x /opt/limogiai/trading_server
```

## Step 5: Create Systemd Service

```bash
sudo cat > /etc/systemd/system/limogiai.service << 'EOF'
[Unit]
Description=LimogiAI HFT Trading Engine
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=/opt/limogiai
EnvironmentFile=/opt/limogiai/.env
ExecStart=/opt/limogiai/trading_server
Restart=always
RestartSec=5

# Performance optimizations
CPUSchedulingPolicy=fifo
CPUSchedulingPriority=99
LimitNOFILE=65535
LimitNPROC=65535

[Install]
WantedBy=multi-user.target
EOF

# Enable and start
sudo systemctl daemon-reload
sudo systemctl enable limogiai
sudo systemctl start limogiai

# Check logs
sudo journalctl -u limogiai -f
```

## Step 6: Initialize Database

From your local machine or Beeks VM:

```bash
# Apply migrations to AWS RDS
psql "postgresql://krakencryptox:PASSWORD@YOUR_RDS_ENDPOINT:5432/krakencryptox" \
  -f db/migrations/apply_all_migrations.sql
```

## Step 7: Deploy Frontend (Vercel)

```bash
cd frontend

# Install Vercel CLI
npm i -g vercel

# Deploy
vercel

# Set environment variables in Vercel dashboard:
# VITE_API_URL=http://YOUR_BEEKS_IP:8000
# VITE_API_KEY=your_api_key (if using authentication)
```

## Step 8: Firewall Configuration

On Beeks VM:

```bash
# Allow API access from your IPs only
sudo ufw allow from YOUR_HOME_IP to any port 8000
sudo ufw allow from VERCEL_IP_RANGE to any port 8000
sudo ufw enable
```

## Performance Tuning (Beeks VM)

```bash
# /etc/sysctl.conf additions for low-latency networking
cat >> /etc/sysctl.conf << 'EOF'
# Low-latency networking
net.core.rmem_max=16777216
net.core.wmem_max=16777216
net.ipv4.tcp_rmem=4096 87380 16777216
net.ipv4.tcp_wmem=4096 65536 16777216
net.ipv4.tcp_nodelay=1
net.core.netdev_max_backlog=30000
EOF

sudo sysctl -p
```

## Monitoring

Check latency to Kraken:

```bash
# From Beeks VM
ping api.kraken.com

# Should see <1ms with cross-connect
# Compare to AWS (~50-100ms)
```

## Troubleshooting

### Check service status
```bash
sudo systemctl status limogiai
sudo journalctl -u limogiai --since "5 minutes ago"
```

### Test API
```bash
curl http://localhost:8000/api/status
curl http://localhost:8000/api/live/positions
```

### Database connection
```bash
psql $DATABASE_URL -c "SELECT 1"
```

## Cost Summary

| Service | Monthly Cost |
|---------|-------------|
| Beeks Gold VM | £116.40 (~$147) |
| Beeks Cross-Connect | £550.00 (~$695) |
| AWS RDS db.t3.micro | ~$15 |
| Vercel (Free tier) | $0 |
| **Total** | **~$860/month** |

## Expected Latency Improvement

| Metric | AWS EC2 | Beeks + Cross-Connect |
|--------|---------|----------------------|
| Ping to Kraken | 50-200ms | <1ms |
| Order execution | 100-300ms | 1-5ms |
| WebSocket updates | 50-150ms | <1ms |

This 50-200x latency improvement is critical for HFT arbitrage where opportunities exist for milliseconds.
