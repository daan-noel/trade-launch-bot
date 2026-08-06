import {
  useCallback,
  useRef,
  type KeyboardEvent,
  type ReactNode,
} from 'react';
import { cn } from 'lib/cn';

export type ToggleGroupTone = 'primary' | 'mode' | 'neutral';
export type ToggleGroupSize = 'sm' | 'md';

export interface ToggleGroupOption<T extends string = string> {
  value: T;
  label: ReactNode;
  /** Optional count shown after the label (tabular). */
  count?: number;
  /** Optional rail swatch (e.g. mode color bar). */
  swatch?: string;
  /** Tooltip / title on the option button. */
  title?: string;
  /**
   * Active classes when `tone="mode"`. Literal Tailwind strings so the scanner
   * sees them at the call site (or here for the built-in mode options).
   */
  activeClassName?: string;
}

export interface ToggleGroupProps<T extends string = string> {
  options: ToggleGroupOption<T>[];
  value: T;
  onChange: (next: T) => void;
  /** Accessible name for the group. */
  'aria-label': string;
  size?: ToggleGroupSize;
  /**
   * `primary` — selected = primary wash (ranges, score scope).
   * `mode` — selected uses each option's `activeClassName` (paper/real hues).
   * `neutral` — selected = white wash (SOL/USD-style dual).
   */
  tone?: ToggleGroupTone;
  className?: string;
}

const sizeClasses: Record<ToggleGroupSize, string> = {
  sm: 'px-2 py-1 text-[11px]',
  md: 'px-2 py-1 text-xs',
};

const toneActive: Record<Exclude<ToggleGroupTone, 'mode'>, string> = {
  primary: 'bg-primary/20 text-primary',
  neutral: 'bg-white/10 text-text shadow-sm',
};

/**
 * Segmented exclusive-choice control for filters (mode, date range, score scope).
 * Not for panel swaps — use `Tabs` for those.
 */
export function ToggleGroup<T extends string>({
  options,
  value,
  onChange,
  'aria-label': ariaLabel,
  size = 'md',
  tone = 'primary',
  className,
}: ToggleGroupProps<T>) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);

  const focusAt = useCallback((i: number) => {
    const el = refs.current[i];
    el?.focus();
  }, []);

  const onKeyDown = useCallback(
    (e: KeyboardEvent<HTMLDivElement>) => {
      if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
      const idx = options.findIndex((o) => o.value === value);
      if (idx < 0) return;
      e.preventDefault();
      const next =
        e.key === 'ArrowRight'
          ? (idx + 1) % options.length
          : (idx - 1 + options.length) % options.length;
      onChange(options[next].value);
      focusAt(next);
    },
    [focusAt, onChange, options, value],
  );

  return (
    <div
      role="group"
      aria-label={ariaLabel}
      onKeyDown={onKeyDown}
      className={cn('inline-flex items-center gap-0.5 rounded-md bg-white/5 p-0.5', className)}
    >
      {options.map((opt, i) => {
        const selected = value === opt.value;
        // Per-option activeClassName wins when set (mode hues, SOL/USD dual colors).
        const activeCls =
          opt.activeClassName ??
          (tone === 'mode' ? toneActive.primary : toneActive[tone]);
        return (
          <button
            key={opt.value}
            ref={(el) => {
              refs.current[i] = el;
            }}
            type="button"
            aria-pressed={selected}
            title={opt.title}
            onClick={() => onChange(opt.value)}
            className={cn(
              'flex cursor-pointer items-center gap-1.5 rounded font-semibold transition-colors',
              sizeClasses[size],
              selected ? activeCls : 'text-text-dim hover:bg-white/8 hover:text-text',
            )}
          >
            {opt.swatch && <span className={cn('h-3 w-0.5 rounded-full', opt.swatch)} />}
            {opt.label}
            {opt.count != null && (
              <span className="tabular-nums text-[10px] font-normal opacity-70">{opt.count}</span>
            )}
          </button>
        );
      })}
    </div>
  );
}
