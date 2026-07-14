import { useEffect, useMemo, useRef } from 'react';
import { Button } from 'components/ui/Button';
import { Checkbox } from 'components/ui/Checkbox';
import { BucketChip } from 'components/ui/BucketChip';
import { cn } from 'lib/cn';
import {
  GROUP_FIELDS,
  GROUP_FIELD_LABELS,
  BUCKETED_GROUP_FIELDS,
  SOL_BUCKET_WIDTH,
  type GroupField,
} from './groupedTypes';

/** Tri-state corpus filter on the `is_cashback_enabled` flag. */
export type CashbackFilter = 'all' | 'true' | 'false';

interface FingerprintGroupPickerProps {
  /** Selected grouping fields, in compound-key (selection) order. */
  groupBy: GroupField[];
  onToggleField: (f: GroupField) => void;
  /** Per-(numeric)field comma-separated value-filter text. */
  fieldFiltersText: Record<string, string>;
  onSetFieldFilter: (field: string, value: string) => void;
  cashbackFilter: CashbackFilter;
  onSetCashback: (v: CashbackFilter) => void;
  /** Raw JSON-array text for the exact ix_labels set filter. */
  ixLabelsText: string;
  onSetIxLabels: (v: string) => void;
  /** Parsed ix filter ({ labels, error }) — caller owns the parse so both the
   *  picker (display) and the caller (submit) agree on the same result. */
  ixFilter: { labels: string[] | null; error: string | null };
  /** Hint shown when no grouping field is selected (the single "ALL" group). */
  emptyHint?: string;
}

/** Fields that take a numeric comma-list filter (rendered in the 2-col grid).
 *  `is_cashback_enabled` (a select) and `ix_labels` (a JSON set) are special. */
const NUMERIC_FIELDS = GROUP_FIELDS.filter(
  (f) => f !== 'ix_labels' && f !== 'is_cashback_enabled',
);

/** Units note shown at the bottom of each numeric field's filter tooltip. */
// Discrete fields group on their exact value; the continuous SOL amounts are
// grouped into fixed 0.1-SOL buckets (chips read as ranges like "1.0–1.1"), so the
// value filter here is best used to pin a bucket rather than an exact amount.
const FIELD_UNIT_HINTS: Partial<Record<GroupField, string>> = {
  cu_limit: 'Raw integer (e.g. 200000), exact grouping. Match values shown in group keys.',
  cu_price: 'Raw integer (e.g. 1000), exact grouping. Match values shown in group keys.',
  max_cost_lamports: 'Grouped into 0.1-SOL buckets (chips read as ranges, e.g. 1.0–1.1).',
  spendable_lamports_in: 'Grouped into 0.1-SOL buckets (chips read as ranges, e.g. 1.0–1.1).',
  initial_buy_sol: 'Grouped into 0.1-SOL buckets (chips read as ranges, e.g. 1.0–1.1).',
  first_slot_buy_sol: 'Creation-slot buy SOL, grouped into 0.1-SOL buckets (ranges, e.g. 1.0–1.1).',
  first_slot_sell_sol: 'Creation-slot sell SOL, grouped into 0.1-SOL buckets (ranges, e.g. 1.0–1.1).',
};

/** Tooltip for a numeric field's filter input — explains the 3-state interaction
 *  (neutral wording so it reads correctly on the sweep page and the dashboard). */
function fieldFilterTooltip(field: GroupField, isGrouped: boolean): string {
  const label = GROUP_FIELD_LABELS[field];
  const units = FIELD_UNIT_HINTS[field] ?? 'Comma-separated numbers.';
  const whenOn = isGrouped
    ? '☑ ON  + values here → only tokens matching a value are kept,\n        then split into one group PER value (narrowed grouping).\n☑ ON  + empty       → all values included, each in its own group (default).'
    : '☐ OFF + values here → only tokens matching a value are kept,\n        all combined into ONE group.\n☐ OFF + empty       → no filter; all values pass through (default).';
  return [
    `Filter to specific ${label} values (comma-separated numbers).`,
    `Leave empty = all values pass through (no filter on this field).`,
    ``,
    whenOn,
    ``,
    `Units: ${units}`,
  ].join('\n');
}

