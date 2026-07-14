import { memo, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { DataTable } from 'components/table/DataTable';
import { Accordion } from 'components/ui/Accordion';
import { Badge, type BadgeVariant } from 'components/ui/Badge';
import { SectionDivider } from 'components/ui/SectionDivider';
import { Button } from 'components/ui/Button';
import { Input } from 'components/ui/Input';
import { InlineAlert } from 'components/ui/Modal';
import { SpecRuleForm } from 'components/strategy/SpecRuleForm';
import {
  blobToDetectParams,
  buildCreatePayload,
  buildUpdatePayload,
  emptyForm,
  formFromRule,
  getSpec,
  serializeRule,
  serializeRuleJson,
  RULE_NAME_KEY,
  type FormState,
  type LockState,
} from 'lib/params';
// swing1 rules carry the generic RuleRecord shape, so it reuses tpsl1's rule /
// position / sim / matched column sets (the fingerprint-only columns just render
// `-`). This mirrors the live Swing1Page, which reuses the same columns.
import { ruleColumns as allRuleColumns, RuleRowProvider } from 'components/tpsl1/ruleColumns';
import { SimSummaryCard } from 'components/tpsl1/SimSummaryCard';
import { RunPositionsPanel } from 'components/strategy/RunPositionsPanel';
import { PaperResultSection } from '@lab/components/strategies/PaperResultSection';
import { TokenInspectModal, type InspectTarget } from 'components/tpsl2/TokenInspectModal';
import { inspectFromPosition, inspectFromSim } from 'components/strategy/inspectTarget';
import type { ChartSwingLeg } from 'components/token-price-chart';
import { type Swing1DetectParams } from '@lab/services/swing1Detect';
import {
  makeSwing1DetectRowOverlay,
  useSwing1DetectOverlay,
} from '@lab/hooks/useSwing1DetectOverlay';
import {
  matchedColumns,
  positionColumns,
  simColumns,
  POSITION_KEYS,
  MATCHED_KEYS,
  SIM_KEYS,
} from 'components/strategy/strategyColumns';
import { TokenTable } from 'components/tokens/TokenTable';
import { tokenNumericColKeys } from 'components/tokens/sharedTokenColumns';
import { useTimezone } from 'context/TimezoneContext';
import { datetimeLocalToUtcWallClock } from 'utils/date';
import { usePriceDisplay } from 'hooks/usePriceDisplay';
import { usePolledRules } from 'hooks/usePolledRules';
import { useRulePositions, DEFAULT_POSITIONS_QUERY } from 'hooks/useRulePositions';
import { toSummaryBody, toTableRequest, type TableRequestBody } from 'services/tableRequest';
import type { TableQuery } from 'components/table/types';
import { useDispatch } from 'react-redux';
import {
  clearSwing1PaperResult,
  createSwing1Rule,
  deleteSwing1Rule,
  fetchMatchedPage,
  fetchSimulatedPage,
  fetchSimulatedSummary,
  fetchSwing1RulePositions,
  fetchSwing1RulePositionsSummary,
  fetchSwing1Rules,
  startSimulation,
  updateSwing1Rule,
} from 'services/api';
import { connectPaperTestStream, connectSimulationFinished } from 'services/sse';
import { useBackgroundJobActions } from '@lab/context/BackgroundJobsContext';
import { apiErrorMessage } from 'store/apiSlice';
import {
  fetchPaperResultCached,
  invalidatePaperResult,
  invalidateStrategyMatched,
} from '@lab/store/strategyResultCache';
import { useServerTable } from 'hooks/useServerTable';
import type { AppDispatch } from '@lab/store';
import type {
  MatchedTokenRecord,
  PaperResultResponse,
  PaperTestFinishedEvent,
  PositionsSummary,
  RulePositionRecord,
  RuleRecord,
  SimulatedSummary,
  SimulatedTokenResult,
} from 'types';
import { cn } from 'lib/cn';
import { fingerprintKey } from 'lib/ruleColorGroups';

const SWING1_SPEC = getSpec('swing_1');

// swing1 carries TWO identities that must not be conflated:
//  • STRATEGY — the strategy STRING the backend emits over SSE (`swing_1`);
//    keys the rule/position polling hooks + `connectTpsl*` stream filters.
//  • STRATEGY_SEG — the URL segment for the generic `/api/strategies/{seg}/…`
//    routes (`swing1`), which also keys the per-rule result cache args.
// (tpsl1/tpsl2 happen to use the same token for both, so they carry one const.)
const STRATEGY = 'swing_1' as const;
const STRATEGY_SEG = 'swing1' as const;

/** Matched / Simulated columns that filter numerically (same server-side contract
 *  as Positions — `>5`/`1..10` → structured ops). */
const MATCHED_NUMERIC_COLS = tokenNumericColKeys(matchedColumns);
const SIM_NUMERIC_COLS = tokenNumericColKeys(simColumns);
const POSITION_NUMERIC_COLS = tokenNumericColKeys(positionColumns);

/** Adapt the server's Simulated whole-run aggregate onto the shape `SimSummaryCard`
 *  renders (`PositionsSummary`); only the fields the sim summary reports are set. */
function simSummaryToCard(s: SimulatedSummary): PositionsSummary {
  const closed = s.closed_tokens;
  const win = Math.round(s.win_rate * closed);
  return {
    tokens: s.total_tokens,
    open: s.total_tokens - closed,
    win,
    loss: closed - win,
    closed,
    win_rate: s.win_rate * 100, // fraction (0..1) → percent for the card
    avg_pnl_pct: s.avg_pnl_percent,
    total_pnl_sol: s.total_pnl_sol,
    total_entry_sol: 0,
    total_holding_sol: 0,
    total_gains_sol: 0,
    total_losses_sol: 0,
    avg_hold_secs: 0,
    best_pct: null,
    worst_pct: null,
  };
}

// Module-level, referentially-stable rowKey fns: each only reads the row, so a
// single shared identity lets DataTable's page/select effects (and the row
// memo) skip churn that an inline `(r) => r.x` would trigger every render.
const keyByMint = (r: { mint_address: string }) => r.mint_address;
const keyById = (r: { id: string }) => r.id;


/** Heading for a section: a colored marker bar + title + optional count badge,
 *  subtitle, and right-aligned actions. Reused across the page so every section
 *  reads at a glance — content sits directly below it, with no surrounding card
 *  chrome, so only the real tables look like tables. */
function SectionHeading({
  title,
  count,
  marker = 'bg-primary',
  badge = 'primary',
  badgeClass,
  size = 'h3',
  subtitle,
  action,
}: {
  title: string;
  count?: number;
  marker?: string;
  badge?: BadgeVariant;
  badgeClass?: string;
  size?: 'h2' | 'h3';
  subtitle?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="mb-3.5 flex items-center gap-2.5">
      <span className={cn('w-1 rounded-full', size === 'h2' ? 'h-5' : 'h-4', marker)} />
      {size === 'h2' ? (
        <h2 className="text-base font-bold text-primary">{title}</h2>
      ) : (
        <h3 className="text-sm font-bold text-text">{title}</h3>
      )}
      {count != null && (
        <Badge variant={badge} size="sm" className={cn('font-mono font-normal', badgeClass)}>
          {count}
        </Badge>
      )}
      {subtitle && <span className="truncate font-mono text-[11px] text-text-dim">{subtitle}</span>}
      {action && (
        <>
          <span className="flex-1" />
          {action}
        </>
      )}
    </div>
  );
}

/** Inline (margin-free) variant of the section heading, sized to sit inside an
 *  Accordion's custom header row — the marker bar + title + count badge +
 *  subtitle, with the Accordion supplying the chevron toggle. */
function RuleTableHeader({
  title,
  count,
  marker,
  badge,
  subtitle,
}: {
  title: string;
  count: number;
  marker: string;
  badge: BadgeVariant;
  subtitle: string;
}) {
  return (
    <div className="flex min-w-0 flex-1 items-center gap-2.5">
      <span className={cn('h-4 w-1 rounded-full', marker)} />
      <h3 className="text-sm font-bold text-text">{title}</h3>
      <Badge variant={badge} size="sm" className="font-mono font-normal">
        {count}
      </Badge>
      <span className="truncate font-mono text-[11px] text-text-dim">{subtitle}</span>
    </div>
  );
}

/** A row selected for inspection, tagged with its source table so only that
 *  table highlights the selection (the three tables can share a mint/key). */
type InspectState = {
  table: 'positions' | 'sim' | 'paper' | 'matched';
  key: string;
  target: InspectTarget;
  /** The rule whose params produced this row — drives the swing-leg overlay
   *  fetch (absent for `matched`, which runs no funnel yet). */
  ruleId: string | null;
  /** Legs the sim already resolved for this row (single-rule backtest carries
   *  them), so the modal skips the `swing1-detect` round-trip. Absent for rows
   *  with no carried legs (matched, positions) — those still fetch by rule. */
  carriedLegs?: ChartSwingLeg[] | null;
};

// Recognized detect-param keys == the swing1 spec's detect-keyed fields, so a
// rule's serialized blob maps onto exactly the axes `fetchSwing1Detect` reads.
const SWING1_DETECT_KEYS = new Set(
  SWING1_SPEC.fields.filter((f) => f.detectKey).map((f) => f.detectKey!),
);

/** Resolve a swing1 rule's detect params (the axes the sim ran). The chart's
 *  `swing1-detect` overlay is keyed off these so its legs match the row's
 *  entry/exit. Returns null for an unknown rule (overlay then draws nothing). */
function swing1ParamsFromRule(rule: RuleRecord | undefined): Swing1DetectParams | null {
  if (!rule) return null;
  const blob = serializeRule(SWING1_SPEC, rule);
  const { params } = blobToDetectParams(blob, SWING1_DETECT_KEYS);
  return params as Swing1DetectParams;
}

/** Wraps the shared {@link TokenInspectModal} with the swing1 leg overlay for a
 *  rule/sim/paper/position result row. Runs the same detect funnel the rule's
 *  own params drove (full history) so the chart's swing legs match what produced
 *  this row's entry/exit — mirrors the sweep's {@link Swing1InspectModal} but
 *  keyed off a rule instead of a combo. `carriedLegs` (sim rows) skip the fetch. */
function SwingRuleInspectModal({
  target,
  rule,
  carriedLegs,
  onClose,
}: {
  target: InspectTarget;
  rule: RuleRecord | undefined;
  /** Legs the sim already resolved for this row; when present the modal draws
   *  them directly and skips the `swing1-detect` round-trip (they're the exact
   *  legs that produced this row's entry/exit, so a re-detect is redundant). */
  carriedLegs?: ChartSwingLeg[] | null;
  onClose: () => void;
}) {
  const params = useMemo(() => swing1ParamsFromRule(rule), [rule]);
  const swingOverlay = useSwing1DetectOverlay(target.mint_address, params, carriedLegs);
  return <TokenInspectModal target={target} swingOverlay={swingOverlay} onClose={onClose} />;
}

function inspectFromMatched(r: import('types').MatchedTokenRecord): InspectTarget {
  return {
    mint_address: r.mint_address,
    symbol: r.symbol,
    entryTime: null,
    entryPrice: null,
    exitTime: null,
    exitPrice: null,
  };
}



/** No-op run-control bag — lab rules never execute a live run (no keys, no
 *  gRPC), so the 'controls' column is filtered out of the rule table below and
 *  this stands in only to satisfy `RuleRowContextValue`'s shape. */
const NO_CONTROLS = {
  busyId: null,
  onPause: () => {},
  onResume: () => {},
  onStop: () => {},
  onActivate: () => {},
};

/** The per-row action buttons, split out and memoized so a change to one row's
 *  state (or a global loading flag) only re-renders the rows whose buttons
 *  actually change — not every row's button subtree. The handlers are stable
 *  useCallbacks from the page; the booleans are pre-narrowed per row so an
 *  unaffected row's props stay shallow-equal and `memo` skips it.
 *
 *  Copy-params serializes via the unified `serializeRuleJson(SWING1_SPEC, …)`
 *  (every swing1 `p_*` column); the blob pastes back into the swing1 rule form or
 *  a swing1 sweep. */
const RuleActionsCell = memo(function RuleActionsCell({
  rule,
  confirmingDelete,
  deleteLoading,
  onEdit,
  onDuplicate,
  onDelete,
  onRequestDelete,
  onCancelDelete,
}: {
  rule: RuleRecord;
  confirmingDelete: boolean;
  deleteLoading: boolean;
  onEdit: (rule: RuleRecord) => void;
  onDuplicate: (rule: RuleRecord) => void;
  onDelete: (ruleId: string) => void;
  onRequestDelete: (ruleId: string) => void;
  onCancelDelete: () => void;
}) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(serializeRuleJson(SWING1_SPEC, rule));
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch { /* clipboard blocked — ignore */ }
  };

  if (confirmingDelete) {
    return (
      <div className="flex items-center justify-center gap-1">
        <span className="text-[11px] font-semibold text-red">Delete?</span>
        <Button variant="danger" size="xs" disabled={deleteLoading} onClick={() => onDelete(rule.id)}>
          Yes
        </Button>
        <Button variant="ghost" size="xs" onClick={onCancelDelete}>
          No
        </Button>
      </div>
    );
  }
  // Rule-management actions only (edit / duplicate / delete). The read-only
  // analysis tools (simulate / matched / paper) live in their own `Analyze`
  // column so the two intents read as separate groups. Icon-only — tooltips
  // name each action.
  return (
    <div className="flex items-center justify-center gap-1">
      <Button
        variant="ghost"
        size="xs"
        onClick={() => onEdit(rule)}
        title={
          rule.is_active || rule.open_positions > 0
            ? 'Live — only sizing (buy amount + concurrency) is editable'
            : 'Edit rule'
        }
        className="text-info"
      >
        ✎
      </Button>
      <Button
        variant="ghost"
        size="xs"
        onClick={() => onDuplicate(rule)}
        title="Duplicate — open a new rule pre-filled from this one"
        className="text-text-dim hover:text-text"
      >
        ⧉
      </Button>
      <Button
        variant="ghost"
        size="xs"
        onClick={handleCopy}
        title="Copy params to clipboard"
        className={copied ? 'text-green' : 'text-text-dim hover:text-text'}
      >
        {copied ? '✔' : '⎘'}
      </Button>
      <Button
        variant="ghost"
        size="xs"
        disabled={rule.is_active || rule.open_positions > 0}
        onClick={() => onRequestDelete(rule.id)}
        title={
          rule.is_active || rule.open_positions > 0
            ? 'Cannot delete a running rule or one with open positions'
            : 'Delete rule'
        }
        className="text-red"
      >
        ✕
      </Button>
    </div>
  );
});

