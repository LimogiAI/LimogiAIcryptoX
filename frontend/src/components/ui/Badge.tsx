import { ReactNode } from 'react'

type BadgeVariant = 'default' | 'success' | 'warning' | 'danger' | 'info'
type BadgeSize = 'sm' | 'md'

interface BadgeProps {
  variant?: BadgeVariant
  size?: BadgeSize
  children: ReactNode
  className?: string
  dot?: boolean
}

const variantStyles: Record<BadgeVariant, string> = {
  default: 'bg-bg-tertiary text-text-secondary',
  success: 'bg-accent-success/20 text-accent-success',
  warning: 'bg-accent-warning/20 text-accent-warning',
  danger: 'bg-accent-danger/20 text-accent-danger',
  info: 'bg-accent-primary/20 text-accent-primary',
}

const dotStyles: Record<BadgeVariant, string> = {
  default: 'bg-text-muted',
  success: 'bg-accent-success',
  warning: 'bg-accent-warning',
  danger: 'bg-accent-danger',
  info: 'bg-accent-primary',
}

const sizeStyles: Record<BadgeSize, string> = {
  sm: 'px-2 py-0.5 text-xs',
  md: 'px-2.5 py-1 text-sm',
}

export function Badge({
  variant = 'default',
  size = 'sm',
  children,
  className = '',
  dot = false,
}: BadgeProps) {
  return (
    <span
      className={`
        inline-flex items-center gap-1.5 rounded font-medium
        ${variantStyles[variant]}
        ${sizeStyles[size]}
        ${className}
      `}
    >
      {dot && (
        <span
          className={`w-1.5 h-1.5 rounded-full ${dotStyles[variant]}`}
        />
      )}
      {children}
    </span>
  )
}
