import { Fragment, useEffect, useMemo, useState } from 'react';
import { skipToken } from '@reduxjs/toolkit/query/react';
import { Button } from 'components/ui/Button';
import { Select } from 'components/ui/Select';
import { useGetGroupedCreationStatsQuery, apiErrorMessage } from 'store/apiSlice';
import { parseNumbers, parseIxLabelsFilter } from 'components/sweep/fingerprintFilters';
import { formatWithCommas } from 'utils/format';
import { cn } from 'lib/cn';
import { CreationHeatmap } from './CreationHeatmap';
import { GroupedCreationTrendChart } from './GroupedCreationTrendChart';
import {
  bucketOptionsForRange,
  clampBucketToRange,
  type CreationBucket,
  type CreationHeatCell,
  type CreationSegment,
} from './creationStats';
import {
  GROUP_FIELDS,
  GROUP_FIELD_LABELS,
  TOP_OPTIONS,
  MISSING_VALUE,
  groupColor,
  groupValueParts,
  type GroupedCreationArgs,
  type GroupedCreationCell,
  type GroupedCreationGroup,
  type GroupField,
} from './groupedCreationStats';

interface GroupedCreationSectionProps {
  /** RFC3339 window lower bound (shared with the page's range control). */
  from: string;
  tz: string;
  segment: CreationSegment;
  /** Look-back days — gates the bucket-granularity options. */
  rangeDays: number;
}

/** Default grouping — CU limit + instruction-label set: the two fields that
 *  separate the bulk of launch fingerprints (validated against live data). */
const DEFAULT_GROUP_BY: GroupField[] = ['cu_limit', 'ix_labels'];

/** Scalar fingerprint fields that take a comma-separated value filter (numeric).
 *  `is_cashback_enabled` (a tri-state select) and `ix_labels` (a JSON-set textarea)
 *  are handled separately. */
const SCALAR_FILTER_FIELDS: GroupField[] = [
  'cu_limit',
  'cu_price',
  'max_sol_cost',
  'spendable_sol_in',
  'initial_buy_sol',
];

type CashbackFilter = 'all' | 'true' | 'false';

/** A `GroupedCreationCell` lacks the outcome fields `CreationHeatmap` reads; the
 *  count view never touches them, so zero-fill is safe (count = volume). */
function toHeatCell(c: GroupedCreationCell): CreationHeatCell {
  return { dow: c.dow, hour: c.hour, count: c.count, matured: 0, known: 0, migrated: 0, dead: 0 };
}

/**
 * Dashboard section: partition token creation by a fingerprint key and show each
 * group's recurring time-of-day bias (small-multiple day×hour heatmaps) plus its
 * calendar trend (multi-series line chart). Reuses the page's window / timezone /
 * segment; owns its own group-by, value filters, bucket, and top-N. Count only.
 *
 * The query is **manual**: nothing fetches until you click **Analyze**, which
 * snapshots the current draft (incl. the page window/tz/segment). The trend
 * chart's lines can be isolated by clicking a legend entry.
 */
