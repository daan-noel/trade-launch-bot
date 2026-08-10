import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { TokenTable } from 'components/tokens/TokenTable';
import { InlineAlert } from 'components/ui/Modal';
import { Badge } from 'components/ui/Badge';
import { IconButton } from 'components/ui/IconButton';
import { IconButtonGroup } from 'components/ui/IconButtonGroup';
import { PauseIcon, PlayIcon, SpinnerIcon, StopIcon } from 'components/ui/icons';
import { ModeBadge } from 'components/strategy/ModeBadge';
import {
  capitalDetailSection,
  PositionSummarySection,
} from 'components/strategy/PositionSummarySection';
import { toRunSummaryFromPositions } from 'components/strategy/SimSummaryCard';
import {
  positionColumns,
  POSITION_KEYS,
} from 'components/strategy/strategyColumns';
import { inspectFromPosition, markerRowOverlay } from 'components/strategy/inspectTarget';
import {
  PositionChartCardExtra,
  positionChartFactsFromRule,
} from 'components/strategy/PositionChartCardExtra';
import { useResolvedFlowPatternKeys } from 'hooks/useFlowPatternKeys';
import { usePriceDisplay } from 'hooks/usePriceDisplay';
import { useServerTable, DEFAULT_POSITIONS_QUERY } from 'hooks/useServerTable';
import { numericColKeys, toSummaryBody, toTableRequest } from 'services/tableRequest';
import {
  fetchRulePositionsPage,
  fetchRulePositionsSummary,
  type PositionFetchScope,
} from 'services/api';
import { fetchAllTablePages } from 'lib/strategy/fetchPositionChartSeries';
import {
  buildFocusTableFilters,
  type PositionFocusLens,
} from 'lib/strategy/positionFocus';
import type { PositionChartPoint } from 'lib/strategy/positionChartPoints';
import { connectStrategyPositionUpdate } from 'services/sse';
import { apiErrorMessage } from 'store/baseApi';
import {
  useActivateStrategyRuleMutation,
  useGetStrategyRuleRunsQuery,
  usePauseStrategyRuleMutation,
  useStopStrategyRuleMutation,
} from 'store/sharedEndpoints';
import { useTimezone } from 'context/TimezoneContext';
import type { TableQuery } from 'components/table/types';
import type { FilterSpec } from 'components/table/numericFilter';
import type { PositionsSummary, RulePositionRecord } from 'types';
import type { StrategyRule, StrategyRuleRun } from 'lib/strategy/types';
import type { TemporalRow } from 'lib/strategy/temporalSummary';
import { signedToneClass } from 'lib/signedTone';

const STRATEGY_SEG = 'generic';
const POS_NUMERIC = numericColKeys(positionColumns);
const posRowOverlay = markerRowOverlay(inspectFromPosition);

/**
 * Evidence-table deviation from the appended token-info defaults. The row is a
 * closed position, so the token's `token_amount` is the size actually held —
 * the sanity check against `entry_sol`/`exit_sol` — rather than the background
 * fact it is on a token list, where the shared set hides it.
 */
const EVIDENCE_DEFAULT_COLS = { token_amount: true } as const;

const TABLE_RELOAD_STATUSES = new Set([
  'Holding',
  'End',
  'EntryFailed',
  'ExitStuck',
  'ExitUnconfirmed',
]);

/**
 * Evidence scope: current run · one run #N · all-time (both modes or one).
 *
 * A run is keyed by `(runSeq, mode)`, never `runSeq` alone — `run_seq` restarts per
 * mode, so a rule toggled from real to paper owns two runs called `#1`. `all` spans
 * both modes (the rule's whole history, not the history of its current switch
 * position); `current` is the newest run in the mode the rule is in now.
 *
 * `all` + `mode` is the same all-time population narrowed to one trade mode — the
 * per-mode ledger, which is what you compare against. It rides the server-side `mode`
 * filter rather than a fourth server scope: one predicate for the page, the pager
 * total, the summary card and the chart series, so they cannot drift apart.
 */
export type EvidenceScope =
  | { kind: 'current' }
  | { kind: 'run'; runSeq: number; mode: string }
  | { kind: 'all'; mode?: string };

const analyzePositionRowKey = (r: RulePositionRecord) => r.id;

function holdingSecs(r: RulePositionRecord): number {
  if (!r.entry_time) return 0;
  const start = Date.parse(r.entry_time);
  if (!Number.isFinite(start)) return 0;
  const end = r.exit_time ? Date.parse(r.exit_time) : Date.now();
  if (!Number.isFinite(end)) return 0;
  return Math.max(0, (end - start) / 1000);
}

