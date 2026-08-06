import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type FocusEvent as ReactFocusEvent,
  type MouseEvent as ReactMouseEvent,
  type RefObject,
} from 'react';

export const HOVER_PIN_CLOSE_DELAY_MS = 150;

const GAP = 6;
const MARGIN = 8;

export interface HoverPinCoords {
  left: number;
  top: number;
}

export interface UseHoverPinPopoverOptions {
  /** Preferred placement; flips when the preferred edge lacks room. */
  side?: 'top' | 'bottom';
  /** Fixed panel width in px. */
  width: number;
  /** Delay before hover-dismiss so the pointer can cross the gap (ms). */
  closeDelayMs?: number;
}

export interface HoverPinTriggerHandlers {
  onMouseEnter: () => void;
  onMouseLeave: () => void;
  onFocus: () => void;
  onBlur: (e: ReactFocusEvent) => void;
  onClick: (e: ReactMouseEvent) => void;
}

export interface HoverPinPanelHandlers {
  onMouseEnter: () => void;
  onMouseLeave: () => void;
}

export interface UseHoverPinPopoverResult<T extends HTMLElement> {
  open: boolean;
  pinned: boolean;
  coords: HoverPinCoords | null;
  panelId: string;
  anchorRef: RefObject<T | null>;
  panelRef: RefObject<HTMLDivElement | null>;
  triggerHandlers: HoverPinTriggerHandlers;
  panelHandlers: HoverPinPanelHandlers;
}

/** True when the click is aimed at a nested control, not the anchor chrome. */
function isNestedInteractive(target: EventTarget | null, root: HTMLElement | null): boolean {
  if (!(target instanceof Element) || !root) return false;
  const el = target.closest('a,button,input,select,textarea,label,[role="button"]');
  return el != null && el !== root && root.contains(el);
}

/**
 * Shared open/close for portal help tips: hover (with close delay so the pointer
 * can reach the panel), click-to-pin, Escape / outside-pointer dismiss.
 */
export function useHoverPinPopover<T extends HTMLElement = HTMLElement>({
  side = 'bottom',
  width,
  closeDelayMs = HOVER_PIN_CLOSE_DELAY_MS,
}: UseHoverPinPopoverOptions): UseHoverPinPopoverResult<T> {
  const anchorRef = useRef<T | null>(null);
  const panelRef = useRef<HTMLDivElement | null>(null);
  const closeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pinnedRef = useRef(false);

  const [open, setOpen] = useState(false);
  const [pinned, setPinned] = useState(false);
  const [coords, setCoords] = useState<HoverPinCoords | null>(null);
  const panelId = useId();

  pinnedRef.current = pinned;

  const clearCloseTimer = useCallback(() => {
    if (closeTimerRef.current != null) {
      clearTimeout(closeTimerRef.current);
      closeTimerRef.current = null;
    }
  }, []);

  const reposition = useCallback(() => {
    const anchor = anchorRef.current;
    if (!anchor) return;
    const rect = anchor.getBoundingClientRect();
    const { innerWidth, innerHeight } = window;
    const height = panelRef.current?.offsetHeight ?? 0;

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

  const openNow = useCallback(() => {
    clearCloseTimer();
    setOpen(true);
    // Preliminary position (panel height may still be 0); layout effect remeasures.
    reposition();
  }, [clearCloseTimer, reposition]);

  const close = useCallback(() => {
    clearCloseTimer();
    setPinned(false);
    setOpen(false);
    setCoords(null);
  }, [clearCloseTimer]);

  const scheduleClose = useCallback(() => {
    if (pinnedRef.current) return;
    clearCloseTimer();
    closeTimerRef.current = setTimeout(() => {
      closeTimerRef.current = null;
      if (pinnedRef.current) return;
      setOpen(false);
      setCoords(null);
    }, closeDelayMs);
  }, [clearCloseTimer, closeDelayMs]);

  const togglePin = useCallback(
    (e: ReactMouseEvent) => {
      if (isNestedInteractive(e.target, anchorRef.current)) return;
      e.preventDefault();
      e.stopPropagation();
      if (pinnedRef.current) {
        close();
        return;
      }
      clearCloseTimer();
      setPinned(true);
      setOpen(true);
      reposition();
    },
    [clearCloseTimer, close, reposition],
  );

  const onBlur = useCallback(
    (e: ReactFocusEvent) => {
      if (pinnedRef.current) return;
      const next = e.relatedTarget;
      if (next instanceof Node) {
        if (anchorRef.current?.contains(next) || panelRef.current?.contains(next)) return;
      }
      scheduleClose();
    },
    [scheduleClose],
  );

  useLayoutEffect(() => {
    if (!open) return;
    reposition();
    window.addEventListener('scroll', reposition, true);
    window.addEventListener('resize', reposition);
    return () => {
      window.removeEventListener('scroll', reposition, true);
      window.removeEventListener('resize', reposition);
    };
  }, [open, reposition]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (e: PointerEvent) => {
      const t = e.target;
      if (!(t instanceof Node)) return;
      if (anchorRef.current?.contains(t) || panelRef.current?.contains(t)) return;
      close();
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        close();
      }
    };
    document.addEventListener('pointerdown', onPointerDown, true);
    document.addEventListener('keydown', onKeyDown, true);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown, true);
      document.removeEventListener('keydown', onKeyDown, true);
    };
  }, [open, close]);

  useEffect(() => () => clearCloseTimer(), [clearCloseTimer]);

  return {
    open,
    pinned,
    coords,
    panelId,
    anchorRef,
    panelRef,
    triggerHandlers: {
      onMouseEnter: openNow,
      onMouseLeave: scheduleClose,
      onFocus: openNow,
      onBlur,
      onClick: togglePin,
    },
    panelHandlers: {
      onMouseEnter: openNow,
      onMouseLeave: scheduleClose,
    },
  };
}
