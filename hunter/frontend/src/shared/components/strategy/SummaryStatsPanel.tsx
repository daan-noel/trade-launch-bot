import { type ReactNode } from 'react';
import { cn } from 'lib/cn';

/** One stat tile — either a pre-formatted `value` string or a custom `node`
 *  (e.g. a coloured W/L/Open breakdown). `cls` tones the value text. */
export interface SummaryStat {
  label: string;
  value?: string;
  node?: ReactNode;
  cls?: string;
}

interface SummaryStatsPanelProps {
  /** Section heading. */
  title: string;
  /** Small mono-dim caption after the title (rule name, combo descriptor, …). */
  subtitle?: string;
  /** Dismiss handler. Omit to render without a ✕ (e.g. when it's an intrinsic
   *  part of a section rather than a dismissible overlay). */
  onClose?: () => void;
  /** Large headline KPIs, shown big (2–4 read best). */
  heroStats: SummaryStat[];
  /** Secondary strip below the divider — the long-tail detail metrics. */
  detailStats?: SummaryStat[];
  /** Left marker-bar colour class (default `bg-primary`). */
  accentClass?: string;
  className?: string;
}

/**
 * The shared "summary above a positions/results table" panel — one hero KPI row
 * plus a lighter detail strip. Purely presentational: callers pass already-formatted
 * tile strings (or nodes), so it stays decoupled from any particular summary wire
 * shape or price-unit context. `SimSummaryCard` (live/paper positions), the Simulate
 * page, and the grouped-sweep combo drill-in all render through this so the summary
 * reads identically everywhere.
 */
export function SummaryStatsPanel({
  title,
  subtitle,
  onClose,
  heroStats,
  detailStats = [],
  accentClass = 'bg-primary',
  className,
}: SummaryStatsPanelProps) {
  return (
    <div className={cn('mb-5', className)}>
      <div className="mb-4 flex items-center gap-2.5">
        <span className={cn('h-4 w-1 rounded-full', accentClass)} />
        <h3 className="text-sm font-bold text-text">{title}</h3>
        {subtitle && (
          <span className="truncate font-mono text-[11px] text-text-dim">{subtitle}</span>
        )}
        <span className="flex-1" />
        {onClose && (
          <button
            type="button"
            onClick={onClose}
            className="text-text-dim transition hover:text-text"
          >
            ✕
          </button>
        )}
      </div>

      <div className="flex flex-wrap gap-x-10 gap-y-4">
        {heroStats.map((s) => (
          <div key={s.label} className="flex flex-col gap-1">
            <span className="text-[10px] font-semibold uppercase tracking-wider text-text-dim">
              {s.label}
            </span>
            <span
              className={cn(
                'font-mono text-3xl font-extrabold leading-none tracking-tight text-text',
                s.cls,
              )}
            >
              {s.node ?? s.value}
            </span>
          </div>
        ))}
      </div>

      {detailStats.length > 0 && (
        <div className="mt-5 flex flex-wrap gap-x-8 gap-y-3 border-t border-white/6 pt-4">
          {detailStats.map((s) => (
            <div key={s.label} className="flex min-w-[84px] flex-col gap-0.5">
              <span className="text-[9px] font-semibold uppercase tracking-wider text-text-dim">
                {s.label}
              </span>
              <span className={cn('font-mono text-sm font-bold text-text', s.cls)}>
                {s.node ?? s.value}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
