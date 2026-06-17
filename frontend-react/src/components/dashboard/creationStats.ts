// Token-creation-time bias dashboard — shared types + pure helpers.
// Mirrors the backend `GET /api/tokens/creation-stats` response (handler
// `creation_stats.rs`). All three color metrics ship in one payload, so the
// metric toggle is a pure client-side re-color (no refetch).

export type CreationView = 'heatmap' | 'trend';
export type CreationBucket = 'hour' | 'day' | 'week';
export type CreationMetric = 'count' | 'migrate_rate' | 'dead_rate';
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
}

/** One absolute-calendar bucket; `bucket` is local wall-clock (naive ISO). */
export interface CreationTrendPoint {
  bucket: string;
  count: number;
  matured: number;
  known: number;
  migrated: number;
  dead: number;
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
];

export const BUCKET_OPTIONS: { value: CreationBucket; label: string }[] = [
  { value: 'hour', label: 'Hour' },
  { value: 'day', label: 'Day' },
  { value: 'week', label: 'Week' },
];

/** Date-range presets → look-back days (drives the `from` window bound). */
export const RANGE_OPTIONS: { value: number; label: string }[] = [
  { value: 7, label: '7d' },
  { value: 30, label: '30d' },
  { value: 90, label: '90d' },
  { value: 180, label: '180d' },
];

/**
 * The metric value for a cell-or-point. `count` returns the raw creation count
 * (volume view — never censored). Outcome rates divide by `known` (matured AND
 * has a `tokens_info` row); `null` when coverage is zero so the UI can render
 * "no data" rather than a misleading 0% (trap #2).
 */
export function metricValue(
  d: { count: number; known: number; migrated: number; dead: number },
  metric: CreationMetric,
): number | null {
  if (metric === 'count') return d.count;
  if (d.known === 0) return null;
  return metric === 'migrate_rate' ? d.migrated / d.known : d.dead / d.known;
}

const METRIC_RGB: Record<CreationMetric, string> = {
  count: '19,206,175', // primary teal — volume
  migrate_rate: '34,197,94', // green — good outcome
  dead_rate: '239,68,68', // red — bad outcome
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
