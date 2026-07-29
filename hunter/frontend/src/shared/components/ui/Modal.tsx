import { useEffect, useId, useRef, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { cn } from 'lib/cn';

interface ModalProps {
  title: string;
  open: boolean;
  onClose: () => void;
  children: ReactNode;
  /** Width preset: `md` (default, ~700px), `xl` (~1200px, e.g. for a chart), or
   *  `xxl` (~1600px, e.g. a token-detail modal with chart + metric panes). */
  size?: 'md' | 'xl' | 'xxl';
}

const MODAL_WIDTH: Record<NonNullable<ModalProps['size']>, string> = {
  md: 'max-w-[700px]',
  xl: 'max-w-[1200px]',
  xxl: 'max-w-[1600px]',
};

/** Selector for the focusable elements we keep `Tab` cycling between. */
const FOCUSABLE =
  'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';

/**
 * Ref-count of currently-open modals so the page body scroll stays locked while
 * ANY modal is open and only unlocks once the last one closes (handles stacking).
 * Compensating for the scrollbar's width avoids a layout shift when it vanishes.
 *
 * Escape + Tab focus-trap apply only to the topmost modal: each open instance
 * pushes a handle onto `modalStack` so nested portals (e.g. dry-run inspect over
 * the rule editor) don't all dismiss on Escape or steal Tab from the top dialog.
 */
let openModalCount = 0;
type ModalStackHandle = {
  onClose: () => void;
  dialogRef: { current: HTMLDivElement | null };
};
const modalStack: ModalStackHandle[] = [];

function lockBodyScroll() {
  if (openModalCount === 0) {
    const scrollbarWidth = window.innerWidth - document.documentElement.clientWidth;
    document.body.style.overflow = 'hidden';
    if (scrollbarWidth > 0) document.body.style.paddingRight = `${scrollbarWidth}px`;
  }
  openModalCount += 1;
}
function unlockBodyScroll() {
  openModalCount = Math.max(0, openModalCount - 1);
  if (openModalCount === 0) {
    document.body.style.overflow = '';
    document.body.style.paddingRight = '';
  }
}

function pushModalHandle(handle: ModalStackHandle) {
  modalStack.push(handle);
}
function popModalHandle(handle: ModalStackHandle) {
  const idx = modalStack.lastIndexOf(handle);
  if (idx >= 0) modalStack.splice(idx, 1);
}

/** Document-level Escape + Tab listener — installed once while any modal is open. */
let keyListening = false;
function ensureKeyListener() {
  if (keyListening) return;
  keyListening = true;
  document.addEventListener('keydown', onDocumentModalKey);
}
function releaseKeyListener() {
  if (modalStack.length > 0) return;
  keyListening = false;
  document.removeEventListener('keydown', onDocumentModalKey);
}
function onDocumentModalKey(e: KeyboardEvent) {
  const top = modalStack[modalStack.length - 1];
  if (!top) return;
  if (e.key === 'Escape') {
    e.stopPropagation();
    top.onClose();
    return;
  }
  if (e.key !== 'Tab') return;
  const dialog = top.dialogRef.current;
  if (!dialog) return;
  const focusable = dialog.querySelectorAll<HTMLElement>(FOCUSABLE);
  if (focusable.length === 0) {
    e.preventDefault();
    return;
  }
  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  const active = document.activeElement;
  if (e.shiftKey) {
    if (active === first || !dialog.contains(active)) {
      e.preventDefault();
      last.focus();
    }
  } else if (active === last || !dialog.contains(active)) {
    e.preventDefault();
    first.focus();
  }
}

export function Modal({ title, open, onClose, children, size = 'md' }: ModalProps) {
  const pressedOnBackdrop = useRef(false);
  const dialogRef = useRef<HTMLDivElement>(null);
  const titleId = useId();
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  // Lock page-body scroll while open; register Escape/Tab on the stack. Stable
  // handle so an inline `onClose={() => ...}` identity change does not reshuffle
  // nested modals.
  useEffect(() => {
    if (!open) return;
    const handle: ModalStackHandle = {
      onClose: () => onCloseRef.current(),
      dialogRef,
    };
    lockBodyScroll();
    pushModalHandle(handle);
    ensureKeyListener();
    return () => {
      popModalHandle(handle);
      unlockBodyScroll();
      releaseKeyListener();
    };
  }, [open]);

  // Focus the first focusable element on open; restore focus on close/unmount.
  useEffect(() => {
    if (!open) return;
    const previouslyFocused = document.activeElement as HTMLElement | null;
    const dialog = dialogRef.current;
    const focusable = dialog?.querySelectorAll<HTMLElement>(FOCUSABLE);
    (focusable && focusable.length > 0 ? focusable[0] : dialog)?.focus();
    return () => previouslyFocused?.focus();
  }, [open]);

  if (!open) return null;

  return createPortal(
    <div
      className="fixed inset-0 z-[200] flex justify-center overflow-y-auto bg-black/65 p-5 backdrop-blur-sm"
      onMouseDown={(e) => {
        pressedOnBackdrop.current = e.target === e.currentTarget;
      }}
      onClick={(e) => {
        if (pressedOnBackdrop.current && e.target === e.currentTarget) onClose();
      }}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        className={cn(
          'flex h-fit w-full flex-col overflow-hidden rounded-xl border border-white/8 bg-bg-panel shadow-[0_24px_80px_rgba(0,0,0,0.7)] focus:outline-none',
          MODAL_WIDTH[size],
        )}
      >
        <div className="flex items-center justify-between border-b border-white/6 px-5 py-3.5">
          <h2 id={titleId} className="text-[15px] font-bold text-text">
            {title}
          </h2>
          <button
            type="button"
            onClick={onClose}
            className="rounded px-2 py-1 text-2xl leading-none text-text-dim hover:bg-white/6 hover:text-text"
          >
            ×
          </button>
        </div>
        <div className="scrollbar-gutter-stable overflow-y-auto overflow-x-hidden p-5">
          {children}
        </div>
      </div>
    </div>,
    document.body,
  );
}

interface AlertProps {
  variant: 'error' | 'success' | 'warning';
  children: ReactNode;
}

/** Theme-token tones only — never hard-code hex here (drifts from `index.css`). */
const alertVariants: Record<AlertProps['variant'], string> = {
  error: 'border-red/25 bg-red/8 text-red',
  success: 'border-green/25 bg-green/8 text-green',
  warning: 'border-warning/25 bg-warning/8 text-warning',
};

export function InlineAlert({ variant, children }: AlertProps) {
  return (
    <div className={cn('my-2.5 rounded-md border px-3.5 py-2.5 text-xs', alertVariants[variant])}>
      {children}
    </div>
  );
}
