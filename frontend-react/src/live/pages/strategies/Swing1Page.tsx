import { memo, useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { DataTable } from 'components/table/DataTable';
import { Accordion } from 'components/ui/Accordion';
import { Badge, type BadgeVariant } from 'components/ui/Badge';
import { SectionDivider } from 'components/ui/SectionDivider';
import { Button } from 'components/ui/Button';
import { InlineAlert, Modal } from 'components/ui/Modal';
import { SimSummaryCard } from 'components/tpsl1/SimSummaryCard';
import { TokenInspectModal, type InspectTarget } from 'components/tpsl1/TokenInspectModal';
import { positionColumns } from 'components/tpsl1/tableColumns';
import { ruleColumns as ruleCols1, RuleRowProvider } from 'components/tpsl1/ruleColumns';
import {
  buildCreatePayload,
  buildUpdatePayload,
  emptyForm,
  formFromRule,
  Swing1RuleAccordion,
  type RuleFormData,
  type LockGroupState,
} from './Swing1RuleAccordion';
import {
  activateSwing1Rule,
  createSwing1Rule,
  deleteSwing1Rule,
  fetchSwing1RulePositions,
  fetchSwing1Rules,
  pauseSwing1Rule,
  stopSwing1Rule,
  updateSwing1Rule,
} from 'services/api';
import { connectTpslPositionsChanged } from 'services/sse';
import { apiErrorMessage, useGetTokensByMintsQuery } from 'store/apiSlice';
import { useSellTokenMutation } from '@live/store/liveEndpoints';
import { mergeTokenData } from 'components/tokens/sharedTokenColumns';
import { usePolledRules } from 'hooks/usePolledRules';
import { useRulePositions } from 'hooks/useRulePositions';
import { usePriceDisplay } from 'hooks/usePriceDisplay';
import { cn } from 'lib/cn';
import { VisibilityToggleButton } from 'components/ui/VisibilityToggleButton';
import type { RulePositionRecord, RuleRecord } from 'types';
import type { ColumnDef } from 'components/table/types';

/** swing1's SSE / positions filter key — the backend emits `swing_1`. */
const STRATEGY = 'swing_1';

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
function positionToSimResult(p: RulePositionRecord) {
  const pnlPercent = p.pnl_percent;
  const entryPrice = p.entry_price ?? 0;
  const entryTokens = p.entry_token_amount ?? 0;
  const pnlSol = pnlPercent != null ? entryTokens * (pnlPercent / 100) : null;
  const holdingSecs =
    p.entry_time && p.exit_time
      ? Math.round((new Date(p.exit_time).getTime() - new Date(p.entry_time).getTime()) / 1000)
      : null;
  const athPrice = p.exit_price != null ? Math.max(p.exit_price, entryPrice) : entryPrice;
  return {
    mint: p.mint, symbol: p.symbol ?? '',
    target_price: p.target_price, target_token_amount: p.target_token_amount,
    target_time: p.target_time, target_tx: p.target_tx,
    entry_price: entryPrice, ath_price: athPrice,
    entry_token_amount: entryTokens, entry_tx: p.entry_tx,
    entry_time: p.entry_time ?? p.created_at,
    exit_price: p.exit_price, exit_tx: p.exit_tx, exit_time: p.exit_time,
    holding_secs: holdingSecs, pnl_percent: pnlPercent, pnl_sol: pnlSol,
    exit_reason: p.exit_reason ?? 'Open', total_trades: 0,
  };
}

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

export function Swing1Page() {
  const price = usePriceDisplay();

  // Filter out the 'analyze' column — no sim/matched/paper on the live bin.
  const ruleColumns = useMemo(
    () => ruleCols1.filter((c: ColumnDef<RuleRecord>) => c.key !== 'analyze'),
    [],
  );

  const { rules, setRules, loading, error, refresh: loadRules } = usePolledRules(
    fetchSwing1Rules,
    STRATEGY,
  );
  const [selectedRuleId, setSelectedRuleId] = useState<string | null>(null);
  const { positions, loading: positionsLoading, error: positionsError } =
    useRulePositions(selectedRuleId, rules, fetchSwing1RulePositions, STRATEGY);

  const [modalOpen, setModalOpen] = useState(false);
  const [editRule, setEditRule] = useState<RuleRecord | null>(null);
  const [form, setForm] = useState<RuleFormData>(emptyForm());
  const [formError, setFormError] = useState<string | null>(null);
  const [formLoading, setFormLoading] = useState(false);

  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [deleteLoading, setDeleteLoading] = useState(false);

  const [lifecycleBusyId, setLifecycleBusyId] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [stopConfirm, setStopConfirm] = useState<RuleRecord | null>(null);
  const [reactivate, setReactivate] = useState<RuleRecord | null>(null);

  const [inspect, setInspect] = useState<{ key: string; target: InspectTarget } | null>(null);
  const [showPending, setShowPending] = useState(false);

  const [sellToken] = useSellTokenMutation();
  const [sellingPositionMint, setSellingPositionMint] = useState<string | null>(null);

  // SSE: position deltas for the selected rule keep the positions list live.
  useEffect(() => {
    const handle = connectTpslPositionsChanged(STRATEGY, (delta) => {
      if (delta.ruleId !== selectedRuleId) return;
      void loadRules(false);
    });
    return () => handle.close();
  }, [selectedRuleId, loadRules]);

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
    if (rule.trade_mode === 'paper') setReactivate(rule);
    else void handleActivate(rule, 'fresh');
  }, [handleActivate]);

  const handleStopConfirm = useCallback(async (rule: RuleRecord) => {
    setLifecycleBusyId(rule.id);
    setActionError(null);
    try { applyRuleUpdate(await stopSwing1Rule(rule.id)); setStopConfirm(null); }
    catch (e) { setActionError(e instanceof Error ? e.message : 'Failed to stop rule'); }
    finally { setLifecycleBusyId(null); }
  }, [applyRuleUpdate]);

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

  const openDuplicate = useCallback((rule: RuleRecord) => {
    setEditRule(null);
    setForm({ ...formFromRule(rule), ruleName: `${rule.rule_name} (copy)` });
    setFormError(null);
    setModalOpen(true);
  }, []);

  const handleSave = async (unlocked: LockGroupState) => {
    setFormError(null);
    if (!form.ruleName.trim()) { setFormError('Rule name is required'); return; }
    for (const [label, val] of [
      ['buy amount', form.buyAmount],
      ['take profit', form.axes.take_profit],
      ['stop loss', form.axes.stop_loss],
    ] as const) {
      if (!val?.trim() || Number.isNaN(parseFloat(val))) {
        setFormError(`Invalid ${label}`);
        return;
      }
    }
    setFormLoading(true);
    try {
      if (editRule) {
        const updated = await updateSwing1Rule(editRule.id, buildUpdatePayload(form, unlocked));
        setRules((prev) => prev.map((r) => (r.id === updated.id ? updated : r)));
      } else {
        const created = await createSwing1Rule(buildCreatePayload(form));
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

  const isPreFill = (s: string) => s === 'Arming' || s === 'BuySubmitted';
  const positionSummaryTokens = useMemo(
    () => positions.filter((p) => !isPreFill(p.status)).map(positionToSimResult),
    [positions],
  );
  const pendingCount = useMemo(() => positions.filter((p) => isPreFill(p.status)).length, [positions]);
  const visiblePositions = useMemo(
    () => showPending ? positions : positions.filter((p) => !isPreFill(p.status)),
    [positions, showPending],
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
          count={positionsLoading || positionsError ? undefined : visiblePositions.length}
          subtitle={selectedRuleName ?? undefined}
          action={
            pendingCount > 0 ? (
              <VisibilityToggleButton
                visible={showPending}
                onToggle={() => setShowPending((v) => !v)}
                label="pending entry"
              >
                {showPending ? `Hide pending (${pendingCount})` : `Show pending (${pendingCount})`}
              </VisibilityToggleButton>
            ) : undefined
          }
        />
        {positionsLoading && <p className="text-text-dim">Loading positions…</p>}
        {positionsError && <InlineAlert variant="error">{positionsError}</InlineAlert>}
        {!positionsLoading && !positionsError && positionSummaryTokens.length > 0 && (
          <SimSummaryCard
            title="Positions Summary"
            ruleName={selectedRuleName ?? ''}
            tokens={positionSummaryTokens}
            price={price}
          />
        )}
        {!positionsLoading && !positionsError && (
          <DataTable
            columns={positionColumns}
            rows={mergeTokenData(visiblePositions, tokenMap)}
            rowKey={keyById}
            selectedKey={inspect?.key ?? null}
            onSelect={onSelectPosition}
            rowActions={isRealRuleSelected ? positionRowActions : undefined}
            defaultPageSize={20}
            pageSizeOptions={[20, 50, 100]}
            colFilters
            colToggle
            emptyMessage="No positions for this rule."
          />
        )}
      </section>
    </>
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

      {reactivate && (
        <ReactivateDialog
          rule={reactivate}
          busy={lifecycleBusyId === reactivate.id}
          onCancel={() => setReactivate(null)}
          onActivate={handleActivate}
        />
      )}

      <Swing1RuleAccordion
        open={modalOpen} editRule={editRule}
        loading={formLoading} error={formError}
        form={form} onChange={setForm}
        onClose={() => setModalOpen(false)}
        onSave={handleSave}
      />

      {inspect && (
        <TokenInspectModal target={inspect.target} onClose={() => setInspect(null)} />
      )}
    </div>
  );
}
