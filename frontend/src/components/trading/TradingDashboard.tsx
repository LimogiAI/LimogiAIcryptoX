import { TradingControls } from './TradingControls'
import { LiveTrades } from './LiveTrades'
import { PartialTrades } from './PartialTrades'

export function TradingDashboard() {
  return (
    <div className="max-w-6xl mx-auto px-4 py-6">
      <div className="space-y-6">
        {/* Partial Trades Warning (shows only if there are partial trades) */}
        <PartialTrades />

        {/* Trading Controls */}
        <TradingControls />

        {/* Live Trades Table */}
        <LiveTrades />
      </div>
    </div>
  )
}
