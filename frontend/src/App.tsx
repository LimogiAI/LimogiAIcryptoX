import { useState, useEffect } from 'react'
import { api } from './services/api'
import { SetupDashboard } from './components/config'
import { TradingDashboard } from './components/trading'
import { Header } from './components/layout'
import type { ConfigurationStatus } from './types'

function LoadingScreen() {
  return (
    <div className="min-h-screen bg-bg-primary flex items-center justify-center">
      <div className="text-center">
        <div className="inline-block animate-spin rounded-full h-8 w-8 border-2 border-accent-primary border-t-transparent mb-4" />
        <p className="text-text-secondary">Loading...</p>
      </div>
    </div>
  )
}

function ErrorScreen({ error, onRetry }: { error: string; onRetry: () => void }) {
  return (
    <div className="min-h-screen bg-bg-primary flex items-center justify-center">
      <div className="text-center max-w-md mx-auto px-4">
        <div className="text-4xl mb-4">!</div>
        <h2 className="text-xl font-semibold mb-2">Connection Error</h2>
        <p className="text-text-secondary mb-6">{error}</p>
        <button
          onClick={onRetry}
          className="px-4 py-2 bg-accent-primary hover:bg-accent-primary/90 rounded-lg transition-colors"
        >
          Retry
        </button>
      </div>
    </div>
  )
}

export default function App() {
  const [configStatus, setConfigStatus] = useState<ConfigurationStatus | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [showSettings, setShowSettings] = useState(false)

  const checkConfig = async () => {
    setLoading(true)
    setError(null)
    try {
      const status = await api.getConfigurationStatus()
      setConfigStatus(status)
    } catch (err) {
      setError('Failed to connect to the trading server. Please ensure the backend is running.')
      console.error('Failed to check configuration:', err)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    checkConfig()
  }, [])

  const handleConfigured = () => {
    setShowSettings(false)
    checkConfig()
  }

  if (loading) {
    return <LoadingScreen />
  }

  if (error) {
    return <ErrorScreen error={error} onRetry={checkConfig} />
  }

  const isConfigured = configStatus?.is_configured ?? false

  // Show settings overlay when requested
  if (showSettings && isConfigured) {
    return (
      <div className="min-h-screen bg-bg-primary">
        <Header isConfigured={isConfigured} />
        <div className="pt-4">
          <div className="max-w-2xl mx-auto px-4 mb-4">
            <button
              onClick={() => setShowSettings(false)}
              className="text-text-secondary hover:text-text-primary transition-colors"
            >
              &larr; Back to Dashboard
            </button>
          </div>
          <SetupDashboard
            configStatus={configStatus}
            onConfigured={handleConfigured}
          />
        </div>
      </div>
    )
  }

  return (
    <div className="min-h-screen bg-bg-primary">
      <Header
        isConfigured={isConfigured}
        onSettingsClick={isConfigured ? () => setShowSettings(true) : undefined}
      />
      <main className="pt-4">
        {isConfigured ? (
          <TradingDashboard />
        ) : (
          <SetupDashboard
            configStatus={configStatus}
            onConfigured={handleConfigured}
          />
        )}
      </main>
    </div>
  )
}
