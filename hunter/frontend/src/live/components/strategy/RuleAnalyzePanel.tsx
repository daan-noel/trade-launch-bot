import { useCallback, useEffect, useMemo, useState } from 'react';
import { useSelector } from 'react-redux';
import { TokenTable } from 'components/tokens/TokenTable';
import { InlineAlert } from 'components/ui/Modal';
import { Badge } from 'components/ui/Badge';
import { IconButton } from 'components/ui/IconButton';
import { IconButtonGroup } from 'components/ui/IconButtonGroup';
import { PauseIcon, PlayIcon, SpinnerIcon, StopIcon } from 'components/ui/icons';
import { SimSummaryCard } from 'components/strategy/SimSummaryCard';
import { TemporalSummary, type TemporalSelection } from 'components/strategy/TemporalSummary';
import {
  positionColumns,
  POSITION_KEYS,
} from 'components/strategy/strategyColumns';
import { inspectFromPosition, markerRowOverlay } from 'components/strategy/inspectTarget';
import { usePriceDisplay } from 'hooks/usePriceDisplay';
import { useServerTable, DEFAULT_POSITIONS_QUERY } from 'hooks/useServerTable';
import { numericColKeys, toSummaryBody, toTableRequest } from 'services/tableRequest';
import {
  fetchRulePositionsPage,
  fetchRulePositionsSummary,
  type PositionFetchScope,
} from 'services/api';
import { connectStrategyPositionUpdate } from 'services/sse';
import { apiErrorMessage } from 'store/baseApi';
import {
  useActivateStrategyRuleMutation,
  useGetStrategyRuleRunsQuery,
  usePauseStrategyRuleMutation,
  useStopStrategyRuleMutation,
} from 'store/sharedEndpoints';
import type { TableQuery } from 'components/table/types';
import type { PositionsSummary, RulePositionRecord } from 'types';
import type { StrategyRule, StrategyRuleRun } from 'lib/strategy/types';
import type { TemporalRow } from 'lib/strategy/temporalSummary';
import { selectOpenByRule } from '@live/slices/liveStatusSlice';
import { signedToneClass } from 'lib/signedTone';

const STRATEGY_SEG = 'generic';
const POS_NUMERIC = numericColKeys(positionColumns);
const posRowOverlay = markerRowOverlay(inspectFromPosition);

const TABLE_RELOAD_STATUSES = new Set([
  'Holding',
  'End',
  'ExitFailed',
  'ExitUnconfirmed',
]);

/** Evidence scope: current run · one run #N · all-time. */
export type EvidenceScope =
  | { kind: 'current' }
  | { kind: 'run'; runSeq: number }
  | { kind: 'all' };

function holdingSecs(r: RulePositionRecord): number {
  if (!r.entry_time) return 0;
  const start = Date.parse(r.entry_time);
  if (!Number.isFinite(start)) return 0;
  const end = r.exit_time ? Date.parse(r.exit_time) : Date.now();
  if (!Number.isFinite(end)) return 0;
  return Math.max(0, (end - start) / 1000);
}

function toTemporalRow(r: RulePositionRecord): TemporalRow {
  const open = !r.exit_time && r.status !== 'End' && r.status !== 'ExitFailed';
  return {
    mint_address: r.mint_address,
    fired: r.entry_price != null,
    exit: open ? 'Open' : (r.exit_reason ?? r.status),
    pnl_sol: r.pnl_sol ?? 0,
    holding_secs: holdingSecs(r),
    entry_time: r.entry_time,
    created_at: r.created_at,
  };
}

function toFetchScope(s: EvidenceScope): PositionFetchScope {
  if (s.kind === 'run') return { kind: 'run', runSeq: s.runSeq };
  return { kind: s.kind };
}

function scopeKey(s: EvidenceScope): string {
  return s.kind === 'run' ? `run:${s.runSeq}` : s.kind;
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
}

/**
 * Rules Evidence — run navigator + Positions Summary + temporal + history.
 * Activate/Pause/Stop live in the header so keep/kill stays next to the proof.
 */
