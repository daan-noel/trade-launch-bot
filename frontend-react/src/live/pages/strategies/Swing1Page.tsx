import { memo, useCallback, useMemo, useState, type ReactNode } from 'react';
import { DataTable } from 'components/table/DataTable';
import { Accordion } from 'components/ui/Accordion';
import { Badge, type BadgeVariant } from 'components/ui/Badge';
import { SectionDivider } from 'components/ui/SectionDivider';
import { Button } from 'components/ui/Button';
import { InlineAlert, Modal } from 'components/ui/Modal';
import { RunPositionsPanel } from 'components/strategy/RunPositionsPanel';
import { TokenInspectModal, type InspectTarget } from 'components/tpsl2/TokenInspectModal';
import { inspectFromPosition } from 'components/strategy/inspectTarget';
import { positionColumns } from 'components/tpsl1/tableColumns';
import type { ChartSwingOverlay } from 'components/token-price-chart';
import { ruleColumns as ruleCols1, RuleRowProvider } from 'components/tpsl1/ruleColumns';
import { SpecRuleForm } from 'components/strategy/SpecRuleForm';
import {
  buildCreatePayload,
  buildUpdatePayload,
  emptyForm,
  formFromRule,
  getSpec,
  serializeRuleJson,
  RULE_NAME_KEY,
  type FormState,
  type LockState,
} from 'lib/params';
import {
  activateSwing1Rule,
  createSwing1Rule,
  deleteSwing1Rule,
  fetchSwing1RulePositions,
  fetchSwing1RulePositionsSummary,
  fetchSwing1Rules,
  pauseSwing1Rule,
  pauseAllSwing1Rules,
  stopSwing1Rule,
  stopAllSwing1Rules,
  updateSwing1Rule,
} from 'services/api';
import { apiErrorMessage } from 'store/apiSlice';
import { useSellTokenMutation } from '@live/store/liveEndpoints';
import { usePolledRules } from 'hooks/usePolledRules';
import { usePriceDisplay } from 'hooks/usePriceDisplay';
import { cn } from 'lib/cn';
import type { BulkRuleResult, RulePositionRecord, RuleRecord } from 'types';
import type { ColumnDef } from 'components/table/types';

/** Trade-mode section a bulk lifecycle action targets — one section = one `trade_mode`. */
type RuleMode = 'real' | 'paper';

/** swing1's SSE / positions filter key — the backend emits `swing_1`. */
const STRATEGY = 'swing_1';
const SWING1_SPEC = getSpec('swing_1');

// Stable row-key functions (no inline lambdas — keeps DataTable memoization clean).
const keyById = (r: { id: string }) => r.id;

