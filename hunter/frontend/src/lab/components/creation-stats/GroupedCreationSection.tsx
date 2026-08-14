import { Fragment, lazy, Suspense, useEffect, useMemo, useRef, useState } from 'react';
import { Link } from 'react-router-dom';
import { useStoredField } from 'hooks/useLocalStorage';
import { STORAGE_KEYS } from 'lib/storage';
import { skipToken } from '@reduxjs/toolkit/query/react';
import { Button } from 'components/ui/Button';
import { Badge } from 'components/ui/Badge';
import { DateTimeRangePicker } from 'components/ui/DateTimeRangePicker';
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
import { parseIxLabelsFilter, buildFieldFilters } from '@lab/components/sweep/fingerprintFilters';
import {
  FingerprintGroupPicker,
  type CashbackFilter,
} from '@lab/components/sweep/FingerprintGroupPicker';
import { SOL_BUCKET_WIDTH, BUCKETED_GROUP_FIELDS } from '@lab/components/sweep/groupedTypes';
import { formatWithCommas } from 'utils/format';
import { ixLabelsCountTail } from 'lib/ixLabels';
import { cn } from 'lib/cn';
import { CreationHeatmap } from 'components/creation-stats/CreationHeatmap';
import { DOW_ROWS } from 'components/creation-stats/creationStats';
import { FingerprintScopeControl } from 'components/strategy/FingerprintScopeControl';
import { useFingerprintMatches } from '@lab/components/strategy/useFingerprintMatches';
import { useFlowPatternSource } from 'hooks/useFlowPatternKeys';
import { CREATION_FIELD_HELP } from 'lib/strategy/strategyHelp';
import { useGetFingerprintsQuery, useCreateFingerprintMutation } from 'store/sharedEndpoints';
import {
  fingerprintIdentityFromGroupKey,
  matchFingerprintsForGroups,
  withIxLabelsFilter,
} from 'lib/strategy/matchGroupFingerprint';
import { fingerprintNameFromGroupKey } from 'lib/strategy/fingerprintNameFromGroupKey';
import { fingerprintsHref } from 'lib/strategy/nav';

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

const LOOKBACK_PRESETS = RANGE_OPTIONS.map((o) => ({
  value: String(o.value),
  label: `Last ${o.label}`,
}));
import {
  GROUP_FIELDS,
  GROUP_FIELD_LABELS,
  TOP_OPTIONS,
  RANK_BY_OPTIONS,
  MISSING_VALUE,
  groupColor,
  groupValueParts,
  drillTokenFilters,
  groupedCreationArgsEqual,
  type GroupedCreationArgs,
  type GroupedCreationCell,
  type GroupedCreationGroup,
  type GroupedCreationTokensArgs,
  type GroupField,
  type GroupRankBy,
} from './groupedCreationStats';

/** Debounce localStorage writes for high-churn filter text (React state stays live). */
const FILTER_LS_DEBOUNCE_MS = 400;

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

/** A `GroupedCreationCell` lacks the outcome/trade fields `CreationHeatmap`
 *  reads; the count view never touches them, so zero-fill is safe (count =
 *  volume). Per-cell trades are deferred (trade-counts plan §5) — the
 *  small-multiple heatmaps here always show `metric="count"`. */
