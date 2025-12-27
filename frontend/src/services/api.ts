import axios, { AxiosInstance } from 'axios'
import type {
  ConfigurationStatus,
  LiveConfig,
  LiveState,
  LiveTrade,
  ScannerStatus,
  Opportunity,
  StatusResponse,
  FeeStats,
  KrakenFees,
  ConfigUpdate,
  RestrictionsConfig,
  PositionsResponse,
} from '../types'

const API_URL = import.meta.env.VITE_API_URL || 'http://localhost:8000'
const API_KEY = import.meta.env.VITE_API_KEY || ''

const client: AxiosInstance = axios.create({
  baseURL: API_URL,
  timeout: 120000,
  headers: {
    'Content-Type': 'application/json',
    ...(API_KEY && { 'X-API-Key': API_KEY }),
  },
})

// Add request interceptor to ensure API key is always included
client.interceptors.request.use((config) => {
  if (API_KEY && !config.headers['X-API-Key']) {
    config.headers['X-API-Key'] = API_KEY
  }
  return config
})

// Add response interceptor to handle auth errors
client.interceptors.response.use(
  (response) => response,
  (error) => {
    if (error.response?.status === 401) {
      console.error('API Authentication required. Set VITE_API_KEY in your .env file.')
    } else if (error.response?.status === 403) {
      console.error('API Authentication failed. Check your VITE_API_KEY.')
    }
    return Promise.reject(error)
  }
)

export interface GetOpportunitiesOptions {
  limit?: number
  profitable_only?: boolean
  hours?: number
  sort_by?: string
  base_currency?: string
  min_profit_pct?: number | null
  minutes_ago?: number | null
}