function SectionHeading({
  title,
  count,
  marker = 'bg-primary',
  badge = 'primary',
  size = 'h3',
  subtitle,
  action,
}: {
  title: string;
  count?: number;
  marker?: string;
  badge?: BadgeVariant;
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
        <Badge variant={badge} size="sm" className="font-mono font-normal">
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

function RuleTableHeader({
  title,
  count,
  marker,
  badge,
  subtitle,
  action,
}: {
  title: string;
  count: number;
  marker: string;
  badge: BadgeVariant;
  subtitle: string;
  action?: ReactNode;
}) {
  return (
    <div className="flex min-w-0 flex-1 items-center gap-2.5">
      <span className={cn('h-4 w-1 rounded-full', marker)} />
      <h3 className="text-sm font-bold text-text">{title}</h3>
      <Badge variant={badge} size="sm" className="font-mono font-normal">
        {count}
      </Badge>
      <span className="truncate font-mono text-[11px] text-text-dim">{subtitle}</span>
      {action && (
        <>
          <span className="flex-1" />
          {action}
        </>
      )}
    </div>
  );
}

/** Pause All / Stop All for one Real/Paper rule section. Hidden entirely when
 *  there's nothing to act on (no active and no draining rules). */
function BulkRuleActions({
  activeCount,
  stoppableCount,
  busy,
  onPauseAll,
  onStopAllClick,
}: {
  activeCount: number;
  stoppableCount: number;
  busy: boolean;
  onPauseAll: () => void;
  onStopAllClick: () => void;
}) {
  if (activeCount === 0 && stoppableCount === 0) return null;
  return (
    <div className="flex items-center gap-1.5">
      <Button
        variant="ghost" size="xs"
        disabled={busy || activeCount === 0}
        onClick={onPauseAll}
        title="Pause every active rule here — stop new entries, let open positions drain"
      >
        {busy ? '…' : `⏸ Pause All (${activeCount})`}
      </Button>
      <Button
        variant="danger" size="xs"
        disabled={busy || stoppableCount === 0}
        onClick={onStopAllClick}
        title="Stop every rule here and force-close all open positions now"
      >
        {busy ? '…' : `■ Stop All (${stoppableCount})`}
      </Button>
    </div>
  );
}

/** Confirms a bulk Stop All — summarizes how many rules/positions are affected
 *  across the whole section (parallels `StopConfirmDialog` for a single rule). */
function StopAllConfirmDialog({
  modeLabel,
  real,
  rules,
  busy,
  onCancel,
  onConfirm,
}: {
  modeLabel: string;
  real: boolean;
  rules: RuleRecord[];
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const totalOpen = rules.reduce((sum, r) => sum + r.open_positions, 0);
  return (
    <Modal title={`Stop all ${modeLabel} rules`} open onClose={onCancel}>
      <div className="space-y-4 text-sm text-text">
        <p>
          <span className="font-mono font-bold">{rules.length}</span> rule{rules.length === 1 ? '' : 's'} will be
          stopped;{' '}
          <span className="font-mono font-bold">{totalOpen}</span> open position{totalOpen === 1 ? '' : 's'} will be{' '}
          <span className="font-semibold">exited now</span> at the current mark.
        </p>
        {real && totalOpen > 0 && (
          <InlineAlert variant="error">
            ⚠ REAL mode — this sends live on-chain sell transactions.
          </InlineAlert>
        )}
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={onCancel} disabled={busy}>
            Cancel
          </Button>
          <Button variant="danger" onClick={onConfirm} disabled={busy}>
            {busy ? 'Stopping…' : 'Stop & close all'}
          </Button>
        </div>
      </div>
    </Modal>
  );
}

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
    <Modal title={`Stop & close "${rule.rule_name}"`} open onClose={onCancel}>
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

/** Re-activate prompt for paper rules: fresh run vs continue previous. */
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
  const [mode, setMode] = useState<'fresh' | 'continue'>('fresh');
  return (
    <Modal title={`Activate "${rule.rule_name}"`} open onClose={onCancel}>
      <div className="space-y-4 text-sm text-text">
        <div className="space-y-2">
          <label className="flex cursor-pointer items-start gap-2">
            <input type="radio" name="paperRun" checked={mode === 'fresh'} onChange={() => setMode('fresh')} className="mt-1" />
            <span>
              <span className="font-semibold">Fresh run</span>
              <span className="text-text-dim"> — reset counters and start a new paper run.</span>
            </span>
          </label>
          <label className="flex cursor-pointer items-start gap-2">
            <input type="radio" name="paperRun" checked={mode === 'continue'} onChange={() => setMode('continue')} className="mt-1" />
            <span>
              <span className="font-semibold">Continue</span>
              <span className="text-text-dim"> — resume taking entries from where it left off.</span>
            </span>
          </label>
        </div>
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={onCancel} disabled={busy}>Cancel</Button>
          <Button variant="primary" onClick={() => onActivate(rule, mode)} disabled={busy}>
            {busy ? 'Activating…' : 'Activate'}
          </Button>
        </div>
      </div>
    </Modal>
  );
}

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
        <Button variant="danger" size="xs" disabled={deleteLoading} onClick={() => onDelete(rule.id)}>Yes</Button>
        <Button variant="ghost" size="xs" onClick={onCancelDelete}>No</Button>
      </div>
    );
  }
  return (
    <div className="flex items-center justify-center gap-1">
      <Button
        variant="ghost" size="xs" onClick={() => onEdit(rule)}
        title={rule.is_active || rule.open_positions > 0 ? 'Live — only sizing is editable' : 'Edit rule'}
        className="text-info"
      >
        ✎
      </Button>
      <Button
        variant="ghost" size="xs" onClick={() => onDuplicate(rule)}
        title="Duplicate — open a new rule pre-filled from this one"
        className="text-text-dim hover:text-text"
      >
        ⧉
      </Button>
      <Button
        variant="ghost" size="xs" onClick={handleCopy}
        title="Copy params to clipboard"
        className={copied ? 'text-green' : 'text-text-dim hover:text-text'}
      >
        {copied ? '✔' : '⎘'}
      </Button>
      <Button
        variant="ghost" size="xs"
        disabled={rule.is_active || rule.open_positions > 0}
        onClick={() => onRequestDelete(rule.id)}
        title={rule.is_active || rule.open_positions > 0 ? 'Cannot delete a running rule' : 'Delete rule'}
        className="text-red"
      >
        ✕
      </Button>
    </div>
  );
});