function toTemporalRow(r: RulePositionRecord): TemporalRow {
  const open = !r.exit_time && r.status !== 'End' && r.status !== 'EntryFailed';
  return {
    mint_address: r.mint_address,
    fired: r.entry_price != null,
    exit: open ? 'Open' : (r.exit_reason ?? r.status),
    pnl_sol: r.pnl_sol ?? 0,
    holding_secs: holdingSecs(r),
    entry_time: r.entry_time,
    created_at: r.created_at,
    exit_time: r.exit_time,
  };
}

function evidenceChartPoints(rows: RulePositionRecord[]): PositionChartPoint[] {
  const out: PositionChartPoint[] = [];
  for (const r of rows) {
    if (r.entry_price == null) continue;
    const open = !r.exit_time && r.status !== 'End' && r.status !== 'EntryFailed';
    // Open rows often lack realized pnl on the wire — still plot when %/SOL exist.
    const pnlSol = r.pnl_sol;
    const pnlPct = r.pnl_percent;
    if (!open && (pnlSol == null || !Number.isFinite(pnlSol))) continue;
    const timeIso = open ? r.entry_time ?? r.created_at : r.exit_time ?? r.entry_time;
    const timeMs = timeIso ? Date.parse(timeIso) : NaN;
    if (!Number.isFinite(timeMs)) continue;
    const hold = holdingSecs(r);
    out.push({
      key: r.id,
      label: r.symbol || r.mint_address.slice(0, 8),
      mint_address: r.mint_address,
      timeMs,
      pnlSol: pnlSol ?? 0,
      pnlPct: pnlPct ?? null,
      holdSeconds: hold > 0 ? hold : open ? 1 : null,
      entrySol: r.entry_sol ?? 0.1,
      isOpen: open,
      exit_reason: open ? null : r.exit_reason,
      is_migrated: r.is_migrated ?? null,
    });
  }
  return out;
}

function toFetchScope(s: EvidenceScope): PositionFetchScope {
  if (s.kind === 'run') return { kind: 'run', runSeq: s.runSeq, mode: s.mode };
  // A mode-pinned all-time is still `scope=all` on the wire — the narrowing is a
  // filter, so it lands on the summary and the series with no second code path.
  return { kind: s.kind };
}

function scopeKey(s: EvidenceScope): string {
  if (s.kind === 'run') return `run:${s.mode}:${s.runSeq}`;
  if (s.kind === 'all' && s.mode) return `all:${s.mode}`;
  return s.kind;
}

/** The server-side filter a mode-pinned all-time scope narrows with, if any. */
function scopeFiltersFor(s: EvidenceScope): Record<string, FilterSpec> {
  return s.kind === 'all' && s.mode ? { mode: { op: 'eq', val: s.mode } } : {};
}

function formatRunPnl(run: StrategyRuleRun): string {
  if (!run.has_metrics || run.total_pnl_sol == null) return '—';
  const v = run.total_pnl_sol;
  return `${v > 0 ? '+' : ''}${v.toFixed(3)}◎`;
}

/** Win rate (0–1 fraction) → `NN%`, or `—`. */
function formatWinRate(v: number | null | undefined): string {
  return v == null || !Number.isFinite(v) ? '—' : `${Math.round(v * 100)}%`;
}

/** A one-line description of a run's outcome for the chip/bar hover title. */
function runTitle(run: StrategyRuleRun): string {
  if (!run.has_metrics) return `Run #${run.run_seq} · ${run.status}`;
  const parts = [`Run #${run.run_seq}`, `PnL ${formatRunPnl(run)}`];
  if (run.win_rate != null) parts.push(`Win ${formatWinRate(run.win_rate)}`);
  if (run.expectancy_sol != null) {
    const e = run.expectancy_sol;
    parts.push(`Exp ${e > 0 ? '+' : ''}${e.toFixed(4)}◎`);
  }
  if (run.n_closed != null) {
    parts.push(`${run.n_closed} closed${run.n_open ? ` · ${run.n_open} open` : ''}`);
  }
  return parts.join(' · ');
}

