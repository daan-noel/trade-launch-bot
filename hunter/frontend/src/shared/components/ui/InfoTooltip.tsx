import { createPortal } from 'react-dom';
import { cn } from 'lib/cn';
import { useHoverPinPopover } from 'hooks/useHoverPinPopover';

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
  /**
   * Optional ASCII figure drawn above the body — for controls whose meaning is a
   * shape (two states side by side, a before/after) that a paragraph has to
   * describe serially. Rendered `whitespace-pre` in the mono face, so column
   * alignment survives; the body stays proportional `pre-line` prose.
   */
  figure?: string;
  /** Preferred placement relative to the icon (default `bottom`); auto-flips
   *  to the opposite side when there isn't enough room in the viewport. */
  side?: 'top' | 'bottom';
  className?: string;
}

const WIDTH = 320; // wide enough for detailed help paragraphs

/**
 * A small ⓘ icon that reveals a styled popover on hover or keyboard focus.
 * Hover stays open across the icon↔panel gap (close delay); click pins until
 * a second click, Escape, or an outside pointer. The panel is portaled with
 * `position: fixed` so it escapes ancestor overflow clipping.
 */
export function InfoTooltip({
  title,
  body,
  figure,
  side = 'bottom',
  className,
}: InfoTooltipProps) {
  const {
    open,
    pinned,
    coords,
    panelId,
    anchorRef,
    panelRef,
    triggerHandlers,
    panelHandlers,
  } = useHoverPinPopover<HTMLButtonElement>({ side, width: WIDTH });

  return (
    <span className={cn('relative inline-flex align-middle', className)}>
      <button
        ref={anchorRef}
        type="button"
        aria-label={title ? `Info: ${title}` : 'Info'}
        aria-expanded={open}
        aria-pressed={pinned}
        aria-controls={open ? panelId : undefined}
        className={cn(
          'inline-flex items-center justify-center rounded-full transition focus:outline-none',
          pinned
            ? 'text-accent ring-1 ring-accent/40'
            : 'text-text-dim/70 hover:text-text focus:text-text',
        )}
        {...triggerHandlers}
      >
        <InfoIcon />
      </button>
      {open &&
        coords &&
        createPortal(
          <div
            ref={panelRef}
            id={panelId}
            role={pinned ? 'dialog' : 'tooltip'}
            aria-label={title ?? 'Info'}
            style={{ position: 'fixed', left: coords.left, top: coords.top, width: WIDTH }}
            className="z-300 max-h-[min(70vh,28rem)] overflow-y-auto rounded-md border border-border bg-bg-card p-2.5 text-left shadow-lg"
            {...panelHandlers}
          >
            {title && (
              <span className="mb-1 block text-[11px] font-bold normal-case leading-snug tracking-normal text-text">
                {title}
              </span>
            )}
            {figure && (
              // A figure that wraps is worse than one you scroll — the alignment
              // IS the content, so it gets its own scroller instead of reflowing.
              <span className="mb-1.5 block overflow-x-auto rounded-sm border border-white/6 bg-black/25 p-1.5 whitespace-pre font-mono text-[10px] font-normal normal-case leading-tight tracking-normal text-text-mid">
                {figure}
              </span>
            )}
            <span className="block whitespace-pre-line text-[11px] font-normal normal-case leading-snug tracking-normal text-text-dim">
              {body}
            </span>
          </div>,
          document.body,
        )}
    </span>
  );
}
