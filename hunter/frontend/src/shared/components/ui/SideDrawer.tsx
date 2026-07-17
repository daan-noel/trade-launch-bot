import { useEffect, useId, useRef, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { cn } from 'lib/cn';
import { Button } from './Button';

interface SideDrawerProps {
  open: boolean;
  onClose: () => void;
  title: string;
  children: ReactNode;
  /** Width class; default ~min(560px, 100vw). */
  widthClass?: string;
}

/**
 * Right-side detail drawer. Escape / backdrop / Close dismiss. Used for token
 * inspect so the table stays in place (no below-the-fold scroll).
 */
export function SideDrawer({
  open,
  onClose,
  title,
  children,
  widthClass = 'w-[min(560px,100vw)]',
}: SideDrawerProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const titleId = useId();

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [open, onClose]);

  useEffect(() => {
    if (!open) return;
    const prev = document.activeElement as HTMLElement | null;
    panelRef.current?.focus();
    return () => prev?.focus();
  }, [open]);

  if (!open) return null;

  return createPortal(
    <div className="fixed inset-0 z-[180] flex justify-end">
      <button
        type="button"
        aria-label="Close drawer"
        className="absolute inset-0 bg-black/50 backdrop-blur-[1px]"
        onClick={onClose}
      />
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        className={cn(
          'relative flex h-full flex-col border-l border-white/8 bg-bg-panel shadow-2xl outline-none',
          widthClass,
        )}
      >
        <div className="flex shrink-0 items-center justify-between gap-3 border-b border-white/6 px-3 py-2.5">
          <h2 id={titleId} className="truncate text-sm font-bold text-text">
            {title}
          </h2>
          <Button variant="subtle" size="sm" onClick={onClose} aria-label="Close">
            Close
          </Button>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto p-3">{children}</div>
      </div>
    </div>,
    document.body,
  );
}
