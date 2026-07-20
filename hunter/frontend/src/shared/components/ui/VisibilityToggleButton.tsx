import type { ButtonHTMLAttributes, ReactNode } from 'react';
import { cn } from 'lib/cn';

interface VisibilityToggleButtonProps
  extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'onClick' | 'children'> {
  visible: boolean;
  onToggle: () => void;
  /** Accessible name suffix, e.g. "results table" */
  label: string;
  /** When provided, renders as a pill with text. Without it, renders as an icon-only square button. */
  children?: ReactNode;
}

function EyeIcon({ open, className }: { open: boolean; className?: string }) {
  if (open) {
    return (
      <svg viewBox="0 0 20 20" fill="none" aria-hidden className={cn('size-4', className)}>
        <path
          d="M1.5 10s3-6 8.5-6 8.5 6 8.5 6-3 6-8.5 6S1.5 10 1.5 10Z"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinejoin="round"
        />
        <circle cx="10" cy="10" r="2.5" stroke="currentColor" strokeWidth="1.5" />
      </svg>
    );
  }
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden className={cn('size-4', className)}>
      <path
        d="M3.5 3.5 16.5 16.5M8.2 8.8A2.5 2.5 0 0 0 10 12.5a2.5 2.5 0 0 0 2.5-2.5M6.1 6.4C4.5 7.6 2.8 9.5 1.5 10s3 6 8.5 6c1.6 0 3.1-.4 4.4-1.1M13.9 13.6c1.6-1.2 3.3-3.1 4.6-4.6"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

export function VisibilityToggleButton({
  visible,
  onToggle,
  label,
  children,
  className,
  ...props
}: VisibilityToggleButtonProps) {
  const action = visible ? 'Hide' : 'Show';

  if (children !== undefined) {
    return (
      <button
        type="button"
        onClick={onToggle}
        title={`${action} ${label}`}
        aria-label={`${action} ${label}`}
        aria-pressed={visible}
        className={cn(
          'flex items-center gap-1 rounded border px-2 py-0.5 text-xs font-medium transition-colors',
          visible
            ? 'border-primary text-primary hover:bg-primary/10'
            : 'border-border text-text-dim hover:border-primary hover:text-primary',
          'disabled:cursor-not-allowed disabled:opacity-45 disabled:hover:bg-transparent',
          className,
        )}
        {...props}
      >
        <EyeIcon open={visible} className="size-3" />
        {children}
      </button>
    );
  }

  return (
    <button
      type="button"
      onClick={onToggle}
      title={`${action} ${label}`}
      aria-label={`${action} ${label}`}
      aria-pressed={visible}
      className={cn(
        'inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-md border border-white/8 bg-white/4 text-text-dim transition hover:text-text',
        visible && 'border-primary/35 bg-primary/12 text-primary',
        className,
      )}
      {...props}
    >
      <EyeIcon open={visible} />
    </button>
  );
}
