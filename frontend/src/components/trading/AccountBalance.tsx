import { useState, useEffect, useCallback } from 'react'
import { Card, Badge } from '../ui'
import { api } from '../../services/api'
import type { PositionsResponse, Position } from '../../types'

export function AccountBalance() {
  const [data, setData] = useState<PositionsResponse | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [showAllPositions, setShowAllPositions] = useState(false)

  const fetchPositions = useCallback(async () => {
    try {
      const response = await api.getPositions()
      setData(response)
      setError(null)
    } catch (err) {
      setError('Failed to fetch account balances')
      console.error('Failed to fetch positions:', err)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    fetchPositions()
    // Refresh every 30 seconds (not too frequent to avoid rate limits)
    const interval = setInterval(fetchPositions, 30000)
    return () => clearInterval(interval)
  }, [fetchPositions])

  if (loading) {
    return (
      <Card className="animate-pulse">
        <div className="h-24 bg-bg-tertiary rounded" />
      </Card>
    )
  }

  if (error || !data?.connected) {
    return (
      <Card>
        <div className="flex items-center justify-between">
          <div>
            <h3 className="text-lg font-semibold">Kraken Account</h3>
            <Badge variant="danger" dot>Disconnected</Badge>
          </div>
          <button
            onClick={fetchPositions}
            className="text-sm text-accent-primary hover:underline"
          >
            Retry
          </button>
        </div>
        {error && <p className="text-sm text-text-muted mt-2">{error}</p>}
      </Card>
    )
  }

  const { balances, positions, fetched_at } = data

  // Filter significant positions (balance > 0.01)
  const significantPositions = positions.filter(p => p.balance > 0.01)

  return (
    <Card>
      <div className="flex items-center justify-between mb-4">
        <div>
          <h3 className="text-lg font-semibold">Kraken Account</h3>
          <div className="flex items-center gap-2 mt-1">
            <Badge variant="success" dot>Connected</Badge>
            <span className="text-xs text-text-muted">
              Last updated: {fetched_at}
            </span>
          </div>
        </div>
        <button
          onClick={fetchPositions}
          className="text-sm text-accent-primary hover:underline"
        >
          Refresh
        </button>
      </div>

      {/* Main Balances */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-4 p-4 bg-bg-tertiary rounded-lg">
        <div>
          <p className="text-xs text-text-muted uppercase">USD Balance</p>
          <p className="text-xl font-bold text-accent-success">
            ${balances.usd.toFixed(2)}
          </p>
        </div>
        <div>
          <p className="text-xs text-text-muted uppercase">EUR Balance</p>
          <p className="text-xl font-bold">
            {balances.eur.toFixed(2)}
          </p>
          <p className="text-xs text-text-muted">
            (${balances.eur_in_usd.toFixed(2)} USD)
          </p>
        </div>
        <div>
          <p className="text-xs text-text-muted uppercase">EUR/USD Rate</p>
          <p className="text-lg font-semibold">
            {balances.eur_usd_rate.toFixed(4)}
          </p>
        </div>
        <div>
          <p className="text-xs text-text-muted uppercase">Total Portfolio</p>
          <p className="text-xl font-bold text-accent-primary">
            ${balances.total_usd.toFixed(2)}
          </p>
        </div>
      </div>

      {/* Other Positions */}
      {significantPositions.length > 0 && (
        <div className="mt-4">
          <button
            onClick={() => setShowAllPositions(!showAllPositions)}
            className="text-sm text-text-secondary hover:text-text-primary flex items-center gap-1"
          >
            {showAllPositions ? '▼' : '▶'} Other positions ({significantPositions.length})
          </button>

          {showAllPositions && (
            <div className="mt-2 grid grid-cols-3 md:grid-cols-6 gap-2">
              {significantPositions
                .filter(p => !['USD', 'ZUSD', 'EUR', 'ZEUR'].includes(p.currency))
                .map((pos: Position) => (
                  <div key={pos.currency} className="p-2 bg-bg-secondary rounded text-sm">
                    <p className="font-medium">{pos.currency}</p>
                    <p className="text-text-muted">{pos.balance.toFixed(6)}</p>
                  </div>
                ))}
            </div>
          )}
        </div>
      )}
    </Card>
  )
}
