-- ============================================
-- Migration: 002_rename_start_currency.sql
-- Rename base_currency to start_currency for clarity
-- The column represents the starting currency for triangular arbitrage
-- ============================================

-- Rename column from base_currency to start_currency
-- This is more descriptive: it's the currency where arbitrage starts and ends
DO $$
BEGIN
    -- Check if the column exists with the old name and rename it
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
END $$;

-- Update column comment for documentation
COMMENT ON COLUMN live_trading_config.start_currency IS
    'Starting currency for triangular arbitrage (USD, EUR, or both). The cycle starts and ends with this currency.';
