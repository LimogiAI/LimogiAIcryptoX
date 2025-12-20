-- Migration: Add live_opportunities and live_scanner_status tables
-- Date: 2024-12-19
-- Description: Track opportunities found by live trading scanner

-- ============================================
-- TABLE: live_opportunities
-- Record of opportunities found by scanner
-- ============================================
CREATE TABLE IF NOT EXISTS live_opportunities (
    id SERIAL PRIMARY KEY,
    
    -- When found
    found_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    
    -- Opportunity details
    path VARCHAR(500) NOT NULL,
    legs INT NOT NULL DEFAULT 3,
    expected_profit_pct FLOAT NOT NULL,
    expected_profit_usd FLOAT,
    trade_amount FLOAT,
    
    -- Status: PENDING, EXECUTED, SKIPPED, MISSED, EXPIRED
    status VARCHAR(20) NOT NULL DEFAULT 'PENDING',
    status_reason VARCHAR(500),
    
    -- Link to trade if executed
    trade_id VARCHAR(100),
    
    -- Scan info
    pairs_scanned INT,
    paths_found INT,
    
    -- Timestamps
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Indexes for live_opportunities
CREATE INDEX IF NOT EXISTS idx_live_opportunities_found_at ON live_opportunities(found_at DESC);
CREATE INDEX IF NOT EXISTS idx_live_opportunities_status ON live_opportunities(status);
CREATE INDEX IF NOT EXISTS idx_live_opportunities_path ON live_opportunities(path);
CREATE INDEX IF NOT EXISTS idx_live_opportunities_trade_id ON live_opportunities(trade_id);

-- ============================================
-- TABLE: live_scanner_status
-- Current scanner status (single row)
-- ============================================
CREATE TABLE IF NOT EXISTS live_scanner_status (
    id INT PRIMARY KEY DEFAULT 1,
    
    -- Status
    is_running BOOLEAN NOT NULL DEFAULT FALSE,
    
    -- Last scan info
    last_scan_at TIMESTAMP,
    pairs_scanned INT NOT NULL DEFAULT 0,
    paths_found INT NOT NULL DEFAULT 0,
    opportunities_found INT NOT NULL DEFAULT 0,
    profitable_count INT NOT NULL DEFAULT 0,
    
    -- Scan timing
    scan_duration_ms FLOAT,
    
    -- Error tracking
    last_error TEXT,
    last_error_at TIMESTAMP,
    
    -- Timestamps
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    
    -- Ensure only one row
    CONSTRAINT single_row CHECK (id = 1)
);

-- Insert default scanner status row
INSERT INTO live_scanner_status (id, is_running, pairs_scanned, paths_found)
VALUES (1, FALSE, 0, 0)
ON CONFLICT (id) DO NOTHING;

-- ============================================
-- FUNCTION: Clean old opportunities (keep 7 days)
-- ============================================
CREATE OR REPLACE FUNCTION clean_old_live_opportunities()
RETURNS void AS $$
BEGIN
    DELETE FROM live_opportunities
    WHERE found_at < NOW() - INTERVAL '7 days';
END;
$$ LANGUAGE plpgsql;

-- Trigger for updated_at
CREATE OR REPLACE TRIGGER update_live_opportunities_updated_at
    BEFORE UPDATE ON live_opportunities
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE OR REPLACE TRIGGER update_live_scanner_status_updated_at
    BEFORE UPDATE ON live_scanner_status
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
