import { useState, useEffect, useCallback } from 'react'
import { Card, Badge, Button } from '../ui'
import { api } from '../../services/api'
import type { LiveTrade } from '../../types'

export function PartialTrades() {
  const [trades, setTrades] = useState<LiveTrade[]>([])
  const [loading, setLoading] = useState(true)
  const [resolving, setResolving] = useState<string | null>(null)

  const fetchTrades = useCallback(async () => {
    try {
      const data = await api.getLivePartialTrades()
      setTrades(data)
    } catch (error) {
      console.error('Failed to fetch partial trades:', error)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    fetchTrades()
    const interval = setInterval(fetchTrades, 10000)
    return () => clearInterval(interval)
  }, [fetchTrades])

  const handleResolve = async (tradeId: string) => {
    setResolving(tradeId)
    try {
      await api.resolvePartialTrade(tradeId)
      await fetchTrades()
    } catch (error) {
      console.error('Failed to resolve trade:', error)
    } finally {
      setResolving(null)
    }
  }

  if (loading) {
    return (
      <Card className="animate-pulse">
        <div className="h-32 bg-bg-tertiary rounded" />
      </Card>
    )
  }

  if (trades.length === 0) {
    return null
  }

  return (
    <Card status="warning">
      <div className="flex items-center justify-between mb-4">
        <div>
          <h3 className="text-lg font-semibold">Partial Trades</h3>
          <p className="text-sm text-text-secondary mt-1">
            These trades did not complete fully and are holding crypto
          </p>
        </div>
        <Badge variant="warning">{trades.length} pending</Badge>
      </div>

      <div className="space-y-3">
        {trades.map((trade) => (
          <div
            key={trade.id}
            className="p-4 bg-bg-tertiary rounded-lg border border-border"
          >
            <div className="flex items-start justify-between">
              <div>
                <p className="font-mono text-sm">{trade.path}</p>
                <div className="flex items-center gap-3 mt-2 text-sm text-text-secondary">
                  <span>
                    Holding: <strong className="text-text-primary">{trade.held_amount?.toFixed(6)} {trade.held_currency}</strong>
                  </span>
                  {trade.held_value_usd && (
                    <span>
                      Value: <strong className="text-text-primary">${trade.held_value_usd.toFixed(2)}</strong>
                    </span>
                  )}
                </div>
                <p className="text-xs text-text-muted mt-2">
                  Stopped at leg {trade.current_leg} of {trade.legs}
                  {trade.error_message && ` - ${trade.error_message}`}
                </p>
              </div>
              <Button
                variant="primary"
                size="sm"
                onClick={() => handleResolve(trade.trade_id)}
                loading={resolving === trade.trade_id}
                disabled={resolving !== null}
              >
                Resolve
              </Button>
            </div>
          </div>
        ))}
      </div>
    </Card>
  )
}
