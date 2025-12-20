// ============================================
// KrakenCryptoX v2.0 - Paper Trading Panel
// Single Balance Pool with Trade Controls
// Updated UI - Simplified Performance, Toast, No Popup
// Kill Switch Protection Added
// ============================================

import React, { useState, useEffect, useCallback } from 'react';
import { api } from '../services/api';

export function PaperTradingPanel() {
  const [wallet, setWallet] = useState(null);
  const [trades, setTrades] = useState([]);
  const [stats, setStats] = useState(null);
  const [settings, setSettings] = useState(null);
  const [killSwitch, setKillSwitch] = useState(null);
  const [engineSettings, setEngineSettings] = useState(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  const [toast, setToast] = useState(null);
  const [pendingTradeSettings, setPendingTradeSettings] = useState({});
  
  // Base currency filter
  const [showCustomCurrencies, setShowCustomCurrencies] = useState(false);
  const [customCurrencies, setCustomCurrencies] = useState([]);
  const [initialLoadDone, setInitialLoadDone] = useState(false);
  
  const AVAILABLE_CURRENCIES = ['USD', 'USDT', 'EUR', 'BTC', 'ETH'];

  // Show toast notification
  const showToast = (message, type = 'success') => {
    setToast({ message, type });
    setTimeout(() => setToast(null), 3000);
  };

  // Fetch all paper trading data
  const fetchData = useCallback(async () => {
    try {
      setError(null);
      const [walletData, tradesData, statsData, settingsData, killSwitchData, engineData] = await Promise.all([
        api.getPaperWallet(),
        api.getPaperTrades(50),
        api.getPaperTradingStats(),
        api.getPaperTradingSettings(),
        api.getKillSwitchStatus(),
        api.getEngineSettings(),
      ]);
      setWallet(walletData);
      setTrades(tradesData.trades || tradesData || []);
      setStats(statsData);
      setSettings(settingsData);
      setKillSwitch(killSwitchData);
      setEngineSettings(engineData);
      
      // Load custom currencies from settings ONLY on initial load
      if (!initialLoadDone) {
        if (settingsData?.custom_currencies && settingsData.custom_currencies.length > 0) {
          setCustomCurrencies(settingsData.custom_currencies);
        }
        if (settingsData?.base_currency === 'CUSTOM') {
          setShowCustomCurrencies(true);
        }
        setInitialLoadDone(true);
      }
    } catch (err) {
      console.error('Error fetching paper trading data:', err);
      setError(err.message);
      
      try {
        await api.initializePaperTrading();
        const [walletData, tradesData, statsData, settingsData, killSwitchData] = await Promise.all([
          api.getPaperWallet(),
          api.getPaperTrades(50),
          api.getPaperTradingStats(),
          api.getPaperTradingSettings(),
          api.getKillSwitchStatus(),
        ]);
        setWallet(walletData);
        setTrades(tradesData.trades || tradesData || []);
        setStats(statsData);
        setSettings(settingsData);
        setKillSwitch(killSwitchData);
        setError(null);
      } catch (initErr) {
        console.error('Error initializing:', initErr);
      }
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchData();
    const interval = setInterval(fetchData, 5000);
    return () => clearInterval(interval);
  }, [fetchData]);

  const handleToggle = async () => {
    try {
      const newState = !settings.is_active;
      await api.togglePaperTrading(newState);
      setSettings({ ...settings, is_active: newState });
      showToast(newState ? '✅ Auto-trading enabled' : '⏸️ Auto-trading paused');
    } catch (err) {
      setError('Failed to toggle paper trading');
    }
  };

  // Handle Starting Balance change - resets wallet immediately
  const handleStartingBalanceChange = async (newBalance) => {
    try {
      await api.resetPaperWallet(newBalance);
      showToast(`✅ Wallet reset to $${newBalance}`);
      fetchData();
    } catch (err) {
      setError('Failed to reset wallet');
    }
  };

  // Handle Reset button - no popup, just reset with toast
  const handleReset = async () => {
    const currentStartingBalance = wallet?.initial_balance || 100;
    try {
      await api.resetPaperWallet(currentStartingBalance);
      showToast(`✅ Wallet reset to $${currentStartingBalance}`);
      fetchData();
    } catch (err) {
      setError('Failed to reset wallet');
    }
  };

  // Handle Kill Switch Reset
  const handleResetKillSwitch = async () => {
    try {
      await api.resetKillSwitch();
      showToast('✅ Kill switch reset - trading resumed');
      fetchData();
    } catch (err) {
      setError('Failed to reset kill switch');
    }
  };

  // Handle Kill Switch ON/OFF toggle
  const handleKillSwitchToggle = async (enabled) => {
    try {
      await api.updateKillSwitchSettings({ enabled });
      showToast(enabled ? '🛡️ Kill switch enabled' : '⚠️ Kill switch disabled');
      fetchData();
    } catch (err) {
      setError('Failed to update kill switch');
    }
  };

  const handleSettingChange = (key, value) => {
    // Store in pending changes instead of auto-saving
    setPendingTradeSettings(prev => ({ ...prev, [key]: value }));
  };

  const handleApplyTradeSettings = async () => {
    if (Object.keys(pendingTradeSettings).length === 0) return;
    
    try {
      await api.updatePaperTradingSettings(pendingTradeSettings);
      setSettings({ ...settings, ...pendingTradeSettings });
      setPendingTradeSettings({});
      setToast({ type: 'success', message: 'Trade settings applied!' });
      setTimeout(() => setToast(null), 3000);
    } catch (err) {
      setError('Failed to update settings');
    }
  };

  const getTradeSettingValue = (key, defaultValue) => {
    if (pendingTradeSettings[key] !== undefined) return pendingTradeSettings[key];
    if (settings?.[key] !== undefined) return settings[key];
    return defaultValue;
  };

  const hasPendingTradeSettings = Object.keys(pendingTradeSettings).length > 0;

  const formatTime = (timestamp) => {
    if (!timestamp) return '--';
    try {
      let ts = timestamp;
      if (!timestamp.endsWith('Z') && !timestamp.includes('+')) {
        ts = timestamp + 'Z';
      }
      return new Date(ts).toLocaleTimeString('en-US', { 
        timeZone: 'America/New_York', 
        hour: '2-digit', 
        minute: '2-digit', 
        second: '2-digit' 
      });
    } catch {
      return '--';
    }
  };

  const formatPct = (value) => {
    if (value === null || value === undefined) return '--';
    const num = parseFloat(value);
    if (isNaN(num)) return '--';
    const sign = num >= 0 ? '+' : '';
    return `${sign}${num.toFixed(2)}%`;
  };

  const formatCurrency = (value) => {
    if (value === null || value === undefined) return '--';
    const num = parseFloat(value);
    if (isNaN(num)) return '--';
    const sign = num >= 0 ? '+' : '';
    return `${sign}$${num.toFixed(2)}`;
  };

  const formatProfitLoss = (value) => {
    if (value === null || value === undefined) return '$0.00';
    const num = parseFloat(value);
    if (isNaN(num)) return '$0.00';
    if (num >= 0) {
      return `+$${num.toFixed(2)}`;
    } else {
      return `-$${Math.abs(num).toFixed(2)}`;
    }
  };

  // Calculate avg profit and avg loss per trade
  const getAvgProfit = () => {
    if (!trades || trades.length === 0) return 0;
    const wins = trades.filter(t => t.actual_profit_amount > 0);
    if (wins.length === 0) return 0;
    const total = wins.reduce((sum, t) => sum + t.actual_profit_amount, 0);
    return total / wins.length;
  };

  const getAvgLoss = () => {
    if (!trades || trades.length === 0) return 0;
    const losses = trades.filter(t => t.actual_profit_amount < 0);
    if (losses.length === 0) return 0;
    const total = losses.reduce((sum, t) => sum + Math.abs(t.actual_profit_amount), 0);
    return total / losses.length;
  };

  if (loading) {
    return (
      <div className="panel paper-trading-panel loading">
        <p>Loading paper trading data...</p>
      </div>
    );
  }

  return (
    <div className="panel paper-trading-panel">
      {/* Toast Notification */}
      {toast && (
        <div className={`toast-notification ${toast.type}`}>
          {toast.message}
        </div>
      )}

      {error && (
        <div className="error-message">
          ⚠️ {error}
          <button onClick={() => setError(null)}>×</button>
        </div>
      )}

      {/* Kill Switch Alert Banner */}
      {killSwitch?.is_killed && (
        <div className="kill-switch-banner">
          <div className="kill-switch-content">
            <span className="kill-switch-icon">🛑</span>
            <div className="kill-switch-text">
              <strong>KILL SWITCH ACTIVATED</strong>
              <p>{killSwitch.kill_reason}</p>
            </div>
            <button className="kill-switch-reset-btn" onClick={handleResetKillSwitch}>
              🔓 Reset & Resume Trading
            </button>
          </div>
        </div>
      )}

      {/* Kill Switch Status - With ON/OFF Toggle */}
      {killSwitch && !killSwitch.is_killed && (
        <div className="kill-switch-status-bar">
          <div className="kill-switch-indicator">
            <span className="ks-icon">🛡️</span>
            <span className="ks-label">Kill Switch</span>
            <div className="ks-toggle-buttons">
              <button 
                className={`ks-toggle-btn ${killSwitch.settings?.enabled !== false ? 'active' : ''}`}
                onClick={() => handleKillSwitchToggle(true)}
              >
                ON
              </button>
              <button 
                className={`ks-toggle-btn off ${killSwitch.settings?.enabled === false ? 'active' : ''}`}
                onClick={() => handleKillSwitchToggle(false)}
              >
                OFF
              </button>
            </div>
          </div>
          {killSwitch.settings?.enabled !== false ? (
            <div className="kill-switch-details">
              <span className="ks-stat">
                Loss from peak: {killSwitch.loss_from_peak_pct?.toFixed(1) || 0}% / {killSwitch.settings?.max_loss_pct || 30}%
              </span>
              <span className="ks-stat">
                Consecutive losses: {killSwitch.consecutive_losses || 0} / {killSwitch.settings?.max_consecutive_losses || 10}
              </span>
            </div>
          ) : (
            <div className="kill-switch-disabled-warning">
              ⚠️ Kill Switch disabled - no loss protection active
            </div>
          )}
        </div>
      )}

      {/* Wallet Section */}
      <div className="wallet-section">
        <div className="wallet-header">
          <div className="wallet-controls">
            {(() => {
              const scannerOff = engineSettings && engineSettings.scanner_enabled === false;
              const isKilled = killSwitch?.is_killed;
              const isActive = settings?.is_active;
              
              return (
                <>
                  <button 
                    className={`toggle-btn ${isActive && !scannerOff ? 'active' : 'inactive'}`}
                    onClick={handleToggle}
                    disabled={isKilled || scannerOff}
                    title={scannerOff ? 'Enable Scanner first' : ''}
                  >
                    {isKilled ? '🛑 KILLED' : 
                     scannerOff ? '⚠️ SCANNER OFF' :
                     (isActive ? '🟢 AUTO-TRADING ON' : '🔴 AUTO-TRADING OFF')}
                  </button>
                  {scannerOff && !isKilled && (
                    <span className="scanner-off-hint">Enable Scanner in Engine Settings</span>
                  )}
                </>
              );
            })()}
            <button className="reset-btn" onClick={handleReset}>
              🔄 Reset
            </button>
          </div>
        </div>

        <div className="wallet-stats">
          {/* Starting Balance - Now a dropdown */}
          <div className="stat-card">
            <span className="stat-label">Starting Balance</span>
            <select 
              className="starting-balance-select"
              value={wallet?.initial_balance || 100}
              onChange={(e) => handleStartingBalanceChange(parseFloat(e.target.value))}
            >
              <option value={100}>$100</option>
              <option value={200}>$200</option>
              <option value={500}>$500</option>
              <option value={1000}>$1,000</option>
              <option value={5000}>$5,000</option>
              <option value={10000}>$10,000</option>
            </select>
          </div>
          <div className="stat-card highlight">
            <span className="stat-label">Current Balance</span>
            <span className="stat-value">${wallet?.balance?.toFixed(2) || '100.00'}</span>
          </div>
          {/* Peak Balance */}
          <div className="stat-card peak">
            <span className="stat-label">📈 Peak Balance</span>
            <span className="stat-value">${killSwitch?.peak_balance?.toFixed(2) || wallet?.balance?.toFixed(2) || '100.00'}</span>
          </div>
          {/* Profit/Loss - Just amount, no percentage */}
          <div className={`stat-card ${(wallet?.profit_loss || 0) >= 0 ? 'positive' : 'negative'}`}>
            <span className="stat-label">Profit/Loss</span>
            <span className="stat-value">
              {formatProfitLoss(wallet?.profit_loss)}
            </span>
          </div>
        </div>
      </div>

      {/* Performance Section - Simplified to 1 row */}
      <div className="stats-section">
        <h3>📊 Performance</h3>
        <div className="stats-grid-single-row">
          <div className="stat-item">
            <span className="stat-label">Total Trades</span>
            <span className="stat-value">{stats?.total_trades || 0}</span>
          </div>
          <div className="stat-item positive">
            <span className="stat-label">Wins</span>
            <span className="stat-value">{stats?.winning_trades || 0}</span>
          </div>
          <div className="stat-item negative">
            <span className="stat-label">Losses</span>
            <span className="stat-value">{stats?.losing_trades || 0}</span>
          </div>
          <div className="stat-item">
            <span className="stat-label">Avg Profit/Trade</span>
            <span className="stat-value positive-text">{stats?.winning_trades > 0 ? formatCurrency(getAvgProfit()) : '$0.00'}</span>
          </div>
          <div className="stat-item">
            <span className="stat-label">Avg Loss/Trade</span>
            <span className="stat-value negative-text">{stats?.losing_trades > 0 && getAvgLoss() > 0 ? `-$${getAvgLoss().toFixed(2)}` : '$0.00'}</span>
          </div>
        </div>
      </div>

      {/* Trade Controls Section */}
      <div className="trade-controls-section">
        <div className="trade-controls-header">
          <h3>🎛️ Trade Controls</h3>
          {hasPendingTradeSettings && (
            <span className="pending-badge">⚠️ {Object.keys(pendingTradeSettings).length} unsaved</span>
          )}
        </div>
        
        {/* Base Currency Selection */}
        <div className="base-currency-section">
          <div className="base-currency-row">
            <label className="control-label">Base Currency:</label>
            <select 
              className="control-select base-currency-select"
              value={getTradeSettingValue('base_currency', 'ALL')}
              onChange={async (e) => {
                const value = e.target.value;
                if (value === 'CUSTOM') {
                  setShowCustomCurrencies(true);
                  // Don't apply yet - wait for user to select currencies and click Apply
                } else {
                  setShowCustomCurrencies(false);
                  setCustomCurrencies([]);
                  // Apply immediately for non-CUSTOM selections
                  try {
                    await api.updatePaperTradingSettings({
                      base_currency: value,
                      custom_currencies: []
                    });
                    const newSettings = await api.getPaperTradingSettings();
                    setSettings(newSettings);
                    showToast(`✓ Trading ${value === 'ALL' ? 'all' : value} paths`);
                  } catch (err) {
                    console.error('Failed to apply base currency:', err);
                    showToast('Failed to apply settings', 'error');
                  }
                }
              }}
            >
              <option value="ALL">ALL (any path)</option>
              <option value="USD">USD</option>
              <option value="USDT">USDT</option>
              <option value="EUR">EUR</option>
              <option value="BTC">BTC</option>
              <option value="ETH">ETH</option>
              <option value="CUSTOM">CUSTOM...</option>
            </select>
            
            {/* Show selected currencies when CUSTOM is active */}
            {getTradeSettingValue('base_currency', 'ALL') === 'CUSTOM' && settings?.custom_currencies?.length > 0 && (
              <div className="selected-currencies">
                <span className="selected-label">Active:</span>
                {settings.custom_currencies.map(c => (
                  <span key={c} className="selected-currency-tag">{c}</span>
                ))}
              </div>
            )}
            
            <span className="control-hint">ℹ️ Only execute paths starting with this currency</span>
          </div>
          
          {/* Custom Currency Selection */}
          {(showCustomCurrencies || getTradeSettingValue('base_currency', 'ALL') === 'CUSTOM') && (
            <div className="custom-currencies-panel">
              <p className="custom-label">Select currencies to trade:</p>
              <div className="currency-checkboxes">
                {AVAILABLE_CURRENCIES.map(currency => (
                  <label key={currency} className={`currency-checkbox ${customCurrencies.includes(currency) ? 'checked' : ''}`}>
                    <input
                      type="checkbox"
                      checked={customCurrencies.includes(currency)}
                      onChange={(e) => {
                        e.stopPropagation();
                        let newCurrencies;
                        if (e.target.checked) {
                          newCurrencies = [...customCurrencies, currency];
                        } else {
                          newCurrencies = customCurrencies.filter(c => c !== currency);
                        }
                        setCustomCurrencies(newCurrencies);
                      }}
                    />
                    <span>{currency}</span>
                  </label>
                ))}
              </div>
              <div className="custom-actions">
                <button 
                  className="apply-custom-btn"
                  onClick={async () => {
                    if (customCurrencies.length > 0) {
                      try {
                        await api.updatePaperTradingSettings({
                          base_currency: 'CUSTOM',
                          custom_currencies: customCurrencies
                        });
                        showToast(`✓ Trading: ${customCurrencies.join(', ')} paths`);
                        // Refresh settings to show updated values
                        const newSettings = await api.getPaperTradingSettings();
                        setSettings(newSettings);
                      } catch (err) {
                        console.error('Failed to apply settings:', err);
                        showToast('Failed to apply settings', 'error');
                      }
                    } else {
                      showToast('Select at least one currency', 'error');
                    }
                  }}
                  disabled={customCurrencies.length === 0}
                >
                  ✓ Apply ({customCurrencies.length} selected)
                </button>
                {customCurrencies.length > 0 && (
                  <span className="pending-selection">
                    Selected: {customCurrencies.join(', ')}
                  </span>
                )}
              </div>
            </div>
          )}
        </div>
        
        <div className="trade-controls-grid">
          
          {/* Row 1: Min Profit Threshold */}
          <div className="control-card">
            <label className="control-label">Min Profit Threshold</label>
            <select 
              className="control-select"
              value={getTradeSettingValue('min_profit_threshold', 0.0005)}
              onChange={(e) => handleSettingChange('min_profit_threshold', parseFloat(e.target.value))}
            >
              <option value={0.0001}>0.01%</option>
              <option value={0.0005}>0.05%</option>
              <option value={0.001}>0.1%</option>
              <option value={0.002}>0.2%</option>
              <option value={0.003}>0.3%</option>
              <option value={0.005}>0.5%</option>
              <option value={0.01}>1.0%</option>
            </select>
            <span className="control-hint">ℹ️ Skip trades below this profit %</span>
          </div>

          {/* Row 1: Trade Amount */}
          <div className="control-card">
            <label className="control-label">Trade Amount</label>
            <select 
              className="control-select"
              value={getTradeSettingValue('trade_amount', 10)}
              onChange={(e) => handleSettingChange('trade_amount', parseFloat(e.target.value))}
            >
              <option value={1}>$1</option>
              <option value={2}>$2</option>
              <option value={3}>$3</option>
              <option value={4}>$4</option>
              <option value={5}>$5</option>
              <option value={6}>$6</option>
              <option value={7}>$7</option>
              <option value={8}>$8</option>
              <option value={9}>$9</option>
              <option value={10}>$10</option>
              <option value={25}>$25</option>
              <option value={50}>$50</option>
              <option value={100}>$100</option>
              <option value={250}>$250</option>
              <option value={500}>$500</option>
              <option value={1000}>$1000</option>
            </select>
            <span className="control-hint">ℹ️ USD amount per trade</span>
          </div>

          {/* Row 1: Fee Tier */}
          <div className="control-card">
            <label className="control-label">Fee Tier (Taker)</label>
            <select 
              className="control-select"
              value={getTradeSettingValue('fee_rate', 0.0026)}
              onChange={(e) => handleSettingChange('fee_rate', parseFloat(e.target.value))}
            >
              <option value={0.004}>Starter (0.40%)</option>
              <option value={0.0035}>Intermediate (0.35%)</option>
              <option value={0.0026}>Pro (0.26%)</option>
              <option value={0.0024}>Advanced (0.24%)</option>
              <option value={0.0022}>Expert (0.22%)</option>
              <option value={0.002}>VIP (0.20%)</option>
            </select>
            <span className="control-hint">ℹ️ Kraken taker fee based on 30-day volume</span>
          </div>

          {/* Row 2: Cooldown */}
          <div className="control-card">
            <label className="control-label">Cooldown</label>
            <select 
              className="control-select"
              value={getTradeSettingValue('cooldown_seconds', 0)}
              onChange={(e) => handleSettingChange('cooldown_seconds', parseInt(e.target.value))}
            >
              <option value={0}>No cooldown</option>
              <option value={1}>1 second</option>
              <option value={2}>2 seconds</option>
              <option value={3}>3 seconds</option>
              <option value={5}>5 seconds</option>
              <option value={10}>10 seconds</option>
            </select>
            <span className="control-hint">ℹ️ Wait time between trades</span>
          </div>

          {/* Row 2: Max Trades per Cycle */}
          <div className="control-card">
            <label className="control-label">Max Trades per Cycle</label>
            <select 
              className="control-select"
              value={getTradeSettingValue('max_trades_per_cycle', 5)}
              onChange={(e) => handleSettingChange('max_trades_per_cycle', parseInt(e.target.value))}
            >
              <option value={1}>1 trade</option>
              <option value={2}>2 trades</option>
              <option value={3}>3 trades</option>
              <option value={4}>4 trades</option>
              <option value={5}>5 trades</option>
              <option value={6}>6 trades</option>
              <option value={7}>7 trades</option>
              <option value={8}>8 trades</option>
              <option value={9}>9 trades</option>
              <option value={10}>10 trades</option>
              <option value={15}>15 trades</option>
              <option value={20}>20 trades</option>
            </select>
            <span className="control-hint">ℹ️ Max trades per scan cycle</span>
          </div>

          {/* Row 2: Latency Penalty */}
          <div className="control-card">
            <label className="control-label">Latency Penalty (per leg)</label>
            <select 
              className="control-select"
              value={getTradeSettingValue('latency_penalty_pct', 0.001)}
              onChange={(e) => handleSettingChange('latency_penalty_pct', parseFloat(e.target.value))}
            >
              <option value={0}>0% (none)</option>
              <option value={0.0005}>0.05%</option>
              <option value={0.001}>0.10%</option>
              <option value={0.0015}>0.15%</option>
              <option value={0.002}>0.20%</option>
              <option value={0.0025}>0.25%</option>
            </select>
            <span className="control-hint">ℹ️ Simulates price movement during execution</span>
          </div>

        </div>
        
        {/* Apply Changes Button */}
        {hasPendingTradeSettings && (
          <div className="trade-controls-actions">
            <button className="apply-btn" onClick={handleApplyTradeSettings}>
              ✓ Apply Changes
            </button>
          </div>
        )}
      </div>

      {/* Recent Trades Section */}
      <div className="trades-section">
        <h3>📜 Recent Trades</h3>
        {trades.length === 0 ? (
          <div className="empty-state">
            <p>No trades yet.</p>
            <p className="hint">Trades will appear here when paper trading is active and profitable opportunities are found.</p>
          </div>
        ) : (
          <div className="trades-table-container">
            <table className="trades-table">
              <thead>
                <tr>
                  <th>Time</th>
                  <th>Path</th>
                  <th>Amount</th>
                  <th>Expected</th>
                  <th>Slippage</th>
                  <th>Actual</th>
                  <th>Profit</th>
                  <th>Status</th>
                </tr>
              </thead>
              <tbody>
                {trades.map((trade, idx) => (
                  <tr key={trade.id || idx} className={trade.status === 'WIN' ? 'win' : 'loss'}>
                    <td className="time">{formatTime(trade.executed_at)}</td>
                    <td className="path"><code>{trade.path}</code></td>
                    <td className="amount">${trade.trade_amount?.toFixed(2)}</td>
                    <td className="expected">{formatPct(trade.expected_net_profit_pct)}</td>
                    <td className="slippage">-{trade.slippage_pct?.toFixed(2)}%</td>
                    <td className={`actual ${trade.actual_net_profit_pct >= 0 ? 'positive' : 'negative'}`}>
                      {formatPct(trade.actual_net_profit_pct)}
                    </td>
                    <td className={`profit ${trade.actual_profit_amount >= 0 ? 'positive' : 'negative'}`}>
                      {formatCurrency(trade.actual_profit_amount)}
                    </td>
                    <td className="status">
                      <span className={`badge ${trade.status.toLowerCase()}`}>
                        {trade.status === 'WIN' ? '✓ Win' : '✗ Loss'}
                      </span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            <div className="trades-footer-note">
              Showing last 50 trades
            </div>
          </div>
        )}
      </div>

      {/* Footer - Removed auto-refresh text */}
      <div className="panel-footer">
        <span>Paper trading uses simulated money - no real funds at risk</span>
      </div>

      <style jsx>{`
        /* Toast Notification */
        .toast-notification {
          position: fixed;
          top: 20px;
          right: 20px;
          padding: 16px 24px;
          border-radius: 10px;
          font-weight: 600;
          z-index: 1000;
          animation: slideIn 0.3s ease, fadeOut 0.3s ease 2.7s;
        }

        .toast-notification.success {
          background: linear-gradient(135deg, #00d4aa, #00b894);
          color: #fff;
          box-shadow: 0 4px 20px rgba(0, 212, 170, 0.4);
        }

        .toast-notification.error {
          background: linear-gradient(135deg, #ff6b6b, #ee5a5a);
          color: #fff;
          box-shadow: 0 4px 20px rgba(255, 107, 107, 0.4);
        }

        @keyframes slideIn {
          from { transform: translateX(100%); opacity: 0; }
          to { transform: translateX(0); opacity: 1; }
        }

        @keyframes fadeOut {
          from { opacity: 1; }
          to { opacity: 0; }
        }

        /* Kill Switch Banner */
        .kill-switch-banner {
          background: linear-gradient(135deg, #8B0000, #B22222);
          border: 2px solid #ff4444;
          border-radius: 12px;
          padding: 20px;
          margin-bottom: 20px;
          animation: pulse-red 2s infinite;
        }

        @keyframes pulse-red {
          0%, 100% { box-shadow: 0 0 10px rgba(255, 68, 68, 0.5); }
          50% { box-shadow: 0 0 25px rgba(255, 68, 68, 0.8); }
        }

        .kill-switch-content {
          display: flex;
          align-items: center;
          gap: 20px;
          flex-wrap: wrap;
        }

        .kill-switch-icon {
          font-size: 2.5rem;
        }

        .kill-switch-text {
          flex: 1;
          min-width: 200px;
        }

        .kill-switch-text strong {
          color: #fff;
          font-size: 1.3rem;
          display: block;
          margin-bottom: 5px;
        }

        .kill-switch-text p {
          color: #ffcccc;
          margin: 0;
          font-size: 0.95rem;
        }

        .kill-switch-reset-btn {
          background: #fff;
          color: #8B0000;
          border: none;
          padding: 12px 24px;
          border-radius: 8px;
          font-weight: 700;
          font-size: 1rem;
          cursor: pointer;
          transition: all 0.2s;
        }

        .kill-switch-reset-btn:hover {
          background: #00d4aa;
          color: #fff;
          transform: scale(1.05);
        }

        /* Kill Switch Status Bar */
        .kill-switch-status-bar {
          display: flex;
          align-items: center;
          gap: 20px;
          margin-bottom: 15px;
          flex-wrap: wrap;
        }

        /* Kill Switch Simple Indicator */
        .kill-switch-indicator {
          display: inline-flex;
          align-items: center;
          gap: 8px;
          background: linear-gradient(135deg, #1a2a1a, #1a3a2a);
          border: 1px solid #2d4a2d;
          border-radius: 20px;
          padding: 8px 16px;
        }

        .kill-switch-indicator .ks-icon {
          font-size: 1.1rem;
        }

        .kill-switch-indicator .ks-label {
          color: #00d4aa;
          font-weight: 600;
          font-size: 0.9rem;
        }

        .ks-toggle-buttons {
          display: flex;
          gap: 4px;
          margin-left: 8px;
        }

        .ks-toggle-btn {
          padding: 4px 12px;
          border-radius: 4px;
          border: 1px solid #3a3a5a;
          background: #252542;
          color: #888;
          cursor: pointer;
          font-size: 0.8rem;
          font-weight: 600;
          transition: all 0.2s;
        }

        .ks-toggle-btn:hover {
          border-color: #00d4aa;
        }

        .ks-toggle-btn.active {
          background: rgba(0, 212, 170, 0.2);
          border-color: #00d4aa;
          color: #00d4aa;
        }

        .ks-toggle-btn.off.active {
          background: rgba(255, 107, 107, 0.2);
          border-color: #ff6b6b;
          color: #ff6b6b;
        }

        /* Kill Switch Details */
        .kill-switch-details {
          display: flex;
          align-items: center;
          gap: 20px;
        }

        .kill-switch-details .ks-stat {
          color: #888;
          font-size: 0.9rem;
        }

        .kill-switch-disabled-warning {
          color: #f0ad4e;
          font-size: 0.9rem;
          background: rgba(240, 173, 78, 0.1);
          padding: 6px 12px;
          border-radius: 6px;
          border: 1px solid rgba(240, 173, 78, 0.3);
        }

        /* Peak Balance Card */
        .stat-card.peak {
          background: linear-gradient(135deg, #2a2a4a, #1a1a3a);
          border: 1px solid #ffd700;
        }

        .stat-card.peak .stat-value {
          color: #ffd700;
        }

        /* Starting Balance Dropdown */
        .starting-balance-select {
          background: #1a1a2e;
          border: 1px solid #3a3a5a;
          border-radius: 8px;
          color: #fff;
          padding: 10px 16px;
          font-size: 1.3rem;
          font-weight: 700;
          cursor: pointer;
          width: 100%;
          text-align: center;
          margin-top: 8px;
        }

        .starting-balance-select:hover {
          border-color: #00d4aa;
        }

        .starting-balance-select:focus {
          outline: none;
          border-color: #00d4aa;
        }

        /* Performance - Single Row */
        .stats-grid-single-row {
          display: grid;
          grid-template-columns: repeat(5, 1fr);
          gap: 15px;
        }

        @media (max-width: 900px) {
          .stats-grid-single-row {
            grid-template-columns: repeat(3, 1fr);
          }
        }

        @media (max-width: 600px) {
          .stats-grid-single-row {
            grid-template-columns: repeat(2, 1fr);
          }
        }

        .positive-text {
          color: #00d4aa !important;
        }

        .negative-text {
          color: #ff6b6b !important;
        }

        /* Trade Controls */
        .trade-controls-section {
          background: #1a1a2e;
          border-radius: 12px;
          padding: 20px;
          margin-bottom: 20px;
        }

        .trade-controls-header {
          display: flex;
          justify-content: space-between;
          align-items: center;
          margin-bottom: 20px;
        }

        .trade-controls-header h3 {
          color: #00d4aa;
          margin: 0;
          font-size: 1.1rem;
        }

        .trade-controls-section h3 {
          color: #00d4aa;
          margin-bottom: 20px;
          font-size: 1.1rem;
        }

        /* Base Currency Section */
        .base-currency-section {
          background: #1a1a2e;
          border-radius: 10px;
          padding: 15px;
          margin-bottom: 20px;
          border: 1px solid #3a3a5a;
        }

        .base-currency-row {
          display: flex;
          align-items: center;
          gap: 15px;
          flex-wrap: wrap;
        }

        .base-currency-row .control-label {
          color: #00d4aa;
          font-weight: 600;
          min-width: 120px;
        }

        .base-currency-select {
          min-width: 180px;
        }

        .selected-currencies {
          display: flex;
          align-items: center;
          gap: 8px;
          flex-wrap: wrap;
        }

        .selected-label {
          color: #888;
          font-size: 0.85rem;
        }

        .selected-currency-tag {
          background: rgba(0, 212, 170, 0.2);
          color: #00d4aa;
          padding: 4px 10px;
          border-radius: 12px;
          font-size: 0.85rem;
          font-weight: 600;
        }

        .custom-currencies-panel {
          margin-top: 15px;
          padding-top: 15px;
          border-top: 1px solid #3a3a5a;
        }

        .custom-label {
          color: #bbb;
          margin: 0 0 12px 0;
          font-size: 0.9rem;
        }

        .currency-checkboxes {
          display: flex;
          gap: 15px;
          flex-wrap: wrap;
          margin-bottom: 15px;
        }

        .currency-checkbox {
          display: flex;
          align-items: center;
          gap: 8px;
          background: #252542;
          padding: 10px 15px;
          border-radius: 8px;
          cursor: pointer;
          transition: all 0.2s;
          border: 2px solid #3a3a5a;
          user-select: none;
        }

        .currency-checkbox:hover {
          border-color: #00d4aa;
          background: #2a2a4a;
        }

        .currency-checkbox.checked {
          border-color: #00d4aa;
          background: rgba(0, 212, 170, 0.15);
        }

        .currency-checkbox input[type="checkbox"] {
          width: 18px;
          height: 18px;
          accent-color: #00d4aa;
          cursor: pointer;
        }

        .currency-checkbox span {
          color: #fff;
          font-weight: 600;
          font-size: 0.95rem;
        }

        .custom-actions {
          display: flex;
          align-items: center;
          gap: 15px;
          flex-wrap: wrap;
        }

        .apply-custom-btn {
          background: linear-gradient(135deg, #00d4aa, #00b894);
          border: none;
          color: #1a1a2e;
          padding: 10px 20px;
          border-radius: 8px;
          font-weight: 600;
          cursor: pointer;
          transition: all 0.2s;
        }

        .apply-custom-btn:hover:not(:disabled) {
          transform: translateY(-1px);
          box-shadow: 0 4px 12px rgba(0, 212, 170, 0.3);
        }

        .apply-custom-btn:disabled {
          background: #555;
          color: #999;
          cursor: not-allowed;
        }

        .pending-selection {
          color: #ffc107;
          font-size: 0.9rem;
        }

        .pending-badge {
          color: #f0ad4e;
          font-size: 0.85rem;
          background: rgba(240, 173, 78, 0.1);
          padding: 4px 10px;
          border-radius: 12px;
          border: 1px solid #f0ad4e;
        }

        .trade-controls-actions {
          margin-top: 20px;
          display: flex;
          justify-content: flex-end;
        }

        .apply-btn {
          background: linear-gradient(135deg, #00d4aa, #00b894);
          border: none;
          color: #1a1a2e;
          padding: 12px 24px;
          border-radius: 8px;
          font-weight: 600;
          font-size: 0.95rem;
          cursor: pointer;
          transition: all 0.2s;
        }

        .apply-btn:hover {
          background: linear-gradient(135deg, #00e4ba, #00c8a4);
          transform: translateY(-1px);
        }

        .trade-controls-grid {
          display: grid;
          grid-template-columns: repeat(3, 1fr);
          gap: 20px;
        }

        @media (max-width: 992px) {
          .trade-controls-grid {
            grid-template-columns: repeat(2, 1fr);
          }
        }

        @media (max-width: 768px) {
          .trade-controls-grid {
            grid-template-columns: 1fr;
          }
        }

        .control-card {
          background: #252542;
          border-radius: 10px;
          padding: 16px;
          display: flex;
          flex-direction: column;
          gap: 8px;
        }

        .control-label {
          color: #fff;
          font-weight: 600;
          font-size: 0.95rem;
        }

        .control-select {
          background: #1a1a2e;
          border: 1px solid #3a3a5a;
          border-radius: 8px;
          color: #fff;
          padding: 12px 16px;
          font-size: 1rem;
          cursor: pointer;
          transition: border-color 0.2s, box-shadow 0.2s;
        }

        .control-select:hover {
          border-color: #00d4aa;
        }

        .control-select:focus {
          outline: none;
          border-color: #00d4aa;
          box-shadow: 0 0 0 2px rgba(0, 212, 170, 0.2);
        }

        .control-hint {
          color: #888;
          font-size: 0.8rem;
          display: flex;
          align-items: center;
          gap: 4px;
        }

        /* Wallet Section */
        .wallet-section {
          background: #1a1a2e;
          border-radius: 12px;
          padding: 20px;
          margin-bottom: 20px;
        }

        .wallet-header {
          display: flex;
          justify-content: space-between;
          align-items: center;
          margin-bottom: 20px;
        }

        .wallet-controls {
          display: flex;
          gap: 10px;
          align-items: center;
          flex-wrap: wrap;
        }

        .scanner-off-hint {
          color: #f0ad4e;
          font-size: 0.85rem;
          margin-left: 5px;
        }

        .toggle-btn {
          padding: 10px 20px;
          border-radius: 20px;
          border: none;
          cursor: pointer;
          font-weight: 600;
          transition: all 0.2s;
        }

        .toggle-btn.active {
          background: linear-gradient(135deg, #1a2a1a, #1a3a2a);
          border: 1px solid #2d4a2d;
          color: #00d4aa;
        }

        .toggle-btn.inactive {
          background: linear-gradient(135deg, #2a1a1a, #3a1a1a);
          border: 1px solid #4a2d2d;
          color: #ff6b6b;
        }

        .toggle-btn:disabled {
          background: #3a3a3a;
          border: 1px solid #555;
          color: #888;
          cursor: not-allowed;
          opacity: 0.9;
        }

        .reset-btn {
          padding: 10px 20px;
          border-radius: 8px;
          border: 1px solid #3a3a5a;
          background: transparent;
          color: #fff;
          cursor: pointer;
          transition: all 0.2s;
        }

        .reset-btn:hover {
          border-color: #00d4aa;
          color: #00d4aa;
        }

        .wallet-stats {
          display: grid;
          grid-template-columns: repeat(4, 1fr);
          gap: 15px;
        }

        @media (max-width: 900px) {
          .wallet-stats {
            grid-template-columns: repeat(2, 1fr);
          }
        }

        @media (max-width: 500px) {
          .wallet-stats {
            grid-template-columns: 1fr;
          }
        }

        .stat-card {
          background: #252542;
          border-radius: 10px;
          padding: 20px;
          text-align: center;
        }

        .stat-card.highlight {
          background: linear-gradient(135deg, #1a3a4a, #1a2a3a);
          border: 1px solid #00d4aa;
        }

        .stat-card.positive {
          border-left: 3px solid #00d4aa;
        }

        .stat-card.negative {
          border-left: 3px solid #ff6b6b;
        }

        .stat-label {
          display: block;
          color: #bbb;
          font-size: 0.9rem;
          margin-bottom: 8px;
        }

        .stat-value {
          display: block;
          color: #fff;
          font-size: 1.4rem;
          font-weight: 700;
        }

        .positive .stat-value {
          color: #00d4aa;
        }

        .negative .stat-value {
          color: #ff6b6b;
        }

        .stats-section, .trades-section {
          background: #1a1a2e;
          border-radius: 12px;
          padding: 20px;
          margin-bottom: 20px;
        }

        .stats-section h3, .trades-section h3 {
          color: #00d4aa;
          margin-bottom: 15px;
        }

        .stat-item {
          background: #252542;
          border-radius: 8px;
          padding: 15px;
          text-align: center;
        }

        .trades-table-container {
          overflow-x: auto;
        }

        .trades-footer-note {
          text-align: center;
          color: #666;
          font-size: 0.85rem;
          padding: 12px;
          border-top: 1px solid #2a2a4a;
          margin-top: 10px;
        }

        .trades-table {
          width: 100%;
          border-collapse: collapse;
        }

        .trades-table th {
          background: #00d4aa;
          color: #1a1a2e;
          padding: 12px 8px;
          text-align: left;
          font-weight: 600;
        }

        .trades-table td {
          padding: 12px 8px;
          border-bottom: 1px solid #2a2a4a;
        }

        .trades-table tr.win {
          background: rgba(0, 212, 170, 0.05);
        }

        .trades-table tr.loss {
          background: rgba(255, 107, 107, 0.05);
        }

        .trades-table code {
          background: #2a2a4a;
          padding: 4px 8px;
          border-radius: 4px;
          font-size: 0.85rem;
        }

        .badge {
          padding: 4px 12px;
          border-radius: 20px;
          font-size: 0.8rem;
          font-weight: 600;
        }

        .badge.win {
          background: rgba(0, 212, 170, 0.2);
          color: #00d4aa;
        }

        .badge.loss {
          background: rgba(255, 107, 107, 0.2);
          color: #ff6b6b;
        }

        .empty-state {
          text-align: center;
          padding: 40px;
          color: #888;
        }

        .empty-state .hint {
          font-size: 0.9rem;
          margin-top: 10px;
        }

        .panel-footer {
          text-align: center;
          color: #666;
          font-size: 0.85rem;
          padding: 15px;
        }

        .error-message {
          background: rgba(255, 107, 107, 0.1);
          border: 1px solid #ff6b6b;
          color: #ff6b6b;
          padding: 12px 20px;
          border-radius: 8px;
          margin-bottom: 20px;
          display: flex;
          justify-content: space-between;
          align-items: center;
        }

        .error-message button {
          background: none;
          border: none;
          color: #ff6b6b;
          font-size: 1.2rem;
          cursor: pointer;
        }

        @media (max-width: 768px) {
          .wallet-stats {
            grid-template-columns: 1fr;
          }
        }
      `}</style>
    </div>
  );
}

export default PaperTradingPanel;