import type { ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { cn } from 'lib/cn';
import { useHoverPinPopover } from 'hooks/useHoverPinPopover';

const DEFAULT_WIDTH = 360;

export interface HoverPopoverProps {
  /** Rich body — mounted only while open (cheap for dense tables). */
  content: ReactNode;
  children: ReactNode;
  /** Preferred placement; flips when the preferred edge lacks room. */
  side?: 'top' | 'bottom';
  /** Fixed popover width in px (default 360). */
  width?: number;
  /** Pointer must rest this long before hover opens the panel. `0` is immediate. */
  openDelayMs?: number;
  /**
   * Cover the whole cell. The parent `td` must be `relative`. Children stay in
   * flow so the row keeps its height; the hit target is a separate layer.
   */
  fillCell?: boolean;
  className?: string;
}

/**
 * Portal tooltip for arbitrary React content. Escapes table `overflow` clipping.
 * Content is not in the DOM until hover/focus/pin — one open popover at a time
 * per trigger, so N rows don't pay for N hidden detail trees. Hover bridges the
 * gap into the panel; click pins (nested links/buttons are left alone).
 * `openDelayMs` waits that long before hover opens; until then a click is not
 * swallowed.
 */
export function HoverPopover({
  content,
  children,
  side = 'bottom',
  width = DEFAULT_WIDTH,
  openDelayMs = 0,
  fillCell = false,
  className,
}: HoverPopoverProps) {
  const {
    open,
    pinned,
    coords,
    panelId,
    anchorRef,
    panelRef,
    triggerHandlers,
    panelHandlers,
  } = useHoverPinPopover<HTMLSpanElement>({ side, width, openDelayMs });

  const panel =
    open &&
    coords &&
    createPortal(
      <div
        ref={panelRef}
        id={panelId}
        role={pinned ? 'dialog' : 'tooltip'}
        style={{ position: 'fixed', left: coords.left, top: coords.top, width }}
        className="z-300 max-h-[min(70vh,28rem)] overflow-y-auto rounded-md border border-border bg-bg-card p-2.5 text-left shadow-lg"
        {...panelHandlers}
      >
        {content}
      </div>,
      document.body,
    );

  if (fillCell) {
    return (
      <>
        {children}
        <span
          ref={anchorRef}
          className={cn('absolute inset-0', pinned && 'rounded-sm ring-1 ring-accent/30', className)}
          aria-expanded={open}
          aria-controls={open ? panelId : undefined}
          {...triggerHandlers}
        >
          {panel}
        </span>
      </>
    );
  }

  return (
    <span
      ref={anchorRef}
      className={cn(
        'inline-flex max-w-full',
        pinned && 'rounded-sm ring-1 ring-accent/30',
        className,
      )}
      aria-expanded={open}
      aria-controls={open ? panelId : undefined}
      {...triggerHandlers}
    >
      {children}
      {panel}
    </span>
  );
}
