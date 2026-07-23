import { Fragment, lazy, Suspense, useEffect, useMemo, useRef, useState } from 'react';
import { useLocalStorage } from 'hooks/useLocalStorage';
import { STORAGE_KEYS } from 'lib/storage';
import { skipToken } from '@reduxjs/toolkit/query/react';
import { Button } from 'components/ui/Button';
import { Select } from 'components/ui/Select';
import { IxLabelsDisplay } from 'components/ui/IxLabelsDisplay';
import { LoadingState } from 'components/ui/LoadingState';
import { apiErrorMessage } from 'store/apiSlice';
import { TokenTable } from 'components/tokens/TokenTable';
import { ALL_TOKEN_INFO_KEYS } from 'components/tokens/sharedTokenColumns';
import { tokenColumns } from 'components/tokens/tokenColumns';
import { inspectFromMint } from 'components/strategy/inspectTarget';
import type { TableQuery } from 'components/table/types';
import type { TokenRecord } from 'types';
import { LazyLabTokenInspectModal } from '@lab/components/strategy/LazyLabTokenInspectModal';
import {
  useGetGroupedCreationStatsQuery,
  useGetGroupedCreationTokensQuery,
} from '@lab/store/labEndpoints';
import { parseNumbers, parseIxLabelsFilter } from '@lab/components/sweep/fingerprintFilters';
import {
  FingerprintGroupPicker,
  type CashbackFilter,
} from '@lab/components/sweep/FingerprintGroupPicker';
import { SOL_BUCKET_WIDTH } from '@lab/components/sweep/groupedTypes';
import { formatWithCommas } from 'utils/format';
import { cn } from 'lib/cn';
import { CreationHeatmap } from 'components/creation-stats/CreationHeatmap';
import { DOW_ROWS } from 'components/creation-stats/creationStats';
import { FingerprintScopeControl } from 'components/strategy/FingerprintScopeControl';
import { CREATION_FIELD_HELP } from 'lib/strategy/strategyHelp';
import { useGetFingerprintsQuery } from 'store/sharedEndpoints';

const GroupedCreationTrendChart = lazy(() =>
  import('./GroupedCreationTrendChart').then((m) => ({ default: m.GroupedCreationTrendChart })),
);
import {
  RANGE_OPTIONS,
  bucketOptionsForRange,
  clampBucketToRange,
  windowFrom,
  type CreationBucket,
  type CreationHeatCell,
  type CreationSegment,
} from 'components/creation-stats/creationStats';
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
  type GroupedCreationTokensArgs,
  type GroupField,
} from './groupedCreationStats';

interface GroupedCreationSectionProps {
  tz: string;
  segment: CreationSegment;
}

/** Default grouping — CU limit + instruction-label set: the two fields that
 *  separate the bulk of launch fingerprints (validated against live data). */
const DEFAULT_GROUP_BY: GroupField[] = ['cu_limit', 'cu_price', 'ix_labels'];

/** Scalar fingerprint fields that take a comma-separated value filter (numeric).
 *  `is_cashback_enabled` (a tri-state select) and `ix_labels` (label-set textarea)
 *  are special — handled separately in `FingerprintGroupPicker`. */
const SCALAR_FILTER_FIELDS: GroupField[] = [
  'cu_limit',
  'cu_price',
  'max_cost_lamports',
  'spendable_lamports_in',
  'initial_buy_sol',
  'first_slot_buy_sol',
  'first_slot_sell_sol',
];

/** A `GroupedCreationCell` lacks the outcome fields `CreationHeatmap` reads; the
 *  count view never touches them, so zero-fill is safe (count = volume). */
function toHeatCell(c: GroupedCreationCell): CreationHeatCell {
  return { dow: c.dow, hour: c.hour, count: c.count, matured: 0, known: 0, migrated: 0, dead: 0 };
}

/** What the shared drill-down section is currently showing: one group card
 *  (`dow`/`hour` both `null`), or one of its heatmap tiles (both set). */
interface DrillTarget {
  g: number;
  dow: number | null;
  hour: number | null;
}

/** Short label for a recurring weekly slot, e.g. "Mon 15:00 (every week)". */
function dowLabel(dow: number): string {
  return DOW_ROWS.find((r) => r.dow === dow)?.label ?? String(dow);
}

/** Stable empty reference so the drill-down table doesn't remount on every
 *  render while its query is loading. */
const EMPTY_DRILL_TOKENS: TokenRecord[] = [];