export function Swing1Page() {
  const price = usePriceDisplay();
  const dispatch = useDispatch<AppDispatch>();
  const { timezone } = useTimezone();
  // Simulation progress/running is tracked app-wide so it survives navigation
  // (the backtest runs on the backend regardless); the global indicator renders
  // its progress bar + cancel.
  const { markStarting, markFinished } = useBackgroundJobActions();

  // Rule list: one initial load then a visibility-gated silent poll, deduped
  // into a shared hook (see usePolledRules). `loadRules` is the silent/forced
  // refresh used by the paper-test SSE handler below.
  const { rules, setRules, loading, error, refresh: loadRules } =
    usePolledRules(fetchSwing1Rules, STRATEGY);

  const [selectedRuleId, setSelectedRuleId] = useState<string | null>(null);
  // Positions for the selected rule are split into Current run + Old runs by
  // `RunPositionsPanel` (below); each section owns its own server-side table +
  // summary. The paper-result view keeps its own `useRulePositions` instance.

  const [modalOpen, setModalOpen] = useState(false);
  const [editRule, setEditRule] = useState<RuleRecord | null>(null);
  const [form, setForm] = useState<FormState>(() => emptyForm(SWING1_SPEC));
  const [formError, setFormError] = useState<string | null>(null);
  const [formLoading, setFormLoading] = useState(false);

  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [deleteLoading, setDeleteLoading] = useState(false);

  // Simulated + Matched are now server-side tables (POST + JSON, paged/sorted/
  // filtered on the backend), each driven by a `useServerTable` below. The page
  // only holds the open/active rule, the per-table view-state query, and the sim
  // start/error flags. `simRuleId` is set when the sim START POST fires and stays
  // until the `simulation_finished` SSE flips `simReady`, which enables the table.
  const [simRuleId, setSimRuleId] = useState<string | null>(null);
  const [simRuleName, setSimRuleName] = useState('');
  const [simReady, setSimReady] = useState(false);
  const [simQuery, setSimQuery] = useState<TableQuery>(DEFAULT_POSITIONS_QUERY);
  const [simError, setSimError] = useState<string | null>(null);
  const [simLoading, setSimLoading] = useState(false);

  // Matched view: which rule's matched set is open (null = closed) + its query.
  const [matchedRuleId, setMatchedRuleId] = useState<string | null>(null);
  const [matchedQuery, setMatchedQuery] = useState<TableQuery>(DEFAULT_POSITIONS_QUERY);
  // Transient creation-time window for matched + simulate (NOT persisted on any
  // rule). Empty = all-time. `datetime-local` strings; converted to ISO before the
  // request so they ride the matched/simulate RTK cache key (different ranges cache
  // separately). Scopes the backend's full-`tokens`-table scan.
  const [analysisFrom, setAnalysisFrom] = useState('');
  const [analysisTo, setAnalysisTo] = useState('');

  const [paperResult, setPaperResult] = useState<{
    ruleId: string;
    data: PaperResultResponse;
  } | null>(null);
  const [paperError, setPaperError] = useState<string | null>(null);
  const [paperLoading, setPaperLoading] = useState(false);
  const [paperClearing, setPaperClearing] = useState(false);
  // Token selected (in any result table) to inspect in the detail/chart modal.
  const [inspect, setInspect] = useState<InspectState | null>(null);
  // Transient banner shown when a paper test finishes (cap reached + all exited).
  const [paperNotice, setPaperNotice] = useState<PaperTestFinishedEvent | null>(null);
  // Mirror of the rule whose paper result is open, read by the SSE handler so it
  // can refresh that view (status → Finished) without re-subscribing the stream.
  const openPaperRuleId = useRef<string | null>(null);
  useEffect(() => {
    openPaperRuleId.current = paperResult?.ruleId ?? null;
  }, [paperResult]);

  // Server-side positions for the OPEN paper-result view (its own page/sort/filter
  // + whole-run summary, kept live via SSE). Scoped to the paper rule id; resolves
  // to that rule's latest paper run — the same run `paper-result` reports. A second
  // `useRulePositions` instance next to the selection-driven one above; the two
  // never conflict (different rule ids) and each aborts its own stale fetches.
  const [paperPosQuery, setPaperPosQuery] = useState<TableQuery>(DEFAULT_POSITIONS_QUERY);
  const {
    positions: paperPositions,
    total: paperPositionsTotal,
    summary: paperPositionsSummary,
    loading: paperPositionsLoading,
  } = useRulePositions(
    paperResult?.ruleId ?? null, rules, fetchSwing1RulePositions,
    fetchSwing1RulePositionsSummary, STRATEGY, paperPosQuery, POSITION_NUMERIC_COLS,
  );

  // Live paper-test completion: when a run finishes (cap reached + all exited)
  // the backend auto-deactivates the rule and broadcasts `paper_test_finished`.
  // Show a banner, refresh the rule list (so it flips to Inactive), and refresh
  // the open paper-result view if it's the finished rule.
  useEffect(() => {
    const es = connectPaperTestStream((ev) => {
      setPaperNotice(ev);
      loadRules(true);
      if (openPaperRuleId.current === ev.rule_id) {
        // The run just changed (status → Finished) — force-refetch past any
        // cached entry so the open view reflects the final state.
        fetchPaperResultCached(dispatch, { strategy: STRATEGY_SEG, ruleId: ev.rule_id }, true)
          .then((data) => setPaperResult({ ruleId: ev.rule_id, data }))
          .catch(() => {});
      }
    });
    return () => es.close();
  }, [loadRules, dispatch]);

  // The open paper-result token table is now a server-side positions table
  // (`useRulePositions` below, scoped to the paper rule) — it live-patches its
  // visible rows + summary from the same `tpsl_positions_changed` stream itself,
  // so no bespoke paper-delta patching is needed here. Run-meta (status →
  // Finished) still arrives via `paper_test_finished` above.
  useEffect(() => {
    if (!paperNotice) return;
    const id = setTimeout(() => setPaperNotice(null), 9000);
    return () => clearTimeout(id);
  }, [paperNotice]);

  // Rule table: no Run-control column — lab never executes a live/paper run
  // itself (no keys, no gRPC), that's deploy's job. Filtered once per mount.
  const ruleColumns = useMemo(
    () => allRuleColumns.filter((c) => c.key !== 'controls'),
    [],
  );
  // positionColumns/simColumns are referentially-stable module constants — their
  // price cells read the unit/rate from context, so a USD-rate tick no longer
  // rebuilds the column arrays or re-renders the whole table.
  const posCols = positionColumns;
  const simCols = simColumns;

  // Resolve the transient picker window to ISO bounds (UTC wall-clock, tz-aware),
  // omitting unset sides. Rides the matched/simulate request body so it both scopes
  // the backend scan and re-keys the server-side fetch per range.
  const analysisRange = useMemo(
    () => ({
      from: datetimeLocalToUtcWallClock(analysisFrom, timezone, 'lower') || undefined,
      to: datetimeLocalToUtcWallClock(analysisTo, timezone, 'upper') || undefined,
    }),
    [analysisFrom, analysisTo, timezone],
  );
  // Only pass a `range` when at least one bound is set — an empty range object
  // would still change the body key vs a truly all-time scan.
  const analysisRangeExtras = useMemo(
    () =>
      analysisRange.from || analysisRange.to
        ? { range: { from: analysisRange.from, to: analysisRange.to } }
        : undefined,
    [analysisRange],
  );

  // Serialized request bodies — memoized so a SOL/USD tick or unrelated re-render
  // doesn't churn the fetch key (the hook re-fetches on body change).
  const matchedBody = useMemo(
    () => toTableRequest(matchedQuery, MATCHED_NUMERIC_COLS, analysisRangeExtras),
    [matchedQuery, analysisRangeExtras],
  );
  const simBody = useMemo(
    () => toTableRequest(simQuery, SIM_NUMERIC_COLS, analysisRangeExtras),
    [simQuery, analysisRangeExtras],
  );
  const simSummaryBody = useMemo(
    () => toSummaryBody(simQuery, SIM_NUMERIC_COLS, analysisRangeExtras),
    [simQuery.search, simQuery.colFilters, simQuery.structuredFilters, analysisRangeExtras],
  );

  // Matched table: enabled while its view is open for the active rule.
  const {
    items: matchedTokens,
    total: matchedTotal,
    loading: matchedLoading,
    error: matchedError,
    reload: reloadMatched,
  } = useServerTable<MatchedTokenRecord>(
    matchedRuleId != null,
    matchedBody,
    (body, signal) =>
      fetchMatchedPage(STRATEGY_SEG, matchedRuleId ?? '', body as TableRequestBody, signal),
  );

  // Simulated table: enabled once a finished sim result exists for the rule.
  const {
    items: simTokens,
    total: simTotal,
    summary: simSummary,
    loading: simTableLoading,
    error: simTableError,
    reload: reloadSim,
  } = useServerTable<SimulatedTokenResult, SimulatedSummary>(
    simReady && simRuleId != null,
    simBody,
    (body, signal) =>
      fetchSimulatedPage(STRATEGY_SEG, simRuleId ?? '', body as TableRequestBody, signal),
    (summaryBody, signal) =>
      fetchSimulatedSummary(STRATEGY_SEG, simRuleId ?? '', summaryBody as TableRequestBody, signal),
    simSummaryBody,
  );

  const simCardSummary = useMemo(
    () => (simSummary ? simSummaryToCard(simSummary) : null),
    [simSummary],
  );

  const openAdd = () => {
    setEditRule(null);
    setForm(emptyForm(SWING1_SPEC));
    setFormError(null);
    setModalOpen(true);
  };

  const openEdit = useCallback((rule: RuleRecord) => {
    setEditRule(rule);
    setForm(formFromRule(SWING1_SPEC, rule));
    setFormError(null);
    setModalOpen(true);
  }, []);

  // Duplicate: open the form in CREATE mode (no editRule → every field editable,
  // no locks) pre-filled from the source rule, with a distinct name so the copy
  // saves as a brand-new rule via the create path.
  const openDuplicate = useCallback((rule: RuleRecord) => {
    setEditRule(null);
    setForm({ ...formFromRule(SWING1_SPEC, rule), [RULE_NAME_KEY]: `${rule.rule_name} (copy)` });
    setFormError(null);
    setModalOpen(true);
  }, []);

  const handleSave = async (unlocked: LockState) => {
    setFormError(null);
    if (!(form[RULE_NAME_KEY] ?? '').trim()) {
      setFormError('Rule name is required');
      return;
    }
    // buy amount is universal; the swing exit ladder requires take-profit +
    // stop-loss (the required exit-ladder columns).
    for (const [label, col] of [
      ['buy amount', 'buy_amount_sol'],
      ['take profit', 'p_exit_take_profit'],
      ['stop loss', 'p_exit_stop_loss'],
    ] as const) {
      const val = form[col] ?? '';
      if (!val.trim() || Number.isNaN(parseFloat(val))) {
        setFormError(`Invalid ${label}`);
        return;
      }
    }

    setFormLoading(true);
    try {
      if (editRule) {
        const updated = await updateSwing1Rule(
          editRule.id,
          buildUpdatePayload(SWING1_SPEC, form, unlocked),
        );
        setRules((prev) => prev.map((r) => (r.id === updated.id ? updated : r)));
        const fpChanged =
          editRule != null && fingerprintKey(editRule) !== fingerprintKey(updated);
        if (fpChanged) {
          invalidateStrategyMatched(dispatch, { strategy: STRATEGY_SEG, ruleId: updated.id });
          if (matchedRuleId === updated.id) reloadMatched();
        }
        if (simRuleId === updated.id) {
          setSimReady(false);
        }
      } else {
        const created = await createSwing1Rule(buildCreatePayload(SWING1_SPEC, form));
        setRules((prev) => [...prev, created]);
      }
      setModalOpen(false);
    } catch (e) {
      setFormError(e instanceof Error ? e.message : 'Save failed');
    } finally {
      setFormLoading(false);
    }
  };

  const handleDelete = useCallback(
    async (ruleId: string) => {
      setDeleteLoading(true);
      try {
        await deleteSwing1Rule(ruleId);
        setRules((prev) => prev.filter((r) => r.id !== ruleId));
        if (selectedRuleId === ruleId) setSelectedRuleId(null);
      } catch {
        /* ignore */
      } finally {
        setConfirmDeleteId(null);
        setDeleteLoading(false);
      }
    },
    [selectedRuleId, setRules],
  );

  // Start (or restart) a rule's backtest. The result is NOT collected here — the
  // sim table's `useServerTable` pages it from the server once `simulation_finished`
  // (below) flips `simReady` + reloads. Reset the query so a fresh run starts on
  // page 1. `markStarting`/`markFinished` drive the app-wide progress indicator.
  const handleSimulate = useCallback(
    async (rule: RuleRecord) => {
      setSimReady(false);
      setSimError(null);
      setSimRuleId(rule.id);
      setSimRuleName(rule.rule_name);
      setSimQuery(DEFAULT_POSITIONS_QUERY);
      markStarting('simulation', rule.id, `Sim: ${rule.rule_name}`);
      setSimLoading(true);
      try {
        await startSimulation(STRATEGY_SEG, rule.id, analysisRange);
      } catch (e) {
        setSimError(apiErrorMessage(e as Parameters<typeof apiErrorMessage>[0], 'Simulation failed'));
        setSimLoading(false);
        markFinished('simulation', rule.id);
      }
    },
    [markStarting, markFinished, analysisRange],
  );

  // Open / close the Matched view for a rule (toggle). Paging is driven by
  // `useServerTable` (enabled while `matchedRuleId` is set); switching rules resets
  // the query to page 1 so the new set starts fresh.
  const handleMatched = useCallback(
    (rule: RuleRecord) => {
      setMatchedRuleId((prev) => {
        if (prev === rule.id) return null; // second click closes
        setMatchedQuery(DEFAULT_POSITIONS_QUERY);
        return rule.id;
      });
    },
    [],
  );

  // Collect the first sim page + summary once the backend reports the backtest
  // finished (the same frame that clears the progress bar). Scoped to the rule the
  // page kicked off; other rules' runs are ignored.
  useEffect(() => {
    const handle = connectSimulationFinished((ev) => {
      if (ev.rule_id !== simRuleId) return;
      setSimReady(true);
      setSimLoading(false);
      markFinished('simulation', ev.rule_id);
      reloadSim();
    });
    return () => handle.close();
  }, [simRuleId, markFinished, reloadSim]);

  const handlePaperResult = useCallback(
    async (rule: RuleRecord) => {
      // Toggle: a second click on the open rule closes the result.
      if (paperResult?.ruleId === rule.id) {
        setPaperResult(null);
        setPaperError(null);
        return;
      }
      setPaperResult(null);
      setPaperError(null);
      setPaperLoading(true);
      try {
        const data = await fetchPaperResultCached(dispatch, { strategy: STRATEGY_SEG, ruleId: rule.id });
        setPaperResult({ ruleId: rule.id, data });
      } catch (e) {
        setPaperError(
          apiErrorMessage(e as Parameters<typeof apiErrorMessage>[0], 'Failed to load paper result'),
        );
      } finally {
        setPaperLoading(false);
      }
    },
    [paperResult, dispatch],
  );

  // Clear the open paper rule's recorded run history. Backend rejects live rules
  // (the Clear button is disabled for them too), so this only fires when idle.
  // On success: close the panel, drop the cached paper result, and refresh the
  // rule list so its lifecycle flips back to Idle.
  const handleClearPaperResult = useCallback(async () => {
    if (!paperResult) return;
    const ruleId = paperResult.ruleId;
    setPaperClearing(true);
    setPaperError(null);
    try {
      await clearSwing1PaperResult(ruleId);
      invalidatePaperResult(dispatch, { strategy: STRATEGY_SEG, ruleId });
      setPaperResult(null);
      loadRules(true);
    } catch (e) {
      setPaperError(e instanceof Error ? e.message : 'Failed to clear results');
    } finally {
      setPaperClearing(false);
    }
  }, [paperResult, dispatch, loadRules]);

  // Cancel a pending delete confirmation; stable so it doesn't churn the cell.
  const cancelDelete = useCallback(() => setConfirmDeleteId(null), []);

  // Read-only analysis tools for the `Analyze` column. Defined here (after the
  // result handlers) and rebuilt when a tool's loading flag flips or the open
  // result panel changes, so only the affected buttons restyle.
  const ruleAnalysis = useMemo(
    () => ({
      simLoading,
      matchedLoading,
      paperLoading,
      matchedActiveId: matchedRuleId,
      paperActiveId: paperResult?.ruleId ?? null,
      onSimulate: handleSimulate,
      onMatched: handleMatched,
      onPaperResult: handlePaperResult,
    }),
    [
      simLoading,
      matchedLoading,
      paperLoading,
      matchedRuleId,
      paperResult,
      handleSimulate,
      handleMatched,
      handlePaperResult,
    ],
  );
  // Single context value for the Run/Analyze cells — `ruleColumns` is now a
  // static array, so only the cells (not the column defs) re-read these.
  const rowContext = useMemo(
    () => ({ controls: NO_CONTROLS, analysis: ruleAnalysis }),
    [ruleAnalysis],
  );

  // Row-action cell renders the memoized <RuleActionsCell> (edit/delete only):
  // narrowing the per-row booleans here means a global toggle only re-renders
  // the rows whose buttons actually change. Selection, polling and the banner
  // timer never touch these deps, so they don't churn it.
  const ruleActions = useCallback(
    (rule: RuleRecord) => (
      <RuleActionsCell
        rule={rule}
        confirmingDelete={confirmDeleteId === rule.id}
        deleteLoading={deleteLoading}
        onEdit={openEdit}
        onDuplicate={openDuplicate}
        onDelete={handleDelete}
        onRequestDelete={setConfirmDeleteId}
        onCancelDelete={cancelDelete}
      />
    ),
    [confirmDeleteId, deleteLoading, openEdit, openDuplicate, handleDelete, cancelDelete],
  );

  // The rule whose paper result is open — used to gate "Clear results" (idle only).
  const paperCanClear = useMemo(() => {
    if (!paperResult) return false;
    const r = rules.find((x) => x.id === paperResult.ruleId);
    return !!r && !r.is_active && r.open_positions === 0;
  }, [paperResult, rules]);

  const matchedRuleName = useMemo(
    () => (matchedRuleId ? rules.find((r) => r.id === matchedRuleId)?.rule_name : null),
    [matchedRuleId, rules],
  );

  const selectedRuleName = useMemo(
    () => (selectedRuleId ? rules.find((r) => r.id === selectedRuleId)?.rule_name ?? null : null),
    [selectedRuleId, rules],
  );

  // Which table owns the selected rule — the Positions section renders directly
  // under that table (real positions below the Real table, paper positions below
  // the Paper table) so it stays visually attached to the rule it belongs to.
  const selectedRuleMode = useMemo(
    () => rules.find((r) => r.id === selectedRuleId)?.trade_mode ?? null,
    [selectedRuleId, rules],
  );
  const isRealRuleSelected = selectedRuleMode === 'real';
  const isPaperRuleSelected = selectedRuleId != null && selectedRuleMode != null && !isRealRuleSelected;

  // Split the single rule list into real vs paper so each renders in its own
  // table (real on top, flagged as live). Lab never executes real rules — the
  // partition only fills if a rule was authored/synced with trade_mode=real.
  const realRules = useMemo(() => rules.filter((r) => r.trade_mode === 'real'), [rules]);
  const paperRules = useMemo(() => rules.filter((r) => r.trade_mode !== 'real'), [rules]);

  // Stable row-select handlers so the result tables' memoized rows survive an
  // unrelated page render (these closures are passed straight to DataTable).
  const onInspectPosition = useCallback(
    (row: RulePositionRecord | null) => {
      setInspect(
        row
          ? { table: 'positions', key: row.id, target: inspectFromPosition(row), ruleId: selectedRuleId }
          : null,
      );
    },
    [selectedRuleId],
  );

  const onSelectSim = useCallback(
    (key: string | null) => {
      const row = key ? simTokens.find((t) => t.mint_address === key) ?? null : null;
      setInspect(
        row
          ? {
              table: 'sim',
              key: row.mint_address,
              target: inspectFromSim(row),
              ruleId: simRuleId,
              carriedLegs: row.swing_legs ?? null,
            }
          : null,
      );
    },
    [simTokens, simRuleId],
  );

  const onSelectMatched = useCallback(
    (key: string | null) => {
      const row = key ? matchedTokens.find((t) => t.mint_address === key) ?? null : null;
      setInspect(
        row ? { table: 'matched', key: row.mint_address, target: inspectFromMatched(row), ruleId: null } : null,
      );
    },
    [matchedTokens],
  );

  // The paper table is now a server-side positions table (id-keyed rows), so this
  // resolves the clicked position by id — mirroring `onSelectPosition`.
  const onSelectPaperToken = useCallback(
    (key: string | null) => {
      const row = key ? paperPositions.find((p) => p.id === key) ?? null : null;
      setInspect(
        row
          ? { table: 'paper', key: row.id, target: inspectFromPosition(row), ruleId: paperResult?.ruleId ?? null }
          : null,
      );
    },
    [paperPositions, paperResult],
  );

  // Charts-grid overlays: each section draws entry/exit + the swing legs the
  // `swing1-detect` funnel reconstructs from that section's rule params (the same
  // legs its inspect modal shows). Sim rows carry their own legs, so those skip
  // the fetch. Keyed off the section's rule so a rule switch re-detects.
  const positionRowOverlay = useMemo(
    () => makeSwing1DetectRowOverlay(inspectFromPosition, swing1ParamsFromRule(rules.find((r) => r.id === selectedRuleId))),
    [rules, selectedRuleId],
  );
  const paperRowOverlay = useMemo(
    () => makeSwing1DetectRowOverlay(inspectFromPosition, swing1ParamsFromRule(rules.find((r) => r.id === paperResult?.ruleId))),
    [rules, paperResult],
  );
  const matchedRowOverlay = useMemo(
    () => makeSwing1DetectRowOverlay(inspectFromMatched, swing1ParamsFromRule(rules.find((r) => r.id === matchedRuleId))),
    [rules, matchedRuleId],
  );
  const simRowOverlay = useMemo(
    () =>
      makeSwing1DetectRowOverlay(
        inspectFromSim,
        swing1ParamsFromRule(rules.find((r) => r.id === simRuleId)),
        (r) => r.swing_legs ?? null,
      ),
    [rules, simRuleId],
  );

  // Positions for the selected rule — split into Current run + Old runs. Rendered
  // under whichever table owns the rule (real → below Real, paper → below Paper);
  // only one of isReal/isPaperRuleSelected is true at a time, so it renders once.
  const positionsSection = (
    <RunPositionsPanel
      strategy={STRATEGY}
      selectedRuleId={selectedRuleId}
      selectedRuleName={selectedRuleName}
      rules={rules}
      columns={posCols}
      existingKeys={POSITION_KEYS}
      fetchPositions={fetchSwing1RulePositions}
      fetchSummary={fetchSwing1RulePositionsSummary}
      price={price}
      selectedKey={inspect?.table === 'positions' ? inspect.key : null}
      onInspect={onInspectPosition}
      useRowOverlay={positionRowOverlay}
      isReal={isRealRuleSelected}
    />
  );

  return (
    <div>
      {paperNotice && (
        <div className="mb-4 flex items-center gap-3 rounded-lg border border-primary/40 bg-primary/10 px-4 py-3">
          <span className="text-base text-primary">✓</span>
          <div className="flex-1 text-sm text-text">
            <span className="font-bold text-primary">Paper test finished</span>
            <span className="text-text-dim"> — </span>
            <span className="font-mono">{paperNotice.rule_name}</span>
            <span className="text-text-dim">
              {' '}(run #{paperNotice.run_seq}, {paperNotice.tokens_traded} tokens). Rule
              deactivated.
            </span>
          </div>
          <button
            type="button"
            onClick={() => setPaperNotice(null)}
            className="text-text-dim transition hover:text-text"
          >
            ✕
          </button>
        </div>
      )}

      <SectionHeading
        size="h2"
        title="Swing 1 Strategies"
        count={!loading && !error ? rules.length : undefined}
        action={
          <Button variant="primary" onClick={openAdd}>
            + Add Rule
          </Button>
        }
      />

      {loading && <p className="text-text-dim">Loading rules…</p>}
      {error && <InlineAlert variant="error">{error}</InlineAlert>}

      {modalOpen && (
        <div className="mb-5">
          <SpecRuleForm
            spec={SWING1_SPEC}
            open={modalOpen}
            editRule={editRule}
            loading={formLoading}
            error={formError}
            form={form}
            onChange={setForm}
            onClose={() => setModalOpen(false)}
            onSave={handleSave}
          />
        </div>
      )}

      {/* Transient creation-time window for Matched + Simulate (not saved on any
          rule). Empty = all-time; set a range to bound the full-table scan. */}
      <div className="mb-3 flex flex-wrap items-end gap-3 text-[11px] text-text-dim">
        <span className="font-semibold uppercase tracking-wide">Analysis window</span>
        <label className="flex items-center gap-1.5">
          From
          <Input
            type="datetime-local"
            value={analysisFrom}
            onChange={(e) => setAnalysisFrom(e.target.value)}
          />
        </label>
        <label className="flex items-center gap-1.5">
          To
          <Input
            type="datetime-local"
            value={analysisTo}
            onChange={(e) => setAnalysisTo(e.target.value)}
          />
        </label>
        {(analysisFrom || analysisTo) && (
          <Button
            variant="ghost"
            size="sm"
            onClick={() => {
              setAnalysisFrom('');
              setAnalysisTo('');
            }}
          >
            Clear
          </Button>
        )}
        <span className="text-text-dim/70">
          Scopes Matched / Simulate by token creation time. Empty = all tokens.
        </span>
      </div>

      {!loading && !error && (
        <RuleRowProvider value={rowContext}>
          {realRules.length > 0 && (
            <Accordion
              className="mb-4"
              bordered={false}
              header={
                <RuleTableHeader
                  title="Real Trading"
                  marker="bg-red"
                  badge="danger"
                  count={realRules.length}
                  subtitle="Live on-chain — buys & sells execute for real"
                />
              }
            >
              <DataTable
                columns={ruleColumns}
                rows={realRules}
                rowKey={keyById}
                rowActions={ruleActions}
                selectedKey={selectedRuleId}
                onSelect={setSelectedRuleId}
                searchable
                colFilters
                colToggle
                tableId="swing1_rules_real"
                emptyMessage="No real rules"
              />
            </Accordion>
          )}

          {isRealRuleSelected && positionsSection}

          {realRules.length > 0 && <SectionDivider gap="xl" />}

          <Accordion
            bordered={false}
            header={
              <RuleTableHeader
                title="Paper Trading"
                marker="bg-info"
                badge="info"
                count={paperRules.length}
                subtitle="Simulated — no on-chain execution"
              />
            }
          >
            <DataTable
              columns={ruleColumns}
              rows={paperRules}
              rowKey={keyById}
              rowActions={ruleActions}
              selectedKey={selectedRuleId}
              onSelect={setSelectedRuleId}
              searchable
              colFilters
              colToggle
              tableId="swing1_rules_paper"
              emptyMessage="No paper rules"
            />
          </Accordion>

          {isPaperRuleSelected && positionsSection}
        </RuleRowProvider>
      )}

      {(matchedRuleId || matchedError) && <SectionDivider />}
      {matchedError && <InlineAlert variant="error">{matchedError}</InlineAlert>}
      {matchedRuleId && !matchedError && (
        <section>
          <SectionHeading
            title="Matched Tokens"
            marker="bg-[#9370db]"
            badge="neutral"
            badgeClass="border-[#9370db]/40 bg-[#9370db]/12 text-[#9370db]"
            count={matchedTotal}
            subtitle={matchedRuleName ?? undefined}
            action={
              <button
                type="button"
                onClick={() => setMatchedRuleId(null)}
                className="text-text-dim transition hover:text-text"
              >
                ✕
              </button>
            }
          />
          <TokenTable
            columns={matchedColumns}
            existingKeys={MATCHED_KEYS}
            mintSetFilter
            charts
            useRowOverlay={matchedRowOverlay}
            rows={matchedTokens}
            rowKey={keyByMint}
            selectedKey={inspect?.table === 'matched' ? inspect.key : null}
            onSelect={onSelectMatched}
            serverSide
            serverTotal={matchedTotal}
            onQueryChange={setMatchedQuery}
            loading={matchedLoading}
            resetKey={matchedRuleId}
            searchable
            colFilters
            colToggle
            tableId="swing1_matched"
            emptyMessage="No tokens in the database match this rule's entry criteria."
          />
        </section>
      )}

      {(simError || simTableError || simReady) && <SectionDivider />}
      {(simError || simTableError) && (
        <InlineAlert variant="error">{simError ?? simTableError}</InlineAlert>
      )}
      {simReady && !simError && (
        <>
          {/* Summary is server-computed over the filtered cohort (not the visible page). */}
          {simCardSummary && (
            <SimSummaryCard
              ruleName={simRuleName}
              summary={simCardSummary}
              price={price}
              onClose={() => {
                setSimReady(false);
                setSimRuleId(null);
                setSimError(null);
              }}
            />
          )}
          <section>
            <SectionHeading
              title="Simulated Tokens"
              count={simTotal}
              subtitle={simRuleName}
            />
            <TokenTable
              columns={simCols}
              existingKeys={SIM_KEYS}
              mintSetFilter
              charts
              useRowOverlay={simRowOverlay}
              rows={simTokens}
              rowKey={keyByMint}
              selectedKey={inspect?.table === 'sim' ? inspect.key : null}
              onSelect={onSelectSim}
              serverSide
              serverTotal={simTotal}
              onQueryChange={setSimQuery}
              loading={simTableLoading}
              resetKey={simRuleId ?? ''}
              searchable
              colFilters
              colToggle
              tableId="swing1_sim"
              emptyMessage="No tokens matched this rule's entry criteria."
            />
          </section>
        </>
      )}

      {(paperLoading || paperError || paperResult) && <SectionDivider />}
      {paperLoading && <p className="text-text-dim">Loading paper-test result…</p>}
      {paperError && <InlineAlert variant="error">{paperError}</InlineAlert>}
      {paperResult && !paperLoading && (
        <PaperResultSection
          data={paperResult.data}
          positionColumns={posCols}
          positionKeys={POSITION_KEYS}
          positions={paperPositions}
          positionsTotal={paperPositionsTotal}
          positionsSummary={paperPositionsSummary}
          positionsLoading={paperPositionsLoading}
          price={price}
          tableId="swing1_paper"
          resetKey={paperResult.ruleId}
          selectedKey={inspect?.table === 'paper' ? inspect.key : null}
          onSelectPosition={onSelectPaperToken}
          onQueryChange={setPaperPosQuery}
          onClose={() => {
            setPaperResult(null);
            setPaperError(null);
          }}
          onClear={handleClearPaperResult}
          clearing={paperClearing}
          canClear={paperCanClear}
          useRowOverlay={paperRowOverlay}
        />
      )}

      {inspect && (
        <SwingRuleInspectModal
          target={inspect.target}
          rule={inspect.ruleId ? rules.find((r) => r.id === inspect.ruleId) : undefined}
          carriedLegs={inspect.carriedLegs}
          onClose={() => setInspect(null)}
        />
      )}

    </div>
  );
}
