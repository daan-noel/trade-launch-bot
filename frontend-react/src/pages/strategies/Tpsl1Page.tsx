import { memo, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { DataTable } from 'components/table/DataTable';
import { Badge, type BadgeVariant } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { InlineAlert, Modal } from 'components/ui/Modal';
import {
  buildCreatePayload,
  buildUpdatePayload,
  emptyForm,
  formFromRule,
  RuleFormModal,
  type RuleFormData,
} from 'components/tpsl1/RuleFormModal';
import { ruleColumns } from 'components/tpsl1/ruleColumns';
import { SimSummaryCard } from 'components/tpsl1/SimSummaryCard';
import { TokenInspectModal, type InspectTarget } from 'components/tpsl1/TokenInspectModal';
import {
  matchedColumns,
  positionColumns,
  simColumns,
} from 'components/tpsl1/tableColumns';
import { useTimezone } from 'context/TimezoneContext';
import { formatIso } from 'utils/date';
import { usePriceDisplay } from 'hooks/usePriceDisplay';
import { usePolledRules } from 'hooks/usePolledRules';
import { useRulePositions } from 'hooks/useRulePositions';
import { useDispatch } from 'react-redux';
import {
  activateTpsl1Rule,
  createTpsl1Rule,
  deleteTpsl1Rule,
  fetchTpsl1PaperResult,
  fetchTpsl1RulePositions,
  fetchTpsl1Rules,
  pauseTpsl1Rule,
  stopTpsl1Rule,
  updateTpsl1Rule,
} from 'services/api';
import { connectPaperTestStream, connectSimulationProgress } from 'services/sse';
import { apiErrorMessage } from 'store/apiSlice';
import {
  fetchMatchedCached,
  fetchPaperResultCached,
  fetchSimulateCached,
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

function SectionDivider() {
  return <div role="separator" className="my-6 border-t border-white/6" />;
}

/** Determinate progress bar for a simulation run. The backend streams real
 *  `processed / total` candidate counts over the `simulation_progress` SSE
 *  event (keyed by rule), so the bar reports actual progress instead of a fake
 *  trickle. Before the first frame arrives the total is unknown, so it shows an
 *  indeterminate pulse; the bar unmounts when the result lands. */
function SimProgressBar({ ruleId }: { ruleId: string }) {
  const [progress, setProgress] = useState<{ processed: number; total: number } | null>(null);
  useEffect(() => {
    setProgress(null);
    const h = connectSimulationProgress(ruleId, (processed, total) =>
      setProgress({ processed, total }),
    );
    return () => h.close();
  }, [ruleId]);

  // Determinate once the first frame lands with a non-empty candidate set.
  const determinate = progress != null && progress.total > 0;
  const percent = determinate
    ? Math.min(100, Math.round((progress.processed / progress.total) * 100))
    : null;
  return (
    <div className="mt-4">
      <div className="mb-2 flex items-center justify-between gap-2">
        <span className="text-[11px] font-bold uppercase tracking-widest text-primary">
          Running Simulation
        </span>
        <span className="font-mono text-[11px] text-text-dim">
          {determinate ? `${progress.processed} / ${progress.total} · ${percent}%` : 'Starting…'}
        </span>
      </div>
      <div className="h-2 overflow-hidden rounded-full bg-white/6">
        <div
          className="h-full animate-pulse rounded-full bg-primary transition-[width] duration-300"
          style={{ width: determinate ? `${percent}%` : '15%' }}
        />
      </div>
    </div>
  );
}

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
}: {
  data: PaperResultResponse;
  price: ReturnType<typeof usePriceDisplay>;
  simCols: typeof simColumns;
  selectedMint: string | null;
  onSelectToken: (row: SimulatedTokenResult | null) => void;
  onClose: () => void;
}) {
  const { timezone } = useTimezone();
  const { run } = data;

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
            rows={data.tokens}
            rowKey={(r) => r.mint}
            selectedKey={selectedMint}
            onSelect={(key) =>
              onSelectToken(key ? data.tokens.find((t) => t.mint === key) ?? null : null)
            }
            defaultPageSize={20}
            pageSizeOptions={[20, 50, 100]}
            searchable
            colFilters
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
    fetchTpsl1PaperResult(rule.id)
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
  matchedActive,
  paperActive,
  simLoading,
  matchedLoading,
  paperLoading,
  onEdit,
  onDelete,
  onRequestDelete,
  onCancelDelete,
  onSimulate,
  onMatched,
  onPaperResult,
}: {
  rule: RuleRecord;
  confirmingDelete: boolean;
  deleteLoading: boolean;
  matchedActive: boolean;
  paperActive: boolean;
  simLoading: boolean;
  matchedLoading: boolean;
  paperLoading: boolean;
  onEdit: (rule: RuleRecord) => void;
  onDelete: (ruleId: string) => void;
  onRequestDelete: (ruleId: string) => void;
  onCancelDelete: () => void;
  onSimulate: (rule: RuleRecord) => void;
  onMatched: (rule: RuleRecord) => void;
  onPaperResult: (rule: RuleRecord) => void;
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
  return (
    <div className="flex items-center justify-center gap-1">
      <Button
        variant="ghost"
        size="xs"
        disabled={rule.is_active}
        onClick={() => onEdit(rule)}
        title={rule.is_active ? 'Cannot edit active rules' : 'Edit'}
      >
        Edit
      </Button>
      <Button
        variant="ghost"
        size="xs"
        disabled={rule.is_active}
        onClick={() => onRequestDelete(rule.id)}
        title={rule.is_active ? 'Cannot delete active rules' : 'Delete'}
        className="text-red"
      >
        Del
      </Button>
      <Button
        variant="ghost"
        size="xs"
        disabled={simLoading}
        onClick={() => onSimulate(rule)}
        className="text-primary"
        title="Simulate"
      >
        ▶
      </Button>
      <Button
        variant="ghost"
        size="xs"
        disabled={matchedLoading}
        onClick={() => onMatched(rule)}
        className={cn(matchedActive && 'border-[#9370db]/45 bg-[#9370db]/8 text-[#9370db]')}
        title="Matched tokens"
      >
        ⊞
      </Button>
      {rule.trade_mode === 'paper' && (
        <Button
          variant="ghost"
          size="xs"
          disabled={paperLoading}
          onClick={() => onPaperResult(rule)}
          className={cn('text-info', paperActive && 'border-info/45 bg-info/8')}
          title="Paper test result"
        >
          ▦
        </Button>
      )}
    </div>
  );
});

