import { type ReactNode } from 'react';
import { cn } from 'lib/cn';

/** One stat tile — either a pre-formatted `value` string or a custom `node`
 *  (e.g. a coloured W/L/Open breakdown). `cls` tones the value text. */
export interface SummaryStat {
  label: string;
  value?: string;
  node?: ReactNode;
  cls?: string;
  /** When set, the tile is a toggle button (focus lens). Capital tiles omit this. */
  onClick?: () => void;
  /** Highlight when the matching focus lens is active. */
  active?: boolean;
}

/** A labelled band of stat tiles rendered under the hero row. Use when the
 *  metrics split into groups that must not be read as one undifferentiated
 *  strip — e.g. realized-only figures beside their mark-to-market counterparts,
 *  where mistaking one band for the other changes the conclusion. */
export interface SummarySection {
  /** Band label, shown small-caps at the left of the row. */
  title: string;
  /** Optional one-line gloss under the title — say what the band measures over. */
  hint?: string;
  /** Tones the band's title (e.g. `text-green` / `text-warning`). */
  titleCls?: string;
  /** Optional full-width element rendered above the band's tiles — for a figure
   *  the tiles annotate rather than a tile itself (e.g. the exit-mix proportion
   *  bar, whose whole point is spanning the row so segment widths are
   *  comparable). Tiles can't express this: they're fixed-width and wrap. */
  lead?: ReactNode;
  stats: SummaryStat[];
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
  /** Labelled bands rendered after `detailStats`, each on its own row with its
   *  own divider. Prefer over a single long `detailStats` strip when the metrics
   *  fall into groups the reader must tell apart at a glance. */
  sections?: SummarySection[];
  /** Left marker-bar colour class (default `bg-primary`). */
  accentClass?: string;
  className?: string;
}

/**
 * The shared "summary above a positions/results table" panel — one hero KPI row,
 * a lighter detail strip, and any number of labelled `sections` bands below it
 * (each its own row, so groups that must not be conflated stay visually apart).
 * Purely presentational: callers pass already-formatted
 * tile strings (or nodes), so it stays decoupled from any particular summary wire
 * shape or price-unit context. `PositionSummarySection` (Evidence / Simulate /
 * Sweep) renders its hero through this so the summary reads identically everywhere.
 */
export function SummaryStatsPanel({
  title,
  subtitle,
  onClose,
  heroStats,
  detailStats = [],
  sections = [],
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
          <HeroTile key={s.label} stat={s} />
        ))}
      </div>

      {detailStats.length > 0 && (
        <div className="mt-5 flex flex-wrap gap-x-8 gap-y-3 border-t border-white/6 pt-4">
          {detailStats.map((s) => (
            <DetailTile key={s.label} stat={s} />
          ))}
        </div>
      )}

      {sections.map((sec) => (
        <div key={sec.title} className="mt-4 border-t border-white/6 pt-4">
          {/* Label gutter left, tiles right — on narrow viewports the gutter
              stacks above its band so the grouping never collapses into one
              undifferentiated strip. */}
          <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:gap-6">
            <div className="shrink-0 sm:w-44">
              <div
                className={cn(
                  'text-[10px] font-bold uppercase tracking-wider text-text-mid',
                  sec.titleCls,
                )}
              >
                {sec.title}
              </div>
              {sec.hint && (
                <div className="mt-0.5 text-[10px] leading-snug text-text-dim">{sec.hint}</div>
              )}
            </div>
            <div className="flex flex-1 flex-col gap-2.5">
              {sec.lead}
              <div className="flex flex-wrap gap-x-8 gap-y-3">
                {sec.stats.map((s) => (
                  <DetailTile key={s.label} stat={s} />
                ))}
              </div>
            </div>
          </div>
        </div>
      ))}
    </div>
  );
}

function HeroTile({ stat }: { stat: SummaryStat }) {
  const inner = (
    <>
      <span className="text-[10px] font-semibold uppercase tracking-wider text-text-dim">
        {stat.label}
      </span>
      <span
        className={cn(
          'font-mono text-3xl font-extrabold leading-none tracking-tight text-text',
          stat.cls,
        )}
      >
        {stat.node ?? stat.value}
      </span>
    </>
  );
  if (!stat.onClick) {
    return <div className="flex flex-col gap-1">{inner}</div>;
  }
  return (
    <button
      type="button"
      onClick={stat.onClick}
      className={cn(
        'flex flex-col gap-1 rounded-md px-1.5 py-1 text-left transition hover:bg-white/5',
        stat.active && 'bg-primary/15 ring-1 ring-primary/50',
      )}
    >
      {inner}
    </button>
  );
}

/** One small label-over-value tile — the shared atom of the detail strip and of
 *  every `SummarySection` band, so the two never drift apart visually. */
function DetailTile({ stat }: { stat: SummaryStat }) {
  const body = (
    <>
      <span className="text-[9px] font-semibold uppercase tracking-wider text-text-dim">
        {stat.label}
      </span>
      <span className={cn('font-mono text-sm font-bold text-text', stat.cls)}>
        {stat.node ?? stat.value}
      </span>
    </>
  );
  if (!stat.onClick) {
    return <div className="flex min-w-[84px] flex-col gap-0.5">{body}</div>;
  }
  return (
    <button
      type="button"
      onClick={stat.onClick}
      className={cn(
        'flex min-w-[84px] flex-col gap-0.5 rounded-md px-1 py-0.5 text-left transition hover:bg-white/5',
        stat.active && 'bg-primary/15 ring-1 ring-primary/50',
      )}
    >
      {body}
    </button>
  );
}
