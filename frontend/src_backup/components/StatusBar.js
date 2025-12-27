import React, { useState, useEffect, useCallback } from 'react';
import { api } from '../services/api';

export function StatusBar({ status, connected }) {
  const [uptimeSeconds, setUptimeSeconds] = useState(0);
  const [engineSettings, setEngineSettings] = useState(null);
  const [showSettings, setShowSettings] = useState(false);
  const [pendingChanges, setPendingChanges] = useState({});
  const [restarting, setRestarting] = useState(false);
  const [message, setMessage] = useState(null);
  const [cooldownSeconds, setCooldownSeconds] = useState(0);

  // Rust execution engine stats
  const [executionStats, setExecutionStats] = useState(null);
  const [scannerStatus, setScannerStatus] = useState(null);
  const [lastScanSeconds, setLastScanSeconds] = useState(null);

  const COOLDOWN_DURATION = 30;

  // Reset state when disconnected
  useEffect(() => {
    if (!connected) {
      setExecutionStats(null);
      setScannerStatus(null);
      setLastScanSeconds(null);
      setUptimeSeconds(0);
    }
  }, [connected]);

  useEffect(() => {
    if (status?.uptime_seconds && connected) {
      setUptimeSeconds(status.uptime_seconds);
    }
  }, [status?.uptime_seconds, connected]);

  useEffect(() => {
    // Only count uptime when connected
    if (!connected) return;
    
    const interval = setInterval(() => {
      setUptimeSeconds(prev => prev + 1);
    }, 1000);
    return () => clearInterval(interval);
  }, [connected]);

  useEffect(() => {
    if (cooldownSeconds > 0) {
      const timer = setTimeout(() => {
        setCooldownSeconds(prev => prev - 1);
      }, 1000);
      return () => clearTimeout(timer);
    }
  }, [cooldownSeconds]);

  const fetchSettings = useCallback(async () => {
    if (!connected) return;
    try {
      const data = await api.getEngineSettings();
      setEngineSettings(data);
    } catch (err) {
      console.error('Failed to fetch engine settings:', err);
    }
  }, [connected]);

  const fetchExecutionStats = useCallback(async () => {
    if (!connected) return;
    try {
      const [statusData, scannerData] = await Promise.all([
        api.getStatus().catch(() => null),
        api.getLiveScannerStatus().catch(() => null),
      ]);
      
      // Set execution stats from status - connected if engine is running
      if (statusData) {
        setExecutionStats({
          connected: statusData.is_running || false,
          stats: {
            trades_executed: 0,
            avg_execution_ms: null,
          }
        });
      } else {
        setExecutionStats(null);
      }
      
      // Set scanner status and calculate seconds ago
      if (scannerData?.data) {
        setScannerStatus(scannerData.data);
        if (scannerData.data.last_scan_at) {
          const lastScanTime = new Date(scannerData.data.last_scan_at);
          const secondsAgo = Math.floor((Date.now() - lastScanTime.getTime()) / 1000);
          setLastScanSeconds(secondsAgo >= 0 ? secondsAgo : null);
        } else {
          setLastScanSeconds(null);
        }
      } else {
        setScannerStatus(null);
        setLastScanSeconds(null);
      }
    } catch (err) {
      // On error, clear the stats
      setExecutionStats(null);
      setScannerStatus(null);
      setLastScanSeconds(null);
    }
  }, [connected]);

  useEffect(() => {
    if (connected) {
      fetchSettings();
      fetchExecutionStats();
    }
    // Refresh execution stats every 10 seconds
    const interval = setInterval(fetchExecutionStats, 10000);
    return () => clearInterval(interval);
  }, [fetchSettings, fetchExecutionStats, connected]);

  if (!status) return null;

  const formatUptime = (seconds) => {
    const hours = Math.floor(seconds / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    const secs = seconds % 60;
    return hours + 'h ' + minutes + 'm ' + secs + 's';
  };

  const formatScanInterval = (ms) => {
    if (!ms) return '';
    if (ms >= 1000) return (ms / 1000) + 's';
    return ms + 'ms';
  };

  const handleSettingChange = (key, value) => {
    const parsedValue = key === 'scanner_enabled' ? value === 'true' : parseInt(value);
    setPendingChanges(prev => ({ ...prev, [key]: parsedValue }));
  };

  // Scanner ON/OFF applies immediately (no restart needed)
  const handleScannerToggle = async (enabled) => {
    try {
      setMessage({ type: 'info', text: enabled ? 'Enabling scanner...' : 'Disabling scanner...' });
      await api.updateEngineSettings({ scanner_enabled: enabled });
      setMessage({ type: 'success', text: enabled ? '🟢 Scanner enabled' : '🔴 Scanner disabled' });
      // Remove from pending if it was there
      setPendingChanges(prev => {
        const { scanner_enabled, ...rest } = prev;
        return rest;
      });
      // Refresh settings
      setTimeout(() => fetchSettings(), 500);
    } catch (err) {
      setMessage({ type: 'error', text: err.response?.data?.detail || 'Failed to toggle scanner' });
    }
  };

  const handleRestartEngine = async () => {
    if (cooldownSeconds > 0) {
      setMessage({ type: 'warning', text: 'Please wait ' + cooldownSeconds + ' seconds before restarting again.' });
      return;
    }

    setRestarting(true);
    setMessage({ type: 'info', text: 'Restarting engine...' });

    try {
      if (Object.keys(pendingChanges).length > 0) {
        await api.updateEngineSettings(pendingChanges);
      }
      const result = await api.restartEngine();
      setMessage({ type: 'success', text: result.message });
      setPendingChanges({});
      setCooldownSeconds(COOLDOWN_DURATION);
      setTimeout(() => fetchSettings(), 2000);
    } catch (err) {
      setMessage({ type: 'error', text: err.response?.data?.detail || 'Failed to restart engine' });
    } finally {
      setRestarting(false);
    }
  };

  const getCurrentValue = (key) => {
    if (pendingChanges[key] !== undefined) return pendingChanges[key];
    if (engineSettings) return engineSettings[key];
    if (key === 'scan_interval_ms') return status?.scan_interval_ms;
    if (key === 'max_pairs') return status?.max_pairs;
    if (key === 'orderbook_depth') return status?.orderbook_depth;
    if (key === 'scanner_enabled') return true;
    return null;
  };

  const hasPendingChanges = Object.keys(pendingChanges).length > 0;
  const scannerEnabled = getCurrentValue('scanner_enabled') ?? true;
  const isOnline = connected && status?.is_running;

  return (
    <div className="status-bar-container">
      <div className="status-bar">
        <div className="status-item">
          <span className="status-label">Scanner</span>
          <span className={'status-value ' + (isOnline && scannerEnabled ? 'running' : 'stopped')}>
            {!connected ? '○ Offline' : (isOnline && scannerEnabled ? '● Active' : '○ Stopped')}
          </span>
          <span className="status-note">(prices)</span>
        </div>
        
        <div className="status-item">
          <span className="status-label">Pairs</span>
          <span className="status-value">{connected ? (status?.pairs_monitored || 0) : '--'}</span>
          <span className="status-note">(top volume)</span>
        </div>

        <div className="status-item">
          <span className="status-label">Depth</span>
          <span className="status-value">{connected ? (getCurrentValue('orderbook_depth') || 25) : '--'}</span>
          <span className="status-note">(levels)</span>
        </div>

        <div className="status-item">
          <span className="status-label">Last Scan</span>
          <span className="status-value">
            {connected && lastScanSeconds != null ? `${lastScanSeconds}s ago` : '--'}
          </span>
        </div>

        {/* Rust Execution Engine Status (Private WebSocket for order execution) */}
        <div className="status-divider"></div>
        <div className="status-item">
          <span className="status-label">Executor</span>
          <span className={'status-value ' + (connected && executionStats?.connected ? 'running' : 'stopped')}>
            {!connected ? '○ Offline' : (executionStats?.connected ? '● Ready' : '○ Offline')}
          </span>
          <span className="status-note">(orders)</span>
        </div>

        <div className="status-item">
          <span className="status-label">Uptime</span>
          <span className="status-value uptime">{connected ? formatUptime(uptimeSeconds) : '--'}</span>
        </div>

        <button
          className={'settings-toggle-btn ' + (showSettings ? 'active' : '')}
          onClick={() => setShowSettings(!showSettings)}
          title="Engine Settings"
        >
          ⚙️
        </button>
      </div>

      {showSettings && connected && (
        <div className="engine-settings-panel">
          <div className="settings-header">
            <h4>⚙️ Engine Settings</h4>
            <span className="settings-note">
              {hasPendingChanges ? '⚠️ Unsaved changes' : '✓ Settings synced'}
            </span>
          </div>

          {message && (
            <div className={'settings-message ' + message.type}>
              {message.text}
              <button className="close-message" onClick={() => setMessage(null)}>×</button>
            </div>
          )}

          <div className="scanner-toggle-section">
            <span className="toggle-label">Scanner</span>
            <div className="toggle-buttons">
              <button 
                className={'toggle-btn ' + (scannerEnabled ? 'active' : '')}
                onClick={() => handleScannerToggle(true)}
              >
                🟢 ON
              </button>
              <button 
                className={'toggle-btn ' + (!scannerEnabled ? 'active off' : '')}
                onClick={() => handleScannerToggle(false)}
              >
                🔴 OFF
              </button>
            </div>
            {!scannerEnabled && (
              <span className="toggle-warning">⚠️ Scanner disabled - no trading will occur</span>
            )}
          </div>

          <div className="settings-grid">
            {/* Scan Interval removed - Rust uses event-driven scanning (triggers on every order book update) */}

            <div className="setting-item">
              <label>Max Pairs</label>
              <select 
                value={getCurrentValue('max_pairs') || 300}
                onChange={(e) => handleSettingChange('max_pairs', e.target.value)}
              >
                <option value={100}>100 pairs</option>
                <option value={200}>200 pairs</option>
                <option value={300}>300 pairs</option>
                <option value={400}>400 pairs</option>
              </select>
            </div>

            <div className="setting-item">
              <label>Order Book Depth</label>
              <select 
                value={getCurrentValue('orderbook_depth') || 25}
                onChange={(e) => handleSettingChange('orderbook_depth', e.target.value)}
              >
                <option value={10}>10 levels</option>
                <option value={25}>25 levels</option>
                <option value={100}>100 levels</option>
                <option value={500}>500 levels</option>
                <option value={1000}>1000 levels</option>
              </select>
            </div>
          </div>

          {(pendingChanges.max_pairs !== undefined || pendingChanges.orderbook_depth !== undefined) && (
            <div className="reconnect-warning">
              ⚠️ Changing Max Pairs or Depth will pause trading for ~10 seconds while reconnecting to Kraken.
            </div>
          )}

          <div className="settings-actions">
            {hasPendingChanges && (
              <span className="pending-indicator">
                {Object.keys(pendingChanges).length} change(s) pending
              </span>
            )}
            
            <button 
              className="restart-btn"
              onClick={handleRestartEngine}
              disabled={restarting || cooldownSeconds > 0}
              title={cooldownSeconds > 0 ? 'Wait ' + cooldownSeconds + 's' : 'Apply changes and restart engine'}
            >
              {restarting ? '⏳ Restarting...' : cooldownSeconds > 0 ? '⏱️ Wait ' + cooldownSeconds + 's' : '🔄 Restart Engine'}
            </button>
          </div>

          {cooldownSeconds > 0 && (
            <div className="cooldown-note">
              ℹ️ Can restart again in {cooldownSeconds} seconds
            </div>
          )}
        </div>
      )}

      <style jsx>{`
        .status-bar-container { position: relative; }
        .status-bar { display: flex; align-items: center; gap: 30px; padding: 15px 20px; background: #1a1a2e; border-radius: 10px; margin-bottom: 20px; }
        .status-item { display: flex; flex-direction: column; align-items: center; gap: 4px; }
        .status-label { color: #aaa; font-size: 0.8rem; text-transform: uppercase; letter-spacing: 0.5px; }
        .status-value { color: #fff; font-size: 1.1rem; font-weight: 600; }
        .status-value.running { color: #00d4aa; }
        .status-value.stopped { color: #ff6b6b; }
        .status-note { color: #aaa; font-size: 0.8rem; }
        .settings-toggle-btn { margin-left: auto; background: #252542; border: 1px solid #3a3a5a; border-radius: 8px; padding: 8px 12px; cursor: pointer; font-size: 1.2rem; transition: all 0.2s; }
        .settings-toggle-btn:hover, .settings-toggle-btn.active { border-color: #00d4aa; background: #2a2a4a; }
        .engine-settings-panel { background: #252542; border: 1px solid #3a3a5a; border-radius: 10px; padding: 20px; margin-bottom: 20px; }
        .settings-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 15px; }
        .settings-header h4 { color: #00d4aa; margin: 0; }
        .settings-note { color: #888; font-size: 0.8rem; }
        .settings-message { padding: 10px 15px; border-radius: 6px; margin-bottom: 15px; font-size: 0.9rem; display: flex; justify-content: space-between; align-items: center; }
        .settings-message.success { background: rgba(0, 212, 170, 0.1); border: 1px solid #00d4aa; color: #00d4aa; }
        .settings-message.error { background: rgba(255, 107, 107, 0.1); border: 1px solid #ff6b6b; color: #ff6b6b; }
        .settings-message.warning { background: rgba(240, 173, 78, 0.1); border: 1px solid #f0ad4e; color: #f0ad4e; }
        .settings-message.info { background: rgba(100, 149, 237, 0.1); border: 1px solid #6495ed; color: #6495ed; }
        .close-message { background: none; border: none; color: inherit; font-size: 1.2rem; cursor: pointer; padding: 0 5px; }
        .scanner-toggle-section { background: #1a1a2e; border-radius: 8px; padding: 15px; margin-bottom: 15px; display: flex; align-items: center; gap: 15px; flex-wrap: wrap; }
        .toggle-label { color: #fff; font-weight: 600; min-width: 70px; }
        .toggle-buttons { display: flex; gap: 8px; }
        .toggle-btn { padding: 8px 16px; border-radius: 6px; border: 1px solid #3a3a5a; background: #252542; color: #888; cursor: pointer; transition: all 0.2s; font-size: 0.9rem; }
        .toggle-btn:hover { border-color: #00d4aa; }
        .toggle-btn.active { background: rgba(0, 212, 170, 0.2); border-color: #00d4aa; color: #00d4aa; }
        .toggle-btn.active.off { background: rgba(255, 107, 107, 0.2); border-color: #ff6b6b; color: #ff6b6b; }
        .toggle-warning { color: #f0ad4e; font-size: 0.85rem; }
        .settings-grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 15px; margin-bottom: 15px; }
        @media (max-width: 768px) { .settings-grid { grid-template-columns: 1fr; } }
        .setting-item { display: flex; flex-direction: column; gap: 6px; }
        .setting-item label { color: #888; font-size: 0.85rem; }
        .setting-item select { background: #1a1a2e; border: 1px solid #3a3a5a; border-radius: 6px; color: #fff; padding: 10px 12px; font-size: 0.95rem; cursor: pointer; }
        .setting-item select:hover { border-color: #00d4aa; }
        .setting-item select:focus { outline: none; border-color: #00d4aa; }
        .reconnect-warning { background: rgba(240, 173, 78, 0.1); border: 1px solid #f0ad4e; border-radius: 6px; padding: 10px 15px; color: #f0ad4e; font-size: 0.85rem; margin-bottom: 15px; }
        .settings-actions { display: flex; gap: 15px; justify-content: flex-end; align-items: center; }
        .pending-indicator { color: #f0ad4e; font-size: 0.85rem; }
        .restart-btn { padding: 12px 24px; border-radius: 6px; font-size: 0.95rem; cursor: pointer; transition: all 0.2s; background: linear-gradient(135deg, #f0ad4e, #e09a3e); border: none; color: #1a1a2e; font-weight: 600; }
        .restart-btn:hover:not(:disabled) { background: linear-gradient(135deg, #f5b75e, #e5a448); transform: translateY(-1px); }
        .restart-btn:disabled { background: #555; color: #999; cursor: not-allowed; transform: none; }
        .cooldown-note { text-align: right; color: #888; font-size: 0.8rem; margin-top: 10px; }
        .status-divider { width: 1px; height: 30px; background: #3a3a5a; margin: 0 10px; }
        .status-value.positive { color: #00d4aa; }
      `}</style>
    </div>
  );
}

export default StatusBar;
