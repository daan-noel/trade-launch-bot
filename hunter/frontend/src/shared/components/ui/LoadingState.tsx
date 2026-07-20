import { cn } from 'lib/cn';

export type LoadingStateVariant = 'page' | 'panel' | 'inline';

/**
 * Shared Suspense / lazy-chunk placeholder. Same visual language for route
 * panes (`page`), chart/inspect shells (`panel`), and compact embeds (`inline`).
 */
export function LoadingState({
  label = 'Loading…',
  variant = 'panel',
  className,
}: {
  label?: string;
  variant?: LoadingStateVariant;
  className?: string;
}) {
  const ring =
    variant === 'page' ? 'size-9' : variant === 'panel' ? 'size-7' : 'size-4';
  const pad =
    variant === 'page' ? 'min-h-[42vh] py-24' : variant === 'panel' ? 'py-16' : 'py-6';

  return (
    <div
      role="status"
      aria-live="polite"
      aria-busy="true"
      className={cn(
        'flex flex-col items-center justify-center gap-3',
        'animate-[loading-fade-in_0.35s_ease_both]',
        pad,
        className,
      )}
    >
      <div className={cn('relative', ring)} aria-hidden>
        <span className="absolute inset-0 rounded-full border border-white/10" />
        <span className="absolute inset-0 animate-spin rounded-full border-2 border-transparent border-t-primary border-r-primary/35" />
        <span className="absolute inset-[28%] animate-[loading-breathe_1.6s_ease-in-out_infinite] rounded-full bg-primary/20" />
      </div>

      <p
        className={cn(
          'font-medium tracking-[0.08em] text-text-dim',
          variant === 'page' ? 'text-sm' : 'text-xs',
        )}
      >
        {label}
      </p>

      {variant !== 'inline' && (
        <div className="h-px w-28 overflow-hidden rounded-full bg-white/8" aria-hidden>
          <div className="h-full w-1/2 animate-[loading-shimmer_1.25s_ease-in-out_infinite] rounded-full bg-primary/55" />
        </div>
      )}
    </div>
  );
}
