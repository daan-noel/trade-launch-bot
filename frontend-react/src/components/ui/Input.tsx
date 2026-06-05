import {
  forwardRef,
  useCallback,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
  type InputHTMLAttributes,
  type TextareaHTMLAttributes,
} from 'react';
import { cn } from '../../lib/cn';

export type FieldSize = 'sm' | 'md' | 'lg' | 'table' | 'page';
export type FieldVariant = 'default' | 'card';

const sizeClasses: Record<FieldSize, string> = {
  sm: 'rounded-md px-2 py-1 text-[11px]',
  md: 'rounded-md px-2.5 py-2 text-[13px]',
  lg: 'rounded-lg px-3 py-1.5 text-[13px]',
  table: 'rounded px-1.5 py-0.5 text-[11px]',
  page:
    'h-6 w-11 shrink-0 rounded px-1 text-center text-[13px] font-medium tabular-nums [appearance:textfield] hover:border-white/20 focus:ring-1 focus:ring-primary/25 [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none',
};

const variantClasses: Record<FieldVariant, string> = {
  default:
    'border-white/10 bg-white/4 focus:border-primary/50 focus:bg-white/6',
  card: 'border-white/10 bg-bg-card focus:border-primary/50',
};

export function fieldClassName({
  size = 'sm',
  variant = 'default',
  type,
  className,
}: {
  size?: FieldSize;
  variant?: FieldVariant;
  type?: string;
  className?: string;
}) {
  return cn(
    size !== 'lg' && size !== 'page' && 'w-full',
    'border text-text outline-none transition placeholder:text-text-dim/45 disabled:cursor-not-allowed disabled:opacity-50',
    sizeClasses[size],
    size === 'page' ? 'border-border bg-bg-card focus:border-primary/40' : variantClasses[variant],
    (type === 'number' || type === 'datetime-local' || type === 'date') && 'font-mono',
    type === 'datetime-local' && 'scheme-dark',
    className,
  );
}

export type FieldProps = { fieldSize?: FieldSize; variant?: FieldVariant };

export const Input = forwardRef<
  HTMLInputElement,
  InputHTMLAttributes<HTMLInputElement> & FieldProps
>(function Input({ className, type = 'text', fieldSize = 'sm', variant = 'default', ...props }, ref) {
  return (
    <input
      ref={ref}
      type={type}
      className={fieldClassName({ size: fieldSize, variant, type, className })}
      {...props}
    />
  );
});

export const Textarea = forwardRef<
  HTMLTextAreaElement,
  TextareaHTMLAttributes<HTMLTextAreaElement> & FieldProps & { autoResize?: boolean }
>(function Textarea(
  { className, rows = 2, fieldSize = 'sm', variant = 'default', autoResize = false, value, ...props },
  ref,
) {
  const innerRef = useRef<HTMLTextAreaElement | null>(null);
  useImperativeHandle(ref, () => innerRef.current as HTMLTextAreaElement, []);

  const resize = useCallback(() => {
    const el = innerRef.current;
    if (!el || !autoResize) return;
    el.style.height = 'auto';
    el.style.height = `${el.scrollHeight}px`;
  }, [autoResize]);

  useLayoutEffect(() => {
    resize();
  }, [resize, value]);

  return (
    <textarea
      ref={innerRef}
      rows={rows}
      value={value}
      className={cn(
        fieldClassName({ size: fieldSize, variant, className }),
        'leading-snug',
        autoResize ? 'resize-none overflow-hidden' : 'resize-y',
      )}
      {...props}
    />
  );
});
