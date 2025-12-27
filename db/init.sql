-- KrakenCryptoX Database Schema
-- Multi-Pair Arbitrage Opportunity Scanner

-- Enable UUID extension
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- ============================================
-- TABLE: trading_pairs
-- All available trading pairs on Kraken
-- ============================================
CREATE TABLE trading_pairs (
    id SERIAL PRIMARY KEY,
    pair_name VARCHAR(20) NOT NULL UNIQUE,      -- e.g., 'BTC/USDT'
    base_currency VARCHAR(10) NOT NULL,          -- e.g., 'BTC'
    quote_currency VARCHAR(10) NOT NULL,         -- e.g., 'USDT'
    kraken_symbol VARCHAR(20) NOT NULL UNIQUE,   -- Kraken's internal symbol
    is_active BOOLEAN DEFAULT TRUE,
    min_volume DECIMAL(20, 10),                  -- Minimum order size
    price_decimals INT,                          -- Price precision
    volume_decimals INT,                         -- Volume precision
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Index for quick lookups
CREATE INDEX idx_trading_pairs_base ON trading_pairs(base_currency);
CREATE INDEX idx_trading_pairs_quote ON trading_pairs(quote_currency);
CREATE INDEX idx_trading_pairs_active ON trading_pairs(is_active);

-- ============================================
-- TABLE: currencies
-- All currencies (coins/tokens/fiat)
-- ============================================
CREATE TABLE currencies (
    id SERIAL PRIMARY KEY,
    symbol VARCHAR(10) NOT NULL UNIQUE,          -- e.g., 'BTC', 'USDT', 'USD'
    name VARCHAR(100),                           -- e.g., 'Bitcoin'
    currency_type VARCHAR(20) NOT NULL,          -- 'crypto', 'fiat', 'stablecoin'
    is_active BOOLEAN DEFAULT TRUE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- ============================================
-- TABLE: price_ticks
-- Real-time price data (high frequency)
-- ============================================
CREATE TABLE price_ticks (
    id BIGSERIAL PRIMARY KEY,
    pair_id INT NOT NULL REFERENCES trading_pairs(id),
    bid_price DECIMAL(20, 10) NOT NULL,          -- Best bid
    ask_price DECIMAL(20, 10) NOT NULL,          -- Best ask
    bid_volume DECIMAL(20, 10),                  -- Volume at bid
    ask_volume DECIMAL(20, 10),                  -- Volume at ask
    last_price DECIMAL(20, 10),                  -- Last trade price
    volume_24h DECIMAL(20, 10),                  -- 24h volume
    timestamp TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Partition by time for efficient querying (keep recent data fast)
CREATE INDEX idx_price_ticks_pair_time ON price_ticks(pair_id, timestamp DESC);
CREATE INDEX idx_price_ticks_timestamp ON price_ticks(timestamp DESC);

-- ============================================
-- TABLE: arbitrage_opportunities
-- Detected arbitrage opportunities
-- ============================================
CREATE TABLE arbitrage_opportunities (
    id BIGSERIAL PRIMARY KEY,
    opportunity_id UUID DEFAULT uuid_generate_v4(),
    path TEXT NOT NULL,                          -- e.g., 'USDT->BTC->ETH->USDT'
    path_pairs TEXT[] NOT NULL,                  -- Array of pair names
    legs INT NOT NULL,                           -- Number of trades
    start_currency VARCHAR(10) NOT NULL,
    start_amount DECIMAL(20, 10) NOT NULL,
    end_amount DECIMAL(20, 10) NOT NULL,
    gross_profit_pct DECIMAL(10, 6) NOT NULL,    -- Before fees
    total_fees_pct DECIMAL(10, 6) NOT NULL,      -- Total fees
    net_profit_pct DECIMAL(10, 6) NOT NULL,      -- After fees
    net_profit_amount DECIMAL(20, 10) NOT NULL,
    is_profitable BOOLEAN NOT NULL,
    min_volume_available DECIMAL(20, 10),        -- Limiting factor
    prices_snapshot JSONB,                       -- Price data at detection
    detected_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expired_at TIMESTAMP                         -- When opportunity closed
);

-- Indexes for analysis
CREATE INDEX idx_opportunities_profitable ON arbitrage_opportunities(is_profitable, detected_at DESC);
CREATE INDEX idx_opportunities_profit ON arbitrage_opportunities(net_profit_pct DESC);
CREATE INDEX idx_opportunities_time ON arbitrage_opportunities(detected_at DESC);
CREATE INDEX idx_opportunities_path ON arbitrage_opportunities(path);

-- ============================================
-- TABLE: price_matrix
-- Current prices in matrix form for quick lookups
-- ============================================
CREATE TABLE price_matrix (
    id SERIAL PRIMARY KEY,
    base_currency VARCHAR(10) NOT NULL,
    quote_currency VARCHAR(10) NOT NULL,
    bid_price DECIMAL(20, 10),
    ask_price DECIMAL(20, 10),
    mid_price DECIMAL(20, 10),
    spread_pct DECIMAL(10, 6),
    volume_24h DECIMAL(20, 10),
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(base_currency, quote_currency)
);

CREATE INDEX idx_price_matrix_base ON price_matrix(base_currency);
CREATE INDEX idx_price_matrix_quote ON price_matrix(quote_currency);

-- ============================================
-- TABLE: scanner_stats
-- Hourly/daily statistics
-- ============================================
CREATE TABLE scanner_stats (
    id SERIAL PRIMARY KEY,
    period_start TIMESTAMP NOT NULL,
    period_end TIMESTAMP NOT NULL,
    period_type VARCHAR(10) NOT NULL,            -- 'hour', 'day'
    opportunities_found INT DEFAULT 0,
    profitable_opportunities INT DEFAULT 0,
    best_profit_pct DECIMAL(10, 6),
    avg_profit_pct DECIMAL(10, 6),
    total_volume_scanned DECIMAL(20, 2),
    pairs_monitored INT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(period_start, period_type)
);

-- ============================================
-- NOTE: system_config table REMOVED
-- All configuration now comes from:
-- - User input via dashboard (live_trading_config table)
-- - Kraken API (fee_configuration table)
-- - No hardcoded defaults allowed
-- ============================================

-- ============================================
-- VIEW: latest_prices
-- Most recent price for each pair
-- ============================================
CREATE VIEW latest_prices AS
SELECT DISTINCT ON (pair_id)
    pt.pair_id,
    tp.pair_name,
    tp.base_currency,
    tp.quote_currency,
    pt.bid_price,
    pt.ask_price,
    pt.last_price,
    pt.volume_24h,
    pt.timestamp
FROM price_ticks pt
JOIN trading_pairs tp ON pt.pair_id = tp.id
ORDER BY pair_id, timestamp DESC;

-- ============================================
-- VIEW: profitable_opportunities_24h
-- Profitable opportunities in last 24 hours
-- ============================================
CREATE VIEW profitable_opportunities_24h AS
SELECT 
    path,
    legs,
    net_profit_pct,
    net_profit_amount,
    detected_at
FROM arbitrage_opportunities
WHERE is_profitable = TRUE
    AND detected_at > NOW() - INTERVAL '24 hours'
ORDER BY net_profit_pct DESC;

-- ============================================
-- FUNCTION: Update timestamp trigger
-- ============================================
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ language 'plpgsql';

-- Apply trigger to tables with updated_at
CREATE TRIGGER update_trading_pairs_updated_at
    BEFORE UPDATE ON trading_pairs
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_price_matrix_updated_at
    BEFORE UPDATE ON price_matrix
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- NOTE: system_config table was removed - trigger removed as well

-- ============================================
-- FUNCTION: Clean old price ticks (keep 24h)
-- ============================================
CREATE OR REPLACE FUNCTION clean_old_price_ticks()
RETURNS void AS $$
BEGIN
    DELETE FROM price_ticks
    WHERE timestamp < NOW() - INTERVAL '24 hours';
END;
$$ LANGUAGE plpgsql;

-- Grant permissions (if needed for app user)
-- GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA public TO krakencryptox;
-- GRANT ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public TO krakencryptox;
