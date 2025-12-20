// ============================================
// KrakenCryptoX v2.0 - Order Book Health Panel
// Shows data quality metrics and validation stats
// ============================================

import React, { useState, useEffect, useCallback } from 'react';
import { api } from '../services/api';

export function OrderBookHealthPanel() {
  const [health, setHealth] = useState(null);
  const [history, setHistory] = useState([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  const [historyHours, setHistoryHours] = useState(24);

  const fetchHealth = useCallback(async () => {
    try {
      setError(null);
      const data = await api.getOrderbookHealth();
      setHealth(data);
    } catch (err) {
      console.error('Error fetching order book health:', err);
      setError(err.message);
    } finally {
      setLoading(false);
    }
  }, []);

  const fetchHistory = useCallback(async () => {
    try {
      const data = await api.getOrderbookHealthHistory(historyHours);
      setHistory(data.history || []);
    } catch (err) {
      console.error('Error fetching health history:', err);
    }
  }, [historyHours]);

  useEffect(() => {
    fetchHealth();
    fetchHistory();
    const healthInterval = setInterval(fetchHealth, 5000);
    const historyInterval = setInterval(fetchHistory, 60000); // Update history every minute
    return () => {
      clearInterval(healthInterval);
      clearInterval(historyInterval);
    };
  }, [fetchHealth, fetchHistory]);

  useEffect(() => {
    fetchHistory();
  }, [historyHours, fetchHistory]);

  if (loading) {
    return (
      <div className="panel orderbook-health-panel loading">
        <p>Loading order book health data...</p>
      </div>
    );
  }

  if (error) {
    return (
      <div className="panel orderbook-health-panel error">
        <p>⚠️ Error: {error}</p>
        <button onClick={fetchHealth}>Retry</button>
      </div>
    );
  }

  const getStatusColor = (pct) => {
    if (pct >= 80) return 'green';
    if (pct >= 60) return 'yellow';
    return 'red';
  };

  const getFreshnessStatus = (ms) => {
    if (ms < 500) return { color: 'green', label: 'Excellent' };
    if (ms < 1500) return { color: 'yellow', label: 'Good' };
    return { color: 'red', label: 'Slow' };
  };

  const freshnessStatus = getFreshnessStatus(health?.averages?.freshness_ms || 0);

  // Calculate chart dimensions
  const chartWidth = 100;
  const chartHeight = 60;
  const maxValidPairs = Math.max(...history.map(h => h.valid_pairs), 1);
  const maxFreshness = Math.max(...history.map(h => h.averages?.freshness_ms || 0), 1);

  const getValidPairsPath = () => {
    if (history.length < 2) return '';
    const points = history.map((h, i) => {
      const x = (i / (history.length - 1)) * chartWidth;
      const y = chartHeight - (h.valid_pairs / maxValidPairs) * chartHeight;
      return `${x},${y}`;
    });
    return `M ${points.join(' L ')}`;
  };

  const getFreshnessPath = () => {
    if (history.length < 2) return '';
    const points = history.map((h, i) => {
      const x = (i / (history.length - 1)) * chartWidth;
      const y = chartHeight - ((h.averages?.freshness_ms || 0) / maxFreshness) * chartHeight;
      return `${x},${y}`;
    });
    return `M ${points.join(' L ')}`;
  };

  const formatTime = (timestamp) => {
    return new Date(timestamp).toLocaleTimeString('en-US', {
      hour: '2-digit',
      minute: '2-digit',
    });
  };

  return (
    <div className="panel orderbook-health-panel">
      {/* Overview Section */}
      <div className="health-section">
        <h3>📊 Order Book Health Overview</h3>
        <div className="overview-grid">
          <div className={`overview-card ${getStatusColor(health?.valid_pct || 0)}`}>
            <span className="overview-value">{health?.valid_pairs || 0}</span>
            <span className="overview-label">Valid Pairs</span>
            <span className="overview-sub">of {health?.total_pairs || 0} total ({health?.valid_pct || 0}%)</span>
          </div>
          <div className={`overview-card ${freshnessStatus.color}`}>
            <span className="overview-value">{health?.averages?.freshness_ms?.toFixed(0) || 0}ms</span>
            <span className="overview-label">Avg Freshness</span>
            <span className="overview-sub">{freshnessStatus.label}</span>
          </div>
          <div className="overview-card">
            <span className="overview-value">{health?.averages?.spread_pct?.toFixed(3) || 0}%</span>
            <span className="overview-label">Avg Spread</span>
            <span className="overview-sub">Bid-Ask spread</span>
          </div>
          <div className="overview-card">
            <span className="overview-value">{health?.averages?.depth?.toFixed(0) || 0}</span>
            <span className="overview-label">Avg Depth</span>
            <span className="overview-sub">Levels per side</span>
          </div>
        </div>
      </div>

      {/* Trend Charts Section */}
      <div className="health-section">
        <div className="chart-header">
          <h3>📈 Health Trends</h3>
          <div className="chart-controls">
            <select 
              value={historyHours} 
              onChange={(e) => setHistoryHours(parseInt(e.target.value))}
            >
              <option value={6}>Last 6 hours</option>
              <option value={24}>Last 24 hours</option>
              <option value={72}>Last 3 days</option>
              <option value={168}>Last 7 days</option>
              <option value={720}>Last 30 days</option>
            </select>
          </div>
        </div>
        
        {history.length < 2 ? (
          <div className="no-history">
            <p>📊 No historical data yet</p>
            <p className="sub">Health snapshots are saved every 5 minutes. Check back soon!</p>
          </div>
        ) : (
          <div className="charts-grid">
            {/* Valid Pairs Chart */}
            <div className="chart-card">
              <div className="chart-title">
                <span>Valid Pairs</span>
                <span className="chart-current">{health?.valid_pairs || 0}</span>
              </div>
              <svg viewBox={`0 0 ${chartWidth} ${chartHeight}`} className="trend-chart">
                <defs>
                  <linearGradient id="validGradient" x1="0%" y1="0%" x2="0%" y2="100%">
                    <stop offset="0%" stopColor="#00d4aa" stopOpacity="0.3"/>
                    <stop offset="100%" stopColor="#00d4aa" stopOpacity="0"/>
                  </linearGradient>
                </defs>
                <path d={getValidPairsPath()} fill="none" stroke="#00d4aa" strokeWidth="1.5"/>
                <path d={getValidPairsPath() + ` L ${chartWidth},${chartHeight} L 0,${chartHeight} Z`} fill="url(#validGradient)"/>
              </svg>
              <div className="chart-labels">
                <span>{formatTime(history[0]?.timestamp)}</span>
                <span>{formatTime(history[history.length - 1]?.timestamp)}</span>
              </div>
            </div>

            {/* Freshness Chart */}
            <div className="chart-card">
              <div className="chart-title">
                <span>Avg Freshness (ms)</span>
                <span className="chart-current">{health?.averages?.freshness_ms?.toFixed(0) || 0}ms</span>
              </div>
              <svg viewBox={`0 0 ${chartWidth} ${chartHeight}`} className="trend-chart">
                <defs>
                  <linearGradient id="freshnessGradient" x1="0%" y1="0%" x2="0%" y2="100%">
                    <stop offset="0%" stopColor="#ffd700" stopOpacity="0.3"/>
                    <stop offset="100%" stopColor="#ffd700" stopOpacity="0"/>
                  </linearGradient>
                </defs>
                <path d={getFreshnessPath()} fill="none" stroke="#ffd700" strokeWidth="1.5"/>
                <path d={getFreshnessPath() + ` L ${chartWidth},${chartHeight} L 0,${chartHeight} Z`} fill="url(#freshnessGradient)"/>
              </svg>
              <div className="chart-labels">
                <span>{formatTime(history[0]?.timestamp)}</span>
                <span>{formatTime(history[history.length - 1]?.timestamp)}</span>
              </div>
            </div>

            {/* Skipped Total Chart */}
            <div className="chart-card">
              <div className="chart-title">
                <span>Total Skipped</span>
                <span className="chart-current">{health?.skipped?.total || 0}</span>
              </div>
              <svg viewBox={`0 0 ${chartWidth} ${chartHeight}`} className="trend-chart">
                <defs>
                  <linearGradient id="skippedGradient" x1="0%" y1="0%" x2="0%" y2="100%">
                    <stop offset="0%" stopColor="#ff6b6b" stopOpacity="0.3"/>
                    <stop offset="100%" stopColor="#ff6b6b" stopOpacity="0"/>
                  </linearGradient>
                </defs>
                {(() => {
                  const maxSkipped = Math.max(...history.map(h => h.skipped?.total || 0), 1);
                  const path = history.length < 2 ? '' : (() => {
                    const points = history.map((h, i) => {
                      const x = (i / (history.length - 1)) * chartWidth;
                      const y = chartHeight - ((h.skipped?.total || 0) / maxSkipped) * chartHeight;
                      return `${x},${y}`;
                    });
                    return `M ${points.join(' L ')}`;
                  })();
                  return (
                    <>
                      <path d={path} fill="none" stroke="#ff6b6b" strokeWidth="1.5"/>
                      <path d={path + ` L ${chartWidth},${chartHeight} L 0,${chartHeight} Z`} fill="url(#skippedGradient)"/>
                    </>
                  );
                })()}
              </svg>
              <div className="chart-labels">
                <span>{formatTime(history[0]?.timestamp)}</span>
                <span>{formatTime(history[history.length - 1]?.timestamp)}</span>
              </div>
            </div>

            {/* Spread Chart */}
            <div className="chart-card">
              <div className="chart-title">
                <span>Avg Spread (%)</span>
                <span className="chart-current">{health?.averages?.spread_pct?.toFixed(3) || 0}%</span>
              </div>
              <svg viewBox={`0 0 ${chartWidth} ${chartHeight}`} className="trend-chart">
                <defs>
                  <linearGradient id="spreadGradient" x1="0%" y1="0%" x2="0%" y2="100%">
                    <stop offset="0%" stopColor="#9b59b6" stopOpacity="0.3"/>
                    <stop offset="100%" stopColor="#9b59b6" stopOpacity="0"/>
                  </linearGradient>
                </defs>
                {(() => {
                  const maxSpread = Math.max(...history.map(h => h.averages?.spread_pct || 0), 0.001);
                  const path = history.length < 2 ? '' : (() => {
                    const points = history.map((h, i) => {
                      const x = (i / (history.length - 1)) * chartWidth;
                      const y = chartHeight - ((h.averages?.spread_pct || 0) / maxSpread) * chartHeight;
                      return `${x},${y}`;
                    });
                    return `M ${points.join(' L ')}`;
                  })();
                  return (
                    <>
                      <path d={path} fill="none" stroke="#9b59b6" strokeWidth="1.5"/>
                      <path d={path + ` L ${chartWidth},${chartHeight} L 0,${chartHeight} Z`} fill="url(#spreadGradient)"/>
                    </>
                  );
                })()}
              </svg>
              <div className="chart-labels">
                <span>{formatTime(history[0]?.timestamp)}</span>
                <span>{formatTime(history[history.length - 1]?.timestamp)}</span>
              </div>
            </div>
          </div>
        )}
        <div className="chart-info">
          <span>📊 {history.length} data points | Snapshots every 5 minutes | Kept for 30 days</span>
        </div>
      </div>

      {/* Validation Rules Section */}
      <div className="health-section">
        <h3>🔍 Validation Rules</h3>
        <div className="rules-table">
          <table>
            <thead>
              <tr>
                <th>Check</th>
                <th>Threshold</th>
                <th>Description</th>
                <th>Status</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td><strong>Order Book Exists</strong></td>
                <td>Must have data</td>
                <td>Skips pairs with NO order book (only ticker)</td>
                <td className="status-cell">
                  <span className={`status-badge ${health?.skipped?.no_orderbook === 0 ? 'green' : 'yellow'}`}>
                    {health?.skipped?.no_orderbook || 0} skipped
                  </span>
                </td>
              </tr>
              <tr>
                <td><strong>Minimum Depth</strong></td>
                <td>≥ {health?.thresholds?.min_depth || 3} levels each side</td>
                <td>Skips thin order books (&lt; 3 bids or &lt; 3 asks)</td>
                <td className="status-cell">
                  <span className={`status-badge ${health?.skipped?.thin_depth === 0 ? 'green' : 'yellow'}`}>
                    {health?.skipped?.thin_depth || 0} skipped
                  </span>
                </td>
              </tr>
              <tr>
                <td><strong>Freshness</strong></td>
                <td>&lt; {(health?.thresholds?.max_staleness_ms || 5000) / 1000}s old</td>
                <td>Skips stale order books (&gt; 5000ms old)</td>
                <td className="status-cell">
                  <span className={`status-badge ${health?.skipped?.stale === 0 ? 'green' : 'yellow'}`}>
                    {health?.skipped?.stale || 0} skipped
                  </span>
                </td>
              </tr>
              <tr>
                <td><strong>Spread Check</strong></td>
                <td>&lt; {health?.thresholds?.max_spread_pct || 10}% spread</td>
                <td>Skips unrealistic spreads (likely bad data)</td>
                <td className="status-cell">
                  <span className={`status-badge ${health?.skipped?.bad_spread === 0 ? 'green' : 'yellow'}`}>
                    {health?.skipped?.bad_spread || 0} skipped
                  </span>
                </td>
              </tr>
              <tr>
                <td><strong>Price Validation</strong></td>
                <td>Ticker vs Order Book</td>
                <td>Uses order book prices if ticker differs &gt; 5%</td>
                <td className="status-cell">
                  <span className="status-badge green">Active</span>
                </td>
              </tr>
              <tr>
                <td><strong>Profit Sanity</strong></td>
                <td>&lt; {health?.thresholds?.max_profit_pct || 5}% gross profit</td>
                <td>Rejects "too good to be true" opportunities</td>
                <td className="status-cell">
                  <span className={`status-badge ${health?.rejected_opportunities === 0 ? 'green' : 'yellow'}`}>
                    {health?.rejected_opportunities || 0} rejected
                  </span>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>

      {/* Skip Breakdown Section */}
      <div className="health-section">
        <h3>📉 Skip Breakdown</h3>
        <div className="skip-grid">
          <div className="skip-card">
            <div className="skip-icon">📭</div>
            <div className="skip-value">{health?.skipped?.no_orderbook || 0}</div>
            <div className="skip-label">No Order Book</div>
            <div className="skip-desc">Only ticker data available</div>
          </div>
          <div className="skip-card">
            <div className="skip-icon">📊</div>
            <div className="skip-value">{health?.skipped?.thin_depth || 0}</div>
            <div className="skip-label">Thin Depth</div>
            <div className="skip-desc">&lt; 3 levels on a side</div>
          </div>
          <div className="skip-card">
            <div className="skip-icon">⏰</div>
            <div className="skip-value">{health?.skipped?.stale || 0}</div>
            <div className="skip-label">Stale Data</div>
            <div className="skip-desc">&gt; 5 seconds old</div>
          </div>
          <div className="skip-card">
            <div className="skip-icon">↔️</div>
            <div className="skip-value">{health?.skipped?.bad_spread || 0}</div>
            <div className="skip-label">Bad Spread</div>
            <div className="skip-desc">&gt; 10% bid-ask spread</div>
          </div>
          <div className="skip-card">
            <div className="skip-icon">💰</div>
            <div className="skip-value">{health?.skipped?.no_price || 0}</div>
            <div className="skip-label">No Price</div>
            <div className="skip-desc">Missing bid/ask</div>
          </div>
          <div className="skip-card total">
            <div className="skip-icon">Σ</div>
            <div className="skip-value">{health?.skipped?.total || 0}</div>
            <div className="skip-label">Total Skipped</div>
            <div className="skip-desc">All validation failures</div>
          </div>
        </div>
      </div>

      {/* Last Update */}
      <div className="health-footer">
        <span>Last updated: {health?.last_update ? new Date(health.last_update).toLocaleTimeString() : '--'}</span>
        <button className="refresh-btn" onClick={() => { fetchHealth(); fetchHistory(); }}>🔄 Refresh</button>
      </div>

      <style jsx>{`
        .orderbook-health-panel {
          padding: 20px;
        }

        .health-section {
          background: #1a1a2e;
          border-radius: 12px;
          padding: 20px;
          margin-bottom: 20px;
        }

        .health-section h3 {
          color: #00d4aa;
          margin-bottom: 20px;
          font-size: 1.1rem;
        }

        /* Chart Header */
        .chart-header {
          display: flex;
          justify-content: space-between;
          align-items: center;
          margin-bottom: 20px;
        }

        .chart-header h3 {
          margin-bottom: 0;
        }

        .chart-controls select {
          background: #252542;
          border: 1px solid #3a3a5a;
          border-radius: 6px;
          color: #fff;
          padding: 8px 12px;
          cursor: pointer;
        }

        .chart-controls select:hover {
          border-color: #00d4aa;
        }

        /* Charts Grid */
        .charts-grid {
          display: grid;
          grid-template-columns: repeat(2, 1fr);
          gap: 20px;
        }

        @media (max-width: 800px) {
          .charts-grid {
            grid-template-columns: 1fr;
          }
        }

        .chart-card {
          background: #252542;
          border-radius: 10px;
          padding: 15px;
        }

        .chart-title {
          display: flex;
          justify-content: space-between;
          align-items: center;
          margin-bottom: 10px;
          color: #888;
          font-size: 0.9rem;
        }

        .chart-current {
          color: #fff;
          font-weight: 600;
        }

        .trend-chart {
          width: 100%;
          height: 80px;
        }

        .chart-labels {
          display: flex;
          justify-content: space-between;
          color: #666;
          font-size: 0.75rem;
          margin-top: 5px;
        }

        .chart-info {
          text-align: center;
          color: #666;
          font-size: 0.8rem;
          margin-top: 15px;
        }

        .no-history {
          text-align: center;
          padding: 40px;
          color: #888;
        }

        .no-history .sub {
          font-size: 0.85rem;
          color: #666;
          margin-top: 10px;
        }

        /* Overview Grid */
        .overview-grid {
          display: grid;
          grid-template-columns: repeat(4, 1fr);
          gap: 15px;
        }

        @media (max-width: 900px) {
          .overview-grid {
            grid-template-columns: repeat(2, 1fr);
          }
        }

        .overview-card {
          background: #252542;
          border-radius: 10px;
          padding: 20px;
          text-align: center;
          border-left: 4px solid #3a3a5a;
        }

        .overview-card.green {
          border-left-color: #00d4aa;
        }

        .overview-card.yellow {
          border-left-color: #ffd700;
        }

        .overview-card.red {
          border-left-color: #ff6b6b;
        }

        .overview-value {
          display: block;
          font-size: 2rem;
          font-weight: 700;
          color: #fff;
          margin-bottom: 5px;
        }

        .overview-label {
          display: block;
          color: #888;
          font-size: 0.9rem;
          margin-bottom: 5px;
        }

        .overview-sub {
          display: block;
          color: #666;
          font-size: 0.8rem;
        }

        /* Rules Table */
        .rules-table {
          overflow-x: auto;
        }

        .rules-table table {
          width: 100%;
          border-collapse: collapse;
        }

        .rules-table th {
          background: #00d4aa;
          color: #1a1a2e;
          padding: 12px;
          text-align: left;
          font-weight: 600;
        }

        .rules-table td {
          padding: 12px;
          border-bottom: 1px solid #2a2a4a;
          color: #ccc;
        }

        .rules-table tr:hover {
          background: rgba(255, 255, 255, 0.02);
        }

        .status-cell {
          text-align: center;
        }

        .status-badge {
          display: inline-block;
          padding: 4px 12px;
          border-radius: 20px;
          font-size: 0.8rem;
          font-weight: 600;
        }

        .status-badge.green {
          background: rgba(0, 212, 170, 0.2);
          color: #00d4aa;
        }

        .status-badge.yellow {
          background: rgba(255, 215, 0, 0.2);
          color: #ffd700;
        }

        .status-badge.red {
          background: rgba(255, 107, 107, 0.2);
          color: #ff6b6b;
        }

        /* Skip Grid */
        .skip-grid {
          display: grid;
          grid-template-columns: repeat(6, 1fr);
          gap: 15px;
        }

        @media (max-width: 1200px) {
          .skip-grid {
            grid-template-columns: repeat(3, 1fr);
          }
        }

        @media (max-width: 600px) {
          .skip-grid {
            grid-template-columns: repeat(2, 1fr);
          }
        }

        .skip-card {
          background: #252542;
          border-radius: 10px;
          padding: 15px;
          text-align: center;
        }

        .skip-card.total {
          background: linear-gradient(135deg, #2a2a4a, #1a1a3a);
          border: 1px solid #00d4aa;
        }

        .skip-icon {
          font-size: 1.5rem;
          margin-bottom: 8px;
        }

        .skip-value {
          font-size: 1.5rem;
          font-weight: 700;
          color: #fff;
        }

        .skip-label {
          color: #888;
          font-size: 0.85rem;
          margin-top: 5px;
        }

        .skip-desc {
          color: #666;
          font-size: 0.75rem;
          margin-top: 3px;
        }

        /* Footer */
        .health-footer {
          display: flex;
          justify-content: space-between;
          align-items: center;
          padding: 15px 20px;
          background: #1a1a2e;
          border-radius: 8px;
          color: #666;
          font-size: 0.85rem;
        }

        .refresh-btn {
          background: #252542;
          border: 1px solid #3a3a5a;
          color: #fff;
          padding: 8px 16px;
          border-radius: 6px;
          cursor: pointer;
          transition: all 0.2s;
        }

        .refresh-btn:hover {
          border-color: #00d4aa;
          color: #00d4aa;
        }

        /* Loading & Error states */
        .loading, .error {
          text-align: center;
          padding: 40px;
          color: #888;
        }

        .error button {
          margin-top: 10px;
          padding: 8px 16px;
          background: #00d4aa;
          border: none;
          border-radius: 6px;
          color: #1a1a2e;
          cursor: pointer;
        }
      `}</style>
    </div>
  );
}

export default OrderBookHealthPanel;