export function Tpsl1Page() {
  const price = usePriceDisplay();
  const dispatch = useDispatch<AppDispatch>();

  // Rule list: one initial load then a visibility-gated silent poll, deduped
  // into a shared hook (see usePolledRules). `loadRules` is the silent/forced
  // refresh used by the paper-test SSE handler below.
  const { rules, setRules, loading, error, refresh: loadRules } =
    usePolledRules(fetchTpsl1Rules, 'tpsl1');

  const [selectedRuleId, setSelectedRuleId] = useState<string | null>(null);
  // Positions for the selected rule: abortable fetch on select, then a silent
  // poll that pauses when the tab is hidden and stops once the rule is settled.
  const {
    positions,
    loading: positionsLoading,
    error: positionsError,
  } = useRulePositions(selectedRuleId, rules, fetchTpsl1RulePositions, 'tpsl1');

  const [modalOpen, setModalOpen] = useState(false);
  const [editRule, setEditRule] = useState<RuleRecord | null>(null);
  const [form, setForm] = useState<RuleFormData>(emptyForm());
  const [formError, setFormError] = useState<string | null>(null);
  const [formLoading, setFormLoading] = useState(false);

  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [deleteLoading, setDeleteLoading] = useState(false);

  const [simResult, setSimResult] = useState<{
    ruleName: string;
    tokens: SimulatedTokenResult[];
  } | null>(null);
  const [simError, setSimError] = useState<string | null>(null);
  const [simLoading, setSimLoading] = useState(false);
  // Rule whose backtest is in flight — keys the progress bar's SSE subscription.
  const [simRuleId, setSimRuleId] = useState<string | null>(null);

  const [matchedResult, setMatchedResult] = useState<{
    ruleId: string;
    tokens: import('types').MatchedTokenRecord[];
  } | null>(null);
  const [matchedError, setMatchedError] = useState<string | null>(null);
  const [matchedLoading, setMatchedLoading] = useState(false);

  const [paperResult, setPaperResult] = useState<{
    ruleId: string;
    data: PaperResultResponse;
  } | null>(null);
  const [paperError, setPaperError] = useState<string | null>(null);
  const [paperLoading, setPaperLoading] = useState(false);
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
        fetchPaperResultCached(dispatch, { strategy: 'tpsl1', ruleId: ev.rule_id }, true)
          .then((data) => setPaperResult({ ruleId: ev.rule_id, data }))
          .catch(() => {});
      }
    });
    return () => es.close();
  }, [loadRules, dispatch]);

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
        applyRuleUpdate(await pauseTpsl1Rule(rule.id));
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
        applyRuleUpdate(await activateTpsl1Rule(rule.id, paperRun));
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
        applyRuleUpdate(await stopTpsl1Rule(rule.id));
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
  const columns = useMemo(() => ruleColumns(ruleControls), [ruleControls]);
  // positionColumns/simColumns are referentially-stable module constants — their
  // price cells read the unit/rate from context, so a USD-rate tick no longer
  // rebuilds the column arrays or re-renders the whole table.
  const posCols = positionColumns;
  const simCols = simColumns;

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

  const handleSave = async (allowParams: boolean) => {
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
        const updated = await updateTpsl1Rule(
          editRule.id,
          buildUpdatePayload(form, allowParams),
        );
        setRules((prev) => prev.map((r) => (r.id === updated.id ? updated : r)));
        // The rule's entry criteria may have changed — drop its cached
        // matched/simulate results so the next open re-runs.
        invalidateStrategyResult(dispatch, { strategy: 'tpsl1', ruleId: updated.id });
      } else {
        const created = await createTpsl1Rule(buildCreatePayload(form));
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
        await deleteTpsl1Rule(ruleId);
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

  const handleSimulate = useCallback(
    async (rule: RuleRecord) => {
      setSimResult(null);
      setSimError(null);
      setSimRuleId(rule.id);
      setSimLoading(true);
      try {
        const tokens = await fetchSimulateCached(dispatch, { strategy: 'tpsl1', ruleId: rule.id });
        setSimResult({ ruleName: rule.rule_name, tokens });
      } catch (e) {
        setSimError(apiErrorMessage(e as Parameters<typeof apiErrorMessage>[0], 'Simulation failed'));
      } finally {
        setSimLoading(false);
      }
    },
    [dispatch],
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
        const tokens = await fetchMatchedCached(dispatch, { strategy: 'tpsl1', ruleId: rule.id });
        setMatchedResult({ ruleId: rule.id, tokens });
      } catch (e) {
        setMatchedError(
          apiErrorMessage(e as Parameters<typeof apiErrorMessage>[0], 'Failed to load matched tokens'),
        );
      } finally {
        setMatchedLoading(false);
      }
    },
    [matchedResult, dispatch],
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
        const data = await fetchPaperResultCached(dispatch, { strategy: 'tpsl1', ruleId: rule.id });
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

  // Cancel a pending delete confirmation; stable so it doesn't churn the cell.
  const cancelDelete = useCallback(() => setConfirmDeleteId(null), []);

  // Row-action cell renders the memoized <RuleActionsCell>: the per-row booleans
  // are narrowed here so a global toggle (simLoading, the open matched/paper
  // rule…) only re-renders the rows whose buttons actually change. Selection,
  // polling and the banner timer never touch these deps, so they don't churn it.
  const ruleActions = useCallback(
    (rule: RuleRecord) => (
      <RuleActionsCell
        rule={rule}
        confirmingDelete={confirmDeleteId === rule.id}
        deleteLoading={deleteLoading}
        matchedActive={matchedResult?.ruleId === rule.id}
        paperActive={paperResult?.ruleId === rule.id}
        simLoading={simLoading}
        matchedLoading={matchedLoading}
        paperLoading={paperLoading}
        onEdit={openEdit}
        onDelete={handleDelete}
        onRequestDelete={setConfirmDeleteId}
        onCancelDelete={cancelDelete}
        onSimulate={handleSimulate}
        onMatched={handleMatched}
        onPaperResult={handlePaperResult}
      />
    ),
    [
      confirmDeleteId,
      deleteLoading,
      matchedResult,
      simLoading,
      matchedLoading,
      paperLoading,
      paperResult,
      openEdit,
      handleDelete,
      cancelDelete,
      handleSimulate,
      handleMatched,
      handlePaperResult,
    ],
  );

  const matchedRuleName = useMemo(
    () => (matchedResult ? rules.find((r) => r.id === matchedResult.ruleId)?.rule_name : null),
    [matchedResult, rules],
  );

  const selectedRuleName = useMemo(
    () => (selectedRuleId ? rules.find((r) => r.id === selectedRuleId)?.rule_name ?? null : null),
    [selectedRuleId, rules],
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
        title="TPSL1 Strategies"
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

      {!loading && !error && (
        <DataTable
          columns={columns}
          rows={rules}
          rowKey={(r) => r.id}
          rowActions={ruleActions}
          selectedKey={selectedRuleId}
          onSelect={setSelectedRuleId}
          defaultPageSize={10}
          pageSizeOptions={[10, 25, 50]}
          searchable
          colFilters
          colToggle
          storageKey="tpsl1_rules_cols"
          emptyMessage="No rules found"
        />
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
            {!positionsLoading && !positionsError && (
              <DataTable
                columns={posCols}
                rows={positions}
                rowKey={(r) => r.id}
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
            count={matchedResult.tokens.length}
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
          {matchedResult.tokens.length === 0 ? (
            <p className="text-text-dim">
              No tokens in the database match this rule&apos;s entry criteria.
            </p>
          ) : (
            <DataTable
              columns={matchedColumns}
              rows={matchedResult.tokens}
              rowKey={(r) => r.mint}
              defaultPageSize={20}
              pageSizeOptions={[20, 50, 100]}
              searchable
              colFilters
              selectable={false}
            />
          )}
        </section>
      )}

      {(simLoading || simError || simResult) && <SectionDivider />}
      {simLoading && simRuleId && <SimProgressBar ruleId={simRuleId} />}
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
                rows={simResult.tokens}
                rowKey={(r) => r.mint}
                selectedKey={inspect?.table === 'sim' ? inspect.key : null}
                onSelect={onSelectSim}
                defaultPageSize={20}
                pageSizeOptions={[20, 50, 100]}
                searchable
                colFilters
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
          data={paperResult.data}
          price={price}
          simCols={simCols}
          selectedMint={inspect?.table === 'paper' ? inspect.key : null}
          onSelectToken={onSelectPaperToken}
          onClose={() => {
            setPaperResult(null);
            setPaperError(null);
          }}
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
