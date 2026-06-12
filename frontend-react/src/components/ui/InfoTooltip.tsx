import { useCallback, useId, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
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
  /** Preferred placement relative to the icon (default `bottom`); auto-flips
   *  to the opposite side when there isn't enough room in the viewport. */
  side?: 'top' | 'bottom';
  className?: string;
}

const WIDTH = 256; // matches `w-64`
const GAP = 6; // distance between icon and popover
const MARGIN = 8; // min distance from the viewport edge

interface Coords {
  left: number;
  top: number;
}

/**
 * A small ⓘ icon that reveals a styled popover on hover or keyboard focus.
 * The popover is rendered in a portal with `position: fixed`, so it escapes
 * any ancestor's `overflow` clipping/scrolling (e.g. a modal body) and can
 * never widen the page or get cut off. Position is computed from the icon's
 * bounding box: horizontally centered then clamped to the viewport, and
 * placed on `side` unless that edge lacks room (then it flips).
 */
export function InfoTooltip({ title, body, side = 'bottom', className }: InfoTooltipProps) {
  const anchorRef = useRef<HTMLButtonElement>(null);
  const tooltipRef = useRef<HTMLDivElement>(null);
  const [coords, setCoords] = useState<Coords | null>(null);
  const tooltipId = useId();

  const reposition = useCallback(() => {
    const anchor = anchorRef.current;
    if (!anchor) return;
    const rect = anchor.getBoundingClientRect();
    const { innerWidth, innerHeight } = window;
    const height = tooltipRef.current?.offsetHeight ?? 0;

    // Horizontally center on the icon, then clamp inside the viewport.
    const center = rect.left + rect.width / 2;
    const left = Math.min(Math.max(center - WIDTH / 2, MARGIN), innerWidth - WIDTH - MARGIN);

    // Place on the preferred side; flip if it would overflow that edge.
    const below = rect.bottom + GAP;
    const above = rect.top - GAP - height;
    let top: number;
    if (side === 'top') {
      top = above >= MARGIN ? above : below;
    } else {
      top = below + height <= innerHeight - MARGIN ? below : Math.max(above, MARGIN);
    }

    setCoords({ left, top });
  }, [side]);

  const show = useCallback(() => reposition(), [reposition]);
  const hide = useCallback(() => setCoords(null), []);

  // Re-measure once the popover has rendered (its height affects `top`), and
  // keep it pinned to the icon while it's open if the page scrolls/resizes.
  useLayoutEffect(() => {
    if (coords == null) return;
    reposition();
    window.addEventListener('scroll', reposition, true);
    window.addEventListener('resize', reposition);
    return () => {
      window.removeEventListener('scroll', reposition, true);
      window.removeEventListener('resize', reposition);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [coords != null, reposition]);

  return (
    <span className={cn('relative inline-flex align-middle', className)}>
      <button
        ref={anchorRef}
        type="button"
        aria-label={title ? `Info: ${title}` : 'Info'}
        aria-describedby={coords ? tooltipId : undefined}
        onMouseEnter={show}
        onMouseLeave={hide}
        onFocus={show}
        onBlur={hide}
        className="inline-flex items-center justify-center rounded-full text-text-dim/70 transition hover:text-text focus:text-text focus:outline-none"
      >
        <InfoIcon />
      </button>
      {coords &&
        createPortal(
          <div
            ref={tooltipRef}
            id={tooltipId}
            role="tooltip"
            style={{ position: 'fixed', left: coords.left, top: coords.top, width: WIDTH }}
            className="pointer-events-none z-[300] rounded-md border border-border bg-bg-card p-2.5 text-left shadow-lg"
          >
            {title && (
              <span className="mb-1 block text-[11px] font-bold normal-case leading-snug tracking-normal text-text">
                {title}
              </span>
            )}
            <span className="block text-[11px] font-normal normal-case leading-snug tracking-normal text-text-dim">
              {body}
            </span>
          </div>,
          document.body,
        )}
    </span>
  );
}
