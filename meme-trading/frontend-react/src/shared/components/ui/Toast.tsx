import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { cn } from 'lib/cn';

export type ToastVariant = 'neutral' | 'success' | 'info' | 'warning' | 'danger';

export interface ToastItem {
  id: number;
  title: string;
  body?: string;
  variant: ToastVariant;
}

interface ToastContextValue {
  addToast: (title: string, body?: string, variant?: ToastVariant) => void;
}

const ToastContext = createContext<ToastContextValue | null>(null);

let nextId = 0;
const DURATION_MS = 6000;

const variantClasses: Record<ToastVariant, string> = {
  neutral: 'border-white/10 bg-bg-panel text-text',
  success: 'border-green/30 bg-green/8 text-text',
  info: 'border-info/30 bg-info/8 text-text',
  warning: 'border-warning/30 bg-warning/8 text-text',
  danger: 'border-red/30 bg-red/8 text-text',
};

const accentClasses: Record<ToastVariant, string> = {
  neutral: 'bg-white/20',
  success: 'bg-green',
  info: 'bg-info',
  warning: 'bg-warning',
  danger: 'bg-red',
};

function Toast({ item, onDismiss }: { item: ToastItem; onDismiss: (id: number) => void }) {
  const [exiting, setExiting] = useState(false);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const dismiss = useCallback(() => {
    setExiting(true);
    timerRef.current = setTimeout(() => onDismiss(item.id), 300);
  }, [item.id, onDismiss]);

  useEffect(() => {
    timerRef.current = setTimeout(dismiss, DURATION_MS);
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current);
    };
  }, [dismiss]);

  return (
    <div
      className={cn(
        'relative w-80 overflow-hidden rounded-lg border px-3.5 py-3 shadow-lg transition-all duration-300',
        variantClasses[item.variant],
        exiting ? 'translate-x-full opacity-0' : 'translate-x-0 opacity-100',
      )}
    >
      {/* progress bar */}
      <div
        className={cn('absolute bottom-0 left-0 h-0.5', accentClasses[item.variant])}
        style={{ animation: `toast-shrink ${DURATION_MS}ms linear forwards` }}
      />
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <p className="text-xs font-semibold leading-snug">{item.title}</p>
          {item.body && (
            <p className="mt-0.5 truncate font-mono text-[11px] text-text-dim">{item.body}</p>
          )}
        </div>
        <button
          type="button"
          onClick={dismiss}
          className="shrink-0 text-[11px] text-text-dim transition hover:text-text"
          aria-label="Dismiss"
        >
          ✕
        </button>
      </div>
    </div>
  );
}

export function ToastContainer({ toasts, onDismiss }: {
  toasts: ToastItem[];
  onDismiss: (id: number) => void;
}) {
  return (
    <div className="pointer-events-none fixed bottom-4 right-4 z-50 flex flex-col items-end gap-2">
      {toasts.map((t) => (
        <div key={t.id} className="pointer-events-auto">
          <Toast item={t} onDismiss={onDismiss} />
        </div>
      ))}
    </div>
  );
}

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<ToastItem[]>([]);

  const addToast = useCallback((title: string, body?: string, variant: ToastVariant = 'neutral') => {
    const id = ++nextId;
    setToasts((prev) => [...prev.slice(-4), { id, title, body, variant }]);
  }, []);

  const dismiss = useCallback((id: number) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  return (
    <ToastContext.Provider value={{ addToast }}>
      {children}
      <ToastContainer toasts={toasts} onDismiss={dismiss} />
    </ToastContext.Provider>
  );
}

export function useToast(): ToastContextValue {
  const ctx = useContext(ToastContext);
  if (!ctx) throw new Error('useToast must be used inside ToastProvider');
  return ctx;
}
