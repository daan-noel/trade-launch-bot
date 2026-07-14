import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { useLocalStorage } from 'hooks/useLocalStorage';
import { STORAGE_KEYS } from 'lib/storage';
import { DataTable } from 'components/table/DataTable';
import { TokenTable } from 'components/tokens/TokenTable';
import { ALL_TOKEN_INFO_KEYS } from 'components/tokens/sharedTokenColumns';
import { InlineAlert } from 'components/ui/Modal';
import { SectionDivider } from 'components/ui/SectionDivider';
import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { Accordion } from 'components/ui/Accordion';
import { useBackgroundJobActions, useBackgroundJobsState } from '@lab/context/BackgroundJobsContext';
import { buildSweepColumns } from '@lab/components/sweep/sweepColumns';
import { buildGroupColumns } from '@lab/components/sweep/groupColumns';
import { computeParamColumnColors, computePnlColumnColors } from 'lib/sweepParamColors';
import { SweepConfigForm } from '@lab/components/sweep/SweepConfigForm';
import { SelectedSweepHistory } from '@lab/components/sweep/SelectedSweepHistory';
import { TokenInspectModal } from 'components/tpsl2/TokenInspectModal';
import { Swing1InspectModal } from '@lab/pages/strategies/sweep/Swing1InspectModal';
import type { InspectTarget } from 'components/strategy/inspectTarget';
import { makeSwing1DetectRowOverlay } from '@lab/hooks/useSwing1DetectOverlay';
import type { ChartOverlayHook } from 'components/tokens/TokenChartsGrid';
import type { Swing1DetectParams } from '@lab/services/swing1Detect';
import { VisibilityToggleButton } from 'components/ui/VisibilityToggleButton';
import {
  serializeGroupFingerprintJson,
  type AxisDef,
  type GroupedSweepStartArgs,
  type GroupedSweepRunRecord,
  type GroupedSweepGroupRecord,
  type ComboTokenResult,
} from '@lab/components/sweep/groupedTypes';
import { apiErrorMessage } from 'store/apiSlice';
import {
  useGetGroupedSweepRunsQuery,
  useGetGroupedSweepGroupsQuery,
  useGetComboTokenResultsQuery,
  useStartGroupedSweepMutation,
  useDeleteGroupedSweepRunMutation,
  usePruneGroupedSweepsMutation,
} from '@lab/store/labEndpoints';
import { useStreamedSweepResults, COMBO_PAGE_SIZE } from '@lab/hooks/useStreamedSweepResults';
import type { ColumnDef, SortEntry, TableQuery } from 'components/table/types';
import { getSpec, serializeComboJson, type Strategy } from 'lib/params';
import type { SweepResultRecord } from '@lab/components/sweep/types';

/** The grouped-sweep view is strategy-agnostic — the API/data layer and column
 *  builders are all driven by `strategyId` + a swept-param-key list. Each
 *  strategy supplies its own keys + axes via a thin child page (see this
 *  folder's `Tpsl1GroupedSweepPage` / `Tpsl2GroupedSweepPage`). */


/** Map a combo token-result row to an inspect target — one SSOT for the inspect
 *  modal and the charts-grid overlay (both mark the same entry/exit). */
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

/** Run-picker groups label: a completed run shows its full group count; a running
 *  or cancelled (partial) run shows "done / total" so the picker reveals at a
 *  glance that it isn't a full sweep. */
function runGroupsLabel(r: GroupedSweepRunRecord): string {
  if (r.status === 'completed') return `${r.group_count} groups`;
  const tag = r.status === 'running' ? 'running' : 'partial';
  return `${tag} ${r.groups_done}/${r.group_count} groups`;
}

export interface GroupedSweepViewProps {
  /** Resolves the per-strategy backend tables + sweep entry point. */
  strategyId: string;
  /** Swept-param keys, in column order (matches the backend `params_json`). */
  paramKeys: string[];
  /** This strategy's editable param axes for the config form. */
  axes: AxisDef[];
  /** localStorage key for this strategy's persisted form config. */
  storageKey: string;
  /** Page heading. */
  title: string;
  /** Optional per-strategy advisory over the parsed axis spec (see
   *  `SweepConfigForm`). swing1 passes its kill/volume band-overlap check. */
  axesWarning?: (spec: Record<string, (number | null)[]>) => string | null;
}

