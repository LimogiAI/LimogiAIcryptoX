// ============================================
// LimogiAICryptoX - Live Trading Panel
// Real money trading with Kraken
// ============================================

import React, { useState, useEffect, useCallback } from 'react';
import { api } from '../services/api';

export function LiveTradingPanel() {
  const [status, setStatus] = useState(null);
  const [config, setConfig] = useState(null);
  // eslint-disable-next-line no-unused-vars
  const [options, setOptions] = useState(null);
  const [trades, setTrades] = useState([]);
  const [positions, setPositions] = useState(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  const [toast, setToast] = useState(null);
  
  // New: Opportunities and Scanner Status
  const [opportunities, setOpportunities] = useState([]);
  const [scannerStatus, setScannerStatus] = useState(null);
  
  // Partial trades tracking
  const [partialTrades, setPartialTrades] = useState([]);
  const [resolvingTradeId, setResolvingTradeId] = useState(null);
  const [resolvePreview, setResolvePreview] = useState(null);
  const [showResolveModal, setShowResolveModal] = useState(false);
  
  const [showEnableModal, setShowEnableModal] = useState(false);
  const [pendingConfig, setPendingConfig] = useState({});
  const [showCustomCurrencies, setShowCustomCurrencies] = useState(false);
  const [customCurrencies, setCustomCurrencies] = useState([]);
  const [customAmount, setCustomAmount] = useState('');
  
  // Filters for trades
  const [hoursFilter, setHoursFilter] = useState(24);
  const [resultFilter, setResultFilter] = useState('');
  const [pathFilter, setPathFilter] = useState('');
  
  // Pagination
  const [currentPage, setCurrentPage] = useState(1);
  const pageSize = 20;
  
  // Expanded row for leg details
  const [expandedTradeId, setExpandedTradeId] = useState(null);
  
  // Expandable risks section
  const [showRisks, setShowRisks] = useState(false);

  // Rust Execution Engine state
  const [rustEngineStatus, setRustEngineStatus] = useState(null);
  const [feeConfig, setFeeConfig] = useState(null);
  const [feeStats, setFeeStats] = useState(null);
  const [showFeeSettings, setShowFeeSettings] = useState(false);

  const AVAILABLE_CURRENCIES = ['USD', 'USDT', 'EUR', 'BTC', 'ETH'];
  const PRESET_AMOUNTS = [5, 10, 20, 50, 100];

  const showToast = (message, type = 'success') => {
    setToast({ message, type });
    setTimeout(() => setToast(null), 4000);
  };

  const fetchData = useCallback(async () => {
    try {
      setError(null);
      const [statusRes, configRes, tradesRes, positionsRes, opportunitiesRes, scannerRes, partialRes] = await Promise.all([
        api.getLiveStatus(),
        api.getLiveConfig(),
        api.getLiveTrades(500), // Fetch more for filtering
        api.getLivePositions(),
        api.getLiveOpportunities ? api.getLiveOpportunities(100, null, 24) : Promise.resolve({ opportunities: [] }),
        api.getLiveScannerStatus ? api.getLiveScannerStatus() : Promise.resolve(null),
        api.getLivePartialTrades ? api.getLivePartialTrades() : Promise.resolve({ trades: [] }),
      ]);
      setStatus(statusRes);
      setConfig(configRes.config);
      setOptions(configRes.options);
      setTrades(tradesRes.trades || []);
      setPositions(positionsRes);
      setOpportunities(opportunitiesRes?.opportunities || []);
      setScannerStatus(scannerRes);
      setPartialTrades(partialRes?.trades || []);
      if (configRes.config?.custom_currencies) setCustomCurrencies(configRes.config.custom_currencies);
      if (configRes.config?.base_currency === 'CUSTOM') setShowCustomCurrencies(true);

      // Fetch Rust execution engine status
      try {
        const [rustStatus, feeConfigRes, feeStatsRes] = await Promise.all([
          api.getRustExecutionEngineStatus().catch(() => null),
          api.getFeeConfig().catch(() => null),
          api.getFeeStats().catch(() => null),
        ]);
        setRustEngineStatus(rustStatus);
        setFeeConfig(feeConfigRes);
        setFeeStats(feeStatsRes);
      } catch (rustErr) {
        // Rust engine may not be initialized - that's OK
      }
    } catch (err) {
      console.error('Error fetching live trading data:', err);
      setError(err.message || 'Failed to fetch live trading data');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchData();
    const interval = setInterval(fetchData, 5000);
    return () => clearInterval(interval);
  }, [fetchData]);

  const handleEnable = async () => {
    try {
      const result = await api.enableLiveTrading(true, 'I understand the risks');
      showToast(result.message, 'success');
      setShowEnableModal(false);
      fetchData();
    } catch (err) {
      showToast(err.response?.data?.detail || 'Failed to enable live trading', 'error');
    }
  };

  const handleDisable = async () => {
    try {
      await api.disableLiveTrading('Manual disable');
      showToast('Live trading disabled', 'success');
      fetchData();
    } catch (err) {
      showToast('Failed to disable live trading', 'error');
    }
  };

  const handleEmergencyStop = async () => {
    try {
      await api.liveQuickDisable();
      showToast('🛑 Emergency stop activated', 'warning');
      fetchData();
    } catch (err) {
      showToast('Failed to stop trading', 'error');
    }
  };

  const handleConfigChange = (key, value) => setPendingConfig(prev => ({ ...prev, [key]: value }));

  const handleApplyConfig = async () => {
    if (Object.keys(pendingConfig).length === 0) return;
    try {
      await api.updateLiveConfig(pendingConfig);
      showToast('Configuration updated', 'success');
      setPendingConfig({});
      fetchData();
    } catch (err) {
      showToast(err.response?.data?.detail || 'Failed to update config', 'error');
    }
  };

  const getConfigValue = (key, defaultValue) => {
    if (pendingConfig[key] !== undefined) return pendingConfig[key];
    if (config?.[key] !== undefined) return config[key];
    return defaultValue;
  };

  const handleBaseCurrencyChange = (value) => {
    handleConfigChange('base_currency', value);
    setShowCustomCurrencies(value === 'CUSTOM');
  };

  const handleCustomCurrencyToggle = (currency) => {
    const updated = customCurrencies.includes(currency) ? customCurrencies.filter(c => c !== currency) : [...customCurrencies, currency];
    setCustomCurrencies(updated);
    handleConfigChange('custom_currencies', updated);
  };

  const handleCustomAmountSubmit = () => {
    const amount = parseFloat(customAmount);
    if (!isNaN(amount) && amount > 0) {
      handleConfigChange('trade_amount', amount);
      setCustomAmount('');
      showToast(`Trade amount set to $${amount}`, 'success');
    } else {
      showToast('Please enter a valid amount', 'error');
    }
  };

  const handleResetCircuitBreaker = async () => {
    try {
      await api.resetLiveCircuitBreaker();
      showToast('Circuit breaker reset', 'success');
      fetchData();
    } catch (err) {
      showToast('Failed to reset circuit breaker', 'error');
    }
  };

  const handleResetDaily = async () => {
    try {
      await api.resetLiveDaily();
      showToast('Daily statistics reset', 'success');
      fetchData();
    } catch (err) {
      showToast('Failed to reset daily stats', 'error');
    }
  };

  // Get fee config value with default
  const getFeeConfigValue = (key, defaultValue) => {
    if (feeConfig?.[key] !== undefined) return feeConfig[key];
    return defaultValue;
  };

  // Partial trade handlers
  const handlePreviewResolve = async (tradeId) => {
    try {
      setResolvingTradeId(tradeId);
      const preview = await api.previewResolvePartial(tradeId);
      setResolvePreview(preview);
      setShowResolveModal(true);
    } catch (err) {
      showToast(err.response?.data?.detail || 'Failed to preview resolution', 'error');
      setResolvingTradeId(null);
    }
  };

  const handleResolvePartial = async () => {
    if (!resolvingTradeId) return;
    try {
      const result = await api.resolvePartialTrade(resolvingTradeId);
      showToast(`Trade resolved: ${result.resolution?.profit_loss >= 0 ? '+' : ''}$${result.resolution?.profit_loss?.toFixed(2)}`, 
        result.resolution?.profit_loss >= 0 ? 'success' : 'warning');
      setShowResolveModal(false);
      setResolvePreview(null);
      setResolvingTradeId(null);
      fetchData();
    } catch (err) {
      showToast(err.response?.data?.detail || 'Failed to resolve trade', 'error');
    }
  };

  const handleCancelResolve = () => {
    setShowResolveModal(false);
    setResolvePreview(null);
    setResolvingTradeId(null);
  };

  // Toggle expanded row
  const toggleExpandRow = (tradeId) => {
    setExpandedTradeId(expandedTradeId === tradeId ? null : tradeId);
  };

  // Calculate total fee from leg_fills
  const calculateTotalFee = (trade) => {
    if (!trade.leg_fills || !Array.isArray(trade.leg_fills)) return null;
    let totalFee = 0;
    trade.leg_fills.forEach(leg => {
      if (leg.fee) totalFee += parseFloat(leg.fee);
    });
    return totalFee > 0 ? totalFee : null;
  };

  // Calculate total slippage from leg_fills
  const calculateTotalSlippage = (trade) => {
    if (!trade.leg_fills || !Array.isArray(trade.leg_fills)) return { pct: null, usd: null };
    let totalSlippagePct = 0;
    let totalSlippageUsd = 0;
    let hasSlippageData = false;
    
    trade.leg_fills.forEach(leg => {
      if (leg.slippage_pct !== undefined && leg.slippage_pct !== null) {
        totalSlippagePct += parseFloat(leg.slippage_pct);
        hasSlippageData = true;
      }
      if (leg.slippage_usd !== undefined && leg.slippage_usd !== null) {
        totalSlippageUsd += parseFloat(leg.slippage_usd);
      }
    });
    
    return hasSlippageData ? { pct: totalSlippagePct, usd: totalSlippageUsd } : { pct: null, usd: null };
  };

  // Filter trades
  const getFilteredTrades = () => {
    let filtered = [...trades];
    
    // Time filter
    const now = new Date();
    const cutoff = new Date(now.getTime() - hoursFilter * 60 * 60 * 1000);
    filtered = filtered.filter(t => {
      const tradeTime = new Date(t.started_at);
      return tradeTime >= cutoff;
    });
    
    // Result filter
    if (resultFilter === 'win') {
      filtered = filtered.filter(t => t.status === 'COMPLETED' && t.profit_loss >= 0);
    } else if (resultFilter === 'loss') {
      filtered = filtered.filter(t => t.status === 'COMPLETED' && t.profit_loss < 0);
    } else if (resultFilter === 'failed') {
      filtered = filtered.filter(t => t.status === 'FAILED');
    } else if (resultFilter === 'partial') {
      filtered = filtered.filter(t => t.status === 'PARTIAL');
    }
    
    // Path filter
    if (pathFilter) {
      filtered = filtered.filter(t => t.path?.toUpperCase().startsWith(pathFilter));
    }
    
    return filtered;
  };

  // Get paginated trades
  const getPaginatedTrades = () => {
    const filtered = getFilteredTrades();
    const startIndex = (currentPage - 1) * pageSize;
    return filtered.slice(startIndex, startIndex + pageSize);
  };

  // Export to CSV
  const handleExportCSV = () => {
    const filteredTrades = getFilteredTrades();
    if (filteredTrades.length === 0) {
      showToast('No trades to export', 'error');
      return;
    }
    
    const headers = ['Time', 'Path', 'Amount', 'Taker Fee', 'Slippage', 'Latency (ms)', 'Profit/Loss', 'Status'];
    const rows = filteredTrades.map(t => {
      const totalFee = calculateTotalFee(t);
      const slippage = calculateTotalSlippage(t);
      const time = formatTimestamp(t.started_at);
      const path = `"${t.path || ''}"`;
      const amount = t.amount_in?.toFixed(2) || '--';
      const takerFee = totalFee ? `"-$${totalFee.toFixed(4)}"` : '--';
      const slippageStr = slippage.usd !== null ? `"-$${slippage.usd.toFixed(4)}"` : '--';
      const latency = t.total_execution_ms?.toFixed(0) || '--';
      const profitLoss = t.profit_loss !== null ? `"${t.profit_loss >= 0 ? '+' : ''}$${t.profit_loss?.toFixed(4)}"` : '--';
      const status = t.status || '--';
      
      return [time, path, amount, takerFee, slippageStr, latency, profitLoss, status].join(',');
    });
    
    const csvContent = [headers.join(','), ...rows].join('\n');
    const blob = new Blob([csvContent], { type: 'text/csv' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `live_trades_${new Date().toISOString().split('T')[0]}.csv`;
    a.click();
    URL.revokeObjectURL(url);
    showToast('Trades exported to CSV', 'success');
  };

  // Format timestamp
  const formatTimestamp = (timestamp) => {
    if (!timestamp) return '--';
    try {
      let ts = timestamp.endsWith('Z') || timestamp.includes('+') ? timestamp : timestamp + 'Z';
      return new Date(ts).toLocaleString('en-US', {
        month: 'short',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
        hour12: true
      });
    } catch { return '--'; }
  };

  const hasPendingConfig = Object.keys(pendingConfig).length > 0;
  const currentTradeAmount = getConfigValue('trade_amount', 10);
  const currentThreshold = getConfigValue('min_profit_threshold', 0.003);

  const formatUSD = (value) => {
    if (value === null || value === undefined) return '--';
    const num = parseFloat(value);
    if (isNaN(num)) return '--';
    return num >= 0 ? `+$${num.toFixed(2)}` : `-$${Math.abs(num).toFixed(2)}`;
  };

  const getThresholdWarning = () => {
    const threshold = currentThreshold * 100;
    if (threshold < 0.1) {
      return { type: 'danger', message: '🔴 Very risky: Trading fees are typically 0.16-0.26%' };
    } else if (threshold < 0.2) {
      return { type: 'warning', message: '⚠️ Warning: Fees may exceed profits at this threshold' };
    }
    return null;
  };

  const thresholdWarning = getThresholdWarning();
  const filteredTrades = getFilteredTrades();
  const paginatedTrades = getPaginatedTrades();
  const totalPages = Math.ceil(filteredTrades.length / pageSize);

  // Reset to page 1 when filters change
  useEffect(() => {
    setCurrentPage(1);
  }, [hoursFilter, resultFilter, pathFilter]);

  if (loading) return <div className="panel live-trading-panel loading"><p>Loading live trading data...</p></div>;

  const isEnabled = status?.enabled;
  const circuitBroken = status?.state?.is_broken;
  const dailyLoss = status?.state?.daily_loss || 0;
  const totalLoss = status?.state?.total_loss || 0;
  const totalProfit = status?.state?.total_profit || 0;
  const totalTradeAmount = status?.state?.total_trade_amount || 0;
  const maxDailyLoss = config?.max_daily_loss || 30;
  const maxTotalLoss = config?.max_total_loss || 30;
  const isConnected = positions?.connected ?? false;
  const krakenBalance = positions?.total_usd || 0;
  const krakenPositions = positions?.positions || [];

  const totalTrades = status?.state?.total_trades || 0;
  const totalWins = status?.state?.total_wins || 0;
  const totalLosses = totalTrades - totalWins;
  
  // Partial trade stats
  const partialCount = status?.state?.partial_trades || 0;
  const partialEstimatedLoss = status?.state?.partial_estimated_loss || 0;
  const partialEstimatedProfit = status?.state?.partial_estimated_profit || 0;
  const partialTradeAmount = status?.state?.partial_trade_amount || 0;
  const partialEstimatedNet = partialEstimatedProfit - partialEstimatedLoss;

  return (
    <div className="panel live-trading-panel">
      {toast && <div className={`toast-notification ${toast.type}`}>{toast.message}</div>}
      {error && <div className="error-message">⚠️ {error}<button onClick={() => setError(null)}>×</button></div>}

      {circuitBroken && (
        <div className="circuit-breaker-banner">
          <div className="cb-content">
            <span className="cb-icon">🛑</span>
            <div className="cb-text"><strong>CIRCUIT BREAKER ACTIVATED</strong><p>{status?.state?.broken_reason}</p></div>
            <button className="cb-reset-btn" onClick={handleResetCircuitBreaker}>🔓 Reset Circuit Breaker</button>
          </div>
        </div>
      )}

      {/* Holdings + Connection Status */}
      <div className="top-section">
        <div className={`connection-indicator ${isConnected ? 'connected' : 'disconnected'}`}>
          <span className="status-dot"></span>
          <span className="status-text">{isConnected ? 'Connected to Kraken' : 'Not Connected'}</span>
        </div>

        {isConnected && (
          <>
            <div className="info-card highlight">
              <span className="label">Total Portfolio Value</span>
              <span className="value">${krakenBalance.toFixed(2)}</span>
            </div>
          </>
        )}
      </div>

      {/* Portfolio Holdings */}
      {isConnected && krakenPositions.length > 0 && (
        <div className="holdings-section">
          <h3>💰 Portfolio Holdings</h3>
          <div className="holdings-grid">
            {krakenPositions.map((pos) => (
              <div key={pos.currency} className="holding-card">
                <div className="holding-header">
                  <span className="holding-currency">{pos.currency}</span>
                  <span className="holding-usd">${pos.usd_value.toFixed(2)}</span>
                </div>
                <div className="holding-balance">
                  {pos.balance < 0.0001
                    ? pos.balance.toExponential(4)
                    : pos.balance < 1
                      ? pos.balance.toFixed(8)
                      : pos.balance < 1000
                        ? pos.balance.toFixed(4)
                        : pos.balance.toLocaleString(undefined, {maximumFractionDigits: 2})
                  }
                </div>
                {pos.usd_value > 0 && pos.currency !== 'USD' && (
                  <div className="holding-price">
                    @ ${(pos.usd_value / pos.balance).toFixed(2)}
                  </div>
                )}
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Performance */}
      <div className="performance-section">
        <h3>📈 Performance</h3>
        <div className="performance-grid">
          <div className="perf-card"><span className="label">Total Trades</span><span className="value">{totalTrades}</span></div>
          <div className="perf-card"><span className="label">Wins</span><span className="value positive">{totalWins}</span></div>
          <div className="perf-card"><span className="label">Losses</span><span className="value negative">{totalLosses}</span></div>
          <div className="perf-card"><span className="label">Total Profit</span><span className="value positive">${totalProfit.toFixed(2)}</span></div>
          <div className="perf-card"><span className="label">Total Lost</span><span className="value negative">${totalLoss.toFixed(2)}</span></div>
          <div className="perf-card"><span className="label">Total Trade Amount</span><span className="value">${totalTradeAmount.toFixed(2)}</span></div>
        </div>
      </div>

      {/* Partial Trades Section */}
      {(partialCount > 0 || partialTrades.length > 0) && (
        <div className="partial-section">
          <h3>⚠️ Partial Trades (Unresolved)</h3>
          <div className="partial-summary">
            <div className="partial-stat">
              <span className="label">Unresolved</span>
              <span className="value warning">{partialCount}</span>
            </div>
            <div className="partial-stat">
              <span className="label">Stuck Amount</span>
              <span className="value">${partialTradeAmount.toFixed(2)}</span>
            </div>
            <div className="partial-stat">
              <span className="label">Est. P/L</span>
              <span className={`value ${partialEstimatedNet >= 0 ? 'positive' : 'negative'}`}>
                {partialEstimatedNet >= 0 ? '+' : ''}${partialEstimatedNet.toFixed(2)}
              </span>
            </div>
          </div>
          
          {partialTrades.length > 0 && (
            <div className="partial-trades-list">
              <table className="partial-table">
                <thead>
                  <tr>
                    <th>Time</th>
                    <th>Path</th>
                    <th>Original</th>
                    <th>Holding</th>
                    <th>Est. Value</th>
                    <th>Est. P/L</th>
                    <th>Action</th>
                  </tr>
                </thead>
                <tbody>
                  {partialTrades.map((trade) => {
                    const estPL = (trade.held_value_usd || 0) - (trade.amount_in || 0);
                    return (
                      <tr key={trade.trade_id}>
                        <td className="time-cell">{formatTimestamp(trade.started_at)}</td>
                        <td><code>{trade.path}</code></td>
                        <td>${trade.amount_in?.toFixed(2)}</td>
                        <td className="holding-cell">
                          {trade.held_amount?.toFixed(6)} {trade.held_currency}
                        </td>
                        <td>${trade.held_value_usd?.toFixed(2) || '--'}</td>
                        <td className={estPL >= 0 ? 'positive' : 'negative'}>
                          {estPL >= 0 ? '+' : ''}${estPL.toFixed(2)}
                        </td>
                        <td>
                          <button 
                            className="resolve-btn"
                            onClick={() => handlePreviewResolve(trade.trade_id)}
                            disabled={resolvingTradeId === trade.trade_id}
                          >
                            {resolvingTradeId === trade.trade_id ? '...' : '🔄 Resolve'}
                          </button>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}
          
          <p className="partial-hint">
            💡 Partial trades occur when a multi-leg trade fails mid-execution. 
            Click "Resolve" to sell the held crypto back to USD.
          </p>
        </div>
      )}

      {/* Controls */}
      <div className="live-controls-section">
        <div className="controls-header">
          <h2>🔴 Live Trading Controls</h2>
          <div className="controls-buttons">
            {!isEnabled ? (
              <>
                <button
                  className="enable-btn"
                  onClick={() => setShowEnableModal(true)}
                  disabled={circuitBroken || !rustEngineStatus?.connected}
                  title={!rustEngineStatus?.connected ? 'Executor offline - cannot place orders' : circuitBroken ? 'Circuit breaker triggered' : 'Start live trading'}
                >
                  🟢 START Live Trading
                </button>
                {!rustEngineStatus?.connected && (
                  <span className="executor-warning">⚠️ Executor offline</span>
                )}
              </>
            ) : (
              <>
                <button className="disable-btn" onClick={handleDisable}>⏹️ Stop Trading</button>
                <button className="emergency-btn" onClick={handleEmergencyStop}>🛑 EMERGENCY STOP</button>
              </>
            )}
          </div>
        </div>
        <div className="status-grid">
          <div className={`status-card ${circuitBroken ? 'danger' : 'ok'}`}><span className="status-label">Circuit Breaker</span><span className="status-value">{circuitBroken ? '🛑 TRIGGERED' : '✅ OK'}</span></div>
          <div className="status-card"><span className="status-label">Execution</span><span className="status-value">⚡ Rust WebSocket</span></div>
          <div className="status-card"><span className="status-label">Trade Amount</span><span className="status-value">${config?.trade_amount || 10}</span></div>
        </div>
      </div>

      {/* P&L */}
      <div className="pnl-section">
        <h3>📊 Profit & Loss</h3>
        <div className="pnl-grid">
          <div className="pnl-card">
            <span className="pnl-label">Daily P&L</span>
            <span className={`pnl-value ${(status?.state?.daily_profit - dailyLoss) >= 0 ? 'positive' : 'negative'}`}>{formatUSD((status?.state?.daily_profit || 0) - dailyLoss)}</span>
            <span className="pnl-detail">{status?.state?.daily_trades || 0} trades, {status?.state?.daily_wins || 0} wins</span>
          </div>
          <div className="pnl-card">
            <span className="pnl-label">Total P&L</span>
            <span className={`pnl-value ${(totalProfit - totalLoss) >= 0 ? 'positive' : 'negative'}`}>{formatUSD(totalProfit - totalLoss)}</span>
            <span className="pnl-detail">{totalTrades} trades, {totalWins} wins</span>
          </div>
          <div className="pnl-card">
            <span className="pnl-label">Daily Loss Limit</span>
            <div className="limit-bar"><div className="limit-fill" style={{ width: `${Math.min((dailyLoss / maxDailyLoss) * 100, 100)}%` }} /></div>
            <span className="pnl-detail-bright">${dailyLoss.toFixed(2)} / ${maxDailyLoss.toFixed(2)}</span>
          </div>
          <div className="pnl-card">
            <span className="pnl-label">Total Loss Limit</span>
            <div className="limit-bar"><div className="limit-fill" style={{ width: `${Math.min((totalLoss / maxTotalLoss) * 100, 100)}%` }} /></div>
            <span className="pnl-detail-bright">${totalLoss.toFixed(2)} / ${maxTotalLoss.toFixed(2)}</span>
          </div>
        </div>
        <div className="pnl-actions"><button className="reset-btn" onClick={handleResetDaily}>🔄 Reset Daily</button></div>
      </div>

      {/* Rust Execution Engine Section */}
      <div className="rust-engine-section">
        <div className="rust-engine-header">
          <h3>⚡ Rust Execution Engine</h3>
          <span className={`engine-status ${rustEngineStatus?.connected ? 'connected' : 'offline'}`}>
            {rustEngineStatus?.connected ? '● Connected' : '○ Offline'}
          </span>
        </div>

        {rustEngineStatus?.connected ? (
          <div className="rust-engine-grid">
            <div className="engine-stat">
              <span className="label">Trades Executed</span>
              <span className="value">{rustEngineStatus?.stats?.trades_executed || 0}</span>
            </div>
            <div className="engine-stat">
              <span className="label">Success Rate</span>
              <span className="value positive">
                {rustEngineStatus?.stats?.success_rate
                  ? `${(rustEngineStatus.stats.success_rate * 100).toFixed(1)}%`
                  : '--'}
              </span>
            </div>
            <div className="engine-stat">
              <span className="label">Avg Latency</span>
              <span className="value">
                {rustEngineStatus?.stats?.avg_execution_ms
                  ? `${rustEngineStatus.stats.avg_execution_ms.toFixed(0)}ms`
                  : '--'}
              </span>
            </div>
            <div className="engine-stat">
              <span className="label">Total Profit</span>
              <span className={`value ${(rustEngineStatus?.stats?.total_profit || 0) >= 0 ? 'positive' : 'negative'}`}>
                ${(rustEngineStatus?.stats?.total_profit || 0).toFixed(2)}
              </span>
            </div>
          </div>
        ) : (
          <div className="engine-offline-message">
            <p>Rust execution engine is offline.</p>
            <p className="engine-setup-note">
              To enable: Add <code>KRAKEN_API_KEY</code> and <code>KRAKEN_API_SECRET</code> to your .env file.
            </p>
            <p className="engine-benefits">The engine provides:</p>
            <ul>
              <li>10x faster order execution via WebSocket</li>
              <li>Parallel leg execution for pre-positioned funds</li>
              <li>Automatic maker/taker order optimization</li>
            </ul>
          </div>
        )}

        {/* Fee Optimization Section */}
        <div className="fee-optimization-section">
          <div className="fee-header" onClick={() => setShowFeeSettings(!showFeeSettings)}>
            <span className="fee-toggle">{showFeeSettings ? '▼' : '▶'}</span>
            <h4>💰 Fee Optimization</h4>
            {feeStats && feeStats.maker_orders_attempted > 0 && (
              <span className="fee-savings">
                Saved: ${feeStats.total_fee_savings?.toFixed(2) || '0.00'}
              </span>
            )}
          </div>

          {showFeeSettings && (
            <div className="fee-settings-content">
              {feeStats && feeStats.maker_orders_attempted > 0 && (
                <div className="fee-stats-grid">
                  <div className="fee-stat">
                    <span className="label">Maker Orders</span>
                    <span className="value">{feeStats.maker_orders_filled || 0} / {feeStats.maker_orders_attempted || 0}</span>
                  </div>
                  <div className="fee-stat">
                    <span className="label">Fill Rate</span>
                    <span className="value">{feeStats.maker_fill_rate?.toFixed(1) || 0}%</span>
                  </div>
                  <div className="fee-stat">
                    <span className="label">Total Saved</span>
                    <span className="value positive">${feeStats.total_fee_savings?.toFixed(4) || '0.00'}</span>
                  </div>
                </div>
              )}

              {/* Kraken Fee Rates (read-only - set by Kraken based on 30-day volume) */}
              <div className="fee-rates-display">
                <div className="fee-rate-item">
                  <span className="fee-rate-label">Maker Fee</span>
                  <span className="fee-rate-value">{(getFeeConfigValue('maker_fee', 0.0016) * 100).toFixed(2)}%</span>
                </div>
                <div className="fee-rate-item">
                  <span className="fee-rate-label">Taker Fee</span>
                  <span className="fee-rate-value">{(getFeeConfigValue('taker_fee', 0.0026) * 100).toFixed(2)}%</span>
                </div>
                <div className="fee-rate-item highlight">
                  <span className="fee-rate-label">Potential Savings</span>
                  <span className="fee-rate-value positive">
                    {((getFeeConfigValue('taker_fee', 0.0026) - getFeeConfigValue('maker_fee', 0.0016)) * 100).toFixed(2)}%
                  </span>
                </div>
              </div>
              <p className="fee-note">Fee rates are set by Kraken based on your 30-day trading volume.</p>
            </div>
          )}
        </div>
      </div>

      {/* Configuration */}
      <div className="config-section">
        <div className="config-header">
          <h3>⚙️ Configuration</h3>
          {hasPendingConfig && <button className="apply-btn" onClick={handleApplyConfig}>✓ Apply Changes</button>}
        </div>
        <div className="config-grid">
          <div className="config-item">
            <label>Trade Amount</label>
            <div className="amount-buttons">
              {PRESET_AMOUNTS.map(amt => (
                <button key={amt} className={currentTradeAmount === amt ? 'active' : ''} onClick={() => handleConfigChange('trade_amount', amt)}>${amt}</button>
              ))}
              {!PRESET_AMOUNTS.includes(currentTradeAmount) && (
                <button className="active custom-active">${currentTradeAmount}</button>
              )}
            </div>
            <div className="custom-amount-row">
              <input type="number" placeholder="Custom amount" value={customAmount} onChange={(e) => setCustomAmount(e.target.value)} onKeyPress={(e) => e.key === 'Enter' && handleCustomAmountSubmit()} />
              <button onClick={handleCustomAmountSubmit} className="custom-btn">Set</button>
            </div>
          </div>
          <div className="config-item">
            <label>Min Profit Threshold</label>
            <div className="slider-container">
              <input type="range" min="0.01" max="1" step="0.01" value={currentThreshold * 100} onChange={(e) => handleConfigChange('min_profit_threshold', parseFloat(e.target.value) / 100)} />
              <span className="slider-value">{(currentThreshold * 100).toFixed(2)}%</span>
            </div>
            {thresholdWarning && <div className={`threshold-warning ${thresholdWarning.type}`}>{thresholdWarning.message}</div>}
          </div>
          <div className="config-item"><label>Max Daily Loss ($)</label><input type="number" min="10" max="200" value={getConfigValue('max_daily_loss', 30)} onChange={(e) => handleConfigChange('max_daily_loss', parseFloat(e.target.value))} /></div>
          <div className="config-item"><label>Max Total Loss ($)</label><input type="number" min="10" max="200" value={getConfigValue('max_total_loss', 30)} onChange={(e) => handleConfigChange('max_total_loss', parseFloat(e.target.value))} /></div>
          {/* Execution mode removed - Rust WebSocket always uses parallel execution */}
          <div className="config-item">
            <label>Base Currency</label>
            <select value={getConfigValue('base_currency', 'USD')} onChange={(e) => handleBaseCurrencyChange(e.target.value)}>
              <option value="ALL">ALL</option>
              <option value="USD">USD</option>
              <option value="EUR">EUR</option>
              <option value="USDT">USDT</option>
              <option value="BTC">BTC</option>
              <option value="ETH">ETH</option>
              <option value="CUSTOM">CUSTOM</option>
            </select>
          </div>
          {/* Max parallel trades removed - Rust handles execution internally */}
          {showCustomCurrencies && (
            <div className="config-item full-width">
              <label>Select Currencies</label>
              <div className="currency-checkboxes">
                {AVAILABLE_CURRENCIES.map(curr => (
                  <label key={curr} className={`checkbox-label ${customCurrencies.includes(curr) ? 'checked' : ''}`}>
                    <input type="checkbox" checked={customCurrencies.includes(curr)} onChange={() => handleCustomCurrencyToggle(curr)} />
                    <span className="checkmark"></span>
                    {curr}
                  </label>
                ))}
              </div>
              {customCurrencies.length > 0 && <div className="selected-currencies">Selected: {customCurrencies.join(', ')}</div>}
            </div>
          )}
          
          {/* Execution settings (retries/timeout) removed - Rust WebSocket handles this internally */}
          
          {/* Expandable Risks Section */}
          <div className="config-item full-width">
            <div className="risks-collapsible" onClick={() => setShowRisks(!showRisks)}>
              <span className="risks-toggle">{showRisks ? '▼' : '▶'}</span>
              <span className="risks-title">⚠️ Important Risks & Information</span>
            </div>
            
            {showRisks && (
              <div className="risks-content">
                <div className="risk-section">
                  <h5>1. Partial Trade Risk</h5>
                  <p>If a leg fails mid-trade (e.g., network error, timeout, insufficient liquidity), you may be left holding a different currency than you started with.</p>
                  <ul>
                    <li><strong>Leg 1 fails:</strong> Safe - you keep your original USD</li>
                    <li><strong>Leg 2 fails:</strong> ⚠️ You hold intermediate currency (e.g., BTC)</li>
                    <li><strong>Leg 3 fails:</strong> ⚠️ You hold intermediate currency (e.g., ETH)</li>
                  </ul>
                  <p className="action-note">Monitor <strong>PARTIAL</strong> trades and manually sell any stuck positions back to USD.</p>
                </div>
                
                <div className="risk-section">
                  <h5>2. Slippage</h5>
                  <p>Actual execution price may differ from expected price, reducing or eliminating profits. Market orders fill at best available price, which may move between order placement and execution.</p>
                </div>
                
                <div className="risk-section">
                  <h5>3. Trading Fees</h5>
                  <p>Each trade incurs ~0.26% taker fee per leg (~0.78% total for 3-leg arbitrage). Fees are deducted from your balance and reduce overall profitability.</p>
                </div>
                
                <div className="risk-section">
                  <h5>4. Retries & Timeout</h5>
                  <p>If an order fails, the system will retry up to the configured number of times. If an order doesn't fill within the timeout period, it will be cancelled and counted as a failed attempt.</p>
                </div>
                
                <div className="risk-section">
                  <h5>5. Circuit Breaker</h5>
                  <p>Trading automatically stops when daily or total loss limits are reached. Daily losses reset at midnight UTC. You can manually reset the circuit breaker after reviewing your positions.</p>
                </div>
                
                <div className="risk-section">
                  <h5>6. Parallel Trading Risks</h5>
                  <p>When running multiple trades in parallel mode:</p>
                  <ul>
                    <li><strong>Slippage cascade:</strong> Each trade makes the price worse for the next trade</li>
                    <li><strong>Balance required:</strong> 5 trades × $10 = $50 minimum balance needed</li>
                    <li><strong>Tracking complexity:</strong> Multiple failures harder to diagnose</li>
                    <li><strong>Circuit breaker delay:</strong> Multiple losing trades may execute before limits are checked</li>
                  </ul>
                  <p className="action-note">Recommendation: Start with 1 parallel trade until you understand the system behavior.</p>
                </div>
              </div>
            )}
          </div>
        </div>
      </div>

      {/* Scanner Status */}
      <div className="scanner-section">
        <h3>📡 Scanner Status</h3>
        <div className="scanner-grid">
          <div className="scanner-card">
            <span className="scanner-label">Status</span>
            <span className={`scanner-value ${scannerStatus?.is_running ? 'running' : 'stopped'}`}>
              {scannerStatus?.is_running ? '🟢 Running' : '⚪ Stopped'}
            </span>
          </div>
          <div className="scanner-card">
            <span className="scanner-label">Pairs Scanned</span>
            <span className="scanner-value">{scannerStatus?.pairs_scanned || 0}</span>
          </div>
          <div className="scanner-card">
            <span className="scanner-label">Paths Found</span>
            <span className="scanner-value">{scannerStatus?.paths_found || 0}</span>
          </div>
          <div className="scanner-card">
            <span className="scanner-label">Opportunities</span>
            <span className="scanner-value">{scannerStatus?.opportunities_found || 0}</span>
          </div>
          <div className="scanner-card">
            <span className="scanner-label">Above Threshold</span>
            <span className="scanner-value highlight">{scannerStatus?.profitable_count || 0}</span>
          </div>
          <div className="scanner-card">
            <span className="scanner-label">Last Scan</span>
            <span className="scanner-value">
              {scannerStatus?.seconds_ago != null 
                ? `${Math.round(scannerStatus.seconds_ago)}s ago` 
                : '--'}
            </span>
          </div>
        </div>
      </div>

      {/* Execution Log - Shows what happened to opportunities */}
      <div className="opportunities-section">
        <h3>📋 Execution Log</h3>
        <p className="section-hint">Shows execution status of detected opportunities (EXECUTED/SKIPPED/MISSED)</p>
        {opportunities.length === 0 ? (
          <div className="empty-state">
            <p>No execution history yet</p>
            <p className="hint">When opportunities are processed, their status will appear here</p>
          </div>
        ) : (
          <div className="opportunities-table-wrapper">
            <table className="opportunities-table">
              <thead>
                <tr>
                  <th>Time</th>
                  <th>Path</th>
                  <th>Expected Profit</th>
                  <th>Status</th>
                  <th>Reason / Trade</th>
                </tr>
              </thead>
              <tbody>
                {opportunities.slice(0, 20).map((opp, idx) => (
                  <tr key={opp.id || idx} className={`opp-row ${opp.status?.toLowerCase()}`}>
                    <td className="opp-time">{formatTimestamp(opp.found_at)}</td>
                    <td className="opp-path">{opp.path}</td>
                    <td className={`opp-profit ${opp.expected_profit_pct >= 0 ? 'positive' : 'negative'}`}>
                      +{opp.expected_profit_pct?.toFixed(3)}%
                      {opp.expected_profit_usd && (
                        <span className="usd">(${opp.expected_profit_usd?.toFixed(4)})</span>
                      )}
                    </td>
                    <td>
                      <span className={`opp-badge ${opp.status?.toLowerCase()}`}>
                        {opp.status === 'EXECUTED' && '✅ '}
                        {opp.status === 'SKIPPED' && '⏭️ '}
                        {opp.status === 'MISSED' && '❌ '}
                        {opp.status === 'PENDING' && '⏳ '}
                        {opp.status === 'EXPIRED' && '⌛ '}
                        {opp.status}
                      </span>
                    </td>
                    <td className="opp-reason">
                      {opp.trade_id ? (
                        <span className="trade-link">Trade: {opp.trade_id.slice(-8)}</span>
                      ) : (
                        opp.status_reason || '--'
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>

      {/* Live Trades Table */}
      <div className="trades-section">
        <div className="trades-header">
          <h3>📜 Recent Live Trades</h3>
          <button className="export-btn" onClick={handleExportCSV}>📥 Export CSV</button>
        </div>
        
        {/* Filters */}
        <div className="filters-bar">
          <div className="filter-group">
            <label>Time Range:</label>
            <select value={hoursFilter} onChange={(e) => setHoursFilter(parseInt(e.target.value))}>
              <option value={1}>Last 1 hour</option>
              <option value={6}>Last 6 hours</option>
              <option value={24}>Last 24 hours</option>
              <option value={72}>Last 3 days</option>
              <option value={168}>Last 7 days</option>
              <option value={720}>Last 30 days</option>
            </select>
          </div>
          <div className="filter-group">
            <label>Result:</label>
            <select value={resultFilter} onChange={(e) => setResultFilter(e.target.value)}>
              <option value="">All</option>
              <option value="win">Wins Only</option>
              <option value="loss">Losses Only</option>
              <option value="failed">Failed</option>
              <option value="partial">Partial</option>
            </select>
          </div>
          <div className="filter-group">
            <label>Starts with:</label>
            <input type="text" placeholder="USD, EUR..." value={pathFilter} onChange={(e) => setPathFilter(e.target.value.toUpperCase())} className="path-input" />
          </div>
          <div className="filter-group results-count">
            <span>{filteredTrades.length} trades</span>
          </div>
        </div>

        {filteredTrades.length === 0 ? (
          <div className="empty-state"><p>No live trades found</p><p className="hint">Enable live trading to start executing trades</p></div>
        ) : (
          <>
            <div className="trades-table-container">
              <table className="trades-table">
                <thead>
                  <tr>
                    <th></th>
                    <th>Time</th>
                    <th>Path</th>
                    <th>Amount</th>
                    <th>Taker Fee</th>
                    <th>Slippage</th>
                    <th>Latency</th>
                    <th>Profit/Loss</th>
                    <th>Status</th>
                  </tr>
                </thead>
                <tbody>
                  {paginatedTrades.map((trade, idx) => {
                    const totalFee = calculateTotalFee(trade);
                    const slippage = calculateTotalSlippage(trade);
                    return (
                      <React.Fragment key={trade.trade_id || idx}>
                        <tr 
                          className={`${
                            trade.status === 'COMPLETED' && trade.profit_loss >= 0 ? 'win' : 
                            trade.status === 'COMPLETED' && trade.profit_loss < 0 ? 'loss' : 
                            trade.status === 'PARTIAL' ? 'partial' : 
                            trade.status === 'FAILED' ? 'failed' : ''
                          } expandable`}
                          onClick={() => toggleExpandRow(trade.trade_id)}
                        >
                          <td className="expand-cell">
                            {expandedTradeId === trade.trade_id ? '▼' : '▶'}
                          </td>
                          <td className="time-cell">{formatTimestamp(trade.started_at)}</td>
                          <td><code>{trade.path}</code></td>
                          <td className="amount-cell">${trade.amount_in?.toFixed(2)}</td>
                          <td className="fee-cell">
                            {totalFee ? (
                              <>
                                <span className="pct">-{((totalFee / trade.amount_in) * 100)?.toFixed(2)}%</span>
                                <span className="usd">(-${totalFee.toFixed(4)})</span>
                              </>
                            ) : '--'}
                          </td>
                          <td className="slippage-cell">
                            {slippage.pct !== null ? (
                              <>
                                <span className="pct">-{Math.abs(slippage.pct).toFixed(4)}%</span>
                                <span className="usd">(-${slippage.usd?.toFixed(4) || '0.0000'})</span>
                              </>
                            ) : '--'}
                          </td>
                          <td className="latency-cell">{trade.total_execution_ms ? `${trade.total_execution_ms.toFixed(0)}ms` : '--'}</td>
                          <td className={trade.profit_loss >= 0 ? 'positive' : 'negative'}>
                            {trade.profit_loss !== null ? (
                              <>
                                <span className="pct">{trade.profit_loss_pct >= 0 ? '+' : ''}{trade.profit_loss_pct?.toFixed(4)}%</span>
                                <span className="usd">({trade.profit_loss >= 0 ? '+' : ''}${trade.profit_loss?.toFixed(4)})</span>
                              </>
                            ) : '--'}
                          </td>
                          <td>
                            <span className={`badge ${trade.status?.toLowerCase()}`}>
                              {trade.status === 'COMPLETED' && trade.profit_loss >= 0 ? '✓ WIN' : 
                               trade.status === 'COMPLETED' && trade.profit_loss < 0 ? '✗ LOSS' : 
                               trade.status}
                            </span>
                          </td>
                        </tr>
                        
                        {/* Expanded Leg Details */}
                        {expandedTradeId === trade.trade_id && trade.leg_fills && trade.leg_fills.length > 0 && (
                          <tr className="expanded-row">
                            <td colSpan="9">
                              <div className="leg-details">
                                <h4>Leg Details (Live Kraken Data)</h4>
                                <table className="leg-table">
                                  <thead>
                                    <tr>
                                      <th>Leg</th>
                                      <th>Pair</th>
                                      <th>Side</th>
                                      <th>Expected Price</th>
                                      <th>Actual Price</th>
                                      <th>Slippage</th>
                                      <th>Fee</th>
                                      <th>Time</th>
                                    </tr>
                                  </thead>
                                  <tbody>
                                    {trade.leg_fills.map((leg, legIdx) => (
                                      <tr key={legIdx}>
                                        <td className="leg-num">{leg.leg || legIdx + 1}</td>
                                        <td className="leg-pair">{leg.pair}</td>
                                        <td className={`leg-side ${leg.side?.toLowerCase()}`}>{leg.side?.toUpperCase()}</td>
                                        <td>{leg.expected_price?.toFixed(8) || '--'}</td>
                                        <td>{leg.executed_price?.toFixed(8) || '--'}</td>
                                        <td className="negative">
                                          {leg.slippage_pct !== undefined && leg.slippage_pct !== null 
                                            ? `-${Math.abs(leg.slippage_pct).toFixed(4)}%` 
                                            : '--'}
                                        </td>
                                        <td className="negative">{leg.fee ? `-${leg.fee} ${leg.fee_currency || ''}` : '--'}</td>
                                        <td>{leg.execution_ms ? `${leg.execution_ms.toFixed(0)}ms` : '--'}</td>
                                      </tr>
                                    ))}
                                  </tbody>
                                </table>
                              </div>
                            </td>
                          </tr>
                        )}
                        
                        {/* Show placeholder if no leg details */}
                        {expandedTradeId === trade.trade_id && (!trade.leg_fills || trade.leg_fills.length === 0) && (
                          <tr className="expanded-row">
                            <td colSpan="9">
                              <div className="leg-details">
                                <p className="no-details">No leg details available for this trade</p>
                              </div>
                            </td>
                          </tr>
                        )}
                      </React.Fragment>
                    );
                  })}
                </tbody>
              </table>
            </div>
            
            {/* Pagination */}
            {totalPages > 1 && (
              <div className="pagination">
                <button 
                  className="page-btn" 
                  onClick={() => setCurrentPage(1)} 
                  disabled={currentPage === 1}
                >
                  ««
                </button>
                <button 
                  className="page-btn" 
                  onClick={() => setCurrentPage(p => Math.max(1, p - 1))} 
                  disabled={currentPage === 1}
                >
                  «
                </button>
                <span className="page-info">
                  Page {currentPage} of {totalPages}
                </span>
                <button 
                  className="page-btn" 
                  onClick={() => setCurrentPage(p => Math.min(totalPages, p + 1))} 
                  disabled={currentPage === totalPages}
                >
                  »
                </button>
                <button 
                  className="page-btn" 
                  onClick={() => setCurrentPage(totalPages)} 
                  disabled={currentPage === totalPages}
                >
                  »»
                </button>
              </div>
            )}
          </>
        )}
      </div>

      {/* Enable Modal */}
      {showEnableModal && (
        <div className="modal-overlay">
          <div className="enable-modal">
            <h2>⚠️ START LIVE TRADING</h2>

            {/* Executor Status Check */}
            <div className={`executor-status-check ${rustEngineStatus?.connected ? 'ready' : 'offline'}`}>
              <span className="status-icon">{rustEngineStatus?.connected ? '✅' : '❌'}</span>
              <span className="status-text">
                {rustEngineStatus?.connected
                  ? 'Executor Ready - Connected to Kraken'
                  : 'Executor Offline - Cannot place orders'}
              </span>
            </div>

            <div className="config-summary">
              <h4>Current Settings:</h4>
              <div className="config-row"><span className="config-label">Trade Amount</span><span className="config-value">${config?.trade_amount || 10} per trade</span></div>
              <div className="config-row"><span className="config-label">Min Profit Threshold</span><span className="config-value">{((config?.min_profit_threshold || 0.003) * 100).toFixed(2)}%</span></div>
              <div className="config-row"><span className="config-label">Max Daily Loss</span><span className="config-value">${config?.max_daily_loss || 30}</span></div>
              <div className="config-row"><span className="config-label">Max Total Loss</span><span className="config-value highlight-red">${config?.max_total_loss || 30}</span></div>
              <div className="config-row"><span className="config-label">Execution</span><span className="config-value">⚡ Rust WebSocket (~50ms)</span></div>
              <div className="config-row"><span className="config-label">Base Currency</span><span className="config-value">{config?.base_currency || 'USD'}</span></div>
            </div>

            <div className="modal-buttons">
              <button className="cancel-btn" onClick={() => setShowEnableModal(false)}>Cancel</button>
              <button
                className="confirm-btn"
                onClick={handleEnable}
                disabled={!rustEngineStatus?.connected}
              >
                {rustEngineStatus?.connected ? '✓ Start Live Trading' : '⚠️ Executor Offline'}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Resolve Partial Trade Modal */}
      {showResolveModal && resolvePreview && (
        <div className="modal-overlay">
          <div className="resolve-modal">
            <h2>🔄 Resolve Partial Trade</h2>
            <p className="modal-subtitle">Sell held crypto back to USD</p>
            
            <div className="resolve-summary">
              <div className="resolve-row">
                <span className="resolve-label">Trade ID</span>
                <span className="resolve-value">{resolvePreview.trade_id?.slice(-12)}</span>
              </div>
              <div className="resolve-row">
                <span className="resolve-label">Holding</span>
                <span className="resolve-value highlight">
                  {resolvePreview.held_amount?.toFixed(6)} {resolvePreview.held_currency}
                </span>
              </div>
              <div className="resolve-row">
                <span className="resolve-label">Original Amount</span>
                <span className="resolve-value">${resolvePreview.original_amount_usd?.toFixed(2)}</span>
              </div>
              <div className="resolve-row">
                <span className="resolve-label">Snapshot Value (at failure)</span>
                <span className="resolve-value">${resolvePreview.snapshot_value_usd?.toFixed(2) || '--'}</span>
              </div>
              <div className="resolve-row">
                <span className="resolve-label">Current Value</span>
                <span className="resolve-value highlight">${resolvePreview.current_value_usd?.toFixed(2) || '--'}</span>
              </div>
              <div className="resolve-row big">
                <span className="resolve-label">Estimated P/L</span>
                <span className={`resolve-value ${resolvePreview.estimated_pl >= 0 ? 'positive' : 'negative'}`}>
                  {resolvePreview.estimated_pl >= 0 ? '+' : ''}${resolvePreview.estimated_pl?.toFixed(2)} 
                  ({resolvePreview.estimated_pl_pct?.toFixed(2)}%)
                </span>
              </div>
            </div>
            
            <div className="resolve-warning">
              <p>⚠️ This will execute a market order to sell {resolvePreview.held_currency} → USD</p>
              <p>Actual P/L may differ due to slippage and fees.</p>
            </div>
            
            <div className="modal-buttons">
              <button className="cancel-btn" onClick={handleCancelResolve}>Cancel</button>
              <button className="resolve-confirm-btn" onClick={handleResolvePartial}>
                ✓ Sell to USD
              </button>
            </div>
          </div>
        </div>
      )}

      <style>{`
        .live-trading-panel { padding: 20px; background: linear-gradient(180deg, #0d0d1a 0%, #1a1a2e 100%); min-height: calc(100vh - 200px); }
        .top-section { display: flex; align-items: center; gap: 20px; padding: 20px; background: linear-gradient(135deg, #1a1a2e, #252545); border: 1px solid #3a3a5a; border-radius: 12px; margin-bottom: 20px; flex-wrap: wrap; }
        .connection-indicator { display: flex; align-items: center; gap: 10px; padding: 10px 15px; background: linear-gradient(135deg, #252542, #2a2a50); border: 1px solid #3a3a5a; border-radius: 8px; }
        .connection-indicator .status-dot { width: 10px; height: 10px; border-radius: 50%; animation: pulse-dot 2s infinite; }
        .connection-indicator.connected .status-dot { background: #00d4aa; box-shadow: 0 0 8px #00d4aa; }
        .connection-indicator.disconnected .status-dot { background: #ff6b6b; box-shadow: 0 0 8px #ff6b6b; }
        .connection-indicator .status-text { color: #fff; font-weight: 600; font-size: 0.9rem; }
        @keyframes pulse-dot { 0%, 100% { opacity: 1; } 50% { opacity: 0.5; } }
        .info-card { padding: 15px 25px; border-radius: 10px; background: #252542; border: 1px solid #3a3a5a; }
        .info-card.highlight { border: 2px solid #00d4aa; background: linear-gradient(135deg, #1a3a2a, #1a2a3a); }
        .info-card .label { display: block; color: #a0a0b0; font-size: 0.85rem; text-transform: uppercase; letter-spacing: 0.5px; margin-bottom: 5px; }
        .info-card .value { display: block; color: #00d4aa; font-size: 1.8rem; font-weight: 700; }
        .holdings-inline { display: flex; align-items: center; gap: 15px; flex: 1; }
        .holdings-label { color: #f0ad4e; font-weight: 600; font-size: 1rem; }
        .holdings-items { display: flex; gap: 12px; flex-wrap: wrap; }
        .holding-item { background: #1a1a2e; padding: 10px 15px; border-radius: 8px; text-align: center; min-width: 100px; }
        .holding-item .currency { display: block; color: #00d4aa; font-weight: 600; font-size: 0.9rem; margin-bottom: 3px; }
        .holding-item .amount { display: block; color: #fff; font-size: 0.85rem; }

        /* Holdings Section */
        .holdings-section { background: linear-gradient(135deg, #1a1a2e, #252545); border: 1px solid #3a3a5a; border-radius: 16px; padding: 25px; margin-bottom: 20px; }
        .holdings-section h3 { color: #f0ad4e; margin-bottom: 20px; font-size: 1.2rem; }
        .holdings-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 15px; }
        .holding-card { background: linear-gradient(135deg, #252542, #2a2a50); border: 1px solid #3a3a5a; border-radius: 12px; padding: 18px; transition: all 0.2s; }
        .holding-card:hover { border-color: #00d4aa; transform: translateY(-2px); }
        .holding-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 10px; }
        .holding-currency { color: #00d4aa; font-weight: 700; font-size: 1.1rem; }
        .holding-usd { color: #fff; font-weight: 600; font-size: 1rem; }
        .holding-balance { color: #a0a0b0; font-size: 0.9rem; font-family: monospace; }
        .holding-price { color: #888; font-size: 0.75rem; margin-top: 5px; }
        @media (max-width: 768px) { .holdings-grid { grid-template-columns: repeat(2, 1fr); } }
        @media (max-width: 480px) { .holdings-grid { grid-template-columns: 1fr; } }

        .performance-section { background: linear-gradient(135deg, #1a1a2e, #252545); border: 1px solid #3a3a5a; border-radius: 16px; padding: 25px; margin-bottom: 20px; }
        .performance-section h3 { color: #00d4aa; margin-bottom: 20px; font-size: 1.2rem; }
        .performance-grid { display: grid; grid-template-columns: repeat(6, 1fr); gap: 15px; }
        .perf-card { background: linear-gradient(135deg, #252542, #2a2a50); border: 1px solid #3a3a5a; border-radius: 10px; padding: 20px; text-align: center; }
        .perf-card .label { display: block; color: #a0a0b0; font-size: 0.85rem; text-transform: uppercase; letter-spacing: 0.5px; margin-bottom: 8px; }
        .perf-card .value { display: block; font-size: 1.4rem; font-weight: 700; color: #fff; }
        .perf-card .value.positive { color: #00d4aa; }
        .perf-card .value.negative { color: #ff6b6b; }
        
        /* Partial Trades Section */
        .partial-section { background: linear-gradient(135deg, #2a2a1e, #3a3525); border: 2px solid #f0ad4e; border-radius: 16px; padding: 25px; margin-bottom: 20px; }
        .partial-section h3 { color: #f0ad4e; margin-bottom: 20px; font-size: 1.2rem; }
        .partial-summary { display: grid; grid-template-columns: repeat(3, 1fr); gap: 15px; margin-bottom: 20px; }
        .partial-stat { background: linear-gradient(135deg, #252542, #2a2a50); border: 1px solid #3a3a5a; border-radius: 10px; padding: 15px; text-align: center; }
        .partial-stat .label { display: block; color: #a0a0b0; font-size: 0.8rem; text-transform: uppercase; margin-bottom: 5px; }
        .partial-stat .value { display: block; font-size: 1.3rem; font-weight: 700; color: #fff; }
        .partial-stat .value.warning { color: #f0ad4e; }
        .partial-stat .value.positive { color: #00d4aa; }
        .partial-stat .value.negative { color: #ff6b6b; }
        .partial-trades-list { margin-bottom: 15px; overflow-x: auto; }
        .partial-table { width: 100%; border-collapse: collapse; }
        .partial-table th { background: linear-gradient(135deg, #f0ad4e, #ec971f); color: #1a1a2e; padding: 12px; text-align: left; font-weight: 700; text-transform: uppercase; font-size: 0.75rem; }
        .partial-table td { padding: 12px; border-bottom: 1px solid #3a3a5a; color: #fff; font-size: 0.9rem; }
        .partial-table .time-cell { color: #a0a0b0; font-size: 0.85rem; }
        .partial-table .holding-cell { color: #f0ad4e; font-weight: 600; }
        .partial-table td.positive { color: #00d4aa; font-weight: 600; }
        .partial-table td.negative { color: #ff6b6b; font-weight: 600; }
        .resolve-btn { background: linear-gradient(135deg, #f0ad4e, #ec971f); color: #1a1a2e; border: none; padding: 8px 16px; border-radius: 6px; font-weight: 600; cursor: pointer; font-size: 0.85rem; }
        .resolve-btn:hover { opacity: 0.9; }
        .resolve-btn:disabled { opacity: 0.5; cursor: not-allowed; }
        .partial-hint { color: #a0a0b0; font-size: 0.85rem; margin: 0; padding: 10px; background: rgba(240, 173, 78, 0.1); border-radius: 8px; }

        /* Rust Execution Engine Section */
        .rust-engine-section { background: linear-gradient(135deg, #1a1a2e, #252545); border: 1px solid #3a3a5a; border-radius: 16px; padding: 25px; margin-bottom: 20px; }
        .rust-engine-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 20px; padding-bottom: 15px; border-bottom: 1px solid #3a3a5a; }
        .rust-engine-header h3 { color: #6c5ce7; margin: 0; font-size: 1.2rem; }
        .engine-status { font-size: 0.9rem; font-weight: 600; padding: 6px 12px; border-radius: 20px; }
        .engine-status.connected { background: rgba(0, 212, 170, 0.2); color: #00d4aa; }
        .engine-status.offline { background: rgba(100, 100, 100, 0.2); color: #888; }
        .rust-engine-grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 15px; margin-bottom: 20px; }
        .engine-stat { background: linear-gradient(135deg, #252542, #2a2a50); border: 1px solid #3a3a5a; border-radius: 10px; padding: 18px; text-align: center; }
        .engine-stat .label { display: block; color: #a0a0b0; font-size: 0.8rem; text-transform: uppercase; margin-bottom: 8px; }
        .engine-stat .value { display: block; font-size: 1.3rem; font-weight: 700; color: #fff; }
        .engine-stat .value.positive { color: #00d4aa; }
        .engine-stat .value.negative { color: #ff6b6b; }
        .engine-offline-message { background: rgba(100, 100, 100, 0.1); border: 1px solid #3a3a5a; border-radius: 10px; padding: 20px; margin-bottom: 20px; }
        .engine-offline-message p { color: #a0a0b0; margin: 0 0 10px 0; }
        .engine-offline-message .engine-setup-note { background: rgba(108, 92, 231, 0.15); border: 1px solid #6c5ce7; border-radius: 6px; padding: 10px 12px; margin: 12px 0; }
        .engine-offline-message .engine-setup-note code { background: rgba(0, 0, 0, 0.3); padding: 2px 6px; border-radius: 4px; font-size: 0.85rem; }
        .engine-offline-message .engine-benefits { color: #888; font-size: 0.9rem; margin-top: 15px; }
        .engine-offline-message ul { color: #888; margin: 0; padding-left: 20px; }
        .engine-offline-message li { margin: 5px 0; }

        /* Fee Optimization Section */
        .fee-optimization-section { background: linear-gradient(135deg, #252542, #2a2a50); border: 1px solid #3a3a5a; border-radius: 12px; overflow: hidden; }
        .fee-header { display: flex; align-items: center; gap: 12px; padding: 15px 20px; cursor: pointer; transition: background 0.2s; }
        .fee-header:hover { background: rgba(255, 255, 255, 0.03); }
        .fee-toggle { color: #f0ad4e; font-size: 0.8rem; }
        .fee-header h4 { color: #f0ad4e; margin: 0; flex: 1; font-size: 1rem; }
        .fee-savings { color: #00d4aa; font-weight: 600; font-size: 0.9rem; }
        .fee-settings-content { padding: 0 20px 20px 20px; }
        .fee-stats-grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 12px; margin-bottom: 20px; }
        .fee-stat { background: linear-gradient(135deg, #1a1a2e, #202035); border: 1px solid #3a3a5a; border-radius: 8px; padding: 12px; text-align: center; }
        .fee-stat .label { display: block; color: #888; font-size: 0.75rem; text-transform: uppercase; margin-bottom: 5px; }
        .fee-stat .value { display: block; color: #fff; font-size: 1rem; font-weight: 600; }
        .fee-stat .value.positive { color: #00d4aa; }
        .fee-config-grid { display: grid; grid-template-columns: repeat(2, 1fr); gap: 15px; }
        .fee-config-item { display: flex; flex-direction: column; gap: 8px; }
        .fee-config-item label { color: #a0a0b0; font-size: 0.85rem; font-weight: 500; }
        .fee-config-item input[type="number"] { background: linear-gradient(135deg, #1a1a2e, #202035); border: 1px solid #3a3a5a; color: #fff; padding: 10px 12px; border-radius: 6px; font-size: 0.9rem; }
        .fee-config-item input[type="number"]:focus { outline: none; border-color: #f0ad4e; }
        .fee-config-item.checkbox-item label { display: flex; align-items: center; gap: 10px; cursor: pointer; }
        .fee-config-item.checkbox-item input[type="checkbox"] { width: 18px; height: 18px; cursor: pointer; }
        .fee-input-group { display: flex; align-items: center; gap: 10px; }
        .fee-input-group input { flex: 1; }
        .fee-suffix { color: #888; font-size: 0.85rem; min-width: 60px; }
        .fee-actions { margin-top: 15px; text-align: right; }
        .apply-fee-btn { background: linear-gradient(135deg, #f0ad4e, #ec971f); color: #1a1a2e; border: none; padding: 10px 20px; border-radius: 8px; font-weight: 600; cursor: pointer; }
        .apply-fee-btn:hover { opacity: 0.9; }
        .config-hint { color: #666; font-size: 0.75rem; }
        .fee-rates-display { display: flex; gap: 15px; margin-bottom: 15px; }
        .fee-rate-item { flex: 1; background: linear-gradient(135deg, #1a1a2e, #202035); border: 1px solid #3a3a5a; border-radius: 8px; padding: 12px; text-align: center; }
        .fee-rate-item.highlight { border-color: #00d4aa; }
        .fee-rate-label { display: block; color: #888; font-size: 0.75rem; text-transform: uppercase; margin-bottom: 5px; }
        .fee-rate-value { display: block; color: #fff; font-size: 1.1rem; font-weight: 600; }
        .fee-rate-value.positive { color: #00d4aa; }
        .fee-note { color: #666; font-size: 0.8rem; margin: 0; font-style: italic; }

        /* Resolve Modal */
        .resolve-modal { background: linear-gradient(135deg, #1a1a2e, #252545); border: 2px solid #f0ad4e; border-radius: 20px; padding: 35px; max-width: 500px; width: 90%; }
        .resolve-modal h2 { color: #f0ad4e; margin-bottom: 5px; font-size: 1.4rem; }
        .resolve-modal .modal-subtitle { color: #a0a0b0; margin-bottom: 25px; font-size: 0.95rem; }
        .resolve-summary { background: linear-gradient(135deg, #252542, #2a2a50); border: 1px solid #3a3a5a; border-radius: 12px; padding: 20px; margin-bottom: 20px; }
        .resolve-row { display: flex; justify-content: space-between; align-items: center; padding: 10px 0; border-bottom: 1px solid #3a3a5a; }
        .resolve-row:last-child { border-bottom: none; }
        .resolve-row.big { padding: 15px 0; margin-top: 10px; border-top: 2px solid #3a3a5a; }
        .resolve-label { color: #a0a0b0; font-size: 0.9rem; }
        .resolve-value { color: #fff; font-weight: 600; font-size: 1rem; }
        .resolve-value.highlight { color: #f0ad4e; }
        .resolve-value.positive { color: #00d4aa; font-size: 1.2rem; }
        .resolve-value.negative { color: #ff6b6b; font-size: 1.2rem; }
        .resolve-warning { background: rgba(240, 173, 78, 0.1); border: 1px solid #f0ad4e; border-radius: 8px; padding: 12px 15px; margin-bottom: 20px; }
        .resolve-warning p { color: #f0ad4e; font-size: 0.85rem; margin: 5px 0; }
        .resolve-confirm-btn { flex: 1; background: linear-gradient(135deg, #f0ad4e, #ec971f); border: none; color: #1a1a2e; padding: 14px; border-radius: 10px; cursor: pointer; font-weight: 700; font-size: 1rem; }
        .resolve-confirm-btn:hover { opacity: 0.9; }
        
        .circuit-breaker-banner { background: linear-gradient(135deg, #ff000022, #ff6b6b11); border: 2px solid #ff0000; border-radius: 12px; padding: 20px; margin-bottom: 20px; }
        .cb-content { display: flex; align-items: center; gap: 15px; }
        .cb-icon { font-size: 2rem; }
        .cb-text { flex: 1; }
        .cb-text strong { color: #ff0000; font-size: 1.2rem; }
        .cb-text p { color: #ff6b6b; margin-top: 5px; }
        .cb-reset-btn { background: #ff6b6b; color: white; border: none; padding: 10px 20px; border-radius: 8px; cursor: pointer; font-weight: 600; }
        .live-controls-section { background: linear-gradient(135deg, #1a1a2e, #252545); border: 1px solid #3a3a5a; border-radius: 16px; padding: 25px; margin-bottom: 20px; }
        .controls-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 25px; padding-bottom: 15px; border-bottom: 1px solid #3a3a5a; }
        .controls-header h2 { color: #ff6b6b; font-size: 1.5rem; }
        .controls-buttons { display: flex; gap: 12px; }
        .enable-btn { background: linear-gradient(135deg, #00d4aa, #00b894); color: #1a1a2e; border: none; padding: 14px 28px; border-radius: 10px; font-weight: 700; cursor: pointer; font-size: 1rem; }
        .enable-btn:disabled { background: #444; color: #888; cursor: not-allowed; }
        .executor-warning { color: #f0ad4e; font-size: 0.85rem; display: flex; align-items: center; }
        .disable-btn { background: #3a3a5a; color: white; border: 1px solid #555; padding: 14px 28px; border-radius: 10px; font-weight: 600; cursor: pointer; }
        .emergency-btn { background: linear-gradient(135deg, #ff0000, #cc0000); color: white; border: none; padding: 14px 28px; border-radius: 10px; font-weight: 700; cursor: pointer; animation: pulse 2s infinite; }
        @keyframes pulse { 0%, 100% { box-shadow: 0 0 0 0 rgba(255, 0, 0, 0.4); } 50% { box-shadow: 0 0 0 10px rgba(255, 0, 0, 0); } }
        .status-grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 15px; }
        .status-card { background: linear-gradient(135deg, #252542, #2a2a50); border-radius: 12px; padding: 20px; text-align: center; border: 1px solid #3a3a5a; }
        .status-card.danger { border: 2px solid #ff0000; background: linear-gradient(135deg, #3a1a1a, #2a1a1a); }
        .status-card.ok { border: 2px solid #00d4aa; }
        .status-label { display: block; color: #a0a0b0; font-size: 0.85rem; margin-bottom: 8px; text-transform: uppercase; letter-spacing: 0.5px; font-weight: 600; }
        .status-value { display: block; font-size: 1.2rem; font-weight: 700; color: #fff; }
        .pnl-section, .config-section, .trades-section { background: linear-gradient(135deg, #1a1a2e, #252545); border: 1px solid #3a3a5a; border-radius: 16px; padding: 25px; margin-bottom: 20px; }
        .pnl-section h3, .config-section h3, .trades-section h3 { color: #00d4aa; margin-bottom: 20px; font-size: 1.2rem; }
        .pnl-grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 15px; }
        .pnl-card { background: linear-gradient(135deg, #252542, #2a2a50); border-radius: 12px; padding: 20px; text-align: center; border: 1px solid #3a3a5a; }
        .pnl-label { display: block; color: #a0a0b0; font-size: 0.85rem; margin-bottom: 8px; text-transform: uppercase; letter-spacing: 0.5px; font-weight: 600; }
        .pnl-value { display: block; font-size: 1.5rem; font-weight: 700; }
        .pnl-value.positive { color: #00d4aa; }
        .pnl-value.negative { color: #ff6b6b; }
        .pnl-detail { display: block; color: #888; font-size: 0.85rem; margin-top: 8px; }
        .pnl-detail-bright { display: block; color: #fff; font-size: 1rem; margin-top: 8px; font-weight: 600; }
        .limit-bar { background: #333; height: 10px; border-radius: 5px; overflow: hidden; margin: 12px 0; }
        .limit-fill { height: 100%; background: linear-gradient(90deg, #00d4aa, #f0ad4e, #ff6b6b); }
        .pnl-actions { margin-top: 20px; text-align: right; }
        .reset-btn { background: transparent; border: 1px solid #3a3a5a; color: #a0a0b0; padding: 10px 20px; border-radius: 8px; cursor: pointer; }
        .reset-btn:hover { border-color: #00d4aa; color: #00d4aa; }
        .config-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 20px; padding-bottom: 15px; border-bottom: 1px solid #3a3a5a; }
        .apply-btn { background: linear-gradient(135deg, #00d4aa, #00b894); color: #1a1a2e; border: none; padding: 10px 20px; border-radius: 8px; font-weight: 600; cursor: pointer; }
        .config-grid { display: grid; grid-template-columns: repeat(2, 1fr); gap: 25px; }
        .config-item { display: flex; flex-direction: column; gap: 10px; }
        .config-item.full-width { grid-column: span 2; }
        .config-item label { color: #a0a0b0; font-size: 0.95rem; font-weight: 600; text-transform: uppercase; letter-spacing: 0.5px; }
        .amount-buttons, .mode-buttons { display: flex; gap: 10px; flex-wrap: wrap; }
        .amount-buttons button, .mode-buttons button { flex: 1; min-width: 60px; background: linear-gradient(135deg, #252542, #2a2a50); border: 1px solid #3a3a5a; color: #fff; padding: 12px; border-radius: 8px; cursor: pointer; font-weight: 600; }
        .amount-buttons button.active, .mode-buttons button.active { background: linear-gradient(135deg, #00d4aa, #00b894); color: #1a1a2e; border-color: #00d4aa; }
        .amount-buttons button.custom-active { background: linear-gradient(135deg, #6c5ce7, #a29bfe); border-color: #6c5ce7; color: #fff; }
        .custom-amount-row { display: flex; gap: 10px; margin-top: 10px; }
        .custom-amount-row input { flex: 1; background: linear-gradient(135deg, #252542, #2a2a50); border: 1px solid #3a3a5a; color: #fff; padding: 10px 12px; border-radius: 8px; font-size: 0.95rem; }
        .custom-amount-row input::placeholder { color: #666; }
        .custom-btn { background: linear-gradient(135deg, #6c5ce7, #a29bfe); color: white; border: none; padding: 10px 20px; border-radius: 8px; font-weight: 600; cursor: pointer; }
        .slider-container { display: flex; align-items: center; gap: 15px; }
        .slider-container input[type="range"] { flex: 1; }
        .slider-value { background: linear-gradient(135deg, #252542, #2a2a50); padding: 10px 15px; border-radius: 8px; min-width: 80px; text-align: center; font-weight: 600; border: 1px solid #3a3a5a; color: #fff; }
        .threshold-warning { margin-top: 10px; padding: 10px 15px; border-radius: 8px; font-size: 0.9rem; font-weight: 500; }
        .threshold-warning.warning { background: rgba(240, 173, 78, 0.15); border: 1px solid #f0ad4e; color: #f0ad4e; }
        .threshold-warning.danger { background: rgba(255, 107, 107, 0.15); border: 1px solid #ff6b6b; color: #ff6b6b; }
        .config-item input[type="number"], .config-item select { background: linear-gradient(135deg, #252542, #2a2a50); border: 1px solid #3a3a5a; color: #fff; padding: 12px 15px; border-radius: 8px; font-size: 1rem; }
        .config-item select { appearance: none; background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' fill='%23fff' viewBox='0 0 16 16'%3E%3Cpath d='M8 11L3 6h10l-5 5z'/%3E%3C/svg%3E"); background-repeat: no-repeat; background-position: right 15px center; padding-right: 40px; cursor: pointer; }
        .config-item select option { background: #1a1a2e; color: #fff; }
        .currency-checkboxes { display: flex; gap: 15px; flex-wrap: wrap; margin-top: 5px; }
        .checkbox-label { display: flex; align-items: center; gap: 10px; cursor: pointer; color: #fff; font-weight: 500; background: linear-gradient(135deg, #252542, #2a2a50); border: 1px solid #3a3a5a; padding: 10px 20px; border-radius: 8px; transition: all 0.2s; }
        .checkbox-label:hover { border-color: #00d4aa; }
        .checkbox-label.checked { background: linear-gradient(135deg, #00d4aa22, #00b89422); border-color: #00d4aa; color: #00d4aa; }
        .checkbox-label input[type="checkbox"] { display: none; }
        .checkbox-label .checkmark { width: 18px; height: 18px; border: 2px solid #3a3a5a; border-radius: 4px; display: flex; align-items: center; justify-content: center; }
        .checkbox-label.checked .checkmark { background: #00d4aa; border-color: #00d4aa; }
        .checkbox-label.checked .checkmark::after { content: '✓'; color: #1a1a2e; font-size: 12px; font-weight: 700; }
        .selected-currencies { margin-top: 10px; padding: 10px 15px; background: rgba(0, 212, 170, 0.1); border: 1px solid #00d4aa; border-radius: 8px; color: #00d4aa; font-size: 0.9rem; }
        
        /* Trades Section */
        .trades-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 20px; }
        .trades-header h3 { margin: 0; }
        .export-btn { background: linear-gradient(135deg, #6c5ce7, #a29bfe); color: white; border: none; padding: 10px 20px; border-radius: 8px; font-weight: 600; cursor: pointer; font-size: 0.9rem; }
        .export-btn:hover { opacity: 0.9; }
        
        /* Filters Bar */
        .filters-bar { display: flex; gap: 20px; margin-bottom: 20px; flex-wrap: wrap; align-items: center; }
        .filter-group { display: flex; align-items: center; gap: 10px; }
        .filter-group label { color: #00d4aa; font-weight: 600; font-size: 0.9rem; }
        .filter-group select, .filter-group .path-input { background: linear-gradient(135deg, #252542, #2a2a50); border: 2px solid #00d4aa; color: #fff; padding: 8px 15px; border-radius: 8px; font-size: 0.9rem; min-width: 140px; }
        .filter-group select { appearance: none; background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' fill='%2300d4aa' viewBox='0 0 16 16'%3E%3Cpath d='M8 11L3 6h10l-5 5z'/%3E%3C/svg%3E"); background-repeat: no-repeat; background-position: right 10px center; padding-right: 35px; cursor: pointer; }
        .filter-group select option { background: #1a1a2e; color: #fff; }
        .filter-group .path-input { width: 120px; }
        .filter-group .path-input::placeholder { color: #666; }
        .filter-group.results-count span { color: #a0a0b0; font-size: 0.9rem; }
        
        /* Trades Table */
        .trades-table-container { overflow-x: auto; }
        .trades-table { width: 100%; border-collapse: collapse; }
        .trades-table th { background: linear-gradient(135deg, #00d4aa, #00b894); color: #1a1a2e; padding: 14px 12px; text-align: left; font-weight: 700; text-transform: uppercase; font-size: 0.8rem; white-space: nowrap; }
        .trades-table td { padding: 14px 12px; border-bottom: 1px solid #2a2a4a; color: #fff; vertical-align: middle; }
        .trades-table tr.expandable { cursor: pointer; }
        .trades-table tr.expandable:hover { background: rgba(255, 255, 255, 0.05); }
        .trades-table tr.win { background: rgba(0, 212, 170, 0.08); }
        .trades-table tr.loss { background: rgba(255, 107, 107, 0.08); }
        .trades-table tr.partial { background: rgba(240, 173, 78, 0.08); }
        .trades-table tr.failed { background: rgba(100, 100, 100, 0.08); }
        .trades-table .expand-cell { width: 30px; color: #00d4aa; font-size: 0.8rem; }
        .trades-table .time-cell { white-space: nowrap; font-size: 0.85rem; color: #a0a0b0; }
        .trades-table code { background: #333; padding: 4px 8px; border-radius: 4px; font-size: 0.8rem; }
        .trades-table .amount-cell { font-weight: 600; }
        .trades-table .fee-cell, .trades-table .slippage-cell { font-size: 0.85rem; }
        .trades-table .fee-cell .pct, .trades-table .slippage-cell .pct { display: block; color: #ff6b6b; }
        .trades-table .fee-cell .usd, .trades-table .slippage-cell .usd { display: block; color: #888; font-size: 0.75rem; }
        .trades-table .latency-cell { color: #a0a0b0; font-size: 0.85rem; }
        .trades-table td.positive .pct { color: #00d4aa; display: block; }
        .trades-table td.positive .usd { color: #00d4aa; display: block; font-size: 0.75rem; }
        .trades-table td.negative .pct { color: #ff6b6b; display: block; }
        .trades-table td.negative .usd { color: #ff6b6b; display: block; font-size: 0.75rem; }
        .badge { padding: 6px 14px; border-radius: 20px; font-size: 0.75rem; font-weight: 700; text-transform: uppercase; white-space: nowrap; }
        .badge.completed { background: rgba(0, 212, 170, 0.2); color: #00d4aa; }
        .badge.failed { background: rgba(255, 107, 107, 0.2); color: #ff6b6b; }
        .badge.partial { background: rgba(240, 173, 78, 0.2); color: #f0ad4e; }
        
        /* Expanded Row - Leg Details */
        .expanded-row { background: #1a1a2e !important; }
        .expanded-row td { padding: 0 !important; border-bottom: 2px solid #00d4aa; }
        .leg-details { padding: 20px 30px; }
        .leg-details h4 { color: #00d4aa; margin: 0 0 15px 0; font-size: 1rem; }
        .leg-details .no-details { color: #888; font-style: italic; }
        .leg-table { width: 100%; border-collapse: collapse; background: #252542; border-radius: 8px; overflow: hidden; }
        .leg-table th { background: #2a2a50; color: #a0a0b0; padding: 12px 15px; text-align: left; font-size: 0.8rem; text-transform: uppercase; font-weight: 600; }
        .leg-table td { padding: 12px 15px; border-bottom: 1px solid #3a3a5a; color: #fff; font-size: 0.9rem; }
        .leg-table tr:last-child td { border-bottom: none; }
        .leg-table .leg-num { color: #00d4aa; font-weight: 700; }
        .leg-table .leg-pair { font-weight: 600; }
        .leg-table .leg-side.buy { color: #00d4aa; font-weight: 600; }
        .leg-table .leg-side.sell { color: #ff6b6b; font-weight: 600; }
        .leg-table .negative { color: #ff6b6b; }
        
        /* Pagination */
        .pagination { display: flex; justify-content: center; align-items: center; gap: 10px; margin-top: 20px; padding-top: 20px; border-top: 1px solid #3a3a5a; }
        .page-btn { background: linear-gradient(135deg, #252542, #2a2a50); border: 1px solid #3a3a5a; color: #fff; padding: 8px 15px; border-radius: 6px; cursor: pointer; font-weight: 600; }
        .page-btn:hover:not(:disabled) { border-color: #00d4aa; color: #00d4aa; }
        .page-btn:disabled { opacity: 0.5; cursor: not-allowed; }
        .page-info { color: #a0a0b0; font-size: 0.9rem; padding: 0 15px; }
        
        .empty-state { text-align: center; padding: 50px; color: #888; }
        .empty-state .hint { font-size: 0.95rem; margin-top: 12px; color: #666; }
        
        /* Modal */
        .modal-overlay { position: fixed; top: 0; left: 0; right: 0; bottom: 0; background: rgba(0, 0, 0, 0.85); display: flex; align-items: center; justify-content: center; z-index: 1000; }
        .enable-modal { background: linear-gradient(135deg, #1a1a2e, #252545); border: 1px solid #3a3a5a; border-radius: 20px; padding: 35px; max-width: 500px; width: 90%; }
        .enable-modal h2 { color: #f0ad4e; margin-bottom: 10px; font-size: 1.4rem; }
        .executor-status-check { display: flex; align-items: center; gap: 10px; padding: 12px 16px; border-radius: 8px; margin-bottom: 20px; font-weight: 600; }
        .executor-status-check.ready { background: rgba(0, 212, 170, 0.15); border: 1px solid #00d4aa; color: #00d4aa; }
        .executor-status-check.offline { background: rgba(255, 107, 107, 0.15); border: 1px solid #ff6b6b; color: #ff6b6b; }
        .executor-status-check .status-icon { font-size: 1.2rem; }
        .executor-status-check .status-text { font-size: 0.95rem; }
        .modal-subtitle { color: #a0a0b0; margin-bottom: 25px; font-size: 0.95rem; }
        .config-summary { background: linear-gradient(135deg, #252542, #2a2a50); border: 1px solid #3a3a5a; border-radius: 12px; padding: 20px; margin-bottom: 20px; }
        .config-row { display: flex; justify-content: space-between; align-items: center; padding: 12px 0; border-bottom: 1px solid #3a3a5a; }
        .config-row:last-child { border-bottom: none; }
        .config-label { color: #a0a0b0; font-size: 0.95rem; }
        .config-value { color: #fff; font-weight: 600; font-size: 1rem; }
        .config-value.highlight-red { color: #ff6b6b; }
        .modal-warning-small { background: rgba(255, 107, 107, 0.1); border: 1px solid #ff6b6b; border-radius: 8px; padding: 12px 15px; margin-bottom: 25px; text-align: center; }
        .modal-warning-small p { color: #ff6b6b; font-weight: 500; margin: 0; font-size: 0.9rem; }
        .modal-buttons { display: flex; gap: 15px; }
        .cancel-btn { flex: 1; background: #3a3a5a; border: none; color: #fff; padding: 14px; border-radius: 10px; cursor: pointer; font-weight: 600; font-size: 1rem; }
        .cancel-btn:hover { background: #4a4a6a; }
        .confirm-btn { flex: 1; background: linear-gradient(135deg, #00d4aa, #00b894); border: none; color: #1a1a2e; padding: 14px; border-radius: 10px; cursor: pointer; font-weight: 700; font-size: 1rem; }
        
        .toast-notification { position: fixed; top: 20px; right: 20px; padding: 16px 28px; border-radius: 10px; z-index: 9999; font-weight: 600; animation: slideIn 0.3s ease; }
        .toast-notification.success { background: linear-gradient(135deg, #00d4aa, #00b894); color: #1a1a2e; }
        .toast-notification.error { background: linear-gradient(135deg, #ff6b6b, #ff5252); color: white; }
        .toast-notification.warning { background: linear-gradient(135deg, #f0ad4e, #ec971f); color: #1a1a2e; }
        @keyframes slideIn { from { transform: translateX(100%); opacity: 0; } to { transform: translateX(0); opacity: 1; } }
        .error-message { background: rgba(255, 107, 107, 0.1); border: 1px solid #ff6b6b; border-radius: 10px; padding: 15px 20px; margin-bottom: 20px; display: flex; justify-content: space-between; align-items: center; color: #ff6b6b; }
        .error-message button { background: none; border: none; color: #ff6b6b; font-size: 1.3rem; cursor: pointer; }
        
        @media (max-width: 1200px) { .performance-grid { grid-template-columns: repeat(3, 1fr); } }
        @media (max-width: 900px) { .status-grid, .pnl-grid { grid-template-columns: repeat(2, 1fr); } .config-grid { grid-template-columns: 1fr; } .performance-grid { grid-template-columns: repeat(2, 1fr); } .top-section { flex-direction: column; align-items: flex-start; } .holdings-inline { flex-direction: column; align-items: flex-start; } .filters-bar { flex-direction: column; align-items: flex-start; } }
        @media (max-width: 500px) { .status-grid, .pnl-grid, .performance-grid { grid-template-columns: 1fr; } .controls-buttons { flex-direction: column; } }
        
        /* Config hint text */
        .config-hint { display: block; color: #666; font-size: 0.75rem; margin-top: 5px; }
        
        /* Collapsible Risks Section */
        .risks-collapsible { display: flex; align-items: center; gap: 10px; padding: 15px 20px; background: linear-gradient(135deg, #252542, #2a2a50); border: 1px solid #f0ad4e; border-radius: 10px; cursor: pointer; transition: all 0.2s; }
        .risks-collapsible:hover { background: linear-gradient(135deg, #2a2a50, #353565); border-color: #f5c56d; }
        .risks-toggle { color: #f0ad4e; font-size: 0.9rem; }
        .risks-title { color: #f0ad4e; font-weight: 600; font-size: 1rem; }
        .risks-content { margin-top: 15px; background: linear-gradient(135deg, rgba(240, 173, 78, 0.08), rgba(255, 107, 107, 0.05)); border: 1px solid #3a3a5a; border-radius: 12px; padding: 20px; }
        .risk-section { margin-bottom: 20px; padding-bottom: 15px; border-bottom: 1px solid #3a3a5a; }
        .risk-section:last-child { margin-bottom: 0; padding-bottom: 0; border-bottom: none; }
        .risk-section h5 { color: #f0ad4e; margin: 0 0 10px 0; font-size: 0.95rem; }
        .risk-section p { color: #a0a0b0; font-size: 0.9rem; margin: 0 0 10px 0; line-height: 1.5; }
        .risk-section p:last-child { margin-bottom: 0; }
        .risk-section ul { margin: 10px 0; padding-left: 20px; }
        .risk-section li { color: #a0a0b0; font-size: 0.85rem; margin-bottom: 6px; line-height: 1.4; }
        .risk-section li strong { color: #fff; }
        .risk-section .action-note { color: #f0ad4e; font-weight: 500; margin-top: 10px; padding-top: 10px; border-top: 1px solid rgba(240, 173, 78, 0.3); }
        
        /* Modal config summary */
        .config-summary h4 { color: #a0a0b0; margin: 0 0 15px 0; font-size: 0.9rem; text-transform: uppercase; letter-spacing: 0.5px; }
        
        /* Config select dropdown */
        .config-item select { width: 100%; background: linear-gradient(135deg, #252542, #2a2a50); border: 1px solid #3a3a5a; border-radius: 8px; color: #fff; padding: 10px 15px; font-size: 0.95rem; cursor: pointer; }
        .config-item select option { background: #1a1a2e; color: #fff; }
        
        /* Scanner Status Section */
        .scanner-section { background: linear-gradient(135deg, #1a1a2e, #252545); border: 1px solid #3a3a5a; border-radius: 16px; padding: 25px; margin-bottom: 20px; }
        .scanner-section h3 { color: #6c5ce7; margin-bottom: 20px; font-size: 1.2rem; }
        .scanner-grid { display: grid; grid-template-columns: repeat(6, 1fr); gap: 15px; }
        .scanner-card { background: linear-gradient(135deg, #252542, #2a2a50); border: 1px solid #3a3a5a; border-radius: 10px; padding: 15px; text-align: center; }
        .scanner-card .scanner-label { display: block; color: #a0a0b0; font-size: 0.8rem; text-transform: uppercase; letter-spacing: 0.5px; margin-bottom: 8px; }
        .scanner-card .scanner-value { display: block; font-size: 1.2rem; font-weight: 700; color: #fff; }
        .scanner-card .scanner-value.running { color: #00d4aa; }
        .scanner-card .scanner-value.stopped { color: #888; }
        .scanner-card .scanner-value.highlight { color: #f0ad4e; }
        
        /* Opportunities Section */
        .opportunities-section { background: linear-gradient(135deg, #1a1a2e, #252545); border: 1px solid #3a3a5a; border-radius: 16px; padding: 25px; margin-bottom: 20px; }
        .opportunities-section h3 { color: #00d4aa; margin-bottom: 20px; font-size: 1.2rem; }
        .opportunities-table-wrapper { overflow-x: auto; }
        .opportunities-table { width: 100%; border-collapse: collapse; }
        .opportunities-table th { background: linear-gradient(135deg, #6c5ce7, #a29bfe); color: #fff; padding: 12px; text-align: left; font-weight: 600; text-transform: uppercase; font-size: 0.8rem; white-space: nowrap; }
        .opportunities-table td { padding: 12px; border-bottom: 1px solid #2a2a4a; color: #fff; vertical-align: middle; font-size: 0.9rem; }
        .opportunities-table tr.executed { background: rgba(0, 212, 170, 0.08); }
        .opportunities-table tr.skipped { background: rgba(108, 92, 231, 0.08); }
        .opportunities-table tr.missed { background: rgba(255, 107, 107, 0.08); }
        .opportunities-table tr.pending { background: rgba(240, 173, 78, 0.08); }
        .opportunities-table .opp-time { white-space: nowrap; font-size: 0.85rem; color: #a0a0b0; }
        .opportunities-table .opp-path { font-weight: 500; }
        .opportunities-table .opp-profit { font-weight: 600; }
        .opportunities-table .opp-profit.positive { color: #00d4aa; }
        .opportunities-table .opp-profit .usd { display: block; font-size: 0.75rem; color: #888; }
        .opportunities-table .opp-reason { font-size: 0.85rem; color: #a0a0b0; }
        .opportunities-table .trade-link { color: #6c5ce7; font-weight: 500; }
        .opp-badge { padding: 4px 10px; border-radius: 12px; font-size: 0.75rem; font-weight: 600; text-transform: uppercase; }
        .opp-badge.executed { background: rgba(0, 212, 170, 0.2); color: #00d4aa; }
        .opp-badge.skipped { background: rgba(108, 92, 231, 0.2); color: #a29bfe; }
        .opp-badge.missed { background: rgba(255, 107, 107, 0.2); color: #ff6b6b; }
        .opp-badge.pending { background: rgba(240, 173, 78, 0.2); color: #f0ad4e; }
        .opp-badge.expired { background: rgba(100, 100, 100, 0.2); color: #888; }
        
        @media (max-width: 1200px) { .scanner-grid { grid-template-columns: repeat(3, 1fr); } }
        @media (max-width: 768px) { .scanner-grid { grid-template-columns: repeat(2, 1fr); } }
      `}</style>
    </div>
  );
}

export default LiveTradingPanel;