function toHeatCell(c: GroupedCreationCell): CreationHeatCell {
  return {
    dow: c.dow,
    hour: c.hour,
    count: c.count,
    matured: 0,
    known: 0,
    migrated: 0,
    dead: 0,
    trades: 0,
    trades_avg: null,
    trades_per_day: 0,
  };
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
  // One `mt:page.creationStats` blob for the whole surface (page + this section);
  // each control owns a field, so a new knob costs a field, not a key.
  const P = STORAGE_KEYS.pageCreationStats;
  const [rawGroupBy, setGroupBy] = useStoredField<GroupField[]>(P, 'groupedBy', DEFAULT_GROUP_BY);
  // Sanitize against the current `GROUP_FIELDS` — a stale entry from before a
  // backend field rename (or a removed field like the old `creator_wallet`) is
  // invisible in the picker (which only renders known fields) and unremovable
  // by the user, and the backend's `parse_group_by` hard-rejects it forever.
  const groupBy = useMemo(
    () => rawGroupBy.filter((f) => (GROUP_FIELDS as readonly string[]).includes(f)),
    [rawGroupBy],
  );
  // 16 rows — enough that a launch tool's preset ladder shows up as a ladder
  // rather than as its top few rungs (the 6-preset max-buy client needed it).
  const [top, setTop] = useStoredField<number>(P, 'groupedTop', 16);
  // Ranking criterion for the top-N — "trades per token" is the one that
  // actually surfaces a small elite group over a big group of mediocre
  // launches (raw "trades" still scales with group size like "count" does).
  const [rankBy, setRankBy] = useStoredField<GroupRankBy>(P, 'groupedRankBy', 'trades');
  // Bucket width (SOL) for the continuous SOL group fields — the same knob the
  // grouped sweep uses, so this dashboard groups a corpus identically to a sweep.
  const [bucketWidth, setBucketWidth] = useStoredField<number>(P, 'groupedBucketWidth', 1);
  // Exact mode: key the ◎ SOL fields on the amount itself, one group per distinct
  // value. Separate from the width (never a magic 0) — see Rust `SolPrecision`.
  // Default ON: a launch client's tell is a repeated EXACT amount, which any
  // non-zero bucket width smears across neighbours.
  const [exactSol, setExactSol] = useStoredField<boolean>(P, 'groupedExactSol', true);
  // Hour bins: a launch tool's activity is a burst, and a day bin flattens it.
  const [bucket, setBucket] = useStoredField<CreationBucket>(P, 'groupedBucket', 'hour');
  const [rangeDays, setRangeDays] = useStoredField<number>(P, 'groupedRange', 30);
  const [fieldFiltersText, setFieldFiltersText] = useStoredField<Record<string, string>>(
    P,
    'groupedFilters',
    {},
    { debounceMs: FILTER_LS_DEBOUNCE_MS },
  );
  const [cashbackFilter, setCashbackFilter] = useStoredField<CashbackFilter>(
    P,
    'groupedCashback',
    'all',
  );
  const [ixLabelsText, setIxLabelsText] = useStoredField<string>(P, 'groupedIxLabels', '', {
    debounceMs: FILTER_LS_DEBOUNCE_MS,
  });
  // Saved-fingerprint scope — same "ALL group over the engine-matched tokens"
  // contract as the sweep's/flow discovery's seed select. Set ⇒ the manual
  // group-by/filters above are ignored (both client-side and server-side).
  const [seedFingerprintId, setSeedFingerprintId] = useStoredField<string | null>(
    P,
    'groupedFingerprintId',
    null,
  );
  const { data: fingerprints = [] } = useGetFingerprintsQuery();
  const fingerprintsById = useMemo(() => {
    const map = new Map(fingerprints.map((f) => [f.id, f]));
    return map;
  }, [fingerprints]);
  const seedFp = seedFingerprintId ? fingerprintsById.get(seedFingerprintId) : undefined;
  const fpMatches = useFingerprintMatches(seedFingerprintId, seedFp?.name);
  function selectSeedFingerprint(id: string) {
    setSeedFingerprintId(id || null);
  }

  // Create-fingerprint-from-card: the group card's key is the same fingerprint
  // identity the sweep/flow-discovery promote path uses, so a card can be saved
  // as a fingerprint directly. `fpBusyGroup` = the group whose create is in
  // flight (per-card spinner); `fpError` surfaces a failure inline.
  const [createFingerprint] = useCreateFingerprintMutation();
  const [fpBusyGroup, setFpBusyGroup] = useState<number | null>(null);
  const [fpError, setFpError] = useState<string | null>(null);

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

  // Per-field value filters, parsed once. The bucketed ◎ SOL fields accept an
  // exact amount OR a bucket range and are validated (`parseSolFilterList`); the
  // discrete ones stay plain numbers. A bad entry blocks Analyze rather than
  // shipping a filter that reads as "no filter" or as unsatisfiable — the two
  // ways this control fails silently.
  const scalarFilters = useMemo(
    () =>
      buildFieldFilters(fieldFiltersText, {
        fields: SCALAR_FILTER_FIELDS,
        bucketed: BUCKETED_GROUP_FIELDS,
        cashback: cashbackFilter,
        labels: GROUP_FIELD_LABELS,
      }),
    [fieldFiltersText, cashbackFilter],
  );

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

    const fieldFilters = scalarFilters.filters;

    return {
      bucket: effBucket,
      tz,
      from,
      segment,
      groupBy,
      top,
      // Exact mode replaces the width outright (the backend ignores it there), so
      // the two are never sent together — one knob, one meaning.
      ...(exactSol
        ? { exactSol: true }
        : // Send only a non-default width so the 0.1 case keeps a stable cache key.
          bucketWidth !== SOL_BUCKET_WIDTH
          ? { bucketWidth }
          : {}),
      ...(Object.keys(fieldFilters).length > 0 ? { fieldFilters } : {}),
      // ix_labels grouping and the exact-set filter are mutually exclusive
      // (matches the sweep page): drop the filter when grouping by ix_labels.
      ...(!groupBy.includes('ix_labels') && ixFilter.labels
        ? { ixLabelsFilter: ixFilter.labels }
        : {}),
      // Send only a non-default rank so the `count` case keeps a stable cache key.
      ...(rankBy !== 'count' ? { rankBy } : {}),
    };
  }, [
    effBucket,
    tz,
    from,
    segment,
    groupBy,
    top,
    bucketWidth,
    exactSol,
    scalarFilters.filters,
    cashbackFilter,
    ixFilter.labels,
    rankBy,
    seedFingerprintId,
  ]);

  const { data, isFetching, isError, error } = useGetGroupedCreationStatsQuery(applied ?? skipToken);

  // Reset line isolation + the drill-down selection whenever a fresh analysis
  // is applied — a stale `group_key`/index from the previous grouping would
  // silently target the wrong (or a nonexistent) group otherwise.
  useEffect(() => {
    setIsolatedGroup(null);
    setDrillTarget(null);
    setFpError(null);
  }, [applied]);

  // "Dirty" = the draft differs from what's applied (or nothing applied yet).
  const dirty = !applied || !groupedCreationArgsEqual(draftArgs, applied);

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

  // Was the applied run keyed on exact SOL amounts? Then a card's SOL axes read
  // "1.515", not "1.5–1.6", and the fingerprint it maps to is an **exact** one
  // (NULL stored width ⇒ `SolPrecision::Exact` ⇒ raw-lamports equality), not a
  // bucketed one. (Prefer the Analyze snapshot over the response echo so an
  // in-flight refetch can't briefly flip the precision.)
  const appliedExactSol = applied?.exactSol ?? data?.bucket_width === null;

  // The precision the applied run grouped SOL axes at — the same precision the
  // fingerprint identity match/create must use so a card maps to the fingerprint
  // it would actually match. `null` IS the exact mode, never a substituted width:
  // inventing 0.1 there would mint a rule that arms on a window the card never
  // showed. Prefer the server echo (`data.bucket_width`): scoped runs ignore the
  // draft width and use the fingerprint's own; `applied.bucketWidth` is also
  // omitted for the default 0.1 case. Note the `??` chain only runs when the run
  // is bucketed — in exact mode the echo is `null`, which `??` would swallow.
  const appliedBucketWidth: number | null = appliedExactSol
    ? null
    : data?.bucket_width ?? applied?.bucketWidth ?? SOL_BUCKET_WIDTH;

  // Applied exact-set ix_labels filter. Prefer the Analyze snapshot (`applied`)
  // over the response echo so a in-flight refetch can't briefly drop the axis
  // while previous data (no filter) is still on screen. When Instruction labels
  // isn't in group-by, cards omit that axis unless the backend folded it —
  // `withIxLabelsFilter` re-attaches it for identity / create / display.
  const appliedIxLabels = applied?.ixLabelsFilter ?? data?.ix_labels_filter ?? null;

  // Per-group: the saved fingerprint whose identity matches this card's key (for
  // the "already a fingerprint" badge), and whether the key carries any criterion
  // (ALL / grouping-only cards can't become a fingerprint — hide Create then).
  // When scoped by a saved fingerprint, prefer that id directly (group_key is
  // stamped from it server-side, but the scope select is the authoritative link).
  // Exact grouping is saveable — a plain SOL label parses whole, and the identity
  // it builds carries a `null` width, which `sameWidth` keeps distinct from every
  // bucketed one. The single case that still isn't: an axis on a `u64` ceiling,
  // which no `BIGINT` column can hold (`identityLamportsAreStorable`).
  const fpByGroup = useMemo(() => {
    const scoped =
      applied?.fingerprintId != null
        ? fingerprintsById.get(applied.fingerprintId) ?? null
        : null;
    return matchFingerprintsForGroups(
      groups,
      fingerprints,
      appliedBucketWidth,
      appliedIxLabels,
      scoped,
    );
  }, [groups, fingerprints, fingerprintsById, appliedBucketWidth, applied?.fingerprintId, appliedIxLabels]);

  // Save a group card as a fingerprint. Identity = group_key axes, plus the
  // applied ix_labels filter when that axis wasn't grouped (mutually exclusive
  // with the filter in the picker — without this merge, create silently drops
  // the labels the Analyze already pinned). Plain fingerprint — no metric
  // config; flow patterns are added later on Flow discovery.
  async function createFingerprintFromGroup(group: GroupedCreationGroup) {
    setFpError(null);
    setFpBusyGroup(group.g);
    try {
      const gk = withIxLabelsFilter(group.group_key, appliedIxLabels);
      const identity = fingerprintIdentityFromGroupKey(gk, appliedBucketWidth);
      await createFingerprint({
        name: fingerprintNameFromGroupKey(gk, appliedBucketWidth),
        ...identity,
        metric_config: {},
      }).unwrap();
    } catch (e) {
      setFpError(apiErrorMessage(e as never, 'Failed to create fingerprint'));
    } finally {
      setFpBusyGroup(null);
    }
  }

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
      exactSol: applied.exactSol,
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
      filters: drillTokenFilters(drillQuery),
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
  // Fingerprint-scoped drill-in ⇒ these rows ARE the fingerprint's matched
  // tokens, so their charts / inspect modal draw the vol/non-vol overlay from
  // the SCOPED (applied, not draft) fingerprint's `volume_ix_patterns`. Manual
  // group-by drill-ins have no fingerprint and stay unconfigured (`null`).
  const drillFlowSource = useFlowPatternSource(applied?.fingerprintId ?? null);

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
          <DateTimeRangePicker
            aria-label="Look-back window"
            size="sm"
            zoneLabel={null}
            allowCustom={false}
            emptyLabel="Look-back"
            presets={LOOKBACK_PRESETS}
            value={{ preset: String(rangeDays), from: '', to: '' }}
            onChange={({ preset }) => setRangeDays(Number(preset))}
          />
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
          {/* Ranking criterion for the top-N. Inert (but still shown) under a
              saved-fingerprint scope — there's only ever one group there. */}
          <Select
            value={rankBy}
            onChange={(e) => setRankBy(e.target.value as GroupRankBy)}
            title="Rank groups by"
            className="max-w-[9rem]"
          >
            {RANK_BY_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                Rank by: {o.label}
              </option>
            ))}
          </Select>
          <Button
            size="sm"
            variant={dirty ? 'primary' : 'subtle'}
            active={dirty}
            disabled={isFetching || ixFilter.error != null || scalarFilters.error != null}
            onClick={() => setApplied(draftArgs)}
            title={
              scalarFilters.error ?? ixFilter.error ?? 'Run the analysis with the current settings'
            }
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
        matchedCount={fpMatches.count}
        matchedCountLoading={fpMatches.countLoading}
        onViewMatches={fpMatches.openMatches}
        onRequestMatchCount={fpMatches.ensureCount}
      />
      {fpMatches.matchesModal}

      {/* Group-by + value filters — shared with the sweep page's fingerprint
          control so both read identically. */}
      <div className="mb-3 rounded-md border border-white/8 bg-white/2 p-2.5">
        <FingerprintGroupPicker
          groupBy={groupBy}
          onToggleField={toggleField}
          fieldFiltersText={fieldFiltersText}
          onSetFieldFilter={(f, v) => setFieldFiltersText((prev) => ({ ...prev, [f]: v }))}
          onClearFilters={() => {
            setFieldFiltersText({});
            setCashbackFilter('all');
            setIxLabelsText('');
          }}
          cashbackFilter={cashbackFilter}
          onSetCashback={setCashbackFilter}
          bucketWidthSol={bucketWidth}
          onSetBucketWidth={setBucketWidth}
          exactSol={exactSol}
          onSetExactSol={setExactSol}
          ixLabelsText={ixLabelsText}
          onSetIxLabels={setIxLabelsText}
          ixFilter={ixFilter}
          disabled={!!seedFingerprintId}
          emptyHint={
            seedFingerprintId
              ? 'Scoped to the saved fingerprint → one "ALL" group of matching tokens.'
              : 'No fields selected → one "ALL" group (every token in the window).'
          }
        />
        {scalarFilters.error && (
          <p className="mt-1 text-[11px] text-red">{scalarFilters.error}</p>
        )}
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
                  <GroupKeyInline group={g} ixLabelsFilter={appliedIxLabels} />
                  <span className="text-text-dim/70">
                    · {formatWithCommas(g.total)} tokens · {formatWithCommas(g.trades)} trades
                  </span>
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

          {fpError && <p className="mt-3 text-[11px] text-red">{fpError}</p>}

          {/* Small-multiple day×hour heatmaps (recurring active hours per group). */}
          <div className="mt-4 grid gap-3 xl:grid-cols-2">
            {groups.map((g) => {
              const targetingThisGroup = drillTarget?.g === g.g;
              // Selected = its tokens are showing in the shared drill-down below.
              const selected = targetingThisGroup;
              const fp = fpByGroup.get(g.g) ?? { matched: null, canCreate: false, overflow: false };
              const gk = withIxLabelsFilter(g.group_key, appliedIxLabels);
              const hasIxLabels =
                Object.prototype.hasOwnProperty.call(gk, 'ix_labels') &&
                gk.ix_labels !== MISSING_VALUE;
              return (
                <div
                  key={g.g}
                  className={cn(
                    'rounded-md border p-2.5 transition',
                    selected
                      ? 'border-primary/50 bg-primary/5 ring-1 ring-primary/30'
                      : 'border-white/8 bg-white/2',
                  )}
                >
                  <div className="mb-1.5 flex items-start gap-2">
                    <span
                      className="mt-1 inline-block h-2.5 w-2.5 shrink-0 rounded-sm"
                      style={{ background: groupColor(g.g) }}
                    />
                    <div className="min-w-0 flex-1">
                      <GroupKeyBlock group={g} ixLabelsFilter={appliedIxLabels} />
                    </div>
                    <div className="flex shrink-0 flex-col items-end gap-1.5">
                      <span className="text-[11px] text-text-dim">
                        {formatWithCommas(g.total)} tokens · {formatWithCommas(g.trades)} trades (
                        {g.trades_avg.toFixed(1)}/token)
                      </span>
                      <div className="flex items-center gap-1.5">
                        {/* Already saved → link to it; else offer one-click save.
                            The ALL / grouping-only cards carry no criterion, so
                            neither shows (nothing to bind). */}
                        {fp.matched ? (
                          <Link
                            to={fingerprintsHref(fp.matched.id)}
                            title={`Open fingerprint “${fp.matched.name || fp.matched.id.slice(0, 8)}”`}
                            className="hover:opacity-90"
                          >
                            <Badge variant="info" size="sm">
                              fp · {fp.matched.name || fp.matched.id.slice(0, 8)}
                            </Badge>
                          </Link>
                        ) : fp.canCreate ? (
                          <Button
                            size="sm"
                            variant="subtle"
                            disabled={fpBusyGroup === g.g}
                            onClick={() => createFingerprintFromGroup(g)}
                            title={
                              hasIxLabels
                                ? 'Save this group as a fingerprint (includes ix_labels)'
                                : 'Save this group as a fingerprint — only grouped axes are saved; add Instruction labels to the group-by (or pin an exact set filter) to persist ix_labels'
                            }
                          >
                            {fpBusyGroup === g.g ? 'Creating…' : 'Create fingerprint'}
                          </Button>
                        ) : fp.overflow ? (
                          // Say why the action is gone rather than leaving a silent
                          // gap — the reason is a real constraint, not an oversight.
                          <span
                            className="text-[10px] text-text-dim/60"
                            title={
                              'A SOL axis on this card is an out-of-i64 "no limit" ceiling (pump.fun\n' +
                              'passes max_sol_cost = u64::MAX to mean "fill at any price"), not an\n' +
                              'amount anyone bid. A fingerprint axis is a BIGINT, so that value cannot\n' +
                              'be stored as a criterion — and the live matcher fails a configured axis\n' +
                              'against it rather than wrapping it.\n\n' +
                              'Group by an axis that carries a real amount instead.'
                            }
                          >
                            ceiling — not saveable
                          </span>
                        ) : null}
                        <Button
                          size="sm"
                          variant={selected && drillTarget?.dow == null ? 'primary' : 'subtle'}
                          active={selected && drillTarget?.dow == null}
                          onClick={() => setDrillTarget({ g: g.g, dow: null, hour: null })}
                          title="View the tokens in this group"
                        >
                          View tokens
                        </Button>
                      </div>
                    </div>
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
                  <GroupKeyInline group={drillGroup} ixLabelsFilter={appliedIxLabels} />
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
                  chartsDefaultOn
                  flowPatternKeys={drillFlowSource.keys}
                  flowFingerprintId={drillFlowSource.fingerprintId}
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
              flowPatternKeys={drillFlowSource.keys}
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
function GroupKeyInline({
  group,
  ixLabelsFilter,
}: {
  group: GroupedCreationGroup;
  ixLabelsFilter?: string[] | null;
}) {
  const entries = Object.entries(withIxLabelsFilter(group.group_key, ixLabelsFilter));
  if (entries.length === 0) return <span className="text-text">ALL tokens</span>;
  return (
    <span className="text-text">
      {entries.map(([k, v], i) => (
        <Fragment key={k}>
          {i > 0 && <span className="text-text-dim"> · </span>}
          <span className="text-text-dim">{GROUP_FIELD_LABELS[k as GroupField] ?? k}=</span>
          {k === 'ix_labels' ? (
            // Tail, not a bare count — this legend line is how two groups in
            // the same chart are told apart, and same-length sequences are the
            // common case (a buy-variant swap).
            <span className="font-mono" title={v === MISSING_VALUE ? undefined : v}>
              {v === MISSING_VALUE ? '∅' : ixLabelsCountTail(groupValueParts(k, v))}
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
function GroupKeyBlock({
  group,
  ixLabelsFilter,
}: {
  group: GroupedCreationGroup;
  ixLabelsFilter?: string[] | null;
}) {
  const entries = Object.entries(withIxLabelsFilter(group.group_key, ixLabelsFilter));
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
