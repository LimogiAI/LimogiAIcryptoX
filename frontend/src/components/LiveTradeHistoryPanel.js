// ============================================
// LimogiAICryptoX - Live Trade History Panel
// Full live trade history with filters and statistics
// ============================================

import React, { useState, useEffect, useCallback } from 'react';
import { api } from '../services/api';

export function LiveTradeHistoryPanel() {
  const [trades, setTrades] = useState([]);
  const [filteredTrades, setFilteredTrades] = useState([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  
  // Filters
  const [statusFilter, setStatusFilter] = useState('all'); // all, win, loss, failed, partial
  const [dateFilter, setDateFilter] = useState('all'); // all, today, week, month
  const [sortBy, setSortBy] = useState('time'); // time, profit, fee, slippage, latency
  const [sortOrder, setSortOrder] = useState('desc'); // asc, desc
  
  // Pagination
  const [currentPage, setCurrentPage] = useState(1);
  const tradesPerPage = 20;
  
  // Most Traded Paths
  const [pathSortBy, setPathSortBy] = useState('frequency'); // frequency, profit, winrate
  const [pathPage, setPathPage] = useState(1);
  const pathsPerPage = 5;
  
  // Expanded row for leg details
  const [expandedTradeId, setExpandedTradeId] = useState(null);

  // Fetch trades from live trading endpoint
  const fetchTrades = useCallback(async () => {
    try {
      setError(null);
      // Fetch up to 500 trades (API max), with 720 hours (30 days) range
      const data = await api.getLiveTrades(500, null, 720);
      setTrades(data.trades || []);
    } catch (err) {
      console.error('Error fetching live trades:', err);
      setError(err.message);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchTrades();
    const interval = setInterval(fetchTrades, 10000); // Refresh every 10s
    return () => clearInterval(interval);
  }, [fetchTrades]);

  // Apply filters
  useEffect(() => {
    let result = [...trades];

    // Status filter
    if (statusFilter === 'win') {
      result = result.filter(t => t.status === 'COMPLETED' && t.profit_loss >= 0);
    } else if (statusFilter === 'loss') {
      result = result.filter(t => t.status === 'COMPLETED' && t.profit_loss < 0);
    } else if (statusFilter === 'failed') {
      result = result.filter(t => t.status === 'FAILED');
    } else if (statusFilter === 'partial') {
      result = result.filter(t => t.status === 'PARTIAL');
    }

    // Date filter
    const now = new Date();
    if (dateFilter === 'today') {
      const today = now.toDateString();
      result = result.filter(t => new Date(t.started_at).toDateString() === today);
    } else if (dateFilter === 'week') {
      const weekAgo = new Date(now.getTime() - 7 * 24 * 60 * 60 * 1000);
      result = result.filter(t => new Date(t.started_at) >= weekAgo);
    } else if (dateFilter === 'month') {
      const monthAgo = new Date(now.getTime() - 30 * 24 * 60 * 60 * 1000);
      result = result.filter(t => new Date(t.started_at) >= monthAgo);
    }

    // Sort
    result.sort((a, b) => {
      let valA, valB;
      if (sortBy === 'time') {
        valA = new Date(a.started_at);
        valB = new Date(b.started_at);
      } else if (sortBy === 'profit') {
        valA = a.profit_loss || 0;
        valB = b.profit_loss || 0;
      } else if (sortBy === 'fee') {
        valA = calculateTotalFee(a) || 0;
        valB = calculateTotalFee(b) || 0;
      } else if (sortBy === 'slippage') {
        valA = calculateTotalSlippage(a).pct || 0;
        valB = calculateTotalSlippage(b).pct || 0;
      } else if (sortBy === 'latency') {
        valA = a.total_execution_ms || 0;
        valB = b.total_execution_ms || 0;
      }
      return sortOrder === 'desc' ? valB - valA : valA - valB;
    });

    setFilteredTrades(result);
    setCurrentPage(1);
  }, [trades, statusFilter, dateFilter, sortBy, sortOrder]);

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

  // Calculate statistics
  const stats = {
    total: filteredTrades.length,
    wins: filteredTrades.filter(t => t.status === 'COMPLETED' && t.profit_loss >= 0).length,
    losses: filteredTrades.filter(t => t.status === 'COMPLETED' && t.profit_loss < 0).length,
    failed: filteredTrades.filter(t => t.status === 'FAILED').length,
    partial: filteredTrades.filter(t => t.status === 'PARTIAL').length,
    totalProfit: filteredTrades.reduce((sum, t) => sum + (t.profit_loss > 0 ? t.profit_loss : 0), 0),
    totalLoss: filteredTrades.reduce((sum, t) => sum + (t.profit_loss < 0 ? Math.abs(t.profit_loss) : 0), 0),
    netPnL: filteredTrades.reduce((sum, t) => sum + (t.profit_loss || 0), 0),
    totalFees: filteredTrades.reduce((sum, t) => sum + (calculateTotalFee(t) || 0), 0),
    avgLatency: filteredTrades.length > 0
      ? filteredTrades.reduce((sum, t) => sum + (t.total_execution_ms || 0), 0) / filteredTrades.length
      : 0,
    winRate: filteredTrades.filter(t => t.status === 'COMPLETED').length > 0
      ? (filteredTrades.filter(t => t.status === 'COMPLETED' && t.profit_loss >= 0).length / 
         filteredTrades.filter(t => t.status === 'COMPLETED').length * 100)
      : 0,
  };

  // Calculate Most Traded Paths
  const getPathStats = () => {
    const pathMap = {};
    
    trades.forEach(trade => {
      const path = trade.path;
      if (!path) return;
      
      if (!pathMap[path]) {
        pathMap[path] = {
          path,
          count: 0,
          wins: 0,
          losses: 0,
          totalProfit: 0,
          totalFees: 0,
          totalLatency: 0,
        };
      }
      
      pathMap[path].count++;
      if (trade.status === 'COMPLETED' && trade.profit_loss >= 0) {
        pathMap[path].wins++;
      } else if (trade.status === 'COMPLETED' && trade.profit_loss < 0) {
        pathMap[path].losses++;
      }
      pathMap[path].totalProfit += trade.profit_loss || 0;
      pathMap[path].totalFees += calculateTotalFee(trade) || 0;
      pathMap[path].totalLatency += trade.total_execution_ms || 0;
    });
    
    let pathArray = Object.values(pathMap).map(p => ({
      ...p,
      winRate: p.count > 0 ? ((p.wins / p.count) * 100) : 0,
      avgLatency: p.count > 0 ? (p.totalLatency / p.count) : 0,
    }));
    
    // Sort based on selected criteria
    if (pathSortBy === 'frequency') {
      pathArray.sort((a, b) => b.count - a.count);
    } else if (pathSortBy === 'profit') {
      pathArray.sort((a, b) => b.totalProfit - a.totalProfit);
    } else if (pathSortBy === 'winrate') {
      pathArray.sort((a, b) => b.winRate - a.winRate);
    }
    
    return pathArray;
  };

  const pathStats = getPathStats();
  const totalPathPages = Math.ceil(pathStats.length / pathsPerPage);
  const paginatedPaths = pathStats.slice((pathPage - 1) * pathsPerPage, pathPage * pathsPerPage);

  // Toggle expanded row
  const toggleExpandRow = (tradeId) => {
    setExpandedTradeId(expandedTradeId === tradeId ? null : tradeId);
  };

  // Format timestamp
  const formatTime = (timestamp) => {
    if (!timestamp && timestamp !== 0) return '--';
    try {
      let date;
      if (typeof timestamp === 'number') {
        date = new Date(timestamp);
      } else {
        let ts = timestamp.endsWith('Z') || timestamp.includes('+') ? timestamp : timestamp + 'Z';
        date = new Date(ts);
      }
      if (isNaN(date.getTime())) return '--';
      return date.toLocaleString('en-US', {
        month: 'short',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
        hour12: true
      });
    } catch { return '--'; }
  };

  const formatCurrency = (value) => {
    if (value === null || value === undefined) return '--';
    const num = parseFloat(value);
    if (isNaN(num)) return '--';
    if (num >= 0) {
      return `+$${num.toFixed(2)}`;
    } else {
      return `-$${Math.abs(num).toFixed(2)}`;
    }
  };

  // Export to CSV
  const handleExportCSV = () => {
    if (filteredTrades.length === 0) {
      alert('No trades to export');
      return;
    }
    
    const headers = ['Time', 'Path', 'Amount', 'Taker Fee', 'Slippage', 'Latency (ms)', 'Profit/Loss', 'Status'];
    const rows = filteredTrades.map(t => {
      const totalFee = calculateTotalFee(t);
      const slippage = calculateTotalSlippage(t);
      return [
        formatTime(t.started_at),
        `"${t.path || ''}"`,
        t.amount_in?.toFixed(2) || '--',
        totalFee ? `"-$${totalFee.toFixed(4)}"` : '--',
        slippage.usd !== null ? `"-$${slippage.usd.toFixed(4)}"` : '--',
        t.total_execution_ms?.toFixed(0) || '--',
        t.profit_loss !== null ? `"${t.profit_loss >= 0 ? '+' : ''}$${t.profit_loss?.toFixed(4)}"` : '--',
        t.status || '--'
      ].join(',');
    });
    
    const csvContent = [headers.join(','), ...rows].join('\n');
    const blob = new Blob([csvContent], { type: 'text/csv' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `live_trade_history_${new Date().toISOString().split('T')[0]}.csv`;
    a.click();
    URL.revokeObjectURL(url);
  };

  // Pagination
  const totalPages = Math.ceil(filteredTrades.length / tradesPerPage);
  const paginatedTrades = filteredTrades.slice((currentPage - 1) * tradesPerPage, currentPage * tradesPerPage);

  if (loading) {
    return (
      <div className="panel live-trade-history-panel loading">
        <p>Loading live trade history...</p>
      </div>
    );
  }

  return (
    <div className="panel live-trade-history-panel">
      {error && (
        <div className="error-message">
          ⚠️ {error}
          <button onClick={() => setError(null)}>×</button>
        </div>
      )}

      {/* Statistics Summary */}
      <div className="stats-section">
        <h3>📊 Trade Statistics</h3>
        <div className="stats-grid">
          <div className="stat-card">
            <span className="stat-label">Total Trades</span>
            <span className="stat-value">{stats.total}</span>
          </div>
          <div className="stat-card positive">
            <span className="stat-label">Wins</span>
            <span className="stat-value">{stats.wins}</span>
          </div>
          <div className="stat-card negative">
            <span className="stat-label">Losses</span>
            <span className="stat-value">{stats.losses}</span>
          </div>
          <div className="stat-card">
            <span className="stat-label">Win Rate</span>
            <span className="stat-value">{stats.winRate.toFixed(1)}%</span>
          </div>
          <div className={`stat-card ${stats.netPnL >= 0 ? 'positive' : 'negative'}`}>
            <span className="stat-label">Net P/L</span>
            <span className="stat-value">{formatCurrency(stats.netPnL)}</span>
          </div>
          <div className="stat-card">
            <span className="stat-label">Total Fees</span>
            <span className="stat-value negative-text">-${stats.totalFees.toFixed(2)}</span>
          </div>
          <div className="stat-card">
            <span className="stat-label">Avg Latency</span>
            <span className="stat-value">{stats.avgLatency.toFixed(0)}ms</span>
          </div>
          <div className="stat-card warning">
            <span className="stat-label">Failed/Partial</span>
            <span className="stat-value">{stats.failed + stats.partial}</span>
          </div>
        </div>
      </div>

      {/* Filters */}
      <div className="filters-section">
        <div className="filters-header">
          <h3>🔍 Filters</h3>
          <button className="export-btn" onClick={handleExportCSV}>📥 Export CSV</button>
        </div>
        <div className="filters-grid">
          <div className="filter-group">
            <label>Status</label>
            <select value={statusFilter} onChange={(e) => setStatusFilter(e.target.value)}>
              <option value="all">All Trades</option>
              <option value="win">Wins Only</option>
              <option value="loss">Losses Only</option>
              <option value="failed">Failed</option>
              <option value="partial">Partial</option>
            </select>
          </div>
          <div className="filter-group">
            <label>Time Period</label>
            <select value={dateFilter} onChange={(e) => setDateFilter(e.target.value)}>
              <option value="all">All Time</option>
              <option value="today">Today</option>
              <option value="week">Last 7 Days</option>
              <option value="month">Last 30 Days</option>
            </select>
          </div>
          <div className="filter-group">
            <label>Sort By</label>
            <select value={sortBy} onChange={(e) => setSortBy(e.target.value)}>
              <option value="time">Time</option>
              <option value="profit">Profit/Loss</option>
              <option value="fee">Taker Fee</option>
              <option value="slippage">Slippage</option>
              <option value="latency">Latency</option>
            </select>
          </div>
          <div className="filter-group">
            <label>Order</label>
            <select value={sortOrder} onChange={(e) => setSortOrder(e.target.value)}>
              <option value="desc">Descending</option>
              <option value="asc">Ascending</option>
            </select>
          </div>
        </div>
      </div>

      {/* Most Traded Paths */}
      {pathStats.length > 0 && (
        <div className="paths-section">
          <div className="paths-header">
            <h3>🔥 Most Traded Paths</h3>
            <div className="path-sort">
              <label>Sort:</label>
              <select value={pathSortBy} onChange={(e) => { setPathSortBy(e.target.value); setPathPage(1); }}>
                <option value="frequency">Frequency</option>
                <option value="profit">Total Profit</option>
                <option value="winrate">Win Rate</option>
              </select>
            </div>
          </div>
          
          <div className="paths-list">
            {paginatedPaths.map((p, idx) => (
              <div key={idx} className="path-item">
                <code className="path-code">{p.path}</code>
                <div className="path-stats">
                  <span className="path-count">{p.count}x</span>
                  <span className="path-winrate">{p.winRate.toFixed(0)}% win</span>
                  <span className={`path-profit ${p.totalProfit >= 0 ? 'positive' : 'negative'}`}>
                    {formatCurrency(p.totalProfit)}
                  </span>
                  <span className="path-latency">{p.avgLatency.toFixed(0)}ms avg</span>
                </div>
              </div>
            ))}
          </div>
          
          {totalPathPages > 1 && (
            <div className="path-pagination">
              <button onClick={() => setPathPage(p => Math.max(1, p - 1))} disabled={pathPage === 1}>← Prev</button>
              <span>Page {pathPage} of {totalPathPages}</span>
              <button onClick={() => setPathPage(p => Math.min(totalPathPages, p + 1))} disabled={pathPage >= totalPathPages}>Next →</button>
            </div>
          )}
        </div>
      )}

      {/* Trade History Table */}
      <div className="trades-section">
        <h3>📜 All Live Trades ({filteredTrades.length} trades)</h3>
        
        {filteredTrades.length === 0 ? (
          <div className="empty-state">
            <p>No live trades found matching your filters.</p>
            <p className="hint">Live trades will appear here when executed.</p>
          </div>
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
                          <td className="time-cell">{formatTime(trade.started_at)}</td>
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
                <button onClick={() => setCurrentPage(1)} disabled={currentPage === 1}>««</button>
                <button onClick={() => setCurrentPage(p => Math.max(1, p - 1))} disabled={currentPage === 1}>«</button>
                <span className="page-info">
                  Page {currentPage} of {totalPages}
                  <span className="page-detail">
                    ({(currentPage - 1) * tradesPerPage + 1}-{Math.min(currentPage * tradesPerPage, filteredTrades.length)} of {filteredTrades.length})
                  </span>
                </span>
                <button onClick={() => setCurrentPage(p => Math.min(totalPages, p + 1))} disabled={currentPage === totalPages}>»</button>
                <button onClick={() => setCurrentPage(totalPages)} disabled={currentPage === totalPages}>»»</button>
              </div>
            )}
          </>
        )}
      </div>

      <style>{`
        .live-trade-history-panel {
          padding: 20px;
          background: linear-gradient(180deg, #0d0d1a 0%, #1a1a2e 100%);
          min-height: calc(100vh - 200px);
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

        /* Statistics Section */
        .stats-section {
          background: linear-gradient(135deg, #1a1a2e, #252545);
          border: 1px solid #3a3a5a;
          border-radius: 16px;
          padding: 25px;
          margin-bottom: 20px;
        }

        .stats-section h3 {
          color: #00d4aa;
          margin-bottom: 20px;
          font-size: 1.2rem;
        }

        .stats-grid {
          display: grid;
          grid-template-columns: repeat(8, 1fr);
          gap: 15px;
        }

        @media (max-width: 1400px) {
          .stats-grid { grid-template-columns: repeat(4, 1fr); }
        }

        @media (max-width: 800px) {
          .stats-grid { grid-template-columns: repeat(2, 1fr); }
        }

        .stat-card {
          background: linear-gradient(135deg, #252542, #2a2a50);
          border: 1px solid #3a3a5a;
          border-radius: 10px;
          padding: 15px;
          text-align: center;
        }

        .stat-card.positive { border-left: 3px solid #00d4aa; }
        .stat-card.negative { border-left: 3px solid #ff6b6b; }
        .stat-card.warning { border-left: 3px solid #f0ad4e; }

        .stat-card .stat-label {
          display: block;
          color: #a0a0b0;
          font-size: 0.8rem;
          margin-bottom: 5px;
          text-transform: uppercase;
        }

        .stat-card .stat-value {
          display: block;
          color: #fff;
          font-size: 1.3rem;
          font-weight: 700;
        }

        .stat-card.positive .stat-value { color: #00d4aa; }
        .stat-card.negative .stat-value { color: #ff6b6b; }
        .negative-text { color: #ff6b6b; }

        /* Filters Section */
        .filters-section {
          background: linear-gradient(135deg, #1a1a2e, #252545);
          border: 1px solid #3a3a5a;
          border-radius: 16px;
          padding: 25px;
          margin-bottom: 20px;
        }

        .filters-header {
          display: flex;
          justify-content: space-between;
          align-items: center;
          margin-bottom: 20px;
        }

        .filters-header h3 {
          color: #00d4aa;
          margin: 0;
          font-size: 1.2rem;
        }

        .export-btn {
          background: linear-gradient(135deg, #6c5ce7, #a29bfe);
          color: white;
          border: none;
          padding: 10px 20px;
          border-radius: 8px;
          font-weight: 600;
          cursor: pointer;
        }

        .export-btn:hover { opacity: 0.9; }

        .filters-grid {
          display: grid;
          grid-template-columns: repeat(4, 1fr);
          gap: 20px;
        }

        @media (max-width: 800px) {
          .filters-grid { grid-template-columns: repeat(2, 1fr); }
        }

        .filter-group {
          display: flex;
          flex-direction: column;
          gap: 8px;
        }

        .filter-group label {
          color: #a0a0b0;
          font-size: 0.85rem;
          text-transform: uppercase;
        }

        .filter-group select {
          background: linear-gradient(135deg, #252542, #2a2a50);
          border: 1px solid #3a3a5a;
          border-radius: 8px;
          color: #fff;
          padding: 10px 15px;
          font-size: 0.95rem;
          cursor: pointer;
        }

        .filter-group select option {
          background: #1a1a2e;
          color: #fff;
        }

        /* Most Traded Paths Section */
        .paths-section {
          background: linear-gradient(135deg, #1a1a2e, #252545);
          border: 1px solid #3a3a5a;
          border-radius: 16px;
          padding: 25px;
          margin-bottom: 20px;
        }

        .paths-header {
          display: flex;
          justify-content: space-between;
          align-items: center;
          margin-bottom: 20px;
        }

        .paths-header h3 {
          color: #00d4aa;
          margin: 0;
          font-size: 1.2rem;
        }

        .path-sort {
          display: flex;
          align-items: center;
          gap: 10px;
        }

        .path-sort label {
          color: #a0a0b0;
          font-size: 0.9rem;
        }

        .path-sort select {
          background: linear-gradient(135deg, #252542, #2a2a50);
          border: 1px solid #3a3a5a;
          border-radius: 8px;
          color: #fff;
          padding: 8px 12px;
          cursor: pointer;
        }

        .paths-list {
          display: flex;
          flex-direction: column;
          gap: 10px;
        }

        .path-item {
          background: linear-gradient(135deg, #252542, #2a2a50);
          border: 1px solid #3a3a5a;
          border-radius: 10px;
          padding: 15px 20px;
          display: flex;
          justify-content: space-between;
          align-items: center;
          flex-wrap: wrap;
          gap: 10px;
        }

        .path-code {
          background: #333;
          padding: 6px 12px;
          border-radius: 6px;
          font-size: 0.9rem;
          color: #fff;
        }

        .path-stats {
          display: flex;
          gap: 20px;
          align-items: center;
        }

        .path-count { color: #00d4aa; font-weight: 600; }
        .path-winrate { color: #a0a0b0; }
        .path-profit.positive { color: #00d4aa; font-weight: 600; }
        .path-profit.negative { color: #ff6b6b; font-weight: 600; }
        .path-latency { color: #888; font-size: 0.85rem; }

        .path-pagination {
          display: flex;
          justify-content: center;
          align-items: center;
          gap: 15px;
          margin-top: 15px;
        }

        .path-pagination button {
          background: linear-gradient(135deg, #252542, #2a2a50);
          border: 1px solid #3a3a5a;
          color: #fff;
          padding: 8px 15px;
          border-radius: 6px;
          cursor: pointer;
        }

        .path-pagination button:disabled {
          opacity: 0.5;
          cursor: not-allowed;
        }

        .path-pagination span {
          color: #a0a0b0;
          font-size: 0.9rem;
        }

        /* Trades Section */
        .trades-section {
          background: linear-gradient(135deg, #1a1a2e, #252545);
          border: 1px solid #3a3a5a;
          border-radius: 16px;
          padding: 25px;
        }

        .trades-section h3 {
          color: #00d4aa;
          margin-bottom: 20px;
          font-size: 1.2rem;
        }

        .empty-state {
          text-align: center;
          padding: 50px;
          color: #888;
        }

        .empty-state .hint {
          font-size: 0.9rem;
          margin-top: 10px;
          color: #666;
        }

        /* Trades Table */
        .trades-table-container { overflow-x: auto; }
        
        .trades-table { width: 100%; border-collapse: collapse; }
        
        .trades-table th {
          background: linear-gradient(135deg, #00d4aa, #00b894);
          color: #1a1a2e;
          padding: 14px 12px;
          text-align: left;
          font-weight: 700;
          text-transform: uppercase;
          font-size: 0.8rem;
          white-space: nowrap;
        }
        
        .trades-table td {
          padding: 14px 12px;
          border-bottom: 1px solid #2a2a4a;
          color: #fff;
          vertical-align: middle;
        }
        
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
        
        .badge {
          padding: 6px 14px;
          border-radius: 20px;
          font-size: 0.75rem;
          font-weight: 700;
          text-transform: uppercase;
          white-space: nowrap;
        }
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
        .pagination {
          display: flex;
          justify-content: center;
          align-items: center;
          gap: 10px;
          margin-top: 20px;
          padding-top: 20px;
          border-top: 1px solid #3a3a5a;
        }

        .pagination button {
          background: linear-gradient(135deg, #252542, #2a2a50);
          border: 1px solid #3a3a5a;
          color: #fff;
          padding: 8px 15px;
          border-radius: 6px;
          cursor: pointer;
          font-weight: 600;
        }

        .pagination button:hover:not(:disabled) {
          border-color: #00d4aa;
          color: #00d4aa;
        }

        .pagination button:disabled {
          opacity: 0.5;
          cursor: not-allowed;
        }

        .page-info {
          color: #a0a0b0;
          font-size: 0.9rem;
          padding: 0 15px;
        }

        .page-detail {
          color: #666;
          font-size: 0.8rem;
          margin-left: 10px;
        }
      `}</style>
    </div>
  );
}

export default LiveTradeHistoryPanel;