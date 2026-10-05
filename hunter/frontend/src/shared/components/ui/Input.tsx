import {
  forwardRef,
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

/** Width and flex classes belong on a unit field's wrapper. The rest stay on the input. */
function splitLayoutClass(className?: string): { layout: string; rest?: string } {
  const layout: string[] = [];
  const rest: string[] = [];
  for (const part of (className ?? '').trim().split(/\s+/).filter(Boolean)) {
    if (/^(?:w-|min-w-|max-w-|flex-|shrink-0|shrink$|grow|basis-)/.test(part)) layout.push(part);
    else rest.push(part);
  }
  return { layout: layout.join(' '), rest: rest.join(' ') || undefined };
}

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

/** Opt-in numeric-field behavior (see the `numeric` prop on {@link Input}). */
export type NumericProps = {
  /**
   * Turn the field into a "typed number" input: it holds the raw in-progress
   * text internally while focused, so partial values (`0`, `0.`, `0.00`, `-`,
   * `1e`) survive keystroke-by-keystroke instead of being clobbered by a parse.
   * The parsed number is reported via `onNumericChange`; on blur the field snaps
   * back to the canonical `numericValue`. Use this instead of stringifying a
   * parsed number back into `value` (which fights the user's decimal point).
   */
  numeric?: true;
  /** The canonical numeric value (`null`/`undefined` ⇒ empty field). */
  numericValue?: number | null;
  /** Parsed value on each edit: a finite number, or `null` for empty/`off`. */
  onNumericChange?: (value: number | null) => void;
  /** Parse with `parseInt` instead of `parseFloat` (integer-only fields). */
  integer?: boolean;
};

// Safe stand-in handed back if a ref is read before the element mounts, so
// imperative calls (`focus()`, `select()`) are no-ops instead of throwing on null.
const NOOP_INPUT = { focus() {}, blur() {}, select() {}, value: '' };

/** Empty or an explicit "no bound" sentinel ⇒ the numeric value is `null`. */
const NUMERIC_NULL_RE = /^(off|null|none|-)$/i;

export const Input = forwardRef<
  HTMLInputElement,
  InputHTMLAttributes<HTMLInputElement> & FieldProps & NumericProps & { unit?: string; blankZero?: boolean }
>(function Input(
  {
    className,
    type = 'text',
    fieldSize = 'sm',
    variant = 'default',
    unit,
    blankZero,
    value,
    numeric,
    numericValue,
    onNumericChange,
    integer,
    onChange,
    onBlur,
    onFocus,
    ...props
  },
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

  // Numeric mode: hold the raw typed text while editing so partial numbers
  // aren't destroyed by re-parsing `numericValue` back into the field. `null`
  // draft ⇒ not editing, render the canonical value. Cleared on blur.
  const [draft, setDraft] = useState<string | null>(null);
  // Track focus so display-only reformatting (e.g. `blankZero`) never rewrites
  // the field while the user is mid-typing — that swallows a leading `0` and
  // turns `0.4` into `.4`.
  const [focused, setFocused] = useState(false);
  if (numeric) {
    // Preserve any caller-supplied handlers: numeric mode manages the draft and
    // reports the parsed value via `onNumericChange`, but still forwards the raw
    // change/blur event so a caller keeping its own string-based `onChange`
    // contract (e.g. RangeInputs) keeps working.
    const callerChange = onChange;
    const callerBlur = onBlur;
    value = draft ?? (numericValue == null ? '' : String(numericValue));
    onChange = (e) => {
      const raw = e.target.value;
      setDraft(raw);
      const trimmed = raw.trim();
      let parsed: number | null;
      if (trimmed === '' || NUMERIC_NULL_RE.test(trimmed)) {
        parsed = null;
      } else {
        const n = integer ? parseInt(trimmed, 10) : parseFloat(trimmed);
        parsed = Number.isFinite(n) ? n : null;
      }
      onNumericChange?.(parsed);
      callerChange?.(e);
    };
    onBlur = (e) => {
      setDraft(null); // snap back to the canonical `numericValue`
      callerBlur?.(e);
    };
  }

  // Wrap focus/blur to track editing state. Composed here — after the numeric
  // block above may have reassigned `onBlur` — so both handlers still fire.
  const callerFocus = onFocus;
  onFocus = (e) => {
    setFocused(true);
    callerFocus?.(e);
  };
  const composedBlur = onBlur;
  onBlur = (e) => {
    setFocused(false);
    composedBlur?.(e);
  };

  // `blankZero`: render a literal 0 as an empty field (for "0 = off" params, so
  // an unset gate reads blank). Display-only — the bound value is untouched, so
  // an unedited 0 still saves as 0. Skipped while focused so it never rewrites
  // the field mid-edit (which would drop a leading `0`, e.g. `0.4` → `.4`).
  if (blankZero && !focused && (value === 0 || value === '0')) value = '';

  const split = unit ? splitLayoutClass(className) : null;
  const fieldCls = fieldClassName({ size: fieldSize, variant, type, className: split ? split.rest : className });

  // Measures where the typed value ends, so the unit suffix sits right after it
  // ("5 ◎"). The mirror shares the input's typography + left padding; we zero its
  // right padding/width so its measured width is paddingLeft + textWidth.
  const mirrorRef = useRef<HTMLSpanElement | null>(null);
  const [offset, setOffset] = useState(0);
  useLayoutEffect(() => {
    if (unit && mirrorRef.current) setOffset(mirrorRef.current.offsetWidth);
  }, [unit, value, fieldCls]);

  if (!unit) {
    return (
      <input
        ref={innerRef}
        type={type}
        value={value}
        onChange={onChange}
        onBlur={onBlur}
        onFocus={onFocus}
        className={fieldCls}
        {...props}
      />
    );
  }

  const hasValue = value != null && `${value}` !== '';
  const sized = split != null && /(?:^|\s)w-/.test(split.layout);

  return (
    <span className={cn('relative inline-flex items-center', split?.layout, !sized && 'w-full')}>
      <input
        ref={innerRef}
        type={type}
        value={value}
        onChange={onChange}
        onBlur={onBlur}
        onFocus={onFocus}
        className={cn(fieldCls, 'w-full min-w-0')}
        {...props}
      />
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

  // Height the resize grip last set. Null ⇒ follow the text.
  const userHeight = useRef<number | null>(null);

  useLayoutEffect(() => {
    const el = innerRef.current;
    if (!el || !autoResize) return;

    // True while `fit` itself is the thing changing the box, so the observer
    // does not record that write as a user drag.
    let fromUs = false;
    // Last height `fit` wrote. A grip drag moves the box off this number; a
    // width change does not, and that one still refits to the wrapped text.
    let applied = -1;

    const fit = () => {
      // Pattern rows stay mounted under `display: none`. scrollHeight is 0
      // there, and writing it locks the field at 0px after the row opens.
      // Leave height unset and refit once the element has a box.
      if (el.getClientRects().length === 0) {
        el.style.height = '';
        applied = -1;
        return;
      }
      fromUs = true;
      if (userHeight.current != null) {
        el.style.height = `${userHeight.current}px`;
        applied = el.offsetHeight;
        return;
      }
      el.style.height = 'auto';
      const content = el.scrollHeight;
      if (content < 1) {
        el.style.height = '';
        applied = -1;
        fromUs = false;
        return;
      }
      el.style.height = `${content}px`;
      // border-box: a height of scrollHeight still clips by the border, which
      // then shows a scrollbar and wraps another line. Add that overflow back.
      const extra = el.scrollHeight - el.clientHeight;
      if (extra > 0) el.style.height = `${el.offsetHeight + extra}px`;
      applied = el.offsetHeight;
    };

    fit();

    const ro = new ResizeObserver(() => {
      if (fromUs) {
        fromUs = false;
        return;
      }
      if (el.getClientRects().length === 0) return;
      const h = el.offsetHeight;
      if (applied >= 0 && Math.abs(h - applied) > 1) {
        userHeight.current = h;
        applied = h;
        return;
      }
      if (userHeight.current == null) fit();
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [autoResize, value]);

  return (
    <textarea
      ref={innerRef}
      rows={rows}
      value={value}
      className={cn(
        fieldClassName({ size: fieldSize, variant, className }),
        // Floor keeps a field readable if it is measured before it is shown.
        // The grip always works; auto-resize only picks the starting height.
        'min-h-8 resize-y overflow-auto leading-snug',
      )}
      {...props}
    />
  );
});
