import { Fragment, useMemo, useState } from 'react';
import { Button } from 'components/ui/Button';
import { Select } from 'components/ui/Select';
import { useGetGroupedCreationStatsQuery, apiErrorMessage } from 'store/apiSlice';
import { formatWithCommas } from 'utils/format';
import { cn } from 'lib/cn';
import { CreationHeatmap } from './CreationHeatmap';
import { GroupedCreationTrendChart } from './GroupedCreationTrendChart';
import type { CreationHeatCell, CreationSegment } from './creationStats';
import {
  GROUP_FIELDS,
  GROUP_FIELD_LABELS,
  TOP_OPTIONS,
  MISSING_VALUE,
  groupColor,
  groupValueParts,
  type GroupedCreationCell,
  type GroupedCreationGroup,
  type GroupField,
} from './groupedCreationStats';

interface GroupedCreationSectionProps {
  /** RFC3339 window lower bound (shared with the page's range control). */
  from: string;
  tz: string;
  segment: CreationSegment;
}

/** Default grouping — CU limit + instruction-label set: the two fields that
 *  separate the bulk of launch fingerprints (validated against live data). */
const DEFAULT_GROUP_BY: GroupField[] = ['cu_limit', 'ix_labels'];

/** A `GroupedCreationCell` lacks the outcome fields `CreationHeatmap` reads; the
 *  count view never touches them, so zero-fill is safe (count = volume). */
function toHeatCell(c: GroupedCreationCell): CreationHeatCell {
  return { dow: c.dow, hour: c.hour, count: c.count, matured: 0, known: 0, migrated: 0, dead: 0 };
}

/**
 * Dashboard section: partition token creation by a fingerprint key and show each
 * group's recurring time-of-day bias (small-multiple day×hour heatmaps) plus its
 * calendar trend (multi-series line chart). Reuses the page's window / timezone /
 * segment; manages its own group-by + top-N selection. Count only.
 */
export function GroupedCreationSection({ from, tz, segment }: GroupedCreationSectionProps) {
  // Click order = compound-key order (matches the sweep page semantics).
  const [groupBy, setGroupBy] = useState<GroupField[]>(DEFAULT_GROUP_BY);
  const [top, setTop] = useState(8);

  const { data, isLoading, isError, error } = useGetGroupedCreationStatsQuery({
    bucket: 'day',
    tz,
    from,
    segment,
    groupBy,
    top,
  });

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
            when each fingerprint group launches — pick fields to group by
          </span>
        </div>
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
      </div>

      {/* Group-by picker — order of selection is the compound-key order. */}
      <div className="mb-3 flex flex-wrap items-center gap-1">
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

      {isError && (
        <p className="text-red">
          {apiErrorMessage(error, 'Failed to load grouped creation stats')}
        </p>
      )}

      {isLoading ? (
        <p className="text-text-dim">Loading…</p>
      ) : groups.length === 0 ? (
        <p className="text-text-dim">No tokens created in this window.</p>
      ) : (
        <>
          {/* Color legend — shared rank colors tie the trend lines to the cards. */}
          <div className="mb-2 flex flex-wrap gap-x-4 gap-y-1">
            {groups.map((g) => (
              <span key={g.g} className="flex items-center gap-1.5 text-[11px] text-text-dim">
                <span
                  className="inline-block h-2.5 w-2.5 rounded-sm"
                  style={{ background: groupColor(g.g) }}
                />
                <GroupKeyInline group={g} />
                <span className="text-text-dim/70">· {formatWithCommas(g.total)}</span>
              </span>
            ))}
          </div>

          {/* Multi-series calendar trend (when each group is active). */}
          <GroupedCreationTrendChart points={data?.points ?? []} groups={groups} />

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