/** Tooltip for a group-by checkbox. */
function groupByCheckboxTooltip(field: GroupField, isGrouped: boolean): string {
  const label = GROUP_FIELD_LABELS[field];
  if (isGrouped) {
    return `Grouping by ${label}.\nTokens are split into one group per distinct value.\n\nUncheck to stop splitting by this field.`;
  }
  return `Click to group by ${label}.\nChecked → tokens split into separate groups (one per distinct value).\nUnchecked → this field is ignored for grouping (all values mixed together).\n\nYou can still filter to specific values using the input on the right.`;
}

/** A rank badge (1-based compound-key position) shown when a field is grouped. */
function RankBadge({ index }: { index: number }) {
  return (
    <span className="flex h-4 w-4 shrink-0 items-center justify-center">
      {index >= 0 && (
        <span className="flex h-4 w-4 items-center justify-center rounded-full bg-accent/20 text-[9px] font-bold leading-none text-accent">
          {index + 1}
        </span>
      )}
    </span>
  );
}

/**
 * Shared fingerprint **group-by + value-filter** control, used by the TPSL2/TPSL1
 * sweep config form and the dashboard's "Creation by token group" section so both
 * read identically. Renders: a 2-col grid of numeric fields (rank badge · group-by
 * checkbox · label · comma-list filter), the cashback tri-state select, and the
 * ix_labels group-by + exact-set JSON textarea. Grouping and filtering are
 * independent (a filter restricts the corpus whether or not the field is grouped),
 * except `ix_labels`: grouping by it disables its set filter (mutually exclusive).
 *
 * Stateless — the caller owns all state; this only renders + emits changes. A
 * header row surfaces the active-filter summary + a Clear-filters action.
 */