/** Positions section — rendered under the rule table that owns the selected rule. */

// Stable no-op analysis bag — the 'analyze' column is filtered out of the live
// rule table, so these handlers are never called, but the RuleRowProvider type
// still requires the field.
const NO_ANALYSIS = {
  simLoading: false, matchedLoading: false, paperLoading: false,
  simActiveId: null, matchedActiveId: null, paperActiveId: null,
  onSimulate: () => {}, onMatched: () => {}, onPaperResult: () => {},
};

export function Swing1Page() {
  const price = usePriceDisplay();

  // Filter out the 'analyze' column — no sim/matched/paper on the live bin.
  const ruleColumns = useMemo(
    () => ruleCols1.filter((c: ColumnDef<RuleRecord>) => c.key !== 'analyze'),
    [],
  );

  const { rules, setRules, loading, error } = usePolledRules(fetchSwing1Rules, STRATEGY);
  const [selectedRuleId, setSelectedRuleId] = useState<string | null>(null);

  const [modalOpen, setModalOpen] = useState(false);
  const [editRule, setEditRule] = useState<RuleRecord | null>(null);
  const [form, setForm] = useState<FormState>(() => emptyForm(SWING1_SPEC));
  const [formError, setFormError] = useState<string | null>(null);
  const [formLoading, setFormLoading] = useState(false);

  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [deleteLoading, setDeleteLoading] = useState(false);

  const [lifecycleBusyId, setLifecycleBusyId] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [stopConfirm, setStopConfirm] = useState<RuleRecord | null>(null);
  const [reactivate, setReactivate] = useState<RuleRecord | null>(null);

  const [bulkBusy, setBulkBusy] = useState<RuleMode | null>(null);
  const [stopAllConfirm, setStopAllConfirm] = useState<{ mode: RuleMode; rules: RuleRecord[] } | null>(null);

  const [inspect, setInspect] = useState<
    { key: string; target: InspectTarget; swingOverlay: ChartSwingOverlay | null } | null
  >(null);

  const [sellToken] = useSellTokenMutation();
  const [sellingPositionMint, setSellingPositionMint] = useState<string | null>(null);

  // Position deltas keep both lists live via the hooks themselves:
  // `usePolledRules` patches each rule's counts/lifecycle in place and
  // `useRulePositions` patches the positions list in place — both off the same
  // `tpsl_positions_changed` push, no refetch. (A previous effect here called
  // the NON-silent `loadRules(false)` on every delta, flipping `loading` true
  // and unmounting the whole rule-table region — the "page reload" flash.)

  const applyRuleUpdate = useCallback((updated: RuleRecord) => {
    setRules((prev) => prev.map((r) => (r.id === updated.id ? updated : r)));
  }, [setRules]);

  const handlePause = useCallback(async (rule: RuleRecord) => {
    setLifecycleBusyId(rule.id);
    setActionError(null);
    try { applyRuleUpdate(await pauseSwing1Rule(rule.id)); }
    catch (e) { setActionError(e instanceof Error ? e.message : 'Failed to pause rule'); }
    finally { setLifecycleBusyId(null); }
  }, [applyRuleUpdate]);

  const handleActivate = useCallback(async (rule: RuleRecord, paperRun: 'fresh' | 'continue' = 'fresh') => {
    setLifecycleBusyId(rule.id);
    setActionError(null);
    try { applyRuleUpdate(await activateSwing1Rule(rule.id, paperRun)); setReactivate(null); }
    catch (e) { setActionError(e instanceof Error ? e.message : 'Failed to activate rule'); }
    finally { setLifecycleBusyId(null); }
  }, [applyRuleUpdate]);

  const handleActivateClick = useCallback((rule: RuleRecord) => {
    // Only prompt fresh-vs-continue when there's an actual choice to make: a
    // paper rule whose current/last run ever recorded a position. Otherwise
    // (real rule, or a paper rule with nothing to continue) activate directly.
    if (rule.trade_mode === 'paper' && rule.total_positions > 0) setReactivate(rule);
    else void handleActivate(rule, 'fresh');
  }, [handleActivate]);

  const handleStopConfirm = useCallback(async (rule: RuleRecord) => {
    setLifecycleBusyId(rule.id);
    setActionError(null);
    try { applyRuleUpdate(await stopSwing1Rule(rule.id)); setStopConfirm(null); }
    catch (e) { setActionError(e instanceof Error ? e.message : 'Failed to stop rule'); }
    finally { setLifecycleBusyId(null); }
  }, [applyRuleUpdate]);

  const applyBulkResult = useCallback((result: BulkRuleResult) => {
    setRules((prev) => {
      const byId = new Map(result.updated.map((r) => [r.id, r]));
      return prev.map((r) => byId.get(r.id) ?? r);
    });
    if (result.failed.length > 0) {
      setActionError(`Failed to update ${result.failed.length} rule${result.failed.length === 1 ? '' : 's'}`);
    }
  }, [setRules]);

  const handlePauseAll = useCallback(async (mode: RuleMode) => {
    setBulkBusy(mode);
    setActionError(null);
    try { applyBulkResult(await pauseAllSwing1Rules(mode)); }
    catch (e) { setActionError(e instanceof Error ? e.message : 'Failed to pause all rules'); }
    finally { setBulkBusy(null); }
  }, [applyBulkResult]);

  const handleStopAllConfirm = useCallback(async () => {
    if (!stopAllConfirm) return;
    const { mode } = stopAllConfirm;
    setBulkBusy(mode);
    setActionError(null);
    try { applyBulkResult(await stopAllSwing1Rules(mode)); setStopAllConfirm(null); }
    catch (e) { setActionError(e instanceof Error ? e.message : 'Failed to stop all rules'); }
    finally { setBulkBusy(null); }
  }, [applyBulkResult, stopAllConfirm]);

  const ruleControls = useMemo(() => ({
    busyId: lifecycleBusyId,
    onPause: handlePause,
    onResume: (r: RuleRecord) => void handleActivate(r, 'continue'),
    onStop: (r: RuleRecord) => (r.open_positions === 0 ? void handleStopConfirm(r) : setStopConfirm(r)),
    onActivate: handleActivateClick,
  }), [lifecycleBusyId, handlePause, handleActivate, handleStopConfirm, handleActivateClick]);

  const rowContext = useMemo(
    () => ({ controls: ruleControls, analysis: NO_ANALYSIS }),
    [ruleControls],
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

  const openDuplicate = useCallback((rule: RuleRecord) => {
    setEditRule(null);
    setForm({ ...formFromRule(SWING1_SPEC, rule), [RULE_NAME_KEY]: `${rule.rule_name} (copy)` });
    setFormError(null);
    setModalOpen(true);
  }, []);

  const handleSave = async (unlocked: LockState) => {
    setFormError(null);
    if (!(form[RULE_NAME_KEY] ?? '').trim()) { setFormError('Rule name is required'); return; }
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
        const updated = await updateSwing1Rule(editRule.id, buildUpdatePayload(SWING1_SPEC, form, unlocked));
        setRules((prev) => prev.map((r) => (r.id === updated.id ? updated : r)));
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

  const handleDelete = useCallback(async (ruleId: string) => {
    setDeleteLoading(true);
    try {
      await deleteSwing1Rule(ruleId);
      setRules((prev) => prev.filter((r) => r.id !== ruleId));
      if (selectedRuleId === ruleId) setSelectedRuleId(null);
    } catch { /* ignore */ }
    finally { setConfirmDeleteId(null); setDeleteLoading(false); }
  }, [selectedRuleId, setRules]);

  const cancelDelete = useCallback(() => setConfirmDeleteId(null), []);

  const ruleActions = useCallback((rule: RuleRecord) => (
    <RuleActionsCell
      rule={rule}
      confirmingDelete={confirmDeleteId === rule.id}
      deleteLoading={deleteLoading}
      onEdit={openEdit} onDuplicate={openDuplicate}
      onDelete={handleDelete}
      onRequestDelete={setConfirmDeleteId}
      onCancelDelete={cancelDelete}
    />
  ), [confirmDeleteId, deleteLoading, openEdit, openDuplicate, handleDelete, cancelDelete]);

  const realRules = useMemo(() => rules.filter((r) => r.trade_mode === 'real'), [rules]);
  const paperRules = useMemo(() => rules.filter((r) => r.trade_mode !== 'real'), [rules]);

  // Rules a bulk Stop All would target: active or still draining (mirrors the
  // per-row Stop button's visibility — Idle/Finished rules have nothing to stop).
  const realStoppable = useMemo(
    () => realRules.filter((r) => r.is_active || r.open_positions > 0), [realRules],
  );
  const paperStoppable = useMemo(
    () => paperRules.filter((r) => r.is_active || r.open_positions > 0), [paperRules],
  );

  const selectedRuleMode = useMemo(
    () => rules.find((r) => r.id === selectedRuleId)?.trade_mode ?? null,
    [selectedRuleId, rules],
  );
  const isRealRuleSelected = selectedRuleMode === 'real';
  const isPaperRuleSelected = selectedRuleId != null && selectedRuleMode != null && !isRealRuleSelected;

  const selectedRuleName = useMemo(
    () => selectedRuleId ? rules.find((r) => r.id === selectedRuleId)?.rule_name ?? null : null,
    [selectedRuleId, rules],
  );


  const handleSellPosition = useCallback(async (mint: string) => {
    setSellingPositionMint(mint);
    setActionError(null);
    try { await sellToken({ mint }).unwrap(); }
    catch (e) {
      setActionError(`Sell failed: ${apiErrorMessage(e as Parameters<typeof apiErrorMessage>[0]) ?? 'unknown error'}`);
    }
    finally { setSellingPositionMint(null); }
  }, [sellToken]);

  const handleInspect = useCallback((row: RulePositionRecord | null) => {
    if (!row) { setInspect(null); return; }
    // Legs arrived with the position row itself (harvested from the live exit
    // memo at close) — no detect-endpoint fetch needed on `live`.
    const swingOverlay: ChartSwingOverlay | null =
      row.swing_legs && row.swing_legs.length > 0
        ? { legs: row.swing_legs, segmentMode: 'perLeg', perLegFullSpanEnd: true }
        : null;
    setInspect({ key: row.id, target: inspectFromPosition(row), swingOverlay });
  }, []);

  // Current run + old runs, each with its own summary + table.
  const positionsSection = (
    <RunPositionsPanel
      strategy={STRATEGY}
      selectedRuleId={selectedRuleId}
      selectedRuleName={selectedRuleName}
      rules={rules}
      columns={positionColumns}
      fetchPositions={fetchSwing1RulePositions}
      fetchSummary={fetchSwing1RulePositionsSummary}
      price={price}
      selectedKey={inspect?.key ?? null}
      onInspect={handleInspect}
      isReal={isRealRuleSelected}
      sellingPositionMint={sellingPositionMint}
      onSellPosition={handleSellPosition}
    />
  );

  return (
    <div>
      <SectionHeading
        size="h2"
        title="Swing 1 Strategies"
        count={!loading && !error ? rules.length : undefined}
        action={<Button variant="primary" onClick={openAdd}>+ Add Rule</Button>}
      />

      {actionError && <InlineAlert variant="error">{actionError}</InlineAlert>}
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

      {!loading && !error && (
        <RuleRowProvider value={rowContext}>
          {realRules.length > 0 && (
            <Accordion
              className="mb-4"
              bordered={false}
              header={
                <RuleTableHeader
                  title="Real Trading" marker="bg-red" badge="danger"
                  count={realRules.length}
                  subtitle="Live on-chain — buys & sells execute for real"
                  action={
                    <BulkRuleActions
                      activeCount={realRules.filter((r) => r.is_active).length}
                      stoppableCount={realStoppable.length}
                      busy={bulkBusy === 'real'}
                      onPauseAll={() => void handlePauseAll('real')}
                      onStopAllClick={() => setStopAllConfirm({ mode: 'real', rules: realStoppable })}
                    />
                  }
                />
              }
            >
              <DataTable
                columns={ruleColumns} rows={realRules} rowKey={keyById}
                rowActions={ruleActions} selectedKey={selectedRuleId}
                onSelect={setSelectedRuleId}
                defaultPageSize={10} pageSizeOptions={[10, 25, 50]}
                searchable colFilters colToggle
                tableId={`${STRATEGY}_rules_real`} emptyMessage="No real rules"
              />
            </Accordion>
          )}

          {isRealRuleSelected && positionsSection}

          {realRules.length > 0 && <SectionDivider gap="xl" />}

          <Accordion
            bordered={false}
            header={
              <RuleTableHeader
                title="Paper Trading" marker="bg-info" badge="info"
                count={paperRules.length}
                subtitle="Simulated — no on-chain execution"
                action={
                  <BulkRuleActions
                    activeCount={paperRules.filter((r) => r.is_active).length}
                    stoppableCount={paperStoppable.length}
                    busy={bulkBusy === 'paper'}
                    onPauseAll={() => void handlePauseAll('paper')}
                    onStopAllClick={() => setStopAllConfirm({ mode: 'paper', rules: paperStoppable })}
                  />
                }
              />
            }
          >
            <DataTable
              columns={ruleColumns} rows={paperRules} rowKey={keyById}
              rowActions={ruleActions} selectedKey={selectedRuleId}
              onSelect={setSelectedRuleId}
              defaultPageSize={10} pageSizeOptions={[10, 25, 50]}
              searchable colFilters colToggle
              tableId={`${STRATEGY}_rules_paper`} emptyMessage="No paper rules"
            />
          </Accordion>

          {isPaperRuleSelected && positionsSection}
        </RuleRowProvider>
      )}

      {stopConfirm && (
        <StopConfirmDialog
          rule={stopConfirm}
          busy={lifecycleBusyId === stopConfirm.id}
          onCancel={() => setStopConfirm(null)}
          onConfirm={handleStopConfirm}
        />
      )}

      {stopAllConfirm && (
        <StopAllConfirmDialog
          modeLabel={stopAllConfirm.mode === 'real' ? 'Real' : 'Paper'}
          real={stopAllConfirm.mode === 'real'}
          rules={stopAllConfirm.rules}
          busy={bulkBusy === stopAllConfirm.mode}
          onCancel={() => setStopAllConfirm(null)}
          onConfirm={() => void handleStopAllConfirm()}
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

      {inspect && (
        <TokenInspectModal
          target={inspect.target}
          swingOverlay={inspect.swingOverlay}
          onClose={() => setInspect(null)}
        />
      )}
    </div>
  );
}