const INITIAL_DRILL_QUERY: TableQuery = {
  page: 1,
  pageSize: 10,
  sortKeys: [],
  search: '',
  colFilters: {},
};

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
export function GroupedCreationSection({ tz, segment }: GroupedCreationSectionProps) {
  // --- draft controls (don't fetch until applied) ---------------------------
  // Click order = compound-key order (matches the sweep page semantics).
  const [rawGroupBy, setGroupBy] = useLocalStorage<GroupField[]>(STORAGE_KEYS.groupedBy, DEFAULT_GROUP_BY);
  // Sanitize against the current `GROUP_FIELDS` — a stale entry from before a
  // backend field rename (or a removed field like the old `creator_wallet`) is
  // invisible in the picker (which only renders known fields) and unremovable
  // by the user, and the backend's `parse_group_by` hard-rejects it forever.
  const groupBy = useMemo(
    () => rawGroupBy.filter((f) => (GROUP_FIELDS as readonly string[]).includes(f)),
    [rawGroupBy],
  );
  const [top, setTop] = useLocalStorage<number>(STORAGE_KEYS.groupedTop, 8);
  // Bucket width (SOL) for the continuous SOL group fields — the same knob the
  // grouped sweep uses, so this dashboard groups a corpus identically to a sweep.
  const [bucketWidth, setBucketWidth] = useLocalStorage<number>(STORAGE_KEYS.groupedBucketWidth, 0.1);
  const [bucket, setBucket] = useLocalStorage<CreationBucket>(STORAGE_KEYS.groupedBucket, 'day');
  const [rangeDays, setRangeDays] = useLocalStorage<number>(STORAGE_KEYS.groupedRange, 30);
  const [fieldFiltersText, setFieldFiltersText] = useLocalStorage<Record<string, string>>(STORAGE_KEYS.groupedFilters, {});
  const [cashbackFilter, setCashbackFilter] = useLocalStorage<CashbackFilter>(STORAGE_KEYS.groupedCashback, 'all');
  const [ixLabelsText, setIxLabelsText] = useLocalStorage<string>(STORAGE_KEYS.groupedIxLabels, '');
  // Saved-fingerprint scope — same "ALL group over the engine-matched tokens"
  // contract as the sweep's/flow discovery's seed select. Set ⇒ the manual
  // group-by/filters above are ignored (both client-side and server-side).
  const [seedFingerprintId, setSeedFingerprintId] = useLocalStorage<string | null>(
    STORAGE_KEYS.groupedFingerprintId,
    null,
  );
  const { data: fingerprints = [] } = useGetFingerprintsQuery();
  function selectSeedFingerprint(id: string) {
    setSeedFingerprintId(id || null);
  }

  // --- applied snapshot (drives the query) ----------------------------------
  const [applied, setApplied] = useState<GroupedCreationArgs | null>(null);
  const [isolatedGroup, setIsolatedGroup] = useState<number | null>(null);

  // --- drill-down tokens table (one shared section, shows whichever group
  // card or heatmap tile was last picked) ------------------------------------
  const [drillTarget, setDrillTarget] = useState<DrillTarget | null>(null);
  const [drillQuery, setDrillQuery] = useState<TableQuery>(INITIAL_DRILL_QUERY);
  // Columns built once — same recipe as the Tokens page, so the drill-down
  // table renders with identical columns/enrichment.
  const drillColumns = useMemo(() => tokenColumns(), []);
  // Row click opens the shared detail modal (chart + stats) — the drill-down
  // table sits nested inside an already-scrolling dashboard card, so an
  // inline panel (as the Tokens page uses) has nowhere good to land.
  const [inspectedToken, setInspectedToken] = useState<{ mint: string; symbol?: string } | null>(null);

  const from = useMemo(() => windowFrom(rangeDays), [rangeDays]);
  const bucketOpts = useMemo(() => bucketOptionsForRange(rangeDays), [rangeDays]);
  const effBucket = clampBucketToRange(bucket, rangeDays);

  // Parse the ix_labels textarea once; an active parse error blocks Analyze.
  const ixFilter = useMemo(() => parseIxLabelsFilter(ixLabelsText), [ixLabelsText]);

  // Build the query args from the current draft + page props (the thing Analyze
  // snapshots). Memoized so dirty-checking + click are cheap. Scoped by a saved
  // fingerprint ⇒ the manual group-by/filters below are dropped entirely (the
  // backend ignores them too — see `getGroupedCreationStats`'s query builder).
  const draftArgs = useMemo<GroupedCreationArgs>(() => {
    if (seedFingerprintId) {
      return {
        bucket: effBucket,
        tz,
        from,
        segment,
        groupBy: [],
        top,
        fingerprintId: seedFingerprintId,
      };
    }

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
      // Send only a non-default width so the 0.1 case keeps a stable cache key.
      ...(bucketWidth !== SOL_BUCKET_WIDTH ? { bucketWidth } : {}),
      ...(Object.keys(fieldFilters).length > 0 ? { fieldFilters } : {}),
      // ix_labels grouping and the exact-set filter are mutually exclusive
      // (matches the sweep page): drop the filter when grouping by ix_labels.
      ...(!groupBy.includes('ix_labels') && ixFilter.labels
        ? { ixLabelsFilter: ixFilter.labels }
        : {}),
    };
  }, [
    effBucket,
    tz,
    from,
    segment,
    groupBy,
    top,
    bucketWidth,
    fieldFiltersText,
    cashbackFilter,
    ixFilter.labels,
    seedFingerprintId,
  ]);

  const { data, isFetching, isError, error } = useGetGroupedCreationStatsQuery(applied ?? skipToken);

  // Reset line isolation + the drill-down selection whenever a fresh analysis
  // is applied — a stale `group_key`/index from the previous grouping would
  // silently target the wrong (or a nonexistent) group otherwise.
  useEffect(() => {
    setIsolatedGroup(null);
    setDrillTarget(null);
  }, [applied]);

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
  const drillGroup = drillTarget ? groups.find((g) => g.g === drillTarget.g) ?? null : null;

  // Resets the drill-down table to page 1 when the target (group/tile) changes.
  const drillResetKey = drillTarget
    ? `g=${drillTarget.g}:dow=${drillTarget.dow ?? ''}:hour=${drillTarget.hour ?? ''}`
    : '';

  const drillArgs = useMemo<GroupedCreationTokensArgs | null>(() => {
    if (!applied || !drillTarget || !drillGroup) return null;
    return {
      tz: applied.tz,
      from: applied.from,
      segment: applied.segment,
      groupBy: applied.groupBy,
      bucketWidth: applied.bucketWidth,
      fieldFilters: applied.fieldFilters,
      ixLabelsFilter: applied.ixLabelsFilter,
      fingerprintId: applied.fingerprintId,
      groupKey: drillGroup.group_key,
      ...(drillTarget.dow != null && drillTarget.hour != null
        ? { dow: drillTarget.dow, hour: drillTarget.hour }
        : {}),
      page: drillQuery.page,
      pageSize: drillQuery.pageSize,
      sortKeys: drillQuery.sortKeys,
      search: drillQuery.search,
    };
  }, [applied, drillTarget, drillGroup, drillQuery]);

  const {
    data: drillData,
    isFetching: drillLoading,
    isError: drillIsError,
    error: drillErrorRaw,
  } = useGetGroupedCreationTokensQuery(drillArgs ?? skipToken);
  // Hold the last successful page while a new one loads (target/page/sort
  // change) so the table doesn't flash empty between round-trips.
  const drillItemsRef = useRef<TokenRecord[]>(EMPTY_DRILL_TOKENS);
  if (drillData?.items) drillItemsRef.current = drillData.items;
  const drillTokens = drillArgs ? drillData?.items ?? drillItemsRef.current : EMPTY_DRILL_TOKENS;
  const drillTotal = drillArgs ? drillData?.total ?? 0 : 0;

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
          {/* Look-back window (section-local, independent of page range). */}
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

      {/* Saved-fingerprint scope — the engine-match path (same control the
          sweep/flow discovery pages use). Set → the dashboard shows a single
          "ALL" group over the fingerprint's matched tokens and the manual
          group-by / filters below are ignored; empty → manual selects the corpus. */}
      <FingerprintScopeControl
        fingerprints={fingerprints}
        value={seedFingerprintId}
        onChange={selectSeedFingerprint}
        tip={CREATION_FIELD_HELP.seedFingerprint}
        scopedDescription="Only tokens this fingerprint matches are shown (exact axes exact, SOL axes by bucket) — the manual group-by / filters below are ignored; the dashboard shows a single ALL group for the matched tokens."
        manualHint="Pick a fingerprint to see exactly the tokens it matches — or leave empty and partition the corpus with the manual group-by / filters below."
      />

      {/* Group-by + value filters — shared with the sweep page's fingerprint
          control so both read identically. */}
      <div className="mb-3 rounded-md border border-white/8 bg-white/2 p-2.5">
        <FingerprintGroupPicker
          groupBy={groupBy}
          onToggleField={toggleField}
          fieldFiltersText={fieldFiltersText}
          onSetFieldFilter={(f, v) => setFieldFiltersText((prev) => ({ ...prev, [f]: v }))}
          cashbackFilter={cashbackFilter}
          onSetCashback={setCashbackFilter}
          bucketWidthSol={bucketWidth}
          onSetBucketWidth={setBucketWidth}
          ixLabelsText={ixLabelsText}
          onSetIxLabels={setIxLabelsText}
          ixFilter={ixFilter}
          emptyHint={
            seedFingerprintId
              ? 'Scoped to the saved fingerprint → one "ALL" group of matching tokens.'
              : 'No fields selected → one "ALL" group (every token in the window).'
          }
        />
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
          <Suspense
            fallback={<LoadingState variant="inline" label="Loading chart…" />}
          >
            <GroupedCreationTrendChart
              points={data?.points ?? []}
              groups={groups}
              isolatedGroup={isolatedGroup}
            />
          </Suspense>

          {/* Small-multiple day×hour heatmaps (recurring active hours per group). */}
          <div className="mt-4 grid gap-3 xl:grid-cols-2">
            {groups.map((g) => {
              const targetingThisGroup = drillTarget?.g === g.g;
              return (
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
                    <button
                      type="button"
                      onClick={() => setDrillTarget({ g: g.g, dow: null, hour: null })}
                      title="View the tokens in this group"
                      className={cn(
                        'shrink-0 rounded px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-text-dim transition hover:bg-white/5 hover:text-text',
                        targetingThisGroup && drillTarget?.dow == null && 'bg-primary/12 text-primary',
                      )}
                    >
                      View tokens
                    </button>
                  </div>
                  <CreationHeatmap
                    cells={cellsByGroup.get(g.g) ?? []}
                    metric="count"
                    total={g.total}
                    onCellClick={(dow, hour) => setDrillTarget({ g: g.g, dow, hour })}
                    selectedCell={
                      targetingThisGroup && drillTarget?.dow != null
                        ? { dow: drillTarget.dow, hour: drillTarget.hour! }
                        : null
                    }
                  />
                </div>
              );
            })}
          </div>

          {/* Shared drill-down: the tokens behind whichever group card or
              heatmap tile was last picked above. */}
          {drillTarget && drillGroup && (
            <div className="mt-4 rounded-md border border-white/8 bg-white/2 p-2.5">
              <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
                <div className="flex flex-wrap items-center gap-1.5 text-[11px]">
                  <span
                    className="inline-block h-2.5 w-2.5 shrink-0 rounded-sm"
                    style={{ background: groupColor(drillGroup.g) }}
                  />
                  <span className="font-semibold text-text">Tokens</span>
                  <span className="text-text-dim">·</span>
                  <GroupKeyInline group={drillGroup} />
                  {drillTarget.dow != null && drillTarget.hour != null && (
                    <>
                      <span className="text-text-dim">·</span>
                      <span className="text-text-dim">
                        {dowLabel(drillTarget.dow)} {String(drillTarget.hour).padStart(2, '0')}:00
                        (every week in window)
                      </span>
                    </>
                  )}
                </div>
                <button
                  type="button"
                  onClick={() => setDrillTarget(null)}
                  className="shrink-0 text-[11px] text-text-dim hover:text-text"
                >
                  Close
                </button>
              </div>
              {drillIsError ? (
                <p className="text-red">
                  {apiErrorMessage(drillErrorRaw, 'Failed to load tokens')}
                </p>
              ) : (
                <TokenTable
                  columns={drillColumns}
                  rows={drillTokens}
                  existingKeys={ALL_TOKEN_INFO_KEYS}
                  serverSide
                  serverTotal={drillTotal}
                  onQueryChange={setDrillQuery}
                  loading={drillLoading}
                  resetKey={drillResetKey}
                  charts
                  searchable
                  colToggle
                  hoverable
                  tableId="creation_stats_grouped_drilldown"
                  emptyMessage="No tokens found for this selection"
                  selectedKey={inspectedToken?.mint ?? null}
                  onSelect={(mint) => {
                    const row = mint ? drillTokens.find((r) => r.mint_address === mint) : null;
                    setInspectedToken(mint ? { mint, symbol: row?.symbol } : null);
                  }}
                />
              )}
            </div>
          )}
          {inspectedToken && (
            <LazyLabTokenInspectModal
              target={inspectFromMint(inspectedToken.mint, inspectedToken.symbol)}
              titleSuffix="Token inspect"
              onClose={() => setInspectedToken(null)}
            />
          )}
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

/** Full group-key block for a heatmap card (label/value grid; ix_labels shown
 *  as pretty JSON via `IxLabelsDisplay`, click-to-copy). Mirrors the sweep
 *  page's group-chip layout. */
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
              <IxLabelsDisplay labels={parts} copyJson className="text-secondary" empty="∅" />
            ) : (
              <span className="font-mono text-[11px] text-secondary">{v}</span>
            )}
          </Fragment>
        );
      })}
    </div>
  );
}
