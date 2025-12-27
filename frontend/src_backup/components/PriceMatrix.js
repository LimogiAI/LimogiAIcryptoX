import React, { useState, useEffect, useCallback } from 'react';
import { api } from '../services/api';

// Currency categories for dropdown grouping
const CURRENCY_CATEGORIES = {
  'FIAT': {
    emoji: '💵',
    label: 'FIAT',
    currencies: ['USD', 'EUR', 'GBP', 'CAD', 'AUD', 'CHF', 'JPY']
  },
  'MAJOR_CRYPTO': {
    emoji: '🪙',
    label: 'MAJOR CRYPTO',
    currencies: ['BTC', 'ETH', 'SOL', 'XRP', 'ADA', 'DOT', 'AVAX', 'MATIC', 'LINK', 'LTC']
  },
  'STABLECOINS': {
    emoji: '💲',
    label: 'STABLECOINS',
    currencies: ['USDT', 'USDC', 'DAI', 'USDG', 'TUSD', 'PYUSD']
  },
  'MEMECOINS': {
    emoji: '🐕',
    label: 'MEMECOINS',
    currencies: ['DOGE', 'SHIB', 'PEPE', 'BONK', 'FLOKI', 'WIF', 'DOGEN', 'SNEK']
  },
  'ALTCOINS': {
    emoji: '🎮',
    label: 'ALTCOINS',
    currencies: []
  }
};

// Helper function to normalize currency names
const normalizeCurrency = (currency) => {
  const mappings = {
    'XXBT': 'BTC',
    'XBT': 'BTC',
    'XETH': 'ETH',
    'XXRP': 'XRP',
    'XXLM': 'XLM',
    'XXDG': 'DOGE',
    'ZUSD': 'USD',
    'ZEUR': 'EUR',
    'ZGBP': 'GBP',
    'ZCAD': 'CAD',
    'ZAUD': 'AUD',
    'ZJPY': 'JPY',
  };
  return mappings[currency] || currency;
};

