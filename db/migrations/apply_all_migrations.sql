-- ============================================
-- APPLY ALL MIGRATIONS
-- Run this script on existing databases to apply all schema updates
-- Safe to run multiple times (uses IF NOT EXISTS / IF EXISTS checks)
-- ============================================

-- ============================================
-- 1. Rename base_currency to start_currency
-- ============================================
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'live_trading_config'
        AND column_name = 'base_currency'
    ) THEN
        ALTER TABLE live_trading_config RENAME COLUMN base_currency TO start_currency;
        RAISE NOTICE 'Renamed column base_currency to start_currency';
    ELSE
        RAISE NOTICE 'Column base_currency does not exist or already renamed to start_currency';
    END IF;
END $$;

-- ============================================
-- 2. Add total_trade_amount column
-- ============================================
ALTER TABLE live_trading_state
ADD COLUMN IF NOT EXISTS total_trade_amount FLOAT NOT NULL DEFAULT 0.0;

-- ============================================
-- 3. Add pair selection filter columns to live_trading_config
-- ============================================
ALTER TABLE live_trading_config
ADD COLUMN IF NOT EXISTS max_pairs INT,
ADD COLUMN IF NOT EXISTS min_volume_24h_usd FLOAT,
ADD COLUMN IF NOT EXISTS max_cost_min FLOAT;

-- ============================================
-- 4. Create fee_configuration table
-- ============================================
CREATE TABLE IF NOT EXISTS fee_configuration (
    id INTEGER PRIMARY KEY DEFAULT 1,
    maker_fee DOUBLE PRECISION NOT NULL DEFAULT 0,
    taker_fee DOUBLE PRECISION NOT NULL DEFAULT 0,
    fee_source VARCHAR(20) NOT NULL DEFAULT 'pending',
    volume_tier VARCHAR(50),
    thirty_day_volume DOUBLE PRECISION,
    last_fetched_at TIMESTAMPTZ,
    last_updated_at TIMESTAMPTZ DEFAULT NOW(),
    created_at TIMESTAMPTZ DEFAULT NOW(),
    CONSTRAINT fee_configuration_single_row CHECK (id = 1)
);

INSERT INTO fee_configuration (id, maker_fee, taker_fee, fee_source)
VALUES (1, 0, 0, 'pending')
ON CONFLICT (id) DO NOTHING;

-- Create trigger for fee_configuration
CREATE OR REPLACE FUNCTION update_fee_configuration_timestamp()
RETURNS TRIGGER AS $$
BEGIN
    NEW.last_updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS update_fee_configuration_timestamp ON fee_configuration;
CREATE TRIGGER update_fee_configuration_timestamp
    BEFORE UPDATE ON fee_configuration
    FOR EACH ROW
    EXECUTE FUNCTION update_fee_configuration_timestamp();

-- ============================================
-- 5. Create live_opportunities table
-- ============================================
CREATE TABLE IF NOT EXISTS live_opportunities (
    id SERIAL PRIMARY KEY,
    found_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    path VARCHAR(500) NOT NULL,
    legs INT NOT NULL DEFAULT 3,
    expected_profit_pct FLOAT NOT NULL,
    expected_profit_usd FLOAT,
    trade_amount FLOAT,
    status VARCHAR(20) NOT NULL DEFAULT 'PENDING',
    status_reason VARCHAR(500),
    trade_id VARCHAR(100),
    pairs_scanned INT,
    paths_found INT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_live_opportunities_found_at ON live_opportunities(found_at DESC);
CREATE INDEX IF NOT EXISTS idx_live_opportunities_status ON live_opportunities(status);
CREATE INDEX IF NOT EXISTS idx_live_opportunities_path ON live_opportunities(path);
CREATE INDEX IF NOT EXISTS idx_live_opportunities_trade_id ON live_opportunities(trade_id);

-- ============================================
-- 6. Create live_scanner_status table
-- ============================================
CREATE TABLE IF NOT EXISTS live_scanner_status (
    id INT PRIMARY KEY DEFAULT 1,
    is_running BOOLEAN NOT NULL DEFAULT FALSE,
    last_scan_at TIMESTAMP,
    pairs_scanned INT NOT NULL DEFAULT 0,
    paths_found INT NOT NULL DEFAULT 0,
    opportunities_found INT NOT NULL DEFAULT 0,
    profitable_count INT NOT NULL DEFAULT 0,
    scan_duration_ms FLOAT,
    last_error TEXT,
    last_error_at TIMESTAMP,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT single_row CHECK (id = 1)
);

INSERT INTO live_scanner_status (id, is_running, pairs_scanned, paths_found)
VALUES (1, FALSE, 0, 0)
ON CONFLICT (id) DO NOTHING;

-- ============================================
-- 7. Add partial tracking columns
-- ============================================
ALTER TABLE live_trading_state
ADD COLUMN IF NOT EXISTS partial_trades INTEGER NOT NULL DEFAULT 0,
ADD COLUMN IF NOT EXISTS partial_estimated_loss FLOAT NOT NULL DEFAULT 0.0,
ADD COLUMN IF NOT EXISTS partial_estimated_profit FLOAT NOT NULL DEFAULT 0.0,
ADD COLUMN IF NOT EXISTS partial_trade_amount FLOAT NOT NULL DEFAULT 0.0;

ALTER TABLE live_trades
ADD COLUMN IF NOT EXISTS held_value_usd FLOAT,
ADD COLUMN IF NOT EXISTS resolved_at TIMESTAMP,
ADD COLUMN IF NOT EXISTS resolved_amount_usd FLOAT,
ADD COLUMN IF NOT EXISTS resolution_trade_id VARCHAR(100);

CREATE INDEX IF NOT EXISTS idx_live_trades_partial
ON live_trades(status) WHERE status = 'PARTIAL';

CREATE INDEX IF NOT EXISTS idx_live_trades_resolved
ON live_trades(status) WHERE status = 'RESOLVED';

-- ============================================
-- 8. Ensure update_updated_at_column function exists
-- ============================================
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ language 'plpgsql';

-- ============================================
-- Done!
-- ============================================
SELECT 'All migrations applied successfully!' as status;