export function RuleAnalyzePanel({
  ruleId,
  rule,
  embedded,
  onClose,
  initialScopeKind = 'current',
}: RuleAnalyzePanelProps) {
  const price = usePriceDisplay();
  const [scope, setScope] = useState<EvidenceScope>(() =>
    initialScopeKind === 'all' ? { kind: 'all' } : { kind: 'current' },
  );
  const [query, setQuery] = useState<TableQuery>(DEFAULT_POSITIONS_QUERY);
  const [temporalSel, setTemporalSel] = useState<TemporalSelection>(null);
  const [opErr, setOpErr] = useState<string | null>(null);
  const [pausing, setPausing] = useState(false);

  const liveOpen = useSelector(selectOpenByRule(ruleId));
  const { data: runs = [], isFetching: runsLoading } = useGetStrategyRuleRunsQuery(ruleId, {
    skip: !ruleId,
  });
  const [activate] = useActivateStrategyRuleMutation();
  const [pause] = usePauseStrategyRuleMutation();
  const [stop] = useStopStrategyRuleMutation();

  const currentRun = runs[0] ?? null;
  const priorRuns = runs.slice(1);

  // Runs with a finalized PnL, oldest→newest, for the cross-run trend strip.
  const trendRuns = useMemo(
    () =>
      runs
        .filter((r) => r.has_metrics && r.total_pnl_sol != null)
        .slice()
        .reverse(),
    [runs],
  );

  const body = useMemo(() => {
    const base = toTableRequest(query, POS_NUMERIC);
    if (!temporalSel?.mints.length) return base;
    return {
      ...base,
      filters: {
        ...base.filters,
        mint_address: { op: 'in' as const, val: temporalSel.mints },
      },
    };
  }, [query, temporalSel]);

  const summaryBody = useMemo(() => {
    const base = toSummaryBody(query, POS_NUMERIC);
    if (!temporalSel?.mints.length) return base;
    return {
      ...base,
      filters: {
        ...base.filters,
        mint_address: { op: 'in' as const, val: temporalSel.mints },
      },
    };
  }, [query, temporalSel]);

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
    setTemporalSel(null);
    setScope(initialScopeKind === 'all' ? { kind: 'all' } : { kind: 'current' });
    setQuery(DEFAULT_POSITIONS_QUERY);
    setOpErr(null);
    setPausing(false);
  }, [ruleId, initialScopeKind]);

  useEffect(() => {
    if (!ruleId) return;
    const h = connectStrategyPositionUpdate((d) => {
      if (d.rule_id !== ruleId) return;
      if (!TABLE_RELOAD_STATUSES.has(d.status)) return;
      reload();
    });
    return () => h.close();
  }, [ruleId, reload]);

  const temporalRows = useMemo(() => items.map(toTemporalRow), [items]);

  const showRunCol = scope.kind === 'all';
  const tableColumns = useMemo(
    () =>
      showRunCol
        ? positionColumns
        : positionColumns.filter((c) => c.key !== 'run_seq'),
    [showRunCol],
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
    setScope(next);
    setQuery((q) => ({ ...q, page: 1 }));
    setTemporalSel(null);
  };

  if (!ruleId) {
    return <InlineAlert variant="error">Missing rule id.</InlineAlert>;
  }

  const scopeLabel =
    scope.kind === 'current'
      ? currentRun
        ? `Current run #${currentRun.run_seq}`
        : 'Current run'
      : scope.kind === 'all'
        ? 'All-time'
        : `Run #${scope.runSeq}`;

  return (
    <div
      className={`flex flex-col gap-4 ${embedded ? 'rounded-lg border border-white/8 bg-panel/40 p-4' : ''}`}
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
            {rule && (
              <Badge variant={rule.trade_mode === 'real' ? 'warning' : 'info'}>
                {rule.trade_mode}
              </Badge>
            )}
            {rule && (
              <Badge variant={rule.is_active ? 'success' : 'neutral'}>
                {rule.is_active ? 'Active' : 'Idle'}
              </Badge>
            )}
            <span className="text-sm text-text-mid">
              Evidence · {scopeLabel} · {liveOpen.length} open live
            </span>
          </div>
          <p className="text-[11px] text-text-dim">
            Pause stops new entries; open positions still drain on Ops.
          </p>
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
              ? scope.runSeq
              : scope.kind === 'current'
                ? currentRun?.run_seq ?? null
                : null
          }
          onPick={(runSeq) =>
            selectScope(
              currentRun && runSeq === currentRun.run_seq
                ? { kind: 'current' }
                : { kind: 'run', runSeq },
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
              active={scope.kind === 'run' && scope.runSeq === r.run_seq}
              onClick={() => selectScope({ kind: 'run', runSeq: r.run_seq })}
              title={runTitle(r)}
              label={`#${r.run_seq}`}
              sub={formatRunPnl(r)}
              tone={r.total_pnl_sol}
              win={r.has_metrics ? r.win_rate : undefined}
              n={r.has_metrics ? r.n_closed : undefined}
            />
          ))}
          <ScopeChip
            active={scope.kind === 'all'}
            onClick={() => selectScope({ kind: 'all' })}
            label="All-time"
          />
        </div>
      </div>

      {summary && (
        <SimSummaryCard
          ruleName={rule?.rule_name ?? ruleId.slice(0, 8)}
          price={price}
          title={`Summary · ${scopeLabel}`}
          summary={summary}
        />
      )}

      {temporalRows.length > 0 && (
        <TemporalSummary
          rows={temporalRows}
          selection={temporalSel}
          onSelect={setTemporalSel}
          wallField="entry_time"
        />
      )}

      <TokenTable
        columns={tableColumns}
        existingKeys={tableKeys.size ? tableKeys : POSITION_KEYS}
        rows={items}
        rowKey={(r) => r.id}
        loading={loading}
        serverSide
        serverTotal={total}
        onQueryChange={(q) => {
          setTemporalSel(null);
          setQuery(q);
        }}
        useRowOverlay={posRowOverlay}
        charts
        resetKey={`${ruleId}_${scopeKey(scope)}`}
        tableId={`rule-evidence-${ruleId}`}
        emptyMessage={
          scope.kind === 'all'
            ? 'No positions in any run.'
            : scope.kind === 'run'
              ? `No positions in run #${scope.runSeq}.`
              : 'No positions in the current run yet — pick a prior run or All-time.'
        }
      />
    </div>
  );
}

function ScopeChip({
  active,
  onClick,
  label,
  sub,
  tone,
  win,
  n,
  title,
}: {
  active: boolean;
  onClick: () => void;
  label: string;
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
      <span className="block leading-tight">{label}</span>
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