/**
 * Grouped param-sweep view: select tokens by a created-at range, partition them
 * by a fingerprint key, sweep each group, and rank combos by expectancy per
 * trade. Flow: configure + Run → pick a run → group-summary table → click a
 * group → drill into its full ranked combo table.
 */
export function GroupedSweepView({
  strategyId,
  paramKeys,
  axes,
  storageKey,
  title,
  axesWarning,
}: GroupedSweepViewProps) {
  const runsQuery = useGetGroupedSweepRunsQuery({ strategyId });
  const runs = runsQuery.data ?? [];

  const [selectedRunId, setSelectedRunId] = useLocalStorage<string | null>(
    `${STORAGE_KEYS.sweepSel}.${strategyId}`,
    null,
  );
  // Fall back to the newest run when the stored id is stale (run deleted) or
  // nothing has been selected yet.
  const activeRunId = (selectedRunId && runs.some((r) => r.id === selectedRunId))
    ? selectedRunId
    : (runs[0]?.id ?? null);

  const [activeGroupId, setActiveGroupId] = useState<string | null>(null);
  // A new run invalidates the drilled-in group.
  useEffect(() => {
    setActiveGroupId(null);
  }, [activeRunId]);

  const [activeComboId, setActiveComboId] = useState<number | null>(null);
  // A new group invalidates the drilled-in combo.
  useEffect(() => {
    setActiveComboId(null);
  }, [activeGroupId]);

  const [copiedComboId, setCopiedComboId] = useState<number | null>(null);
  const comboRowActions = useCallback(
    (row: SweepResultRecord): ReactNode => {
      const isCopied = copiedComboId === row.combo_id;
      return (
        <button
          type="button"
          onClick={async (e) => {
            e.stopPropagation();
            const json = serializeComboJson(getSpec(strategyId as Strategy), row);
            try {
              await navigator.clipboard.writeText(json);
              setCopiedComboId(row.combo_id);
              setTimeout(() => setCopiedComboId(null), 1500);
            } catch { /* ignore */ }
          }}
          title="Copy combo params to clipboard"
          className={isCopied ? 'text-green' : 'text-text-dim hover:text-text'}
        >
          {isCopied ? '✓' : '⎘'}
        </button>
      );
    },
    [copiedComboId, strategyId],
  );

  // Copy a group's fingerprint as a paste-able rule-params blob (Token
  // Fingerprint section on the rules page). Mirrors the combo ⎘ button.
  const [copiedGroupId, setCopiedGroupId] = useState<string | null>(null);
  const groupRowActions = useCallback(
    (group: GroupedSweepGroupRecord): ReactNode => {
      const isCopied = copiedGroupId === group.id;
      return (
        <button
          type="button"
          onClick={async (e) => {
            e.stopPropagation();
            const json = serializeGroupFingerprintJson(strategyId as Strategy, group);
            try {
              await navigator.clipboard.writeText(json);
              setCopiedGroupId(group.id);
              setTimeout(() => setCopiedGroupId(null), 1500);
            } catch { /* ignore */ }
          }}
          title="Copy this group's fingerprint as rule params (paste into a rule)"
          className={isCopied ? 'text-green' : 'text-text-dim hover:text-text'}
        >
          {isCopied ? '✓' : '⎘'}
        </button>
      );
    },
    [copiedGroupId, strategyId],
  );

  const [startSweep, startState] = useStartGroupedSweepMutation();
  const startErr = apiErrorMessage(startState.error, 'Failed to start sweep');
  // The sweep's running/progress state lives in the app-wide jobs registry so it
  // survives navigation (the run continues on the backend regardless); the global
  // indicator renders the progress bar + cancel. The page only needs "is it
  // running" to gate the form.
  const { markStarting } = useBackgroundJobActions();
  const { isRunning } = useBackgroundJobsState();
  const sweepRunning = isRunning('sweep', 'sweep') || startState.isLoading;

  const [deleteRun, deleteState] = useDeleteGroupedSweepRunMutation();
  const [pruneRuns, pruneState] = usePruneGroupedSweepsMutation();
  const deleteErr = apiErrorMessage(
    deleteState.error ?? pruneState.error,
    'Failed to delete sweep history',
  );
  // "older than" cutoff for the prune control (a yyyy-mm-dd date).
  const [pruneBefore, setPruneBefore] = useState('');

  // Re-run: bumping this nonce makes the config form adopt the selected run's
  // stored settings; `formRef` scrolls it back into view so the user can review
  // before clicking Run (re-run never auto-fires — a sweep is expensive).
  const [reuseNonce, setReuseNonce] = useState(0);
  const formRef = useRef<HTMLDivElement>(null);

  async function onDeleteRun() {
    if (!activeRunId) return;
    if (!window.confirm('Delete this sweep run and all its groups/results?')) return;
    try {
      await deleteRun({ strategyId, runId: activeRunId }).unwrap();
      setSelectedRunId(null); // fall back to the newest remaining run
    } catch {
      // Surfaced via deleteErr.
    }
  }

  async function onPrune() {
    if (!pruneBefore) return;
    const beforeIso = new Date(pruneBefore).toISOString();
    if (!window.confirm(`Delete all sweep runs created before ${pruneBefore}?`)) return;
    try {
      await pruneRuns({ strategyId, before: beforeIso }).unwrap();
      setSelectedRunId(null);
    } catch {
      // Surfaced via deleteErr.
    }
  }

  async function run(args: GroupedSweepStartArgs) {
    // Register the job immediately so the global indicator shows before the first
    // SSE frame (which only arrives once the backend finishes corpus selection).
    markStarting('sweep', 'sweep', 'Grouped sweep');
    try {
      // The backend returns as soon as the run is admitted (`202 { run_id }`)
      // instead of holding this request open for the whole sweep — that keeps a
      // later Cancel POST from queueing behind it on the browser's per-host
      // connection cap. Jump straight to the new run; it fills in live via the
      // per-group writes + SSE progress, and `connectSweepFinished` refreshes the
      // runs list on completion/cancel.
      const { run_id } = await startSweep(args).unwrap();
      setSelectedRunId(run_id);
    } catch {
      // Surfaced via startState.error (e.g. 409 = one already running).
    }
  }

  const groupsQuery = useGetGroupedSweepGroupsQuery(
    { strategyId, runId: activeRunId ?? '' },
    { skip: !activeRunId },
  );
  const groups = groupsQuery.data ?? [];
  const activeGroup = groups.find((g) => g.id === activeGroupId) ?? null;
  const activeRun = runs.find((r) => r.id === activeRunId) ?? null;
  // Tokens covered by the persisted groups (Σ group token_count). For a partial
  // run this is below the run's total token_count; the history Population row shows
  // it as done/total. `null` while groups are still loading so the row falls back
  // to the plain total instead of flashing "0/total".
  const tokensDone = useMemo(
    () => (groups.length ? groups.reduce((sum, g) => sum + g.token_count, 0) : null),
    [groups],
  );

  // Server-side pagination state for the combo table. `onComboQueryChange` is
  // stable (useCallback) so DataTable's onQueryChange prop identity doesn't
  // churn on every parent re-render, which would reset the pager.
  const [comboPage, setComboPage] = useState(0);
  const [comboPageSize, setComboPageSize] = useState(COMBO_PAGE_SIZE);
  const [comboSortKeys, setComboSortKeys] = useState<SortEntry[]>([]);
  const onComboQueryChange = useCallback((q: TableQuery) => {
    setComboPage(q.page - 1); // DataTable pages are 1-based; hook is 0-based
    setComboPageSize(q.pageSize);
    setComboSortKeys(q.sortKeys);
  }, []);

  // Reset to page 0 whenever the selected group changes.
  useEffect(() => {
    setComboPage(0);
  }, [activeGroupId]);

  const { rows: results, total: resultsTotal, loading: resultsLoading, error: resultsErr } =
    useStreamedSweepResults(strategyId, activeRunId, activeGroupId, comboPage, comboPageSize, comboSortKeys);

  const groupColumns = useMemo(() => buildGroupColumns(paramKeys), [paramKeys]);
  // Per-column tint plan for the drill-in combo table: constant knobs dim out,
  // varying knobs get a per-value cell band so near-identical combos read at a
  // glance. Recomputed per group (cheap, O(rows×params)).
  const paramColors = useMemo(
    () => computeParamColumnColors(results, paramKeys),
    [results, paramKeys],
  );
  const pnlColors = useMemo(
    () => computePnlColumnColors(results),
    [results],
  );
  const comboColumns = useMemo(
    () => buildSweepColumns(paramKeys, paramColors, pnlColors),
    [paramKeys, paramColors, pnlColors],
  );

  const tokenResultsQuery = useGetComboTokenResultsQuery(
    {
      strategyId,
      runId: activeRunId ?? '',
      groupId: activeGroupId ?? '',
      comboId: activeComboId ?? 0,
    },
    { skip: !activeRunId || !activeGroupId || activeComboId === null },
  );
  const tokenResults = tokenResultsQuery.data ?? [];
  const tokenResultsErr = apiErrorMessage(tokenResultsQuery.error, 'Failed to load token results');

  const [showNotFired, setShowNotFired] = useState(true);
  const visibleTokenResults = showNotFired ? tokenResults : tokenResults.filter((r) => r.fired);

  const [selectedTokenMint, setSelectedTokenMint] = useState<string | null>(null);
  // Clear the token selection when the combo changes.
  useEffect(() => {
    setSelectedTokenMint(null);
  }, [activeComboId]);

  const selectedTokenResult = selectedTokenMint
    ? (tokenResults.find((r) => r.mint_address === selectedTokenMint) ?? null)
    : null;

  // The drilled-in combo's swept params — fed to the swing1 inspect modal so the
  // chart's swing overlay matches the exact funnel this combo simulated. Only
  // meaningful for `swing_1`; other strategies show no swing overlay.
  const activeComboParams = useMemo(
    () =>
      activeComboId !== null
        ? (results.find((r) => r.combo_id === activeComboId)?.params ?? null)
        : null,
    [results, activeComboId],
  );

  // Charts-grid overlay for the combo's token results: entry/exit always; swing1
  // combos also draw their detected legs (keyed off the combo's swept params, the
  // same ones its inspect modal fetches). Always the detect factory so the card's
  // hook shape is constant — `null` params (non-swing, or params not yet loaded)
  // just no-ops the detect fetch, leaving entry/exit markers.
  const comboSwingParams =
    strategyId === 'swing_1' ? (activeComboParams as Swing1DetectParams | null) : null;
  const comboRowOverlay = useMemo<ChartOverlayHook<ComboTokenResult>>(
    () => makeSwing1DetectRowOverlay(comboTarget, comboSwingParams),
    [comboSwingParams],
  );

  const tokenColumns = useMemo<ColumnDef<ComboTokenResult>[]>(
    () => [
      // --- Identity ---
      {
        key: 'symbol',
        label: 'Symbol',
        group: 'identity',
        render: (r) => <span className="font-mono text-xs">{r.symbol || '—'}</span>,
        searchValue: (r) => r.symbol,
        filterValue: (r) => r.symbol,
        sortValue: (r) => r.symbol,
        sortable: true,
      },
      {
        key: 'mint_address',
        label: 'Mint',
        group: 'identity',
        render: (r) => (
          <span className="font-mono text-xs text-text-dim" title={r.mint_address}>
            {r.mint_address.slice(0, 8)}…{r.mint_address.slice(-4)}
          </span>
        ),
        searchValue: (r) => r.mint_address,
        filterValue: (r) => r.mint_address,
        sortable: false,
      },
      {
        key: 'creator_wallet',
        label: 'Creator',
        group: 'identity',
        render: (r) =>
          r.creator_wallet ? (
            <span className="font-mono text-xs text-text-dim" title={r.creator_wallet}>
              {r.creator_wallet.slice(0, 6)}…{r.creator_wallet.slice(-4)}
            </span>
          ) : (
            <span className="text-text-dim">—</span>
          ),
        searchValue: (r) => r.creator_wallet ?? '',
        filterValue: (r) => r.creator_wallet ?? '',
        sortValue: (r) => r.creator_wallet ?? '',
        sortable: true,
      },
      // --- Activity ---
      {
        key: 'created_at',
        label: 'Created',
        group: 'activity',
        render: (r) =>
          r.created_at ? (
            <span className="text-xs text-text-dim">
              {new Date(r.created_at).toLocaleString()}
            </span>
          ) : (
            <span className="text-text-dim">—</span>
          ),
        searchValue: () => '',
        sortValue: (r) => (r.created_at ? new Date(r.created_at).getTime() : 0),
        sortable: true,
      },
      {
        key: 'trade_count',
        label: 'Trades',
        group: 'activity',
        render: (r) => (
          <span className="text-xs text-text-dim">
            {r.trade_count != null ? r.trade_count.toLocaleString() : '—'}
          </span>
        ),
        searchValue: () => '',
        filterNumber: (r) => r.trade_count,
        sortValue: (r) => r.trade_count ?? -1,
        sortable: true,
      },
      // --- Price ---
      {
        key: 'ath_price',
        label: 'ATH',
        group: 'price',
        render: (r) => (
          <span className="text-xs text-text-dim">
            {r.ath_price != null ? r.ath_price.toExponential(3) : '—'}
          </span>
        ),
        searchValue: () => '',
        filterNumber: (r) => r.ath_price,
        sortValue: (r) => r.ath_price ?? -1,
        sortable: true,
      },
      {
        key: 'ath_timestamp',
        label: 'ATH At',
        group: 'price',
        render: (r) =>
          r.ath_timestamp ? (
            <span className="text-xs text-text-dim">
              {new Date(r.ath_timestamp).toLocaleString()}
            </span>
          ) : (
            <span className="text-text-dim">—</span>
          ),
        searchValue: () => '',
        sortValue: (r) => (r.ath_timestamp ? new Date(r.ath_timestamp).getTime() : 0),
        sortable: true,
      },
      {
        key: 'current_price',
        label: 'Price',
        group: 'price',
        render: (r) => (
          <span className="text-xs text-text-dim">
            {r.current_price != null ? r.current_price.toExponential(3) : '—'}
          </span>
        ),
        searchValue: () => '',
        filterNumber: (r) => r.current_price,
        sortValue: (r) => r.current_price ?? -1,
        sortable: true,
      },
      // --- Market ---
      {
        key: 'market_cap',
        label: 'MCap',
        group: 'market',
        render: (r) => (
          <span className="text-xs text-text-dim">
            {r.market_cap != null ? `$${(r.market_cap / 1000).toFixed(1)}k` : '—'}
          </span>
        ),
        searchValue: () => '',
        filterNumber: (r) => r.market_cap,
        sortValue: (r) => r.market_cap ?? -1,
        sortable: true,
      },
      {
        key: 'volume_sol_total',
        label: 'Vol (SOL)',
        group: 'market',
        render: (r) => (
          <span className="text-xs text-text-dim">
            {r.volume_sol_total != null ? r.volume_sol_total.toFixed(1) : '—'}
          </span>
        ),
        searchValue: () => '',
        filterNumber: (r) => r.volume_sol_total,
        sortValue: (r) => r.volume_sol_total ?? -1,
        sortable: true,
      },
      // --- Flags ---
      {
        key: 'is_migrated',
        label: 'Migrated',
        group: 'flags',
        render: (r) =>
          r.is_migrated == null ? (
            <span className="text-text-dim">—</span>
          ) : (
            <span className={r.is_migrated ? 'text-primary' : 'text-text-dim'}>
              {r.is_migrated ? 'Yes' : 'No'}
            </span>
          ),
        searchValue: (r) => (r.is_migrated ? 'yes' : 'no'),
        filterValue: (r) => (r.is_migrated ? 'yes' : 'no'),
        sortValue: (r) => (r.is_migrated ? 1 : 0),
        sortable: true,
      },
      {
        key: 'is_dead',
        label: 'Dead',
        group: 'flags',
        render: (r) =>
          r.is_dead == null ? (
            <span className="text-text-dim">—</span>
          ) : (
            <span className={r.is_dead ? 'text-danger' : 'text-text-dim'}>
              {r.is_dead ? 'Yes' : 'No'}
            </span>
          ),
        searchValue: (r) => (r.is_dead ? 'yes' : 'no'),
        filterValue: (r) => (r.is_dead ? 'yes' : 'no'),
        sortValue: (r) => (r.is_dead ? 1 : 0),
        sortable: true,
      },
      // --- Sim results ---
      {
        key: 'fired',
        label: 'Fired',
        group: 'sim',
        render: (r) => (
          <span className={r.fired ? 'text-success' : 'text-text-dim'}>
            {r.fired ? 'Yes' : 'No'}
          </span>
        ),
        searchValue: (r) => (r.fired ? 'yes' : 'no'),
        filterValue: (r) => (r.fired ? 'yes' : 'no'),
        sortValue: (r) => (r.fired ? 1 : 0),
        sortable: true,
      },
      {
        key: 'pnl_sol',
        label: 'PnL (SOL)',
        group: 'sim',
        render: (r) => (
          <span className={r.pnl_sol > 0 ? 'text-success' : r.pnl_sol < 0 ? 'text-danger' : 'text-text-dim'}>
            {r.fired ? r.pnl_sol.toFixed(4) : '—'}
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
        group: 'sim',
        render: (r) => (
          <span className={r.pnl_pct > 0 ? 'text-success' : r.pnl_pct < 0 ? 'text-danger' : 'text-text-dim'}>
            {r.fired ? `${r.pnl_pct.toFixed(1)}%` : '—'}
          </span>
        ),
        searchValue: () => '',
        filterNumber: (r) => (r.fired ? r.pnl_pct : null),
        sortValue: (r) => r.pnl_pct,
        sortable: true,
      },
      {
        key: 'holding_secs',
        label: 'Hold (s)',
        group: 'sim',
        render: (r) => (
          <span className="text-text-dim">
            {r.fired ? r.holding_secs : '—'}
          </span>
        ),
        searchValue: () => '',
        filterNumber: (r) => (r.fired ? r.holding_secs : null),
        sortValue: (r) => r.holding_secs,
        sortable: true,
      },
      {
        key: 'exit',
        label: 'Exit',
        group: 'sim',
        render: (r) => <span className="font-mono text-xs">{r.exit}</span>,
        searchValue: (r) => r.exit,
        filterValue: (r) => r.exit,
        sortValue: (r) => r.exit,
        sortable: true,
      },
    ],
    [],
  );

  const runsErr = apiErrorMessage(runsQuery.error, 'Failed to load sweep runs');
  const groupsErr = apiErrorMessage(groupsQuery.error, 'Failed to load groups');

  return (
    <div>
      <div className="mb-3.5 flex flex-wrap items-center gap-2.5">
        <h2 className="text-base font-bold text-primary">{title}</h2>
        <Badge variant="primary" className="font-mono">
          {runs.length} runs · {groups.length} groups
        </Badge>
      </div>

      <Accordion title="Configure sweep">
        <div ref={formRef}>
          <SweepConfigForm
            strategyId={strategyId}
            axes={axes}
            storageKey={storageKey}
            running={sweepRunning}
            onRun={run}
            reuseNonce={reuseNonce}
            reuseRun={activeRun}
            axesWarning={axesWarning}
          />
        </div>
      </Accordion>

      {startErr && <InlineAlert variant="error">{startErr}</InlineAlert>}
      {runsQuery.isLoading && <p className="text-text-dim">Loading sweep runs…</p>}
      {runsErr && <InlineAlert variant="error">{runsErr}</InlineAlert>}

      {!runsQuery.isLoading && !runsErr && runs.length === 0 && (
        <div className="rounded-md border border-white/10 bg-surface p-3 text-sm text-text-dim">
          No grouped sweeps yet. Set a date range + grouping above and click{' '}
          <span className="text-primary">Run grouped sweep</span>.
        </div>
      )}

      {runs.length > 0 && (
        <>
          <SectionDivider />

          {deleteErr && <InlineAlert variant="error">{deleteErr}</InlineAlert>}
          {groupsErr && <InlineAlert variant="error">{groupsErr}</InlineAlert>}

          {activeRun && (
            <Accordion
              header={<div className="flex flex-1 flex-wrap items-center gap-2.5">
                <label className="text-sm text-text-dim" htmlFor="grouped-sweep-run">
                  Run
                </label>
                <select
                  id="grouped-sweep-run"
                  className="rounded-md border border-white/10 bg-surface px-2.5 py-1.5 text-sm text-primary"
                  value={activeRunId ?? ''}
                  onChange={(e) => setSelectedRunId(e.target.value)}
                >
                  {runs.map((r) => (
                    <option key={r.id} value={r.id}>
                      {r.label ? `${r.label} · ` : ''}
                      {new Date(r.created_at).toLocaleString()} · {r.method} ·{' '}
                      {r.grouping_spec.length ? r.grouping_spec.join('+') : 'ALL'} ·{' '}
                      {r.token_count} tokens · {runGroupsLabel(r)} × {r.combo_count} combos
                    </option>
                  ))}
                </select>

                <Button
                  variant="danger"
                  size="sm"
                  disabled={!activeRunId || deleteState.isLoading}
                  onClick={onDeleteRun}
                >
                  {deleteState.isLoading ? 'Deleting…' : 'Delete Run'}
                </Button>

                <span className="ml-auto flex items-center gap-2">
                  <label className="text-sm text-text-dim" htmlFor="grouped-sweep-prune">
                    Clear runs before
                  </label>
                  <input
                    id="grouped-sweep-prune"
                    type="date"
                    className="rounded-md border border-white/10 bg-surface px-2.5 py-1.5 text-sm text-primary"
                    value={pruneBefore}
                    onChange={(e) => setPruneBefore(e.target.value)}
                  />
                  <Button
                    variant="danger"
                    size="sm"
                    disabled={!pruneBefore || pruneState.isLoading}
                    onClick={onPrune}
                  >
                    {pruneState.isLoading ? 'Clearing…' : 'Clear All OLD'}
                  </Button>
                </span>
              </div>}
              defaultOpen={false} className="mb-3">
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

          {activeRun && activeRun.status !== 'completed' && (
            <InlineAlert variant="warning">
              {activeRun.status === 'running' ? 'In-progress' : 'Partial'} run —{' '}
              {activeRun.groups_done} of {activeRun.group_count} groups
              {activeRun.status === 'running'
                ? ' persisted so far. The sweep is still running; more groups will appear as they finish.'
                : ' completed before the run was cancelled. The remaining groups were not swept — this is not a full sweep.'}
            </InlineAlert>
          )}

          <DataTable
            columns={groupColumns}
            rows={groups}
            rowKey={(g) => g.id}
            rowActions={groupRowActions}
            groupLabels={{ metrics: 'Metrics', entry: 'Entry', exit: 'Exit' }}
            defaultSort={{ col: 'best_score', dir: 'desc' }}
            searchable
            colFilters
            colToggle
            selectable
            selectedKey={activeGroupId}
            onSelect={setActiveGroupId}
            tableId={`${strategyId}_sweep_groups`}
            resetKey={activeRunId ?? ''}
            loading={groupsQuery.isFetching}
            emptyMessage="No groups cleared the min-tokens threshold for this run."
          />

          {activeGroupId && (
            <div className="mt-12">
              <div className="mb-2 flex flex-wrap items-center gap-2">
                <h3 className="text-sm font-bold text-secondary">Combos for group</h3>
                {activeGroup && (
                  <span className="font-mono text-xs text-text-dim">
                    {Object.keys(activeGroup.group_key).length
                      ? Object.entries(activeGroup.group_key)
                        .map(([k, v]) => `${k}=${v}`)
                        .join(' · ')
                      : 'ALL tokens'}{' '}
                    · {activeGroup.token_count} tokens
                  </span>
                )}
              </div>

              {resultsErr && <InlineAlert variant="error">{resultsErr}</InlineAlert>}

              <DataTable
                columns={comboColumns}
                rows={results}
                rowKey={(r) => String(r.combo_id)}
                rowActions={comboRowActions}
                groupLabels={{
                  params: 'Params',
                  counts: 'Counts',
                  pnl: 'PnL',
                  holding: 'Holding',
                  exits: 'Exit reasons',
                }}
                searchable={false}
                colFilters={false}
                colToggle
                selectable
                selectedKey={activeComboId !== null ? String(activeComboId) : null}
                onSelect={(key) => setActiveComboId(key !== null ? Number(key) : null)}
                serverSide
                serverTotal={resultsTotal}
                onQueryChange={onComboQueryChange}
                tableId={`${strategyId}_sweep_combos`}
                resetKey={activeGroupId ?? ''}
                loading={resultsLoading}
                emptyMessage="No combo results for this group."
              />

              {activeComboId !== null && (
                <div className="mt-10">
                  <div className="mb-2 flex flex-wrap items-center gap-2">
                    <h3 className="text-sm font-bold text-secondary">Tokens for combo #{activeComboId}</h3>
                    <span className="text-xs text-text-dim">
                      {tokenResultsQuery.isFetching
                        ? 'Simulating…'
                        : `${visibleTokenResults.length}${!showNotFired ? ` / ${tokenResults.length}` : ''} tokens`}
                    </span>
                    <div className="flex-grow" />
                    <VisibilityToggleButton
                      visible={showNotFired}
                      onToggle={() => setShowNotFired((v) => !v)}
                      label="not-fired tokens"
                    >
                      {showNotFired ? 'Hide not fired' : 'Show not fired'}
                    </VisibilityToggleButton>
                    <button
                      className="text-xs text-text-dim hover:text-primary"
                      onClick={() => setActiveComboId(null)}
                    >
                      ✕ Close
                    </button>
                  </div>

                  {tokenResultsErr && (
                    <InlineAlert variant="error">{tokenResultsErr}</InlineAlert>
                  )}

                  {/* Routed through the shared `TokenTable` (client mode — the
                      combo's per-token results are already resident; `DataTable`
                      pages in-browser). The bespoke columns already lay out the full
                      set, so append nothing (`ALL_TOKEN_INFO_KEYS`); the mint-set
                      paste box + per-token charts toggle come for free. */}
                  <TokenTable
                    columns={tokenColumns}
                    rows={visibleTokenResults}
                    existingKeys={ALL_TOKEN_INFO_KEYS}
                    mintSetFilter
                    charts
                    useRowOverlay={comboRowOverlay}
                    groupLabels={{
                      identity: 'Identity',
                      activity: 'Activity',
                      price: 'Price',
                      market: 'Market',
                      flags: 'Flags',
                      sim: 'Sim Results',
                    }}
                    searchable
                    colFilters
                    colToggle
                    selectable
                    selectedKey={selectedTokenMint}
                    onSelect={setSelectedTokenMint}
                    defaultSort={{ col: 'pnl_sol', dir: 'desc' }}
                    tableId={`${strategyId}_combo_tokens`}
                    resetKey={`${activeComboId}_${showNotFired}`}
                    loading={tokenResultsQuery.isFetching}
                    emptyMessage="No token results for this combo."
                  />

                  {selectedTokenResult &&
                    (() => {
                      const target = comboTarget(selectedTokenResult);
                      const onClose = () => setSelectedTokenMint(null);
                      // swing1 combos overlay their detected swing legs on the
                      // chart; other strategies use the plain modal.
                      return strategyId === 'swing_1' && activeComboParams ? (
                        <Swing1InspectModal
                          target={target}
                          params={activeComboParams}
                          onClose={onClose}
                        />
                      ) : (
                        <TokenInspectModal target={target} onClose={onClose} />
                      );
                    })()}
                </div>
              )}
            </div>
          )}

        </>
      )}
    </div>
  );
}
