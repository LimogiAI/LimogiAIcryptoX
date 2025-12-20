import React, { useState, useEffect, useCallback } from 'react';
import { Header } from './components/Header';
import { StatusBar } from './components/StatusBar';
import { OpportunitiesPanel } from './components/OpportunitiesPanel';
import { PriceMatrix } from './components/PriceMatrix';
import { OrderBookHealthPanel } from './components/OrderBookHealthPanel';
import { LiveTradingPanel } from './components/LiveTradingPanel';
import { LiveTradeHistoryPanel } from './components/LiveTradeHistoryPanel';
import { useWebSocket } from './hooks/useWebSocket';
import { api } from './services/api';

function App() {
  const [status, setStatus] = useState(null);
  const [opportunities, setOpportunities] = useState([]);
  const [prices, setPrices] = useState([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  const [activeTab, setActiveTab] = useState('opportunities');

  // Live trading status for tab styling
  const [liveTradeEnabled, setLiveTradeEnabled] = useState(false);

  // Filter states
  const [sortBy, setSortBy] = useState('time');
  const [baseCurrency, setBaseCurrency] = useState('ALL');
  const [minutesAgo, setMinutesAgo] = useState(5);

  // WebSocket connection
  const { connected } = useWebSocket();

  // Fetch live trading status for tab styling
  const fetchLiveStatus = useCallback(async () => {
    try {
      const liveStatus = await api.getLiveStatus();
      setLiveTradeEnabled(liveStatus?.enabled || false);
    } catch (err) {
      // Silently fail - just for UI styling
    }
  }, []);

  // Fetch opportunities with filters
  const fetchOpportunities = useCallback(async () => {
    try {
      const oppsRes = await api.getOpportunities({
        sort_by: sortBy,
        base_currency: baseCurrency,
        minutes_ago: minutesAgo,
        limit: 50,
      });
      setOpportunities(oppsRes.opportunities || oppsRes || []);
    } catch (err) {
      console.error('Fetch opportunities error:', err);
    }
  }, [sortBy, baseCurrency, minutesAgo]);

  // Fetch all data
  const fetchData = useCallback(async () => {
    try {
      setLoading(true);
      setError(null);

      const [statusRes, oppsRes, pricesRes] = await Promise.all([
        api.getStatus(),
        api.getOpportunities({
          sort_by: sortBy,
          base_currency: baseCurrency,
          minutes_ago: minutesAgo,
          limit: 50,
        }),
        api.getLivePrices(),
      ]);

      setStatus(statusRes);
      setOpportunities(oppsRes.opportunities || oppsRes || []);
      setPrices(pricesRes.prices || []);
    } catch (err) {
      setError(err.message || 'Failed to fetch data');
      console.error('Fetch error:', err);
    } finally {
      setLoading(false);
    }
  }, [sortBy, baseCurrency, minutesAgo]);

  // Initial data load and refresh interval
  useEffect(() => {
    fetchData();
    fetchLiveStatus();
    const interval = setInterval(fetchData, 30000);
    const liveStatusInterval = setInterval(fetchLiveStatus, 5000);
    return () => {
      clearInterval(interval);
      clearInterval(liveStatusInterval);
    };
  }, [fetchData, fetchLiveStatus]);

  // Refetch when filters change
  useEffect(() => {
    if (!loading) {
      fetchOpportunities();
    }
  }, [sortBy, baseCurrency, fetchOpportunities, loading]);

  const handleRefresh = () => {
    fetchOpportunities();
  };

  if (loading && !status) {
    return (
      <div className="app loading">
        <div className="loading-spinner"></div>
        <p>Loading LimogiAICryptoX...</p>
      </div>
    );
  }

  return (
    <div className="app">
      <Header connected={connected} />

      {error && (
        <div className="error-banner">
          <span>⚠️ {error}</span>
          <button onClick={() => setError(null)}>×</button>
        </div>
      )}

      <StatusBar status={status} />

      <div className="tabs">
        <button
          className={activeTab === 'opportunities' ? 'active' : ''}
          onClick={() => setActiveTab('opportunities')}
        >
          📈 Opportunities
        </button>
        <button
          className={activeTab === 'orderbook-health' ? 'active' : ''}
          onClick={() => setActiveTab('orderbook-health')}
        >
          🩺 Order Book Health
        </button>
        <button
          className={activeTab === 'prices' ? 'active' : ''}
          onClick={() => setActiveTab('prices')}
        >
          💱 Price Matrix
        </button>
        <button
          className={`live-trading-tab ${activeTab === 'live-trading' ? 'active' : ''} ${liveTradeEnabled ? 'live-enabled' : ''}`}
          onClick={() => setActiveTab('live-trading')}
        >
          <span className="live-dot-tab"></span>
          Live Trading
        </button>
        <button
          className={`live-history-tab ${activeTab === 'live-trade-history' ? 'active' : ''}`}
          onClick={() => setActiveTab('live-trade-history')}
        >
          📊 Live Trade History
        </button>
      </div>

      <main className="main-content">
        {activeTab === 'opportunities' && (
          <OpportunitiesPanel
            opportunities={opportunities}
            sortBy={sortBy}
            setSortBy={setSortBy}
            baseCurrency={baseCurrency}
            setBaseCurrency={setBaseCurrency}
            minutesAgo={minutesAgo}
            setMinutesAgo={setMinutesAgo}
            onRefresh={handleRefresh}
          />
        )}
        {activeTab === 'orderbook-health' && (
          <OrderBookHealthPanel />
        )}
        {activeTab === 'prices' && (
          <PriceMatrix prices={prices} />
        )}
        {activeTab === 'live-trading' && (
          <LiveTradingPanel />
        )}
        {activeTab === 'live-trade-history' && (
          <LiveTradeHistoryPanel />
        )}
      </main>

      <footer className="footer">
        <span>LimogiAICryptoX v2.0.0</span>
        <span>•</span>
        <span>{status?.pairs_monitored || 0} pairs monitored</span>
        <span>•</span>
        <span>{connected ? '🟢 Connected' : '🔴 Disconnected'}</span>
      </footer>
    </div>
  );
}

export default App;
