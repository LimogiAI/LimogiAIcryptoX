import { Badge, Button } from '../ui'

interface Props {
  isConfigured: boolean
  onSettingsClick?: () => void
}

export function Header({ isConfigured, onSettingsClick }: Props) {
  return (
    <header className="sticky top-0 z-50 bg-bg-secondary/80 backdrop-blur-lg border-b border-border">
      <div className="max-w-6xl mx-auto px-4 h-16 flex items-center justify-between">
        <div className="flex items-center gap-3">
          <h1 className="text-xl font-bold text-gradient">LimogiAI</h1>
          <Badge variant={isConfigured ? 'success' : 'warning'} dot>
            {isConfigured ? 'Ready' : 'Setup Required'}
          </Badge>
        </div>

        {isConfigured && onSettingsClick && (
          <Button variant="ghost" size="sm" onClick={onSettingsClick}>
            Settings
          </Button>
        )}
      </div>
    </header>
  )
}
