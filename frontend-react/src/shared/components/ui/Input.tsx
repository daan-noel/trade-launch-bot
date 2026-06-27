import {
  forwardRef,
  useCallback,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
  useState,
  type InputHTMLAttributes,
  type TextareaHTMLAttributes,
} from 'react';
import { cn } from 'lib/cn';

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
    type === 'number' &&
      '[appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none',
    type === 'datetime-local' && 'scheme-dark',
    className,
  );
}

export type FieldProps = { fieldSize?: FieldSize; variant?: FieldVariant };

// Safe stand-in handed back if a ref is read before the element mounts, so
// imperative calls (`focus()`, `select()`) are no-ops instead of throwing on null.
const NOOP_INPUT = { focus() {}, blur() {}, select() {}, value: '' };

export const Input = forwardRef<
  HTMLInputElement,
  InputHTMLAttributes<HTMLInputElement> & FieldProps & { unit?: string; blankZero?: boolean }
>(function Input(
  { className, type = 'text', fieldSize = 'sm', variant = 'default', unit, blankZero, value, ...props },
  ref,
) {
  const innerRef = useRef<HTMLInputElement | null>(null);
  // Guard against exposing a null handle before mount: resolve to the live node
  // if present, else a safe no-op stand-in so `.focus()` etc. never throw.
  useImperativeHandle(
    ref,
    () => innerRef.current ?? (NOOP_INPUT as unknown as HTMLInputElement),
    [],
  );

  // `blankZero`: render a literal 0 as an empty field (for "0 = off" params, so
  // an unset gate reads blank). Display-only — the bound value is untouched, so
  // an unedited 0 still saves as 0.
  if (blankZero && (value === 0 || value === '0')) value = '';

  const fieldCls = fieldClassName({ size: fieldSize, variant, type, className });

  // Measures where the typed value ends, so the unit suffix sits right after it
  // ("5 ◎"). The mirror shares the input's typography + left padding; we zero its
  // right padding/width so its measured width is paddingLeft + textWidth.
  const mirrorRef = useRef<HTMLSpanElement | null>(null);
  const [offset, setOffset] = useState(0);
  useLayoutEffect(() => {
    if (unit && mirrorRef.current) setOffset(mirrorRef.current.offsetWidth);
  }, [unit, value, fieldCls]);

  if (!unit) {
    return <input ref={innerRef} type={type} value={value} className={fieldCls} {...props} />;
  }

  const hasValue = value != null && `${value}` !== '';

  return (
    <span className="relative inline-flex w-full items-center">
      <input ref={innerRef} type={type} value={value} className={cn(fieldCls, 'w-full')} {...props} />
      <span
        ref={mirrorRef}
        aria-hidden
        className={cn(fieldCls, 'invisible absolute left-0 top-0 w-auto whitespace-pre border-transparent pr-0')}
      >
        {hasValue ? value : ''}
      </span>
      {hasValue && (
        <span
          aria-hidden
          className="pointer-events-none absolute top-1/2 -translate-y-1/2 text-text-dim"
          style={{ left: offset + 4 }}
        >
          {unit}
        </span>
      )}
    </span>
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
  // Guard against exposing a null handle before mount (see Input above).
  useImperativeHandle(
    ref,
    () => innerRef.current ?? (NOOP_INPUT as unknown as HTMLTextAreaElement),
    [],
  );

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
