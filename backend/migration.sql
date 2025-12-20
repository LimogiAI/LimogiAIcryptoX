-- ============================================
-- DATABASE MIGRATION SCRIPT
-- Run this SQL in your PostgreSQL database
-- 
-- Command:
-- docker compose exec db psql -U krakencryptox -d krakencryptox -f /path/to/this/file.sql
--
-- Or run each statement manually:
-- docker compose exec db psql -U krakencryptox -d krakencryptox
-- Then paste each CREATE TABLE statement
-- ============================================

-- Paper Wallet table
CREATE TABLE IF NOT EXISTS paper_wallet (
    id SERIAL PRIMARY KEY,
    currency VARCHAR(20) NOT NULL UNIQUE,
    balance FLOAT NOT NULL DEFAULT 0.0,
    initial_balance FLOAT NOT NULL DEFAULT 0.0,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Paper Trades table
CREATE TABLE IF NOT EXISTS paper_trades (
    id SERIAL PRIMARY KEY,
    opportunity_id INTEGER REFERENCES arbitrage_opportunities(id),
    path VARCHAR(500) NOT NULL,
    legs INTEGER NOT NULL,
    trade_amount FLOAT NOT NULL,
    
    -- Profit calculations
    gross_profit_pct FLOAT NOT NULL,
    fees_pct FLOAT NOT NULL,
    expected_net_profit_pct FLOAT NOT NULL,
    
    -- Slippage
    slippage_pct FLOAT NOT NULL,
    slippage_details JSONB,
    
    -- Final results
    actual_net_profit_pct FLOAT NOT NULL,
    actual_profit_amount FLOAT NOT NULL,
    
    -- Wallet state
    balance_before FLOAT NOT NULL,
    balance_after FLOAT NOT NULL,
    
    -- Status
    status VARCHAR(20) NOT NULL,
    skip_reason VARCHAR(200),
    
    executed_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Paper Trading Settings table
CREATE TABLE IF NOT EXISTS paper_trading_settings (
    id SERIAL PRIMARY KEY,
    is_active BOOLEAN DEFAULT TRUE,
    min_profit_threshold FLOAT DEFAULT 0.1,
    trade_amount FLOAT DEFAULT 100.0,
    cooldown_seconds INTEGER DEFAULT 5,
    base_currency VARCHAR(20) DEFAULT 'USD',
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Create indexes for better performance
CREATE INDEX IF NOT EXISTS idx_paper_trades_executed_at ON paper_trades(executed_at DESC);
CREATE INDEX IF NOT EXISTS idx_paper_trades_status ON paper_trades(status);
CREATE INDEX IF NOT EXISTS idx_paper_wallet_currency ON paper_wallet(currency);

-- Insert default settings if not exists
INSERT INTO paper_trading_settings (is_active, min_profit_threshold, trade_amount, cooldown_seconds, base_currency)
SELECT TRUE, 0.1, 100.0, 5, 'USD'
WHERE NOT EXISTS (SELECT 1 FROM paper_trading_settings LIMIT 1);

-- Insert default wallet if not exists
INSERT INTO paper_wallet (currency, balance, initial_balance)
SELECT 'USD', 100.0, 100.0
WHERE NOT EXISTS (SELECT 1 FROM paper_wallet WHERE currency = 'USD');

-- Verify tables created
SELECT 'paper_wallet' as table_name, COUNT(*) as rows FROM paper_wallet
UNION ALL
SELECT 'paper_trades', COUNT(*) FROM paper_trades
UNION ALL
SELECT 'paper_trading_settings', COUNT(*) FROM paper_trading_settings;
