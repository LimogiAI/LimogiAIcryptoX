import { useState, useEffect } from 'react'
import { Card, Button, Input } from '../ui'
import { api } from '../../services/api'
import type { ConfigurationStatus, FeeConfigStatus, RestrictionsConfig } from '../../types'

interface Props {
  configStatus: ConfigurationStatus | null
  onConfigured: () => void
  isTradingEnabled?: boolean
}

export function SetupDashboard({ configStatus, onConfigured, isTradingEnabled = false }: Props) {
  const [config, setConfig] = useState({
    base_currency: configStatus?.config_summary?.start_currency || '',
    trade_amount: configStatus?.config_summary?.trade_amount?.toString() || '',
    min_profit_threshold: configStatus?.config_summary?.min_profit_threshold != null
      ? (configStatus.config_summary.min_profit_threshold * 100).toString()
      : '',
    max_daily_loss: configStatus?.config_summary?.max_daily_loss?.toString() || '',
    max_total_loss: configStatus?.config_summary?.max_total_loss?.toString() || '',
    // Pair Selection Filters
    max_pairs: configStatus?.config_summary?.max_pairs?.toString() || '',
    min_volume_24h_usd: configStatus?.config_summary?.min_volume_24h_usd?.toString() || '',
    max_cost_min: configStatus?.config_summary?.max_cost_min?.toString() || '',
  })
  const [feeConfig, setFeeConfig] = useState<FeeConfigStatus | null>(configStatus?.fee_config || null)
  const [manualFees, setManualFees] = useState({
    maker_fee: '',
    taker_fee: '',
  })
  const [showManualFeeInput, setShowManualFeeInput] = useState(false)
  const [fetchingFees, setFetchingFees] = useState(false)
  const [feeError, setFeeError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // Restrictions state
  const [restrictions, setRestrictions] = useState<RestrictionsConfig | null>(null)
  const [restrictionsLoading, setRestrictionsLoading] = useState(true)
  const [newBlockedCurrency, setNewBlockedCurrency] = useState('')
  const [addingCurrency, setAddingCurrency] = useState(false)
  const [restrictionsError, setRestrictionsError] = useState<string | null>(null)

  const missingFields = configStatus?.missing_fields ?? []

  const isFieldMissing = (field: string) => {
    return missingFields.some(f => f.toLowerCase().includes(field.toLowerCase()))
  }

  // Load restrictions on mount
  useEffect(() => {
    const loadRestrictions = async () => {
      try {
        const data = await api.getRestrictions()
        setRestrictions(data)
      } catch (err) {
        console.error('Failed to load restrictions:', err)
      } finally {
        setRestrictionsLoading(false)
      }
    }
    loadRestrictions()
  }, [])

  const handleAddBlockedCurrency = async () => {
    if (!newBlockedCurrency.trim()) return

    setAddingCurrency(true)
    setRestrictionsError(null)
    try {
      const result = await api.addBlockedCurrency(newBlockedCurrency.trim().toUpperCase())
      if (result.success) {
        // Reload restrictions
        const data = await api.getRestrictions()
        setRestrictions(data)
        setNewBlockedCurrency('')
      } else {
        setRestrictionsError(result.message)
      }
    } catch (err: unknown) {
      let errorMessage = 'Failed to add currency'
      if (err && typeof err === 'object') {
        const axiosError = err as { response?: { data?: { message?: string; error?: string } } }
        if (axiosError.response?.data?.error) {
          errorMessage = axiosError.response.data.error
        } else if (axiosError.response?.data?.message) {
          errorMessage = axiosError.response.data.message
        }
      }
      setRestrictionsError(errorMessage)
    } finally {
      setAddingCurrency(false)
    }
  }

  const handleRemoveBlockedCurrency = async (currency: string) => {
    setRestrictionsError(null)
    try {
      const result = await api.removeBlockedCurrency(currency)
      if (result.success) {
        // Reload restrictions
        const data = await api.getRestrictions()
        setRestrictions(data)
      } else {
        setRestrictionsError(result.message)
      }
    } catch (err: unknown) {
      let errorMessage = 'Failed to remove currency'
      if (err && typeof err === 'object') {
        const axiosError = err as { response?: { data?: { message?: string; error?: string } } }
        if (axiosError.response?.data?.error) {
          errorMessage = axiosError.response.data.error
        } else if (axiosError.response?.data?.message) {
          errorMessage = axiosError.response.data.message
        }
      }
      setRestrictionsError(errorMessage)
    }
  }

  const handleFetchFees = async () => {
    setFetchingFees(true)
    setFeeError(null)
    try {
      const result = await api.fetchFeesFromKraken()
      if (result.success && result.data) {
        setFeeConfig({
          is_configured: true,
          maker_fee: result.data.maker_fee,
          taker_fee: result.data.taker_fee,
          fee_source: result.data.fee_source,
          volume_tier: null,
          thirty_day_volume: result.data.thirty_day_volume,
          last_fetched_at: result.data.last_fetched_at,
          last_updated_at: null,
        })
        setShowManualFeeInput(false)
      } else {
        setFeeError(result.error || result.message || 'Failed to fetch fees')
        setShowManualFeeInput(true)
      }
    } catch (err: unknown) {
      let errorMessage = 'Failed to fetch fees from Kraken'
      if (err && typeof err === 'object') {
        const axiosError = err as { response?: { data?: { message?: string; error?: string } }; message?: string }
        if (axiosError.response?.data?.error) {
          errorMessage = axiosError.response.data.error
        } else if (axiosError.response?.data?.message) {
          errorMessage = axiosError.response.data.message
        } else if (axiosError.message) {
          errorMessage = axiosError.message
        }
      }
      setFeeError(errorMessage)
      setShowManualFeeInput(true)
    } finally {
      setFetchingFees(false)
    }
  }

  const handleManualFeeSubmit = async () => {
    const makerFee = parseFloat(manualFees.maker_fee) / 100 // Convert from % to decimal
    const takerFee = parseFloat(manualFees.taker_fee) / 100

    if (isNaN(makerFee) || isNaN(takerFee) || makerFee < 0 || takerFee < 0) {
      setFeeError('Please enter valid fee percentages')
      return
    }

    setFetchingFees(true)
    setFeeError(null)
    try {
      const result = await api.updateFeeConfig(makerFee, takerFee)
      if (result.success) {
        setFeeConfig({
          is_configured: true,
          maker_fee: result.data.maker_fee,
          taker_fee: result.data.taker_fee,
          fee_source: result.data.fee_source,
          volume_tier: null,
          thirty_day_volume: null,
          last_fetched_at: null,
          last_updated_at: result.data.last_updated_at,
        })
        setShowManualFeeInput(false)
      }
    } catch (err: unknown) {
      let errorMessage = 'Failed to save fees'
      if (err && typeof err === 'object') {
        const axiosError = err as { response?: { data?: { message?: string; error?: string } }; message?: string }
        if (axiosError.response?.data?.error) {
          errorMessage = axiosError.response.data.error
        } else if (axiosError.response?.data?.message) {
          errorMessage = axiosError.response.data.message
        }
      }
      setFeeError(errorMessage)
    } finally {
      setFetchingFees(false)
    }
  }

  const handleSave = async () => {
    setSaving(true)
    setError(null)

    try {
      await api.updateLiveConfig({
        base_currency: config.base_currency,
        trade_amount: parseFloat(config.trade_amount) || 0,
        min_profit_threshold: (parseFloat(config.min_profit_threshold) || 0) / 100,
        max_daily_loss: parseFloat(config.max_daily_loss) || 0,
        max_total_loss: parseFloat(config.max_total_loss) || 0,
        // Pair Selection Filters
        max_pairs: parseInt(config.max_pairs) || 0,
        min_volume_24h_usd: parseFloat(config.min_volume_24h_usd) || 0,
        max_cost_min: parseFloat(config.max_cost_min) || 0,
      })
      onConfigured()
    } catch (err: unknown) {
      // Handle axios errors which have response.data.message
      let errorMessage = 'Failed to save configuration'
      if (err && typeof err === 'object') {
        const axiosError = err as { response?: { data?: { message?: string } }; message?: string }
        if (axiosError.response?.data?.message) {
          errorMessage = axiosError.response.data.message
        } else if (axiosError.message) {
          errorMessage = axiosError.message
        }
      }
      setError(errorMessage)
      console.error('Save config error:', err)
    } finally {
      setSaving(false)
    }
  }

  const isFormValid = () => {
    return (
      config.base_currency &&
      parseFloat(config.trade_amount) > 0 &&
      config.min_profit_threshold !== '' &&
      parseFloat(config.max_daily_loss) > 0 &&
      parseFloat(config.max_total_loss) > 0 &&
      // Pair Selection Filters validation
      parseInt(config.max_pairs) > 0 &&
      parseFloat(config.min_volume_24h_usd) > 0 &&
      parseFloat(config.max_cost_min) > 0 &&
      feeConfig?.is_configured
    )
  }

  return (
    <div className="max-w-2xl mx-auto px-4 py-8">
      {/* Welcome Banner */}
      <div className="text-center mb-10">
        <h1 className="text-3xl font-bold mb-3 text-gradient">
          LimogiAI Crypto Trading
        </h1>
        <p className="text-text-secondary text-lg">
          Configure your trading parameters to get started
        </p>
      </div>

      {/* Trading Active Warning */}
      {isTradingEnabled && (
        <div className="mb-6 p-4 bg-accent-warning/10 border border-accent-warning/30 rounded-lg flex items-center gap-3">
          <div className="flex-shrink-0 w-10 h-10 bg-accent-warning/20 rounded-full flex items-center justify-center">
            <svg className="w-5 h-5 text-accent-warning" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
            </svg>
          </div>
          <div>
            <h4 className="font-medium text-accent-warning">Trading is Active</h4>
            <p className="text-sm text-text-secondary">
              Settings are locked while trading is running. Stop trading from the dashboard to modify settings.
            </p>
          </div>
        </div>
      )}

      {/* Configuration Cards */}
      <div className="space-y-6">
        {/* Start Currency Card */}
        <Card
          title="Start Currencies"
          description="Select the currencies you want to trade with (multi-select)"
          status={isFieldMissing('currency') ? 'required' : config.base_currency ? 'complete' : 'default'}
        >
          {(() => {
            // Parse current selection into array
            const selectedCurrencies = config.base_currency
              ? config.base_currency === 'ALL'
                ? ['USD', 'EUR']
                : config.base_currency.split(',').map(c => c.trim()).filter(Boolean)
              : [];

            // Available currencies (common Kraken quote currencies)
            const availableCurrencies = ['USD', 'EUR', 'GBP', 'CAD', 'AUD', 'CHF', 'JPY'];

            // Toggle a currency selection
            const toggleCurrency = (currency: string) => {
              const newSelection = selectedCurrencies.includes(currency)
                ? selectedCurrencies.filter(c => c !== currency)
                : [...selectedCurrencies, currency];

              // Store as comma-separated or empty
              const newValue = newSelection.length > 0 ? newSelection.join(',') : '';
              setConfig(c => ({ ...c, base_currency: newValue }));
            };

            // Add a custom currency
            const addCustomCurrency = (currency: string) => {
              const normalized = currency.trim().toUpperCase();
              if (normalized && !selectedCurrencies.includes(normalized)) {
                const newSelection = [...selectedCurrencies, normalized];
                setConfig(c => ({ ...c, base_currency: newSelection.join(',') }));
              }
            };

            return (
              <div className="space-y-4">
                {/* Common currencies - multi-select */}
                <div className="flex flex-wrap gap-2">
                  {availableCurrencies.map(currency => (
                    <button
                      key={currency}
                      onClick={() => toggleCurrency(currency)}
                      disabled={isTradingEnabled}
                      className={`px-4 py-2 rounded-lg font-medium transition-all ${
                        selectedCurrencies.includes(currency)
                          ? 'bg-accent-primary text-white'
                          : 'bg-bg-tertiary text-text-secondary hover:bg-bg-tertiary/80 border border-border'
                      } ${isTradingEnabled ? 'opacity-50 cursor-not-allowed' : ''}`}
                    >
                      {currency}
                      {selectedCurrencies.includes(currency) && (
                        <span className="ml-2">✓</span>
                      )}
                    </button>
                  ))}
                </div>

                {/* Custom currency input */}
                <div className="flex gap-2">
                  <Input
                    placeholder="Add other currency (e.g., USDT)"
                    className="flex-1"
                    disabled={isTradingEnabled}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' && !isTradingEnabled) {
                        addCustomCurrency((e.target as HTMLInputElement).value);
                        (e.target as HTMLInputElement).value = '';
                      }
                    }}
                  />
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={isTradingEnabled}
                    onClick={(e) => {
                      const input = (e.target as HTMLElement).parentElement?.querySelector('input');
                      if (input) {
                        addCustomCurrency(input.value);
                        input.value = '';
                      }
                    }}
                  >
                    Add
                  </Button>
                </div>

                {/* Selected summary */}
                <p className="text-sm text-text-muted">
                  {selectedCurrencies.length > 0
                    ? `Trading pairs ending in: ${selectedCurrencies.join(', ')}`
                    : 'Select at least one currency to trade with'}
                </p>
              </div>
            );
          })()}
        </Card>

        {/* Trade Amount Card */}
        <Card
          title="Trade Amount"
          description="Amount to use per trade in your start currency"
          status={isFieldMissing('trade amount') ? 'required' : parseFloat(config.trade_amount) > 0 ? 'complete' : 'default'}
        >
          <Input
            type="number"
            placeholder="e.g., 50"
            value={config.trade_amount}
            onChange={(e) => setConfig(c => ({ ...c, trade_amount: e.target.value }))}
            prefix="$"
            disabled={isTradingEnabled}
          />
          <p className="text-sm text-text-muted mt-3">
            Recommended: $20 - $100 for testing
          </p>
        </Card>

        {/* Profit Threshold Card */}
        <Card
          title="Minimum Profit Threshold"
          description="Execute trades with profit above this percentage (negative = accept losses)"
          status={isFieldMissing('profit') ? 'required' : config.min_profit_threshold !== '' ? 'complete' : 'default'}
        >
          <Input
            type="number"
            step="0.01"
            placeholder="e.g., 0.15 or -0.5"
            value={config.min_profit_threshold}
            onChange={(e) => setConfig(c => ({ ...c, min_profit_threshold: e.target.value }))}
            suffix="%"
            disabled={isTradingEnabled}
          />
          <p className="text-sm text-text-muted mt-3">
            Recommended: 0.10% - 0.30%. Use negative values to test execution with losses.
          </p>
        </Card>

        {/* Risk Management Card */}
        <Card
          title="Risk Management"
          description="Set your loss limits to protect your capital"
          status={
            isFieldMissing('daily loss') || isFieldMissing('total loss')
              ? 'required'
              : parseFloat(config.max_daily_loss) > 0 && parseFloat(config.max_total_loss) > 0
              ? 'complete'
              : 'default'
          }
        >
          <div className="grid grid-cols-2 gap-4">
            <Input
              label="Daily Loss Limit"
              type="number"
              placeholder="e.g., 10"
              value={config.max_daily_loss}
              onChange={(e) => setConfig(c => ({ ...c, max_daily_loss: e.target.value }))}
              prefix="$"
              disabled={isTradingEnabled}
            />
            <Input
              label="Total Loss Limit"
              type="number"
              placeholder="e.g., 50"
              value={config.max_total_loss}
              onChange={(e) => setConfig(c => ({ ...c, max_total_loss: e.target.value }))}
              prefix="$"
              disabled={isTradingEnabled}
            />
          </div>
          <p className="text-sm text-text-muted mt-3">
            Trading stops automatically when limits are reached
          </p>
        </Card>

        {/* Pair Selection Filters Card */}
        <Card
          title="Pair Selection Filters"
          description="Configure which trading pairs to monitor for arbitrage opportunities"
          status={
            isFieldMissing('max_pairs') || isFieldMissing('volume') || isFieldMissing('cost')
              ? 'required'
              : parseInt(config.max_pairs) > 0 && parseFloat(config.min_volume_24h_usd) > 0 && parseFloat(config.max_cost_min) > 0
              ? 'complete'
              : 'default'
          }
        >
          <div className="space-y-4">
            <Input
              label="Maximum Pairs"
              type="number"
              placeholder="e.g., 50"
              value={config.max_pairs}
              onChange={(e) => setConfig(c => ({ ...c, max_pairs: e.target.value }))}
              suffix="pairs"
              disabled={isTradingEnabled}
            />
            <p className="text-xs text-text-muted -mt-2">
              Limits how many trading pairs to monitor (30-100 recommended)
            </p>

            <Input
              label="Minimum 24h Volume"
              type="number"
              placeholder="e.g., 50000"
              value={config.min_volume_24h_usd}
              onChange={(e) => setConfig(c => ({ ...c, min_volume_24h_usd: e.target.value }))}
              prefix="$"
              disabled={isTradingEnabled}
            />
            <p className="text-xs text-text-muted -mt-2">
              Pairs with less than this daily volume are excluded ($50,000+ recommended)
            </p>

            <Input
              label="Maximum Order Minimum"
              type="number"
              placeholder="e.g., 20"
              value={config.max_cost_min}
              onChange={(e) => setConfig(c => ({ ...c, max_cost_min: e.target.value }))}
              prefix="$"
              disabled={isTradingEnabled}
            />
            <p className="text-xs text-text-muted -mt-2">
              Pairs requiring more than this minimum order are excluded ($20+ recommended)
            </p>
          </div>
        </Card>

        {/* Fee Configuration Card */}
        <Card
          title="Trading Fees"
          description="Maker/Taker fees from your Kraken account"
          status={isFieldMissing('fees') ? 'required' : feeConfig?.is_configured ? 'complete' : 'default'}
        >
          {feeConfig?.is_configured ? (
            <div className="space-y-3">
              <div className="grid grid-cols-2 gap-4">
                <div className="bg-bg-tertiary p-3 rounded-lg">
                  <div className="text-sm text-text-muted">Maker Fee</div>
                  <div className="text-xl font-bold text-accent-success">
                    {((feeConfig.maker_fee || 0) * 100).toFixed(2)}%
                  </div>
                </div>
                <div className="bg-bg-tertiary p-3 rounded-lg">
                  <div className="text-sm text-text-muted">Taker Fee</div>
                  <div className="text-xl font-bold text-accent-warning">
                    {((feeConfig.taker_fee || 0) * 100).toFixed(2)}%
                  </div>
                </div>
              </div>
              <div className="flex items-center justify-between text-sm">
                <span className="text-text-muted">
                  Source: <span className="text-text-primary">
                    {feeConfig.fee_source === 'kraken_api' ? 'Kraken API' : 'Manual'}
                  </span>
                </span>
                {feeConfig.thirty_day_volume && (
                  <span className="text-text-muted">
                    30d Volume: ${feeConfig.thirty_day_volume.toLocaleString()}
                  </span>
                )}
              </div>
              <Button
                variant="outline"
                size="sm"
                onClick={handleFetchFees}
                loading={fetchingFees}
                disabled={isTradingEnabled}
                className="w-full mt-2"
              >
                Refresh Fees from Kraken
              </Button>
            </div>
          ) : (
            <div className="space-y-4">
              {!showManualFeeInput ? (
                <>
                  <Button
                    size="lg"
                    onClick={handleFetchFees}
                    loading={fetchingFees}
                    disabled={isTradingEnabled}
                    fullWidth
                  >
                    {fetchingFees ? 'Fetching...' : 'Fetch Fees from Kraken'}
                  </Button>
                  <div className="text-center text-sm text-text-muted">
                    or{' '}
                    <button
                      className={`text-accent-primary hover:underline ${isTradingEnabled ? 'opacity-50 cursor-not-allowed' : ''}`}
                      onClick={() => !isTradingEnabled && setShowManualFeeInput(true)}
                      disabled={isTradingEnabled}
                    >
                      enter fees manually
                    </button>
                  </div>
                </>
              ) : (
                <>
                  <div className="grid grid-cols-2 gap-4">
                    <Input
                      label="Maker Fee"
                      type="number"
                      step="0.01"
                      placeholder="e.g., 0.16"
                      value={manualFees.maker_fee}
                      onChange={(e) => setManualFees(f => ({ ...f, maker_fee: e.target.value }))}
                      suffix="%"
                      disabled={isTradingEnabled}
                    />
                    <Input
                      label="Taker Fee"
                      type="number"
                      step="0.01"
                      placeholder="e.g., 0.26"
                      value={manualFees.taker_fee}
                      onChange={(e) => setManualFees(f => ({ ...f, taker_fee: e.target.value }))}
                      suffix="%"
                      disabled={isTradingEnabled}
                    />
                  </div>
                  <div className="flex gap-2">
                    <Button
                      variant="outline"
                      onClick={() => {
                        setShowManualFeeInput(false)
                        setFeeError(null)
                      }}
                      className="flex-1"
                    >
                      Cancel
                    </Button>
                    <Button
                      onClick={handleManualFeeSubmit}
                      loading={fetchingFees}
                      disabled={isTradingEnabled}
                      className="flex-1"
                    >
                      Save Fees
                    </Button>
                  </div>
                </>
              )}
              {feeError && (
                <div className="p-3 bg-accent-danger/10 border border-accent-danger/30 rounded-lg text-sm text-accent-danger">
                  {feeError}
                </div>
              )}
            </div>
          )}
        </Card>

        {/* Geographic Restrictions Card */}
        <Card
          title={`Geographic Restrictions${restrictions ? ` (${restrictions.jurisdiction_name})` : ''}`}
          description="Currencies blocked due to regulatory requirements"
          status="info"
        >
          {restrictionsLoading ? (
            <div className="flex items-center justify-center py-4">
              <div className="animate-spin rounded-full h-5 w-5 border-2 border-accent-primary border-t-transparent" />
              <span className="ml-2 text-text-muted">Loading restrictions...</span>
            </div>
          ) : restrictions ? (
            <div className="space-y-4">
              {/* Regulatory Info */}
              <div className="flex items-center gap-2 text-sm text-text-secondary">
                <span className="px-2 py-0.5 bg-accent-warning/20 text-accent-warning rounded text-xs font-medium">
                  {restrictions.regulatory_body}
                </span>
                <span className="text-text-muted">|</span>
                <span>Last updated: {new Date(restrictions.last_updated).toLocaleDateString()}</span>
              </div>

              {/* Blocked Currencies */}
              <div>
                <div className="text-sm font-medium text-text-secondary mb-2">Blocked Currencies:</div>
                <div className="flex flex-wrap gap-2">
                  {restrictions.blocked_base_currencies.map((currency) => (
                    <span
                      key={currency}
                      className="inline-flex items-center gap-1 px-2.5 py-1 bg-accent-danger/10 text-accent-danger rounded-lg text-sm font-medium"
                    >
                      {currency}
                      <button
                        onClick={() => handleRemoveBlockedCurrency(currency)}
                        disabled={isTradingEnabled}
                        className={`ml-1 hover:bg-accent-danger/20 rounded p-0.5 transition-colors ${isTradingEnabled ? 'opacity-50 cursor-not-allowed' : ''}`}
                        title={isTradingEnabled ? "Cannot modify while trading" : "Remove restriction"}
                      >
                        <svg className="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
                        </svg>
                      </button>
                    </span>
                  ))}
                </div>
              </div>

              {/* Add Custom Currency */}
              <div className="flex gap-2">
                <Input
                  placeholder="Add currency (e.g., DOGE)"
                  value={newBlockedCurrency}
                  onChange={(e) => setNewBlockedCurrency(e.target.value.toUpperCase())}
                  className="flex-1"
                  disabled={isTradingEnabled}
                />
                <Button
                  variant="outline"
                  size="sm"
                  onClick={handleAddBlockedCurrency}
                  loading={addingCurrency}
                  disabled={!newBlockedCurrency.trim() || isTradingEnabled}
                >
                  Add
                </Button>
              </div>

              {restrictionsError && (
                <div className="p-2 bg-accent-danger/10 border border-accent-danger/30 rounded text-sm text-accent-danger">
                  {restrictionsError}
                </div>
              )}

              {/* Notes */}
              <div className="p-3 bg-bg-tertiary rounded-lg text-sm text-text-muted">
                <div className="font-medium text-text-secondary mb-1">Note:</div>
                {restrictions.notes}
              </div>

              {/* Sources */}
              {restrictions.sources.length > 0 && (
                <div className="text-xs text-text-muted">
                  Sources:{' '}
                  {restrictions.sources.map((source, idx) => (
                    <span key={idx}>
                      <a
                        href={source}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="text-accent-primary hover:underline"
                      >
                        {source.includes('kraken') ? 'Kraken Support' : 'Regulatory Guidelines'}
                      </a>
                      {idx < restrictions.sources.length - 1 && ', '}
                    </span>
                  ))}
                </div>
              )}
            </div>
          ) : (
            <div className="text-text-muted text-sm">No restrictions configured</div>
          )}
        </Card>
      </div>

      {/* Error Message */}
      {error && (
        <div className="mt-6 p-4 bg-accent-danger/10 border border-accent-danger/30 rounded-lg text-accent-danger">
          {error}
        </div>
      )}

      {/* Warnings */}
      {configStatus?.warnings && configStatus.warnings.length > 0 && (
        <div className="mt-6 p-4 bg-accent-warning/10 border border-accent-warning/30 rounded-lg">
          <h4 className="font-medium text-accent-warning mb-2">Warnings</h4>
          <ul className="text-sm text-text-secondary space-y-1">
            {configStatus.warnings.map((warning, idx) => (
              <li key={idx}>&#8226; {warning}</li>
            ))}
          </ul>
        </div>
      )}

      {/* Save Button */}
      <div className="mt-8">
        <Button
          size="lg"
          fullWidth
          onClick={handleSave}
          loading={saving}
          disabled={!isFormValid() || isTradingEnabled}
        >
          {isTradingEnabled ? 'Settings Locked While Trading' : saving ? 'Saving...' : 'Save Configuration & Continue'}
        </Button>
      </div>

      {/* Footer Note */}
      <p className="text-center text-sm text-text-muted mt-6">
        You can modify these settings later from the dashboard
      </p>
    </div>
  )
}
