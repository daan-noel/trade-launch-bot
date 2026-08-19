import { lazy, Suspense, useMemo } from 'react';
import { LoadingState } from 'components/ui/LoadingState';
import { Button } from 'components/ui/Button';
import { Select } from 'components/ui/Select';
import { TimezoneSelect } from 'components/ui/TimezoneSelect';
import { StatCard } from 'components/ui/StatCard';
import { useTimezone } from 'context/TimezoneContext';
import { useGetCreationStatsQuery } from 'store/apiSlice';
import { apiErrorMessage } from 'store/apiSlice';
import { useStoredField } from 'hooks/useLocalStorage';
import { STORAGE_KEYS } from 'lib/storage';
import { CreationHeatmap } from 'components/creation-stats/CreationHeatmap';
import { CreationWindowPicker } from 'components/creation-stats/CreationWindowPicker';
import { GroupedCreationSection } from '@lab/components/creation-stats/GroupedCreationSection';
import {
  DEFAULT_CREATION_WINDOW,
  METRIC_KIND,
  METRIC_OPTIONS,
  SEGMENT_OPTIONS,
  bucketOptionsForRange,
  clampBucketToRange,
  formatPct,
  resolveCreationWindow,
  toCreationWindow,
  type CreationBucket,
  type CreationMetric,
  type CreationSegment,
  type CreationWindow,
} from 'components/creation-stats/creationStats';
import { formatWithCommas } from 'utils/format';

/** Trend chart pulls `lightweight-charts` — keep it out of the route shell. */
const CreationTrendChart = lazy(() =>
  import('components/creation-stats/CreationTrendChart').then((m) => ({
    default: m.CreationTrendChart,
  })),
);

