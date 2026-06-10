import { useRef, type ReactNode } from 'react';
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
  md: 'max-w-[600px]',
  xl: 'max-w-[1200px]',
};

export function Modal({ title, open, onClose, children, size = 'md' }: ModalProps) {
  const pressedOnBackdrop = useRef(false);

  if (!open) return null;

  return (
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
        className={cn(
          'flex h-fit w-full flex-col overflow-hidden rounded-xl border border-white/8 bg-bg-panel shadow-[0_24px_80px_rgba(0,0,0,0.7)]',
          MODAL_WIDTH[size],
        )}
      >
        <div className="flex items-center justify-between border-b border-white/6 px-5 py-3.5">
          <h2 className="text-[15px] font-bold text-text">{title}</h2>
          <button
            type="button"
            onClick={onClose}
            className="rounded px-2 py-1 text-2xl leading-none text-text-dim hover:bg-white/6 hover:text-text"
          >
            ×
          </button>
        </div>
        <div className="overflow-y-auto overflow-x-hidden p-5">{children}</div>
      </div>
    </div>
  );
}

interface AlertProps {
  variant: 'error' | 'success';
  children: ReactNode;
}

export function InlineAlert({ variant, children }: AlertProps) {
  return (
    <div
      className={cn(
        'my-2.5 rounded-md border px-3.5 py-2.5 text-xs',
        variant === 'error'
          ? 'border-red/25 bg-red/8 text-red'
          : 'border-[rgba(39,174,96,0.25)] bg-[rgba(39,174,96,0.08)] text-[#27ae60]',
      )}
    >
      {children}
    </div>
  );
}
