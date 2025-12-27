#!/bin/bash
# ============================================
# LimogiAICryptoX - Database Upgrade Script
# Run this script to apply schema updates to existing databases
# ============================================

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}============================================${NC}"
echo -e "${GREEN}LimogiAICryptoX - Database Upgrade Script${NC}"
echo -e "${GREEN}============================================${NC}"

# Check if docker compose is available
if command -v docker compose &> /dev/null; then
    COMPOSE_CMD="docker compose"
elif command -v docker-compose &> /dev/null; then
    COMPOSE_CMD="docker-compose"
else
    echo -e "${RED}Error: docker compose not found${NC}"
    exit 1
fi

# Get database credentials from environment or use defaults
DB_USER="${POSTGRES_USER:-krakencryptox}"
DB_NAME="${POSTGRES_DB:-krakencryptox}"

echo -e "\n${YELLOW}Using database: ${DB_NAME} (user: ${DB_USER})${NC}\n"

# Function to run SQL
run_sql() {
    $COMPOSE_CMD exec -T db psql -U "$DB_USER" -d "$DB_NAME" -c "$1"
}

echo -e "${YELLOW}1. Checking current schema...${NC}"
run_sql "SELECT column_name, data_type FROM information_schema.columns WHERE table_name = 'live_trading_config' ORDER BY ordinal_position;" || true

echo -e "\n${YELLOW}2. Adding missing columns if needed...${NC}"

# Add enabled_at column if not exists
run_sql "ALTER TABLE live_trading_config ADD COLUMN IF NOT EXISTS enabled_at TIMESTAMPTZ;" || true

# Add disabled_at column if not exists
run_sql "ALTER TABLE live_trading_config ADD COLUMN IF NOT EXISTS disabled_at TIMESTAMPTZ;" || true

echo -e "\n${YELLOW}3. Fixing column types (TIMESTAMP -> TIMESTAMPTZ)...${NC}"

# Fix enabled_at type
run_sql "ALTER TABLE live_trading_config ALTER COLUMN enabled_at TYPE TIMESTAMPTZ;" || true

# Fix disabled_at type
run_sql "ALTER TABLE live_trading_config ALTER COLUMN disabled_at TYPE TIMESTAMPTZ;" || true

echo -e "\n${YELLOW}3b. Renaming base_currency to start_currency (if needed)...${NC}"

# Rename base_currency to start_currency for clarity
run_sql "DO \$\$ BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'live_trading_config'
        AND column_name = 'base_currency'
    ) THEN
        ALTER TABLE live_trading_config RENAME COLUMN base_currency TO start_currency;
        RAISE NOTICE 'Renamed column base_currency to start_currency';
    ELSE
        RAISE NOTICE 'Column base_currency does not exist or already renamed';
    END IF;
END \$\$;" || true

echo -e "\n${YELLOW}4. Adding partial trade columns if missing...${NC}"

# Add partial trade tracking columns
run_sql "ALTER TABLE live_trading_state ADD COLUMN IF NOT EXISTS partial_trades INT NOT NULL DEFAULT 0;" || true
run_sql "ALTER TABLE live_trading_state ADD COLUMN IF NOT EXISTS partial_estimated_loss FLOAT NOT NULL DEFAULT 0.0;" || true
run_sql "ALTER TABLE live_trading_state ADD COLUMN IF NOT EXISTS partial_estimated_profit FLOAT NOT NULL DEFAULT 0.0;" || true
run_sql "ALTER TABLE live_trading_state ADD COLUMN IF NOT EXISTS partial_trade_amount FLOAT NOT NULL DEFAULT 0.0;" || true
run_sql "ALTER TABLE live_trading_state ADD COLUMN IF NOT EXISTS total_trade_amount FLOAT NOT NULL DEFAULT 0.0;" || true

# Add held_value_usd to live_trades
run_sql "ALTER TABLE live_trades ADD COLUMN IF NOT EXISTS held_value_usd FLOAT;" || true

# Add resolution columns to live_trades
run_sql "ALTER TABLE live_trades ADD COLUMN IF NOT EXISTS resolved_at TIMESTAMP;" || true
run_sql "ALTER TABLE live_trades ADD COLUMN IF NOT EXISTS resolved_amount_usd FLOAT;" || true
run_sql "ALTER TABLE live_trades ADD COLUMN IF NOT EXISTS resolution_trade_id VARCHAR(100);" || true

echo -e "\n${YELLOW}5. Verifying schema...${NC}"
run_sql "SELECT column_name, data_type FROM information_schema.columns WHERE table_name = 'live_trading_config' AND column_name IN ('enabled_at', 'disabled_at');"

echo -e "\n${GREEN}============================================${NC}"
echo -e "${GREEN}Database upgrade complete!${NC}"
echo -e "${GREEN}============================================${NC}"

echo -e "\n${YELLOW}Restarting backend to apply changes...${NC}"
$COMPOSE_CMD restart backend

echo -e "\n${GREEN}Done! Session tracking should now work.${NC}"
