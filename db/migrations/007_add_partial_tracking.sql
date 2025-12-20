-- Migration: Add partial trade tracking (Option C)
-- Run this AFTER the existing tables are created

-- Add partial tracking columns to live_trading_state
ALTER TABLE live_trading_state 
ADD COLUMN IF NOT EXISTS partial_trades INTEGER NOT NULL DEFAULT 0,
ADD COLUMN IF NOT EXISTS partial_estimated_loss FLOAT NOT NULL DEFAULT 0.0,
ADD COLUMN IF NOT EXISTS partial_estimated_profit FLOAT NOT NULL DEFAULT 0.0,
ADD COLUMN IF NOT EXISTS partial_trade_amount FLOAT NOT NULL DEFAULT 0.0;

-- Add resolution tracking columns to live_trades
ALTER TABLE live_trades
ADD COLUMN IF NOT EXISTS held_value_usd FLOAT,
ADD COLUMN IF NOT EXISTS resolved_at TIMESTAMP,
ADD COLUMN IF NOT EXISTS resolved_amount_usd FLOAT,
ADD COLUMN IF NOT EXISTS resolution_trade_id VARCHAR(100);

-- Add index for finding unresolved partial trades
CREATE INDEX IF NOT EXISTS idx_live_trades_partial 
ON live_trades(status) WHERE status = 'PARTIAL';

-- Add index for resolved trades
CREATE INDEX IF NOT EXISTS idx_live_trades_resolved 
ON live_trades(status) WHERE status = 'RESOLVED';

COMMENT ON COLUMN live_trading_state.partial_trades IS 'Count of unresolved PARTIAL trades';
COMMENT ON COLUMN live_trading_state.partial_estimated_loss IS 'Snapshot estimated loss from partial trades';
COMMENT ON COLUMN live_trading_state.partial_estimated_profit IS 'Snapshot estimated profit from partial trades';
COMMENT ON COLUMN live_trading_state.partial_trade_amount IS 'Total $ stuck in partial trades';

COMMENT ON COLUMN live_trades.held_value_usd IS 'Snapshot USD value of held currency at time of failure';
COMMENT ON COLUMN live_trades.resolved_at IS 'When the partial trade was resolved (sold)';
COMMENT ON COLUMN live_trades.resolved_amount_usd IS 'Actual USD received when held currency was sold';
COMMENT ON COLUMN live_trades.resolution_trade_id IS 'ID of the trade that sold the held currency';
