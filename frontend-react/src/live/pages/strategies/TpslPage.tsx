import { memo, useCallback, useMemo, useState, type ReactNode } from 'react';
import { DataTable } from 'components/table/DataTable';
import { Accordion } from 'components/ui/Accordion';
import { Badge, type BadgeVariant } from 'components/ui/Badge';
import { SectionDivider } from 'components/ui/SectionDivider';
import { Button } from 'components/ui/Button';
import { InlineAlert, Modal } from 'components/ui/Modal';
import { SimSummaryCard } from 'components/tpsl1/SimSummaryCard';
import { TokenInspectModal, type InspectTarget } from 'components/tpsl1/TokenInspectModal';
import { positionColumns } from 'components/tpsl1/tableColumns';
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
import { ruleColumns as ruleCols1, RuleRowProvider as RuleRowProvider1 } from 'components/tpsl1/ruleColumns';
import { ruleColumns as ruleCols2, RuleRowProvider as RuleRowProvider2 } from 'components/tpsl2/ruleColumns';
import {
  activateTpsl1Rule,
  activateTpsl2Rule,
  createTpsl1Rule,
  createTpsl2Rule,
  deleteTpsl1Rule,
  deleteTpsl2Rule,
  fetchTpsl1RulePositions,
  fetchTpsl2RulePositions,
  fetchTpsl1RulePositionsSummary,
  fetchTpsl2RulePositionsSummary,
  fetchTpsl1Rules,
  fetchTpsl2Rules,
  pauseTpsl1Rule,
  pauseTpsl2Rule,
  stopTpsl1Rule,
  stopTpsl2Rule,
  updateTpsl1Rule,
  updateTpsl2Rule,
} from 'services/api';
import { apiErrorMessage, useGetTokensByMintsQuery } from 'store/apiSlice';
import { useSellTokenMutation } from '@live/store/liveEndpoints';
import { mergeTokenData } from 'components/tokens/sharedTokenColumns';
import { usePolledRules } from 'hooks/usePolledRules';
import { useRulePositions, DEFAULT_POSITIONS_QUERY } from 'hooks/useRulePositions';
import { numericColKeys } from 'services/tableRequest';
import type { TableQuery } from 'components/table/types';

