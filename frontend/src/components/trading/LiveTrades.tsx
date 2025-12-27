import { useState, useEffect, useCallback, Fragment } from 'react'
import { Card, Badge, Button } from '../ui'
import { api } from '../../services/api'
import type { LiveTrade, LegFill } from '../../types'

export function LiveTrades() {
  const [trades, setTrades] = useState<LiveTrade[]>([])
  const [loading, setLoading] = useState(true)
  const [filter, setFilter] = useState<'all' | 'completed' | 'partial' | 'failed'>('all')
  const [expandedTradeId, setExpandedTradeId] = useState<number | null>(null)

  const fetchTrades = useCallback(async () => {
    try {
      const status = filter === 'all' ? null : filter.toUpperCase()
      const data = await api.getLiveTrades(50, status)
      setTrades(data)
    } catch (error) {
      console.error('Failed to fetch trades:', error)
    } finally {
      setLoading(false)
    }
  }, [filter])

  useEffect(() => {
    fetchTrades()
    const interval = setInterval(fetchTrades, 5000)
    return () => clearInterval(interval)
  }, [fetchTrades])

  const getStatusBadge = (status: string) => {
    switch (status.toUpperCase()) {
      case 'COMPLETED':
        return <Badge variant="success">Completed</Badge>
      case 'PARTIAL':
        return <Badge variant="warning">Partial</Badge>
      case 'FAILED':
        return <Badge variant="danger">Failed</Badge>
      case 'EXECUTING':
        return <Badge variant="info">Executing</Badge>
      default:
        return <Badge>{status}</Badge>
    }
  }

  const formatTime = (timestamp: string | null) => {
    if (!timestamp) return '-'
    const date = new Date(timestamp)
    // Format as Eastern Time with milliseconds
    const options: Intl.DateTimeFormatOptions = {
      timeZone: 'America/New_York',
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
      hour12: false,
    }
    const formatted = date.toLocaleString('en-US', options)
    const ms = date.getMilliseconds().toString().padStart(3, '0')
    return `${formatted}.${ms} ET`
  }

  const formatProfitLoss = (trade: LiveTrade) => {
    if (trade.profit_loss === null) return '-'
    const pct = trade.profit_loss_pct ?? 0
    const isProfit = trade.profit_loss >= 0
    return (
      <span className={isProfit ? 'text-accent-success' : 'text-accent-danger'}>
        {isProfit ? '+' : ''}${trade.profit_loss.toFixed(4)} ({pct.toFixed(3)}%)
      </span>
    )
  }

  const toggleExpanded = (tradeId: number) => {
    setExpandedTradeId(expandedTradeId === tradeId ? null : tradeId)
  }

  const hasLegData = (trade: LiveTrade) => {
    return trade.leg_fills && trade.leg_fills.length > 0
  }

  const renderLegBreakdown = (legFills: LegFill[]) => {
    return (
      <div className="px-4 py-3 bg-bg-secondary border-t border-border/30">
        <div className="text-xs text-text-muted mb-2 font-medium">Leg Execution Breakdown</div>
        <div className="grid gap-2">
          {legFills.map((leg, idx) => (
            <div
              key={idx}
              className="flex items-center justify-between py-1.5 px-3 bg-bg-tertiary rounded text-xs"
            >
              <div className="flex items-center gap-3">
                <span className="font-mono text-text-muted w-8">L{leg.leg}</span>
                <span className="font-mono">{leg.pair}</span>
                <Badge variant={leg.side === 'BUY' ? 'success' : 'danger'} className="text-xs">
                  {leg.side}
                </Badge>
              </div>
              <div className="flex items-center gap-4">
                <span className={`font-mono ${leg.success ? 'text-accent-success' : 'text-accent-danger'}`}>
                  {leg.duration_ms}ms
                </span>
                {leg.success ? (
                  <span className="text-accent-success">OK</span>
                ) : (
                  <span className="text-accent-danger" title={leg.error || 'Failed'}>
                    FAIL
                  </span>
                )}
              </div>
            </div>
          ))}
        </div>
        <div className="mt-2 text-xs text-text-muted text-right">
          Total: {legFills.reduce((sum, leg) => sum + leg.duration_ms, 0)}ms
        </div>
      </div>
    )
  }

  if (loading) {
    return (
      <Card className="animate-pulse">
        <div className="h-64 bg-bg-tertiary rounded" />
      </Card>
    )
  }

  return (
    <Card>
      <div className="flex items-center justify-between mb-4">
        <h3 className="text-lg font-semibold">Recent Trades</h3>
        <div className="flex gap-2">
          {(['all', 'completed', 'partial', 'failed'] as const).map((f) => (
            <Button
              key={f}
              variant={filter === f ? 'primary' : 'ghost'}
              size="sm"
              onClick={() => setFilter(f)}
            >
              {f.charAt(0).toUpperCase() + f.slice(1)}
            </Button>
          ))}
        </div>
      </div>

      {trades.length === 0 ? (
        <div className="text-center py-12 text-text-muted">
          <p>No trades yet</p>
          <p className="text-sm mt-1">Trades will appear here when the scanner finds opportunities</p>
        </div>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full">
            <thead>
              <tr className="border-b border-border text-left text-sm text-text-muted">
                <th className="pb-3 font-medium">Time</th>
                <th className="pb-3 font-medium">Path</th>
                <th className="pb-3 font-medium">Status</th>
                <th className="pb-3 font-medium text-right">Amount</th>
                <th className="pb-3 font-medium text-right">P/L</th>
                <th className="pb-3 font-medium text-right">Duration</th>
              </tr>
            </thead>
            <tbody className="text-sm">
              {trades.map((trade) => (
                <Fragment key={trade.id}>
                  <tr
                    className={`border-b border-border/50 hover:bg-bg-tertiary/50 ${
                      hasLegData(trade) ? 'cursor-pointer' : ''
                    } ${expandedTradeId === trade.id ? 'bg-bg-tertiary/30' : ''}`}
                    onClick={() => hasLegData(trade) && toggleExpanded(trade.id)}
                  >
                    <td className="py-3 text-text-secondary">
                      <div className="flex items-center gap-2">
                        {hasLegData(trade) && (
                          <span className="text-text-muted text-xs">
                            {expandedTradeId === trade.id ? '[-]' : '[+]'}
                          </span>
                        )}
                        {formatTime(trade.created_at)}
                      </div>
                    </td>
                    <td className="py-3 font-mono text-xs">
                      {trade.path}
                    </td>
                    <td className="py-3">
                      {getStatusBadge(trade.status)}
                    </td>
                    <td className="py-3 text-right">
                      ${trade.amount_in.toFixed(2)}
                    </td>
                    <td className="py-3 text-right">
                      {formatProfitLoss(trade)}
                    </td>
                    <td className="py-3 text-right text-text-secondary">
                      {trade.total_execution_ms ? `${trade.total_execution_ms.toFixed(0)}ms` : '-'}
                    </td>
                  </tr>
                  {expandedTradeId === trade.id && trade.leg_fills && (
                    <tr>
                      <td colSpan={6} className="p-0">
                        {renderLegBreakdown(trade.leg_fills)}
                      </td>
                    </tr>
                  )}
                </Fragment>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </Card>
  )
}
