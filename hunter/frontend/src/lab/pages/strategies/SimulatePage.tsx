import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { Link } from 'react-router-dom';

import { DataTable } from 'components/table/DataTable';
import { RelativeTimeCell } from 'components/table/RelativeTimeCell';
import type { ColumnDef, TableQuery } from 'components/table/types';
import { IconButton } from 'components/ui/IconButton';
import { IconButtonGroup } from 'components/ui/IconButtonGroup';
import {
  DisableIcon,
  DuplicateIcon,
  EditIcon,
  EnableIcon,
  LinkIcon,
  SimulateIcon,
  SpinnerIcon,
  TrashIcon,
} from 'components/ui/icons';
import { Badge, type BadgeVariant } from 'components/ui/Badge';
import { Select } from 'components/ui/Select';
import { InlineAlert } from 'components/ui/Modal';
import { SectionDivider } from 'components/ui/SectionDivider';
import { VisibilityToggleButton } from 'components/ui/VisibilityToggleButton';
import { TokenTable } from 'components/tokens/TokenTable';
import { tokenAmountColKeys, tokenNumericColKeys } from 'components/tokens/sharedTokenColumns';
import { dashPercent } from 'components/strategy/cellFormat';
import {
  buildEventMarkersForEpisodes,
  episodeRowKey,
  inspectFromSim,
  markerRowOverlay,
  type InspectTarget,
} from 'components/strategy/inspectTarget';
import type { ChartEventMarker } from 'components/token-price-chart';
import { simColumns, SIM_KEYS } from 'components/strategy/strategyColumns';
import { LazyLabTokenInspectModal } from '@lab/components/strategy/LazyLabTokenInspectModal';
import { SummaryStatsPanel, type SummaryStat } from 'components/strategy/SummaryStatsPanel';
import {
  TemporalSummary,
  type TemporalSelection,
} from 'components/strategy/TemporalSummary';
import { apiErrorMessage } from 'store/baseApi';
import { connectSimulationFinished } from 'services/sse';
import {
  fetchEngineSimPage,
  fetchEngineSimSummary,
  fetchEngineSimTimeSummary,
} from 'services/api';
import {
  toSummaryBody,
  toTableRequest,
  type TableRequestBody,
} from 'services/tableRequest';
import {
  useDisableStrategyRuleMutation,
  useEnableStrategyRuleMutation,
  useGetFingerprintsQuery,
  useGetStrategyRulesQuery,
} from 'store/sharedEndpoints';
import { RuleHoverTip } from 'components/strategy/RuleHoverTip';
import { useRuleActions } from 'components/strategy/useRuleActions';
import { buildCapsColumns } from 'components/strategy/capsRuleColumns';
import { buildFingerprintRuleColumns } from 'components/strategy/fingerprintRuleColumns';
import { buildRuleParamsColumns } from 'components/strategy/ruleParamsColumns';
import { DEFAULT_POSITIONS_QUERY, useServerTable } from 'hooks/useServerTable';
import { useLocalStorage } from 'hooks/useLocalStorage';
import { useSelectionSearchParam } from 'hooks/useSelectionSearchParam';
import { computeSameValueCellClasses } from 'lib/sameValueCellColors';
import { STORAGE_KEYS } from 'lib/storage';
import { rulesHref, STRATEGY_PARAMS } from 'lib/strategy/nav';
import {
  disabledRuleRowClass,
  lamportsToSol,
  COST_MODELS,
  FILL_MODELS,
  type CostModelId,
  type Fingerprint,
  type FillModelId,
  type StrategyRule,
  type TradeMode,
} from 'lib/strategy/types';
import { goodBad, pctText, runSummarySections, solText } from 'lib/strategy/runSummary';
import { pctGradeClass, winRateGradeClass } from 'lib/signedTone';
import { cn } from 'lib/cn';
import type {
  HoldSchemeChoice,
  WallGrainChoice,
  WallTimeField,
} from 'lib/strategy/temporalSummary';
import type { SummarySection } from 'components/strategy/SummaryStatsPanel';
import type {
  SimulatedSummary,
  SimulatedTokenResult,
  TemporalSummaryPayload,
} from 'types';
import {
  useStartEngineSimulationMutation,
  useGetEngineSimSummaryMutation,
  useGetEngineSimSummariesMutation,
} from '@lab/store/labEndpoints';

type RunState = { running: boolean; summary?: SimulatedSummary; error?: string };

const DASH = <span className="text-text-dim/60">—</span>;

/** One distinct hue per fill model so equal values read the same at a glance,
 *  ordered as a pessimism spectrum: worst-case (red) → first-in-window (neutral
 *  blue) → signal-price (green, optimistic bound). */
const FILL_MODEL_VARIANT: Record<FillModelId, BadgeVariant> = {
  worst_case: 'danger',
  first_in_window: 'info',
  signal_price: 'success',
};
/** `pumpfun_default` double-counts slippage against an explicit fill model (see
 *  `COST_MODELS`), so it reads as the cautionary color; `pumpfun_fee_only` is the
 *  honest pairing. */
const COST_MODEL_VARIANT: Record<CostModelId, BadgeVariant> = {
  pumpfun_default: 'danger',
  pumpfun_fee_only: 'info',
};
const SIM_NUMERIC_COLS = tokenNumericColKeys(simColumns);
const SIM_AMOUNT_COLS = tokenAmountColKeys(simColumns);
const simRowOverlay = markerRowOverlay(inspectFromSim);

