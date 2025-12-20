// ============================================
// KrakenCryptoX v2.0 - Trade History Panel
// Full trade history with filters and statistics
// ============================================

import React, { useState, useEffect, useCallback } from 'react';
import { api } from '../services/api';

export function TradeHistoryPanel() {
  const [trades, setTrades] = useState([]);
  const [filteredTrades, setFilteredTrades] = useState([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  
  // Filters
  const [statusFilter, setStatusFilter] = useState('all'); // all, win, loss
  const [dateFilter, setDateFilter] = useState('all'); // all, today, week
  const [sortBy, setSortBy] = useState('time'); // time, profit, slippage
  const [sortOrder, setSortOrder] = useState('desc'); // asc, desc
  
  // Pagination
  const [currentPage, setCurrentPage] = useState(1);
  const tradesPerPage = 50;
  
  // Most Traded Paths
  const [pathSortBy, setPathSortBy] = useState('frequency'); // frequency, profit, winrate
  const [pathPage, setPathPage] = useState(1);
  const pathsPerPage = 5;

  // Fetch trades
  const fetchTrades = useCallback(async () => {
    try {
      setError(null);
      const data = await api.getPaperTrades(500);
      setTrades(data.trades || data || []);
    } catch (err) {
      console.error('Error fetching trades:', err);
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
      result = result.filter(t => t.actual_profit_amount > 0);
    } else if (statusFilter === 'loss') {
      result = result.filter(t => t.actual_profit_amount <= 0);
    }

    // Date filter
    if (dateFilter === 'today') {
      const today = new Date().toDateString();
      result = result.filter(t => new Date(t.executed_at).toDateString() === today);
    } else if (dateFilter === 'week') {
      const weekAgo = new Date();
      weekAgo.setDate(weekAgo.getDate() - 7);
      result = result.filter(t => new Date(t.executed_at) >= weekAgo);
    }

    // Sort
    result.sort((a, b) => {
      let valA, valB;
      if (sortBy === 'time') {
        valA = new Date(a.executed_at);
        valB = new Date(b.executed_at);
      } else if (sortBy === 'profit') {
        valA = a.actual_profit_amount;
        valB = b.actual_profit_amount;
      } else if (sortBy === 'slippage') {
        valA = Math.abs(a.slippage_pct);
        valB = Math.abs(b.slippage_pct);
      }
      return sortOrder === 'desc' ? valB - valA : valA - valB;
    });

    setFilteredTrades(result);
    setCurrentPage(1); // Reset to first page when filters change
  }, [trades, statusFilter, dateFilter, sortBy, sortOrder]);

  // Calculate statistics
  const stats = {
    total: filteredTrades.length,
    wins: filteredTrades.filter(t => t.actual_profit_amount > 0).length,
    losses: filteredTrades.filter(t => t.actual_profit_amount <= 0).length,
    totalProfit: filteredTrades.reduce((sum, t) => sum + (t.actual_profit_amount || 0), 0),
    avgProfit: filteredTrades.length > 0 
      ? filteredTrades.reduce((sum, t) => sum + (t.actual_profit_amount || 0), 0) / filteredTrades.length 
      : 0,
    avgSlippage: filteredTrades.length > 0
      ? filteredTrades.reduce((sum, t) => sum + Math.abs(t.slippage_pct || 0), 0) / filteredTrades.length
      : 0,
    winRate: filteredTrades.length > 0
      ? (filteredTrades.filter(t => t.actual_profit_amount > 0).length / filteredTrades.length * 100)
      : 0,
  };

  // Calculate Most Traded Paths from actual trades
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
          totalProfit: 0,
        };
      }
      
      pathMap[path].count++;
      if (trade.actual_profit_amount > 0) {
        pathMap[path].wins++;
      }
      pathMap[path].totalProfit += trade.actual_profit_amount || 0;
    });
    
    // Convert to array and calculate win rate
    let pathArray = Object.values(pathMap).map(p => ({
      ...p,
      winRate: p.count > 0 ? (p.wins / p.count * 100) : 0,
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

  const formatTime = (timestamp) => {
    if (!timestamp) return '--';
    try {
      let ts = timestamp;
      if (!timestamp.endsWith('Z') && !timestamp.includes('+')) {
        ts = timestamp + 'Z';
      }
      return new Date(ts).toLocaleString('en-US', {
        timeZone: 'America/New_York',
        month: 'short',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit',
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
    if (num >= 0) {
      return `+$${num.toFixed(2)}`;
    } else {
      return `-$${Math.abs(num).toFixed(2)}`;
    }
  };

  if (loading) {
    return (
      <div className="panel trade-history-panel loading">
        <p>Loading trade history...</p>
      </div>
    );
  }

  return (
    <div className="panel trade-history-panel">
      {error && (
        <div className="error-message">
          ⚠️ {error}
          <button onClick={() => setError(null)}>×</button>
        </div>
      )}

      {/* Statistics Summary */}
      <div className="history-stats-section">
        <h3>📊 Trade Statistics</h3>
        <div className="history-stats-grid">
          <div className="history-stat-card">
            <span className="stat-label">Total Trades</span>
            <span className="stat-value">{stats.total}</span>
          </div>
          <div className="history-stat-card positive">
            <span className="stat-label">Wins</span>
            <span className="stat-value">{stats.wins}</span>
          </div>
          <div className="history-stat-card negative">
            <span className="stat-label">Losses</span>
            <span className="stat-value">{stats.losses}</span>
          </div>
          <div className="history-stat-card">
            <span className="stat-label">Win Rate</span>
            <span className="stat-value">{stats.winRate.toFixed(1)}%</span>
          </div>
          <div className={`history-stat-card ${stats.totalProfit >= 0 ? 'positive' : 'negative'}`}>
            <span className="stat-label">Total P/L</span>
            <span className="stat-value">{formatCurrency(stats.totalProfit)}</span>
          </div>
          <div className="history-stat-card">
            <span className="stat-label">Avg Slippage</span>
            <span className="stat-value negative-text">-{stats.avgSlippage.toFixed(2)}%</span>
          </div>
        </div>
      </div>

      {/* Filters */}
      <div className="history-filters-section">
        <h3>🔍 Filters</h3>
        <div className="filters-grid">
          <div className="filter-group">
            <label>Status</label>
            <select value={statusFilter} onChange={(e) => setStatusFilter(e.target.value)}>
              <option value="all">All Trades</option>
              <option value="win">Wins Only</option>
              <option value="loss">Losses Only</option>
            </select>
          </div>
          <div className="filter-group">
            <label>Time Period</label>
            <select value={dateFilter} onChange={(e) => setDateFilter(e.target.value)}>
              <option value="all">All Time</option>
              <option value="today">Today</option>
              <option value="week">Last 7 Days</option>
            </select>
          </div>
          <div className="filter-group">
            <label>Sort By</label>
            <select value={sortBy} onChange={(e) => setSortBy(e.target.value)}>
              <option value="time">Time</option>
              <option value="profit">Profit</option>
              <option value="slippage">Slippage</option>
            </select>
          </div>
          <div className="filter-group">
            <label>Order</label>
            <select value={sortOrder} onChange={(e) => setSortOrder(e.target.value)}>
              <option value="desc">Newest First</option>
              <option value="asc">Oldest First</option>
            </select>
          </div>
        </div>
      </div>

      {/* Most Traded Paths Section */}
      {pathStats.length > 0 && (
        <div className="most-traded-section">
          <div className="most-traded-header">
            <h3>🔥 Most Traded Paths</h3>
            <div className="path-sort-filter">
              <label>Sort by:</label>
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
                    {p.totalProfit >= 0 ? '+' : ''}{formatCurrency(p.totalProfit)}
                  </span>
                </div>
              </div>
            ))}
          </div>
          
          {/* Path Pagination */}
          {totalPathPages > 1 && (
            <div className="path-pagination">
              <button 
                className="pagination-btn-small"
                onClick={() => setPathPage(p => Math.max(1, p - 1))}
                disabled={pathPage === 1}
              >
                ← Prev
              </button>
              <span className="pagination-info-small">
                Page {pathPage} of {totalPathPages}
              </span>
              <button 
                className="pagination-btn-small"
                onClick={() => setPathPage(p => Math.min(totalPathPages, p + 1))}
                disabled={pathPage >= totalPathPages}
              >
                Next →
              </button>
            </div>
          )}
        </div>
      )}

      {/* Trade History Table */}
      <div className="history-table-section">
        <h3>📜 Trade History ({filteredTrades.length} trades)</h3>
        <div className="trades-table-container">
          {filteredTrades.length === 0 ? (
            <div className="empty-state">
              <p>No trades found matching your filters.</p>
            </div>
          ) : (
            <>
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
                  {filteredTrades
                    .slice((currentPage - 1) * tradesPerPage, currentPage * tradesPerPage)
                    .map((trade, index) => (
                    <tr key={trade.id || index} className={trade.actual_profit_amount > 0 ? 'win' : 'loss'}>
                      <td>{formatTime(trade.executed_at)}</td>
                      <td><code>{trade.path}</code></td>
                      <td>${trade.trade_amount?.toFixed(2)}</td>
                      <td className="positive-text">{formatPct(trade.expected_net_profit_pct)}</td>
                      <td className="negative-text">{formatPct(trade.slippage_pct)}</td>
                      <td className={trade.actual_net_profit_pct >= 0 ? 'positive-text' : 'negative-text'}>
                        {formatPct(trade.actual_net_profit_pct)}
                      </td>
                      <td className={trade.actual_profit_amount >= 0 ? 'positive-text' : 'negative-text'}>
                        {formatCurrency(trade.actual_profit_amount)}
                      </td>
                      <td>
                        <span className={`badge ${trade.actual_profit_amount > 0 ? 'win' : 'loss'}`}>
                          {trade.actual_profit_amount > 0 ? '✓ Win' : '✗ Loss'}
                        </span>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
              
              {/* Pagination Controls */}
              {filteredTrades.length > tradesPerPage && (
                <div className="pagination-controls">
                  <button 
                    className="pagination-btn"
                    onClick={() => setCurrentPage(p => Math.max(1, p - 1))}
                    disabled={currentPage === 1}
                  >
                    ← Previous
                  </button>
                  <span className="pagination-info">
                    Page {currentPage} of {Math.ceil(filteredTrades.length / tradesPerPage)}
                    <span className="pagination-detail">
                      (Showing {(currentPage - 1) * tradesPerPage + 1}-{Math.min(currentPage * tradesPerPage, filteredTrades.length)} of {filteredTrades.length})
                    </span>
                  </span>
                  <button 
                    className="pagination-btn"
                    onClick={() => setCurrentPage(p => Math.min(Math.ceil(filteredTrades.length / tradesPerPage), p + 1))}
                    disabled={currentPage >= Math.ceil(filteredTrades.length / tradesPerPage)}
                  >
                    Next →
                  </button>
                </div>
              )}
            </>
          )}
        </div>
      </div>

      <style jsx>{`
        .trade-history-panel {
          padding: 20px;
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
        .history-stats-section {
          background: #1a1a2e;
          border-radius: 12px;
          padding: 20px;
          margin-bottom: 20px;
        }

        .history-stats-section h3 {
          color: #00d4aa;
          margin-bottom: 15px;
        }

        .history-stats-grid {
          display: grid;
          grid-template-columns: repeat(6, 1fr);
          gap: 15px;
        }

        @media (max-width: 1200px) {
          .history-stats-grid {
            grid-template-columns: repeat(3, 1fr);
          }
        }

        @media (max-width: 600px) {
          .history-stats-grid {
            grid-template-columns: repeat(2, 1fr);
          }
        }

        .history-stat-card {
          background: #252542;
          border-radius: 10px;
          padding: 15px;
          text-align: center;
        }

        .history-stat-card.positive {
          border-left: 3px solid #00d4aa;
        }

        .history-stat-card.negative {
          border-left: 3px solid #ff6b6b;
        }

        .history-stat-card .stat-label {
          display: block;
          color: #888;
          font-size: 0.8rem;
          margin-bottom: 5px;
        }

        .history-stat-card .stat-value {
          display: block;
          color: #fff;
          font-size: 1.3rem;
          font-weight: 700;
        }

        .history-stat-card.positive .stat-value {
          color: #00d4aa;
        }

        .history-stat-card.negative .stat-value {
          color: #ff6b6b;
        }

        /* Filters Section */
        .history-filters-section {
          background: #1a1a2e;
          border-radius: 12px;
          padding: 20px;
          margin-bottom: 20px;
        }

        .history-filters-section h3 {
          color: #00d4aa;
          margin-bottom: 15px;
        }

        .filters-grid {
          display: grid;
          grid-template-columns: repeat(4, 1fr);
          gap: 15px;
        }

        @media (max-width: 800px) {
          .filters-grid {
            grid-template-columns: repeat(2, 1fr);
          }
        }

        .filter-group {
          display: flex;
          flex-direction: column;
          gap: 8px;
        }

        .filter-group label {
          color: #888;
          font-size: 0.85rem;
        }

        .filter-group select {
          background: #252542;
          border: 1px solid #3a3a5a;
          border-radius: 8px;
          color: #fff;
          padding: 10px 12px;
          font-size: 0.95rem;
          cursor: pointer;
        }

        .filter-group select:hover {
          border-color: #00d4aa;
        }

        .filter-group select:focus {
          outline: none;
          border-color: #00d4aa;
        }

        /* Table Section */
        .history-table-section {
          background: #1a1a2e;
          border-radius: 12px;
          padding: 20px;
        }

        .history-table-section h3 {
          color: #00d4aa;
          margin-bottom: 15px;
        }

        .trades-table-container {
          overflow-x: auto;
          max-height: 600px;
          overflow-y: auto;
        }

        .trades-table {
          width: 100%;
          border-collapse: collapse;
          min-width: 900px;
        }

        .trades-table th {
          background: #00d4aa;
          color: #1a1a2e;
          padding: 12px 8px;
          text-align: left;
          font-weight: 600;
          position: sticky;
          top: 0;
          z-index: 1;
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

        .trades-table tr:hover {
          background: rgba(255, 255, 255, 0.05);
        }

        .trades-table code {
          background: #2a2a4a;
          padding: 4px 8px;
          border-radius: 4px;
          font-size: 0.85rem;
        }

        .positive-text {
          color: #00d4aa !important;
        }

        .negative-text {
          color: #ff6b6b !important;
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

        /* Pagination */
        .pagination-controls {
          display: flex;
          justify-content: center;
          align-items: center;
          gap: 20px;
          padding: 20px;
          border-top: 1px solid #2a2a4a;
          margin-top: 10px;
        }

        .pagination-btn {
          background: #252542;
          border: 1px solid #3a3a5a;
          color: #fff;
          padding: 10px 20px;
          border-radius: 8px;
          cursor: pointer;
          font-size: 0.9rem;
          transition: all 0.2s;
        }

        .pagination-btn:hover:not(:disabled) {
          border-color: #00d4aa;
          background: #2a2a4a;
        }

        .pagination-btn:disabled {
          color: #555;
          cursor: not-allowed;
          border-color: #2a2a4a;
        }

        .pagination-info {
          color: #fff;
          font-size: 0.95rem;
        }

        .pagination-detail {
          color: #888;
          font-size: 0.85rem;
          margin-left: 10px;
        }

        /* Most Traded Paths */
        .most-traded-section {
          background: #1a1a2e;
          border-radius: 12px;
          padding: 20px;
          margin-bottom: 20px;
        }

        .most-traded-header {
          display: flex;
          justify-content: space-between;
          align-items: center;
          margin-bottom: 15px;
        }

        .most-traded-header h3 {
          color: #00d4aa;
          margin: 0;
        }

        .path-sort-filter {
          display: flex;
          align-items: center;
          gap: 10px;
        }

        .path-sort-filter label {
          color: #888;
          font-size: 0.9rem;
        }

        .path-sort-filter select {
          background: #252542;
          border: 1px solid #3a3a5a;
          border-radius: 6px;
          color: #fff;
          padding: 8px 12px;
          font-size: 0.9rem;
          cursor: pointer;
        }

        .path-sort-filter select:hover {
          border-color: #00d4aa;
        }

        .paths-list {
          display: flex;
          flex-direction: column;
          gap: 10px;
        }

        .path-item {
          background: #252542;
          border-radius: 8px;
          padding: 15px;
          display: flex;
          justify-content: space-between;
          align-items: center;
          flex-wrap: wrap;
          gap: 10px;
        }

        .path-code {
          background: #1a1a2e;
          padding: 6px 12px;
          border-radius: 6px;
          color: #00d4aa;
          font-size: 0.9rem;
        }

        .path-stats {
          display: flex;
          gap: 20px;
          align-items: center;
        }

        .path-count {
          color: #f0ad4e;
          font-weight: 600;
          font-size: 1rem;
        }

        .path-winrate {
          color: #888;
          font-size: 0.9rem;
        }

        .path-profit {
          font-weight: 600;
          font-size: 1rem;
        }

        .path-profit.positive {
          color: #00d4aa;
        }

        .path-profit.negative {
          color: #ff6b6b;
        }

        .path-pagination {
          display: flex;
          justify-content: center;
          align-items: center;
          gap: 15px;
          margin-top: 15px;
          padding-top: 15px;
          border-top: 1px solid #2a2a4a;
        }

        .pagination-btn-small {
          background: #252542;
          border: 1px solid #3a3a5a;
          color: #fff;
          padding: 6px 12px;
          border-radius: 6px;
          cursor: pointer;
          font-size: 0.85rem;
          transition: all 0.2s;
        }

        .pagination-btn-small:hover:not(:disabled) {
          border-color: #00d4aa;
        }

        .pagination-btn-small:disabled {
          color: #555;
          cursor: not-allowed;
        }

        .pagination-info-small {
          color: #888;
          font-size: 0.85rem;
        }
      `}</style>
    </div>
  );
}

export default TradeHistoryPanel;