/** Token creation analysis: heatmap + trend + grouped (lab) section. */
export function CreationStatsPage() {
  const { timezone } = useTimezone();
  // Page + grouped-section controls share one `mt:page.creationStats` blob —
  // one key for the surface, one field per control.
  const P = STORAGE_KEYS.pageCreationStats;
  const [metric, setMetric] = useStoredField<CreationMetric>(P, 'metric', 'count');
  const [segment, setSegment] = useStoredField<CreationSegment>(P, 'segment', 'all');
  const [bucket, setBucket] = useStoredField<CreationBucket>(P, 'bucket', 'hour');
  // `range` also holds the legacy bare day count, which `toCreationWindow` reads
  // as the equivalent preset — a stored look-back survives the upgrade.
  const [storedWindow, setStoredWindow] = useStoredField<CreationWindow | number>(
    P,
    'range',
    DEFAULT_CREATION_WINDOW,
  );
  const win = useMemo(() => toCreationWindow(storedWindow), [storedWindow]);

  const { from, to, spanDays } = useMemo(
    () => resolveCreationWindow(win, timezone),
    [win, timezone],
  );

  // Span-gated bucket granularities; clamp the current pick so a window change
  // (e.g. 10m → 180d) never leaves an out-of-range bucket selected.
  const bucketOpts = useMemo(() => bucketOptionsForRange(spanDays), [spanDays]);
  const effBucket = clampBucketToRange(bucket, spanDays);

  // Two cache entries: the heatmap fold + the absolute-time trend. The metric
  // toggle re-colors the heatmap client-side (all metrics ship in one payload),
  // so changing it never refetches.
  const heat = useGetCreationStatsQuery({
    view: 'heatmap',
    bucket: 'day',
    tz: timezone,
    from,
    to,
    segment,
  });
  const trend = useGetCreationStatsQuery({
    view: 'trend',
    bucket: effBucket,
    tz: timezone,
    from,
    to,
    segment,
  });

  const coverage =
    heat.data && heat.data.matured > 0 ? heat.data.known / heat.data.matured : null;

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-2.5">
        <h2 className="text-base font-bold text-primary">Token Creation Bias</h2>
        <span className="text-xs text-text-dim">
          When tokens launch vs. how they end up
        </span>
      </div>

      {/* Shared control bar */}
      <div className="flex flex-wrap items-center gap-2">
        <TimezoneSelect />

        <CreationWindowPicker
          aria-label="Creation window"
          value={win}
          onChange={setStoredWindow}
          timezone={timezone}
        />

        <Select
          value={segment}
          onChange={(e) => setSegment(e.target.value as CreationSegment)}
          title="Segment"
          className="max-w-[12rem]"
        >
          {SEGMENT_OPTIONS.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </Select>

        <div className="flex items-center gap-1">
          {METRIC_OPTIONS.map((o) => (
            <Button
              key={o.value}
              size="sm"
              variant="subtle"
              active={metric === o.value}
              onClick={() => setMetric(o.value)}
            >
              {o.label}
            </Button>
          ))}
        </div>
      </div>

      {/* Summary stats */}
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-5">
        <StatCard
          label="Tokens created"
          value={heat.data ? formatWithCommas(heat.data.total) : '—'}
        />
        <StatCard
          label="Matured"
          value={heat.data ? formatWithCommas(heat.data.matured) : '—'}
        />
        <StatCard
          label="Outcome coverage"
          value={formatPct(coverage)}
          variant={coverage != null && coverage < 0.5 ? 'warning' : 'default'}
        />
        <StatCard
          label="Maturity window"
          value={
            heat.data ? `${Math.round(heat.data.maturity_secs / 3600)}h` : '24h'
          }
        />
        <StatCard
          label="Trades"
          value={heat.data ? formatWithCommas(heat.data.trades) : '—'}
        />
      </div>

      {heat.isError && (
        <p className="text-red">
          {apiErrorMessage(heat.error, 'Failed to load creation stats')}
        </p>
      )}

      {/* Panel B — seasonality heatmap (centerpiece) */}
      <section className="rounded-lg border border-white/8 bg-white/2 p-3">
        <div className="mb-2 flex items-center justify-between">
          <h3 className="text-sm font-semibold text-text">
            Weekly seasonality — day × hour
          </h3>
          <span className="text-[10px] text-text-dim">
            {METRIC_KIND[metric] === 'magnitude'
              ? 'shade = share of max'
              : METRIC_KIND[metric] === 'rate'
                ? 'shade = rate, scaled across cells · label = actual %'
                : 'shade = avg per token (log-scaled), scaled across cells'}
          </span>
        </div>
        {heat.isLoading ? (
          <p className="text-text-dim">Loading…</p>
        ) : heat.data && heat.data.cells.length > 0 ? (
          <CreationHeatmap
            cells={heat.data.cells}
            metric={metric}
            total={heat.data.total}
          />
        ) : (
          <p className="text-text-dim">No tokens created in this window.</p>
        )}
      </section>

      {/* Panel A — absolute-time trend */}
      <section className="rounded-lg border border-white/8 bg-white/2 p-3">
        <div className="mb-2 flex items-center justify-between">
          <h3 className="text-sm font-semibold text-text">Creation trend</h3>
          <div className="flex items-center gap-1">
            {bucketOpts.map((o) => (
              <Button
                key={o.value}
                size="sm"
                variant="subtle"
                active={effBucket === o.value}
                onClick={() => setBucket(o.value)}
              >
                {o.label}
              </Button>
            ))}
          </div>
        </div>
        {trend.isLoading ? (
          <p className="text-text-dim">Loading…</p>
        ) : trend.data && trend.data.points.length > 0 ? (
          <Suspense
            fallback={<LoadingState variant="inline" label="Loading chart…" />}
          >
            <CreationTrendChart points={trend.data.points} metric={metric} />
          </Suspense>
        ) : (
          <p className="text-text-dim">No tokens created in this window.</p>
        )}
      </section>

      {/* Panel C — per-fingerprint creation activity (lab-only page). */}
      <GroupedCreationSection tz={timezone} segment={segment} />
    </div>
  );
}