export interface RuleAnalyzePanelProps {
  ruleId: string;
  rule?: StrategyRule | null;
  /** Compact header when embedded under the Rules Control table. */
  embedded?: boolean;
  onClose?: () => void;
  /** Align Evidence default with Control score scope when selecting a rule. */
  initialScopeKind?: 'current' | 'all';
  /** Open-position count from the live SSE status slice (live app only). Omit on
   *  the lab app — there is no engine there, so "N open live" would be a lie. */
  liveOpenCount?: number | null;
  /** Subscribe to `strategy_position_update` SSE and reload the table on status
   *  transitions. Live app only; the lab bin emits no such stream. */
  liveUpdates?: boolean;
  /** Rendered under the header — e.g. the lab's "snapshot as of last sync" note. */
  notice?: ReactNode;
  /** Injects a token-inspect modal for the selected position. When provided, the
   *  positions table becomes selectable (row + chart-card click) and this renders
   *  the open modal. Live passes `LivePositionInspectModal` (chart + fills; no
   *  metric panes — that bin has no `metric-series` route); lab passes
   *  `LabTokenInspectModal` (chart + metric panes). */
  renderInspect?: (ctx: {
    position: RulePositionRecord;
    rule: StrategyRule | null;
    onClose: () => void;
  }) => ReactNode;
}

/**
 * Rules Evidence — run navigator + Positions Summary + temporal + history.
 *
 * Shared by both apps: the **live** app embeds it under Rules Control with the SSE
 * position feed, open count, and a Floor chart/fills inspect modal; the **lab** app
 * renders it over the synced position mirror with metric-pane inspect.
 *
 * Activate/Pause/Stop render in both. On the lab app they hit the lab CRUD routes,
 * so they flip the rule in the **local mirror** only — the live engine doesn't see
 * it, and the next `db-incremental-sync.ps1` overwrites the row (server wins).
 */
