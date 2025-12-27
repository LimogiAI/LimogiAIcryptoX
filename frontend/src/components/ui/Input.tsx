import { InputHTMLAttributes, forwardRef } from 'react'

interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  label?: string
  error?: string
  prefix?: string
  suffix?: string
}

export const Input = forwardRef<HTMLInputElement, InputProps>(
  ({ label, error, prefix, suffix, className = '', ...props }, ref) => {
    return (
      <div className="w-full">
        {label && (
          <label className="block text-sm text-text-secondary mb-1.5">
            {label}
          </label>
        )}
        <div className="relative flex items-center">
          {prefix && (
            <span className="absolute left-3 text-text-muted pointer-events-none">
              {prefix}
            </span>
          )}
          <input
            ref={ref}
            className={`
              w-full bg-bg-tertiary border border-border rounded-lg
              px-4 py-2.5 text-text-primary placeholder:text-text-muted
              focus:outline-none focus:ring-2 focus:ring-accent-primary/50 focus:border-accent-primary
              transition-all duration-150
              ${prefix ? 'pl-8' : ''}
              ${suffix ? 'pr-8' : ''}
              ${error ? 'border-accent-danger focus:ring-accent-danger/50' : ''}
              ${className}
            `}
            {...props}
          />
          {suffix && (
            <span className="absolute right-3 text-text-muted pointer-events-none">
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
