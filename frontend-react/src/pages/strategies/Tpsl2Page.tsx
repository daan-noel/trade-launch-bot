import { memo, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { DataTable } from 'components/table/DataTable';
import { Badge, type BadgeVariant } from 'components/ui/Badge';
import { SectionDivider } from 'components/ui/SectionDivider';
import { Button } from 'components/ui/Button';
import { IconButton } from 'components/ui/IconButton';
import { Input } from 'components/ui/Input';
import { InlineAlert, Modal } from 'components/ui/Modal';
import {
  buildCreatePayload,
  buildUpdatePayload,
  emptyForm,
  formFromRule,
  RuleFormModal,
  type RuleFormData,
  type LockGroupState,
} from 'components/tpsl2/RuleFormModal';
import { ruleColumns, RuleRowProvider } from 'components/tpsl2/ruleColumns';
import { SimSummaryCard } from 'components/tpsl2/SimSummaryCard';
import { TokenInspectModal, type InspectTarget } from 'components/tpsl2/TokenInspectModal';
import {
  matchedColumns,
  positionColumns,
  simColumns,
} from 'components/tpsl2/tableColumns';
import { useTimezone } from 'context/TimezoneContext';
import { datetimeLocalToUtcWallClock, formatIso } from 'utils/date';
import { usePriceDisplay } from 'hooks/usePriceDisplay';
import { usePolledRules } from 'hooks/usePolledRules';
import { useRulePositions } from 'hooks/useRulePositions';
import { useDispatch } from 'react-redux';
import {
  activateTpsl2Rule,
  clearTpsl2PaperResult,
  createTpsl2Rule,
  deleteTpsl2Rule,
  fetchTpsl2PaperResult,
  fetchTpsl2RulePositions,
  fetchTpsl2Rules,
  pauseTpsl2Rule,
  stopTpsl2Rule,
  updateTpsl2Rule,
} from 'services/api';
import { connectPaperTestStream, connectTpslPositionsChanged } from 'services/sse';
import { useBackgroundJobActions } from 'context/BackgroundJobsContext';
import { apiErrorMessage, useGetTokensByMintsQuery } from 'store/apiSlice';
import { mergeTokenData } from 'components/tokens/sharedTokenColumns';
import {
  fetchMatchedCached,
  fetchPaperResultCached,
  fetchSimulateCached,
  invalidatePaperResult,
  invalidateStrategyResult,
} from 'store/strategyResultCache';
import type { AppDispatch } from '../../store';
import type {
  PaperResultResponse,
  PaperRunResponse,
  PaperTestFinishedEvent,
  RulePositionRecord,
  RuleRecord,
  SimulatedTokenResult,
} from 'types';
import { cn } from 'lib/cn';
import { computeRuleColorClasses } from 'lib/ruleColorGroups';

// Module-level, referentially-stable rowKey fns: each only reads the row, so a
// single shared identity lets DataTable's page/select effects (and the row
// memo) skip churn that an inline `(r) => r.x` would trigger every render.
const keyByMint = (r: { mint: string }) => r.mint;
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

/** Renders the latest paper-test run for a rule: run-status header, the shared
 *  summary card, and the per-token table (reusing the simulation column set). */
function PaperResultSection({
  data,
  price,
  simCols,
  selectedMint,
  onSelectToken,
  onClose,
  onClear,
  clearing,
  canClear,
}: {
  data: PaperResultResponse;
  price: ReturnType<typeof usePriceDisplay>;
  simCols: typeof simColumns;
  selectedMint: string | null;
  onSelectToken: (row: SimulatedTokenResult | null) => void;
  onClose: () => void;
  onClear: () => void;
  clearing: boolean;
  canClear: boolean;
}) {
  const { timezone } = useTimezone();
  const { run } = data;
  // Inline confirm for the destructive Clear (mirrors the row-delete pattern).
  const [confirmClear, setConfirmClear] = useState(false);

  // Stable onSelect so the paper table's memoized rows survive an unrelated
  // re-render (e.g. a price tick); resolves the clicked key back to its row.
  const tokens = data.tokens;
  const handleSelect = useCallback(
    (key: string | null) => onSelectToken(key ? tokens.find((t) => t.mint === key) ?? null : null),
    [tokens, onSelectToken],
  );

  if (!run) {
    return (
      <section>
        <SectionHeading
          title="Paper Test"
          marker="bg-info"
          badge="info"
          subtitle={data.rule_name}
          action={
            <button
              type="button"
              onClick={onClose}
              className="text-text-dim transition hover:text-text"
            >
              ✕
            </button>
          }
        />
        <p className="text-text-dim">
          This rule hasn&apos;t been run in paper mode yet. Activate it to start a paper test.
        </p>
      </section>
    );
  }

  const statusVariant: BadgeVariant =
    run.status === 'Finished' ? 'primary' : run.status === 'Stopped' ? 'neutral' : 'info';

  return (
    <>
      <div className="mb-4 flex flex-wrap items-center gap-x-4 gap-y-2 text-[11px] text-text-dim">
        <Badge variant={statusVariant} size="sm" pill className="uppercase">
          {run.status === 'Running' ? '● Running' : run.status}
        </Badge>
        <span className="font-mono">Run #{run.run_seq}</span>
        <span>
          Cap:{' '}
          <span className="font-mono text-text">
            {run.max_total_tokens != null ? run.max_total_tokens : '∞'}
          </span>
        </span>
        <span>
          Started <span className="font-mono text-text">{formatIso(run.started_at, timezone)}</span>
        </span>
        {run.finished_at && (
          <span>
            Ended <span className="font-mono text-text">{formatIso(run.finished_at, timezone)}</span>
          </span>
        )}
        <span className="flex-1" />
        {confirmClear ? (
          <span className="flex items-center gap-1">
            <span className="font-semibold text-red">Clear results?</span>
            <Button variant="danger" size="xs" disabled={clearing} onClick={onClear}>
              {clearing ? 'Clearing…' : 'Yes'}
            </Button>
            <Button
              variant="ghost"
              size="xs"
              disabled={clearing}
              onClick={() => setConfirmClear(false)}
            >
              No
            </Button>
          </span>
        ) : (
          <Button
            variant="ghost"
            size="xs"
            disabled={!canClear}
            onClick={() => setConfirmClear(true)}
            title={
              canClear
                ? 'Clear results — delete this rule’s paper run history'
                : 'Stop the rule before clearing its results'
            }
            className="text-red"
          >
            🗑 Clear results
          </Button>
        )}
      </div>

      <SimSummaryCard
        title="Paper Test Results"
        ruleName={data.rule_name}
        tokens={data.tokens}
        price={price}
        onClose={onClose}
      />

      <section>
        <SectionHeading title="Paper Positions" count={data.tokens.length} subtitle={data.rule_name} />
        {data.tokens.length === 0 ? (
          <p className="text-text-dim">No positions recorded for this run yet.</p>
        ) : (
          <DataTable
            columns={simCols}
            rows={tokens}
            rowKey={keyByMint}
            selectedKey={selectedMint}
            onSelect={handleSelect}
            defaultPageSize={20}
            pageSizeOptions={[20, 50, 100]}
            searchable
            colFilters
            colToggle
            tableId="tpsl2_paper"
          />
        )}
      </section>
    </>
  );
}

/** A row selected for inspection, tagged with its source table so only that
 *  table highlights the selection (the three tables can share a mint/key). */
type InspectState = {
  table: 'positions' | 'sim' | 'paper';
  key: string;
  target: InspectTarget;
};

function inspectFromSim(r: SimulatedTokenResult): InspectTarget {
  return {
    mint: r.mint,
    symbol: r.symbol,
    entryTime: r.entry_time,
    entryPrice: r.entry_price,
    entryTx: r.entry_tx,
    exitTime: r.exit_time,
    exitPrice: r.exit_price,
    exitTx: r.exit_tx,
    exitLabel: r.exit_reason && r.exit_reason !== 'Open' ? r.exit_reason : null,
  };
}

function inspectFromPosition(r: RulePositionRecord): InspectTarget {
  return {
    mint: r.mint,
    entryTime: r.entry_time,
    entryPrice: r.entry_price,
    entryTx: r.entry_tx,
    exitTime: r.exit_time,
    exitPrice: r.exit_price,
    exitTx: r.exit_tx,
    exitLabel: r.status && r.status !== 'Open' ? r.status : null,
  };
}

/** Map a live position row into the sim-shaped result the shared `SimSummaryCard`
 *  aggregates, so the Positions section shows the same KPI / TP-SL / PnL summary
 *  as the paper-test and simulation views. Mirrors the backend
 *  `paper_position_to_sim_result`: still-open rows read as `"Open"`; PnL in SOL is
 *  the entry-allocated SOL scaled by the realized PnL %. */
function positionToSimResult(p: RulePositionRecord): SimulatedTokenResult {
  const pnlPercent = p.pnl_percent;
  const pnlSol = pnlPercent != null ? p.entry_amount * (pnlPercent / 100) : null;
  const holdingSecs =
    p.entry_time && p.exit_time
      ? Math.round((new Date(p.exit_time).getTime() - new Date(p.entry_time).getTime()) / 1000)
      : null;
  const athPrice =
    p.exit_price != null ? Math.max(p.exit_price, p.entry_price) : p.entry_price;
  return {
    mint: p.mint,
    symbol: p.symbol ?? '',
    target_price: p.target_price,
    target_amount: p.target_amount,
    target_time: p.target_time,
    target_tx: p.target_tx,
    entry_price: p.entry_price,
    ath_price: athPrice,
    entry_amount: p.entry_amount,
    entry_tx: p.entry_tx,
    entry_time: p.entry_time ?? p.created_at,
    exit_price: p.exit_price,
    exit_tx: p.exit_tx,
    exit_time: p.exit_time,
    holding_secs: holdingSecs,
    pnl_percent: pnlPercent,
    pnl_sol: pnlSol,
    exit_reason: p.exit_reason ?? 'Open',
    total_trades: 0,
  };
}

/** Confirm dialog for Stop & close. Spells out how many positions will be
 *  force-exited and hard-warns when the rule trades real (on-chain sells). */
function StopConfirmDialog({
  rule,
  busy,
  onCancel,
  onConfirm,
}: {
  rule: RuleRecord;
  busy: boolean;
  onCancel: () => void;
  onConfirm: (rule: RuleRecord) => void;
}) {
  const open = rule.open_positions;
  const real = rule.trade_mode === 'real';
  return (
    <Modal title={`Stop & close “${rule.rule_name}”?`} open onClose={onCancel}>
      <div className="space-y-4 text-sm text-text">
        {open > 0 ? (
          <p>
            <span className="font-mono font-bold">{open}</span> open position
            {open === 1 ? '' : 's'} will be <span className="font-semibold">exited now</span> at the
            current mark.
          </p>
        ) : (
          <p className="text-text-dim">This rule has no open positions; it will just be deactivated.</p>
        )}
        {real && open > 0 && (
          <InlineAlert variant="error">
            ⚠ REAL mode — this sends live on-chain sell transactions.
          </InlineAlert>
        )}
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={onCancel} disabled={busy}>
            Cancel
          </Button>
          <Button variant="danger" onClick={() => onConfirm(rule)} disabled={busy}>
            {busy ? 'Stopping…' : 'Stop & close'}
          </Button>
        </div>
      </div>
    </Modal>
  );
}

/** Re-activate prompt for a paper rule: loads the prior run so the user can
 *  choose a fresh run vs continuing it. Defaults to Continue for a resumable
 *  (non-finished) run, Fresh otherwise; warns that continuing a capped run takes
 *  no new entries. Real rules never reach this — they activate directly. */
function ReactivateDialog({
  rule,
  busy,
  onCancel,
  onActivate,
}: {
  rule: RuleRecord;
  busy: boolean;
  onCancel: () => void;
  onActivate: (rule: RuleRecord, paperRun: 'fresh' | 'continue') => void;
}) {
  const [loading, setLoading] = useState(true);
  const [run, setRun] = useState<PaperRunResponse | null>(null);
  const [tokenCount, setTokenCount] = useState(0);
  const [mode, setMode] = useState<'fresh' | 'continue'>('fresh');

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    fetchTpsl2PaperResult(rule.id)
      .then((data) => {
        if (cancelled) return;
        setRun(data.run);
        setTokenCount(data.tokens.length);
        setMode(data.run && data.run.status !== 'Finished' ? 'continue' : 'fresh');
      })
      .catch(() => {
        if (!cancelled) setRun(null);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [rule.id]);

  const finished = run?.status === 'Finished';

  return (
    <Modal title={`Activate “${rule.rule_name}”`} open onClose={onCancel}>
      {loading ? (
        <p className="text-text-dim">Loading previous run…</p>
      ) : !run ? (
        <div className="space-y-4 text-sm text-text">
          <p>No previous run — a fresh paper run will start.</p>
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={onCancel} disabled={busy}>
              Cancel
            </Button>
            <Button variant="primary" onClick={() => onActivate(rule, 'fresh')} disabled={busy}>
              {busy ? 'Activating…' : 'Activate'}
            </Button>
          </div>
        </div>
      ) : (
        <div className="space-y-4 text-sm text-text">
          <p className="text-text-dim">
            This rule has a previous run (
            <span className="font-mono text-text">#{run.run_seq}</span>, {tokenCount} token
            {tokenCount === 1 ? '' : 's'}). How do you want to start?
          </p>
          <div className="space-y-2">
            <label className="flex cursor-pointer items-start gap-2">
              <input
                type="radio"
                name="paperRun"
                checked={mode === 'fresh'}
                onChange={() => setMode('fresh')}
                className="mt-1"
              />
              <span>
                <span className="font-semibold">Fresh run</span>
                <span className="text-text-dim"> — reset counters, clear run #{run.run_seq}’s positions.</span>
              </span>
            </label>
            <label className="flex cursor-pointer items-start gap-2">
              <input
                type="radio"
                name="paperRun"
                checked={mode === 'continue'}
                onChange={() => setMode('continue')}
                className="mt-1"
              />
              <span>
                <span className="font-semibold">Continue #{run.run_seq}</span>
                <span className="text-text-dim"> — keep its tokens &amp; counters, resume taking entries.</span>
              </span>
            </label>
          </div>
          {finished && mode === 'continue' && (
            <InlineAlert variant="error">
              ⚠ Run #{run.run_seq} hit its token cap — continuing won’t take new entries until you
              raise Max Total Tokens.
            </InlineAlert>
          )}
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={onCancel} disabled={busy}>
              Cancel
            </Button>
            <Button variant="primary" onClick={() => onActivate(rule, mode)} disabled={busy}>
              {busy ? 'Activating…' : 'Activate'}
            </Button>
          </div>
        </div>
      )}
    </Modal>
  );
}

/** The per-row action buttons, split out and memoized so a change to one row's
 *  state (or a global loading flag) only re-renders the rows whose buttons
 *  actually change — not every row's button subtree. The handlers are stable
 *  useCallbacks from the page; the booleans are pre-narrowed per row so an
 *  unaffected row's props stay shallow-equal and `memo` skips it. */
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
  // Rule-management actions only (edit / delete). The read-only analysis tools
  // (simulate / matched / paper) live in their own `Analyze` column so the two
  // intents read as separate groups. Icon-only — tooltips name each action.
  return (
    <div className="flex items-center justify-center gap-1">
      <IconButton
        onClick={() => onEdit(rule)}
        title={
          rule.is_active || rule.open_positions > 0
            ? 'Live — only sizing (buy amount + concurrency) is editable'
            : 'Edit rule'
        }
        className="text-info"
      >
        ✎
      </IconButton>
      <IconButton
        onClick={() => onDuplicate(rule)}
        title="Duplicate — open a new rule pre-filled from this one"
      >
        ⧉
      </IconButton>
      <IconButton
        variant="danger"
        disabled={rule.is_active || rule.open_positions > 0}
        onClick={() => onRequestDelete(rule.id)}
        title={
          rule.is_active || rule.open_positions > 0
            ? 'Cannot delete a running rule or one with open positions'
            : 'Delete rule'
        }
      >
        ✕
      </IconButton>
    </div>
  );
});