export function RuleAnalyzePanel({
  ruleId,
  rule,
  embedded,
  onClose,
  initialScopeKind = 'current',
  liveOpenCount = null,
  liveUpdates = false,
  notice,
  renderInspect,
}: RuleAnalyzePanelProps) {
  const price = usePriceDisplay();
  const { timezone } = useTimezone();
  const flowPatternKeys = useResolvedFlowPatternKeys({
    fingerprintId: rule?.fingerprint_id,
    ruleId,
  });
  const [scope, setScope] = useState<EvidenceScope>(() =>
    initialScopeKind === 'all' ? { kind: 'all' } : { kind: 'current' },
  );
  const [query, setQuery] = useState<TableQuery>(DEFAULT_POSITIONS_QUERY);
  const [focus, setFocus] = useState<PositionFocusLens[]>([]);
  const [chartPoints, setChartPoints] = useState<PositionChartPoint[]>([]);
  const [temporalBaseRows, setTemporalBaseRows] = useState<TemporalRow[]>([]);
  const [opErr, setOpErr] = useState<string | null>(null);
  const [pausing, setPausing] = useState(false);
  const [inspectId, setInspectId] = useState<string | null>(null);
  /** Guards the one-shot empty-current-run fallback below; set once it has fired or
   *  the user has picked a scope, so their choice is never overridden. */
  const autoScoped = useRef(false);
  const wasLoading = useRef(false);
  const loadSettled = useRef(false);

  const { data: runs = [], isFetching: runsLoading } = useGetStrategyRuleRunsQuery(ruleId, {
    skip: !ruleId,
  });
  const [activate] = useActivateStrategyRuleMutation();
  const [pause] = usePauseStrategyRuleMutation();
  const [stop] = useStopStrategyRuleMutation();

  // The endpoint serves both modes, the rule's own mode first (newest-first within
  // each). Split them here: "current" and the PnL trend only mean something inside
  // one mode, while the other mode's runs stay reachable as their own chips —
  // flipping `trade_mode` must not hide the history the rule already recorded.
  const ownMode = rule?.trade_mode ?? runs[0]?.mode ?? 'real';
  const ownRuns = useMemo(() => runs.filter((r) => r.mode === ownMode), [runs, ownMode]);
  const otherRuns = useMemo(() => runs.filter((r) => r.mode !== ownMode), [runs, ownMode]);

  const currentRun = ownRuns[0] ?? null;
  const priorRuns = ownRuns.slice(1);

  // Modes to offer as their own all-time ledger, the rule's own first. Only when it
  // has actually traded in both: with one mode "All-time" already IS that mode, and
  // a second chip repeating it is noise.
  const allTimeModes = useMemo(() => {
    const others = [...new Set(otherRuns.map((r) => r.mode))];
    return others.length > 0 ? [ownMode, ...others] : [];
  }, [otherRuns, ownMode]);

  // Runs with a finalized PnL, oldest→newest, for the cross-run trend strip. Own
  // mode only: paper PnL and real PnL are not one series (paper pays no slippage a
  // real fill pays), so one strip mixing them reads as decay that never happened.
  const trendRuns = useMemo(
    () =>
      ownRuns
        .filter((r) => r.has_metrics && r.total_pnl_sol != null)
        .slice()
        .reverse(),
    [ownRuns],
  );

  const focusFilters = useMemo(
    () => buildFocusTableFilters(focus, chartPoints, timezone, 'positionId'),
    [focus, chartPoints, timezone],
  );

  // The scope's own narrowing (mode-pinned all-time) rides with the focus lenses:
  // both are wrapper-injected predicates the user never typed, and both must reach
  // the page, the pager total AND the summary card or those three disagree.
  const scopeFilters = useMemo(() => scopeFiltersFor(scope), [scope]);
  const injectedFilters = useMemo(
    () => ({ ...scopeFilters, ...focusFilters }),
    [scopeFilters, focusFilters],
  );

  const body = useMemo(() => {
    const base = toTableRequest(query, POS_NUMERIC);
    if (Object.keys(injectedFilters).length === 0) return base;
    return {
      ...base,
      filters: {
        ...base.filters,
        ...injectedFilters,
      },
    };
  }, [query, injectedFilters]);

  const summaryBody = useMemo(() => {
    const base = toSummaryBody(query, POS_NUMERIC);
    if (Object.keys(injectedFilters).length === 0) return base;
    return {
      ...base,
      filters: {
        ...base.filters,
        ...injectedFilters,
      },
    };
  }, [query, injectedFilters]);

  const fetchScope = useMemo(() => toFetchScope(scope), [scope]);

  const fetchPage = useCallback(
    (b: unknown, signal: AbortSignal) =>
      fetchRulePositionsPage(STRATEGY_SEG, ruleId, b as never, fetchScope, signal),
    [ruleId, fetchScope],
  );
  const fetchSummary = useCallback(
    (b: unknown, signal: AbortSignal) =>
      fetchRulePositionsSummary(STRATEGY_SEG, ruleId, b as never, fetchScope, signal),
    [ruleId, fetchScope],
  );

  const { items, total, summary, loading, error, reload } = useServerTable<
    RulePositionRecord,
    PositionsSummary
  >(!!ruleId, body, fetchPage, fetchSummary, summaryBody, `${ruleId}:${scopeKey(scope)}`);

  useEffect(() => {
    setFocus([]);
    setScope(initialScopeKind === 'all' ? { kind: 'all' } : { kind: 'current' });
    setQuery(DEFAULT_POSITIONS_QUERY);
    setOpErr(null);
    setPausing(false);
    setInspectId(null);
    setChartPoints([]);
    setTemporalBaseRows([]);
    autoScoped.current = false;
    loadSettled.current = false;
  }, [ruleId, initialScopeKind]);

  // One-shot fallback out of an empty "current run". Restarting a rule opens a
  // brand-new run that has traded nothing yet, and flipping `trade_mode` leaves it
  // with no current run at all — either way the default scope pages an empty table
  // while the rule's whole history sits one chip away, which reads as "the rule has
  // no positions". Fires only on a settled load of the pristine view, so an empty
  // run the user picked (or filtered down to) stays empty and stays put.
  const queryIsPristine =
    !query.search &&
    Object.keys(query.colFilters).length === 0 &&
    Object.keys(query.structuredFilters ?? {}).length === 0;
  useEffect(() => {
    // Latched, not edge-triggered: the run list arrives on its own request, so the
    // decision often can't be made on the tick the page load lands.
    if (wasLoading.current && !loading) loadSettled.current = true;
    wasLoading.current = loading;
    if (!loadSettled.current || autoScoped.current) return;
    if (scope.kind !== 'current' || error || total !== 0) return;
    if (!queryIsPristine || focus.length > 0) return;
    // Nothing to fall back to when the current run is the rule's only run.
    if (runs.length === 0 || (currentRun != null && runs.length === 1)) return;
    autoScoped.current = true;
    setScope({ kind: 'all' });
  }, [loading, error, total, scope.kind, queryIsPristine, focus.length, runs.length, currentRun]);

  const [seriesTick, setSeriesTick] = useState(0);
  useEffect(() => {
    if (!ruleId || !liveUpdates) return;
    const h = connectStrategyPositionUpdate((d) => {
      if (d.rule_id !== ruleId) return;
      if (!TABLE_RELOAD_STATUSES.has(d.status)) return;
      reload();
      setSeriesTick((t) => t + 1);
    });
    return () => h.close();
  }, [ruleId, reload, liveUpdates]);

  // Full-cohort series for charts + Temporal (table filters only; focus applied in shell / request).
  const tableFilterKey = JSON.stringify({
    s: query.search,
    c: query.colFilters,
    f: query.structuredFilters,
  });
  useEffect(() => {
    setFocus([]);
  }, [tableFilterKey, scope]);

  useEffect(() => {
    if (!ruleId) return;
    const ctrl = new AbortController();
    // Scope filters only — the focus lenses filter *against* this cohort, so folding
    // them in here would shrink the very population the lenses are drawn from.
    const base = toSummaryBody(query, POS_NUMERIC);
    const seriesBody =
      Object.keys(scopeFilters).length === 0
        ? base
        : { ...base, filters: { ...base.filters, ...scopeFilters } };
    void fetchAllTablePages(
      (b, signal) => fetchRulePositionsPage(STRATEGY_SEG, ruleId, b, fetchScope, signal),
      seriesBody,
      ctrl.signal,
    )
      .then((rows) => {
        if (ctrl.signal.aborted) return;
        setTemporalBaseRows(rows.map(toTemporalRow));
        setChartPoints(evidenceChartPoints(rows));
      })
      .catch((e) => {
        if (e instanceof DOMException && e.name === 'AbortError') return;
        if (!ctrl.signal.aborted) {
          setTemporalBaseRows([]);
          setChartPoints([]);
        }
      });
    return () => ctrl.abort();
  }, [ruleId, fetchScope, scopeFilters, tableFilterKey, seriesTick]);

  // Selected position for the injected inspect modal. Resolved off the current page
  // rather than held as a row, so a reload/refilter can't strand a stale copy.
  const inspectRow = useMemo(
    () => (inspectId ? (items.find((r) => r.id === inspectId) ?? null) : null),
    [inspectId, items],
  );

  // Run + Mode are run-identity columns: they earn their width only where a row's
  // run isn't already fixed by the scope, i.e. all-time. Mode stays on for a
  // single-run scope in the *other* mode too — that is the one place the header
  // says one thing and every mode-carrying row would otherwise say nothing. A
  // mode-pinned all-time drops it again: every row says the same word.
  const showRunCol = scope.kind === 'all';
  const showModeCol =
    (scope.kind === 'all' && !scope.mode) ||
    (scope.kind === 'run' && scope.mode !== ownMode);
  const tableColumns = useMemo(
    () =>
      positionColumns.filter(
        (c) =>
          (c.key !== 'run_seq' || showRunCol) && (c.key !== 'mode' || showModeCol),
      ),
    [showRunCol, showModeCol],
  );
  const tableKeys = useMemo(
    () => new Set(tableColumns.map((c) => c.key)),
    [tableColumns],
  );

  const runAction = async (fn: () => Promise<unknown>, fail: string) => {
    setOpErr(null);
    try {
      await fn();
    } catch (e) {
      setOpErr(apiErrorMessage(e as never) ?? fail);
    }
  };

  const pauseRule = () => {
    if (!rule) return;
    setPausing(true);
    void runAction(async () => {
      try {
        await pause(rule.id).unwrap();
      } finally {
        setPausing(false);
      }
    }, 'Pause failed');
  };

  const stopRule = () => {
    if (!rule) return;
    if (
      rule.trade_mode === 'real' &&
      !window.confirm(
        `Stop "${rule.rule_name}" and close its open positions? REAL mode sends on-chain sells.`,
      )
    ) {
      return;
    }
    void runAction(() => stop(rule.id).unwrap(), 'Stop failed');
  };

  const selectScope = (next: EvidenceScope) => {
    // A scope the user picked is theirs to keep, empty or not.
    autoScoped.current = true;
    setScope(next);
    setQuery((q) => ({ ...q, page: 1 }));
    setFocus([]);
  };

  const runSummary = useMemo(
    () => (summary ? toRunSummaryFromPositions(summary) : null),
    [summary],
  );

  const capitalSection = useMemo(() => {
    if (!summary) return [];
    const avgEntry = summary.tokens > 0 ? summary.total_entry_sol / summary.tokens : null;
    return [
      capitalDetailSection([
        { label: `Total Entry (${price.unitLabel})`, value: price.displayAmount(summary.total_entry_sol) },
        { label: `Total Holding (${price.unitLabel})`, value: price.displayAmount(summary.total_holding_sol) },
        { label: 'Avg Entry', value: avgEntry != null ? price.displayAmount(avgEntry) : '—' },
        {
          label: `Total Gains (${price.unitLabel})`,
          value: price.displayAmount(summary.total_gains_sol),
          cls: 'text-green',
        },
        {
          label: `Total Losses (${price.unitLabel})`,
          value: price.displayAmount(summary.total_losses_sol),
          cls: 'text-red',
        },
      ]),
    ];
  }, [summary, price]);

  if (!ruleId) {
    return <InlineAlert variant="error">Missing rule id.</InlineAlert>;
  }

  const scopeLabel =
    scope.kind === 'current'
      ? currentRun
        ? `Current run #${currentRun.run_seq}`
        : 'Current run'
      : scope.kind === 'all'
        ? scope.mode
          ? `All-time · ${scope.mode}`
          : allTimeModes.length > 0
            ? 'All-time · paper + real'
            : 'All-time'
        : // The mode is part of the run's identity once the rule has both.
          `Run #${scope.runSeq}${scope.mode === ownMode ? '' : ` (${scope.mode})`}`;

  return (
    <div
      className={`flex flex-col gap-4 ${embedded ? 'rounded-lg bg-panel/40 pt-8' : ''}`}
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="flex min-w-0 flex-col gap-1">
          <div className="flex flex-wrap items-baseline gap-3">
            {embedded ? (
              <h2 className="text-base font-extrabold text-text">
                {rule?.rule_name ?? ruleId.slice(0, 8)}
              </h2>
            ) : (
              <h1 className="text-lg font-extrabold text-text">
                {rule?.rule_name ?? ruleId.slice(0, 8)}
              </h1>
            )}
            {rule && <ModeBadge mode={rule.trade_mode} size="md" />}
            {rule && (
              <Badge variant={rule.is_active ? 'success' : 'neutral'}>
                {rule.is_active ? 'Active' : 'Idle'}
              </Badge>
            )}
            <span className="text-sm text-text-mid">
              Evidence · {scopeLabel}
              {liveOpenCount != null && ` · ${liveOpenCount} open live`}
            </span>
          </div>
          {notice ?? (
            <p className="text-[11px] text-text-dim">
              Pause stops new entries; open positions still drain on Ops.
            </p>
          )}
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {rule?.is_enabled && (
            <IconButtonGroup>
              {rule.is_active ? (
                <>
                  <IconButton
                    variant="ghost"
                    size="md"
                    disabled={pausing}
                    onClick={pauseRule}
                    title={pausing ? 'Pausing…' : 'Pause — stop new entries'}
                    aria-label={pausing ? 'Pausing…' : 'Pause'}
                  >
                    {pausing ? <SpinnerIcon /> : <PauseIcon />}
                  </IconButton>
                  <IconButton
                    variant="danger"
                    size="md"
                    onClick={stopRule}
                    title="Stop — deactivate and force-close open positions"
                    aria-label="Stop"
                  >
                    <StopIcon />
                  </IconButton>
                </>
              ) : (
                <IconButton
                  variant="primary"
                  size="md"
                  onClick={() =>
                    void runAction(() => activate(rule.id).unwrap(), 'Activate failed')
                  }
                  title="Activate — arm this rule"
                  aria-label="Activate"
                >
                  <PlayIcon />
                </IconButton>
              )}
            </IconButtonGroup>
          )}
          {onClose && (
            <button
              type="button"
              onClick={onClose}
              className="rounded-md px-2 py-1 text-xs text-text-dim hover:bg-white/8 hover:text-text"
            >
              Close
            </button>
          )}
        </div>
      </div>

      {(error || opErr) && (
        <InlineAlert variant="error">{error || opErr}</InlineAlert>
      )}

      {/* Run navigator */}
      <div className="flex flex-col gap-2">
        <div className="text-[11px] font-semibold uppercase tracking-wide text-text-dim">
          Runs {runsLoading ? '…' : `(${runs.length})`}
        </div>
        {/* Cross-run PnL trend — spot decay/improvement run-over-run at a glance,
            and click a bar to scope the Evidence to that run. */}
        <RunTrendStrip
          runs={trendRuns}
          activeRunSeq={
            scope.kind === 'run'
              ? // The strip only plots `ownMode`, so a run picked in the other mode
                // highlights nothing rather than the same number in this one.
                scope.mode === ownMode
                ? scope.runSeq
                : null
              : scope.kind === 'current'
                ? currentRun?.run_seq ?? null
                : null
          }
          onPick={(runSeq) =>
            selectScope(
              currentRun && runSeq === currentRun.run_seq
                ? { kind: 'current' }
                : { kind: 'run', runSeq, mode: ownMode },
            )
          }
        />
        <div className="flex flex-wrap gap-1">
          <ScopeChip
            active={scope.kind === 'current'}
            onClick={() => selectScope({ kind: 'current' })}
            title={currentRun ? runTitle(currentRun) : undefined}
            label={
              currentRun
                ? `#${currentRun.run_seq} current`
                : 'Current run'
            }
            sub={
              currentRun?.has_metrics
                ? formatRunPnl(currentRun)
                : currentRun?.status ?? undefined
            }
            tone={currentRun?.total_pnl_sol}
            win={currentRun?.has_metrics ? currentRun.win_rate : undefined}
            n={currentRun?.has_metrics ? currentRun.n_closed : undefined}
          />
          {priorRuns.slice(0, 12).map((r) => (
            <ScopeChip
              key={r.id}
              active={
                scope.kind === 'run' && scope.mode === r.mode && scope.runSeq === r.run_seq
              }
              onClick={() => selectScope({ kind: 'run', runSeq: r.run_seq, mode: r.mode })}
              title={runTitle(r)}
              label={`#${r.run_seq}`}
              sub={formatRunPnl(r)}
              tone={r.total_pnl_sol}
              win={r.has_metrics ? r.win_rate : undefined}
              n={r.has_metrics ? r.n_closed : undefined}
            />
          ))}
          {/* Runs from the mode the rule is no longer in. Badged, because `#2` here
              and `#2` above are different runs — the badge is the disambiguator, not
              decoration. */}
          {otherRuns.slice(0, 12).map((r) => (
            <ScopeChip
              key={r.id}
              active={
                scope.kind === 'run' && scope.mode === r.mode && scope.runSeq === r.run_seq
              }
              onClick={() => selectScope({ kind: 'run', runSeq: r.run_seq, mode: r.mode })}
              title={runTitle(r)}
              label={`#${r.run_seq}`}
              badge={r.mode}
              sub={formatRunPnl(r)}
              tone={r.total_pnl_sol}
              win={r.has_metrics ? r.win_rate : undefined}
              n={r.has_metrics ? r.n_closed : undefined}
            />
          ))}
          <ScopeChip
            active={scope.kind === 'all' && !scope.mode}
            onClick={() => selectScope({ kind: 'all' })}
            label="All-time"
            sub={allTimeModes.length > 0 ? 'paper + real' : undefined}
          />
          {/* The same all-time population, one trade mode at a time. Paper and real
              are not one ledger — paper pays no slippage a real fill pays — so the
              combined chip above is for coverage, these are for reading results. */}
          {allTimeModes.map((m) => (
            <ScopeChip
              key={`all-${m}`}
              active={scope.kind === 'all' && scope.mode === m}
              onClick={() => selectScope({ kind: 'all', mode: m })}
              label="All-time"
              badge={m}
              title={`Every ${m} run of this rule`}
            />
          ))}
        </div>
      </div>

      {runSummary && (
        <PositionSummarySection
          title={`Summary · ${scopeLabel}`}
          subtitle={rule?.rule_name ?? ruleId.slice(0, 8)}
          summary={runSummary}
          summaryExtras={{ migrated: summary?.migrated }}
          extraDetailSections={capitalSection}
          chartPoints={chartPoints}
          focus={focus}
          onFocusChange={setFocus}
          temporal={{
            rows: temporalBaseRows,
            wallField: 'entry_time',
            enabled: temporalBaseRows.length > 0,
          }}
        />
      )}

      <TokenTable
        columns={tableColumns}
        existingKeys={tableKeys.size ? tableKeys : POSITION_KEYS}
        rows={items}
        rowKey={analyzePositionRowKey}
        loading={loading}
        serverSide
        serverTotal={total}
        onQueryChange={setQuery}
        useRowOverlay={posRowOverlay}
        charts
        chartsDefaultOn
        flowPatternKeys={flowPatternKeys}
        renderChartCardExtra={(row) => (
          <PositionChartCardExtra facts={positionChartFactsFromRule(row)} />
        )}
        {...(renderInspect
          ? { selectedKey: inspectId, onSelect: setInspectId }
          : {})}
        resetKey={`${ruleId}_${scopeKey(scope)}`}
        tableId="rule-evidence"
        defaultCols={EVIDENCE_DEFAULT_COLS}
        emptyMessage={
          scope.kind === 'all'
            ? scope.mode
              ? `No ${scope.mode} positions in any run.`
              : 'No positions in any run, in either mode.'
            : scope.kind === 'run'
              ? `No positions in ${scope.mode} run #${scope.runSeq}.`
              : 'No positions in the current run yet — pick a prior run or All-time.'
        }
      />

      {renderInspect && inspectRow
        ? renderInspect({
            position: inspectRow,
            rule: rule ?? null,
            onClose: () => setInspectId(null),
          })
        : null}
    </div>
  );
}

