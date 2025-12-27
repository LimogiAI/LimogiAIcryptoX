import { Badge, Button } from '../ui'
import { useTheme } from '../../hooks'

interface Props {
  isConfigured: boolean
  onSettingsClick?: () => void
}

export function Header({ isConfigured, onSettingsClick }: Props) {
  const { theme, toggleTheme } = useTheme()

  return (
    <header className="sticky top-0 z-50 bg-bg-secondary/80 backdrop-blur-lg border-b border-border">
      <div className="max-w-6xl mx-auto px-4 h-16 flex items-center justify-between">
        <div className="flex items-center gap-3">
          <h1 className="text-xl font-bold text-gradient">LimogiAI</h1>
          <Badge variant={isConfigured ? 'success' : 'warning'} dot>
            {isConfigured ? 'Ready' : 'Setup Required'}
          </Badge>
        </div>

        <div className="flex items-center gap-2">
          <Button
            variant="ghost"
            size="sm"
            onClick={toggleTheme}
            aria-label={`Switch to ${theme === 'dark' ? 'light' : 'dark'} mode`}
          >
            {theme === 'dark' ? (
              <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 3v1m0 16v1m9-9h-1M4 12H3m15.364 6.364l-.707-.707M6.343 6.343l-.707-.707m12.728 0l-.707.707M6.343 17.657l-.707.707M16 12a4 4 0 11-8 0 4 4 0 018 0z" />
              </svg>
            ) : (
              <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M20.354 15.354A9 9 0 018.646 3.646 9.003 9.003 0 0012 21a9.003 9.003 0 008.354-5.646z" />
              </svg>
            )}
          </Button>
          {isConfigured && onSettingsClick && (
            <Button variant="ghost" size="sm" onClick={onSettingsClick}>
              Settings
            </Button>
          )}
        </div>
      </div>
    </header>
  )
}
