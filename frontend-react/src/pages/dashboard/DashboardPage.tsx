import { useMemo, useState } from 'react';
import { Button } from 'components/ui/Button';
import { Select } from 'components/ui/Select';
import { TimezoneSelect } from 'components/ui/TimezoneSelect';
import { StatCard } from 'components/ui/StatCard';
import { useTimezone } from 'context/TimezoneContext';
import { useGetCreationStatsQuery } from 'store/apiSlice';
import { apiErrorMessage } from 'store/apiSlice';
import { CreationHeatmap } from 'components/dashboard/CreationHeatmap';
import { CreationTrendChart } from 'components/dashboard/CreationTrendChart';
import { GroupedCreationSection } from 'components/dashboard/GroupedCreationSection';
import {
  METRIC_OPTIONS,
  RANGE_OPTIONS,
  SEGMENT_OPTIONS,
  bucketOptionsForRange,
  clampBucketToRange,
  formatPct,
  windowFrom,
  type CreationBucket,
  type CreationMetric,
  type CreationSegment,
} from 'components/dashboard/creationStats';
import { formatWithCommas } from 'utils/format';

export function DashboardPage() {
  const { timezone } = useTimezone();
  const [metric, setMetric] = useState<CreationMetric>('migrate_rate');
  const [segment, setSegment] = useState<CreationSegment>('all');
  const [bucket, setBucket] = useState<CreationBucket>('day');
  const [rangeDays, setRangeDays] = useState(30);

  const from = useMemo(() => windowFrom(rangeDays), [rangeDays]);

  // Range-gated bucket granularities; clamp the current pick so a range change
  // (e.g. 10m → 180d) never leaves an out-of-range bucket selected.
  const bucketOpts = useMemo(() => bucketOptionsForRange(rangeDays), [rangeDays]);
  const effBucket = clampBucketToRange(bucket, rangeDays);

  // Two cache entries: the heatmap fold + the absolute-time trend. The metric
  // toggle re-colors the heatmap client-side (all metrics ship in one payload),
  // so changing it never refetches.
  const heat = useGetCreationStatsQuery({
    view: 'heatmap',
    bucket: 'day',
    tz: timezone,
    from,
    segment,
  });
  const trend = useGetCreationStatsQuery({
    view: 'trend',
    bucket: effBucket,
    tz: timezone,
    from,
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

        <Select
          value={String(rangeDays)}
          onChange={(e) => setRangeDays(Number(e.target.value))}
          title="Look-back window"
          className="max-w-[7rem]"
        >
          {RANGE_OPTIONS.map((o) => (
            <option key={o.value} value={o.value}>
              Last {o.label}
            </option>
          ))}
        </Select>

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
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
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
            {metric === 'count'
              ? 'shade = share of total created'
              : 'shade = rate (matured tokens with known outcome)'}
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
          <CreationTrendChart points={trend.data.points} />
        ) : (
          <p className="text-text-dim">No tokens created in this window.</p>
        )}
      </section>

      {/* Panel C — per-fingerprint creation activity (shares the control bar's
          window / timezone / segment; owns its own group-by + top-N). */}
      <GroupedCreationSection tz={timezone} segment={segment} />
    </div>
  );
}
