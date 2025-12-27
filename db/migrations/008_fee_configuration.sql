-- Fee Configuration Table
-- Stores maker/taker fees fetched from Kraken or manually entered

CREATE TABLE IF NOT EXISTS fee_configuration (
    id INTEGER PRIMARY KEY DEFAULT 1,
    maker_fee DOUBLE PRECISION NOT NULL DEFAULT 0,    -- e.g., 0.0016 = 0.16%
    taker_fee DOUBLE PRECISION NOT NULL DEFAULT 0,    -- e.g., 0.0026 = 0.26%
    fee_source VARCHAR(20) NOT NULL DEFAULT 'pending', -- 'kraken_api', 'manual', 'pending'
    volume_tier VARCHAR(50),                           -- e.g., "Pro", "Starter", etc.
    thirty_day_volume DOUBLE PRECISION,               -- 30-day trading volume in USD
    last_fetched_at TIMESTAMPTZ,                       -- When fees were last fetched from Kraken
    last_updated_at TIMESTAMPTZ DEFAULT NOW(),         -- When config was last modified
    created_at TIMESTAMPTZ DEFAULT NOW(),

    -- Ensure only one row exists
    CONSTRAINT fee_configuration_single_row CHECK (id = 1)
);

-- Insert initial row with pending state (user must configure or fetch)
INSERT INTO fee_configuration (id, maker_fee, taker_fee, fee_source)
VALUES (1, 0, 0, 'pending')
ON CONFLICT (id) DO NOTHING;

-- Add comment explaining the table
COMMENT ON TABLE fee_configuration IS 'Stores Kraken maker/taker fee configuration. Fees should be fetched from Kraken API or manually entered by user.';
COMMENT ON COLUMN fee_configuration.fee_source IS 'Source of fee data: kraken_api (auto-fetched), manual (user entered), pending (not yet configured)';
COMMENT ON COLUMN fee_configuration.volume_tier IS 'Kraken volume tier from TradeVolume API';
COMMENT ON COLUMN fee_configuration.thirty_day_volume IS '30-day trading volume in USD from Kraken API';

-- Create trigger to update last_updated_at
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