function ScopeChip({
  active,
  onClick,
  label,
  badge,
  sub,
  tone,
  win,
  n,
  title,
}: {
  active: boolean;
  onClick: () => void;
  label: string;
  /** Trade mode pill beside the label — set only when the chip's mode differs from
   *  the rule's, where the run number alone is ambiguous. */
  badge?: string;
  sub?: string;
  tone?: number | null;
  /** Win rate (0–1 fraction) for the stat line; omit to hide. */
  win?: number | null;
  /** Closed-position count for the stat line. */
  n?: number | null;
  title?: string;
}) {
  const hasStats = win != null && Number.isFinite(win);
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      className={`rounded-md px-2.5 py-1 text-left text-xs font-semibold ${
        active
          ? 'bg-primary/20 text-primary'
          : 'bg-white/5 text-text-dim hover:bg-white/8'
      }`}
    >
      <span className="flex items-center gap-1 leading-tight">
        {label}
        {(badge === 'paper' || badge === 'real') && <ModeBadge mode={badge} size="sm" />}
      </span>
      {sub && (
        <span
          className={`block text-[10px] font-medium tabular-nums ${
            tone != null && Number.isFinite(tone)
              ? signedToneClass(tone)
              : 'text-text-dim'
          }`}
        >
          {sub}
        </span>
      )}
      {hasStats && (
        <span className="block text-[10px] font-medium tabular-nums text-text-dim">
          {formatWinRate(win)} W{n != null ? ` · ${n}` : ''}
        </span>
      )}
    </button>
  );
}

