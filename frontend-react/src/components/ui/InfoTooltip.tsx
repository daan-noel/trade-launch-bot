import { cn } from 'lib/cn';

function InfoIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden className={cn('size-3.5', className)}>
      <circle cx="10" cy="10" r="7.25" stroke="currentColor" strokeWidth="1.5" />
      <path d="M10 9v4.4" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      <circle cx="10" cy="6.4" r="0.95" fill="currentColor" />
    </svg>
  );
}

interface InfoTooltipProps {
  /** Bold heading line (optional). */
  title?: string;
  /** Explanation body. */
  body: string;
  /** Popover placement relative to the icon (default `bottom`). */
  side?: 'top' | 'bottom';
  className?: string;
}

/**
 * A small ⓘ icon that reveals a styled popover on hover or keyboard focus.
 * CSS-only (no dependency): the popover is an absolutely-positioned sibling
 * toggled via `group-hover`/`group-focus-within`. Place it inline next to a
 * label. Note: it can be clipped by an ancestor with `overflow: hidden`.
 */
export function InfoTooltip({ title, body, side = 'bottom', className }: InfoTooltipProps) {
  return (
    <span className={cn('group/info relative inline-flex align-middle', className)}>
      <button
        type="button"
        aria-label={title ? `Info: ${title}` : 'Info'}
        className="inline-flex items-center justify-center rounded-full text-text-dim/70 transition hover:text-text focus:text-text focus:outline-none"
      >
        <InfoIcon />
      </button>
      <span
        role="tooltip"
        className={cn(
          'pointer-events-none absolute left-0 z-50 w-64 rounded-md border border-border bg-bg-card p-2.5 text-left opacity-0 shadow-lg transition-opacity duration-100',
          'group-hover/info:opacity-100 group-focus-within/info:opacity-100',
          side === 'bottom' ? 'top-full mt-1.5' : 'bottom-full mb-1.5',
        )}
      >
        {title && (
          <span className="mb-1 block text-[11px] font-bold normal-case leading-snug tracking-normal text-text">
            {title}
          </span>
        )}
        <span className="block text-[11px] font-normal normal-case leading-snug tracking-normal text-text-dim">
          {body}
        </span>
      </span>
    </span>
  );
}