/** Positions columns that filter numerically (`>5`/`1..10` → structured ops). */
const POSITION_NUMERIC_COLS = numericColKeys(positionColumns);
import { usePriceDisplay } from 'hooks/usePriceDisplay';
import { cn } from 'lib/cn';
import type { RulePositionRecord, RuleRecord } from 'types';
import type { ColumnDef } from 'components/table/types';

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
  strategy,
}: {
  rule: RuleRecord;
  confirmingDelete: boolean;
  deleteLoading: boolean;
  onEdit: (rule: RuleRecord) => void;
  onDuplicate: (rule: RuleRecord) => void;
  onDelete: (ruleId: string) => void;
  onRequestDelete: (ruleId: string) => void;
  onCancelDelete: () => void;
  strategy: 'tpsl1' | 'tpsl2';
}) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    const json = serializeRuleJson(getSpec(strategy), rule);
    try {
      await navigator.clipboard.writeText(json);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch { /* ignore */ }
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


function inspectFromPosition(r: RulePositionRecord): InspectTarget {
  return {
    mint: r.mint,
    entryTime: r.entry_time, entryPrice: r.entry_price, entryTx: r.entry_tx,
    exitTime: r.exit_time, exitPrice: r.exit_price, exitTx: r.exit_tx,
    exitLabel: r.status && r.status !== 'Open' ? r.status : null,
  };
}

// Stable no-op analysis bag — the 'analyze' column is filtered out of the live
// rule table, so these handlers are never called, but the RuleRowProvider type
// still requires the field.
const NO_ANALYSIS = {
  simLoading: false, matchedLoading: false, paperLoading: false,
  simActiveId: null, matchedActiveId: null, paperActiveId: null,
  onSimulate: () => {}, onMatched: () => {}, onPaperResult: () => {},
};

export function TpslPage({ strategy }: { strategy: 'tpsl1' | 'tpsl2' }) {
  const is1 = strategy === 'tpsl1';
  const price = usePriceDisplay();

  // Per-strategy API functions and UI
  const fetchRules = is1 ? fetchTpsl1Rules : fetchTpsl2Rules;
  const createRule = is1 ? createTpsl1Rule : createTpsl2Rule;
  const updateRule = is1 ? updateTpsl1Rule : updateTpsl2Rule;
  const deleteRule = is1 ? deleteTpsl1Rule : deleteTpsl2Rule;
  const activateRule = is1 ? activateTpsl1Rule : activateTpsl2Rule;
  const pauseRule = is1 ? pauseTpsl1Rule : pauseTpsl2Rule;
  const stopRule = is1 ? stopTpsl1Rule : stopTpsl2Rule;
  const fetchPositions = is1 ? fetchTpsl1RulePositions : fetchTpsl2RulePositions;
  const fetchSummary = is1 ? fetchTpsl1RulePositionsSummary : fetchTpsl2RulePositionsSummary;
  const spec = getSpec(strategy);
  // Filter out the 'analyze' column — no sim/matched/paper on the live bin.
  const ruleColumns = useMemo(
    () => (is1 ? ruleCols1 : ruleCols2).filter((c: ColumnDef<RuleRecord>) => c.key !== 'analyze'),
    [is1],
  );

  const { rules, setRules, loading, error } = usePolledRules(fetchRules, strategy);
  const [selectedRuleId, setSelectedRuleId] = useState<string | null>(null);
  // Server-side positions view: the DataTable emits page/pageSize/sort/search/filter
  // via `onQueryChange`; the hook fetches that page + the (whole-run) summary.
  const [posQuery, setPosQuery] = useState<TableQuery>(DEFAULT_POSITIONS_QUERY);
  const { positions, total: positionsTotal, summary: positionsSummary,
    loading: positionsLoading, error: positionsError } =
    useRulePositions(selectedRuleId, rules, fetchPositions, fetchSummary, strategy, posQuery, POSITION_NUMERIC_COLS);

  const [modalOpen, setModalOpen] = useState(false);
  const [editRule, setEditRule] = useState<RuleRecord | null>(null);
  const [form, setForm] = useState<FormState>(() => emptyForm(spec));
  const [formError, setFormError] = useState<string | null>(null);
  const [formLoading, setFormLoading] = useState(false);

  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [deleteLoading, setDeleteLoading] = useState(false);

  const [lifecycleBusyId, setLifecycleBusyId] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [stopConfirm, setStopConfirm] = useState<RuleRecord | null>(null);
  const [reactivate, setReactivate] = useState<RuleRecord | null>(null);

  const [inspect, setInspect] = useState<{ key: string; target: InspectTarget } | null>(null);

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
  }, []);

  const handlePause = useCallback(async (rule: RuleRecord) => {
    setLifecycleBusyId(rule.id);
    setActionError(null);
    try { applyRuleUpdate(await pauseRule(rule.id)); }
    catch (e) { setActionError(e instanceof Error ? e.message : 'Failed to pause rule'); }
    finally { setLifecycleBusyId(null); }
  }, [applyRuleUpdate, pauseRule]);

  const handleActivate = useCallback(async (rule: RuleRecord, paperRun: 'fresh' | 'continue' = 'fresh') => {
    setLifecycleBusyId(rule.id);
    setActionError(null);
    try { applyRuleUpdate(await activateRule(rule.id, paperRun)); setReactivate(null); }
    catch (e) { setActionError(e instanceof Error ? e.message : 'Failed to activate rule'); }
    finally { setLifecycleBusyId(null); }
  }, [applyRuleUpdate, activateRule]);

  const handleActivateClick = useCallback((rule: RuleRecord) => {
    if (rule.trade_mode === 'paper') setReactivate(rule);
    else void handleActivate(rule, 'fresh');
  }, [handleActivate]);

  const handleStopConfirm = useCallback(async (rule: RuleRecord) => {
    setLifecycleBusyId(rule.id);
    setActionError(null);
    try { applyRuleUpdate(await stopRule(rule.id)); setStopConfirm(null); }
    catch (e) { setActionError(e instanceof Error ? e.message : 'Failed to stop rule'); }
    finally { setLifecycleBusyId(null); }
  }, [applyRuleUpdate, stopRule]);

  const ruleControls = useMemo(() => ({
    busyId: lifecycleBusyId,
    onPause: handlePause,
    onResume: (r: RuleRecord) => void handleActivate(r, 'continue'),
    onStop: (r: RuleRecord) => setStopConfirm(r),
    onActivate: handleActivateClick,
  }), [lifecycleBusyId, handlePause, handleActivate, handleActivateClick]);

  const rowContext = useMemo(
    () => ({ controls: ruleControls, analysis: NO_ANALYSIS }),
    [ruleControls],
  );

  const openAdd = () => {
    setEditRule(null);
    setForm(emptyForm(spec));
    setFormError(null);
    setModalOpen(true);
  };

  const openEdit = useCallback((rule: RuleRecord) => {
    setEditRule(rule);
    setForm(formFromRule(spec, rule));
    setFormError(null);
    setModalOpen(true);
  }, [spec]);

  const openDuplicate = useCallback((rule: RuleRecord) => {
    setEditRule(null);
    setForm({ ...formFromRule(spec, rule), [RULE_NAME_KEY]: `${rule.rule_name} (copy)` });
    setFormError(null);
    setModalOpen(true);
  }, [spec]);

  const handleSave = async (unlocked: LockState) => {
    setFormError(null);
    if (!(form[RULE_NAME_KEY] ?? '').trim()) { setFormError('Rule name is required'); return; }
    for (const [label, col] of [
      ['buy amount', 'buy_amount'],
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
        const updated = await updateRule(editRule.id, buildUpdatePayload(spec, form, unlocked));
        setRules((prev) => prev.map((r) => (r.id === updated.id ? updated : r)));
      } else {
        const created = await createRule(buildCreatePayload(spec, form));
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
      await deleteRule(ruleId);
      setRules((prev) => prev.filter((r) => r.id !== ruleId));
      if (selectedRuleId === ruleId) setSelectedRuleId(null);
    } catch { /* ignore */ }
    finally { setConfirmDeleteId(null); setDeleteLoading(false); }
  }, [selectedRuleId, deleteRule, setRules]);

  const cancelDelete = useCallback(() => setConfirmDeleteId(null), []);

  const ruleActions = useCallback((rule: RuleRecord) => (
    <RuleActionsCell
      rule={rule} strategy={strategy}
      confirmingDelete={confirmDeleteId === rule.id}
      deleteLoading={deleteLoading}
      onEdit={openEdit} onDuplicate={openDuplicate}
      onDelete={handleDelete}
      onRequestDelete={setConfirmDeleteId}
      onCancelDelete={cancelDelete}
    />
  ), [confirmDeleteId, deleteLoading, openEdit, openDuplicate, handleDelete, cancelDelete, strategy]);

  const realRules = useMemo(() => rules.filter((r) => r.trade_mode === 'real'), [rules]);
  const paperRules = useMemo(() => rules.filter((r) => r.trade_mode !== 'real'), [rules]);

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


  const allMints = useMemo(() => {
    const s = new Set(positions.map((r) => r.mint));
    return [...s].sort();
  }, [positions]);
  const { data: tokenBatch } = useGetTokensByMintsQuery(allMints, { skip: allMints.length === 0 });
  const tokenMap = useMemo(
    () => new Map((tokenBatch ?? []).map((t) => [t.mint_address, t])),
    [tokenBatch],
  );

  const onSelectPosition = useCallback((key: string | null) => {
    const row = key ? positions.find((p) => p.id === key) ?? null : null;
    setInspect(row ? { key: row.id, target: inspectFromPosition(row) } : null);
  }, [positions]);

  const handleSellPosition = useCallback(async (mint: string) => {
    setSellingPositionMint(mint);
    setActionError(null);
    try { await sellToken({ mint }).unwrap(); }
    catch (e) {
      setActionError(`Sell failed: ${apiErrorMessage(e as Parameters<typeof apiErrorMessage>[0]) ?? 'unknown error'}`);
    }
    finally { setSellingPositionMint(null); }
  }, [sellToken]);

  const positionRowActions = useCallback((row: RulePositionRecord) => {
    if (row.status !== 'Holding') return null;
    const isSelling = sellingPositionMint === row.mint;
    return (
      <div className="flex items-center justify-center" onClick={(e) => e.stopPropagation()}>
        <button
          type="button" disabled={isSelling}
          onClick={() => { void handleSellPosition(row.mint); }}
          className="rounded border border-red/50 bg-red/12 px-2 py-0.5 text-[11px] font-semibold text-red hover:bg-red/22 disabled:opacity-45"
        >
          {isSelling ? 'Selling…' : 'Sell ALL'}
        </button>
      </div>
    );
  }, [sellingPositionMint, handleSellPosition]);

  const positionsSection = (
    <>
      <SectionDivider />
      <section>
        <SectionHeading
          title="Positions"
          marker="bg-info"
          badge="info"
          count={positionsError ? undefined : positionsTotal}
          subtitle={selectedRuleName ?? undefined}
        />
        {positionsError && <InlineAlert variant="error">{positionsError}</InlineAlert>}
        {/* Summary is server-computed over the whole run (not the visible page), so
            it renders as long as a rule is selected — independent of paging. */}
        {!positionsError && positionsSummary && positionsSummary.tokens > 0 && (
          <SimSummaryCard
            title="Positions Summary"
            ruleName={selectedRuleName ?? ''}
            summary={positionsSummary}
            price={price}
          />
        )}
        {!positionsError && (
          <DataTable
            columns={positionColumns}
            rows={mergeTokenData(positions, tokenMap)}
            rowKey={keyById}
            selectedKey={inspect?.key ?? null}
            onSelect={onSelectPosition}
            rowActions={isRealRuleSelected ? positionRowActions : undefined}
            serverSide
            serverTotal={positionsTotal}
            onQueryChange={setPosQuery}
            loading={positionsLoading}
            resetKey={selectedRuleId ?? ''}
            defaultPageSize={20}
            pageSizeOptions={[20, 50, 100]}
            colFilters
            colToggle
            tableId={`${strategy}_positions`}
            emptyMessage="No positions for this rule."
          />
        )}
      </section>
    </>
  );

  const label = strategy === 'tpsl1' ? 'TPSL1' : 'TPSL2';
  // tpsl1/tpsl2 ruleColumns each define their OWN RuleRowContext, and a row's
  // RunControls cell reads its own strategy's context — so the page must provide
  // the provider that matches the columns being rendered, else the tpsl2 control
  // cell renders outside its provider and throws.
  const RuleRowProvider = is1 ? RuleRowProvider1 : RuleRowProvider2;

  return (
    <div>
      <SectionHeading
        size="h2"
        title={`${label} Strategies`}
        count={!loading && !error ? rules.length : undefined}
        action={<Button variant="primary" onClick={openAdd}>+ Add Rule</Button>}
      />

      {actionError && <InlineAlert variant="error">{actionError}</InlineAlert>}
      {loading && <p className="text-text-dim">Loading rules…</p>}
      {error && <InlineAlert variant="error">{error}</InlineAlert>}

      {modalOpen && (
        <div className="mb-5">
          <SpecRuleForm
            spec={spec}
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
                />
              }
            >
              <DataTable
                columns={ruleColumns} rows={realRules} rowKey={keyById}
                rowActions={ruleActions} selectedKey={selectedRuleId}
                onSelect={setSelectedRuleId}
                defaultPageSize={10} pageSizeOptions={[10, 25, 50]}
                searchable colFilters colToggle
                tableId={`${strategy}_rules_real`} emptyMessage="No real rules"
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
              />
            }
          >
            <DataTable
              columns={ruleColumns} rows={paperRules} rowKey={keyById}
              rowActions={ruleActions} selectedKey={selectedRuleId}
              onSelect={setSelectedRuleId}
              defaultPageSize={10} pageSizeOptions={[10, 25, 50]}
              searchable colFilters colToggle
              tableId={`${strategy}_rules_paper`} emptyMessage="No paper rules"
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

      {reactivate && (
        <ReactivateDialog
          rule={reactivate}
          busy={lifecycleBusyId === reactivate.id}
          onCancel={() => setReactivate(null)}
          onActivate={handleActivate}
        />
      )}

      {inspect && (
        <TokenInspectModal target={inspect.target} onClose={() => setInspect(null)} />
      )}
    </div>
  );
}