/**
 * Full-corpus simulate for saved rules (lab app, FE3.2). Replaces the per-strategy
 * simulate flows with one generic surface: run a saved rule over the whole lake,
 * show every rule's funnel summary as sortable/filterable columns, and list the
 * per-token positions of the selected rule below. The dry-run panel (unsaved-draft
 * loop) lives in the rule editor; this page is for persisted rules.
 */
export function SimulatePage() {
  const { data: rules = [], isLoading } = useGetStrategyRulesQuery();
  const { data: fps = [] } = useGetFingerprintsQuery();
  const actions = useRuleActions();
  const [enable] = useEnableStrategyRuleMutation();
  const [disable] = useDisableStrategyRuleMutation();
  const [start] = useStartEngineSimulationMutation();
  const [fetchSummary] = useGetEngineSimSummaryMutation();
  const [fetchSummaries] = useGetEngineSimSummariesMutation();
  const [runs, setRuns] = useState<Record<string, RunState>>({});
  const [bulkMode, setBulkMode] = useState<TradeMode | null>(null);
  const [fillModel, setFillModel] = useState<FillModelId>('worst_case');
  // Kept alongside fillModel — pairing an explicit fill model with the default
  // cost model double-counts slippage (see `COST_MODELS`), so the two travel
  // together. Default stays `pumpfun_default` (the historically hardcoded value)
  // so a user who never touches this control sees unchanged numbers.
  const [costModel, setCostModel] = useState<CostModelId>('pumpfun_default');
  // Read through a ref in the run handlers: `runRule` is captured inside the
  // memoized `columns` (deps: runs/fpById/fpTints), so a plain closure over
  // `fillModel`/`costModel` would go stale until one of those changes. The refs
  // are always current.
  const fillModelRef = useRef(fillModel);
  fillModelRef.current = fillModel;
  const costModelRef = useRef(costModel);
  costModelRef.current = costModel;
  const [selectedRuleId, setSelectedRuleId] = useSelectionSearchParam(STRATEGY_PARAMS.rule);
  const [inspect, setInspect] = useState<{
    key: string;
    target: InspectTarget;
    rule: StrategyRule;
  } | null>(null);
  // All re-entry episodes for the inspected token, overlaid on one chart. A rule can
  // re-enter the same mint many times; the clicked row is only one episode, so fetch
  // the mint's full episode set (server-side, filtered) and build the union of their
  // entry/exit markers. Null ⇒ the modal falls back to the single clicked episode
  // (shown while this loads, or if the fetch fails).
  const [episodeMarkers, setEpisodeMarkers] = useState<ChartEventMarker[] | null>(null);
  useEffect(() => {
    setEpisodeMarkers(null);
    if (!inspect) return;
    const { rule, target } = inspect;
    const ctrl = new AbortController();
    const body = toTableRequest(
      {
        page: 1,
        pageSize: 1000,
        sortKeys: [],
        search: '',
        colFilters: {},
        structuredFilters: { mint_address: { op: 'in', val: [target.mint_address] } },
      },
      SIM_NUMERIC_COLS,
      { amountCols: SIM_AMOUNT_COLS },
    );
    void (async () => {
      try {
        const page = await fetchEngineSimPage(rule.id, body as TableRequestBody, ctrl.signal);
        if (ctrl.signal.aborted) return;
        const targets = page.items
          .filter((r) => r.fired !== false && r.exit_reason !== 'NoEntry')
          .map(inspectFromSim);
        setEpisodeMarkers(
          buildEventMarkersForEpisodes(targets.length ? targets : [target]),
        );
      } catch (e) {
        if (ctrl.signal.aborted || (e instanceof DOMException && e.name === 'AbortError')) return;
        setEpisodeMarkers(null);
      }
    })();
    return () => ctrl.abort();
  }, [inspect]);
  const [reloadNonce, setReloadNonce] = useState(0);
  /** Soft-archived rules are hidden by default — toggle to review them. */
  const [showDisabled, setShowDisabled] = useState(false);
  const [opErr, setOpErr] = useState<string | null>(null);
  const handleRef = useRef<{ close: () => void } | null>(null);
  const hydratedIds = useRef<Set<string>>(new Set());

  const runLifecycle = async (fn: () => Promise<unknown>, fail: string) => {
    setOpErr(null);
    try {
      await fn();
    } catch (e) {
      setOpErr(apiErrorMessage(e as never) ?? fail);
    }
  };

  const fpById = useMemo(() => new Map(fps.map((f) => [f.id, f])), [fps]);

  const disabledCount = useMemo(() => rules.filter((r) => !r.is_enabled).length, [rules]);
  const visibleRules = useMemo(
    () => (showDisabled ? rules : rules.filter((r) => r.is_enabled)),
    [rules, showDisabled],
  );

  // Tint fingerprint cells when ≥2 rules share the same fingerprint_id.
  const fpTints = useMemo(
    () =>
      computeSameValueCellClasses(visibleRules, (r) => r.id, [
        { key: 'fingerprint', valueOf: (r) => r.fingerprint_id || null },
      ]),
    [visibleRules],
  );

  const paperCount = useMemo(
    () => visibleRules.filter((r) => r.trade_mode === 'paper').length,
    [visibleRules],
  );
  const realCount = useMemo(
    () => visibleRules.filter((r) => r.trade_mode === 'real').length,
    [visibleRules],
  );

  // Hydrate every rule's resident sim summary on load (and when new rules appear),
  // so the table columns + position panels show without needing a row click.
  // One batch round-trip — missing/expired runs are omitted from the map.
  useEffect(() => {
    if (rules.length === 0) return;
    let cancelled = false;
    const pending = rules.filter((r) => !hydratedIds.current.has(r.id));
    if (pending.length === 0) return;
    for (const rule of pending) hydratedIds.current.add(rule.id);

    void (async () => {
      try {
        const summaries = await fetchSummaries(pending.map((r) => r.id)).unwrap();
        if (cancelled) return;
        setRuns((prev) => {
          const next = { ...prev };
          for (const [id, summary] of Object.entries(summaries)) {
            if (next[id]?.running) continue;
            next[id] = { running: false, summary };
          }
          return next;
        });
      } catch {
        /* batch failed — leave columns empty until a per-rule fetch succeeds */
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [rules, fetchSummaries]);

  // One page-level subscription routes each finished run to its rule (run_id ==
  // rule_id for saved rules).
  useEffect(() => {
    handleRef.current = connectSimulationFinished(async (ev) => {
      const id = ev.rule_id;
      if (ev.cancelled) {
        setRuns((r) => ({ ...r, [id]: { running: false } }));
        return;
      }
      try {
        const summary = await fetchSummary(id).unwrap();
        hydratedIds.current.add(id);
        setRuns((r) => ({ ...r, [id]: { running: false, summary } }));
      } catch (e) {
        setRuns((r) => ({ ...r, [id]: { running: false, error: apiErrorMessage(e as never) ?? 'summary failed' } }));
      }
      setReloadNonce((n) => n + 1);
    });
    return () => handleRef.current?.close();
  }, [fetchSummary]);

  const runRule = async (rule: StrategyRule) => {
    setRuns((r) => ({ ...r, [rule.id]: { running: true } }));
    setSelectedRuleId(rule.id);
    try {
      await start({
        rule_id: rule.id,
        fill_model: fillModelRef.current,
        cost_model: costModelRef.current,
      }).unwrap();
    } catch (e) {
      setRuns((r) => ({ ...r, [rule.id]: { running: false, error: apiErrorMessage(e as never) ?? 'start failed' } }));
    }
  };

  /** Queue a lake backtest for every saved rule in `mode` that isn't already running. */
  const runAll = async (mode: TradeMode) => {
    const targets = visibleRules.filter((r) => r.trade_mode === mode && !runs[r.id]?.running);
    if (targets.length === 0) return;
    setBulkMode(mode);
    setRuns((prev) => {
      const next = { ...prev };
      for (const r of targets) next[r.id] = { running: true };
      return next;
    });
    try {
      await Promise.all(
        targets.map(async (rule) => {
          try {
            await start({
              rule_id: rule.id,
              fill_model: fillModelRef.current,
              cost_model: costModelRef.current,
            }).unwrap();
          } catch (e) {
            setRuns((r) => ({
              ...r,
              [rule.id]: { running: false, error: apiErrorMessage(e as never) ?? 'start failed' },
            }));
          }
        }),
      );
    } finally {
      setBulkMode(null);
    }
  };

  const columns = useMemo<ColumnDef<StrategyRule>[]>(
    () => [
      ...buildColumns(runs, fpById, fpTints),
      {
        key: 'execute',
        label: 'Execute',
        render: (r) => (
          <IconButton
            variant="primary"
            size="md"
            disabled={runs[r.id]?.running}
            onClick={() => void runRule(r)}
            title={runs[r.id]?.running ? 'Running…' : 'Simulate'}
            aria-label={runs[r.id]?.running ? 'Running…' : 'Simulate'}
          >
            {runs[r.id]?.running ? <SpinnerIcon /> : <SimulateIcon />}
          </IconButton>
        ),
        searchValue: () => 'simulate',
      },
    ],
    // runRule closes over stable RTK/setState refs; runs drives the disabled state.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [runs, fpById, fpTints],
  );

  // Only the selected rule's positions render below the table; every rule's summary
  // is still hydrated for the table's metric columns.
  const selectedRule = useMemo(
    () => rules.find((r) => r.id === selectedRuleId) ?? null,
    [rules, selectedRuleId],
  );
  const selectedRun = selectedRuleId ? runs[selectedRuleId] : undefined;

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-3">
        <div className="flex flex-wrap items-baseline gap-3">
          <h1 className="text-lg font-semibold text-text">Simulate</h1>
          <span className="text-sm text-text-mid">
            Lake backtest for saved rules · drafts use Rules dry-run
          </span>
        </div>
        <div className="ml-auto flex flex-wrap items-center gap-2">
          <label className="flex items-center gap-1.5 text-[12px] text-text-dim">
            <span>Fill</span>
            <Select
              fieldSize="sm"
              value={fillModel}
              onChange={(e) => setFillModel(e.target.value as FillModelId)}
              className="w-36"
              title={FILL_MODELS.find((m) => m.id === fillModel)?.hint}
            >
              {FILL_MODELS.map((m) => (
                <option key={m.id} value={m.id} title={m.hint}>
                  {m.label}
                </option>
              ))}
            </Select>
          </label>
          <label className="flex items-center gap-1.5 text-[12px] text-text-dim">
            <span>Cost</span>
            <Select
              fieldSize="sm"
              value={costModel}
              onChange={(e) => setCostModel(e.target.value as CostModelId)}
              className="w-36"
              title={COST_MODELS.find((m) => m.id === costModel)?.hint}
            >
              {COST_MODELS.map((m) => (
                <option key={m.id} value={m.id} title={m.hint}>
                  {m.label}
                </option>
              ))}
            </Select>
          </label>
          {disabledCount > 0 && (
            <label className="flex cursor-pointer items-center gap-1.5 text-[12px] text-text-dim">
              <input
                type="checkbox"
                checked={showDisabled}
                onChange={(e) => setShowDisabled(e.target.checked)}
                className="accent-accent"
              />
              Show disabled ({disabledCount})
            </label>
          )}
          <IconButton
            variant="subtle"
            size="lg"
            disabled={paperCount === 0 || bulkMode !== null}
            onClick={() => runAll('paper')}
            label={
              bulkMode === 'paper'
                ? 'Starting paper…'
                : `Simulate All Paper (${paperCount})`
            }
            title={
              bulkMode === 'paper'
                ? 'Starting paper…'
                : `Simulate All Paper (${paperCount})`
            }
          >
            {bulkMode === 'paper' ? <SpinnerIcon /> : <SimulateIcon />}
          </IconButton>
          <IconButton
            variant="subtle"
            size="lg"
            disabled={realCount === 0 || bulkMode !== null}
            onClick={() => runAll('real')}
            label={
              bulkMode === 'real' ? 'Starting real…' : `Simulate All Real (${realCount})`
            }
            title={
              bulkMode === 'real' ? 'Starting real…' : `Simulate All Real (${realCount})`
            }
          >
            {bulkMode === 'real' ? <SpinnerIcon /> : <SimulateIcon />}
          </IconButton>
        </div>
      </div>
      {(actions.err || opErr) && (
        <p className="text-[12px] text-red">{actions.err || opErr}</p>
      )}
      <DataTable
        columns={columns}
        rows={visibleRules}
        rowKey={(r) => r.id}
        loading={isLoading}
        searchable
        tableId="simulate-rules"
        emptyMessage="No rules yet — author one on the Rules page."
        selectedKey={selectedRuleId}
        onSelect={setSelectedRuleId}
        rowClassName={disabledRuleRowClass}
        rowActions={(r) => (
          <IconButtonGroup>
            <IconButton
              variant="accent"
              size="md"
              onClick={() => actions.edit(r)}
              title="Edit"
              aria-label="Edit"
            >
              <EditIcon />
            </IconButton>
            <IconButton
              variant="ghost"
              size="md"
              onClick={() => actions.duplicate(r)}
              title="Duplicate"
              aria-label="Duplicate"
            >
              <DuplicateIcon />
            </IconButton>
            {r.is_enabled ? (
              <IconButton
                variant="ghost"
                size="md"
                title="Disable — keep this rule but hide it from the default list"
                aria-label="Disable — keep this rule but hide it from the default list"
                onClick={() => void runLifecycle(() => disable(r.id).unwrap(), 'Disable failed')}
              >
                <DisableIcon />
              </IconButton>
            ) : (
              <IconButton
                variant="ghost"
                size="md"
                title="Enable — restore this rule to the active list"
                aria-label="Enable — restore this rule to the active list"
                onClick={() => void runLifecycle(() => enable(r.id).unwrap(), 'Enable failed')}
              >
                <EnableIcon />
              </IconButton>
            )}
            <IconButton
              variant="danger"
              size="md"
              onClick={() => void actions.remove(r)}
              title="Delete"
              aria-label="Delete"
            >
              <TrashIcon />
            </IconButton>
          </IconButtonGroup>
        )}
      />

      {selectedRule && <SectionDivider gap="md" />}

      {selectedRule && selectedRun?.running && (
        <p className="text-sm text-text-dim">
          Simulating <span className="font-medium text-text">{selectedRule.rule_name}</span>…
        </p>
      )}

      {selectedRule && !selectedRun?.running && (
        <RuleSimPositionsPanel
          key={selectedRule.id}
          rule={selectedRule}
          reloadNonce={reloadNonce}
          onInspect={setInspect}
          inspectKey={inspect?.key ?? null}
        />
      )}

      {inspect && (
        <LazyLabTokenInspectModal
          target={inspect.target}
          titleSuffix="Sim inspect"
          ruleOverride={{
            paramsJson: inspect.rule.params,
            fingerprintId: inspect.rule.fingerprint_id,
            label: inspect.rule.rule_name,
          }}
          eventMarkers={episodeMarkers}
          onClose={() => setInspect(null)}
        />
      )}

      {actions.editorNode}
    </div>
  );
}

/** The selected rule's result detail — one server-side table of the sim run's
 *  per-token outcomes (fired + matched-but-not-fired `NoEntry` rows), same
 *  full-slice contract as the sweep combo drill-in. Show/Hide not-fired mirrors
 *  that drill-in so Charts can compare both or focus on fired only. */
function RuleSimPositionsPanel({
  rule,
  reloadNonce,
  onInspect,
  inspectKey,
}: {
  rule: StrategyRule;
  reloadNonce: number;
  onInspect: (v: { key: string; target: InspectTarget; rule: StrategyRule } | null) => void;
  inspectKey: string | null;
}) {
  const [simQuery, setSimQuery] = useState<TableQuery>(DEFAULT_POSITIONS_QUERY);
  const [showNotFired, setShowNotFired] = useLocalStorage(
    STORAGE_KEYS.simShowNotFired,
    true,
  );
  const [temporalSel, setTemporalSel] = useState<TemporalSelection>(null);
  const [wallField, setWallField] = useState<WallTimeField>('created_at');
  const [wallGrain, setWallGrain] = useState<WallGrainChoice>('auto');
  const [holdScheme, setHoldScheme] = useState<HoldSchemeChoice>('auto');
  const [timeSummary, setTimeSummary] = useState<TemporalSummaryPayload | null>(null);
  /** Linked brush cohort — same grain/scheme as base, filtered to the selection mints. */
  const [linkedTimeSummary, setLinkedTimeSummary] = useState<TemporalSummaryPayload | null>(null);

  // Hide-not-fired injects a server-side `exit_reason != NoEntry` (text `neq`) so
  // paging/totals stay correct — same toggle as the sweep combo drill-in.
  const applyNotFiredFilter = useCallback(
    (q: TableQuery): TableQuery => {
      if (showNotFired) return q;
      return {
        ...q,
        structuredFilters: {
          ...q.structuredFilters,
          exit_reason: { op: 'neq', val: 'NoEntry' },
        },
      };
    },
    [showNotFired],
  );

  // Temporal mint-set is applied to the page fetch only — summary + base time-summary
  // stay on the table's own filters so the driving chart doesn't collapse after a click.
  const simQueryForPage = useMemo(() => {
    const base = applyNotFiredFilter(simQuery);
    if (!temporalSel?.mints.length) return base;
    return {
      ...base,
      structuredFilters: {
        ...base.structuredFilters,
        mint_address: { op: 'in' as const, val: temporalSel.mints },
      },
    };
  }, [simQuery, temporalSel, applyNotFiredFilter]);

  const simBody = useMemo(
    () => toTableRequest(simQueryForPage, SIM_NUMERIC_COLS, { amountCols: SIM_AMOUNT_COLS }),
    [simQueryForPage],
  );
  // KPI summary tracks the page cohort (includes temporal click-filter).
  const simSummaryBody = useMemo(
    () => toSummaryBody(simQueryForPage, SIM_NUMERIC_COLS, { amountCols: SIM_AMOUNT_COLS }),
    [simQueryForPage],
  );
  // Base time chart stays on the table's own filters (+ not-fired toggle).
  const timeSummaryBody = useMemo(
    () =>
      toSummaryBody(applyNotFiredFilter(simQuery), SIM_NUMERIC_COLS, {
        amountCols: SIM_AMOUNT_COLS,
      }),
    [simQuery.search, simQuery.colFilters, simQuery.structuredFilters, applyNotFiredFilter],
  );
  // Linked chart: mint-filtered fold with base's resolved grain/scheme locked.
  const linkedTimeSummaryBody = useMemo(() => {
    if (!temporalSel?.mints.length) return null;
    return toSummaryBody(simQueryForPage, SIM_NUMERIC_COLS, { amountCols: SIM_AMOUNT_COLS });
  }, [temporalSel, simQueryForPage]);
  // Stable fetchers — inline arrows are new every render and `useServerTable`
  // lists them as effect deps, which cleared the summary card in a loop and made
  // Temporal pattern mount/unmount (the "vibration" / style break).
  const fetchSimPage = useCallback(
    (body: unknown, signal: AbortSignal) =>
      fetchEngineSimPage(rule.id, body as TableRequestBody, signal),
    [rule.id],
  );
  const fetchSimSummary = useCallback(
    (summaryBody: unknown, signal: AbortSignal) =>
      fetchEngineSimSummary(rule.id, summaryBody as TableRequestBody, signal),
    [rule.id],
  );
  const {
    items: simTokens,
    total: simTotal,
    summary: simSummary,
    loading: simTableLoading,
    error: simTableError,
    reload: reloadSim,
  } = useServerTable<SimulatedTokenResult, SimulatedSummary>(
    true,
    simBody,
    fetchSimPage,
    fetchSimSummary,
    simSummaryBody,
    rule.id,
  );

  // Clear temporal cohort when the table's own filters change or the rule switches.
  const baseFilterKey = JSON.stringify({
    s: simQuery.search,
    c: simQuery.colFilters,
    f: simQuery.structuredFilters,
  });
  useEffect(() => {
    setTemporalSel(null);
    setLinkedTimeSummary(null);
  }, [rule.id, baseFilterKey]);

  useEffect(() => {
    const ctrl = new AbortController();
    void fetchEngineSimTimeSummary(
      rule.id,
      timeSummaryBody as TableRequestBody,
      wallField,
      wallGrain,
      holdScheme,
      ctrl.signal,
    )
      .then((t) => {
        if (!ctrl.signal.aborted) setTimeSummary(t);
      })
      .catch((e) => {
        if (e instanceof DOMException && e.name === 'AbortError') return;
        if (!ctrl.signal.aborted) setTimeSummary(null);
      });
    return () => ctrl.abort();
  }, [rule.id, timeSummaryBody, wallField, wallGrain, holdScheme, reloadNonce]);

  useEffect(() => {
    if (!linkedTimeSummaryBody || !timeSummary) {
      setLinkedTimeSummary(null);
      return;
    }
    // Lock edges to the base cohort so ghost bars align.
    const lockedGrain = timeSummary.wallGrain;
    const lockedHold = timeSummary.holdScheme ?? 'mid_30m';
    const ctrl = new AbortController();
    void fetchEngineSimTimeSummary(
      rule.id,
      linkedTimeSummaryBody as TableRequestBody,
      wallField,
      lockedGrain,
      lockedHold,
      ctrl.signal,
    )
      .then((t) => {
        if (!ctrl.signal.aborted) setLinkedTimeSummary(t);
      })
      .catch((e) => {
        if (e instanceof DOMException && e.name === 'AbortError') return;
        if (!ctrl.signal.aborted) setLinkedTimeSummary(null);
      });
    return () => ctrl.abort();
  }, [
    rule.id,
    linkedTimeSummaryBody,
    timeSummary?.wallGrain,
    timeSummary?.holdScheme,
    wallField,
    reloadNonce,
  ]);

  useEffect(() => {
    reloadSim();
  }, [reloadNonce, reloadSim]);

  const onSelectSim = useCallback(
    (key: string | null) => {
      // A table row selects by episode key; a grouped charts-grid card selects by
      // mint — resolve either. The inspect modal overlays all episodes regardless.
      const row = key
        ? simTokens.find((t) => episodeRowKey(t) === key) ??
          simTokens.find((t) => t.mint_address === key) ??
          null
        : null;
      onInspect(row ? { key: episodeRowKey(row), target: inspectFromSim(row), rule } : null);
    },
    [simTokens, onInspect, rule],
  );

  const simStats = useMemo(() => (simSummary ? simSummaryStats(simSummary) : null), [simSummary]);

  // "no simulation result" just means this rule has no resident run — the parent
  // only mounts us once it has a summary, so show the empty table, not an error.
  const simError = simTableError && !simTableError.includes('no simulation result')
    ? simTableError
    : null;

  // Full cohort size while Show not-fired is on — badge renders `fired / all`
  // after hide (sweep drill-in shape).
  const allTotal = useRef(0);
  const allTotalRule = useRef(rule.id);
  if (allTotalRule.current !== rule.id) {
    allTotalRule.current = rule.id;
    allTotal.current = 0;
  }
  if (showNotFired && simTotal > 0) allTotal.current = simTotal;
  const nFired = simSummary?.realized.n_fired ?? 0;
  const hideNoOp = showNotFired && !simTableLoading && simTotal > 0 && simTotal <= nFired;
  const badge =
    !showNotFired && allTotal.current > simTotal
      ? `${simTotal} / ${allTotal.current}`
      : showNotFired && !simTableLoading && simTotal > nFired && nFired > 0
        ? `${simTotal} · ${nFired} fired`
        : String(simTotal);

  return (
    <section id={`sim-positions-${rule.id}`}>
      <div className="mb-3.5 flex flex-wrap items-center gap-2.5">
        <span className="h-4 w-1 rounded-full bg-info" />
        <h3 className="text-sm font-bold text-text">Simulated Positions</h3>
        <Badge variant="info" size="sm" className="font-mono font-normal">
          {badge}
        </Badge>
        <VisibilityToggleButton
          visible={showNotFired}
          disabled={hideNoOp}
          {...(hideNoOp
            ? {
                title:
                  'This result has no not-fired (NoEntry) rows — re-run Simulate with the current lab binary.',
              }
            : {})}
          onToggle={() => {
            setShowNotFired((v) => !v);
            setSimQuery((q) => ({ ...q, page: 1 }));
          }}
          label="not-fired tokens"
        >
          {showNotFired ? 'Hide not fired' : 'Show not fired'}
        </VisibilityToggleButton>
        <span className="truncate font-mono text-[11px] text-text-dim">{rule.rule_name}</span>
      </div>

      {simError ? (
        <InlineAlert variant="error">{simError}</InlineAlert>
      ) : (
        <>
          {simStats && (
            <SummaryStatsPanel
              title="Simulated results summary"
              subtitle="Tracks the table's filters"
              heroStats={simStats.hero}
              sections={simStats.sections}
              accentClass="bg-info"
            />
          )}
          {timeSummary && timeSummary.nFired > 0 && (
            <TemporalSummary
              data={{
                ...timeSummary,
                holdScheme: timeSummary.holdScheme ?? 'mid_30m',
                holdSchemeAuto:
                  timeSummary.holdSchemeAuto ?? timeSummary.holdScheme ?? 'mid_30m',
                wallGrainAuto: timeSummary.wallGrainAuto ?? timeSummary.wallGrain,
                wallSpanMs: timeSummary.wallSpanMs ?? 0,
              }}
              linkedData={
                linkedTimeSummary
                  ? {
                      ...linkedTimeSummary,
                      holdScheme: linkedTimeSummary.holdScheme ?? timeSummary.holdScheme ?? 'mid_30m',
                      holdSchemeAuto:
                        linkedTimeSummary.holdSchemeAuto ??
                        linkedTimeSummary.holdScheme ??
                        'mid_30m',
                      wallGrainAuto:
                        linkedTimeSummary.wallGrainAuto ?? linkedTimeSummary.wallGrain,
                      wallSpanMs: linkedTimeSummary.wallSpanMs ?? 0,
                    }
                  : null
              }
              selection={temporalSel}
              onSelect={setTemporalSel}
              wallField={wallField}
              onWallFieldChange={setWallField}
              wallGrain={wallGrain}
              onWallGrainChange={setWallGrain}
              holdScheme={holdScheme}
              onHoldSchemeChange={setHoldScheme}
            />
          )}
          <TokenTable
            columns={simColumns}
            existingKeys={SIM_KEYS}
            mintSetFilter
            charts
            useRowOverlay={simRowOverlay}
            chartsGroupByMint
            mintChartGroupOverlay={(rows, _mint) => ({
              eventMarkers: buildEventMarkersForEpisodes(
                rows
                  .filter((r) => r.fired !== false && r.exit_reason !== 'NoEntry')
                  .map(inspectFromSim),
              ),
            })}
            rows={simTokens}
            rowKey={episodeRowKey}
            selectedKey={inspectKey}
            onSelect={onSelectSim}
            serverSide
            serverTotal={simTotal}
            onQueryChange={setSimQuery}
            loading={simTableLoading}
            resetKey={`${rule.id}_${showNotFired}`}
            tableId="simulate-positions"
            emptyMessage={
              showNotFired
                ? 'No tokens in this simulation result.'
                : 'No fired positions in this simulation result.'
            }
          />
        </>
      )}
    </section>
  );
}

/**
 * `SimulatedSummary` → summary-panel tiles, via the **shared** run-summary
 * renderer — the same builder the grouped sweep and the live/paper positions card
 * use (parity plan F1-F8). Server-computed over the filtered cohort (the summary
 * endpoint takes the table's search/filters), so these update as the user filters.
 *
 * This page previously hand-rolled four tiles whose headline "Total PnL" summed
 * still-open marks into the realized figure, so a rule that simply never closed
 * its losers read as profitable here while the sweep reported the loss. The
 * backend now sends the two-band `RunSummary` and this just renders it.
 */
function simSummaryStats(s: SimulatedSummary): { hero: SummaryStat[]; sections: SummarySection[] } {
  return runSummarySections(s, { migrated: s.n_migrated });
}

function buildColumns(
  runs: Record<string, RunState>,
  fpById: Map<string, Fingerprint>,
  fpTints: Map<string, string>,
): ColumnDef<StrategyRule>[] {
  const runOf = (r: StrategyRule) => runs[r.id];
  const summaryOf = (r: StrategyRule) => runOf(r)?.summary;

  /** One SimulatedSummary numeric field as its own sortable/filterable column. */
  const simMetric = (
    key: string,
    label: string,
    value: (s: SimulatedSummary) => number,
    renderVal: (s: SimulatedSummary) => ReactNode,
    opts?: { tooltip?: string; displayUnits?: (n: number) => number },
  ): ColumnDef<StrategyRule> => {
    const units = opts?.displayUnits ?? ((n: number) => n);
    return {
      key,
      label,
      group: 'sim',
      tooltip: opts?.tooltip,
      sortable: true,
      render: (r) => {
        const run = runOf(r);
        if (!run) return DASH;
        if (run.running) return <span className="text-text-dim">…</span>;
        if (run.error || !run.summary) return DASH;
        return renderVal(run.summary);
      },
      sortValue: (r) => {
        const s = summaryOf(r);
        return s ? value(s) : null;
      },
      filterNumber: (r) => {
        const s = summaryOf(r);
        return s ? units(value(s)) : null;
      },
      filterValue: (r) => {
        const s = summaryOf(r);
        return s ? String(units(value(s))) : '';
      },
      searchValue: (r) => {
        const s = summaryOf(r);
        return s ? String(units(value(s))) : '';
      },
    };
  };

  return [
    {
      key: 'rule_name',
      label: 'Rule',
      group: 'name',
      render: (r) => (
        <RuleHoverTip rule={r} fingerprint={fpById.get(r.fingerprint_id)}>
          <div className="flex min-w-40 cursor-default flex-col gap-0.5">
            <div className="flex items-center justify-center gap-1">
              <span className="font-medium text-text">{r.rule_name}</span>
              <Link
                to={rulesHref(r.id)}
                title={`Open rule “${r.rule_name}”`}
                aria-label={`Open rule ${r.rule_name}`}
                className="inline-flex shrink-0 rounded p-0.5 text-accent hover:bg-accent/15 hover:text-primary"
                onClick={(e) => e.stopPropagation()}
              >
                <LinkIcon className="h-3.5 w-3.5" />
              </Link>
            </div>
            <span className="text-[10px] text-text-dim">
              {!r.is_enabled
                ? 'disabled'
                : r.is_active
                  ? 'armed on live'
                  : 'idle on live'}
            </span>
          </div>
        </RuleHoverTip>
      ),
      searchValue: (r) =>
        `${r.rule_name} ${!r.is_enabled ? 'disabled' : r.is_active ? 'active' : 'idle'}`,
    },
    {
      key: 'mode',
      label: 'Mode',
      group: 'status',
      render: (r) => (
        <Badge variant={r.trade_mode === 'real' ? 'warning' : 'info'}>{r.trade_mode}</Badge>
      ),
      searchValue: (r) => r.trade_mode,
      sortValue: (r) => r.trade_mode,
    },
    ...buildFingerprintRuleColumns(fpById, {
      cellClassName: (r) => fpTints.get(`${r.id}\0fingerprint`),
    }),
    {
      key: 'buy',
      label: 'Buy',
      render: (r) => <span className="tabular-nums">{lamportsToSol(r.buy_amount_lamports)}◎</span>,
      searchValue: (r) => String(lamportsToSol(r.buy_amount_lamports)),
      sortValue: (r) => r.buy_amount_lamports,
      sortable: true,
    },
    ...buildCapsColumns(),
    ...buildRuleParamsColumns(),
    {
      key: 'sim_run',
      label: 'Run',
      group: 'sim',
      tooltip: 'When this rule’s last simulation result was generated',
      render: (r) => {
        const run = runOf(r);
        if (!run) return DASH;
        if (run.running) return <span className="text-text-dim">running…</span>;
        if (run.error) return <span className="text-red">{run.error}</span>;
        if (run.summary) {
          return run.summary.computed_at ? (
            <RelativeTimeCell iso={run.summary.computed_at} />
          ) : (
            <span className="text-text-dim">done</span>
          );
        }
        return <span className="text-text-dim/60">cancelled</span>;
      },
      searchValue: (r) => {
        const run = runOf(r);
        if (!run) return '';
        if (run.running) return 'running';
        if (run.error) return run.error;
        if (run.summary) return 'done';
        return 'cancelled';
      },
    },
    {
      key: 'sim_fill_model',
      label: 'Fill',
      group: 'sim',
      tooltip:
        'Which fill model priced this result’s round-trips — the pessimism band it was booked under',
      sortable: true,
      render: (r) => {
        const run = runOf(r);
        if (!run || run.running || run.error || !run.summary) return DASH;
        const id = run.summary.fill_model ?? 'worst_case';
        const model = FILL_MODELS.find((m) => m.id === id);
        return (
          <Badge variant={FILL_MODEL_VARIANT[id]} size="sm" title={model?.hint}>
            {model?.label ?? id}
          </Badge>
        );
      },
      sortValue: (r) => summaryOf(r)?.fill_model ?? null,
      searchValue: (r) => {
        const id = summaryOf(r)?.fill_model;
        return id ? (FILL_MODELS.find((m) => m.id === id)?.label ?? id) : '';
      },
    },
    {
      key: 'sim_cost_model',
      label: 'Cost',
      group: 'sim',
      tooltip:
        'Which execution-cost model priced this result’s round-trips — pairing "Fee + slippage" with a non-default Fill double-counts slippage',
      sortable: true,
      render: (r) => {
        const run = runOf(r);
        if (!run || run.running || run.error || !run.summary) return DASH;
        const id = run.summary.cost_model ?? 'pumpfun_default';
        const model = COST_MODELS.find((m) => m.id === id);
        return (
          <Badge variant={COST_MODEL_VARIANT[id]} size="sm" title={model?.hint}>
            {model?.label ?? id}
          </Badge>
        );
      },
      sortValue: (r) => summaryOf(r)?.cost_model ?? null,
      searchValue: (r) => {
        const id = summaryOf(r)?.cost_model;
        return id ? (COST_MODELS.find((m) => m.id === id)?.label ?? id) : '';
      },
    },
    // Mirrors the grouped-sweep combo table's stat columns (same metrics, same
    // formatters) so a rule reads identically in both places — including the
    // open cohort, which this table used to omit entirely (parity plan F1).
    simMetric('sim_entered', 'Entered', (s) => s.realized.n_fired, (s) => (
      <span className="tabular-nums text-text">{s.realized.n_fired}</span>
    ), { tooltip: 'Tokens that took a position' }),
    simMetric('sim_closed', 'Closed', (s) => s.realized.n_closed, (s) => (
      <span className="tabular-nums text-text">{s.realized.n_closed}</span>
    ), { tooltip: 'Tokens that closed a position' }),
    simMetric('sim_open', 'Open', (s) => s.realized.n_open, (s) => (
      <span className="tabular-nums text-text">{s.realized.n_open}</span>
    ), { tooltip: 'Positions still open at the end of the run (unrealized)' }),
    simMetric(
      'sim_win_rate',
      'Win %',
      (s) => s.realized.win_rate,
      (s) => (
        <span className={cn('tabular-nums', winRateGradeClass(s.realized.win_rate))}>
          {dashPercent(s.realized.win_rate * 100)}
        </span>
      ),
      { tooltip: 'Share of closed tokens with PnL > 0', displayUnits: (n) => n * 100 },
    ),
    simMetric(
      'sim_avg_pnl',
      'Mean %',
      (s) => s.realized.mean_pnl_pct,
      (s) => (
        <span className={cn('tabular-nums', pctGradeClass(s.realized.mean_pnl_pct))}>
          {pctText(s.realized.mean_pnl_pct)}
        </span>
      ),
      { tooltip: 'Mean PnL % over closed tokens' },
    ),
    simMetric(
      'sim_total_pnl',
      'Total PnL',
      (s) => s.realized.total_pnl_sol,
      (s) => (
        <span className={`tabular-nums ${goodBad(s.realized.total_pnl_sol)}`}>
          {solText(s.realized.total_pnl_sol)}
        </span>
      ),
      { tooltip: 'Sum of REALIZED PnL in SOL (closed positions only)' },
    ),
    simMetric(
      'sim_open_pnl',
      'Open PnL',
      (s) => s.realized.open_pnl_sol,
      (s) => (
        <span className={`tabular-nums ${goodBad(s.realized.open_pnl_sol)}`}>
          {solText(s.realized.open_pnl_sol)}
        </span>
      ),
      { tooltip: 'Unrealized mark-to-last-price PnL of the still-open positions' },
    ),
    simMetric(
      'sim_pnl_mtm',
      'PnL (MTM)',
      (s) => s.mtm.total_pnl_sol,
      (s) => (
        <span className={`tabular-nums ${goodBad(s.mtm.total_pnl_sol)}`}>
          {solText(s.mtm.total_pnl_sol)}
        </span>
      ),
      { tooltip: 'Realized + unrealized — what the run is currently worth' },
    ),
  ];
}
