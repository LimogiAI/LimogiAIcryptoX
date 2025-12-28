#!/bin/bash
# Beeks Exchange Cloud Deployment Script
# Usage: ./deploy.sh <beeks_ip> <rds_endpoint>

set -e

BEEKS_IP="${1:-}"
RDS_ENDPOINT="${2:-}"
BEEKS_USER="${BEEKS_USER:-root}"
APP_DIR="/opt/limogiai"

if [ -z "$BEEKS_IP" ]; then
    echo "Usage: ./deploy.sh <beeks_ip> [rds_endpoint]"
    echo "Example: ./deploy.sh 10.0.1.100 mydb.xyz.eu-west-1.rds.amazonaws.com"
    exit 1
fi

echo "========================================"
echo "  LimogiAI Beeks Deployment"
echo "========================================"
echo "Target: $BEEKS_USER@$BEEKS_IP"
echo "App Dir: $APP_DIR"
echo ""

# Step 1: Build release binary
echo "[1/5] Building release binary..."
cd "$(dirname "$0")/../../backend"

# Check if cross-compilation is needed
if [[ "$(uname)" == "Darwin" ]]; then
    echo "  macOS detected - using cross-compilation"

    # Check if target is installed
    if ! rustup target list | grep -q "x86_64-unknown-linux-gnu (installed)"; then
        echo "  Installing x86_64-unknown-linux-gnu target..."
        rustup target add x86_64-unknown-linux-gnu
    fi

    # Check for cross-compilation linker
    if ! command -v x86_64-linux-gnu-gcc &> /dev/null; then
        echo "  WARNING: Cross-compiler not found. Install with:"
        echo "    brew install filosottile/musl-cross/musl-cross"
        echo "  Or build on a Linux machine instead."
        exit 1
    fi

    CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc \
        cargo build --release --target x86_64-unknown-linux-gnu
    BINARY_PATH="target/x86_64-unknown-linux-gnu/release/trading_server"
else
    cargo build --release
    BINARY_PATH="target/release/trading_server"
fi

if [ ! -f "$BINARY_PATH" ]; then
    echo "ERROR: Binary not found at $BINARY_PATH"
    exit 1
fi

BINARY_SIZE=$(du -h "$BINARY_PATH" | cut -f1)
echo "  Binary built: $BINARY_PATH ($BINARY_SIZE)"

# Step 2: Create remote directory
echo "[2/5] Setting up remote directory..."
ssh "$BEEKS_USER@$BEEKS_IP" "mkdir -p $APP_DIR/config"

# Step 3: Upload files
echo "[3/5] Uploading files..."
scp "$BINARY_PATH" "$BEEKS_USER@$BEEKS_IP:$APP_DIR/trading_server"
scp -r "$(dirname "$0")/../../backend/config/"* "$BEEKS_USER@$BEEKS_IP:$APP_DIR/config/" 2>/dev/null || true
scp "$(dirname "$0")/../../db/migrations/apply_all_migrations.sql" "$BEEKS_USER@$BEEKS_IP:$APP_DIR/" 2>/dev/null || true

# Step 4: Set permissions
echo "[4/5] Setting permissions..."
ssh "$BEEKS_USER@$BEEKS_IP" "chmod +x $APP_DIR/trading_server"

# Step 5: Create systemd service
echo "[5/5] Creating systemd service..."
ssh "$BEEKS_USER@$BEEKS_IP" "cat > /etc/systemd/system/limogiai.service << 'SERVICEEOF'
[Unit]
Description=LimogiAI HFT Trading Engine
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=$APP_DIR
EnvironmentFile=$APP_DIR/.env
ExecStart=$APP_DIR/trading_server
Restart=always
RestartSec=5

# Performance optimizations for HFT
CPUSchedulingPolicy=fifo
CPUSchedulingPriority=99
LimitNOFILE=65535
LimitNPROC=65535
Nice=-20

[Install]
WantedBy=multi-user.target
SERVICEEOF"

ssh "$BEEKS_USER@$BEEKS_IP" "systemctl daemon-reload"

echo ""
echo "========================================"
echo "  Deployment Complete!"
echo "========================================"
echo ""
echo "Next steps:"
echo ""
echo "1. Create .env file on Beeks VM:"
echo "   ssh $BEEKS_USER@$BEEKS_IP"
echo "   cat > $APP_DIR/.env << 'EOF'"
echo "   DATABASE_URL=postgresql://krakencryptox:PASSWORD@$RDS_ENDPOINT:5432/krakencryptox"
echo "   KRAKEN_API_KEY=your_key"
echo "   KRAKEN_API_SECRET=your_secret"
echo "   PORT=8000"
echo "   RUST_LOG=info"
echo "   EOF"
echo ""
echo "2. Initialize database (if new):"
echo "   psql \$DATABASE_URL -f $APP_DIR/apply_all_migrations.sql"
echo ""
echo "3. Start the service:"
echo "   ssh $BEEKS_USER@$BEEKS_IP 'systemctl start limogiai'"
echo ""
echo "4. Check logs:"
echo "   ssh $BEEKS_USER@$BEEKS_IP 'journalctl -u limogiai -f'"
echo ""
echo "5. Test API:"
echo "   curl http://$BEEKS_IP:8000/api/status"