/**
 * Cross-run PnL trend — one baseline-centered bar per finalized run, oldest on
 * the left. Height is the run's realized PnL scaled to the largest |PnL| in the
 * window, colored by sign; the active run is ringed. Clicking a bar scopes the
 * Evidence to that run, so this doubles as a picker *and* the at-a-glance read on
 * whether the rule is decaying or improving run-over-run.
 */
function RunTrendStrip({
  runs,
  activeRunSeq,
  onPick,
}: {
  runs: StrategyRuleRun[];
  activeRunSeq: number | null;
  onPick: (runSeq: number) => void;
}) {
  // Two bars is the minimum where a "trend" means anything.
  if (runs.length < 2) return null;
  const maxAbs = Math.max(
    ...runs.map((r) => Math.abs(r.total_pnl_sol ?? 0)),
    1e-9,
  );
  return (
    <div
      className="relative flex h-12 items-stretch gap-px rounded-md bg-white/[0.03] px-1"
      role="img"
      aria-label="Realized PnL by run (oldest to newest)"
    >
      {/* Zero baseline */}
      <div className="pointer-events-none absolute inset-x-1 top-1/2 h-px -translate-y-px bg-white/10" />
      {runs.map((r) => {
        const pnl = r.total_pnl_sol ?? 0;
        // Floor a non-zero bar so a tiny-but-real PnL is still visible.
        const frac = pnl === 0 ? 0 : Math.max(0.08, Math.abs(pnl) / maxAbs);
        const up = pnl > 0;
        const active = activeRunSeq != null && r.run_seq === activeRunSeq;
        return (
          <button
            key={r.id}
            type="button"
            onClick={() => onPick(r.run_seq)}
            title={runTitle(r)}
            className={`group relative flex min-w-0 flex-1 flex-col rounded-sm ${
              active ? 'bg-primary/10 ring-1 ring-primary/60' : 'hover:bg-white/5'
            }`}
          >
            <div className="flex h-1/2 items-end justify-center pb-px">
              {up && (
                <div
                  className="w-full max-w-[12px] rounded-t-sm bg-green opacity-80 group-hover:opacity-100"
                  style={{ height: `${frac * 100}%` }}
                />
              )}
            </div>
            <div className="flex h-1/2 items-start justify-center pt-px">
              {!up && pnl < 0 && (
                <div
                  className="w-full max-w-[12px] rounded-b-sm bg-red opacity-80 group-hover:opacity-100"
                  style={{ height: `${frac * 100}%` }}
                />
              )}
            </div>
          </button>
        );
      })}
    </div>
  );
}
