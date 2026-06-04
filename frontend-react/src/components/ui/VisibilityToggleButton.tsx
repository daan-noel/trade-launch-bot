import type { ButtonHTMLAttributes } from 'react';
import { cn } from '../../lib/cn';

interface VisibilityToggleButtonProps
  extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'onClick' | 'children'> {
  visible: boolean;
  onToggle: () => void;
  /** Accessible name suffix, e.g. "results table" */
  label: string;
}

function EyeIcon({ open }: { open: boolean }) {
  if (open) {
    return (
      <svg viewBox="0 0 20 20" fill="none" aria-hidden className="size-4">
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
    <svg viewBox="0 0 20 20" fill="none" aria-hidden className="size-4">
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
  className,
  ...props
}: VisibilityToggleButtonProps) {
  const action = visible ? 'Hide' : 'Show';

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