export function PriceMatrix({ prices }) {
  const [fromCurrency, setFromCurrency] = useState('USD');
  const [toCurrency, setToCurrency] = useState('BTC');
  const [amount, setAmount] = useState(1);
  const [conversionResult, setConversionResult] = useState(null);
  const [categorizedCurrencies, setCategorizedCurrencies] = useState({});
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(null);
  const [lastUpdate, setLastUpdate] = useState(null);
  const [currenciesLoaded, setCurrenciesLoaded] = useState(false);

  // Define fetchConversion BEFORE useEffect that uses it
  const fetchConversion = useCallback(async (from, to, amt) => {
    if (!from || !to) return;
    
    if (from === to) {
      setConversionResult({
        rate: 1,
        inverseRate: 1,
        result: amt,
        bid: 1,
        ask: 1,
        spread: 0,
        pairFound: true,
        pairName: `${from}/${to}`,
      });
      setLastUpdate(new Date());
      return;
    }
    
    setLoading(true);
    setError(null);
    
    try {
      const response = await api.getLivePrices(500);
      const pricesData = response.prices || response || [];
      
      // Look for direct pair (FROM/TO or TO/FROM)
      const directPair = pricesData.find(p => {
        const pair = p.pair || '';
        const [base, quote] = pair.includes('/') ? pair.split('/') : [null, null];
        return (base === from && quote === to) ||
               (base === to && quote === from);
      });
      
      if (directPair) {
        const pair = directPair.pair;
        const [base] = pair.split('/');
        const bid = parseFloat(directPair.bid);
        const ask = parseFloat(directPair.ask);
        
        let rate, inverseRate, usedBid, usedAsk;
        
        if (base === from) {
          rate = bid;
          inverseRate = 1 / ask;
          usedBid = bid;
          usedAsk = ask;
        } else {
          rate = 1 / ask;
          inverseRate = bid;
          usedBid = 1 / ask;
          usedAsk = 1 / bid;
        }
        
        const spread = Math.abs(((ask - bid) / bid) * 100);
        
        setConversionResult({
          rate: rate,
          inverseRate: inverseRate,
          result: amt * rate,
          bid: usedBid,
          ask: usedAsk,
          spread: spread,
          pairFound: true,
          pairName: pair,
        });
        setLastUpdate(new Date());
      } else {
        setConversionResult({
          pairFound: false,
        });
      }
    } catch (err) {
      console.error('Failed to fetch conversion:', err);
      setError('Failed to fetch conversion rate');
    } finally {
      setLoading(false);
    }
  }, []);

  // Fetch currencies on mount
  useEffect(() => {
    const fetchCurrencies = async () => {
      try {
        const response = await api.getLivePrices(500);
        const pricesData = response.prices || response || [];
        
        const currencies = new Set();
        pricesData.forEach(p => {
          const pair = p.pair || '';
          if (pair.includes('/')) {
            const [base, quote] = pair.split('/');
            currencies.add(base);
            currencies.add(quote);
          }
        });
        
        const currencyList = Array.from(currencies).sort();
        
        const categorized = {
          FIAT: { ...CURRENCY_CATEGORIES.FIAT, currencies: [] },
          MAJOR_CRYPTO: { ...CURRENCY_CATEGORIES.MAJOR_CRYPTO, currencies: [] },
          STABLECOINS: { ...CURRENCY_CATEGORIES.STABLECOINS, currencies: [] },
          MEMECOINS: { ...CURRENCY_CATEGORIES.MEMECOINS, currencies: [] },
          ALTCOINS: { ...CURRENCY_CATEGORIES.ALTCOINS, currencies: [] },
        };
        
        currencyList.forEach(currency => {
          const normalized = normalizeCurrency(currency);
          
          if (CURRENCY_CATEGORIES.FIAT.currencies.includes(normalized) ||
              CURRENCY_CATEGORIES.FIAT.currencies.includes(currency)) {
            categorized.FIAT.currencies.push(currency);
          } else if (CURRENCY_CATEGORIES.MAJOR_CRYPTO.currencies.includes(normalized) ||
                     CURRENCY_CATEGORIES.MAJOR_CRYPTO.currencies.includes(currency)) {
            categorized.MAJOR_CRYPTO.currencies.push(currency);
          } else if (CURRENCY_CATEGORIES.STABLECOINS.currencies.includes(normalized) ||
                     CURRENCY_CATEGORIES.STABLECOINS.currencies.includes(currency)) {
            categorized.STABLECOINS.currencies.push(currency);
          } else if (CURRENCY_CATEGORIES.MEMECOINS.currencies.includes(normalized) ||
                     CURRENCY_CATEGORIES.MEMECOINS.currencies.includes(currency)) {
            categorized.MEMECOINS.currencies.push(currency);
          } else {
            categorized.ALTCOINS.currencies.push(currency);
          }
        });
        
        categorized.ALTCOINS.currencies.sort();
        setCategorizedCurrencies(categorized);
        setCurrenciesLoaded(true);
        
      } catch (err) {
        console.error('Failed to fetch currencies:', err);
      }
    };
    
    fetchCurrencies();
  }, []);

  // Fetch conversion when currencies change
  useEffect(() => {
    if (currenciesLoaded && fromCurrency && toCurrency && amount > 0) {
      fetchConversion(fromCurrency, toCurrency, amount);
    }
  }, [currenciesLoaded, fromCurrency, toCurrency, amount, fetchConversion]);

  // Auto-refresh every 10 seconds
  useEffect(() => {
    if (!currenciesLoaded) return;
    
    const interval = setInterval(() => {
      if (fromCurrency && toCurrency && amount > 0) {
        fetchConversion(fromCurrency, toCurrency, amount);
      }
    }, 10000);
    
    return () => clearInterval(interval);
  }, [currenciesLoaded, fromCurrency, toCurrency, amount, fetchConversion]);

  const formatNumber = (num) => {
    if (num === null || num === undefined || isNaN(num)) return '--';
    if (num >= 1000) return num.toLocaleString('en-US', { maximumFractionDigits: 2 });
    if (num >= 1) return num.toLocaleString('en-US', { maximumFractionDigits: 4 });
    if (num >= 0.0001) return num.toLocaleString('en-US', { maximumFractionDigits: 6 });
    return num.toLocaleString('en-US', { maximumFractionDigits: 8 });
  };

  const swapCurrencies = () => {
    const temp = fromCurrency;
    setFromCurrency(toCurrency);
    setToCurrency(temp);
  };

  const renderCurrencyDropdown = (value, onChange, label) => (
    <div className="currency-select-wrapper">
      <label>{label}</label>
      <select 
        value={value} 
        onChange={(e) => onChange(e.target.value)}
        className="currency-select"
      >
        {Object.entries(categorizedCurrencies).map(([key, category]) => (
          category.currencies && category.currencies.length > 0 && (
            <optgroup key={key} label={`${category.emoji} ${category.label}`}>
              {category.currencies.map(currency => (
                <option key={currency} value={currency}>
                  {currency}
                </option>
              ))}
            </optgroup>
          )
        ))}
      </select>
    </div>
  );

  return (
    <div className="panel price-matrix-panel">
      <div className="converter-header">
        <h2>💱 Kraken Live Prices - Currency Converter</h2>
        <span className="live-indicator">
          <span className="pulse"></span>
          Live
        </span>
      </div>

      <div className="converter-container">
        <div className="amount-input-wrapper">
          <label>Amount</label>
          <input
            type="number"
            value={amount}
            onChange={(e) => setAmount(parseFloat(e.target.value) || 0)}
            min="0"
            step="any"
            className="amount-input"
          />
        </div>

        <div className="currency-selection">
          {renderCurrencyDropdown(fromCurrency, setFromCurrency, 'From')}
          
          <button className="swap-btn" onClick={swapCurrencies} title="Swap currencies">
            ⇄
          </button>
          
          {renderCurrencyDropdown(toCurrency, setToCurrency, 'To')}
        </div>

        <div className="conversion-result">
          {loading ? (
            <div className="loading-state">
              <span className="spinner"></span>
              Fetching rate...
            </div>
          ) : error ? (
            <div className="error-state">
              ⚠️ {error}
            </div>
          ) : conversionResult ? (
            conversionResult.pairFound ? (
              <>
                <div className="result-main">
                  <span className="result-amount">{formatNumber(conversionResult.result)}</span>
                  <span className="result-currency">{toCurrency}</span>
                </div>
                
                <div className="rate-info">
                  <div className="rate-row">
                    <span className="rate-label">Rate:</span>
                    <span className="rate-value">
                      1 {fromCurrency} = {formatNumber(conversionResult.rate)} {toCurrency}
                    </span>
                  </div>
                  <div className="rate-row">
                    <span className="rate-label">Inverse:</span>
                    <span className="rate-value">
                      1 {toCurrency} = {formatNumber(conversionResult.inverseRate)} {fromCurrency}
                    </span>
                  </div>
                </div>
                
                <div className="market-info">
                  <div className="market-row">
                    <span className="market-label">Bid</span>
                    <span className="market-value bid">{formatNumber(conversionResult.bid)}</span>
                  </div>
                  <div className="market-row">
                    <span className="market-label">Ask</span>
                    <span className="market-value ask">{formatNumber(conversionResult.ask)}</span>
                  </div>
                  <div className="market-row">
                    <span className="market-label">Spread</span>
                    <span className={`market-value spread ${conversionResult.spread > 1 ? 'high' : 'low'}`}>
                      {conversionResult.spread.toFixed(4)}%
                    </span>
                  </div>
                </div>
                
                <div className="update-info">
                  ⟳ Last updated: {lastUpdate ? lastUpdate.toLocaleTimeString() : '--'}
                  <span className="pair-info">(Pair: {conversionResult.pairName})</span>
                </div>
              </>
            ) : (
              <div className="no-pair-state">
                <span className="warning-icon">⚠️</span>
                <span className="no-pair-text">No direct pair available on Kraken</span>
                <span className="no-pair-hint">
                  Try converting via USD, EUR, or BTC
                </span>
              </div>
            )
          ) : (
            <div className="empty-state">
              Select currencies to see conversion rate
            </div>
          )}
        </div>
      </div>

      <style jsx>{`
        .converter-header {
          display: flex;
          justify-content: space-between;
          align-items: center;
          margin-bottom: 30px;
        }

        .converter-header h2 {
          margin: 0;
          color: #fff;
        }

        .live-indicator {
          display: flex;
          align-items: center;
          gap: 8px;
          color: #00d4aa;
          font-size: 0.9rem;
          font-weight: 600;
        }

        .pulse {
          width: 10px;
          height: 10px;
          background: #00d4aa;
          border-radius: 50%;
          animation: pulse 2s infinite;
        }

        @keyframes pulse {
          0% { opacity: 1; transform: scale(1); }
          50% { opacity: 0.5; transform: scale(1.2); }
          100% { opacity: 1; transform: scale(1); }
        }

        .converter-container {
          background: #252542;
          border-radius: 16px;
          padding: 30px;
          max-width: 600px;
          margin: 0 auto;
        }

        .amount-input-wrapper {
          margin-bottom: 25px;
        }

        .amount-input-wrapper label {
          display: block;
          color: #888;
          font-size: 0.9rem;
          margin-bottom: 8px;
        }

        .amount-input {
          width: 100%;
          padding: 15px 20px;
          font-size: 1.5rem;
          background: #1a1a2e;
          border: 2px solid #3a3a5a;
          border-radius: 12px;
          color: #fff;
          outline: none;
          transition: border-color 0.2s;
          box-sizing: border-box;
        }

        .amount-input:focus {
          border-color: #00d4aa;
        }

        .currency-selection {
          display: flex;
          align-items: flex-end;
          gap: 15px;
          margin-bottom: 25px;
        }

        .currency-select-wrapper {
          flex: 1;
        }

        .currency-select-wrapper label {
          display: block;
          color: #888;
          font-size: 0.9rem;
          margin-bottom: 8px;
        }

        .currency-select {
          width: 100%;
          padding: 15px 20px;
          font-size: 1.1rem;
          background: #1a1a2e;
          border: 2px solid #3a3a5a;
          border-radius: 12px;
          color: #fff;
          outline: none;
          cursor: pointer;
          transition: border-color 0.2s;
        }

        .currency-select:focus {
          border-color: #00d4aa;
        }

        .currency-select optgroup {
          background: #1a1a2e;
          color: #00d4aa;
          font-weight: 600;
        }

        .currency-select option {
          background: #1a1a2e;
          color: #fff;
        }

        .swap-btn {
          padding: 15px 20px;
          font-size: 1.3rem;
          background: #3a3a5a;
          border: none;
          border-radius: 12px;
          color: #fff;
          cursor: pointer;
          transition: all 0.2s;
        }

        .swap-btn:hover {
          background: #00d4aa;
          color: #1a1a2e;
        }

        .conversion-result {
          background: #1a1a2e;
          border-radius: 12px;
          padding: 25px;
          min-height: 200px;
        }

        .loading-state, .error-state, .empty-state {
          display: flex;
          align-items: center;
          justify-content: center;
          height: 150px;
          color: #888;
          font-size: 1.1rem;
        }

        .error-state {
          color: #ff6b6b;
        }

        .spinner {
          width: 20px;
          height: 20px;
          border: 2px solid #3a3a5a;
          border-top-color: #00d4aa;
          border-radius: 50%;
          animation: spin 1s linear infinite;
          margin-right: 10px;
        }

        @keyframes spin {
          to { transform: rotate(360deg); }
        }

        .result-main {
          text-align: center;
          margin-bottom: 25px;
          padding-bottom: 20px;
          border-bottom: 1px solid #3a3a5a;
        }

        .result-amount {
          font-size: 2.5rem;
          font-weight: 700;
          color: #00d4aa;
          margin-right: 10px;
        }

        .result-currency {
          font-size: 1.5rem;
          color: #888;
        }

        .rate-info {
          margin-bottom: 20px;
          padding-bottom: 15px;
          border-bottom: 1px solid #3a3a5a;
        }

        .rate-row {
          display: flex;
          justify-content: space-between;
          align-items: center;
          padding: 8px 0;
        }

        .rate-label {
          color: #888;
          font-size: 0.95rem;
        }

        .rate-value {
          color: #fff;
          font-size: 0.95rem;
          font-weight: 500;
        }

        .market-info {
          display: grid;
          grid-template-columns: repeat(3, 1fr);
          gap: 15px;
          margin-bottom: 20px;
        }

        .market-row {
          display: flex;
          flex-direction: column;
          align-items: center;
          background: #252542;
          padding: 12px;
          border-radius: 8px;
        }

        .market-label {
          color: #888;
          font-size: 0.85rem;
          margin-bottom: 5px;
        }

        .market-value {
          font-size: 1rem;
          font-weight: 600;
        }

        .market-value.bid {
          color: #00d4aa;
        }

        .market-value.ask {
          color: #ff6b6b;
        }

        .market-value.spread.low {
          color: #00d4aa;
        }

        .market-value.spread.high {
          color: #ffc107;
        }

        .update-info {
          text-align: center;
          color: #666;
          font-size: 0.85rem;
        }

        .pair-info {
          margin-left: 10px;
          color: #555;
        }

        .no-pair-state {
          display: flex;
          flex-direction: column;
          align-items: center;
          justify-content: center;
          height: 150px;
          gap: 10px;
        }

        .warning-icon {
          font-size: 2rem;
        }

        .no-pair-text {
          color: #ffc107;
          font-size: 1.1rem;
          font-weight: 500;
        }

        .no-pair-hint {
          color: #888;
          font-size: 0.9rem;
        }

        @media (max-width: 600px) {
          .currency-selection {
            flex-direction: column;
            gap: 10px;
          }

          .swap-btn {
            transform: rotate(90deg);
            align-self: center;
          }

          .market-info {
            grid-template-columns: 1fr;
          }

          .result-amount {
            font-size: 2rem;
          }
        }
      `}</style>
    </div>
  );
}

export default PriceMatrix;
