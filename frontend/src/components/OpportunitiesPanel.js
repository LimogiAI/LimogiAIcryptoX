import React, { useState, useEffect, useCallback } from 'react';
import { api } from '../services/api';

export function OpportunitiesPanel({
  opportunities,
  tradeAmount = 10.0,
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
  const [livePage, setLivePage] = useState(1);
  const [expandedLiveRows, setExpandedLiveRows] = useState({});
  const historyPerPage = 50;
  const livePerPage = 50;

  const toggleLiveRow = (id) => {
    setExpandedLiveRows(prev => ({
      ...prev,
      [id]: !prev[id]
    }));
  };

  const handleLiveRefresh = async () => {
    setLiveRefreshing(true);
    try {
      await onRefresh();
    } finally {
      setTimeout(() => setLiveRefreshing(false), 500); // Show spinner briefly
    }
  };

  // Filter to only profitable opportunities
  const profitableOpportunities = opportunities?.filter(opp => opp.is_profitable) || [];
  
  // Pagination for live opportunities
  const totalLivePages = Math.ceil(profitableOpportunities.length / livePerPage);
  const paginatedLiveOpportunities = profitableOpportunities.slice(
    (livePage - 1) * livePerPage,
    livePage * livePerPage
  );

  // Reset page when opportunities change significantly
  useEffect(() => {
    if (livePage > totalLivePages && totalLivePages > 0) {
      setLivePage(1);
    }
  }, [livePage, totalLivePages]);

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
    if (!timestamp && timestamp !== 0) return '--';
    try {
      let date;
      // Handle milliseconds timestamp (number)
      if (typeof timestamp === 'number') {
        date = new Date(timestamp);
      } else {
        // Handle string timestamp
        let ts = timestamp;
        if (!timestamp.endsWith('Z') && !timestamp.includes('+')) {
          ts = timestamp + 'Z';
        }
        date = new Date(ts);
      }
      if (isNaN(date.getTime())) return '--';
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
    if (!timestamp && timestamp !== 0) return '--';
    try {
      let date;
      // Handle milliseconds timestamp (number)
      if (typeof timestamp === 'number') {
        date = new Date(timestamp);
      } else {
        // Handle string timestamp
        let ts = timestamp;
        if (!timestamp.endsWith('Z') && !timestamp.includes('+')) {
          ts = timestamp + 'Z';
        }
        date = new Date(ts);
      }
      if (isNaN(date.getTime())) return '--';
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
                  value={minutesAgo || 60} 
                  onChange={(e) => setMinutesAgo(parseInt(e.target.value))}
                  className="filter-select"
                >
                  <option value={5}>Last 5 min</option>
                  <option value={15}>Last 15 min</option>
                  <option value={30}>Last 30 min</option>
                  <option value={60}>Last 1 hour</option>
                  <option value={360}>Last 6 hours</option>
                  <option value={1440}>Last 24 hours</option>
                  <option value={10080}>Last 7 days</option>
                  <option value={0}>All Time</option>
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

          {profitableOpportunities.length === 0 ? (
            <div className="empty-state">
              <p>No profitable opportunities found.</p>
              <p className="hint">Scanner is looking for arbitrage... {opportunities?.length > 0 ? `(${opportunities.length} total, none profitable)` : ''}</p>
            </div>
          ) : (
            <>
              <div className="opportunities-table-container">
                <table className="opportunities-table">
                  <thead>
                    <tr>
                      <th></th>
                      <th>Time (EST)</th>
                      <th>Path</th>
                      <th>Legs</th>
                      <th>Gross %</th>
                      <th>Fees %</th>
                      <th>Net %</th>
                      <th>Profit (${tradeAmount >= 1000 ? `${(tradeAmount/1000).toFixed(0)}k` : tradeAmount.toFixed(0)})</th>
                      <th>Status</th>
                    </tr>
                  </thead>
                  <tbody>
                    {paginatedLiveOpportunities.map((opp, idx) => {
                      const rowId = opp.id || idx;
                      const isExpanded = expandedLiveRows[rowId];
                      const hasPriceSnapshot = opp.prices_snapshot && opp.prices_snapshot.legs;
                      
                      return (
                        <React.Fragment key={rowId}>
                          <tr 
                            className={`${opp.is_profitable ? 'profitable' : 'unprofitable'} ${hasPriceSnapshot ? 'expandable' : ''}`}
                            onClick={() => hasPriceSnapshot && toggleLiveRow(rowId)}
                            style={{ cursor: hasPriceSnapshot ? 'pointer' : 'default' }}
                          >
                            <td className="expand-toggle">
                              {hasPriceSnapshot ? (isExpanded ? '▼' : '▶') : ''}
                            </td>
                            <td className="time">{formatTime(opp.detected_at)}</td>
                            <td className="path">
                              <code>{opp.path || '--'}</code>
                              {opp.source === 'history' && <span className="source-badge">saved</span>}
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
                          {isExpanded && hasPriceSnapshot && (
                            <tr className="expanded-details">
                              <td colSpan="9">
                                <div className="leg-details">
                                  <div className="leg-details-header">
                                    <h4>Leg Details (Snapshot at Detection)</h4>
                                    <span className="snapshot-fee">
                                      Fee Rate: <strong>{(opp.prices_snapshot.fee_rate * 100).toFixed(2)}%</strong> per leg
                                      <span className={`fee-badge ${opp.prices_snapshot.fee_source}`}>
                                        {opp.prices_snapshot.fee_source}
                                      </span>
                                      <span className="calc-basis">(calculated for $1.00)</span>
                                    </span>
                                  </div>
                                  <table className="leg-table">
                                    <thead>
                                      <tr>
                                        <th>LEG</th>
                                        <th>PAIR</th>
                                        <th>ACTION</th>
                                        <th>EXPECTED PRICE</th>
                                        <th>FEE %</th>
                                        <th>FEE (USD)</th>
                                      </tr>
                                    </thead>
                                    <tbody>
                                      {(() => {
                                        const feeRate = opp.prices_snapshot.fee_rate;
                                        let runningValue = 1.0;
                                        
                                        return opp.prices_snapshot.legs.map((leg, legIdx) => {
                                          const feeUsd = runningValue * feeRate;
                                          runningValue = runningValue * (1 - feeRate);
                                          
                                          return (
                                            <tr key={legIdx}>
                                              <td className="leg-num">{legIdx + 1}</td>
                                              <td className="leg-pair">{leg.pair}</td>
                                              <td className={`leg-side ${leg.action}`}>{leg.action.toUpperCase()}</td>
                                              <td className="leg-price">{leg.rate.toFixed(8)}</td>
                                              <td className="leg-fee-pct negative">-{(feeRate * 100).toFixed(2)}%</td>
                                              <td className="leg-fee-usd negative">-${feeUsd.toFixed(6)}</td>
                                            </tr>
                                          );
                                        });
                                      })()}
                                    </tbody>
                                    <tfoot>
                                      <tr className="total-row">
                                        <td colSpan="4" className="total-label">TOTAL FEES ({opp.prices_snapshot.legs.length} legs)</td>
                                        <td className="leg-fee-pct negative">-{(opp.prices_snapshot.fee_rate * 100 * opp.prices_snapshot.legs.length).toFixed(2)}%</td>
                                        <td className="leg-fee-usd negative">
                                          -${((() => {
                                            let runningValue = 1.0;
                                            let totalFees = 0;
                                            const feeRate = opp.prices_snapshot.fee_rate;
                                            opp.prices_snapshot.legs.forEach(() => {
                                              totalFees += runningValue * feeRate;
                                              runningValue = runningValue * (1 - feeRate);
                                            });
                                            return totalFees;
                                          })()).toFixed(6)}
                                        </td>
                                      </tr>
                                    </tfoot>
                                  </table>
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

              <div className="panel-footer">
                <span>Showing {paginatedLiveOpportunities.length} of {profitableOpportunities.length} profitable opportunities</span>
                
                {totalLivePages > 1 && (
                  <div className="pagination">
                    <button 
                      onClick={() => setLivePage(1)} 
                      disabled={livePage === 1}
                      className="page-btn"
                    >
                      ««
                    </button>
                    <button 
                      onClick={() => setLivePage(p => Math.max(1, p - 1))} 
                      disabled={livePage === 1}
                      className="page-btn"
                    >
                      «
                    </button>
                    <span className="page-info">Page {livePage} of {totalLivePages}</span>
                    <button 
                      onClick={() => setLivePage(p => Math.min(totalLivePages, p + 1))} 
                      disabled={livePage === totalLivePages}
                      className="page-btn"
                    >
                      »
                    </button>
                    <button 
                      onClick={() => setLivePage(totalLivePages)} 
                      disabled={livePage === totalLivePages}
                      className="page-btn"
                    >
                      »»
                    </button>
                  </div>
                )}
                
                <span className="filter-info">
                  {sortBy === 'profit' ? '(sorted by profit)' : '(sorted by time)'}
                  {minutesAgo > 0 ? ` • Last ${minutesAgo >= 60 ? `${Math.floor(minutesAgo/60)} hour${minutesAgo >= 120 ? 's' : ''}` : `${minutesAgo} min`}` : ' • All time'}
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

        /* Expandable rows for Live Opportunities */
        .opportunities-table .expand-toggle {
          width: 30px;
          text-align: center;
          color: #00d4aa;
          font-size: 0.8rem;
        }

        .opportunities-table tr.expandable:hover {
          background: rgba(0, 212, 170, 0.1);
        }

        .source-badge {
          margin-left: 8px;
          padding: 2px 6px;
          background: rgba(108, 92, 231, 0.3);
          color: #a29bfe;
          border-radius: 4px;
          font-size: 0.7rem;
          font-weight: 500;
        }

        .opportunities-table .expanded-details {
          background: #1a1a2e;
        }

        .opportunities-table .expanded-details td {
          padding: 0;
        }

        .opportunities-table .leg-details {
          padding: 15px 20px;
          border-left: 3px solid #00d4aa;
          margin-left: 30px;
        }

        .opportunities-table .leg-details-header {
          display: flex;
          justify-content: space-between;
          align-items: center;
          margin-bottom: 12px;
        }

        .opportunities-table .leg-details h4 {
          color: #00d4aa;
          font-size: 0.9rem;
          margin: 0;
        }

        .opportunities-table .snapshot-fee {
          color: #888;
          font-size: 0.85rem;
        }

        .opportunities-table .snapshot-fee strong {
          color: #fff;
        }

        .opportunities-table .leg-table {
          width: 100%;
          border-collapse: collapse;
          font-size: 0.85rem;
        }

        .opportunities-table .leg-table th {
          text-align: left;
          padding: 8px 12px;
          color: #00d4aa;
          font-weight: 600;
          font-size: 0.75rem;
          border-bottom: 1px solid #2a2a4a;
        }

        .opportunities-table .leg-table td {
          padding: 10px 12px;
          border-bottom: 1px solid #252542;
        }

        .opportunities-table .leg-table tr:last-child td {
          border-bottom: none;
        }

        .opportunities-table .leg-num {
          color: #00d4aa;
          font-weight: 600;
        }

        .opportunities-table .leg-pair {
          color: #fff;
          font-family: monospace;
          font-weight: 500;
        }

        .opportunities-table .leg-side {
          padding: 3px 10px;
          border-radius: 4px;
          font-size: 0.75rem;
          font-weight: 600;
          display: inline-block;
        }

        .opportunities-table .leg-side.buy {
          background: rgba(0, 212, 170, 0.2);
          color: #00d4aa;
        }

        .opportunities-table .leg-side.sell {
          background: rgba(255, 107, 107, 0.2);
          color: #ff6b6b;
        }

        .opportunities-table .leg-price {
          color: #e0e0e0;
          font-family: monospace;
        }

        .opportunities-table .leg-fee-pct,
        .opportunities-table .leg-fee-usd {
          color: #ff6b6b;
        }

        .opportunities-table .leg-fee-usd {
          font-family: monospace;
        }

        .opportunities-table .calc-basis {
          color: #888;
          font-size: 0.75rem;
          margin-left: 8px;
        }

        .opportunities-table .leg-table tfoot {
          border-top: 2px solid #00d4aa;
        }

        .opportunities-table .leg-table tfoot .total-row {
          background: rgba(0, 212, 170, 0.1);
        }

        .opportunities-table .leg-table tfoot .total-label {
          font-weight: 600;
          color: #00d4aa;
          text-align: right;
          padding-right: 20px;
        }

        .opportunities-table .leg-table tfoot td {
          padding: 12px;
          font-weight: 600;
        }

        .opportunities-table .fee-badge {
          margin-left: 8px;
          padding: 2px 8px;
          border-radius: 10px;
          font-size: 0.75rem;
          font-weight: 500;
        }

        .opportunities-table .fee-badge.live {
          background: rgba(0, 212, 170, 0.2);
          color: #00d4aa;
        }

        .opportunities-table .fee-badge.default {
          background: rgba(255, 193, 7, 0.2);
          color: #ffc107;
        }

        /* Pagination */
        .pagination {
          display: flex;
          align-items: center;
          gap: 8px;
        }

        .page-btn {
          background: linear-gradient(135deg, #2a2a4a, #3a3a5a);
          border: 1px solid #4a4a6a;
          color: #fff;
          padding: 6px 12px;
          border-radius: 6px;
          cursor: pointer;
          font-size: 0.85rem;
          transition: all 0.2s;
        }

        .page-btn:hover:not(:disabled) {
          background: linear-gradient(135deg, #3a3a5a, #4a4a6a);
          border-color: #6c5ce7;
        }

        .page-btn:disabled {
          opacity: 0.4;
          cursor: not-allowed;
        }

        .page-info {
          color: #a0a0b0;
          font-size: 0.85rem;
          padding: 0 8px;
        }

        .panel-footer {
          display: flex;
          justify-content: space-between;
          align-items: center;
          flex-wrap: wrap;
          gap: 12px;
        }
      `}</style>
    </div>
  );
}

export default OpportunitiesPanel;