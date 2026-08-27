import { useMemo } from 'react';
import { Button } from 'components/ui/Button';
import { Checkbox } from 'components/ui/Checkbox';
import { IxLabelsInput } from 'components/ui/IxLabelsInput';
import { cn } from 'lib/cn';
import { compareBounds, lamportsToSolLabel, parseBound } from 'lib/strategy/fingerprintAxes';
import {
  GROUP_FIELDS,
  GROUP_FIELD_LABELS,
  LAMPORTS_GROUP_FIELDS,
  NUMERIC_GROUP_FIELDS,
  type GroupField,
  type PartitionSpec,
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
  /**
   * Clear every value filter in **one** parent update (numeric texts + cashback
   * + ix_labels). Prefer this over N× `onSetFieldFilter` — each call is a
   * separate setState / localStorage write on the host pages.
   */
  onClearFilters: () => void;
  cashbackFilter: CashbackFilter;
  onSetCashback: (v: CashbackFilter) => void;
  /** Bucket width (SOL) the continuous SOL fields (◎) are binned at — the one knob
   *  the partition, the promoted rule's matcher, and this dashboard all share. */
  /** Per-field partition, keyed by field tag. A field not named here is
   *  `{kind:'distinct'}` — one group per value, which is also the default. */
  partition: Record<string, PartitionSpec>;
  onSetPartition: (field: GroupField, spec: PartitionSpec) => void;
  /** Raw textarea text for the exact ix_labels set filter (pretty JSON array). */
  ixLabelsText: string;
  onSetIxLabels: (v: string) => void;
  /** Parsed ix filter ({ labels, error }) — caller owns the parse so both the
   *  picker (display) and the caller (submit) agree on the same result. */
  ixFilter: { labels: string[] | null; error: string | null };
  /** Hint shown when no grouping field is selected (the single "ALL" group). */
  emptyHint?: string;
  /**
   * Whole picker inert — Creation Stats when scoped by a fingerprint (manual
   * group-by / filters are dropped from the query entirely).
   */
  disabled?: boolean;
  /**
   * Value-filter controls inert, group-by still usable — Flow Discovery / Sweep
   * when scoped (engine match replaces filters; group-by can still split the
   * matched slice).
   */
  filtersDisabled?: boolean;
}

/** Fields that take a numeric comma-list filter (rendered in the 2-col grid).
 *  `is_cashback_enabled` (a select) and `ix_labels` (a label-set textarea)
 *  are special. */
const NUMERIC_FIELDS = GROUP_FIELDS.filter(
  (f) => f !== 'ix_labels' && f !== 'is_cashback_enabled',
);

/** Units note shown at the bottom of each numeric field's filter tooltip.
 *
 *  Grouping and filtering use DIFFERENT precisions on the ◎ SOL fields, which is
 *  the thing worth stating: the group key is a `width`-wide bucket *range*
 *  ("1.5–1.6"), but the value filter pins an **exact SOL amount** (1.515) on the
 *  underlying lamports — independent of the bucket width. Typing a range here
 *  matches nothing (the backend 400s on it). */
function fieldUnitHint(field: GroupField): string {
  if (!NUMERIC_GROUP_FIELDS.has(field)) return 'Comma-separated values.';
  const unit = LAMPORTS_GROUP_FIELDS.has(field) ? 'SOL amount' : 'integer';
  const ex = LAMPORTS_GROUP_FIELDS.has(field) ? ['1.515', '1.5-1.6', '>=1.5'] : ['3', '3-5', '>=3'];
  // One grammar for every numeric field — only what the digits MEAN differs, which
  // is a unit, not a mode. Independent of how the field is partitioned above.
  return `${unit}: "${ex[0]}" pins that value, "${ex[1]}" a half-open window, "${ex[2]}" an open-ended bound.`;
}

/** Tooltip for a numeric field's filter input — explains the 3-state interaction
 *  (neutral wording so it reads correctly on the sweep page and the dashboard). */
