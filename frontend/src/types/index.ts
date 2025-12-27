// Configuration Status Types
export interface ConfigurationStatusResponse {
  success: boolean;
  status: ConfigurationStatus;
}

export interface ConfigurationStatus {
  is_configured: boolean;
  can_start_engine: boolean;
  missing_fields: string[];
  warnings: string[];
  config_summary: ConfigSummary;
  fee_config: FeeConfigStatus;
}

export interface ConfigSummary {
  start_currency: string | null;  // Note: API uses start_currency, not base_currency
  trade_amount: number | null;
  min_profit_threshold: number | null;
  max_daily_loss: number | null;
  max_total_loss: number | null;
  // Pair Selection Filters (REQUIRED for pair filtering)
  max_pairs: number | null;            // Maximum trading pairs to monitor (30-100 recommended)
  min_volume_24h_usd: number | null;   // Minimum 24h USD volume filter ($50,000+ recommended)
  max_cost_min: number | null;         // Maximum order minimum cost filter ($20+ recommended)
}

export interface FeeConfigStatus {
  is_configured: boolean;
  maker_fee: number | null;
  taker_fee: number | null;
  fee_source: string;  // 'kraken_api', 'manual', 'pending'
  volume_tier: string | null;
  thirty_day_volume: number | null;
  last_fetched_at: string | null;
  last_updated_at: string | null;
}

// Live Trading Config
export interface LiveConfig {
  id: number;
  is_enabled: boolean;
  trade_amount: number | null;
  min_profit_threshold: number | null;
  max_daily_loss: number | null;
  max_total_loss: number | null;
  base_currency: string | null;
  custom_currencies: string[] | null;
  // Pair Selection Filters (REQUIRED for pair filtering)
  max_pairs: number | null;
  min_volume_24h_usd: number | null;
  max_cost_min: number | null;
  created_at: string | null;
  updated_at: string | null;
  enabled_at: string | null;
  disabled_at: string | null;
}

export interface ConfigUpdate {
  trade_amount?: number;
  min_profit_threshold?: number;
  max_daily_loss?: number;
  max_total_loss?: number;
  base_currency?: string;
  // Pair Selection Filters
  max_pairs?: number;
  min_volume_24h_usd?: number;
  max_cost_min?: number;
}

// Live Trading State
export interface LiveState {
  id: number;
  daily_loss: number;
  daily_profit: number;
  daily_trades: number;
  daily_wins: number;
  total_loss: number;
  total_profit: number;
  total_trades: number;
  total_wins: number;
  total_trade_amount: number;
  partial_trades: number;
  partial_estimated_loss: number;
  partial_estimated_profit: number;
  partial_trade_amount: number;
  is_circuit_broken: boolean;
  circuit_broken_at: string | null;
  circuit_broken_reason: string | null;
  last_trade_at: string | null;
  last_daily_reset: string | null;
  is_executing: boolean;
  current_trade_id: string | null;
  created_at: string | null;
  updated_at: string | null;
}

// Live Trade
export interface LiveTrade {
  id: number;
  trade_id: string;
  path: string;
  legs: number;
  amount_in: number;
  amount_out: number | null;
  profit_loss: number | null;
  profit_loss_pct: number | null;
  status: string;
  current_leg: number | null;
  error_message: string | null;
  held_currency: string | null;
  held_amount: number | null;
  held_value_usd: number | null;
  resolved_at: string | null;
  resolved_amount_usd: number | null;
  resolution_trade_id: string | null;
  order_ids: string[] | null;
  leg_fills: LegFill[] | null;
  started_at: string | null;
  completed_at: string | null;
  total_execution_ms: number | null;
  opportunity_profit_pct: number | null;
  created_at: string | null;
}

export interface LegFill {
  leg: number;
  pair: string;
  side: string;
  duration_ms: number;
  success: boolean;
  error: string | null;
}

// Scanner Status
export interface ScannerStatus {
  is_running: boolean;
  is_live_enabled: boolean;
  last_scan_at: string | null;
  pairs_scanned: number;
  scan_duration_ms: number;
  opportunities_found: number;
  scanner_type: string;
}

// Opportunity
export interface Opportunity {
  id: number;
  path: string;
  profit_pct: number;
  profit_amount: number;
  legs: number;
  base_currency: string;
  found_at: string;
  prices: OpportunityPrice[];
}

export interface OpportunityPrice {
  pair: string;
  side: string;
  price: number;
  volume: number;
}

// Status Response
export interface StatusResponse {
  status: string;
  scanner: {
    is_running: boolean;
    last_scan_at: string | null;
    pairs_count: number;
  };
  database: {
    connected: boolean;
  };
  uptime_seconds: number;
}

// Fee Configuration
export interface FeeConfig {
  maker_fee_pct: number;
  taker_fee_pct: number;
  use_maker_orders: boolean;
  fee_buffer_pct: number;
}

export interface FeeStats {
  total_fees_paid: number;
  average_fee_pct: number;
  estimated_savings: number;
}

export interface KrakenFees {
  maker_fee: number;
  taker_fee: number;
  volume_tier: string;
  thirty_day_volume: number;
}

// API Response types
export interface ApiResponse<T> {
  data: T;
  success: boolean;
  message?: string;
}

// Geographic Restrictions
export interface RestrictionsConfig {
  version: string;
  jurisdiction: string;
  jurisdiction_name: string;
  last_updated: string;
  update_source: string;
  regulatory_body: string;
  blocked_base_currencies: string[];
  allowed_specified_assets: string[];
  blocked_pairs: string[];
  notes: string;
  sources: string[];
}
