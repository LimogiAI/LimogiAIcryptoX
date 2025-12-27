import { useState, useEffect, useCallback } from 'react'
import { Button, Card, Badge } from '../ui'
import { api } from '../../services/api'
import type { LiveConfig, LiveState, ScannerStatus } from '../../types'

interface Props {
  onRefresh?: () => void
}

export function TradingControls({ onRefresh }: Props) {
  const [config, setConfig] = useState<LiveConfig | null>(null)
  const [state, setState] = useState<LiveState | null>(null)
  const [scanner, setScanner] = useState<ScannerStatus | null>(null)
  const [loading, setLoading] = useState(true)
  const [actionLoading, setActionLoading] = useState<string | null>(null)

  const fetchData = useCallback(async () => {
    try {
      const [configRes, stateRes, scannerRes] = await Promise.all([
        api.getLiveConfig(),
        api.getLiveState(),
        api.getLiveScannerStatus(),
      ])
      setConfig(configRes)
      setState(stateRes)
      setScanner(scannerRes)
    } catch (error) {
      console.error('Failed to fetch trading data:', error)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    fetchData()
    const interval = setInterval(fetchData, 2000)
    return () => clearInterval(interval)
  }, [fetchData])

  const handleStartTrading = async () => {
    setActionLoading('start')
    try {
      await api.enableLiveTrading(true, 'I understand the risks')
      await fetchData()
      onRefresh?.()
    } catch (error) {
      console.error('Failed to start trading:', error)
    } finally {
      setActionLoading(null)
    }
  }

  const handleStopTrading = async () => {
    setActionLoading('stop')
    try {
      await api.disableLiveTrading('Manual stop from dashboard')
      await fetchData()
      onRefresh?.()
    } catch (error) {
      console.error('Failed to stop trading:', error)
    } finally {
      setActionLoading(null)
    }
  }

  const handleResetCircuitBreaker = async () => {
    setActionLoading('reset-cb')
    try {
      await api.resetLiveCircuitBreaker()
      await fetchData()
    } catch (error) {
      console.error('Failed to reset circuit breaker:', error)
    } finally {
      setActionLoading(null)
    }
  }

  if (loading) {
    return (
      <Card className="animate-pulse">
        <div className="h-32 bg-bg-tertiary rounded" />
      </Card>
    )
  }

  const isTrading = config?.is_enabled
  const isCircuitBroken = state?.is_circuit_broken
  const isScannerRunning = scanner?.is_running

  return (
    <div className="space-y-4">
      {/* Main HFT Control - Scanner + Execution unified */}
      <Card>
        <div className="flex items-center justify-between mb-4">
          <div>
            <h3 className="text-lg font-semibold">HFT Engine</h3>
            <div className="flex items-center gap-2 mt-1">
              <Badge variant={isTrading ? 'success' : 'default'} dot>
                {isTrading ? 'RUNNING' : 'STOPPED'}
              </Badge>
              {isTrading && isScannerRunning && (
                <Badge variant="info">
                  Scanning
                </Badge>
              )}
              {isCircuitBroken && (
                <Badge variant="danger" dot>
                  Circuit Broken
                </Badge>
              )}
            </div>
          </div>
          <div className="flex gap-2">
            {isTrading ? (
              <Button
                variant="danger"
                onClick={handleStopTrading}
                loading={actionLoading === 'stop'}
                disabled={actionLoading !== null}
              >
                Stop HFT
              </Button>
            ) : (
              <Button
                variant="success"
                onClick={handleStartTrading}
                loading={actionLoading === 'start'}
                disabled={actionLoading !== null || isCircuitBroken}
              >
                Start HFT
              </Button>
            )}
          </div>
        </div>

        {/* Circuit Breaker Warning */}
        {isCircuitBroken && (
          <div className="p-3 bg-accent-danger/10 border border-accent-danger/30 rounded-lg mb-4">
            <div className="flex items-center justify-between">
              <div>
                <p className="text-accent-danger font-medium">Circuit Breaker Triggered</p>
                <p className="text-sm text-text-secondary mt-1">
                  {state?.circuit_broken_reason || 'Trading halted due to loss limits'}
                </p>
              </div>
              <Button
                variant="outline"
                size="sm"
                onClick={handleResetCircuitBreaker}
                loading={actionLoading === 'reset-cb'}
              >
                Reset
              </Button>
            </div>
          </div>
        )}

        {/* Config Summary */}
        <div className="grid grid-cols-2 md:grid-cols-4 gap-4 pt-4 border-t border-border">
          <div>
            <p className="text-xs text-text-muted uppercase">Currency</p>
            <p className="text-lg font-semibold">{config?.base_currency || 'N/A'}</p>
          </div>
          <div>
            <p className="text-xs text-text-muted uppercase">Trade Amount</p>
            <p className="text-lg font-semibold">${config?.trade_amount?.toFixed(2) || '0.00'}</p>
          </div>
          <div>
            <p className="text-xs text-text-muted uppercase">Min Profit</p>
            <p className="text-lg font-semibold">{((config?.min_profit_threshold ?? 0) * 100).toFixed(5)}%</p>
          </div>
          <div>
            <p className="text-xs text-text-muted uppercase">Daily Limit</p>
            <p className="text-lg font-semibold">${config?.max_daily_loss?.toFixed(2) || '0.00'}</p>
          </div>
        </div>
      </Card>

      {/* Scanner Status (info only - controlled by Start/Stop Trading) */}
      {isTrading && (
        <Card>
          <div className="flex items-center justify-between">
            <div>
              <h3 className="text-lg font-semibold">Scanner Status</h3>
              <div className="flex items-center gap-2 mt-1">
                <Badge variant={isScannerRunning ? 'success' : 'warning'} dot>
                  {isScannerRunning ? 'SCANNING' : 'WAITING'}
                </Badge>
                {scanner?.pairs_scanned && (
                  <span className="text-sm text-text-secondary">
                    {scanner.pairs_scanned} pairs monitored
                  </span>
                )}
              </div>
            </div>
          </div>

          {scanner?.last_scan_at && (
            <div className="mt-3 pt-3 border-t border-border text-sm text-text-secondary">
              Last scan: {new Date(scanner.last_scan_at).toLocaleTimeString()}
              {scanner.scan_duration_ms > 0 && ` (${scanner.scan_duration_ms}ms)`}
            </div>
          )}
        </Card>
      )}

      {/* P&L Summary */}
      <Card>
        <h3 className="text-lg font-semibold mb-4">Performance</h3>
        <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
          <div>
            <p className="text-xs text-text-muted uppercase">Daily P/L</p>
            <p className={`text-lg font-semibold ${
              (state?.daily_profit ?? 0) - (state?.daily_loss ?? 0) >= 0
                ? 'text-accent-success'
                : 'text-accent-danger'
            }`}>
              ${((state?.daily_profit ?? 0) - (state?.daily_loss ?? 0)).toFixed(2)}
            </p>
          </div>
          <div>
            <p className="text-xs text-text-muted uppercase">Total P/L</p>
            <p className={`text-lg font-semibold ${
              (state?.total_profit ?? 0) - (state?.total_loss ?? 0) >= 0
                ? 'text-accent-success'
                : 'text-accent-danger'
            }`}>
              ${((state?.total_profit ?? 0) - (state?.total_loss ?? 0)).toFixed(2)}
            </p>
          </div>
          <div>
            <p className="text-xs text-text-muted uppercase">Daily Trades</p>
            <p className="text-lg font-semibold">{state?.daily_trades ?? 0}</p>
          </div>
          <div>
            <p className="text-xs text-text-muted uppercase">Win Rate</p>
            <p className="text-lg font-semibold">
              {state?.total_trades ? ((state.total_wins / state.total_trades) * 100).toFixed(1) : '0.0'}%
            </p>
          </div>
        </div>
      </Card>
    </div>
  )
}