export function GroupedCreationSection({ from, tz, segment, rangeDays }: GroupedCreationSectionProps) {
  // --- draft controls (don't fetch until applied) ---------------------------
  // Click order = compound-key order (matches the sweep page semantics).
  const [groupBy, setGroupBy] = useState<GroupField[]>(DEFAULT_GROUP_BY);
  const [top, setTop] = useState(8);
  const [bucket, setBucket] = useState<CreationBucket>('day');
  const [fieldFiltersText, setFieldFiltersText] = useState<Record<string, string>>({});
  const [cashbackFilter, setCashbackFilter] = useState<CashbackFilter>('all');
  const [ixLabelsText, setIxLabelsText] = useState('');

  // --- applied snapshot (drives the query) ----------------------------------
  const [applied, setApplied] = useState<GroupedCreationArgs | null>(null);
  const [isolatedGroup, setIsolatedGroup] = useState<number | null>(null);

  const bucketOpts = useMemo(() => bucketOptionsForRange(rangeDays), [rangeDays]);
  const effBucket = clampBucketToRange(bucket, rangeDays);

  // Parse the ix_labels textarea once; an active parse error blocks Analyze.
  const ixFilter = useMemo(() => parseIxLabelsFilter(ixLabelsText), [ixLabelsText]);

  // Build the query args from the current draft + page props (the thing Analyze
  // snapshots). Memoized so dirty-checking + click are cheap.
  const draftArgs = useMemo<GroupedCreationArgs>(() => {
    const fieldFilters: Record<string, string[]> = {};
    for (const f of SCALAR_FILTER_FIELDS) {
      const nums = parseNumbers(fieldFiltersText[f] ?? '');
      if (nums.length > 0) fieldFilters[f] = nums.map(String);
    }
    if (cashbackFilter !== 'all') fieldFilters['is_cashback_enabled'] = [cashbackFilter];

    return {
      bucket: effBucket,
      tz,
      from,
      segment,
      groupBy,
      top,
      ...(Object.keys(fieldFilters).length > 0 ? { fieldFilters } : {}),
      ...(ixFilter.labels ? { ixLabelsFilter: ixFilter.labels } : {}),
    };
  }, [effBucket, tz, from, segment, groupBy, top, fieldFiltersText, cashbackFilter, ixFilter.labels]);

  const { data, isFetching, isError, error } = useGetGroupedCreationStatsQuery(applied ?? skipToken);

  // Reset line isolation whenever a fresh analysis is applied.
  useEffect(() => setIsolatedGroup(null), [applied]);

  // "Dirty" = the draft differs from what's applied (or nothing applied yet).
  // Same builder on both sides ⇒ key order is stable, so a string compare works.
  const dirty = !applied || JSON.stringify(draftArgs) !== JSON.stringify(applied);

  // Partition cells by group rank so each heatmap gets only its own cells.
  // Memoized so the high-frequency SOL/USD + live-trade ticks never re-bucket.
  const cellsByGroup = useMemo(() => {
    const map = new Map<number, CreationHeatCell[]>();
    for (const c of data?.cells ?? []) {
      const arr = map.get(c.g) ?? [];
      arr.push(toHeatCell(c));
      map.set(c.g, arr);
    }
    return map;
  }, [data?.cells]);

  const toggleField = (field: GroupField) =>
    setGroupBy((prev) =>
      prev.includes(field) ? prev.filter((f) => f !== field) : [...prev, field],
    );

  const groups = data?.groups ?? [];

  return (
    <section className="rounded-lg border border-white/8 bg-white/2 p-3">
      <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
        <div className="flex flex-wrap items-center gap-2.5">
          <h3 className="text-sm font-semibold text-text">Creation by token group</h3>
          <span className="text-[10px] text-text-dim">
            when each fingerprint group launches — pick fields, filter values, then Analyze
          </span>
        </div>
        <div className="flex items-center gap-2">
          {/* Bucket granularity (range-gated). */}
          <Select
            value={effBucket}
            onChange={(e) => setBucket(e.target.value as CreationBucket)}
            title="Trend bucket granularity"
            className="max-w-[6rem]"
          >
            {bucketOpts.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </Select>
          <Select
            value={String(top)}
            onChange={(e) => setTop(Number(e.target.value))}
            title="Number of groups"
            className="max-w-[7rem]"
          >
            {TOP_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </Select>
          <Button
            size="sm"
            variant={dirty ? 'primary' : 'subtle'}
            active={dirty}
            disabled={isFetching || ixFilter.error != null}
            onClick={() => setApplied(draftArgs)}
            title={ixFilter.error ?? 'Run the analysis with the current settings'}
          >
            {isFetching ? 'Analyzing…' : dirty ? 'Analyze' : 'Analyzed'}
          </Button>
        </div>
      </div>

      {/* Group-by picker — order of selection is the compound-key order. */}
      <div className="mb-2 flex flex-wrap items-center gap-1">
        <span className="mr-1 text-xs text-text-dim">Group by:</span>
        {GROUP_FIELDS.map((f) => (
          <Button
            key={f}
            size="sm"
            variant="subtle"
            active={groupBy.includes(f)}
            onClick={() => toggleField(f)}
          >
            {GROUP_FIELD_LABELS[f]}
          </Button>
        ))}
      </div>

      {/* Per-field value filters — independent of grouping; restrict the corpus. */}
      <div className="mb-3 rounded-md border border-white/8 bg-white/2 p-2">
        <div className="mb-1.5 text-[10px] uppercase tracking-wide text-text-dim/70">
          Filter values (leave blank for all)
        </div>
        <div className="grid grid-cols-1 gap-x-4 gap-y-1.5 sm:grid-cols-2">
          {SCALAR_FILTER_FIELDS.map((f) => (
            <label key={f} className="flex items-center gap-2 text-xs text-text-mid">
              <span className="w-32 shrink-0 whitespace-nowrap">{GROUP_FIELD_LABELS[f]}</span>
              <input
                type="text"
                value={fieldFiltersText[f] ?? ''}
                onChange={(e) =>
                  setFieldFiltersText((prev) => ({ ...prev, [f]: e.target.value }))
                }
                placeholder="all values"
                title="Comma-separated values to keep (match the group-key text). e.g. 200000, 300000"
                className="min-w-0 flex-1 rounded border border-white/10 bg-surface px-2 py-0.5 text-xs text-text-mid placeholder:text-text-dim/30 focus:border-white/25 focus:outline-none"
              />
            </label>
          ))}
          {/* Cashback — tri-state select. */}
          <label className="flex items-center gap-2 text-xs text-text-mid">
            <span className="w-32 shrink-0 whitespace-nowrap">
              {GROUP_FIELD_LABELS['is_cashback_enabled']}
            </span>
            <select
              value={cashbackFilter}
              onChange={(e) => setCashbackFilter(e.target.value as CashbackFilter)}
              className="min-w-0 flex-1 rounded border border-white/10 bg-surface px-2 py-0.5 text-xs text-text-mid focus:border-white/25 focus:outline-none"
            >
              <option value="all">all</option>
              <option value="true">cashback only</option>
              <option value="false">no cashback</option>
            </select>
          </label>
        </div>
        {/* Instruction labels — exact JSON set. */}
        <label className="mt-1.5 flex flex-col gap-0.5 text-xs text-text-mid">
          <span className="whitespace-nowrap">
            {GROUP_FIELD_LABELS['ix_labels']} — exact set (JSON array)
          </span>
          <textarea
            value={ixLabelsText}
            onChange={(e) => setIxLabelsText(e.target.value)}
            placeholder='all sets — e.g. ["Pump.Fun: Create","System: Transfer"]'
            rows={2}
            className={cn(
              'w-full rounded border bg-surface px-2 py-1 font-mono text-[11px] text-text-mid placeholder:text-text-dim/30 focus:outline-none',
              ixFilter.error ? 'border-red focus:border-red' : 'border-white/10 focus:border-white/25',
            )}
          />
          {ixFilter.error && <span className="text-[10px] text-red">{ixFilter.error}</span>}
        </label>
      </div>

      {isError && (
        <p className="text-red">
          {apiErrorMessage(error, 'Failed to load grouped creation stats')}
        </p>
      )}

      {!applied ? (
        <p className="text-text-dim">Set your grouping and filters, then click Analyze.</p>
      ) : isFetching ? (
        <p className="text-text-dim">Loading…</p>
      ) : groups.length === 0 ? (
        <p className="text-text-dim">No tokens match this grouping in the window.</p>
      ) : (
        <>
          {/* Color legend — click an entry to isolate its line; click again to
              restore. Shared rank colors tie the trend lines to the cards. */}
          <div className="mb-2 flex flex-wrap gap-x-4 gap-y-1">
            {groups.map((g) => {
              const active = isolatedGroup === g.g;
              const dimmed = isolatedGroup != null && !active;
              return (
                <button
                  key={g.g}
                  type="button"
                  onClick={() => setIsolatedGroup((prev) => (prev === g.g ? null : g.g))}
                  title={active ? 'Click to show all lines' : 'Click to isolate this line'}
                  className={cn(
                    'flex items-center gap-1.5 rounded px-1 text-[11px] text-text-dim transition-opacity hover:bg-white/5',
                    dimmed && 'opacity-40',
                    active && 'bg-white/5',
                  )}
                >
                  <span
                    className="inline-block h-2.5 w-2.5 rounded-sm"
                    style={{ background: groupColor(g.g) }}
                  />
                  <GroupKeyInline group={g} />
                  <span className="text-text-dim/70">· {formatWithCommas(g.total)}</span>
                </button>
              );
            })}
          </div>

          {/* Multi-series calendar trend (when each group is active). */}
          <GroupedCreationTrendChart
            points={data?.points ?? []}
            groups={groups}
            isolatedGroup={isolatedGroup}
          />

          {/* Small-multiple day×hour heatmaps (recurring active hours per group). */}
          <div className="mt-4 grid gap-3 xl:grid-cols-2">
            {groups.map((g) => (
              <div key={g.g} className="rounded-md border border-white/8 bg-white/2 p-2.5">
                <div className="mb-1.5 flex items-start gap-2">
                  <span
                    className="mt-1 inline-block h-2.5 w-2.5 shrink-0 rounded-sm"
                    style={{ background: groupColor(g.g) }}
                  />
                  <div className="min-w-0 flex-1">
                    <GroupKeyBlock group={g} />
                  </div>
                  <span className="shrink-0 text-[11px] text-text-dim">
                    {formatWithCommas(g.total)} tokens
                  </span>
                </div>
                <CreationHeatmap
                  cells={cellsByGroup.get(g.g) ?? []}
                  metric="count"
                  total={g.total}
                />
              </div>
            ))}
          </div>
        </>
      )}
    </section>
  );
}

/** One-line group-key summary for the legend (field labels + values, ix_labels
 *  collapsed to a count). */
function GroupKeyInline({ group }: { group: GroupedCreationGroup }) {
  const entries = Object.entries(group.group_key);
  if (entries.length === 0) return <span className="text-text">ALL tokens</span>;
  return (
    <span className="text-text">
      {entries.map(([k, v], i) => (
        <Fragment key={k}>
          {i > 0 && <span className="text-text-dim"> · </span>}
          <span className="text-text-dim">{GROUP_FIELD_LABELS[k as GroupField] ?? k}=</span>
          {k === 'ix_labels' ? (
            <span className="font-mono">
              {v === MISSING_VALUE ? '∅' : `${groupValueParts(k, v).length} ix`}
            </span>
          ) : (
            <span className="font-mono">{v}</span>
          )}
        </Fragment>
      ))}
    </span>
  );
}

/** `ix_labels` rendered verbatim (on-chain order, NOT sorted) as a multi-line
 *  JSON array. Click to copy the exact JSON. */
function IxLabelsJson({ parts }: { parts: string[] }) {
  const [copied, setCopied] = useState(false);
  const json = useMemo(() => JSON.stringify(parts, null, 2), [parts]);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(json);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* ignore */
    }
  };

  return (
    <pre
      onClick={copy}
      title={copied ? 'Copied!' : 'Click to copy JSON'}
      className={cn(
        'm-0 cursor-pointer whitespace-pre font-mono text-[11px] leading-tight',
        copied ? 'text-primary' : 'text-secondary',
      )}
    >
      {json}
    </pre>
  );
}

/** Full group-key block for a heatmap card (label/value grid; ix_labels shown as
 *  copyable multi-line JSON in on-chain order). Mirrors the sweep page's
 *  group-chip layout. */
function GroupKeyBlock({ group }: { group: GroupedCreationGroup }) {
  const entries = Object.entries(group.group_key);
  if (entries.length === 0)
    return <span className="text-xs text-text-dim">ALL tokens</span>;
  return (
    <div className="grid grid-cols-[auto_1fr] items-start gap-x-2 gap-y-0.5 text-left">
      {entries.map(([k, v]) => {
        const label = GROUP_FIELD_LABELS[k as GroupField] ?? k;
        const isIx = k === 'ix_labels';
        const parts = isIx ? (v === MISSING_VALUE ? [] : groupValueParts(k, v)) : null;
        return (
          <Fragment key={k}>
            <span className="text-[11px] leading-tight text-text-dim" title={`${label}: ${v}`}>
              {label}:
            </span>
            {parts ? (
              <IxLabelsJson parts={parts} />
            ) : (
              <span className="font-mono text-[11px] text-secondary">{v}</span>
            )}
          </Fragment>
        );
      })}
    </div>
  );
}