export const api = {
  // ==================== STATUS ====================
  async getStatus(): Promise<StatusResponse> {
    const response = await client.get('/api/status')
    return response.data
  },

  // ==================== CONFIGURATION STATUS ====================
  async getConfigurationStatus(): Promise<ConfigurationStatus> {
    const response = await client.get('/api/live/configuration-status')
    // API returns { success: true, status: {...} } - extract status
    return response.data.status
  },

  // ==================== LIVE TRADING CONFIG ====================
  async getLiveConfig(): Promise<LiveConfig> {
    const response = await client.get('/api/live/config')
    // API returns config directly (simplified)
    return response.data
  },

  async updateLiveConfig(config: ConfigUpdate): Promise<LiveConfig> {
    const response = await client.put('/api/live/config', config)
    // API returns { success: true, message: "...", config: {...} } - extract config
    return response.data.config
  },

  // ==================== LIVE TRADING STATE ====================
  async getLiveState(): Promise<LiveState> {
    const response = await client.get('/api/live/state')
    // API returns { success: true, data: {...} } - extract data
    return response.data.data
  },

  async getLiveStatus(): Promise<{ config: LiveConfig; state: LiveState }> {
    const response = await client.get('/api/live/status')
    return response.data
  },

  // ==================== ACCOUNT POSITIONS ====================
  async getPositions(): Promise<PositionsResponse> {
    const response = await client.get('/api/live/positions')
    return response.data
  },

  // ==================== TRADING CONTROLS ====================
  async enableLiveTrading(confirm = false, confirmText = ''): Promise<{ success: boolean; message: string }> {
    const response = await client.post('/api/live/enable', {
      confirm,
      confirm_text: confirmText,
    })
    return response.data
  },

  async disableLiveTrading(reason = 'Manual disable'): Promise<{ success: boolean; message: string }> {
    const response = await client.post('/api/live/disable', { reason })
    return response.data
  },

  async liveQuickDisable(): Promise<{ success: boolean; message: string }> {
    const response = await client.post('/api/live/quick-disable')
    return response.data
  },

  // ==================== CIRCUIT BREAKER ====================
  async getLiveCircuitBreaker(): Promise<{ is_broken: boolean; reason: string | null }> {
    const response = await client.get('/api/live/circuit-breaker')
    return response.data
  },

  async resetLiveCircuitBreaker(): Promise<{ success: boolean; message: string }> {
    const response = await client.post('/api/live/circuit-breaker/reset')
    return response.data
  },

  async triggerLiveCircuitBreaker(reason = 'Manual trigger'): Promise<{ success: boolean; message: string }> {
    const response = await client.post(`/api/live/circuit-breaker/trigger?reason=${encodeURIComponent(reason)}`)
    return response.data
  },

  // ==================== LIVE TRADES ====================
  async getLiveTrades(limit = 50, status: string | null = null, hours = 24): Promise<LiveTrade[]> {
    let url = `/api/live/trades?limit=${limit}&hours=${hours}`
    if (status) url += `&status=${status}`
    const response = await client.get(url)
    // API returns { count: N, trades: [...] } - extract trades
    return response.data.trades
  },

  async getLivePartialTrades(): Promise<LiveTrade[]> {
    const response = await client.get('/api/live/trades/partial')
    // API returns { count: N, trades: [...] } - extract trades
    return response.data.trades
  },

  async getLiveTrade(tradeId: string): Promise<LiveTrade> {
    const response = await client.get(`/api/live/trades/${tradeId}`)
    return response.data
  },

  async previewResolvePartial(tradeId: string): Promise<{
    trade_id: string
    held_currency: string
    held_amount: number
    current_value_usd: number
    estimated_return: number
    profit_loss: number
  }> {
    const response = await client.get(`/api/live/trades/${tradeId}/resolve-preview`)
    return response.data
  },

  async resolvePartialTrade(tradeId: string): Promise<{ success: boolean; message: string }> {
    const response = await client.post(`/api/live/trades/${tradeId}/resolve`)
    return response.data
  },

  // ==================== SCANNER ====================
  async getLiveScannerStatus(): Promise<ScannerStatus> {
    const response = await client.get('/api/live/scanner/status')
    // API returns { success: true, data: {...} } - extract data
    return response.data.data
  },

  async startLiveScanner(): Promise<{ success: boolean; message: string }> {
    const response = await client.post('/api/live/scanner/start')
    return response.data
  },

  async stopLiveScanner(): Promise<{ success: boolean; message: string }> {
    const response = await client.post('/api/live/scanner/stop')
    return response.data
  },

  // ==================== ENGINE CONTROL ====================
  async restartEngine(): Promise<{ success: boolean; message: string }> {
    const response = await client.post('/api/engine/restart')
    return response.data
  },

  // ==================== OPPORTUNITIES ====================
  async getOpportunities(options: GetOpportunitiesOptions = {}): Promise<Opportunity[]> {
    const {
      limit = 50,
      profitable_only = true,
      hours = 24,
      sort_by = 'time',
      base_currency = 'ALL',
      min_profit_pct = null,
      minutes_ago = null,
    } = options

    let url = `/api/opportunities?limit=${limit}&profitable_only=${profitable_only}&hours=${hours}&sort_by=${sort_by}`

    if (minutes_ago && minutes_ago > 0) {
      url += `&minutes_ago=${minutes_ago}`
    }

    if (base_currency && base_currency !== 'ALL') {
      url += `&base_currency=${base_currency}`
    }

    if (min_profit_pct !== null) {
      url += `&min_profit_pct=${min_profit_pct}`
    }

    const response = await client.get(url)
    return response.data
  },

  async getBestOpportunities(limit = 10): Promise<Opportunity[]> {
    const response = await client.get(`/api/opportunities/best?limit=${limit}`)
    return response.data
  },

  async getOpportunityDetail(id: number): Promise<Opportunity> {
    const response = await client.get(`/api/opportunities/${id}`)
    return response.data
  },

  // ==================== STATS RESET ====================
  async resetLiveDaily(): Promise<{ success: boolean; message: string }> {
    const response = await client.post('/api/live/reset-daily')
    return response.data
  },

  async resetLiveAll(confirm = false, confirmText = ''): Promise<{ success: boolean; message: string }> {
    const response = await client.post(`/api/live/reset-all?confirm=${confirm}&confirm_text=${encodeURIComponent(confirmText)}`)
    return response.data
  },

  async resetLivePartial(confirm = false): Promise<{ success: boolean; message: string }> {
    const response = await client.post(`/api/live/reset-partial?confirm=${confirm}`)
    return response.data
  },

  // ==================== FEE CONFIGURATION ====================
  async getFeeConfig(): Promise<{
    success: boolean;
    data: {
      maker_fee: number;
      taker_fee: number;
      fee_source: string;
      volume_tier: string | null;
      thirty_day_volume: number | null;
      last_fetched_at: string | null;
      last_updated_at: string | null;
      is_configured: boolean;
    };
  }> {
    const response = await client.get('/api/fees')
    return response.data
  },

  async updateFeeConfig(makerFee: number, takerFee: number): Promise<{
    success: boolean;
    message: string;
    data: {
      maker_fee: number;
      taker_fee: number;
      fee_source: string;
      last_updated_at: string;
    };
  }> {
    const response = await client.put('/api/fees', {
      maker_fee: makerFee,
      taker_fee: takerFee,
    })
    return response.data
  },

  async fetchFeesFromKraken(): Promise<{
    success: boolean;
    message: string;
    error?: string;
    data?: {
      maker_fee: number;
      taker_fee: number;
      fee_source: string;
      thirty_day_volume: number | null;
      last_fetched_at: string;
    };
  }> {
    const response = await client.post('/api/fees/fetch')
    return response.data
  },

  async getFeeStats(): Promise<FeeStats> {
    const response = await client.get('/api/fees/stats')
    return response.data
  },

  async getKrakenFees(): Promise<KrakenFees> {
    const response = await client.get('/api/live/kraken-fees')
    return response.data
  },

  // ==================== MANUAL TRADE ====================
  async executeLiveTrade(path: string, amount: number | null = null): Promise<{
    success: boolean
    trade_id: string
    message: string
  }> {
    const response = await client.post('/api/live/execute', { path, amount })
    return response.data
  },

  // ==================== RESTRICTIONS ====================
  async getRestrictions(): Promise<RestrictionsConfig> {
    const response = await client.get('/api/config/restrictions')
    return response.data.data
  },

  async addBlockedCurrency(currency: string): Promise<{ success: boolean; message: string }> {
    const response = await client.post('/api/config/restrictions/add', { currency })
    return response.data
  },

  async removeBlockedCurrency(currency: string): Promise<{ success: boolean; message: string }> {
    const response = await client.post('/api/config/restrictions/remove', { currency })
    return response.data
  },

  async reloadRestrictions(): Promise<{ success: boolean; message: string }> {
    const response = await client.post('/api/config/restrictions/reload')
    return response.data
  },
}

export default api
