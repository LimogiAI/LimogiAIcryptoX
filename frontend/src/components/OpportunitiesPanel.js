import React, { useState, useEffect, useCallback } from 'react';
import { api } from '../services/api';

export function OpportunitiesPanel({ 
  opportunities, 
  sortBy, 
  setSortBy, 
  baseCurrency, 
  setBaseCurrency,
  minutesAgo,
  setMinutesAgo,
  onRefresh 
}) {
  const [activeSubTab, setActiveSubTab] = useState('live');
  const [historyData, setHistoryData] = useState([]);
  const [historyStats, setHistoryStats] = useState(null);
  const [historyHours, setHistoryHours] = useState(24);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [liveRefreshing, setLiveRefreshing] = useState(false);
  const [historyPage, setHistoryPage] = useState(1);
  const historyPerPage = 50;

  const handleLiveRefresh = async () => {
    setLiveRefreshing(true);
    try {
      await onRefresh();
    } finally {
      setTimeout(() => setLiveRefreshing(false), 500); // Show spinner briefly
    }
  };

  const fetchHistory = useCallback(async () => {
    setHistoryLoading(true);
    setHistoryPage(1); // Reset to first page
    try {
      const [history, stats] = await Promise.all([
        api.getOpportunityHistory({ limit: 500, hours: historyHours }),
        api.getOpportunityHistoryStats(historyHours),
      ]);
      setHistoryData(history.opportunities || []);
      setHistoryStats(stats);
    } catch (err) {
      console.error('Failed to fetch history:', err);
    } finally {
      setHistoryLoading(false);
    }
  }, [historyHours]);

  useEffect(() => {
    if (activeSubTab === 'history') {
      fetchHistory();
    }
  }, [activeSubTab, fetchHistory]);
  
  const formatTime = (timestamp) => {
    if (!timestamp) return '--';
    try {
      let ts = timestamp;
      if (!timestamp.endsWith('Z') && !timestamp.includes('+')) {
        ts = timestamp + 'Z';
      }
      const date = new Date(ts);
      return date.toLocaleTimeString('en-US', {
        timeZone: 'America/New_York',
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit',
        hour12: true
      });
    } catch {
      return '--';
    }
  };

  const formatDateTime = (timestamp) => {
    if (!timestamp) return '--';
    try {
      let ts = timestamp;
      if (!timestamp.endsWith('Z') && !timestamp.includes('+')) {
        ts = timestamp + 'Z';
      }
      const date = new Date(ts);
      return date.toLocaleString('en-US', {
        timeZone: 'America/New_York',
        month: 'short',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit',
        hour12: true
      });
    } catch {
      return '--';
    }
  };

  const formatProfit = (pct) => {
    if (pct === null || pct === undefined || pct === '') return '--';
    const value = parseFloat(pct);
    if (isNaN(value)) return '--';
    const sign = value >= 0 ? '+' : '';
    return `${sign}${value.toFixed(4)}%`;
  };

  const formatFees = (pct) => {
    if (pct === null || pct === undefined || pct === '') return '--';
    const value = parseFloat(pct);
    if (isNaN(value)) return '--';
    return `-${value.toFixed(2)}%`;
  };

  const formatAmount = (amount) => {
    if (amount === null || amount === undefined || amount === '') return '--';
    const value = parseFloat(amount);
    if (isNaN(value)) return '--';
    return `$${value.toFixed(2)}`;
  };

  return (
    <div className="panel opportunities-panel">
      {/* Sub-tabs for Live vs History */}
      <div className="sub-tabs">
        <button 
          className={activeSubTab === 'live' ? 'active' : ''} 
          onClick={() => setActiveSubTab('live')}
        >
          🔴 Live Opportunities
        </button>
        <button 
          className={activeSubTab === 'history' ? 'active' : ''} 
          onClick={() => setActiveSubTab('history')}
        >
          📜 Past Opportunities
        </button>
      </div>

      {activeSubTab === 'live' && (
        <>
          <div className="panel-header">
            <h2>📈 Arbitrage Opportunities</h2>
            
            <div className="filter-controls">
              <div className="filter-group">
                <label>Sort by:</label>
                <select 
                  value={sortBy} 
                  onChange={(e) => setSortBy(e.target.value)}
                  className="filter-select"
                >
                  <option value="time">Most Recent</option>
                  <option value="profit">Highest Profit</option>
                </select>
              </div>

              <div className="filter-group">
                <label>Time Range:</label>
                <select 
                  value={minutesAgo || 5} 
                  onChange={(e) => setMinutesAgo(parseInt(e.target.value))}
                  className="filter-select"
                >
                  <option value={1}>Last 1 min</option>
                  <option value={2}>Last 2 min</option>
                  <option value={5}>Last 5 min</option>
                  <option value={10}>Last 10 min</option>
                  <option value={30}>Last 30 min</option>
                  <option value={60}>Last 1 hour</option>
                </select>
              </div>
              
              <div className="filter-group">
                <label>Base Currency:</label>
                <select 
                  value={baseCurrency} 
                  onChange={(e) => setBaseCurrency(e.target.value)}
                  className="filter-select"
                >
                  <option value="ALL">ALL</option>
                  <option value="USD">USD</option>
                  <option value="EUR">EUR</option>
                  <option value="USDT">USDT</option>
                  <option value="ETH">ETH</option>
                  <option value="BTC">BTC</option>
                </select>
              </div>
              
              <button 
                onClick={handleLiveRefresh} 
                className={`refresh-btn ${liveRefreshing ? 'refreshing' : ''}`}
                disabled={liveRefreshing}
              >
                {liveRefreshing ? '⏳ Refreshing...' : '🔄 Refresh'}
              </button>
            </div>
          </div>

          {(!opportunities || opportunities.length === 0) ? (
            <div className="empty-state">
              <p>No opportunities found.</p>
              <p className="hint">Try changing filters or wait for scanner...</p>
            </div>
          ) : (
            <>
              <div className="opportunities-table-container">
                <table className="opportunities-table">
                  <thead>
                    <tr>
                      <th>Time (EST)</th>
                      <th>Path</th>
                      <th>Legs</th>
                      <th>Gross %</th>
                      <th>Fees %</th>
                      <th>Net %</th>
                      <th>Profit ($10k)</th>
                      <th>Status</th>
                    </tr>
                  </thead>
                  <tbody>
                    {opportunities.map((opp, idx) => (
                      <tr 
                        key={opp.id || idx} 
                        className={opp.is_profitable ? 'profitable' : 'unprofitable'}
                      >
                        <td className="time">{formatTime(opp.detected_at)}</td>
                        <td className="path">
                          <code>{opp.path || '--'}</code>
                        </td>
                        <td className="legs">{opp.legs || '--'}</td>
                        <td className="gross">
                          {formatProfit(opp.gross_profit_pct)}
                        </td>
                        <td className="fees">
                          {formatFees(opp.fees_pct)}
                        </td>
                        <td className={`net ${opp.is_profitable ? 'positive' : 'negative'}`}>
                          {formatProfit(opp.net_profit_pct)}
                        </td>
                        <td className={`amount ${opp.is_profitable ? 'positive' : 'negative'}`}>
                          {formatAmount(opp.profit_amount)}
                        </td>
                        <td className="status">
                          {opp.is_profitable ? (
                            <span className="badge profitable">✓ Profitable</span>
                          ) : (
                            <span className="badge unprofitable">✗ Loss</span>
                          )}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>

              <div className="panel-footer">
                <span>Showing {opportunities.length} opportunities</span>
                <span className="filter-info">
                  {sortBy === 'profit' ? '(sorted by profit)' : '(sorted by time)'}
                  {minutesAgo > 0 ? ` • Last ${minutesAgo} min` : ' • All time'}
                  {baseCurrency !== 'ALL' ? ` • ${baseCurrency} only` : ''}
                </span>
              </div>
            </>
          )}
        </>
      )}

      {activeSubTab === 'history' && (
        <>
          <div className="panel-header">
            <h2>📜 Past Opportunities</h2>
            
            <div className="filter-controls">
              <div className="filter-group">
                <label>Time Range:</label>
                <select 
                  value={historyHours} 
                  onChange={(e) => setHistoryHours(parseInt(e.target.value))}
                  className="filter-select"
                >
                  <option value={1}>Last 1 hour</option>
                  <option value={6}>Last 6 hours</option>
                  <option value={24}>Last 24 hours</option>
                  <option value={72}>Last 3 days</option>
                  <option value={168}>Last 7 days</option>
                  <option value={720}>Last 30 days</option>
                </select>
              </div>
              
              <button onClick={fetchHistory} className="refresh-btn">
                🔄 Refresh
              </button>
            </div>
          </div>

          {/* Stats Summary */}
          {historyStats && (
            <div className="history-stats">
              <div className="stat-card">
                <span className="label">Total Detected</span>
                <span className="value">{historyStats.total_opportunities}</span>
              </div>
              <div className="stat-card">
                <span className="label">Profitable</span>
                <span className="value positive">{historyStats.profitable_opportunities}</span>
              </div>
              <div className="stat-card">
                <span className="label">Actually Traded</span>
                <span className="value">{historyStats.traded_opportunities}</span>
              </div>
              <div className="stat-card">
                <span className="label">Trade Rate</span>
                <span className="value">{historyStats.trade_rate_pct?.toFixed(1)}%</span>
              </div>
              <div className="stat-card">
                <span className="label">Avg Expected Profit</span>
                <span className="value">{historyStats.avg_expected_profit_pct?.toFixed(3)}%</span>
              </div>
            </div>
          )}

          {historyLoading ? (
            <div className="empty-state">
              <p>Loading history...</p>
            </div>
          ) : historyData.length === 0 ? (
            <div className="empty-state">
              <p>No historical opportunities found.</p>
              <p className="hint">Opportunities will be saved as the scanner runs.</p>
            </div>
          ) : (
            <>
              <div className="opportunities-table-container">
                <table className="opportunities-table history-table">
                  <thead>
                    <tr>
                      <th>Detected At (EST)</th>
                      <th>Path</th>
                      <th>Legs</th>
                      <th>Expected %</th>
                      <th>Status</th>
                      <th>Slippage %</th>
                      <th>Actual %</th>
                      <th>Result</th>
                    </tr>
                  </thead>
                  <tbody>
                    {historyData
                      .slice((historyPage - 1) * historyPerPage, historyPage * historyPerPage)
                      .map((opp, idx) => (
                      <tr 
                        key={opp.id || idx} 
                        className={opp.was_traded ? (opp.actual_profit_pct >= 0 ? 'traded-win' : 'traded-loss') : (opp.is_profitable ? 'profitable' : 'unprofitable')}
                      >
                        <td className="time">{formatDateTime(opp.timestamp)}</td>
                        <td className="path">
                          <code>{opp.path || '--'}</code>
                        </td>
                        <td className="legs">{opp.legs || '--'}</td>
                        <td className={`net ${opp.is_profitable ? 'positive' : 'negative'}`}>
                          {formatProfit(opp.expected_profit_pct)}
                        </td>
                        <td className="status">
                          {opp.was_traded ? (
                            <span className="badge traded">✓ TRADED</span>
                          ) : (
                            <span className="badge not-traded">Not Traded</span>
                          )}
                        </td>
                        <td className="negative">
                          {opp.was_traded && opp.slippage_pct != null ? `-${Math.abs(opp.slippage_pct).toFixed(2)}%` : '--'}
                        </td>
                        <td className={`net ${(opp.actual_profit_pct || 0) >= 0 ? 'positive' : 'negative'}`}>
                          {opp.was_traded ? formatProfit(opp.actual_profit_pct) : '--'}
                        </td>
                        <td className="result">
                          {opp.was_traded ? (
                            opp.actual_profit_pct >= 0 ? (
                              <span className="badge win">✓ Win</span>
                            ) : (
                              <span className="badge loss">✗ Loss</span>
                            )
                          ) : (
                            <span className="badge skipped">--</span>
                          )}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>

              {/* Pagination Controls */}
              {historyData.length > historyPerPage && (
                <div className="history-pagination">
                  <button 
                    className="pagination-btn"
                    onClick={() => setHistoryPage(p => Math.max(1, p - 1))}
                    disabled={historyPage === 1}
                  >
                    ← Previous
                  </button>
                  <span className="pagination-info">
                    Page {historyPage} of {Math.ceil(historyData.length / historyPerPage)}
                    <span className="pagination-detail">
                      (Showing {(historyPage - 1) * historyPerPage + 1}-{Math.min(historyPage * historyPerPage, historyData.length)} of {historyData.length})
                    </span>
                  </span>
                  <button 
                    className="pagination-btn"
                    onClick={() => setHistoryPage(p => Math.min(Math.ceil(historyData.length / historyPerPage), p + 1))}
                    disabled={historyPage >= Math.ceil(historyData.length / historyPerPage)}
                  >
                    Next →
                  </button>
                </div>
              )}

              <div className="panel-footer">
                <span>Showing {historyData.length} historical opportunities</span>
                <span className="filter-info">
                  Last {historyHours} hours • Auto-deletes after 30 days
                </span>
              </div>
            </>
          )}

          <style jsx>{`
            .history-stats {
              display: grid;
              grid-template-columns: repeat(5, 1fr);
              gap: 15px;
              margin-bottom: 20px;
            }

            .history-stats .stat-card {
              background: #252542;
              padding: 15px;
              border-radius: 10px;
              text-align: center;
            }

            .history-stats .label {
              display: block;
              color: #888;
              font-size: 0.85rem;
              margin-bottom: 5px;
            }

            .history-stats .value {
              display: block;
              color: #fff;
              font-size: 1.3rem;
              font-weight: 700;
            }

            .history-stats .value.positive {
              color: #00d4aa;
            }

            .top-paths-section {
              background: #252542;
              padding: 15px 20px;
              border-radius: 10px;
              margin-bottom: 20px;
            }

            .top-paths-section h4 {
              color: #ffc107;
              margin: 0 0 15px 0;
            }

            .top-paths-list {
              display: flex;
              flex-direction: column;
              gap: 10px;
            }

            .top-path-item {
              display: flex;
              justify-content: space-between;
              align-items: center;
              background: #1a1a2e;
              padding: 10px 15px;
              border-radius: 8px;
            }

            .top-path-item code {
              color: #00d4aa;
              font-size: 0.85rem;
            }

            .top-path-item .count {
              color: #ffc107;
              font-weight: 600;
            }

            .badge.traded {
              background: rgba(0, 212, 170, 0.2);
              color: #00d4aa;
              padding: 4px 10px;
              border-radius: 12px;
              font-size: 0.8rem;
            }

            .badge.not-traded {
              background: rgba(136, 136, 136, 0.2);
              color: #888;
              padding: 4px 10px;
              border-radius: 12px;
              font-size: 0.8rem;
            }

            tr.traded {
              background: rgba(0, 212, 170, 0.05);
            }

            tr.traded-win {
              background: rgba(0, 212, 170, 0.1);
              border-left: 3px solid #00d4aa;
            }

            tr.traded-loss {
              background: rgba(255, 107, 107, 0.1);
              border-left: 3px solid #ff6b6b;
            }

            .badge.win {
              background: rgba(0, 212, 170, 0.2);
              color: #00d4aa;
              padding: 4px 10px;
              border-radius: 12px;
              font-size: 0.8rem;
              font-weight: 600;
            }

            .badge.loss {
              background: rgba(255, 107, 107, 0.2);
              color: #ff6b6b;
              padding: 4px 10px;
              border-radius: 12px;
              font-size: 0.8rem;
              font-weight: 600;
            }

            .badge.skipped {
              background: rgba(136, 136, 136, 0.1);
              color: #666;
              padding: 4px 10px;
              border-radius: 12px;
              font-size: 0.8rem;
            }

            .history-table th {
              font-size: 0.85rem;
            }

            @media (max-width: 900px) {
              .history-stats {
                grid-template-columns: repeat(3, 1fr);
              }
            }

            @media (max-width: 600px) {
              .history-stats {
                grid-template-columns: repeat(2, 1fr);
              }
            }

            /* Pagination */
            .history-pagination {
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
          `}</style>
        </>
      )}

      {/* Styles - always rendered */}
      <style jsx>{`
        .sub-tabs {
          display: flex;
          gap: 10px;
          margin-bottom: 20px;
          border-bottom: 1px solid #3a3a5a;
          padding-bottom: 10px;
        }

        .sub-tabs button {
          padding: 10px 20px;
          border: none;
          background: transparent;
          color: #888;
          cursor: pointer;
          font-size: 1rem;
          border-radius: 8px 8px 0 0;
          transition: all 0.2s;
        }

        .sub-tabs button:hover {
          color: #fff;
          background: #252542;
        }

        .sub-tabs button.active {
          color: #00d4aa;
          background: #252542;
          border-bottom: 2px solid #00d4aa;
        }

        .refresh-btn.refreshing {
          opacity: 0.7;
          cursor: wait;
        }

        .refresh-btn:disabled {
          cursor: wait;
        }
      `}</style>
    </div>
  );
}

export default OpportunitiesPanel;
