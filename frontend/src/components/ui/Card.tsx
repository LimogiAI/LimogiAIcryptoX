import { ReactNode } from 'react'

type CardStatus = 'default' | 'required' | 'complete' | 'warning' | 'error' | 'info'

interface CardProps {
  title?: string
  description?: string
  status?: CardStatus
  children: ReactNode
  className?: string
  padding?: 'sm' | 'md' | 'lg'
}

const statusStyles: Record<CardStatus, string> = {
  default: '',
  required: 'border-l-4 border-l-accent-warning',
  complete: 'border-l-4 border-l-accent-success',
  warning: 'border-l-4 border-l-accent-warning',
  error: 'border-l-4 border-l-accent-danger',
  info: 'border-l-4 border-l-accent-primary',
}

const paddingStyles = {
  sm: 'p-4',
  md: 'p-6',
  lg: 'p-8',
}

export function Card({
  title,
  description,
  status = 'default',
  children,
  className = '',
  padding = 'md',
}: CardProps) {
  return (
    <div
      className={`
        bg-bg-secondary rounded-lg border border-border
        ${statusStyles[status]}
        ${paddingStyles[padding]}
        ${className}
      `}
    >
      {(title || status !== 'default') && (
        <div className="mb-4">
          <h3 className="text-lg font-semibold flex items-center gap-2">
            {title}
            {status === 'required' && (
              <span className="text-xs bg-accent-warning/20 text-accent-warning px-2 py-0.5 rounded">
                Required
              </span>
            )}
            {status === 'complete' && (
              <span className="text-accent-success text-lg">&#10003;</span>
            )}
          </h3>
          {description && (
            <p className="text-sm text-text-secondary mt-1">{description}</p>
          )}
        </div>
      )}
      {children}
    </div>
  )
}
