import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { useSearchParams } from 'react-router-dom';
import { useLocalStorage } from 'hooks/useLocalStorage';
import { useTimezone } from 'context/TimezoneContext';
import { ACCORDION_IDS, STORAGE_KEYS } from 'lib/storage';
import { DataTable } from 'components/table/DataTable';
import { TokenTable } from 'components/tokens/TokenTable';
import type { ColumnDef, SortEntry, TableQuery } from 'components/table/types';
import type { FilterSpec } from 'components/table/numericFilter';
import { parseFilterSpec } from 'components/table/numericFilter';
import { numericColKeys } from 'services/tableRequest';
import { InlineAlert } from 'components/ui/Modal';
import { PageHeader } from 'components/ui/PageHeader';
import { SectionDivider } from 'components/ui/SectionDivider';
import { Badge } from 'components/ui/Badge';
import { DatePicker } from 'components/ui/DatePicker';
import { IconButton } from 'components/ui/IconButton';
import { PromoteIcon, SpinnerIcon, TrashIcon } from 'components/ui/icons';
import { Accordion } from 'components/ui/Accordion';
import { VisibilityToggleButton } from 'components/ui/VisibilityToggleButton';
import {
  buildEventMarkersForEpisodes,
  episodeRowKey,
  markerRowOverlay,
  type InspectTarget,
} from 'components/strategy/inspectTarget';
import { PositionSummarySection } from 'components/strategy/PositionSummarySection';
import {
  PositionChartCardExtra,
  positionChartFactsFromEpisodes,
  type PositionChartEpisode,
} from 'components/strategy/PositionChartCardExtra';
import { runSummaryFromRows } from 'lib/strategy/runSummary';
import {
  filterRowsByFocus,
  type PositionFocusLens,
  type PositionFocusRow,
} from 'lib/strategy/positionFocus';
import type { PositionChartPoint } from 'lib/strategy/positionChartPoints';
import { cn } from 'lib/cn';
import { flowPatternKeysOf } from 'lib/flow/flowPatternKeys';
import { formatSigned, formatSignedPct, pctGradeClass, signedToneClass } from 'lib/signedTone';
import { useBackgroundJobActions, useBackgroundJobsState } from '@lab/context/BackgroundJobsContext';
import { apiErrorMessage } from 'store/apiSlice';
import {
  useGetGroupedSweepRunsQuery,
  useGetGroupedSweepGroupsQuery,
  useGetComboTokenResultsQuery,
  useStartGroupedSweepMutation,
  useDeleteGroupedSweepRunMutation,
  usePruneGroupedSweepsMutation,
  usePromoteSweepGroupMutation,
} from '@lab/store/labEndpoints';
import { useGetFingerprintsQuery, useGetStrategyRulesQuery } from 'store/sharedEndpoints';
import { useStreamedSweepResults, COMBO_PAGE_SIZE } from '@lab/hooks/useStreamedSweepResults';
import { SelectedSweepHistory } from '@lab/components/sweep/SelectedSweepHistory';
import { GenericSweepConfigForm, GENERIC_STRATEGY_ID } from '@lab/components/sweep/GenericSweepConfigForm';
import { PromoteRuleModal } from '@lab/components/sweep/PromoteRuleModal';
import { LazyLabTokenInspectModal } from '@lab/components/strategy/LazyLabTokenInspectModal';
import {
  buildGenericComboColumns,
  buildGenericGroupColumns,
} from '@lab/components/sweep/genericSweepColumns';
import type {
  GroupedSweepGroupRecord,
  GroupedSweepRunRecord,
  GroupedSweepResultRecord,
  ComboTokenResult,
  GroupedSweepStartArgs,
} from '@lab/components/sweep/groupedTypes';
import { fingerprintMatchesIdentity } from 'lib/strategy/matchGroupFingerprint';
import { isLegacyCostModel } from 'lib/strategy/types';
import type { Fingerprint, PromotedRuleDraft, StrategyRule } from 'lib/strategy/types';

const sweepGroupRowKey = (g: GroupedSweepGroupRecord) => g.id;
const sweepComboRowKey = (r: GroupedSweepResultRecord) => String(r.combo_id);

/**
 * Convert the DataTable's raw per-column filter strings into structured
 * {@link FilterSpec}s for the server. Only numeric combo columns (`filterNumber`)
 * participate — every filterable combo column is numeric — and only recognized
 * numeric expressions (`>5`, `1..10`, …) are forwarded; anything else is dropped
 * (the backend resolves each column through the same allowlist as sort). Operands
 * stay in the column's *displayed* units (e.g. Win% as percent points); the server
 * expression is scaled to match, so no unit conversion is needed here.
 */
function comboFiltersFromColFilters(
  colFilters: Record<string, string>,
  numericKeys: ReadonlySet<string>,
): Record<string, FilterSpec> {
  const out: Record<string, FilterSpec> = {};
  for (const [key, raw] of Object.entries(colFilters)) {
    if (!numericKeys.has(key)) continue;
    const text = raw.trim();
    if (!text) continue;
    const spec = parseFilterSpec(text);
    if (spec) out[key] = spec;
  }
  return out;
}

/** Combo columns that filter numerically (declare `filterNumber`) — the set the
 *  server accepts. `buyAmountSol` doesn't change *which* columns are filterable,
 *  so this is a stable, build-once set. */
const COMBO_NUMERIC_KEYS: ReadonlySet<string> = numericColKeys(buildGenericComboColumns());

/**
 * Local-midnight instant for a `YYYY-MM-DD` date-input value, as an ISO timestamp.
 * A bare `new Date('2026-08-01')` parses a date-only string as **UTC** midnight,
 * but every run timestamp on this page renders in **local** time
 * ({@link runPickerLine}) — so the bare parse prunes on a boundary the user can't
 * see, off by their UTC offset in whichever direction they sit from UTC.
 * Returns `null` if the input isn't a real date.
 */
function localMidnightIso(day: string): string | null {
  const d = new Date(`${day}T00:00:00`);
  return Number.isNaN(d.getTime()) ? null : d.toISOString();
}

