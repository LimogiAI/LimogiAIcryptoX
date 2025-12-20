-- ============================================
-- LIVE TRADING TABLES
-- Migration: 001_add_live_trading.sql
-- ============================================

-- ============================================
-- TABLE: live_trading_config
-- User-configurable settings (stored in DB, not env)
-- ============================================
CREATE TABLE IF NOT EXISTS live_trading_config (
    id SERIAL PRIMARY KEY,
    
    -- Enable/disable
    is_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    
    -- Trade parameters (user-selectable)
    trade_amount FLOAT NOT NULL DEFAULT 10.0,
    min_profit_threshold FLOAT NOT NULL DEFAULT 0.003,  -- 0.3%
    
    -- Loss limits (user-configurable)
    max_daily_loss FLOAT NOT NULL DEFAULT 30.0,
    max_total_loss FLOAT NOT NULL DEFAULT 30.0,
    
    -- Execution mode
    execution_mode VARCHAR(20) NOT NULL DEFAULT 'sequential',  -- 'sequential' or 'parallel'
    max_parallel_trades INT NOT NULL DEFAULT 1,
    
    -- Order execution settings
    max_retries_per_leg INT NOT NULL DEFAULT 2,
    order_timeout_seconds INT NOT NULL DEFAULT 30,
    
    -- Base currency filter (same as paper trading)
    base_currency VARCHAR(20) NOT NULL DEFAULT 'USD',
    custom_currencies JSONB DEFAULT '[]'::jsonb,
    
    -- Timestamps
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    enabled_at TIMESTAMP,
    disabled_at TIMESTAMP
);

-- Insert default config if not exists
INSERT INTO live_trading_config (id)
VALUES (1)
ON CONFLICT (id) DO NOTHING;

