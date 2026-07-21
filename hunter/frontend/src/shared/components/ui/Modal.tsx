import { useEffect, useId, useRef, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { cn } from 'lib/cn';

interface ModalProps {
  title: string;
  open: boolean;
  onClose: () => void;
  children: ReactNode;
  /** Width preset: `md` (default, ~600px) or `xl` (~1200px, e.g. for a chart). */
  size?: 'md' | 'xl';
}

const MODAL_WIDTH: Record<NonNullable<ModalProps['size']>, string> = {
  md: 'max-w-[700px]',
  xl: 'max-w-[1200px]',
};

/** Selector for the focusable elements we keep `Tab` cycling between. */
const FOCUSABLE =
  'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';

/**
 * Ref-count of currently-open modals so the page body scroll stays locked while
 * ANY modal is open and only unlocks once the last one closes (handles stacking).
 * Compensating for the scrollbar's width avoids a layout shift when it vanishes.
 */
let openModalCount = 0;
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

export function Modal({ title, open, onClose, children, size = 'md' }: ModalProps) {
  const pressedOnBackdrop = useRef(false);
  const dialogRef = useRef<HTMLDivElement>(null);
  const titleId = useId();

  // Escape-to-close + Tab/Shift+Tab focus trap, mirroring the portal popovers.
  useEffect(() => {
    if (!open) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onClose();
        return;
      }
      if (e.key !== 'Tab') return;
      const dialog = dialogRef.current;
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
    };
    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, [open, onClose]);

  // Lock page-body scroll while open so only the modal (backdrop) scrolls; the
  // ref-count restores it correctly when modals stack or unmount mid-open.
  useEffect(() => {
    if (!open) return;
    lockBodyScroll();
    return unlockBodyScroll;
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

const alertVariants: Record<AlertProps['variant'], string> = {
  error: 'border-red/25 bg-red/8 text-red',
  success: 'border-[rgba(39,174,96,0.25)] bg-[rgba(39,174,96,0.08)] text-[#27ae60]',
  warning: 'border-[rgba(241,196,15,0.25)] bg-[rgba(241,196,15,0.08)] text-[#f1c40f]',
};

export function InlineAlert({ variant, children }: AlertProps) {
  return (
    <div className={cn('my-2.5 rounded-md border px-3.5 py-2.5 text-xs', alertVariants[variant])}>
      {children}
    </div>
  );
}
