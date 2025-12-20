// ============================================
// KrakenCryptoX - Shadow Mode Panel
// Shows Kraken connection status and shadow trading stats
// ============================================

import React, { useState, useEffect, useCallback } from 'react';
import { api } from '../services/api';

export function ShadowModePanel() {
  const [status, setStatus] = useState(null);
  const [trades, setTrades] = useState([]);
  const [tradesTotal, setTradesTotal] = useState(0);
  const [detailedTrades, setDetailedTrades] = useState([]);
  const [detailedTotal, setDetailedTotal] = useState(0);
  const [accuracy, setAccuracy] = useState(null);
  const [stats, setStats] = useState(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  
  // Active tab
  const [activeTab, setActiveTab] = useState('detailed'); // 'detailed' or 'executions'
  
  // Pagination for executions table
  const [execCurrentPage, setExecCurrentPage] = useState(1);
  const [execPageSize] = useState(20);
  
  // Pagination for detailed trades table
  const [detailedCurrentPage, setDetailedCurrentPage] = useState(1);
  const [detailedPageSize] = useState(20);
  
  // Filters
  const [hoursFilter, setHoursFilter] = useState(24);
  const [resultFilter, setResultFilter] = useState('');
  const [pathFilter, setPathFilter] = useState('');
  
  // Expanded row for leg details
  const [expandedTradeId, setExpandedTradeId] = useState(null);

  // Calculate performance stats from detailed trades
  const calculatePerformance = useCallback(() => {
    if (!detailedTrades || detailedTrades.length === 0) {
      return {
        totalTrades: stats?.total_trades || 0,
        wins: stats?.wins || 0,
        losses: stats?.losses || 0,
        avgProfit: 0,
        avgLoss: 0,
      };
    }
    
    const wins = detailedTrades.filter(t => t.status === 'WIN');
    const losses = detailedTrades.filter(t => t.status === 'LOSS');
    
    const avgProfit = wins.length > 0 
      ? wins.reduce((sum, t) => sum + (t.net_profit_usd || 0), 0) / wins.length 
      : 0;
    
    const avgLoss = losses.length > 0 
      ? Math.abs(losses.reduce((sum, t) => sum + (t.net_profit_usd || 0), 0) / losses.length)
      : 0;
    
    return {
      totalTrades: stats?.total_trades || detailedTotal,
      wins: stats?.wins || wins.length,
      losses: stats?.losses || losses.length,
      avgProfit,
      avgLoss,
    };
  }, [detailedTrades, detailedTotal, stats]);

  const fetchData = useCallback(async () => {
    try {
      setError(null);
      
      const [statusData, tradesData, detailedData, statsData] = await Promise.all([
        api.getShadowStatus(),
        api.getShadowTradesHistory({
          limit: execPageSize,
          offset: (execCurrentPage - 1) * execPageSize,
          hours: hoursFilter,
          resultFilter: resultFilter || null,
          pathFilter: pathFilter || null,
        }),
        api.getShadowTradesDetailed({
          limit: detailedPageSize,
          offset: (detailedCurrentPage - 1) * detailedPageSize,
          hours: hoursFilter,
          resultFilter: resultFilter || null,
          pathFilter: pathFilter || null,
        }),
        api.getShadowTradesStats(hoursFilter),
      ]);
      
      setStatus(statusData);
      setTrades(tradesData.trades || []);
      setTradesTotal(tradesData.total || 0);
      setDetailedTrades(detailedData.trades || []);
      setDetailedTotal(detailedData.total || 0);
      setStats(statsData);
      
      // Fetch accuracy report if we have trades
      if (statsData?.total_trades > 0) {
        const accuracyData = await api.getShadowAccuracy();
        setAccuracy(accuracyData);
      }
      
    } catch (err) {
      console.error('Error fetching shadow mode data:', err);
      setError(err.message);
    } finally {
      setLoading(false);
    }
  }, [execCurrentPage, execPageSize, detailedCurrentPage, detailedPageSize, hoursFilter, resultFilter, pathFilter]);

  useEffect(() => {
    fetchData();
    const interval = setInterval(fetchData, 15000); // Update every 15s
    return () => clearInterval(interval);
  }, [fetchData]);

  const execTotalPages = Math.ceil(tradesTotal / execPageSize);
  const detailedTotalPages = Math.ceil(detailedTotal / detailedPageSize);

  const handleFilterChange = () => {
    setExecCurrentPage(1);
    setDetailedCurrentPage(1);
  };
  
  const toggleExpandRow = (tradeId) => {
    setExpandedTradeId(expandedTradeId === tradeId ? null : tradeId);
  };

  if (loading) {
    return (
      <div className="panel shadow-mode-panel loading">
        <p>Loading shadow mode data...</p>
      </div>
    );
  }

  const isConnected = status?.connected;
  const mode = status?.mode || 'disconnected';
  const krakenBalance = status?.kraken_balance_usd || 0;
  const performance = calculatePerformance();

  return (
    <div className="panel shadow-mode-panel">
      {error && (
        <div className="error-banner">
          ⚠️ {error}
        </div>
      )}

      {/* Connection Status */}
      <div className="connection-status">
        <div className={`status-indicator ${isConnected ? 'connected' : 'disconnected'}`}>
          <span className="status-dot"></span>
          <span className="status-text">
            {isConnected ? 'Connected to Kraken' : 'Not Connected'}
          </span>
        </div>
        
        <div className="mode-badge">
          {mode === 'shadow' && <span className="badge shadow">🔍 SHADOW MODE</span>}
          {mode === 'live' && <span className="badge live">⚡ LIVE MODE</span>}
          {mode === 'disconnected' && <span className="badge disconnected">❌ DISCONNECTED</span>}
        </div>
      </div>

      {/* Top Section: Total Value + Holdings */}
      {isConnected && (
        <div className="top-section">
          <div className="info-card highlight">
            <span className="label">Total Value (USD)</span>
            <span className="value">${krakenBalance.toFixed(2)}</span>
          </div>
          
          {status?.kraken_balances && Object.keys(status.kraken_balances).length > 0 && (
            <div className="holdings-inline">
              <span className="holdings-label">💰 Holdings</span>
              <div className="holdings-items">
                {Object.entries(status.kraken_balances).map(([currency, amount]) => (
                  <div key={currency} className="holding-item">
                    <span className="currency">{currency.replace('XXBT', 'BTC').replace('XETH', 'ETH').replace('ZUSD', 'USD').replace('ZEUR', 'EUR')}</span>
                    <span className="amount">{parseFloat(amount).toFixed(8)}</span>
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      )}

      {/* Performance Section */}
      <div className="performance-section">
        <h3>📈 Performance</h3>
        <div className="performance-grid">
          <div className="perf-card">
            <span className="label">Total Trades</span>
            <span className="value">{performance.totalTrades}</span>
          </div>
          <div className="perf-card">
            <span className="label">Wins</span>
            <span className="value positive">{performance.wins}</span>
          </div>
          <div className="perf-card">
            <span className="label">Losses</span>
            <span className="value negative">{performance.losses}</span>
          </div>
          <div className="perf-card">
            <span className="label">Avg Profit/Trade</span>
            <span className="value positive">${performance.avgProfit.toFixed(2)}</span>
          </div>
          <div className="perf-card">
            <span className="label">Avg Loss/Trade</span>
            <span className="value negative">${performance.avgLoss.toFixed(2)}</span>
          </div>
          <div className="perf-card">
            <span className="label">Max Loss Limit</span>
            <span className="value">${status?.max_loss_usd || 30}</span>
          </div>
        </div>
      </div>

      {/* Tab Navigation + Filters (Same Line) */}
      <div className="tabs-filters-row">
        <div className="tab-navigation">
          <button 
            className={`tab-btn ${activeTab === 'detailed' ? 'active' : ''}`}
            onClick={() => setActiveTab('detailed')}
          >
            📊 Shadow Trades
          </button>
          <button 
            className={`tab-btn ${activeTab === 'executions' ? 'active' : ''}`}
            onClick={() => setActiveTab('executions')}
          >
            🔍 Shadow Executions (Paper vs Shadow)
          </button>
        </div>
        
        <div className="filters-inline">
          <div className="filter-group">
            <label>Time Range:</label>
            <select 
              value={hoursFilter} 
              onChange={(e) => { setHoursFilter(parseInt(e.target.value)); handleFilterChange(); }}
            >
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
            <select 
              value={resultFilter} 
              onChange={(e) => { setResultFilter(e.target.value); handleFilterChange(); }}
            >
              <option value="">All</option>
              <option value="win">Wins Only</option>
              <option value="loss">Losses Only</option>
            </select>
          </div>
          
          <div className="filter-group">
            <label>Starts with:</label>
            <input 
              type="text" 
              placeholder="USD, EUR..."
              value={pathFilter}
              onChange={(e) => { setPathFilter(e.target.value.toUpperCase()); handleFilterChange(); }}
              className="path-search"
            />
          </div>
        </div>
      </div>

      {/* Shadow Trades Table (Detailed) */}
      {activeTab === 'detailed' && (
        <div className="trades-section">
          <div className="section-header">
            <h3>📊 Shadow Trades</h3>
            <p className="section-desc">Real fees and slippage from Kraken live order books</p>
          </div>
          
          {detailedTrades.length === 0 ? (
            <div className="empty-state">
              <p>No detailed shadow trades found.</p>
              <p className="hint">Shadow trades will appear here when opportunities are executed.</p>
            </div>
          ) : (
            <>
              <div className="trades-table-container">
                <table className="trades-table detailed-table">
                  <thead>
                    <tr>
                      <th></th>
                      <th>Time</th>
                      <th>Path</th>
                      <th>Amount</th>
                      <th>Taker Fee</th>
                      <th>Slippage</th>
                      <th>Profit</th>
                      <th>Status</th>
                    </tr>
                  </thead>
                  <tbody>
                    {detailedTrades.map((trade, idx) => (
                      <React.Fragment key={trade.id || idx}>
                        <tr 
                          className={`${trade.status === 'WIN' ? 'win' : 'loss'} expandable`}
                          onClick={() => toggleExpandRow(trade.id)}
                        >
                          <td className="expand-cell">
                            {expandedTradeId === trade.id ? '▼' : '▶'}
                          </td>
                          <td className="time-cell">
                            {trade.timestamp ? new Date(trade.timestamp).toLocaleString('en-US', {
                              timeZone: 'America/New_York',
                              month: 'short',
                              day: 'numeric',
                              hour: '2-digit',
                              minute: '2-digit',
                              hour12: true
                            }) : '--'}
                          </td>
                          <td><code>{trade.path}</code></td>
                          <td className="amount-cell">${trade.amount?.toFixed(2)}</td>
                          <td className="fee-cell">
                            <span className="pct">-{trade.taker_fee_pct?.toFixed(2)}%</span>
                            <span className="usd">(-${trade.taker_fee_usd?.toFixed(4)})</span>
                          </td>
                          <td className="slippage-cell">
                            <span className="pct">-{trade.total_slippage_pct?.toFixed(4)}%</span>
                            <span className="usd">(-${trade.total_slippage_usd?.toFixed(4)})</span>
                          </td>
                          <td className={trade.net_profit_usd >= 0 ? 'positive' : 'negative'}>
                            <span className="pct">{trade.net_profit_pct >= 0 ? '+' : ''}{trade.net_profit_pct?.toFixed(4)}%</span>
                            <span className="usd">({trade.net_profit_usd >= 0 ? '+' : ''}${trade.net_profit_usd?.toFixed(4)})</span>
                          </td>
                          <td>
                            <span className={`badge ${trade.status === 'WIN' ? 'win' : 'loss'}`}>
                              {trade.status === 'WIN' ? '✓ WIN' : '✗ LOSS'}
                            </span>
                          </td>
                        </tr>
                        
                        {/* Expanded Leg Details */}
                        {expandedTradeId === trade.id && trade.leg_details && (
                          <tr className="expanded-row">
                            <td colSpan="8">
                              <div className="leg-details">
                                <h4>Leg Details (Live Kraken Data)</h4>
                                <table className="leg-table">
                                  <thead>
                                    <tr>
                                      <th>Leg</th>
                                      <th>Pair</th>
                                      <th>Side</th>
                                      <th>Best Price</th>
                                      <th>Avg Price</th>
                                      <th>Slippage</th>
                                      <th>Fee</th>
                                    </tr>
                                  </thead>
                                  <tbody>
                                    {trade.leg_details.map((leg, legIdx) => (
                                      <tr key={legIdx}>
                                        <td>{leg.leg}</td>
                                        <td><code>{leg.pair}</code></td>
                                        <td className={leg.side === 'buy' ? 'buy-side' : 'sell-side'}>
                                          {leg.side?.toUpperCase()}
                                        </td>
                                        <td>{leg.best_price?.toFixed(8)}</td>
                                        <td>{leg.avg_price?.toFixed(8)}</td>
                                        <td className="negative">-{leg.slippage_pct?.toFixed(4)}%</td>
                                        <td className="negative">-{leg.fee_pct?.toFixed(2)}%</td>
                                      </tr>
                                    ))}
                                  </tbody>
                                </table>
                              </div>
                            </td>
                          </tr>
                        )}
                      </React.Fragment>
                    ))}
                  </tbody>
                </table>
              </div>
              
              {/* Pagination */}
              <div className="pagination">
                <span className="pagination-info">
                  Showing {((detailedCurrentPage - 1) * detailedPageSize) + 1} - {Math.min(detailedCurrentPage * detailedPageSize, detailedTotal)} of {detailedTotal}
                </span>
                <div className="pagination-buttons">
                  <button 
                    onClick={() => setDetailedCurrentPage(p => p - 1)} 
                    disabled={detailedCurrentPage === 1}
                    className="pagination-btn"
                  >
                    ← Previous
                  </button>
                  <span className="page-number">Page {detailedCurrentPage} of {detailedTotalPages || 1}</span>
                  <button 
                    onClick={() => setDetailedCurrentPage(p => p + 1)} 
                    disabled={detailedCurrentPage >= detailedTotalPages}
                    className="pagination-btn"
                  >
                    Next →
                  </button>
                </div>
              </div>
            </>
          )}
        </div>
      )}

      {/* Shadow Executions Table (Paper vs Shadow Comparison) */}
      {activeTab === 'executions' && (
        <div className="trades-section">
          <div className="section-header">
            <h3>🔍 Shadow Executions</h3>
            <p className="section-desc">Compare paper trading predictions vs shadow reality</p>
          </div>
          
          {/* Accuracy Report */}
          {accuracy && accuracy.total_samples > 0 && (
            <div className="accuracy-section">
              <div className="accuracy-grid">
                <div className="accuracy-card">
                  <span className="label">Samples</span>
                  <span className="value">{accuracy.total_samples}</span>
                </div>
                <div className="accuracy-card">
                  <span className="label">False Positive Rate</span>
                  <span className="value warning">{accuracy.false_positive_rate?.toFixed(1)}%</span>
                </div>
                <div className="accuracy-card">
                  <span className="label">Avg Difference</span>
                  <span className="value">{accuracy.avg_difference_pct?.toFixed(4)}%</span>
                </div>
                <div className="accuracy-card">
                  <span className="label">Paper Win Rate</span>
                  <span className="value">{accuracy.paper_win_rate?.toFixed(1)}%</span>
                </div>
                <div className="accuracy-card">
                  <span className="label">Shadow Win Rate</span>
                  <span className="value">{accuracy.shadow_win_rate?.toFixed(1)}%</span>
                </div>
              </div>
              
              {accuracy.paper_vs_reality_gap !== 0 && (
                <div className="gap-warning">
                  <span className="icon">⚠️</span>
                  <span>
                    Paper trading {accuracy.paper_vs_reality_gap > 0 ? 'overestimated' : 'underestimated'} profits by ${Math.abs(accuracy.paper_vs_reality_gap).toFixed(4)}
                  </span>
                </div>
              )}
            </div>
          )}
          
          {trades.length === 0 ? (
            <div className="empty-state">
              <p>No shadow executions found.</p>
              <p className="hint">Shadow executions will appear here when paper trades are validated.</p>
            </div>
          ) : (
            <>
              <div className="trades-table-container">
                <table className="trades-table">
                  <thead>
                    <tr>
                      <th>Time (EST)</th>
                      <th>Path</th>
                      <th>Amount</th>
                      <th>Paper %</th>
                      <th>Shadow %</th>
                      <th>Diff</th>
                      <th>Result</th>
                    </tr>
                  </thead>
                  <tbody>
                    {trades.map((trade, idx) => (
                      <tr key={trade.id || idx} className={trade.would_have_profited ? 'win' : 'loss'}>
                        <td className="time-cell">
                          {trade.timestamp ? new Date(trade.timestamp).toLocaleString('en-US', {
                            timeZone: 'America/New_York',
                            month: 'short',
                            day: 'numeric',
                            hour: '2-digit',
                            minute: '2-digit',
                            second: '2-digit',
                            hour12: true
                          }) : '--'}
                        </td>
                        <td><code>{trade.path}</code></td>
                        <td className="amount-cell">${trade.trade_amount?.toFixed(2) || '10.00'}</td>
                        <td>{trade.paper_profit_pct?.toFixed(4)}%</td>
                        <td>{trade.shadow_profit_pct?.toFixed(4)}%</td>
                        <td className={trade.difference_pct > 0 ? 'negative' : 'positive'}>
                          {trade.difference_pct > 0 ? '-' : '+'}{Math.abs(trade.difference_pct)?.toFixed(4)}%
                        </td>
                        <td>
                          <span className={`badge ${trade.would_have_profited ? 'win' : 'loss'}`}>
                            {trade.would_have_profited ? '✓ PROFIT' : '✗ LOSS'}
                          </span>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              
              {/* Pagination */}
              <div className="pagination">
                <span className="pagination-info">
                  Showing {((execCurrentPage - 1) * execPageSize) + 1} - {Math.min(execCurrentPage * execPageSize, tradesTotal)} of {tradesTotal}
                </span>
                <div className="pagination-buttons">
                  <button 
                    onClick={() => setExecCurrentPage(p => p - 1)} 
                    disabled={execCurrentPage === 1}
                    className="pagination-btn"
                  >
                    ← Previous
                  </button>
                  <span className="page-number">Page {execCurrentPage} of {execTotalPages || 1}</span>
                  <button 
                    onClick={() => setExecCurrentPage(p => p + 1)} 
                    disabled={execCurrentPage >= execTotalPages}
                    className="pagination-btn"
                  >
                    Next →
                  </button>
                </div>
              </div>
            </>
          )}
        </div>
      )}

      {/* Instructions */}
      {!isConnected && (
        <div className="setup-instructions">
          <h3>🔧 Setup Instructions</h3>
          <ol>
            <li>Create Kraken API key at <a href="https://pro.kraken.com" target="_blank" rel="noopener noreferrer">pro.kraken.com</a></li>
            <li>Add key to <code>.env.kraken</code> file on your server</li>
            <li>Restart the backend container</li>
          </ol>
        </div>
      )}

      <style jsx>{`
        .shadow-mode-panel {
          background: #1a1a2e;
          border-radius: 12px;
          padding: 20px;
        }

        .error-banner {
          background: rgba(255, 107, 107, 0.1);
          border: 1px solid #ff6b6b;
          color: #ff6b6b;
          padding: 12px;
          border-radius: 8px;
          margin-bottom: 20px;
        }

        .connection-status {
          display: flex;
          justify-content: space-between;
          align-items: center;
          margin-bottom: 20px;
        }

        .status-indicator {
          display: flex;
          align-items: center;
          gap: 10px;
        }

        .status-dot {
          width: 12px;
          height: 12px;
          border-radius: 50%;
          background: #ff6b6b;
        }

        .status-indicator.connected .status-dot {
          background: #00d4aa;
          box-shadow: 0 0 10px #00d4aa;
        }

        .status-text {
          font-weight: 600;
          color: #fff;
        }

        .mode-badge .badge {
          padding: 6px 12px;
          border-radius: 20px;
          font-size: 0.85rem;
          font-weight: 600;
        }

        .badge.shadow {
          background: rgba(255, 193, 7, 0.2);
          color: #ffc107;
        }

        .badge.live {
          background: rgba(255, 107, 107, 0.2);
          color: #ff6b6b;
        }

        .badge.disconnected {
          background: rgba(136, 136, 136, 0.2);
          color: #888;
        }

        /* Top Section: Total Value + Holdings */
        .top-section {
          display: flex;
          align-items: stretch;
          gap: 20px;
          margin-bottom: 20px;
          background: #252542;
          border-radius: 10px;
          padding: 15px;
        }

        .info-card {
          background: #1a1a2e;
          padding: 15px 25px;
          border-radius: 10px;
          text-align: center;
          min-width: 150px;
        }

        .info-card.highlight {
          border: 1px solid #00d4aa;
        }

        .info-card .label {
          display: block;
          color: #888;
          font-size: 0.8rem;
          margin-bottom: 5px;
        }

        .info-card .value {
          display: block;
          color: #fff;
          font-size: 1.5rem;
          font-weight: 600;
        }

        .holdings-inline {
          display: flex;
          align-items: center;
          gap: 15px;
          flex: 1;
        }

        .holdings-label {
          color: #ffc107;
          font-weight: 600;
          white-space: nowrap;
        }

        .holdings-items {
          display: flex;
          gap: 10px;
          flex-wrap: wrap;
        }

        .holding-item {
          background: #1a1a2e;
          padding: 10px 15px;
          border-radius: 8px;
          text-align: center;
          min-width: 100px;
        }

        .holding-item .currency {
          display: block;
          color: #00d4aa;
          font-weight: 600;
          font-size: 0.9rem;
          margin-bottom: 3px;
        }

        .holding-item .amount {
          display: block;
          color: #fff;
          font-size: 0.85rem;
        }

        /* Performance Section */
        .performance-section {
          background: #252542;
          border-radius: 10px;
          padding: 20px;
          margin-bottom: 20px;
        }

        .performance-section h3 {
          color: #00d4aa;
          margin: 0 0 15px 0;
          font-size: 1.1rem;
        }

        .performance-grid {
          display: grid;
          grid-template-columns: repeat(6, 1fr);
          gap: 15px;
        }

        .perf-card {
          background: #1a1a2e;
          padding: 15px;
          border-radius: 8px;
          text-align: center;
        }

        .perf-card .label {
          display: block;
          color: #888;
          font-size: 0.8rem;
          margin-bottom: 5px;
        }

        .perf-card .value {
          display: block;
          color: #fff;
          font-size: 1.2rem;
          font-weight: 600;
        }

        .perf-card .value.positive {
          color: #00d4aa;
        }

        .perf-card .value.negative {
          color: #ff6b6b;
        }

        /* Tabs + Filters Row */
        .tabs-filters-row {
          display: flex;
          justify-content: space-between;
          align-items: center;
          flex-wrap: wrap;
          gap: 15px;
          margin-bottom: 20px;
          background: #252542;
          border-radius: 10px;
          padding: 15px;
        }

        .tab-navigation {
          display: flex;
          gap: 10px;
        }

        .tab-btn {
          background: #1a1a2e;
          border: 1px solid #3a3a5a;
          border-radius: 8px;
          padding: 10px 20px;
          color: #aaa;
          cursor: pointer;
          transition: all 0.2s;
          font-size: 0.9rem;
        }

        .tab-btn:hover {
          border-color: #00d4aa;
          color: #fff;
        }

        .tab-btn.active {
          background: #00d4aa;
          border-color: #00d4aa;
          color: #1a1a2e;
          font-weight: 600;
        }

        .filters-inline {
          display: flex;
          gap: 20px;
          align-items: center;
        }

        .filter-group {
          display: flex;
          align-items: center;
          gap: 8px;
        }

        .filter-group label {
          color: #00d4aa;
          font-size: 0.9rem;
          font-weight: 600;
        }

        .filter-group select,
        .filter-group input {
          background: #1a1a2e;
          border: 2px solid #00d4aa;
          border-radius: 6px;
          padding: 8px 12px;
          color: #fff;
          font-size: 0.9rem;
        }

        .filter-group select:focus,
        .filter-group input:focus {
          outline: none;
          border-color: #ffc107;
          box-shadow: 0 0 5px rgba(255, 193, 7, 0.3);
        }

        .path-search {
          width: 120px;
        }

        .path-search::placeholder {
          color: #666;
        }

        /* Trades Section */
        .trades-section {
          background: #252542;
          border-radius: 10px;
          padding: 20px;
        }

        .section-header {
          margin-bottom: 20px;
        }

        .trades-section h3 {
          color: #00d4aa;
          margin: 0 0 5px 0;
        }

        .section-desc {
          color: #888;
          font-size: 0.85rem;
          margin: 0;
        }

        .empty-state {
          text-align: center;
          padding: 30px;
          color: #888;
        }

        .empty-state .hint {
          font-size: 0.85rem;
          margin-top: 10px;
        }

        .accuracy-section {
          margin-bottom: 20px;
        }

        .accuracy-grid {
          display: grid;
          grid-template-columns: repeat(5, 1fr);
          gap: 15px;
        }

        .accuracy-card {
          background: #1a1a2e;
          padding: 12px;
          border-radius: 8px;
          text-align: center;
        }

        .accuracy-card .label {
          display: block;
          color: #888;
          font-size: 0.8rem;
          margin-bottom: 5px;
        }

        .accuracy-card .value {
          display: block;
          color: #fff;
          font-size: 1.1rem;
          font-weight: 600;
        }

        .accuracy-card .value.warning {
          color: #ffc107;
        }

        .gap-warning {
          margin-top: 15px;
          padding: 12px;
          background: rgba(255, 193, 7, 0.1);
          border: 1px solid #ffc107;
          border-radius: 8px;
          color: #ffc107;
          display: flex;
          align-items: center;
          gap: 10px;
        }

        .trades-table-container {
          overflow-x: auto;
        }

        .trades-table {
          width: 100%;
          border-collapse: collapse;
        }

        .trades-table th {
          background: #1a1a2e;
          color: #888;
          padding: 12px 8px;
          text-align: left;
          font-weight: 600;
          font-size: 0.85rem;
        }

        .trades-table td {
          padding: 12px 8px;
          border-bottom: 1px solid #3a3a5a;
          color: #fff;
        }

        .trades-table tr.win {
          background: rgba(0, 212, 170, 0.05);
        }

        .trades-table tr.loss {
          background: rgba(255, 107, 107, 0.05);
        }

        .trades-table tr.expandable {
          cursor: pointer;
        }

        .trades-table tr.expandable:hover {
          background: rgba(0, 212, 170, 0.1);
        }

        .expand-cell {
          width: 30px;
          color: #888;
        }

        .trades-table code {
          background: #1a1a2e;
          padding: 4px 8px;
          border-radius: 4px;
          font-size: 0.8rem;
        }

        .trades-table .badge {
          padding: 4px 10px;
          border-radius: 12px;
          font-size: 0.75rem;
          font-weight: 600;
        }

        .trades-table .badge.win {
          background: rgba(0, 212, 170, 0.2);
          color: #00d4aa;
        }

        .trades-table .badge.loss {
          background: rgba(255, 107, 107, 0.2);
          color: #ff6b6b;
        }

        .trades-table td.positive,
        .trades-table .positive {
          color: #00d4aa;
        }

        .trades-table td.negative,
        .trades-table .negative {
          color: #ff6b6b;
        }

        .trades-table .time-cell {
          font-size: 0.8rem;
          color: #aaa;
          white-space: nowrap;
        }

        .trades-table .amount-cell {
          font-weight: 600;
          color: #ffc107;
        }

        .fee-cell, .slippage-cell {
          font-size: 0.85rem;
        }

        .fee-cell .pct, .slippage-cell .pct {
          display: block;
          color: #ff6b6b;
        }

        .fee-cell .usd, .slippage-cell .usd {
          display: block;
          font-size: 0.75rem;
          color: #888;
        }

        .expanded-row {
          background: #1a1a2e !important;
        }

        .expanded-row td {
          padding: 0;
        }

        .leg-details {
          padding: 15px 20px;
        }

        .leg-details h4 {
          color: #ffc107;
          margin: 0 0 15px 0;
          font-size: 0.9rem;
        }

        .leg-table {
          width: 100%;
          border-collapse: collapse;
          font-size: 0.85rem;
        }

        .leg-table th {
          background: #252542;
          color: #888;
          padding: 8px;
          text-align: left;
        }

        .leg-table td {
          padding: 8px;
          border-bottom: 1px solid #3a3a5a;
        }

        .leg-table .buy-side {
          color: #00d4aa;
        }

        .leg-table .sell-side {
          color: #ff6b6b;
        }

        .pagination {
          display: flex;
          justify-content: space-between;
          align-items: center;
          margin-top: 20px;
          padding-top: 15px;
          border-top: 1px solid #3a3a5a;
        }

        .pagination-info {
          color: #888;
          font-size: 0.85rem;
        }

        .pagination-buttons {
          display: flex;
          align-items: center;
          gap: 15px;
        }

        .pagination-btn {
          background: #3a3a5a;
          border: none;
          border-radius: 6px;
          padding: 8px 15px;
          color: #fff;
          cursor: pointer;
          transition: all 0.2s;
        }

        .pagination-btn:hover:not(:disabled) {
          background: #00d4aa;
          color: #1a1a2e;
        }

        .pagination-btn:disabled {
          opacity: 0.5;
          cursor: not-allowed;
        }

        .page-number {
          color: #888;
          font-size: 0.9rem;
        }

        .setup-instructions {
          background: #252542;
          border-radius: 10px;
          padding: 20px;
          margin-top: 20px;
        }

        .setup-instructions h3 {
          color: #ffc107;
          margin-bottom: 15px;
        }

        .setup-instructions ol {
          color: #ccc;
          padding-left: 20px;
        }

        .setup-instructions li {
          margin-bottom: 10px;
        }

        .setup-instructions a {
          color: #00d4aa;
        }

        .setup-instructions code {
          background: #1a1a2e;
          padding: 2px 6px;
          border-radius: 4px;
        }

        @media (max-width: 1200px) {
          .performance-grid {
            grid-template-columns: repeat(3, 1fr);
          }
          .tabs-filters-row {
            flex-direction: column;
            align-items: flex-start;
          }
          .filters-inline {
            flex-wrap: wrap;
          }
        }

        @media (max-width: 768px) {
          .top-section {
            flex-direction: column;
          }
          .performance-grid {
            grid-template-columns: repeat(2, 1fr);
          }
          .accuracy-grid {
            grid-template-columns: repeat(2, 1fr);
          }
          .tab-navigation {
            flex-direction: column;
            width: 100%;
          }
          .tab-btn {
            width: 100%;
          }
        }
      `}</style>
    </div>
  );
}

export default ShadowModePanel;
