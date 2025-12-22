import axios from 'axios';

const API_URL = process.env.REACT_APP_API_URL || 'http://localhost:8000';

const client = axios.create({
  baseURL: API_URL,
  timeout: 120000,
  headers: {
    'Content-Type': 'application/json',
  },
});

export const api = {
  // Scanner status
  async getStatus() {
    const response = await client.get('/api/status');
    return response.data;
  },

  // Get order book health stats
  async getOrderbookHealth() {
    const response = await client.get('/api/orderbook-health');
    return response.data;
  },

  // Get order book health history for trends
  async getOrderbookHealthHistory(hours = 24) {
    const response = await client.get(`/api/orderbook-health/history?hours=${hours}`);
    return response.data;
  },

  // Get opportunities with sorting and filtering
  async getOpportunities(options = {}) {
    const {
      limit = 50,
      profitable_only = true,
      hours = 24,
      sort_by = 'time',
      base_currency = 'ALL',
      min_profit_pct = null,
      minutes_ago = null,
    } = options;

    let url = `/api/opportunities?limit=${limit}&profitable_only=${profitable_only}&hours=${hours}&sort_by=${sort_by}`;

    if (minutes_ago && minutes_ago > 0) {
      url += `&minutes_ago=${minutes_ago}`;
    }

    if (base_currency && base_currency !== 'ALL') {
      url += `&base_currency=${base_currency}`;
    }

    if (min_profit_pct !== null) {
      url += `&min_profit_pct=${min_profit_pct}`;
    }

    const response = await client.get(url);
    return response.data;
  },

  // Get best opportunities
  async getBestOpportunities(limit = 10) {
    const response = await client.get(`/api/opportunities/best?limit=${limit}`);
    return response.data;
  },

  // Get live prices
  async getLivePrices(limit = 50) {
    const response = await client.get(`/api/prices/live?limit=${limit}`);
    return response.data;
  },

  // Get price matrix
  async getPriceMatrix(currencies = null) {
    let url = '/api/prices/matrix';
    if (currencies) {
      url += `?currencies=${currencies.join(',')}`;
    }
    const response = await client.get(url);
    return response.data;
  },

  // Get trading pairs
  async getPairs(activeOnly = true, limit = 100) {
    const response = await client.get(`/api/pairs?active_only=${activeOnly}&limit=${limit}`);
    return response.data;
  },

  // Get pair details
  async getPairDetails(pairName) {
    const response = await client.get(`/api/pairs/${encodeURIComponent(pairName)}`);
    return response.data;
  },

  // Get currencies
  async getCurrencies(type = null) {
    let url = '/api/currencies';
    if (type) {
      url += `?currency_type=${type}`;
    }
    const response = await client.get(url);
    return response.data;
  },

  // Get currency connections
  async getCurrencyConnections(symbol) {
    const response = await client.get(`/api/currencies/${symbol}/connections`);
    return response.data;
  },

  // Trigger manual scan
  async triggerScan(baseCurrencies = null) {
    let url = '/api/scan';
    if (baseCurrencies) {
      url += `?base_currencies=${baseCurrencies.join(',')}`;
    }
    const response = await client.post(url);
    return response.data;
  },

  // Get opportunity detail
  async getOpportunityDetail(id) {
    const response = await client.get(`/api/opportunities/${id}`);
    return response.data;
  },

  // ==================== ENGINE SETTINGS API ====================

  // Get engine settings (scan interval, max pairs, depth)
  async getEngineSettings() {
    const response = await client.get('/api/engine-settings');
    return response.data;
  },

  // Update engine settings (requires restart)
  async updateEngineSettings(settings) {
    const params = new URLSearchParams();
    if (settings.scan_interval_ms !== undefined) {
      params.append('scan_interval_ms', settings.scan_interval_ms);
    }
    if (settings.max_pairs !== undefined) {
      params.append('max_pairs', settings.max_pairs);
    }
    if (settings.orderbook_depth !== undefined) {
      params.append('orderbook_depth', settings.orderbook_depth);
    }
    if (settings.scanner_enabled !== undefined) {
      params.append('scanner_enabled', settings.scanner_enabled);
    }
    const response = await client.put(`/api/engine-settings?${params.toString()}`);
    return response.data;
  },

  // Restart engine
  async restartEngine() {
    const response = await client.post('/api/engine/restart');
    return response.data;
  },

  // ==================== OPPORTUNITY HISTORY API ====================

  // Get opportunity history
  async getOpportunityHistory(options = {}) {
    const { limit = 100, hours = 24, startCurrency = null, profitableOnly = false } = options;
    let url = `/api/opportunities/history?limit=${limit}&hours=${hours}&profitable_only=${profitableOnly}`;
    if (startCurrency) {
      url += `&start_currency=${startCurrency}`;
    }
    const response = await client.get(url);
    return response.data;
  },

  // Get opportunity history stats
  async getOpportunityHistoryStats(hours = 24) {
    const response = await client.get(`/api/opportunities/history/stats?hours=${hours}`);
    return response.data;
  },

  // ==================== LIVE TRADING API ====================

  // Get live trading status
  async getLiveStatus() {
    const response = await client.get('/api/live/status');
    return response.data;
  },

  // Get live trading state (circuit breaker with partial stats)
  async getLiveState() {
    const response = await client.get('/api/live/state');
    return response.data;
  },

  // Get live trading config
  async getLiveConfig() {
    const response = await client.get('/api/live/config');
    return response.data;
  },

  // Update live trading config
  async updateLiveConfig(config) {
    const response = await client.put('/api/live/config', config);
    return response.data;
  },

  // Enable live trading
  async enableLiveTrading(confirm = false, confirmText = '') {
    const response = await client.post('/api/live/enable', {
      confirm,
      confirm_text: confirmText,
    });
    return response.data;
  },

  // Disable live trading
  async disableLiveTrading(reason = 'Manual disable') {
    const response = await client.post(`/api/live/disable?reason=${encodeURIComponent(reason)}`);
    return response.data;
  },

  // Get live trades
  async getLiveTrades(limit = 50, status = null, hours = 24) {
    let url = `/api/live/trades?limit=${limit}&hours=${hours}`;
    if (status) url += `&status=${status}`;
    const response = await client.get(url);
    return response.data;
  },

  // Get partial trades only
  async getLivePartialTrades() {
    const response = await client.get('/api/live/trades/partial');
    return response.data;
  },

  // Get single live trade
  async getLiveTrade(tradeId) {
    const response = await client.get(`/api/live/trades/${tradeId}`);
    return response.data;
  },

  // Preview resolving a partial trade
  async previewResolvePartial(tradeId) {
    const response = await client.get(`/api/live/trades/${tradeId}/resolve-preview`);
    return response.data;
  },

  // Resolve a partial trade (sell held crypto back to USD)
  async resolvePartialTrade(tradeId) {
    const response = await client.post(`/api/live/trades/${tradeId}/resolve`);
    return response.data;
  },

  // Get live positions
  async getLivePositions() {
    const response = await client.get('/api/live/positions');
    return response.data;
  },

  // Get circuit breaker status
  async getLiveCircuitBreaker() {
    const response = await client.get('/api/live/circuit-breaker');
    return response.data;
  },

  // Reset circuit breaker
  async resetLiveCircuitBreaker() {
    const response = await client.post('/api/live/circuit-breaker/reset');
    return response.data;
  },

  // Trigger circuit breaker manually
  async triggerLiveCircuitBreaker(reason = 'Manual trigger') {
    const response = await client.post(`/api/live/circuit-breaker/trigger?reason=${encodeURIComponent(reason)}`);
    return response.data;
  },

  // Reset daily stats
  async resetLiveDaily() {
    const response = await client.post('/api/live/reset-daily');
    return response.data;
  },

  // Reset all stats
  async resetLiveAll(confirm = false, confirmText = '') {
    const response = await client.post(`/api/live/reset-all?confirm=${confirm}&confirm_text=${encodeURIComponent(confirmText)}`);
    return response.data;
  },

  // Reset partial stats only
  async resetLivePartial(confirm = false) {
    const response = await client.post(`/api/live/reset-partial?confirm=${confirm}`);
    return response.data;
  },

  // Execute manual trade
  async executeLiveTrade(path, amount = null) {
    const response = await client.post('/api/live/execute', { path, amount });
    return response.data;
  },

  // Quick disable (emergency stop)
  async liveQuickDisable() {
    const response = await client.post('/api/live/quick-disable');
    return response.data;
  },

  // NOTE: getLiveOpportunities removed - was dead code (LiveOpportunity table never written to)
  // All scanning happens in Rust, trade results are in live_trades table

  // Get live scanner status
  async getLiveScannerStatus() {
    const response = await client.get('/api/live/scanner/status');
    return response.data;
  },

  // Start live scanner
  async startLiveScanner() {
    const response = await client.post('/api/live/scanner/start');
    return response.data;
  },

  // Stop live scanner
  async stopLiveScanner() {
    const response = await client.post('/api/live/scanner/stop');
    return response.data;
  },

  // ==================== RUST EXECUTION ENGINE API ====================

  // Initialize Rust execution engine with API credentials
  async initRustExecutionEngine(apiKey, apiSecret) {
    const response = await client.post('/api/live/execution-engine/init', {
      api_key: apiKey,
      api_secret: apiSecret,
    });
    return response.data;
  },

  // Connect Rust execution engine to Kraken private WebSocket
  async connectRustExecutionEngine() {
    const response = await client.post('/api/live/execution-engine/connect');
    return response.data;
  },

  // Disconnect Rust execution engine
  async disconnectRustExecutionEngine() {
    const response = await client.post('/api/live/execution-engine/disconnect');
    return response.data;
  },

  // Get Rust execution engine status
  async getRustExecutionEngineStatus() {
    const response = await client.get('/api/live/execution-engine/status');
    return response.data;
  },

  // Execute trade via Rust engine
  async executeViaRustEngine(path, amount, mode = 'sequential', balances = null) {
    const response = await client.post('/api/live/execution-engine/execute', {
      path,
      amount,
      mode,
      balances,
    });
    return response.data;
  },

  // Get fee configuration
  async getFeeConfig() {
    const response = await client.get('/api/live/fee-config');
    return response.data;
  },

  // Update fee configuration
  async updateFeeConfig(config) {
    const response = await client.put('/api/live/fee-config', config);
    return response.data;
  },

  // Get fee optimization statistics
  async getFeeStats() {
    const response = await client.get('/api/live/fee-stats');
    return response.data;
  },

  // Get real Kraken account fees (from your trading volume tier)
  async getKrakenFees() {
    const response = await client.get('/api/live/kraken-fees');
    return response.data;
  },

  // Sync Kraken fees to engine (fetch real fees and update engine)
  async syncKrakenFees() {
    const response = await client.post('/api/live/sync-kraken-fees');
    return response.data;
  },

  // Analyze parallel execution opportunity
  async analyzeParallelExecution(path, amount, balances = null) {
    const response = await client.post('/api/live/analyze-parallel', {
      path,
      amount,
      balances,
    });
    return response.data;
  },
};

export default api;