export function FingerprintGroupPicker({
  groupBy,
  onToggleField,
  fieldFiltersText,
  onSetFieldFilter,
  cashbackFilter,
  onSetCashback,
  ixLabelsText,
  onSetIxLabels,
  ixFilter,
  emptyHint = 'No fields selected → one "ALL" group.',
}: FingerprintGroupPickerProps) {
  const ixLabelsGrouped = groupBy.includes('ix_labels');
  // Suppress the parse error while grouping by ix_labels (the filter is ignored).
  const ixFilterError = !ixLabelsGrouped ? ixFilter.error : null;

  // Auto-grow the ix_labels textarea to fit its content.
  const ixLabelsRef = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    const el = ixLabelsRef.current;
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = `${el.scrollHeight}px`;
  }, [ixLabelsText]);

  // Active value-filter summary (independent of grouping). Shown in the header so
  // pinned constraints are visible at a glance.
  const summary = useMemo(() => {
    const parts: string[] = [];
    for (const f of NUMERIC_FIELDS) {
      const v = (fieldFiltersText[f] ?? '').trim();
      if (v) parts.push(`${GROUP_FIELD_LABELS[f]}=${v}`);
    }
    if (cashbackFilter === 'true') parts.push('Cashback: on');
    else if (cashbackFilter === 'false') parts.push('Cashback: off');
    if (!ixLabelsGrouped && ixFilter.labels) {
      const n = ixFilter.labels.length;
      parts.push(`${n} ix label${n !== 1 ? 's' : ''}`);
    }
    return parts;
  }, [fieldFiltersText, cashbackFilter, ixLabelsGrouped, ixFilter.labels]);

  const hasFilters =
    NUMERIC_FIELDS.some((f) => (fieldFiltersText[f] ?? '').trim() !== '') ||
    cashbackFilter !== 'all' ||
    ixLabelsText.trim() !== '';

  const clearFilters = () => {
    for (const f of NUMERIC_FIELDS) {
      if ((fieldFiltersText[f] ?? '') !== '') onSetFieldFilter(f, '');
    }
    if (cashbackFilter !== 'all') onSetCashback('all');
    if (ixLabelsText !== '') onSetIxLabels('');
  };

  return (
    <div className="flex flex-col gap-2">
      {/* Header — active-filter summary + clear. */}
      <div className="flex min-h-[1.25rem] items-start justify-between gap-2">
        <div className="min-w-0 flex-1 text-[11px] leading-tight">
          {summary.length > 0 ? (
            <span className="text-text-mid">
              <span className="text-text-dim">Filters: </span>
              {summary.map((s, i) => (
                <span key={s}>
                  {i > 0 && <span className="text-text-dim/50"> · </span>}
                  <span className="font-mono">{s}</span>
                </span>
              ))}
            </span>
          ) : (
            <span className="text-text-dim/60">No value filters — all tokens included.</span>
          )}
        </div>
        <Button
          size="sm"
          variant="subtle"
          disabled={!hasFilters}
          onClick={clearFilters}
          title="Clear every value filter (keeps your group-by selection)"
        >
          Clear filters
        </Button>
      </div>

      {/* Numeric fields — 2-col grid, each row: badge · checkbox · label · filter. */}
      <div className="grid grid-cols-1 gap-x-4 gap-y-1 sm:grid-cols-2">
        {NUMERIC_FIELDS.map((f) => {
          const isGrouped = groupBy.includes(f);
          const groupIndex = groupBy.indexOf(f);
          const filterText = fieldFiltersText[f] ?? '';
          const hasFilter = filterText.trim() !== '';
          return (
            <div key={f} className="flex min-w-0 items-center gap-1.5">
              <RankBadge index={isGrouped ? groupIndex : -1} />
              <label
                className={cn(
                  'flex w-36 shrink-0 cursor-pointer items-center gap-1.5 text-sm',
                  isGrouped ? 'text-text-base' : 'text-text-mid',
                )}
                title={groupByCheckboxTooltip(f, isGrouped)}
              >
                <Checkbox checked={isGrouped} onChange={() => onToggleField(f)} />
                <span className="whitespace-nowrap">{GROUP_FIELD_LABELS[f]}</span>
              </label>
              <input
                type="text"
                value={filterText}
                onChange={(e) => onSetFieldFilter(f, e.target.value)}
                placeholder="all values"
                title={fieldFilterTooltip(f, isGrouped)}
                className="min-w-0 flex-1 rounded border border-white/10 bg-surface px-2 py-0.5 text-xs text-text-mid placeholder:text-text-dim/30 focus:border-white/25 focus:outline-none"
              />
              {hasFilter ? (
                <span
                  className="shrink-0 text-[10px] text-text-dim/60"
                  title={isGrouped ? 'Groups restricted to these values' : 'Corpus pinned to these values'}
                >
                  {isGrouped ? 'filtered' : 'pinned'}
                </span>
              ) : (
                BUCKETED_GROUP_FIELDS.has(f) && (
                  <BucketChip
                    width={SOL_BUCKET_WIDTH}
                    title={`Continuous SOL amount — grouped into ${SOL_BUCKET_WIDTH}-SOL buckets. Group chips read as ranges (e.g. "1.0–1.1"), not exact values.`}
                  />
                )
              )}
            </div>
          );
        })}
      </div>

      {/* Enum + set fields — full-width rows below the numeric grid. */}
      <div className="flex flex-col gap-1.5">
        {/* Cashback — checkbox + tri-state select. */}
        {(() => {
          const f = 'is_cashback_enabled' as const;
          const isGrouped = groupBy.includes(f);
          const groupIndex = groupBy.indexOf(f);
          return (
            <div className="flex min-w-0 items-center gap-1.5">
              <RankBadge index={isGrouped ? groupIndex : -1} />
              <label
                className={cn(
                  'flex w-36 shrink-0 cursor-pointer items-center gap-1.5 text-sm',
                  isGrouped ? 'text-text-base' : 'text-text-mid',
                )}
                title={groupByCheckboxTooltip(f, isGrouped)}
              >
                <Checkbox checked={isGrouped} onChange={() => onToggleField(f)} />
                <span className="whitespace-nowrap">{GROUP_FIELD_LABELS[f]}</span>
              </label>
              <select
                value={cashbackFilter}
                onChange={(e) => onSetCashback(e.target.value as CashbackFilter)}
                title={
                  isGrouped
                    ? 'Filter by Cashback on value.\n\n☑ ON + cashback only → only cashback=true tokens, in their own group.\n☑ ON + no cashback  → only cashback=false tokens, in their own group.\n☑ ON + all          → all tokens, split into true/false groups (default).'
                    : 'Filter by Cashback on value.\n\n☐ OFF + cashback only → only cashback=true tokens, all in ONE group.\n☐ OFF + no cashback  → only cashback=false tokens, all in ONE group.\n☐ OFF + all          → no filter; all tokens pass through (default).'
                }
                className="w-36 rounded border border-white/10 bg-surface px-2 py-0.5 text-xs text-text-mid focus:border-white/25 focus:outline-none"
              >
                <option value="all">all</option>
                <option value="true">cashback only</option>
                <option value="false">no cashback</option>
              </select>
            </div>
          );
        })()}

        {/* Instruction labels — checkbox row, then the exact-set textarea below. */}
        {(() => {
          const f = 'ix_labels' as const;
          const isGrouped = groupBy.includes(f);
          const groupIndex = groupBy.indexOf(f);
          return (
            <div className="flex flex-col gap-0.5">
              <div className="flex min-w-0 items-center gap-1.5">
                <RankBadge index={isGrouped ? groupIndex : -1} />
                <label
                  className={cn(
                    'flex w-36 shrink-0 cursor-pointer items-center gap-1.5 text-sm',
                    isGrouped ? 'text-text-base' : 'text-text-mid',
                  )}
                  title={groupByCheckboxTooltip(f, isGrouped)}
                >
                  <Checkbox checked={isGrouped} onChange={() => onToggleField(f)} />
                  <span className="whitespace-nowrap">{GROUP_FIELD_LABELS[f]}</span>
                </label>
                {!ixLabelsGrouped && ixFilter.labels && (
                  <span className="text-[10px] text-text-dim/60">
                    {ixFilter.labels.length} label{ixFilter.labels.length !== 1 ? 's' : ''} pinned
                  </span>
                )}
              </div>
              <div className="ml-5 flex flex-col gap-0.5">
                <textarea
                  ref={ixLabelsRef}
                  rows={1}
                  value={ixLabelsText}
                  disabled={ixLabelsGrouped}
                  onChange={(e) => onSetIxLabels(e.target.value)}
                  placeholder='["Pump.Fun: Create"]'
                  title={
                    ixLabelsGrouped
                      ? 'Disabled: grouping by instruction labels. Uncheck to pin a specific label set here.'
                      : 'Filter to tokens whose instruction-label set exactly matches this JSON array (order-independent).\nLeave empty = all label sets included.'
                  }
                  className="w-full resize-none overflow-hidden rounded border border-white/10 bg-surface px-2 py-1 font-mono text-xs text-text-mid placeholder:text-text-dim/30 focus:border-white/25 focus:outline-none disabled:cursor-not-allowed disabled:opacity-40"
                />
                {ixFilterError && <span className="text-[10px] text-danger">{ixFilterError}</span>}
              </div>
            </div>
          );
        })()}
      </div>

      {/* Legend — which fields bucket and how (so grouping behavior is self-explaining). */}
      <p className="border-t border-white/5 pt-1.5 text-[11px] leading-snug text-text-dim/70">
        <BucketChip width={SOL_BUCKET_WIDTH} className="align-middle" />{' '}
        <span className="text-text-mid">
          {[...BUCKETED_GROUP_FIELDS].map((f) => GROUP_FIELD_LABELS[f]).join(', ')}
        </span>{' '}
        are continuous SOL amounts, so they group by <b>{SOL_BUCKET_WIDTH}-SOL ranges</b> —
        group chips read as{' '}
        <span className="font-mono">1.0–1.1</span>, not exact values. Every other field
        groups on its <b>exact</b> value.
      </p>

      {groupBy.length === 0 && (
        <p className="text-xs text-text-dim/70">{emptyHint}</p>
      )}
    </div>
  );
}