/**
 * Today as `YYYY-MM-DD` in local time — the newest cutoff the prune input accepts.
 * A future cutoff matches *every* run, including one that is still sweeping and
 * whose sink is mid-write against the run row it would delete.
 */
function todayLocalIsoDate(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/** Compact run-picker line (date · method · tokens · groups · combos). */
function runPickerLine(r: GroupedSweepRunRecord): string {
  const d = new Date(r.created_at);
  const p = (n: number) => String(n).padStart(2, '0');
  const date = `${p(d.getMonth() + 1)}/${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
  const groups =
    r.status === 'completed'
      ? `${r.group_count} grp`
      : `${r.status === 'running' ? 'run' : 'part'} ${r.groups_done}/${r.group_count} grp`;
  const suffix = r.label ? ` · ${r.label}` : '';
  return `${date} · ${r.method} · ${r.token_count.toLocaleString()} tok · ${groups} × ${r.combo_count.toLocaleString()} combos${suffix}`;
}

function comboChartEpisode(r: ComboTokenResult): PositionChartEpisode {
  const open = r.fired && (r.exit === 'Open' || r.exit_time == null);
  return {
    holdSecs: r.fired ? r.holding_secs : null,
    pnlSol: r.fired ? r.pnl_sol : null,
    pnlPct: r.fired ? r.pnl_pct : null,
    entrySol: null,
    entryPrice: r.entry_price,
    exitPrice: r.exit_price,
    exitReason: r.exit,
    isOpen: open,
    include: r.fired,
  };
}

/** ComboTokenResult → inspect target (entry/exit markers). */
function comboTarget(r: ComboTokenResult): InspectTarget {
  return {
    mint_address: r.mint_address,
    symbol: r.symbol,
    entryTime: r.entry_time ?? null,
    entryPrice: r.entry_price ?? null,
    entryTx: r.entry_tx ?? null,
    exitTime: r.exit_time ?? null,
    exitPrice: r.exit_price ?? null,
    exitTx: r.exit_tx ?? null,
    exitLabel: r.fired ? r.exit : null,
  };
}

/** Per-row entry/exit markers for the token-results charts grid. */
const comboRowOverlay = markerRowOverlay(comboTarget);

/**
 * Combo-tokens deviation from the appended token-info defaults. Each row is one
 * simulated position, so `token_amount` is the size the combo actually took —
 * the check that a headline PnL isn't riding a bag too small to matter — where
 * the shared set treats it as token background and hides it.
 */
const COMBO_TOKENS_DEFAULT_COLS = { token_amount: true } as const;

/**
 * Aggregate the combo's per-token rows into the summary tiles, via the **shared**
 * run-summary renderer — the same builder the Simulate page and the live/paper
 * positions card use, so all three show the same metrics under the same labels in
 * the same format (parity plan F1-F8). It runs over whatever cohort it's handed
 * (the FULL results, or the table's current filtered subset), so the summary
 * tracks the table's search/column filters.
 *
 * The realized-vs-MTM two-band split and the metric arithmetic live in
 * `lib/strategy/runSummary`; this is now only the row adapter.
 */
function comboFocusRow(r: ComboTokenResult): PositionFocusRow {
  const open = r.fired && r.exit === 'Open';
  const timeIso = open ? r.entry_time ?? r.created_at : r.exit_time ?? r.entry_time;
  const timeMs = timeIso ? Date.parse(timeIso) : NaN;
  return {
    id: episodeRowKey(r),
    mint_address: r.mint_address,
    fired: r.fired,
    isOpen: open,
    isClosed: r.fired && r.exit !== 'Open' && r.exit !== 'NoEntry',
    exit_reason: r.exit === 'Open' || r.exit === 'NoEntry' ? null : r.exit,
    pnl_sol: r.fired ? r.pnl_sol : null,
    pnl_pct: r.fired ? r.pnl_pct : null,
    hold_secs: r.fired ? r.holding_secs : null,
    is_migrated: r.is_migrated,
    timeMs: Number.isFinite(timeMs) ? timeMs : null,
  };
}

function comboChartPoints(rows: ComboTokenResult[]): PositionChartPoint[] {
  const out: PositionChartPoint[] = [];
  for (const r of rows) {
    if (!r.fired) continue;
    const open = r.exit === 'Open';
    const timeIso = open ? r.entry_time ?? r.created_at : r.exit_time ?? r.entry_time;
    const timeMs = timeIso ? Date.parse(timeIso) : NaN;
    if (!Number.isFinite(timeMs)) continue;
    out.push({
      key: episodeRowKey(r),
      label: r.symbol || r.mint_address.slice(0, 8),
      mint_address: r.mint_address,
      timeMs,
      pnlSol: r.pnl_sol ?? 0,
      pnlPct: Number.isFinite(r.pnl_pct) ? r.pnl_pct : null,
      holdSeconds: r.holding_secs > 0 ? r.holding_secs : open ? 1 : null,
      entrySol: Math.abs(r.pnl_sol) > 0 && r.pnl_pct !== 0 ? Math.abs(r.pnl_sol / (r.pnl_pct / 100)) : 0.1,
      isOpen: open,
      exit_reason: open ? null : r.exit,
      is_migrated: r.is_migrated,
    });
  }
  return out;
}

/**
 * Generic-engine grouped sweep view (redesign FE5.2 + 5.3). One page replaces the
 * three per-strategy sweep pages: configure → run → pick a run → group-summary
 * table → drill into ranked combos → per-token results, with a one-click
 * **Promote** on any group/combo that opens the rule editor pre-filled. Reuses the
 * kept streaming/persistence infrastructure (`useStreamedSweepResults`, the sweep
 * RTK endpoints) with `strategy_id = "generic"`.
 */
export function GenericSweepView() {
  const strategyId = GENERIC_STRATEGY_ID;
  const [searchParams, setSearchParams] = useSearchParams();
  const runFromUrl = searchParams.get('run');
  const runsQuery = useGetGroupedSweepRunsQuery({ strategyId });
  const runs = useMemo(() => runsQuery.data ?? [], [runsQuery.data]);

  const [selectedRunId, setSelectedRunId] = useLocalStorage<string | null>(
    `${STORAGE_KEYS.sweepSel}.generic`,
    null,
  );

  // Home / deep-link `?run=` wins over localStorage — but ONLY on the first load
  // of the runs list. Without the one-shot guard this effect fights the URL-sync
  // effect below: picking a run in the dropdown sets `selectedRunId` while
  // `?run=` still names the *previous* run, so this would immediately write the
  // old id straight back and the page would snap to the old run's results.
  const urlHydratedRef = useRef(false);
  useEffect(() => {
    if (urlHydratedRef.current || runs.length === 0) return;
    urlHydratedRef.current = true;
    if (!runFromUrl) return;
    if (!runs.some((r) => r.id === runFromUrl)) return;
    if (selectedRunId === runFromUrl) return;
    setSelectedRunId(runFromUrl);
  }, [runFromUrl, runs, selectedRunId, setSelectedRunId]);

  const activeRunId = selectedRunId && runs.some((r) => r.id === selectedRunId)
    ? selectedRunId
    : (runs[0]?.id ?? null);
  const activeRun = runs.find((r) => r.id === activeRunId) ?? null;

  // Keep URL in sync so resume links and refresh stay on the same run.
  useEffect(() => {
    if (!activeRunId) return;
    setSearchParams(
      (prev) => {
        if (prev.get('run') === activeRunId) return prev;
        const next = new URLSearchParams(prev);
        next.set('run', activeRunId);
        return next;
      },
      { replace: true },
    );
  }, [activeRunId, setSearchParams]);

  const [activeGroupId, setActiveGroupId] = useState<string | null>(null);
  const [activeComboId, setActiveComboId] = useState<number | null>(null);
  useEffect(() => setActiveComboId(null), [activeGroupId]);

  const selectRun = useCallback(
    (id: string) => {
      setSelectedRunId(id);
      setActiveGroupId(null);
      setActiveComboId(null);
    },
    [setSelectedRunId],
  );

  // --- run lifecycle (start / delete / prune) ---
  const [startSweep, startState] = useStartGroupedSweepMutation();
  const startErr = apiErrorMessage(startState.error, 'Failed to start sweep');
  const { markStarting } = useBackgroundJobActions();
  const { isRunning, jobs } = useBackgroundJobsState();
  const sweepRunning = isRunning('sweep', 'sweep') || startState.isLoading;
  // Live persisted-group counters from the `sweep_group_done` SSE stream — the
  // run row's `groups_done` only refreshes on the terminal frame, so the
  // in-progress banner prefers these when the running job matches this run.
  const sweepJob = jobs.find((j) => j.kind === 'sweep') ?? null;

  const [deleteRun, deleteState] = useDeleteGroupedSweepRunMutation();
  const [pruneRuns, pruneState] = usePruneGroupedSweepsMutation();
  const deleteErr = apiErrorMessage(deleteState.error ?? pruneState.error, 'Failed to delete sweep history');
  const [pruneBefore, setPruneBefore] = useState('');
  // Outcome of the last prune. Without it a cutoff that matched nothing looks
  // byte-identical to a successful one — indistinguishable from a broken button.
  const [pruneNote, setPruneNote] = useState<{ tone: 'success' | 'warning'; text: string } | null>(null);

  const [reuseNonce, setReuseNonce] = useState(0);
  const formRef = useRef<HTMLDivElement>(null);

  async function run(args: GroupedSweepStartArgs) {
    markStarting('sweep', 'sweep', 'Grouped sweep');
    try {
      const { run_id } = await startSweep(args).unwrap();
      setSelectedRunId(run_id);
    } catch {
      // Surfaced via startState.error (e.g. 409 = one already running).
    }
  }

  async function onDeleteRun() {
    if (!activeRunId) return;
    if (!window.confirm('Delete this sweep run and all its groups/results?')) return;
    try {
      await deleteRun({ strategyId, runId: activeRunId }).unwrap();
      setSelectedRunId(null);
    } catch { /* surfaced via deleteErr */ }
  }
  async function onPrune() {
    if (!pruneBefore) return;
    const before = localMidnightIso(pruneBefore);
    if (!before) return;
    // `max` only marks a typed-in future date :invalid — the value still reaches
    // here, and a future cutoff deletes every run including one still sweeping.
    if (pruneBefore > todayLocalIsoDate()) {
      setPruneNote({
        tone: 'warning',
        text: `Cutoff ${pruneBefore} is in the future — that would delete every run, including one still sweeping. Pick today or earlier.`,
      });
      return;
    }
    if (!window.confirm(`Delete all sweep runs created before ${pruneBefore} (local midnight)?`)) return;
    // Whether the run we're looking at is itself in the deleted range. Clearing
    // the selection unconditionally bumped the user to the newest run even when
    // their run survived the cutoff.
    const activeIsPruned = !!activeRun && new Date(activeRun.created_at).toISOString() < before;
    setPruneNote(null);
    try {
      const { deleted } = await pruneRuns({ strategyId, before }).unwrap();
      if (activeIsPruned) setSelectedRunId(null);
      setPruneNote(
        deleted > 0
          ? {
              tone: 'success',
              text: `Deleted ${deleted} sweep run${deleted === 1 ? '' : 's'} created before ${pruneBefore}.`,
            }
          : {
              tone: 'warning',
              text: `No sweep runs were created before ${pruneBefore} — nothing deleted.`,
            },
      );
    } catch { /* surfaced via deleteErr */ }
  }

  // --- groups ---
  const groupsQuery = useGetGroupedSweepGroupsQuery(
    { strategyId, runId: activeRunId ?? '' },
    { skip: !activeRunId },
  );
  const groups = groupsQuery.data ?? [];
  const activeGroup = groups.find((g) => g.id === activeGroupId) ?? null;
  const tokensDone = useMemo(
    () => (groups.length ? groups.reduce((s, g) => s + g.token_count, 0) : null),
    [groups],
  );

  // --- combos (streamed) ---
  const [comboPage, setComboPage] = useState(0);
  const [comboPageSize, setComboPageSize] = useState(COMBO_PAGE_SIZE);
  const [comboSortKeys, setComboSortKeys] = useState<SortEntry[]>([]);
  const [comboFilters, setComboFilters] = useState<Record<string, FilterSpec>>({});
  const onComboQueryChange = useCallback((q: TableQuery) => {
    setComboPage(q.page - 1);
    setComboPageSize(q.pageSize);
    setComboSortKeys(q.sortKeys);
    setComboFilters(comboFiltersFromColFilters(q.colFilters, COMBO_NUMERIC_KEYS));
  }, []);
  useEffect(() => setComboPage(0), [activeGroupId]);

  // Drop the previous run's drill-down DURING render, not in an effect. An
  // effect-based reset lands one commit too late: the results stream would
  // already have fired once for (new run, *old* group) → 404 + error flash.
  const [runScope, setRunScope] = useState(activeRunId);
  if (runScope !== activeRunId) {
    setRunScope(activeRunId);
    setActiveGroupId(null);
    setActiveComboId(null);
    setComboPage(0);
  }

  const {
    rows: results,
    total: resultsTotal,
    exitMetricLegend,
    loading: resultsLoading,
    error: resultsErr,
  } =
    useStreamedSweepResults(
      strategyId,
      activeRunId,
      activeGroupId,
      comboPage,
      comboPageSize,
      comboSortKeys,
      comboFilters,
    );

  /** The drilled-into combo's row. Server-paged, so it is absent whenever the
   *  selection lives on a page the table isn't currently holding. */
  const activeCombo = results.find((r) => r.combo_id === activeComboId) ?? null;
  /** Scroll target for the breadcrumb's combo segment (the token-results drill-in). */
  const comboTokensRef = useRef<HTMLDivElement | null>(null);

  // --- promote ---
  const [promote, promoteState] = usePromoteSweepGroupMutation();
  const [promoteDraft, setPromoteDraft] = useState<PromotedRuleDraft | null>(null);
  /** Fingerprint created by the latest promote — enables flow metric-series on inspect. */
  const [promotedFp, setPromotedFp] = useState<{ groupId: string; fingerprintId: string } | null>(
    null,
  );
  const promoteErr = apiErrorMessage(promoteState.error, 'Promote failed');
  const doPromote = useCallback(
    async (groupId: string, comboId?: number) => {
      if (!activeRunId) return;
      try {
        const draft = await promote({ runId: activeRunId, groupId, comboId }).unwrap();
        setPromoteDraft(draft);
        setPromotedFp({ groupId, fingerprintId: draft.fingerprint_id });
      } catch { /* surfaced via promoteErr */ }
    },
    [activeRunId, promote],
  );

  const buyAmountSol = activeRun?.buy_amount_sol ?? 1;
  const { data: fingerprints = [] } = useGetFingerprintsQuery();
  const { data: strategyRules = [] } = useGetStrategyRulesQuery();
  // group_key → saved fingerprint (promote identity) → rules for the Used-by column.
  const fingerprintByGroupId = useMemo(() => {
    // A scoped run pins the whole corpus to one saved fingerprint; every group is a
    // sub-slice of it, so that fingerprint is the authoritative attribution — but a
    // group that narrowed it further (a group-by axis inside the scope) promotes to
    // its own narrower fingerprint, which the identity below finds.
    const scopeFp =
      activeRun?.fingerprint_id != null
        ? fingerprints.find((f) => f.id === activeRun.fingerprint_id) ?? null
        : null;
    const map = new Map<string, Fingerprint>();
    for (const g of groups) {
      // Compare against the identity the BACKEND resolved for this group
      // (`selection.identity` — exactly the columns `find_or_create` keys on), never
      // one rebuilt here from the group key. That rebuild was a second, lossy copy of
      // the promote-side derivation: it could not see the run's `field_filters` at
      // all, so a pinned run's card never badged the fingerprint promote would create.
      const id = g.selection?.identity;
      const fp = id
        ? fingerprints.find((f) => fingerprintMatchesIdentity(f, id)) ?? scopeFp
        : scopeFp;
      if (fp) map.set(g.id, fp);
    }
    return map;
  }, [groups, fingerprints, activeRun?.fingerprint_id]);
  const rulesByFingerprintId = useMemo(() => {
    const map = new Map<string, StrategyRule[]>();
    for (const r of strategyRules) {
      const list = map.get(r.fingerprint_id);
      if (list) list.push(r);
      else map.set(r.fingerprint_id, [r]);
    }
    return map;
  }, [strategyRules]);
  const groupColumns = useMemo(
    () => buildGenericGroupColumns(buyAmountSol, { fingerprintByGroupId, rulesByFingerprintId }),
    [buyAmountSol, fingerprintByGroupId, rulesByFingerprintId],
  );
  const comboColumns = useMemo(
    () => buildGenericComboColumns(buyAmountSol, exitMetricLegend),
    [buyAmountSol, exitMetricLegend],
  );

  const groupRowActions = useCallback(
    (g: GroupedSweepGroupRecord): ReactNode => (
      <IconButton
        variant="primary"
        size="md"
        label="Promote"
        disabled={promoteState.isLoading}
        onClick={(e) => {
          e.stopPropagation();
          void doPromote(g.id);
        }}
        title="Promote this group's best combo → new rule"
        aria-label="Promote this group's best combo → new rule"
      >
        <PromoteIcon />
      </IconButton>
    ),
    [doPromote, promoteState.isLoading],
  );
  const comboRowActions = useCallback(
    (r: GroupedSweepResultRecord): ReactNode =>
      activeGroupId ? (
        <IconButton
          variant="primary"
          size="md"
          label="Promote"
          disabled={promoteState.isLoading}
          onClick={(e) => {
            e.stopPropagation();
            void doPromote(activeGroupId, r.combo_id);
          }}
          title="Promote this combo → new rule"
          aria-label="Promote this combo → new rule"
        >
          <PromoteIcon />
        </IconButton>
      ) : null,
    [activeGroupId, doPromote, promoteState.isLoading],
  );

  const runsErr = apiErrorMessage(runsQuery.error, 'Failed to load sweep runs');
  const groupsErr = apiErrorMessage(groupsQuery.error, 'Failed to load groups');
  return (
    <div>
      <PageHeader
        title="Grouped Sweep"
        description="Configure → run → promote · drill into combos/tokens"
        actions={
          <Badge variant="primary" className="font-mono">
            {runs.length} runs · {groups.length} groups
          </Badge>
        }
      />

      {/* Sticky drill path. Rendered only once a group is picked — before that the
          group table is the whole page and the bar would say nothing. This is the
          ONE on-screen copy of the group label; the combos section below must not
          repeat it, since only this copy survives scrolling into the token drill-in. */}
      {activeRunId && activeGroup && (
        <nav
          aria-label="Sweep drill path"
          className="sticky top-14 z-40 mb-3 flex flex-wrap items-center gap-1.5 rounded-md border border-white/8 bg-bg/90 px-3 py-2 text-[12px] backdrop-blur-md"
        >
          <button
            type="button"
            className="font-semibold text-primary hover:underline"
            onClick={() => {
              setActiveGroupId(null);
              setActiveComboId(null);
            }}
          >
            Run
          </button>
          <span className="text-text-dim">›</span>
          <button
            type="button"
            className="max-w-md truncate font-medium text-text hover:underline"
            title={groupBreadcrumb(activeGroup)}
            onClick={() => setActiveComboId(null)}
          >
            {groupBreadcrumb(activeGroup)}
          </button>
          <span className="font-mono text-text-dim">· {activeGroup.token_count} tokens</span>
          {activeComboId !== null && (
            <>
              <span className="text-text-dim">›</span>
              <button
                type="button"
                className="font-mono text-secondary hover:underline"
                title="Scroll to this combo's per-token results"
                onClick={() =>
                  comboTokensRef.current?.scrollIntoView({ block: 'start', behavior: 'smooth' })
                }
              >
                combo #{activeComboId}
              </button>
              {/* Absent whenever the selected combo sits on a page the server-paged
                  table isn't holding — show nothing rather than a stale/zero PnL. */}
              {activeCombo && (
                <span className={cn('font-mono', signedToneClass(activeCombo.total_pnl_sol))}>
                  {formatSigned(activeCombo.total_pnl_sol, 2)} SOL
                  <span className="ml-1 text-text-dim">
                    · {(activeCombo.win_rate * 100).toFixed(0)}% win · {activeCombo.n_fired} fired
                  </span>
                </span>
              )}
            </>
          )}
        </nav>
      )}

      <Accordion title="Configure sweep" defaultOpen={runs.length === 0}>
        <div ref={formRef}>
          <GenericSweepConfigForm
            storageKey={`${STORAGE_KEYS.sweepConfig}.generic`}
            running={sweepRunning}
            onRun={run}
            reuseNonce={reuseNonce}
            reuseRun={activeRun}
          />
        </div>
      </Accordion>

      {startErr && <InlineAlert variant="error">{startErr}</InlineAlert>}
      {promoteErr && <InlineAlert variant="error">{promoteErr}</InlineAlert>}
      {runsQuery.isLoading && <p className="text-text-dim">Loading sweep runs…</p>}
      {runsErr && <InlineAlert variant="error">{runsErr}</InlineAlert>}

      {!runsQuery.isLoading && !runsErr && runs.length === 0 && (
        <div className="rounded-md border border-white/10 bg-surface p-3 text-sm text-text-dim">
          No grouped sweeps yet. Set a date range + grouping + axes above and click{' '}
          <span className="text-primary">Run grouped sweep</span>.
        </div>
      )}

      {runs.length > 0 && (
        <>
          <SectionDivider />
          {deleteErr && <InlineAlert variant="error">{deleteErr}</InlineAlert>}
          {/* A prune that matched nothing leaves the page byte-identical — say so,
              or a working button is indistinguishable from a broken one. */}
          {pruneNote && <InlineAlert variant={pruneNote.tone}>{pruneNote.text}</InlineAlert>}
          {groupsErr && <InlineAlert variant="error">{groupsErr}</InlineAlert>}

          {activeRun && (
            <Accordion
              toggleLabel="Run details section : "
              header={
                <div className="flex flex-1 flex-wrap items-center gap-2.5">
                  <label className="text-sm text-text-dim" htmlFor="generic-sweep-run">Run</label>
                  <select
                    id="generic-sweep-run"
                    className="rounded-md border border-white/10 bg-surface px-2.5 py-1.5 text-sm text-primary font-mono tabular-nums"
                    value={activeRunId ?? ''}
                    onChange={(e) => selectRun(e.target.value)}
                  >
                    {runs.map((r) => (
                      <option key={r.id} value={r.id}>{runPickerLine(r)}</option>
                    ))}
                  </select>
                  <IconButton
                    variant="danger"
                    size="md"
                    disabled={!activeRunId || deleteState.isLoading}
                    onClick={onDeleteRun}
                    title={deleteState.isLoading ? 'Deleting…' : 'Delete Run'}
                    aria-label={deleteState.isLoading ? 'Deleting…' : 'Delete Run'}
                  >
                    {deleteState.isLoading ? <SpinnerIcon /> : <TrashIcon />}
                  </IconButton>
                  <span className="ml-auto flex items-center gap-2">
                    <span className="text-sm text-text-dim">Clear runs before</span>
                    <DatePicker
                      aria-label="Clear runs before"
                      size="sm"
                      emptyLabel="Pick date"
                      max={todayLocalIsoDate()}
                      value={pruneBefore}
                      onChange={(next) => {
                        setPruneBefore(next);
                        setPruneNote(null);
                      }}
                      className="text-primary"
                    />
                    <IconButton
                      variant="danger"
                      size="md"
                      disabled={!pruneBefore || pruneState.isLoading}
                      onClick={onPrune}
                      title={
                        pruneState.isLoading
                          ? 'Clearing…'
                          : 'Clear runs created before this date (local midnight)'
                      }
                      aria-label={pruneState.isLoading ? 'Clearing…' : 'Clear all old runs'}
                    >
                      {pruneState.isLoading ? <SpinnerIcon /> : <TrashIcon />}
                    </IconButton>
                  </span>
                </div>
              }
              defaultOpen={false}
              storageKey={ACCORDION_IDS.sweepDetailsGeneric}
              className="mb-3"
            >
              <SelectedSweepHistory
                strategyId={strategyId}
                run={activeRun}
                tokensDone={tokensDone}
                onReuse={() => {
                  setReuseNonce((n) => n + 1);
                  formRef.current?.scrollIntoView({ behavior: 'smooth', block: 'start' });
                }}
              />
            </Accordion>
          )}

          {/* A run priced under the legacy pair charges slippage twice: once inside
              the fill price, once again as `slippage_bps`. Because the fixed cost is
              per leg, that haircut scales with how often a combo fires — so it does
              not just shift every PnL down, it re-orders the ranking. Say so where
              the ranking is read, not only on the run header. */}
          {activeRun && isLegacyCostModel(activeRun.cost_model) && (
            <InlineAlert variant="warning">
              Priced with the retired cost model (fee + slippage) on top of{' '}
              {activeRun.fill_model ?? 'worst_case'} fills, which already price slippage — so
              execution cost is counted twice, and not evenly across combos. Re-run with
              “Fee + real impact” before acting on this ranking.
            </InlineAlert>
          )}

          {activeRun && activeRun.status !== 'completed' && (
            <InlineAlert variant="warning">
              {activeRun.status === 'running' ? 'In-progress' : 'Partial'} run —{' '}
              {(sweepJob && sweepJob.runId === activeRun.id ? sweepJob.groupsDone : null) ??
                activeRun.groups_done}{' '}
              of{' '}
              {(sweepJob && sweepJob.runId === activeRun.id ? sweepJob.groupCount : null) ??
                activeRun.group_count}{' '}
              groups
              {activeRun.status === 'running'
                ? ' persisted so far — each is browsable below as soon as it lands.'
                : activeRun.status === 'partial'
                  ? ' persisted before a write error ended the run — this is not a full sweep.'
                  : ' completed before the run was cancelled — this is not a full sweep.'}
            </InlineAlert>
          )}

          <DataTable
            columns={groupColumns}
            rows={groups}
            rowKey={sweepGroupRowKey}
            rowActions={groupRowActions}
            groupLabels={{
              group: 'Group',
              counts: 'Counts',
              pnl: 'PnL (best combo)',
              holding: 'Holding',
              params: 'Best rule',
            }}
            defaultSort={{ col: 'best_total_pnl_sol', dir: 'desc' }}
            searchable
            colFilters
            colToggle
            selectable
            selectedKey={activeGroupId}
            onSelect={setActiveGroupId}
            sameValueTints
            tableId="generic_sweep_groups"
            pinnable
            resetKey={activeRunId ?? ''}
            loading={groupsQuery.isFetching}
            emptyMessage="No groups cleared the min-tokens threshold for this run."
          />

          {activeGroupId && (
            <div className="mt-4">
              {/* No group label here on purpose — the sticky breadcrumb above owns it
                  (see the nav), and it stays visible once this table scrolls away. */}
              <div className="mb-2 flex flex-wrap items-center gap-2">
                <h3 className="text-sm font-bold text-secondary">Combos for group</h3>
              </div>

              {resultsErr && <InlineAlert variant="error">{resultsErr}</InlineAlert>}

              <DataTable
                columns={comboColumns}
                rows={results}
                rowKey={sweepComboRowKey}
                rowActions={comboRowActions}
                groupLabels={{ params: 'Rule', counts: 'Counts', pnl: 'PnL', holding: 'Holding', exits: 'Exit reasons' }}
                searchable={false}
                // Per-column filters (numeric ops `>5` / `1..10`) are forwarded to the
                // server via `onComboQueryChange` → `useStreamedSweepResults`, and the
                // filtered `X-Total-Count` keeps the pager honest.
                colFilters
                selectable
                selectedKey={activeComboId !== null ? String(activeComboId) : null}
                onSelect={(key) => setActiveComboId(key !== null ? Number(key) : null)}
                sameValueTints
                serverSide
                serverTotal={resultsTotal}
                onQueryChange={onComboQueryChange}
                tableId="generic_sweep_combos"
                pinnable
                resetKey={activeGroupId ?? ''}
                loading={resultsLoading}
                emptyMessage="No combo results for this group."
              />

              {activeComboId !== null && activeGroupId && (
                <div ref={comboTokensRef} className="scroll-mt-28">
                  <ComboTokenResults
                    strategyId={strategyId}
                    runId={activeRunId ?? ''}
                    groupId={activeGroupId}
                    comboId={activeComboId}
                    comboParams={activeCombo?.params ?? null}
                    ixPatterns={activeRun?.ix_patterns ?? null}
                    inspectFingerprintId={
                      promotedFp?.groupId === activeGroupId ? promotedFp.fingerprintId : null
                    }
                    onClose={() => setActiveComboId(null)}
                  />
                </div>
              )}
            </div>
          )}
        </>
      )}

      <PromoteRuleModal draft={promoteDraft} onClose={() => setPromoteDraft(null)} />
    </div>
  );
}

// --- per-token drill-in -----------------------------------------------------

/** The re-simulated per-token results for one combo (fired / PnL / exit) + a
 *  click-through to the token chart with entry/exit markers. */
function ComboTokenResults({
  strategyId,
  runId,
  groupId,
  comboId,
  comboParams,
  ixPatterns,
  inspectFingerprintId,
  onClose,
}: {
  strategyId: string;
  runId: string;
  groupId: string;
  comboId: number;
  /** The combo's swept `RuleParams` blob — pins the inspect's metric panes to the
   *  exact params that produced these rows. Null when the row paged out of view. */
  comboParams: Record<string, unknown> | null;
  /** Corpus-wide run patterns — chart overlay before Promote creates a fingerprint. */
  ixPatterns: string[][] | null;
  /** Promoted fingerprint for this group — enables flow metric-series panes. */
  inspectFingerprintId: string | null;
  onClose: () => void;
}) {
  const { timezone } = useTimezone();
  const flowPatternKeys = useMemo(
    () => flowPatternKeysOf(ixPatterns),
    [ixPatterns],
  );
  const query = useGetComboTokenResultsQuery({ strategyId, runId, groupId, comboId });
  // Stable identities: RTK keeps `query.data` referentially equal across renders, so
  // memoizing keeps `rows`/`visible` from churning — otherwise the filtered-rows
  // callback below (which sets state) would re-fire every render into a loop.
  const rows = useMemo(() => query.data?.rows ?? [], [query.data]);
  const err = apiErrorMessage(query.error, 'Failed to load token results');
  // Default OFF: a combo drill-in is read as the positions the combo TOOK. The
  // matched-but-not-entered `NoEntry` rows usually outnumber them and bury the
  // result — toggle on to audit what the entry gate rejected. (Simulate keeps
  // the opposite default: there the point is auditing the match set.)
  const [showNotFired, setShowNotFired] = useLocalStorage(
    STORAGE_KEYS.sweepShowNotFired,
    false,
  );
  const visible = useMemo(
    () => (showNotFired ? rows : rows.filter((r) => r.fired)),
    [showNotFired, rows],
  );
  const [selected, setSelected] = useState<string | null>(null);
  const [focus, setFocus] = useState<PositionFocusLens[]>([]);
  /** Table-filter cohort pinned when focus activates so charts/summary base don't collapse. */
  const [cohortPinned, setCohortPinned] = useState<ComboTokenResult[] | null>(null);
  useEffect(() => {
    setSelected(null);
    setFocus([]);
    setCohortPinned(null);
  }, [comboId]);

  const [filteredRows, setFilteredRows] = useState<ComboTokenResult[] | null>(null);
  useEffect(() => setFilteredRows(null), [comboId]);
  const onFilteredRowsChange = useCallback(
    (r: ComboTokenResult[]) => {
      // Only track column/search filters while unfocused — once focus is on, the
      // table's row set is already narrowed and would collapse the chart base.
      if (focus.length === 0) setFilteredRows(r);
    },
    [focus.length],
  );

  const onFocusChange = useCallback(
    (next: PositionFocusLens[]) => {
      if (next.length > 0 && focus.length === 0) {
        setCohortPinned(filteredRows ?? visible);
      }
      if (next.length === 0) setCohortPinned(null);
      setFocus(next);
    },
    [focus.length, filteredRows, visible],
  );

  const tableFilterCohort = cohortPinned ?? filteredRows ?? visible;
  const focusOpts = useMemo(() => ({ timeZone: timezone }), [timezone]);
  const rowsForTable = useMemo(() => {
    if (focus.length === 0) return visible;
    const keep = new Set(
      filterRowsByFocus(visible.map(comboFocusRow), focus, focusOpts).map((r) => r.id!),
    );
    return visible.filter((r) => keep.has(episodeRowKey(r)));
  }, [visible, focus, focusOpts]);

  const summaryRun = useMemo(() => {
    const cohort =
      focus.length === 0
        ? tableFilterCohort
        : filterRowsByFocus(tableFilterCohort.map(comboFocusRow), focus, focusOpts).flatMap(
            (fr) => {
              const row = tableFilterCohort.find((r) => episodeRowKey(r) === fr.id);
              return row ? [row] : [];
            },
          );
    const migrated = cohort.filter((r) => r.fired && r.is_migrated).length;
    return { summary: runSummaryFromRows(cohort), migrated };
  }, [tableFilterCohort, focus, focusOpts]);

  const chartPoints = useMemo(
    () => comboChartPoints(tableFilterCohort),
    [tableFilterCohort],
  );

  const temporalRows = useMemo(
    () =>
      tableFilterCohort.map((r) => ({
        mint_address: r.mint_address,
        fired: r.fired,
        exit: r.exit,
        pnl_sol: r.pnl_sol,
        holding_secs: r.holding_secs,
        entry_time: r.entry_time,
        created_at: r.created_at,
        exit_time: r.exit_time,
      })),
    [tableFilterCohort],
  );

  // A table row selects by episode key; a grouped charts-grid card by mint.
  const selectedRow = selected
    ? rows.find((r) => episodeRowKey(r) === selected) ??
      rows.find((r) => r.mint_address === selected) ??
      null
    : null;

  // Overlay every re-entry episode of the selected token on one chart: the combo's
  // full token-result set is already resident client-side, so gather all fired rows
  // for that mint and build the union of their entry/exit markers (numbered per
  // episode). Falls back to the single clicked row when it's a one-shot token.
  const selectedEpisodeMarkers = useMemo(() => {
    if (!selectedRow) return null;
    const eps = rows
      .filter((r) => r.mint_address === selectedRow.mint_address && r.fired)
      .map(comboTarget);
    return buildEventMarkersForEpisodes(eps.length ? eps : [comboTarget(selectedRow)]);
  }, [rows, selectedRow]);

  const columns = useMemo<ColumnDef<ComboTokenResult>[]>(
    () => [
      {
        key: 'symbol',
        label: 'Symbol',
        render: (r) => <span className="font-mono text-xs">{r.symbol || '—'}</span>,
        searchValue: (r) => r.symbol,
        sortValue: (r) => r.symbol,
        sortable: true,
      },
      {
        key: 'mint_address',
        label: 'Mint',
        render: (r) => (
          <span className="font-mono text-xs text-text-dim" title={r.mint_address}>
            {r.mint_address.slice(0, 8)}…{r.mint_address.slice(-4)}
          </span>
        ),
        searchValue: (r) => r.mint_address,
      },
      {
        key: 'fired',
        label: 'Fired',
        render: (r) => <span className={r.fired ? 'text-success' : 'text-text-dim'}>{r.fired ? 'Yes' : 'No'}</span>,
        searchValue: (r) => (r.fired ? 'yes' : 'no'),
        sortValue: (r) => (r.fired ? 1 : 0),
        sortable: true,
      },
      {
        key: 'pnl_sol',
        label: 'PnL (SOL)',
        render: (r) => (
          <span className={cn('tabular-nums', !r.fired || r.pnl_sol == null ? 'text-text-dim' : signedToneClass(r.pnl_sol))}>
            {r.fired && r.pnl_sol != null ? r.pnl_sol.toFixed(4) : '—'}
          </span>
        ),
        searchValue: () => '',
        filterNumber: (r) => (r.fired ? r.pnl_sol : null),
        sortValue: (r) => r.pnl_sol,
        sortable: true,
      },
      {
        key: 'pnl_pct',
        label: 'PnL %',
        render: (r) => {
          if (!r.fired || r.pnl_pct == null) return <span className="text-text-dim">—</span>;
          return (
            <span className={cn('tabular-nums', pctGradeClass(r.pnl_pct))}>
              {formatSignedPct(r.pnl_pct, 1)}
            </span>
          );
        },
        searchValue: () => '',
        filterNumber: (r) => (r.fired ? r.pnl_pct : null),
        sortValue: (r) => r.pnl_pct,
        sortable: true,
      },
      {
        key: 'holding_secs',
        label: 'Hold (s)',
        render: (r) => <span className="text-text-dim">{r.fired ? r.holding_secs : '—'}</span>,
        searchValue: () => '',
        filterNumber: (r) => (r.fired ? r.holding_secs : null),
        sortValue: (r) => r.holding_secs,
        sortable: true,
      },
      {
        key: 'exit',
        label: 'Exit',
        render: (r) => <span className="font-mono text-xs">{r.exit}</span>,
        searchValue: (r) => r.exit,
        sortValue: (r) => r.exit,
        sortable: true,
      },
    ],
    [],
  );
  // The combo's own column keys — everything else in the shared token-info set is
  // appended by TokenTable so the drill-in carries the full enrichment columns.
  const existingKeys = useMemo(() => new Set(columns.map((c) => c.key)), [columns]);

  return (
    <div className="mt-4">
      <div className="mb-2 flex flex-wrap items-center gap-2">
        <h3 className="text-sm font-bold text-secondary">Tokens for combo #{comboId}</h3>
        <span className="text-xs text-text-dim">
          {query.isFetching ? 'Simulating…' : `${visible.length}${!showNotFired ? ` / ${rows.length}` : ''} tokens`}
        </span>
        <div className="grow" />
        <VisibilityToggleButton visible={showNotFired} onToggle={() => setShowNotFired((v) => !v)} label="not-fired tokens">
          {showNotFired ? 'Hide not fired' : 'Show not fired'}
        </VisibilityToggleButton>
        <button className="text-xs text-text-dim hover:text-primary" onClick={onClose}>✕ Close</button>
      </div>

      {err && <InlineAlert variant="error">{err}</InlineAlert>}

      {!err && rows.length > 0 && (
        <PositionSummarySection
          title="Combo results summary"
          subtitle="Tracks the table's filters + focus"
          summary={summaryRun.summary}
          summaryExtras={{ migrated: summaryRun.migrated }}
          chartPoints={chartPoints}
          focus={focus}
          onFocusChange={onFocusChange}
          accentClass="bg-secondary"
          temporal={{ rows: temporalRows }}
        />
      )}

      <TokenTable
        columns={columns}
        existingKeys={existingKeys}
        charts
        chartsDefaultOn
        flowPatternKeys={flowPatternKeys}
        // A finished run's numbers were computed under the run's OWN stored
        // ix_patterns, which are a snapshot rather than a live fingerprint
        // row — so they are readable here but not editable, or a Tagged-badge click
        // would silently retarget some unrelated fingerprint.
        flowReadOnly
        useRowOverlay={comboRowOverlay}
        chartsGroupByMint
        // `rowsForTable` is this combo's FULL (already client-loaded) row set, not
        // just the grid's current page — group over it directly by mint instead of
        // the page-scoped `gr` the grid hands back, so a token re-entered on a
        // different page than its other episodes still overlays all of them.
        mintChartGroupOverlay={(_gr, mint) => ({
          eventMarkers: buildEventMarkersForEpisodes(
            rowsForTable.filter((r) => r.fired && r.mint_address === mint).map(comboTarget),
          ),
        })}
        renderChartCardExtra={(row) => (
          // Full-mint cohort (same reason as mintChartGroupOverlay) so episode
          // count / PnL sum aren't truncated to the current page.
          <PositionChartCardExtra
            facts={positionChartFactsFromEpisodes(
              rowsForTable
                .filter((r) => r.mint_address === row.mint_address)
                .map(comboChartEpisode),
            )}
          />
        )}
        rows={rowsForTable}
        rowKey={episodeRowKey}
        searchable
        colFilters
        selectable
        selectedKey={selected}
        onSelect={setSelected}
        onFilteredRowsChange={onFilteredRowsChange}
        sameValueTints
        defaultSort={{ col: 'pnl_sol', dir: 'desc' }}
        tableId="generic_combo_tokens"
        defaultCols={COMBO_TOKENS_DEFAULT_COLS}
        resetKey={`${comboId}_${showNotFired}`}
        loading={query.isFetching}
        emptyMessage="No token results for this combo."
      />

      {selectedRow && (
        <LazyLabTokenInspectModal
          target={comboTarget(selectedRow)}
          titleSuffix="Sweep inspect"
          flowPatternKeys={flowPatternKeys}
          flowReadOnly
          ruleOverride={
            comboParams
              ? {
                  paramsJson: comboParams,
                  // Flow panes need a fingerprint with metric_config — available
                  // after Promote for this group (run patterns → FP).
                  fingerprintId: inspectFingerprintId,
                  label: `combo #${comboId}`,
                }
              : null
          }
          eventMarkers={selectedEpisodeMarkers}
          onClose={() => setSelected(null)}
        />
      )}
    </div>
  );
}

/** One-line "what this group is" for the drill-in header + inspect breadcrumb.
 *  Reads the backend's resolved selection (scope ∧ run filters ∧ group key) —
 *  the group key alone can't see a pinned corpus, which is what made a filtered
 *  run read as "ALL tokens". */
function groupBreadcrumb(g: GroupedSweepGroupRecord): string {
  const clauses = g.selection?.clauses;
  if (clauses) {
    return clauses.length === 0
      ? 'ALL tokens'
      : clauses.map((c) => `${c.field}=${c.display}`).join(' · ');
  }
  const keys = Object.keys(g.group_key);
  if (keys.length === 0) return 'ALL tokens';
  return Object.entries(g.group_key)
    .map(([k, v]) => `${k}=${v}`)
    .join(' · ');
}
