import type { ReactNode } from 'react';
import { cn } from 'lib/cn';

interface EmptyStateProps {
  message: ReactNode;
  /** Optional single primary action (button / link) under the message. */
  action?: ReactNode;
  className?: string;
  /** Compact for nested panels; default is page/section empty. */
  compact?: boolean;
}

/**
 * Shared empty surface — dashed panel + message + one optional CTA.
 * Use instead of one-off dashed boxes so empties read the same everywhere.
 */
export function EmptyState({
  message,
  action,
  className,
  compact = false,
}: EmptyStateProps) {
  return (
    <div
      className={cn(
        'flex flex-col items-center justify-center gap-3 rounded-lg border border-dashed border-white/10 bg-white/[0.02] text-center',
        compact ? 'px-3 py-4' : 'px-4 py-8',
        className,
      )}
    >
      <p className={cn('min-w-0 text-text-dim', compact ? 'text-xs' : 'text-sm')}>{message}</p>
      {action}
    </div>
  );
}
