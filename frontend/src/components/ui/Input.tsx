import { InputHTMLAttributes, forwardRef } from 'react'

interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  label?: string
  error?: string
  prefix?: string
  suffix?: string
}

export const Input = forwardRef<HTMLInputElement, InputProps>(
  ({ label, error, prefix, suffix, className = '', disabled, ...props }, ref) => {
    return (
      <div className="w-full">
        {label && (
          <label className={`block text-sm mb-1.5 ${disabled ? 'text-text-muted' : 'text-text-secondary'}`}>
            {label}
          </label>
        )}
        <div className="relative flex items-center">
          {prefix && (
            <span className={`absolute left-3 pointer-events-none ${disabled ? 'text-text-muted/50' : 'text-text-muted'}`}>
              {prefix}
            </span>
          )}
          <input
            ref={ref}
            disabled={disabled}
            className={`
              w-full bg-bg-tertiary border border-border rounded-lg
              px-4 py-2.5 text-text-primary placeholder:text-text-muted
              focus:outline-none focus:ring-2 focus:ring-accent-primary/50 focus:border-accent-primary
              transition-all duration-150
              disabled:opacity-50 disabled:cursor-not-allowed disabled:bg-bg-secondary
              ${prefix ? 'pl-8' : ''}
              ${suffix ? 'pr-8' : ''}
              ${error ? 'border-accent-danger focus:ring-accent-danger/50' : ''}
              ${className}
            `}
            {...props}
          />
          {suffix && (
            <span className={`absolute right-3 pointer-events-none ${disabled ? 'text-text-muted/50' : 'text-text-muted'}`}>
              {suffix}
            </span>
          )}
        </div>
        {error && (
          <p className="text-sm text-accent-danger mt-1.5">{error}</p>
        )}
      </div>
    )
  }
)

Input.displayName = 'Input'