-- ============================================
-- TABLE: live_trading_state
-- System-managed state (losses, circuit breaker, etc.)
-- ============================================
CREATE TABLE IF NOT EXISTS live_trading_state (
    id SERIAL PRIMARY KEY,
    
    -- Current session stats
    daily_loss FLOAT NOT NULL DEFAULT 0.0,
    daily_profit FLOAT NOT NULL DEFAULT 0.0,
    daily_trades INT NOT NULL DEFAULT 0,
    daily_wins INT NOT NULL DEFAULT 0,
    
    -- All-time stats (since last reset)
    total_loss FLOAT NOT NULL DEFAULT 0.0,
    total_profit FLOAT NOT NULL DEFAULT 0.0,
    total_trades INT NOT NULL DEFAULT 0,
    total_wins INT NOT NULL DEFAULT 0,
    
    -- Circuit breaker
    is_circuit_broken BOOLEAN NOT NULL DEFAULT FALSE,
    circuit_broken_at TIMESTAMP,
    circuit_broken_reason TEXT,
    
    -- Timing
    last_trade_at TIMESTAMP,
    last_daily_reset TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    
    -- Currently executing (for sequential mode)
    is_executing BOOLEAN NOT NULL DEFAULT FALSE,
    current_trade_id VARCHAR(100),
    
    -- Timestamps
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Insert default state if not exists
INSERT INTO live_trading_state (id)
VALUES (1)
ON CONFLICT (id) DO NOTHING;

-- ============================================
-- TABLE: live_trades
-- Record of all live trade executions
-- ============================================
CREATE TABLE IF NOT EXISTS live_trades (
    id SERIAL PRIMARY KEY,
    trade_id VARCHAR(100) UNIQUE NOT NULL,
    
    -- What was traded
    path VARCHAR(500) NOT NULL,
    legs INT NOT NULL,
    
    -- Money in/out
    amount_in FLOAT NOT NULL,
    amount_out FLOAT,
    profit_loss FLOAT,
    profit_loss_pct FLOAT,
    
    -- Status
    status VARCHAR(20) NOT NULL DEFAULT 'PENDING',
    -- PENDING: Trade started
    -- EXECUTING: Orders being placed
    -- COMPLETED: All legs filled successfully
    -- PARTIAL: Some legs filled, then failed
    -- FAILED: No legs filled / error before execution
    
    current_leg INT DEFAULT 0,
    error_message TEXT,
    
    -- What we're holding if partial failure
    held_currency VARCHAR(20),
    held_amount FLOAT,
    
    -- Kraken references
    order_ids JSONB DEFAULT '[]'::jsonb,
    
    -- Per-leg execution details
    leg_fills JSONB DEFAULT '[]'::jsonb,
    -- Example: [
    --   {"leg": 1, "pair": "BTC/USD", "side": "buy", "price": 100150.00, "amount": 0.0001, "fee": 0.026, "order_id": "OABC-123"},
    --   {"leg": 2, "pair": "ETH/BTC", "side": "buy", "price": 0.035, "amount": 0.00285, "fee": 0.0000003, "order_id": "ODEF-456"},
    --   {"leg": 3, "pair": "ETH/USD", "side": "sell", "price": 3492.00, "amount": 0.00285, "fee": 0.026, "order_id": "OGHI-789"}
    -- ]
    
    -- Timing
    started_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    completed_at TIMESTAMP,
    total_execution_ms FLOAT,
    
    -- What triggered this trade
    opportunity_profit_pct FLOAT,
    
    -- Indexes for querying
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Indexes for live_trades
CREATE INDEX IF NOT EXISTS idx_live_trades_status ON live_trades(status);
CREATE INDEX IF NOT EXISTS idx_live_trades_created ON live_trades(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_live_trades_path ON live_trades(path);

-- ============================================
-- TABLE: live_positions
-- Track what we're currently holding (synced with Kraken)
-- ============================================
CREATE TABLE IF NOT EXISTS live_positions (
    id SERIAL PRIMARY KEY,
    currency VARCHAR(20) UNIQUE NOT NULL,
    balance FLOAT NOT NULL DEFAULT 0.0,
    usd_value FLOAT,
    last_synced_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- ============================================
-- TRIGGER: Update updated_at on config/state changes
-- ============================================
CREATE OR REPLACE FUNCTION update_live_trading_timestamp()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS update_live_trading_config_timestamp ON live_trading_config;
CREATE TRIGGER update_live_trading_config_timestamp
    BEFORE UPDATE ON live_trading_config
    FOR EACH ROW EXECUTE FUNCTION update_live_trading_timestamp();

DROP TRIGGER IF EXISTS update_live_trading_state_timestamp ON live_trading_state;
CREATE TRIGGER update_live_trading_state_timestamp
    BEFORE UPDATE ON live_trading_state
    FOR EACH ROW EXECUTE FUNCTION update_live_trading_timestamp();

-- ============================================
-- FUNCTION: Reset daily stats (call at midnight or manually)
-- ============================================
CREATE OR REPLACE FUNCTION reset_live_trading_daily_stats()
RETURNS void AS $$
BEGIN
    UPDATE live_trading_state
    SET 
        daily_loss = 0.0,
        daily_profit = 0.0,
        daily_trades = 0,
        daily_wins = 0,
        last_daily_reset = CURRENT_TIMESTAMP
    WHERE id = 1;
END;
$$ LANGUAGE plpgsql;

-- ============================================
-- FUNCTION: Check if daily reset needed
-- ============================================
CREATE OR REPLACE FUNCTION check_and_reset_daily_stats()
RETURNS void AS $$
DECLARE
    last_reset TIMESTAMP;
BEGIN
    SELECT last_daily_reset INTO last_reset FROM live_trading_state WHERE id = 1;
    
    -- If last reset was not today (UTC), reset now
    IF last_reset IS NULL OR DATE(last_reset) < DATE(CURRENT_TIMESTAMP) THEN
        PERFORM reset_live_trading_daily_stats();
    END IF;
END;
$$ LANGUAGE plpgsql;