export function Tpsl2Page() {
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
    usePolledRules(fetchTpsl2Rules, 'tpsl2');

  const [selectedRuleId, setSelectedRuleId] = useState<string | null>(null);
  // Positions for the selected rule: abortable fetch on select, then a silent
  // poll that pauses when the tab is hidden and stops once the rule is settled.
  const {
    positions,
    loading: positionsLoading,
    error: positionsError,
  } = useRulePositions(selectedRuleId, rules, fetchTpsl2RulePositions, 'tpsl2');

  const [modalOpen, setModalOpen] = useState(false);
  const [editRule, setEditRule] = useState<RuleRecord | null>(null);
  const [form, setForm] = useState<RuleFormData>(emptyForm());
  const [formError, setFormError] = useState<string | null>(null);
  const [formLoading, setFormLoading] = useState(false);

  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [deleteLoading, setDeleteLoading] = useState(false);

  const [simResult, setSimResult] = useState<{
    ruleId: string;
    ruleName: string;
    tokens: SimulatedTokenResult[];
  } | null>(null);
  const [simError, setSimError] = useState<string | null>(null);
  const [simLoading, setSimLoading] = useState(false);

  const [matchedResult, setMatchedResult] = useState<{
    ruleId: string;
    tokens: import('types').MatchedTokenRecord[];
    total: number;
    capped: boolean;
  } | null>(null);
  const [matchedError, setMatchedError] = useState<string | null>(null);
  const [matchedLoading, setMatchedLoading] = useState(false);
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
        fetchPaperResultCached(dispatch, { strategy: 'tpsl2', ruleId: ev.rule_id }, true)
          .then((data) => setPaperResult({ ruleId: ev.rule_id, data }))
          .catch(() => {});
      }
    });
    return () => es.close();
  }, [loadRules, dispatch]);

  // Keep the open paper-result view live while its run is in progress: every
  // position open/close emits `tpsl_positions_changed`, so (debounced, to
  // coalesce fill bursts) force-refetch the result for the open rule. Without
  // this the summary card froze at open-time and only refreshed when the run
  // finished. Notify over poll, one refetch per debounce window.
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    const handle = connectTpslPositionsChanged('tpsl2', (ruleId) => {
      if (ruleId !== openPaperRuleId.current || timer) return;
      timer = setTimeout(() => {
        timer = null;
        const id = openPaperRuleId.current;
        if (!id) return;
        fetchPaperResultCached(dispatch, { strategy: 'tpsl2', ruleId: id }, true)
          .then((data) => setPaperResult({ ruleId: id, data }))
          .catch(() => {});
      }, 800);
    });
    return () => {
      if (timer) clearTimeout(timer);
      handle.close();
    };
  }, [dispatch]);

  useEffect(() => {
    if (!paperNotice) return;
    const id = setTimeout(() => setPaperNotice(null), 9000);
    return () => clearTimeout(id);
  }, [paperNotice]);

  // Lifecycle controls (activate / pause / stop & close). `lifecycleBusyId`
  // disables a row's buttons mid-transition; the two dialogs gate the re-activate
  // (fresh/continue) and stop-and-close (force-exit) flows.
  const [lifecycleBusyId, setLifecycleBusyId] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [stopConfirm, setStopConfirm] = useState<RuleRecord | null>(null);
  const [reactivate, setReactivate] = useState<RuleRecord | null>(null);

  const applyRuleUpdate = useCallback((updated: RuleRecord) => {
    setRules((prev) => prev.map((r) => (r.id === updated.id ? updated : r)));
  }, []);

  const handlePause = useCallback(
    async (rule: RuleRecord) => {
      setLifecycleBusyId(rule.id);
      setActionError(null);
      try {
        applyRuleUpdate(await pauseTpsl2Rule(rule.id));
      } catch (e) {
        setActionError(e instanceof Error ? e.message : 'Failed to pause rule');
      } finally {
        setLifecycleBusyId(null);
      }
    },
    [applyRuleUpdate],
  );

  const handleActivate = useCallback(
    async (rule: RuleRecord, paperRun: 'fresh' | 'continue' = 'fresh') => {
      setLifecycleBusyId(rule.id);
      setActionError(null);
      try {
        applyRuleUpdate(await activateTpsl2Rule(rule.id, paperRun));
        setReactivate(null);
      } catch (e) {
        setActionError(e instanceof Error ? e.message : 'Failed to activate rule');
      } finally {
        setLifecycleBusyId(null);
      }
    },
    [applyRuleUpdate],
  );

  const handleActivateClick = useCallback(
    (rule: RuleRecord) => {
      // Paper rules prompt fresh-vs-continue; real rules activate immediately.
      if (rule.trade_mode === 'paper') setReactivate(rule);
      else handleActivate(rule, 'fresh');
    },
    [handleActivate],
  );

  const handleStopConfirm = useCallback(
    async (rule: RuleRecord) => {
      setLifecycleBusyId(rule.id);
      setActionError(null);
      try {
        applyRuleUpdate(await stopTpsl2Rule(rule.id));
        setStopConfirm(null);
      } catch (e) {
        setActionError(e instanceof Error ? e.message : 'Failed to stop rule');
      } finally {
        setLifecycleBusyId(null);
      }
    },
    [applyRuleUpdate],
  );

  // Run-control column lives in `ruleColumns`; the lifecycle handlers stay here
  // and are threaded in. Rebuilt only when the busy row changes (handlers are
  // stable useCallbacks).
  const ruleControls = useMemo(
    () => ({
      busyId: lifecycleBusyId,
      onPause: handlePause,
      onResume: (r: RuleRecord) => handleActivate(r, 'continue'),
      onStop: (r: RuleRecord) => setStopConfirm(r),
      onActivate: handleActivateClick,
    }),
    [lifecycleBusyId, handlePause, handleActivate, handleActivateClick],
  );
  // positionColumns/simColumns are now referentially-stable module constants —
  // their price cells read the unit/rate from context, so a USD-rate tick no
  // longer rebuilds the column arrays or re-renders the whole table.
  const posCols = positionColumns;
  const simCols = simColumns;

  // Collect all unique mints from every result table and fetch their full token
  // records in one batch request. The sorted join keeps the RTK cache key
  // stable regardless of the collection order.
  const allMints = useMemo(() => {
    const s = new Set<string>();
    matchedResult?.tokens.forEach((r) => s.add(r.mint));
    simResult?.tokens.forEach((r) => s.add(r.mint));
    positions.forEach((r) => s.add(r.mint));
    paperResult?.data.tokens.forEach((r) => s.add(r.mint));
    return [...s].sort();
  }, [matchedResult, simResult, positions, paperResult]);

  const { data: tokenBatch } = useGetTokensByMintsQuery(allMints, {
    skip: allMints.length === 0,
  });

  const tokenMap = useMemo(
    () => new Map((tokenBatch ?? []).map((t) => [t.mint_address, t])),
    [tokenBatch],
  );

  const openAdd = () => {
    setEditRule(null);
    setForm(emptyForm());
    setFormError(null);
    setModalOpen(true);
  };

  const openEdit = useCallback((rule: RuleRecord) => {
    setEditRule(rule);
    setForm(formFromRule(rule));
    setFormError(null);
    setModalOpen(true);
  }, []);

  // Duplicate: open the form in CREATE mode (no editRule → every field editable,
  // no locks) pre-filled from the source rule, with a distinct name so the copy
  // saves as a brand-new rule via the create path.
  const openDuplicate = useCallback((rule: RuleRecord) => {
    setEditRule(null);
    setForm({ ...formFromRule(rule), ruleName: `${rule.rule_name} (copy)` });
    setFormError(null);
    setModalOpen(true);
  }, []);

  const handleSave = async (unlocked: LockGroupState) => {
    setFormError(null);
    if (!form.ruleName.trim()) {
      setFormError('Rule name is required');
      return;
    }
    for (const [label, val] of [
      ['buy amount', form.buyAmount],
      ['take profit', form.takeProfit],
      ['stop loss', form.stopLoss],
    ] as const) {
      if (!val.trim() || Number.isNaN(parseFloat(val))) {
        setFormError(`Invalid ${label}`);
        return;
      }
    }

    setFormLoading(true);
    try {
      if (editRule) {
        const updated = await updateTpsl2Rule(
          editRule.id,
          buildUpdatePayload(form, unlocked),
        );
        setRules((prev) => prev.map((r) => (r.id === updated.id ? updated : r)));
        // The rule's entry criteria may have changed — drop its cached
        // matched/simulate results so the next open re-runs.
        invalidateStrategyResult(dispatch, { strategy: 'tpsl2', ruleId: updated.id });
      } else {
        const created = await createTpsl2Rule(buildCreatePayload(form));
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
        await deleteTpsl2Rule(ruleId);
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

  // Resolve the transient picker window to ISO bounds (UTC wall-clock, tz-aware),
  // omitting unset sides. Part of the matched/simulate RTK arg, so it both scopes
  // the backend scan and keys the cache per range.
  const analysisRange = useMemo(
    () => ({
      from: datetimeLocalToUtcWallClock(analysisFrom, timezone, 'lower') || undefined,
      to: datetimeLocalToUtcWallClock(analysisTo, timezone, 'upper') || undefined,
    }),
    [analysisFrom, analysisTo, timezone],
  );

  const handleSimulate = useCallback(
    async (rule: RuleRecord) => {
      setSimResult(null);
      setSimError(null);
      markStarting('simulation', rule.id, `Sim: ${rule.rule_name}`);
      setSimLoading(true);
      try {
        const tokens = await fetchSimulateCached(dispatch, {
          strategy: 'tpsl2',
          ruleId: rule.id,
          ...analysisRange,
        });
        if ('cancelled' in tokens) {
          // User cancelled — drop the cached cancel marker so a re-run refetches.
          invalidateStrategyResult(dispatch, { strategy: 'tpsl2', ruleId: rule.id });
          return;
        }
        setSimResult({ ruleId: rule.id, ruleName: rule.rule_name, tokens });
      } catch (e) {
        setSimError(apiErrorMessage(e as Parameters<typeof apiErrorMessage>[0], 'Simulation failed'));
      } finally {
        setSimLoading(false);
        // Clear the optimistic job ourselves: on a cache hit / cancel / missed
        // SSE frame the backend's `simulation_finished` never arrives, so without
        // this the progress bar would spin forever. Idempotent with the SSE.
        markFinished('simulation', rule.id);
      }
    },
    [dispatch, markStarting, markFinished, analysisRange],
  );

  const handleMatched = useCallback(
    async (rule: RuleRecord) => {
      if (matchedResult?.ruleId === rule.id) {
        setMatchedResult(null);
        return;
      }
      setMatchedResult(null);
      setMatchedError(null);
      setMatchedLoading(true);
      try {
        const { tokens, total, capped } = await fetchMatchedCached(dispatch, {
          strategy: 'tpsl2',
          ruleId: rule.id,
          ...analysisRange,
        });
        setMatchedResult({ ruleId: rule.id, tokens, total, capped });
      } catch (e) {
        setMatchedError(
          apiErrorMessage(e as Parameters<typeof apiErrorMessage>[0], 'Failed to load matched tokens'),
        );
      } finally {
        setMatchedLoading(false);
      }
    },
    [matchedResult, dispatch, analysisRange],
  );

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
        const data = await fetchPaperResultCached(dispatch, { strategy: 'tpsl2', ruleId: rule.id });
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
      await clearTpsl2PaperResult(ruleId);
      invalidatePaperResult(dispatch, { strategy: 'tpsl2', ruleId });
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
      simActiveId: simResult?.ruleId ?? null,
      matchedActiveId: matchedResult?.ruleId ?? null,
      paperActiveId: paperResult?.ruleId ?? null,
      onSimulate: handleSimulate,
      onMatched: handleMatched,
      onPaperResult: handlePaperResult,
    }),
    [
      simLoading,
      matchedLoading,
      paperLoading,
      simResult,
      matchedResult,
      paperResult,
      handleSimulate,
      handleMatched,
      handlePaperResult,
    ],
  );
  // Single context value for the Run/Analyze cells — `ruleColumns` is now a
  // static array, so only the cells (not the column defs) re-read these.
  const rowContext = useMemo(
    () => ({ controls: ruleControls, analysis: ruleAnalysis }),
    [ruleControls, ruleAnalysis],
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
    () => (matchedResult ? rules.find((r) => r.id === matchedResult.ruleId)?.rule_name : null),
    [matchedResult, rules],
  );

  const selectedRuleName = useMemo(
    () => (selectedRuleId ? rules.find((r) => r.id === selectedRuleId)?.rule_name ?? null : null),
    [selectedRuleId, rules],
  );

  // Sim-shaped view of the live positions, feeding the Positions summary card.
  // Memoized so the ~10 aggregate passes (and the card) only recompute when the
  // position list actually changes, not on every SOL/USD price tick.
  const positionSummaryTokens = useMemo(
    () => positions.map(positionToSimResult),
    [positions],
  );

  const ruleColorMap = useMemo(() => computeRuleColorClasses(rules), [rules]);
  const ruleCellGroupClassName = useCallback(
    (group: string | undefined, r: RuleRecord) => {
      const colors = ruleColorMap.get(r.id);
      if (!colors) return undefined;
      if (group === 'token_fingerprint') return colors.fp;
      if (group === 'entry') return colors.entry || undefined;
      return undefined;
    },
    [ruleColorMap],
  );

  // Stable row-select handlers so the result tables' memoized rows survive an
  // unrelated page render (these closures are passed straight to DataTable).
  const onSelectPosition = useCallback(
    (key: string | null) => {
      const row = key ? positions.find((p) => p.id === key) ?? null : null;
      setInspect(
        row ? { table: 'positions', key: row.id, target: inspectFromPosition(row) } : null,
      );
    },
    [positions],
  );

  const onSelectSim = useCallback(
    (key: string | null) => {
      const row = key ? simResult?.tokens.find((t) => t.mint === key) ?? null : null;
      setInspect(row ? { table: 'sim', key: row.mint, target: inspectFromSim(row) } : null);
    },
    [simResult],
  );

  const onSelectPaperToken = useCallback((row: SimulatedTokenResult | null) => {
    setInspect(row ? { table: 'paper', key: row.mint, target: inspectFromSim(row) } : null);
  }, []);

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
        title="TPSL2 Strategies"
        count={!loading && !error ? rules.length : undefined}
        action={
          <Button variant="primary" onClick={openAdd}>
            + Add Rule
          </Button>
        }
      />

      {actionError && <InlineAlert variant="error">{actionError}</InlineAlert>}
      {loading && <p className="text-text-dim">Loading rules…</p>}
      {error && <InlineAlert variant="error">{error}</InlineAlert>}

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
          <DataTable
            columns={ruleColumns}
            rows={rules}
            rowKey={keyById}
            rowActions={ruleActions}
            cellGroupClassName={ruleCellGroupClassName}
            selectedKey={selectedRuleId}
            onSelect={setSelectedRuleId}
            defaultPageSize={10}
            pageSizeOptions={[10, 25, 50]}
            searchable
            colFilters
            colToggle
            tableId="tpsl2_rules"
            emptyMessage="No rules found"
          />
        </RuleRowProvider>
      )}

      {selectedRuleId && (
        <>
          <SectionDivider />
          <section>
            <SectionHeading
              title="Positions"
              marker="bg-info"
              badge="info"
              count={positionsLoading || positionsError ? undefined : positions.length}
              subtitle={selectedRuleName ?? undefined}
            />
            {positionsLoading && <p className="text-text-dim">Loading positions…</p>}
            {positionsError && <InlineAlert variant="error">{positionsError}</InlineAlert>}
            {!positionsLoading && !positionsError && positions.length > 0 && (
              <SimSummaryCard
                title="Positions Summary"
                ruleName={selectedRuleName ?? ''}
                tokens={positionSummaryTokens}
                price={price}
              />
            )}
            {!positionsLoading && !positionsError && (
              <DataTable
                columns={posCols}
                rows={mergeTokenData(positions, tokenMap)}
                rowKey={keyById}
                selectedKey={inspect?.table === 'positions' ? inspect.key : null}
                onSelect={onSelectPosition}
                defaultPageSize={20}
                pageSizeOptions={[20, 50, 100]}
                colFilters
                colToggle
                emptyMessage="No positions for this rule."
              />
            )}
          </section>
        </>
      )}

      {(matchedLoading || matchedError || matchedResult) && <SectionDivider />}
      {matchedLoading && <p className="text-text-dim">Loading matched tokens…</p>}
      {matchedError && <InlineAlert variant="error">{matchedError}</InlineAlert>}
      {matchedResult && !matchedLoading && (
        <section>
          <SectionHeading
            title="Matched Tokens"
            marker="bg-[#9370db]"
            badge="neutral"
            badgeClass="border-[#9370db]/40 bg-[#9370db]/12 text-[#9370db]"
            count={matchedResult.total}
            subtitle={matchedRuleName ?? undefined}
            action={
              <button
                type="button"
                onClick={() => setMatchedResult(null)}
                className="text-text-dim transition hover:text-text"
              >
                ✕
              </button>
            }
          />
          {matchedResult.capped && (
            <p className="mb-2 text-sm text-amber-400">
              Showing first 5,000 of {matchedResult.total.toLocaleString()} total matches.
              Matched scans all-time historical tokens — use the date range above to narrow
              results to a recent window.
            </p>
          )}
          {matchedResult.tokens.length === 0 ? (
            <p className="text-text-dim">
              No tokens in the database match this rule&apos;s entry criteria.
            </p>
          ) : (
            <DataTable
              columns={matchedColumns}
              rows={mergeTokenData(matchedResult.tokens, tokenMap)}
              rowKey={keyByMint}
              defaultPageSize={20}
              pageSizeOptions={[20, 50, 100]}
              searchable
              colFilters
              colToggle
              tableId="tpsl2_matched"
              selectable={false}
            />
          )}
        </section>
      )}

      {(simError || simResult) && <SectionDivider />}
      {simError && <InlineAlert variant="error">{simError}</InlineAlert>}
      {simResult && !simLoading && (
        <>
          <SimSummaryCard
            ruleName={simResult.ruleName}
            tokens={simResult.tokens}
            price={price}
            onClose={() => {
              setSimResult(null);
              setSimError(null);
            }}
          />
          <section>
            <SectionHeading
              title="Simulated Tokens"
              count={simResult.tokens.length}
              subtitle={simResult.ruleName}
            />
            {simResult.tokens.length === 0 ? (
              <p className="text-text-dim">No tokens matched this rule&apos;s entry criteria.</p>
            ) : (
              <DataTable
                columns={simCols}
                rows={mergeTokenData(simResult.tokens, tokenMap)}
                rowKey={keyByMint}
                selectedKey={inspect?.table === 'sim' ? inspect.key : null}
                onSelect={onSelectSim}
                defaultPageSize={20}
                pageSizeOptions={[20, 50, 100]}
                searchable
                colFilters
                colToggle
                tableId="tpsl2_sim"
              />
            )}
          </section>
        </>
      )}

      {(paperLoading || paperError || paperResult) && <SectionDivider />}
      {paperLoading && <p className="text-text-dim">Loading paper-test result…</p>}
      {paperError && <InlineAlert variant="error">{paperError}</InlineAlert>}
      {paperResult && !paperLoading && (
        <PaperResultSection
          data={{
            ...paperResult.data,
            tokens: mergeTokenData(paperResult.data.tokens, tokenMap),
          }}
          price={price}
          simCols={simCols}
          selectedMint={inspect?.table === 'paper' ? inspect.key : null}
          onSelectToken={onSelectPaperToken}
          onClose={() => {
            setPaperResult(null);
            setPaperError(null);
          }}
          onClear={handleClearPaperResult}
          clearing={paperClearing}
          canClear={paperCanClear}
        />
      )}

      {inspect && (
        <TokenInspectModal target={inspect.target} onClose={() => setInspect(null)} />
      )}

      {stopConfirm && (
        <StopConfirmDialog
          rule={stopConfirm}
          busy={lifecycleBusyId === stopConfirm.id}
          onCancel={() => setStopConfirm(null)}
          onConfirm={handleStopConfirm}
        />
      )}

      {reactivate && (
        <ReactivateDialog
          rule={reactivate}
          busy={lifecycleBusyId === reactivate.id}
          onCancel={() => setReactivate(null)}
          onActivate={handleActivate}
        />
      )}

      <RuleFormModal
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
  );
}
