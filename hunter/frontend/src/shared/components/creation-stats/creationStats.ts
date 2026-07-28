// Token-creation-time bias dashboard — shared types + pure helpers.
// Mirrors the backend `GET /api/tokens/creation-stats` response (handler
// `creation_stats.rs`). All three color metrics ship in one payload, so the
// metric toggle is a pure client-side re-color (no refetch).

export type CreationView = 'heatmap' | 'trend';
export type CreationBucket =
  | '10m'
  | '30m'
  | 'hour'
  | '4h'
  | '8h'
  | '12h'
  | 'day'
  | 'week';
export type CreationMetric =
  | 'count'
  | 'migrate_rate'
  | 'dead_rate'
  | 'trades'
  | 'trades_per_token'
  | 'trades_per_day';
export type CreationSegment =
  | 'all'
  | 'mayhem'
  | 'non_mayhem'
  | 'cashback'
  | 'non_cashback';

/** One day-of-week × hour-of-day cell. `dow`: 0=Sun … 6=Sat. */
export interface CreationHeatCell {
  dow: number;
  hour: number;
  count: number;
  matured: number;
  known: number;
  migrated: number;
  dead: number;
  /** Lifetime-to-last-sync trade count (see `metricValue`'s age-bias caveat). */
  trades: number;
  /** Mean trades/token (`SUM/COUNT`); `null` when no matured+known token. */
  trades_avg: number | null;
  /** Age-normalized `SUM(trade_count / age_days)` — composes across cells. */
  trades_per_day: number;
}

/** One absolute-calendar bucket; `bucket` is local wall-clock (naive ISO). */
export interface CreationTrendPoint {
  bucket: string;
  count: number;
  matured: number;
  known: number;
  migrated: number;
  dead: number;
  /** See {@link CreationHeatCell.trades}. */
  trades: number;
  /** See {@link CreationHeatCell.trades_avg}. */
  trades_avg: number | null;
  /** See {@link CreationHeatCell.trades_per_day}. */
  trades_per_day: number;
}

export interface CreationStatsResponse {
  view: CreationView;
  bucket: CreationBucket;
  tz: string;
  from: string;
  to: string;
  maturity_secs: number;
  segment: string;
  total: number;
  matured: number;
  known: number;
  /** Σ trades across the window (a plain sum — see the backend `trades_avg`
   *  drop rationale; there's no window-level `trades_avg` for the same reason). */
  trades: number;
  /** Σ trades_per_day across the window. */
  trades_per_day: number;
  cells: CreationHeatCell[];
  points: CreationTrendPoint[];
}

export interface CreationStatsArgs {
  view: CreationView;
  bucket: CreationBucket;
  tz: string;
  /** RFC3339; omit to use the backend default (last 30d). */
  from?: string;
  segment: CreationSegment;
}

export const SEGMENT_OPTIONS: { value: CreationSegment; label: string }[] = [
  { value: 'all', label: 'All tokens' },
  { value: 'mayhem', label: 'Mayhem mode' },
  { value: 'non_mayhem', label: 'Standard (non-mayhem)' },
  { value: 'cashback', label: 'Cashback enabled' },
  { value: 'non_cashback', label: 'Cashback disabled' },
];

export const METRIC_OPTIONS: { value: CreationMetric; label: string }[] = [
  { value: 'count', label: 'Count' },
  { value: 'migrate_rate', label: 'Migrate %' },
  { value: 'dead_rate', label: 'Dead %' },
  { value: 'trades', label: 'Trades' },
  { value: 'trades_per_token', label: 'Trades/token' },
  { value: 'trades_per_day', label: 'Trades/day (age-adj)' },
];

/** All bucket granularities, coarse→fine label, with the max look-back (days)
 *  each one is offered for. Finer buckets are hidden on long ranges so a series
 *  never balloons to tens of thousands of points (payload + render cost). */
interface BucketOption {
  value: CreationBucket;
  label: string;
  /** Largest `rangeDays` this granularity is selectable at. */
  maxRangeDays: number;
}

export const BUCKET_OPTIONS: BucketOption[] = [
  { value: '10m', label: '10m', maxRangeDays: 7 },
  { value: '30m', label: '30m', maxRangeDays: 7 },
  { value: 'hour', label: 'Hour', maxRangeDays: 30 },
  { value: '4h', label: '4h', maxRangeDays: 90 },
  { value: '8h', label: '8h', maxRangeDays: 90 },
  { value: '12h', label: '12h', maxRangeDays: 180 },
  { value: 'day', label: 'Day', maxRangeDays: Infinity },
  { value: 'week', label: 'Week', maxRangeDays: Infinity },
];

/** The bucket granularities sensible for a given look-back window. */
export function bucketOptionsForRange(rangeDays: number): BucketOption[] {
  return BUCKET_OPTIONS.filter((o) => rangeDays <= o.maxRangeDays);
}

/** Clamp a bucket to the coarsest option still valid for `rangeDays` (used when a
 *  range change invalidates the current selection — e.g. 10m → 180d). */
