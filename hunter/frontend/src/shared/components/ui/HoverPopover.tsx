import {
  useCallback,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { createPortal } from 'react-dom';
import { cn } from 'lib/cn';

const GAP = 6;
const MARGIN = 8;
const DEFAULT_WIDTH = 360;

interface Coords {
  left: number;
  top: number;
}

export interface HoverPopoverProps {
  /** Rich body — mounted only while open (cheap for dense tables). */
  content: ReactNode;
  children: ReactNode;
  /** Preferred placement; flips when the preferred edge lacks room. */
  side?: 'top' | 'bottom';
  /** Fixed popover width in px (default 360). */
  width?: number;
  className?: string;
}

/**
 * Portal tooltip for arbitrary React content. Escapes table `overflow` clipping.
 * Content is not in the DOM until hover/focus — one open popover at a time per
 * trigger, so N rows don't pay for N hidden detail trees.
 */
export function HoverPopover({
  content,
  children,
  side = 'bottom',
  width = DEFAULT_WIDTH,
  className,
}: HoverPopoverProps) {
  const anchorRef = useRef<HTMLSpanElement>(null);
  const tooltipRef = useRef<HTMLDivElement>(null);
  const [coords, setCoords] = useState<Coords | null>(null);
  const tooltipId = useId();

  const reposition = useCallback(() => {
    const anchor = anchorRef.current;
    if (!anchor) return;
    const rect = anchor.getBoundingClientRect();
    const { innerWidth, innerHeight } = window;
    const height = tooltipRef.current?.offsetHeight ?? 0;

    const center = rect.left + rect.width / 2;
    const left = Math.min(Math.max(center - width / 2, MARGIN), innerWidth - width - MARGIN);

    const below = rect.bottom + GAP;
    const above = rect.top - GAP - height;
    let top: number;
    if (side === 'top') {
      top = above >= MARGIN ? above : below;
    } else {
      top = below + height <= innerHeight - MARGIN ? below : Math.max(above, MARGIN);
    }

    setCoords({ left, top });
  }, [side, width]);

  const show = useCallback(() => reposition(), [reposition]);
  const hide = useCallback(() => setCoords(null), []);

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
    <span
      ref={anchorRef}
      className={cn('inline-flex max-w-full', className)}
      aria-describedby={coords ? tooltipId : undefined}
      onMouseEnter={show}
      onMouseLeave={hide}
      onFocus={show}
      onBlur={hide}
    >
      {children}
      {coords &&
        createPortal(
          <div
            ref={tooltipRef}
            id={tooltipId}
            role="tooltip"
            style={{ position: 'fixed', left: coords.left, top: coords.top, width }}
            className="pointer-events-none z-[300] max-h-[min(70vh,28rem)] overflow-y-auto rounded-md border border-border bg-bg-card p-2.5 text-left shadow-lg"
          >
            {content}
          </div>,
          document.body,
        )}
    </span>
  );
}