function fieldFilterTooltip(field: GroupField, isGrouped: boolean): string {
  const label = GROUP_FIELD_LABELS[field];
  const units = fieldUnitHint(field);
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
 * ix_labels group-by + exact-set JSON textarea. Grouping and
 * filtering are independent (a filter restricts the corpus whether or not the
 * field is grouped), except `ix_labels`: grouping by it disables its set filter
 * (mutually exclusive).
 *
 * Stateless — the caller owns all state; this only renders + emits changes. A
 * header row surfaces the active-filter summary + a Clear-filters action.
 */
/** A partition's edges as the operator types them, in the field's display unit.
 *  Blank for `distinct` — the default, and the one spelling of "not binned". */
function edgesText(spec: PartitionSpec | undefined, f: GroupField): string {
  if (!spec || spec.kind !== 'ranges') return '';
  return spec.edges
    .map((e) => (LAMPORTS_GROUP_FIELDS.has(f) ? lamportsToSolLabel(e) : e))
    .join(', ');
}

/** Typed edges into a `PartitionSpec`, in the field's display unit.
 *
 *  Sorted and de-duplicated here rather than rejected: the operator is listing
 *  split points, not an ordered protocol, and the backend requires them strictly
 *  ascending. An unparseable entry is dropped — the remaining edges still tile the
 *  whole range, so no token goes missing, unlike a dropped FILTER which widens the
 *  corpus. Nothing parseable at all ⇒ `distinct`. */
function parseEdges(text: string, f: GroupField): PartitionSpec {
  const unit = LAMPORTS_GROUP_FIELDS.has(f) ? 'lamports' : 'count';
  const parsed = text
    .split(',')
    .map((t) => parseBound(t, unit))
    .filter((v): v is string => v != null);
  const uniq = [...new Set(parsed)].sort(compareBounds);
  return uniq.length === 0 ? { kind: 'distinct' } : { kind: 'ranges', edges: uniq };
}

export function FingerprintGroupPicker({
  groupBy,
  onToggleField,
  fieldFiltersText,
  onSetFieldFilter,
  onClearFilters,
  cashbackFilter,
  onSetCashback,
  partition,
  onSetPartition,
  ixLabelsText,
  onSetIxLabels,
  ixFilter,
  emptyHint = 'No fields selected → one "ALL" group.',
  disabled = false,
  filtersDisabled = false,
}: FingerprintGroupPickerProps) {
  const ixLabelsGrouped = groupBy.includes('ix_labels');
  // Suppress the parse error while grouping by ix_labels (the filter is ignored).
  const ixFilterError = !ixLabelsGrouped ? ixFilter.error : null;
  const filtersLocked = disabled || filtersDisabled;
  const groupByLocked = disabled;

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

  return (
    <div
      className={cn('flex flex-col gap-2', disabled && 'opacity-50')}
      aria-disabled={disabled || undefined}
    >
      {(disabled || filtersDisabled) && (
        <p className="text-[11px] leading-snug text-text-dim/80">
          {disabled
            ? 'Scoped to a saved fingerprint — manual group-by / filters are ignored.'
            : 'Scoped to a saved fingerprint — value filters are not sent (group-by still splits the matched slice).'}
        </p>
      )}
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
          disabled={!hasFilters || filtersLocked}
          onClick={onClearFilters}
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
                  'flex w-36 shrink-0 items-center gap-1.5 text-sm',
                  groupByLocked ? 'cursor-not-allowed' : 'cursor-pointer',
                  isGrouped ? 'text-text-base' : 'text-text-mid',
                )}
                title={groupByCheckboxTooltip(f, isGrouped)}
              >
                <Checkbox
                  checked={isGrouped}
                  disabled={groupByLocked}
                  onChange={() => onToggleField(f)}
                />
                <span className="whitespace-nowrap">{GROUP_FIELD_LABELS[f]}</span>
              </label>
              <input
                type="text"
                value={filterText}
                disabled={filtersLocked}
                onChange={(e) => onSetFieldFilter(f, e.target.value)}
                placeholder={LAMPORTS_GROUP_FIELDS.has(f) ? '1.515 or 1.5–1.6' : 'all values'}
                title={fieldFilterTooltip(f, isGrouped)}
                className="min-w-0 flex-1 rounded border border-white/10 bg-surface px-2 py-0.5 text-xs text-text-mid placeholder:text-text-dim/30 focus:border-white/25 focus:outline-none disabled:cursor-not-allowed disabled:opacity-50"
              />
              {/* The edges box states how the field is PARTITIONED; the
                  pinned/filtered badge states that a value filter is active. They
                  are independent facts, so both show at once.

                  Edges, not a width: a width is an infinite implicit lattice every
                  consumer has to re-derive identically, and a `0` in it divides by
                  zero. A list is finite, explicit, and travels with the run. */}
              {NUMERIC_GROUP_FIELDS.has(f) && isGrouped && (
                <input
                  type="text"
                  value={edgesText(partition[f], f)}
                  disabled={groupByLocked}
                  onChange={(e) => onSetPartition(f, parseEdges(e.target.value, f))}
                  placeholder="one group per value"
                  title={
                    `Split ${GROUP_FIELD_LABELS[f]} at these edges` +
                    (LAMPORTS_GROUP_FIELDS.has(f) ? ' (SOL)' : '') +
                    ', e.g. "1, 2, 5".\n\n' +
                    'Edge i opens the window [edge i, edge i+1); below the first and above ' +
                    'the last are open-ended, so the edges tile the whole range and no token ' +
                    'is dropped.\n\n' +
                    'Blank = one group per distinct value. A group card carries the window it ' +
                    'selected, which IS the predicate a promoted rule matches on.'
                  }
                  className="w-28 shrink-0 rounded border border-white/10 bg-surface px-2 py-0.5 text-[11px] text-text-mid placeholder:text-text-dim/30 focus:border-white/25 focus:outline-none disabled:cursor-not-allowed disabled:opacity-50"
                />
              )}
              {hasFilter && (
                <span
                  className="shrink-0 text-[10px] text-text-dim/60"
                  title={
                    LAMPORTS_GROUP_FIELDS.has(f)
                      ? `Pinned to the exact SOL amount(s) "${filterText.trim()}"${isGrouped ? ' — grouped into buckets' : ''}`
                      : isGrouped
                        ? 'Groups restricted to these values'
                        : 'Corpus pinned to these values'
                  }
                >
                  {isGrouped ? 'filtered' : 'pinned'}
                </span>
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
                  'flex w-36 shrink-0 items-center gap-1.5 text-sm',
                  groupByLocked ? 'cursor-not-allowed' : 'cursor-pointer',
                  isGrouped ? 'text-text-base' : 'text-text-mid',
                )}
                title={groupByCheckboxTooltip(f, isGrouped)}
              >
                <Checkbox
                  checked={isGrouped}
                  disabled={groupByLocked}
                  onChange={() => onToggleField(f)}
                />
                <span className="whitespace-nowrap">{GROUP_FIELD_LABELS[f]}</span>
              </label>
              <select
                value={cashbackFilter}
                disabled={filtersLocked}
                onChange={(e) => onSetCashback(e.target.value as CashbackFilter)}
                title={
                  isGrouped
                    ? 'Filter by Cashback on value.\n\n☑ ON + cashback only → only cashback=true tokens, in their own group.\n☑ ON + no cashback  → only cashback=false tokens, in their own group.\n☑ ON + all          → all tokens, split into true/false groups (default).'
                    : 'Filter by Cashback on value.\n\n☐ OFF + cashback only → only cashback=true tokens, all in ONE group.\n☐ OFF + no cashback  → only cashback=false tokens, all in ONE group.\n☐ OFF + all          → no filter; all tokens pass through (default).'
                }
                className="w-36 rounded border border-white/10 bg-surface px-2 py-0.5 text-xs text-text-mid focus:border-white/25 focus:outline-none disabled:cursor-not-allowed disabled:opacity-50"
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
                    'flex w-36 shrink-0 items-center gap-1.5 text-sm',
                    groupByLocked ? 'cursor-not-allowed' : 'cursor-pointer',
                    isGrouped ? 'text-text-base' : 'text-text-mid',
                  )}
                  title={groupByCheckboxTooltip(f, isGrouped)}
                >
                  <Checkbox
                    checked={isGrouped}
                    disabled={groupByLocked}
                    onChange={() => onToggleField(f)}
                  />
                  <span className="whitespace-nowrap">{GROUP_FIELD_LABELS[f]}</span>
                </label>
                {!ixLabelsGrouped && ixFilter.labels && (
                  <span className="text-[10px] text-text-dim/60">
                    {ixFilter.labels.length} label{ixFilter.labels.length !== 1 ? 's' : ''} pinned
                  </span>
                )}
              </div>
              <div className="ml-5 flex flex-col gap-0.5">
                <IxLabelsInput
                  value={ixLabelsText}
                  onValueChange={onSetIxLabels}
                  disabled={ixLabelsGrouped || filtersLocked}
                  error={ixFilterError}
                  title={
                    ixLabelsGrouped
                      ? 'Disabled: grouping by instruction labels. Uncheck to pin a specific label set here.'
                      : filtersLocked
                        ? 'Disabled while scoped to a saved fingerprint.'
                        : 'Filter to tokens whose instruction-label sequence exactly matches this JSON array — same order, same repeats, same length (what a rule matches on).\nLeave empty = all label sets included.'
                  }
                />
              </div>
            </div>
          );
        })()}
      </div>


      {/* Legend — how partitioning and filtering differ, so the two boxes on each
          row are self-explaining. */}
      <p className="text-[11px] leading-snug text-text-dim/70">
        A grouped numeric field splits into <b>one group per distinct value</b> unless you
        give it <b>edges</b> (e.g. <span className="font-mono">1, 2, 5</span>) — then each
        group is the window between two edges, open-ended below the first and above the
        last, so every token lands in exactly one. A group card carries the window it
        selected, which is <b>the same predicate</b> a promoted rule matches on.
        {' '}The filter box is separate and always takes one grammar:{' '}
        <span className="font-mono">1.515</span> pins a value,{' '}
        <span className="font-mono">1.5-1.6</span> a window,{' '}
        <span className="font-mono">&gt;=1.5</span> a bound.
      </p>

      {groupBy.length === 0 && (
        <p className="text-xs text-text-dim/70">{emptyHint}</p>
      )}
    </div>
  );
}
