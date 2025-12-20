-- ============================================
-- Migration: 002_add_total_trade_amount.sql
-- Add total_trade_amount to live_trading_state
-- ============================================

-- Add total_trade_amount column if it doesn't exist
ALTER TABLE live_trading_state 
ADD COLUMN IF NOT EXISTS total_trade_amount FLOAT NOT NULL DEFAULT 0.0;

-- Update existing record
UPDATE live_trading_state SET total_trade_amount = 0.0 WHERE id = 1 AND total_trade_amount IS NULL;
