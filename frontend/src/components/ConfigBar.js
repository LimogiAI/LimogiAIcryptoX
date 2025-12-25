import React, { useState, useEffect, useCallback } from 'react';
import { api } from '../services/api';
import './ConfigBar.css';

export function ConfigBar() {
  const [config, setConfig] = useState(null);
  const [status, setStatus] = useState(null);
  const [pendingConfig, setPendingConfig] = useState({});
  const [krakenFees, setKrakenFees] = useState(null);
  const [message, setMessage] = useState(null);
  const [loading, setLoading] = useState(false);
  const [tradingLoading, setTradingLoading] = useState(false);
  const [customAmount, setCustomAmount] = useState('');
  const [customCurrencies, setCustomCurrencies] = useState([]);
  const [initialLoadDone, setInitialLoadDone] = useState(false);
  const [showEnableModal, setShowEnableModal] = useState(false);

  const PRESET_AMOUNTS = [5, 10, 20, 50, 100];
  const AVAILABLE_CURRENCIES = ['USD', 'EUR', 'USDT', 'BTC', 'ETH'];

  const fetchData = useCallback(async () => {
    try {
      const [configRes, statusRes, feesRes] = await Promise.all([
        api.getLiveConfig(),
        api.getLiveStatus().catch(() => null),
        api.getKrakenFees().catch(() => null)
      ]);
      setConfig(configRes.config);
      if (statusRes) setStatus(statusRes);
      if (feesRes) setKrakenFees(feesRes);
      
      // Only load custom currencies from backend on initial load
      if (!initialLoadDone) {
        if (configRes.config?.base_currency === 'CUSTOM' && configRes.config?.custom_currencies) {
          setCustomCurrencies(configRes.config.custom_currencies);
        }
        setInitialLoadDone(true);
      }
    } catch (err) {
      console.error('Failed to fetch config:', err);
    }
  }, [initialLoadDone]);

  useEffect(() => {
    fetchData();
    const interval = setInterval(fetchData, 5000);
    return () => clearInterval(interval);
  }, [fetchData]);

  const handleConfigChange = (key, value) => {
    setPendingConfig(prev => ({ ...prev, [key]: value }));
  };

  const handleApplyConfig = async () => {
    if (Object.keys(pendingConfig).length === 0) return;
    setLoading(true);
    try {
      const configToSave = { ...pendingConfig };
      if (configToSave.base_currency === 'CUSTOM' || 
          (!configToSave.base_currency && config?.base_currency === 'CUSTOM')) {
        configToSave.custom_currencies = customCurrencies;
      }
      
      await api.updateLiveConfig(configToSave);
      setMessage({ type: 'success', text: '✓ Config saved' });
      setPendingConfig({});
      setConfig(prev => ({ ...prev, ...configToSave }));
      setTimeout(() => fetchData(), 500);
    } catch (err) {
      setMessage({ type: 'error', text: err.response?.data?.detail || 'Failed to save' });
    } finally {
      setLoading(false);
      setTimeout(() => setMessage(null), 3000);
    }
  };

  const handleStartTrading = () => {
    setShowEnableModal(true);
  };

  const handleConfirmEnable = async () => {
    setTradingLoading(true);
    try {
      await api.enableLiveTrading(true, 'I understand the risks');
      setMessage({ type: 'success', text: '🟢 Live trading enabled!' });
      setShowEnableModal(false);
      fetchData();
    } catch (err) {
      setMessage({ type: 'error', text: err.response?.data?.detail || 'Failed to enable trading' });
    } finally {
      setTradingLoading(false);
      setTimeout(() => setMessage(null), 3000);
    }
  };

  const handleStopTrading = async () => {
    setTradingLoading(true);
    try {
      await api.disableLiveTrading('Manual stop from ConfigBar');
      setMessage({ type: 'success', text: '🔴 Live trading stopped' });
      fetchData();
    } catch (err) {
      setMessage({ type: 'error', text: err.response?.data?.detail || 'Failed to stop trading' });
    } finally {
      setTradingLoading(false);
      setTimeout(() => setMessage(null), 3000);
    }
  };

  const handleResetCircuitBreaker = async () => {
    try {
      await api.resetLiveCircuitBreaker();
      setMessage({ type: 'success', text: '✓ Circuit breaker reset' });
      fetchData();
    } catch (err) {
      setMessage({ type: 'error', text: 'Failed to reset circuit breaker' });
    }
    setTimeout(() => setMessage(null), 3000);
  };

  const handleCustomAmountSubmit = () => {
    const amount = parseFloat(customAmount);
    if (!isNaN(amount) && amount > 0) {
      handleConfigChange('trade_amount', amount);
      setCustomAmount('');
    }
  };

  const handleBaseCurrencyChange = (value) => {
    handleConfigChange('base_currency', value);
    if (value !== 'CUSTOM') {
      setCustomCurrencies([]);
    }
  };

  const handleCustomCurrencyToggle = (currency) => {
    setCustomCurrencies(prev => {
      const updated = prev.includes(currency)
        ? prev.filter(c => c !== currency)
        : [...prev, currency];
      handleConfigChange('custom_currencies', updated);
      return updated;
    });
  };

  const getConfigValue = (key, defaultValue) => {
    if (pendingConfig[key] !== undefined) return pendingConfig[key];
    if (config?.[key] !== undefined) return config[key];
    return defaultValue;
  };

  const currentTradeAmount = getConfigValue('trade_amount', 5);
  const currentThreshold = getConfigValue('min_profit_threshold', 0.001);
  const currentBaseCurrency = getConfigValue('base_currency', 'USD');
  const hasPendingChanges = Object.keys(pendingConfig).length > 0;

  // Status checks
  const isEnabled = status?.enabled || config?.is_enabled;
  const isCircuitBroken = status?.state?.is_circuit_broken;

  // Fee calculations
  const makerFee = krakenFees?.maker_fee ? (krakenFees.maker_fee * 100).toFixed(2) : '0.16';
  const takerFee = krakenFees?.taker_fee ? (krakenFees.taker_fee * 100).toFixed(2) : '0.26';
  const totalFees = krakenFees?.taker_fee ? krakenFees.taker_fee * 100 * 3 : 0.78;
  const thresholdPct = currentThreshold * 100;
  
  let thresholdStatus = null;
  if (thresholdPct < totalFees) {
    thresholdStatus = { type: 'danger', text: `⚠️ Below fees (${totalFees.toFixed(2)}%)` };
  } else if (thresholdPct < totalFees * 1.5) {
    thresholdStatus = { type: 'warning', text: `Low margin` };
  }

  const getBaseCurrencyDisplay = () => {
    const base = config?.base_currency || 'USD';
    if (base === 'CUSTOM' && config?.custom_currencies?.length > 0) {
      return config.custom_currencies.join(', ');
    }
    if (base === 'ALL') return 'ALL';
    return base;
  };

  return (
    <div className="config-bar">
      {/* Active Config Summary */}
      <div className="active-config-summary">
        <div className="summary-item">
          <span className="summary-icon">💰</span>
          <span className="summary-label">Amount:</span>
          <span className="summary-value">${config?.trade_amount || 5}</span>
        </div>
        <div className="summary-divider">|</div>
        <div className="summary-item">
          <span className="summary-icon">📊</span>
          <span className="summary-label">Min Profit:</span>
          <span className="summary-value">{((config?.min_profit_threshold || 0.001) * 100).toFixed(2)}%</span>
        </div>
        <div className="summary-divider">|</div>
        <div className="summary-item">
          <span className="summary-icon">💵</span>
          <span className="summary-label">Base:</span>
          <span className="summary-value">{getBaseCurrencyDisplay()}</span>
        </div>
        <div className="summary-divider">|</div>
        <div className="summary-item">
          <span className="summary-icon">🛡️</span>
          <span className="summary-label">Limits:</span>
          <span className="summary-value">${config?.max_daily_loss || 30} / ${config?.max_total_loss || 30}</span>
        </div>
        <div className="summary-divider">|</div>
        <div className="summary-item">
          <span className="summary-icon">💸</span>
          <span className="summary-label">Fees:</span>
          <span className="summary-value fee-value">{makerFee}% / {takerFee}%</span>
          <span className="summary-note">(maker/taker)</span>
        </div>
      </div>

      {/* Edit Config Controls */}
      <div className="config-bar-content">
        {/* Trade Amount */}
        <div className="config-bar-item">
          <span className="config-bar-label">Amount</span>
          <div className="amount-controls">
            <div className="amount-btn-group">
              {PRESET_AMOUNTS.map(amt => (
                <button
                  key={amt}
                  className={`amount-btn ${currentTradeAmount === amt ? 'active' : ''}`}
                  onClick={() => handleConfigChange('trade_amount', amt)}
                >
                  ${amt}
                </button>
              ))}
              {!PRESET_AMOUNTS.includes(currentTradeAmount) && currentTradeAmount > 0 && (
                <button className="amount-btn active">${currentTradeAmount}</button>
              )}
            </div>
            <div className="custom-amount-input">
              <input
                type="number"
                placeholder="Custom"
                value={customAmount}
                onChange={(e) => setCustomAmount(e.target.value)}
                onKeyPress={(e) => e.key === 'Enter' && handleCustomAmountSubmit()}
              />
              <button onClick={handleCustomAmountSubmit} className="set-btn">Set</button>
            </div>
          </div>
        </div>

        {/* Profit Threshold */}
        <div className="config-bar-item threshold-item">
          <span className="config-bar-label">Min Profit</span>
          <div className="threshold-control">
            <input
              type="range"
              min="0"
              max="1"
              step="0.01"
              value={currentThreshold * 100}
              onChange={(e) => handleConfigChange('min_profit_threshold', parseFloat(e.target.value) / 100)}
              className="threshold-slider"
            />
            <span className="threshold-value">{(currentThreshold * 100).toFixed(2)}%</span>
          </div>
          {thresholdStatus && (
            <span className={`threshold-status ${thresholdStatus.type}`}>{thresholdStatus.text}</span>
          )}
        </div>

        {/* Loss Limits */}
        <div className="config-bar-item">
          <span className="config-bar-label">Daily / Total Limit</span>
          <div className="loss-inputs">
            <span className="loss-prefix">$</span>
            <input
              type="number"
              min="10"
              max="500"
              value={getConfigValue('max_daily_loss', 30)}
              onChange={(e) => handleConfigChange('max_daily_loss', parseFloat(e.target.value))}
              className="loss-input"
            />
            <span className="loss-separator">/</span>
            <span className="loss-prefix">$</span>
            <input
              type="number"
              min="10"
              max="500"
              value={getConfigValue('max_total_loss', 30)}
              onChange={(e) => handleConfigChange('max_total_loss', parseFloat(e.target.value))}
              className="loss-input"
            />
          </div>
        </div>

        {/* Base Currency */}
        <div className="config-bar-item base-currency-item">
          <span className="config-bar-label">Base Currency</span>
          <select
            value={currentBaseCurrency}
            onChange={(e) => handleBaseCurrencyChange(e.target.value)}
            className="base-select"
          >
            <option value="ALL">ALL</option>
            <option value="USD">USD</option>
            <option value="EUR">EUR</option>
            <option value="USDT">USDT</option>
            <option value="BTC">BTC</option>
            <option value="ETH">ETH</option>
            <option value="CUSTOM">CUSTOM</option>
          </select>
          
          {currentBaseCurrency === 'CUSTOM' && (
            <div className="custom-currencies">
              {AVAILABLE_CURRENCIES.map(curr => (
                <label key={curr} className={`currency-checkbox ${customCurrencies.includes(curr) ? 'checked' : ''}`}>
                  <input
                    type="checkbox"
                    checked={customCurrencies.includes(curr)}
                    onChange={() => handleCustomCurrencyToggle(curr)}
                  />
                  {curr}
                </label>
              ))}
            </div>
          )}
        </div>

        {/* Circuit Breaker Status */}
        <div className="config-bar-item circuit-item">
          <span className="config-bar-label">Circuit Breaker</span>
          <div className={`circuit-status ${isCircuitBroken ? 'broken' : 'ok'}`}>
            {isCircuitBroken ? (
              <>
                <span className="circuit-icon">🔴</span>
                <span>BROKEN</span>
                <button className="reset-btn" onClick={handleResetCircuitBreaker}>Reset</button>
              </>
            ) : (
              <>
                <span className="circuit-icon">✅</span>
                <span>OK</span>
              </>
            )}
          </div>
        </div>

        {/* Apply Button */}
        {hasPendingChanges && (
          <button
            className="apply-btn"
            onClick={handleApplyConfig}
            disabled={loading}
          >
            {loading ? '...' : '✓ Apply'}
          </button>
        )}

        {/* Trading Control Button */}
        <div className="trading-control">
          {isEnabled ? (
            <button
              className="stop-trading-btn"
              onClick={handleStopTrading}
              disabled={tradingLoading}
            >
              {tradingLoading ? '...' : '⬛ Stop Trading'}
            </button>
          ) : (
            <button
              className="start-trading-btn"
              onClick={handleStartTrading}
              disabled={tradingLoading || isCircuitBroken}
            >
              {tradingLoading ? '...' : '🟢 START Trading'}
            </button>
          )}
        </div>

        {/* Status Message */}
        {message && (
          <span className={`config-message ${message.type}`}>{message.text}</span>
        )}
      </div>

      {/* Enable Trading Modal */}
      {showEnableModal && (
        <div className="modal-overlay" onClick={() => setShowEnableModal(false)}>
          <div className="enable-modal" onClick={e => e.stopPropagation()}>
            <h3>⚠️ Enable Live Trading</h3>
            <div className="modal-content">
              <p>You are about to enable <strong>REAL</strong> trading with <strong>REAL</strong> money.</p>
              <ul>
                <li>Trade Amount: <strong>${config?.trade_amount || 10}</strong></li>
                <li>Min Profit Threshold: <strong>{((config?.min_profit_threshold || 0.003) * 100).toFixed(2)}%</strong></li>
                <li>Daily Loss Limit: <strong>${config?.max_daily_loss || 30}</strong></li>
                <li>Total Loss Limit: <strong>${config?.max_total_loss || 30}</strong></li>
              </ul>
              <p className="warning-text">This will execute real trades on Kraken. Losses are possible.</p>
            </div>
            <div className="modal-actions">
              <button className="cancel-btn" onClick={() => setShowEnableModal(false)}>Cancel</button>
              <button className="confirm-btn" onClick={handleConfirmEnable} disabled={tradingLoading}>
                {tradingLoading ? 'Enabling...' : 'I Understand - Enable Trading'}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