export function clampBucketToRange(bucket: CreationBucket, rangeDays: number): CreationBucket {
  const allowed = bucketOptionsForRange(rangeDays);
  return allowed.some((o) => o.value === bucket)
    ? bucket
    : (allowed[allowed.length - 1]?.value ?? 'day');
}

/** Date-range presets → look-back days (drives the `from` window bound). The
 *  short presets (1d/3d) make the sub-hour buckets actually useful. */
export const RANGE_OPTIONS: { value: number; label: string }[] = [
  { value: 1, label: '1d' },
  { value: 3, label: '3d' },
  { value: 7, label: '7d' },
  { value: 30, label: '30d' },
  { value: 90, label: '90d' },
  { value: 180, label: '180d' },
];

/**
 * How a metric's value should be normalized/labeled — driven off, never a
 * per-callsite `metric === 'count'` ternary (there were three of these before
 * this type existed; a new metric silently fell into the rate branch and
 * rendered as a bogus percent). `magnitude` → normalize against the max
 * observed value (share-of-max), label = compact number. `rate` → a bounded
 * 0..1 fraction, contrast-stretched across cells, label = percent. `ratio` →
 * an unbounded per-token average (backed by `trades_avg`, a mean — NOT a
 * median, see the trade-counts plan §1/§2), contrast-stretched on a
 * log1p scale (buys back the outlier-robustness a true median would have
 * given, without the server-side sort cost), label = compact number.
 */
export type MetricKind = 'magnitude' | 'rate' | 'ratio';

export const METRIC_KIND: Record<CreationMetric, MetricKind> = {
  count: 'magnitude',
  trades: 'magnitude',
  trades_per_day: 'magnitude', // still a per-cell SUM (cohort-wide rate), not a ratio
  migrate_rate: 'rate',
  dead_rate: 'rate',
  trades_per_token: 'ratio', // backed by trades_avg (SUM/COUNT, not a median)
};

/**
 * The metric value for a cell-or-point. `count`/`trades`/`trades_per_day`
 * return the raw magnitude (volume view — never censored by the caller here).
 * `trades_per_token` reads `trades_avg` (already `null` when there's no
 * coverage). Outcome rates divide by `known` (matured AND has a `tokens_info`
 * row); `null` when coverage is zero so the UI can render "no data" rather
 * than a misleading 0% (trap #2).
 */
export function metricValue(
  d: {
    count: number;
    known: number;
    migrated: number;
    dead: number;
    trades: number;
    trades_avg: number | null;
    trades_per_day: number;
  },
  metric: CreationMetric,
): number | null {
  if (metric === 'count') return d.count;
  if (metric === 'trades') return d.trades;
  if (metric === 'trades_per_day') return d.trades_per_day;
  if (metric === 'trades_per_token') return d.trades_avg;
  if (d.known === 0) return null;
  return metric === 'migrate_rate' ? d.migrated / d.known : d.dead / d.known;
}

export const METRIC_RGB: Record<CreationMetric, string> = {
  count: '19,206,175', // primary teal — volume
  migrate_rate: '34,197,94', // green — good outcome
  dead_rate: '239,68,68', // red — bad outcome
  trades: '245,158,11', // amber — trade volume
  trades_per_token: '167,139,250', // violet — per-token average (ratio)
  trades_per_day: '56,189,248', // sky blue — age-adjusted rate
};

/**
 * Background color for a heatmap cell. `norm` is the already-normalized intensity
 * in [0,1] (count → value/max; rates → the rate itself, 0..1). `null` value →
 * a faint "no data" wash so empty cells stay visually distinct from a true zero.
 */
export function heatColor(metric: CreationMetric, norm: number | null): string {
  if (norm == null) return 'rgba(255,255,255,0.02)';
  const a = 0.06 + 0.82 * Math.min(1, Math.max(0, norm));
  return `rgba(${METRIC_RGB[metric]},${a.toFixed(3)})`;
}

export function formatPct(v: number | null): string {
  return v == null ? '—' : `${(v * 100).toFixed(1)}%`;
}

/** DOW order Mon→Sun (rows), with short labels. Postgres DOW: 0=Sun. */
export const DOW_ROWS: { dow: number; label: string }[] = [
  { dow: 1, label: 'Mon' },
  { dow: 2, label: 'Tue' },
  { dow: 3, label: 'Wed' },
  { dow: 4, label: 'Thu' },
  { dow: 5, label: 'Fri' },
  { dow: 6, label: 'Sat' },
  { dow: 0, label: 'Sun' },
];

export const HOURS = Array.from({ length: 24 }, (_, h) => h);

/** Floor `now` to the current hour and step back `days`, as an RFC3339 string.
 *  Hour-stable so the RTK cache key doesn't churn on every render. */
export function windowFrom(days: number): string {
  const d = new Date();
  d.setMinutes(0, 0, 0);
  d.setHours(d.getHours() - days * 24);
  return d.toISOString();
}
