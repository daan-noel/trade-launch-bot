import { useState, type ReactNode } from 'react';
import type { RuleRecord } from 'types';
import { Button } from 'components/ui/Button';
import { Input, Textarea } from 'components/ui/Input';
import { Modal, InlineAlert } from 'components/ui/Modal';
import { Select } from 'components/ui/Select';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { TPSL_PARAM_HELP, type TpslParamKey } from 'lib/tpslParamHelp';
import { cn } from 'lib/cn';
import { EXAMPLE_IX_LABELS, parseIxLabels } from './utils';

/** A field label with the standard uppercase styling plus a ⓘ tooltip that
 *  explains the parameter (copy lives in {@link TPSL_PARAM_HELP}). The tooltip
 *  renders in a viewport-clamped portal, so it never overflows the modal. */
function FieldLabel({
  help,
  children,
  accent = 'text-text-dim',
}: {
  help: TpslParamKey;
  children: ReactNode;
  accent?: string;
}) {
  const h = TPSL_PARAM_HELP[help];
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1 text-[10px] font-bold uppercase tracking-wider',
        accent,
      )}
    >
      {children}
      <InfoTooltip title={h.title} body={h.body} />
    </span>
  );
}

export interface RuleFormData {
  ruleName: string;
  tradeMode: string;
  initialBuy: string;
  tolerance: string;
  cuLimit: string;
  cuPrice: string;
  maxSolCost: string;
  spendableSolIn: string;
  maxConcurrentTokens: string;
  maxTotalTokens: string;
  ixLabels: string;
  buyAmount: string;
  takeProfit: string;
  stopLoss: string;
  trailingStopPct: string;
  timeStopSecs: string;
  stallSecs: string;
  liquidityDropPct: string;
}

export function emptyForm(): RuleFormData {
  return {
    ruleName: '',
    tradeMode: 'paper',
    initialBuy: '',
    tolerance: '0',
    cuLimit: '',
    cuPrice: '',
    maxSolCost: '',
    spendableSolIn: '',
    maxConcurrentTokens: '',
    maxTotalTokens: '',
    ixLabels: '',
    buyAmount: '',
    takeProfit: '',
    stopLoss: '',
    trailingStopPct: '',
    timeStopSecs: '',
    stallSecs: '',
    liquidityDropPct: '',
  };
}

export function formFromRule(rule: RuleRecord): RuleFormData {
  const labels = Array.isArray(rule.p_token_ix_labels)
    ? JSON.stringify(rule.p_token_ix_labels)
    : '';
  return {
    ruleName: rule.rule_name,
    tradeMode: rule.trade_mode,
    initialBuy: rule.p_token_initial_buy_sol?.toString() ?? '',
    tolerance: rule.tolerance_pct.toString(),
    cuLimit: rule.p_token_cu_limit?.toString() ?? '',
    cuPrice: rule.p_token_cu_price?.toString() ?? '',
    maxSolCost: rule.p_token_max_sol_cost?.toString() ?? '',
    spendableSolIn: rule.p_token_spendable_sol_in?.toString() ?? '',
    maxConcurrentTokens: rule.p_max_concurrent_tokens?.toString() ?? '',
    maxTotalTokens: rule.p_max_total_tokens?.toString() ?? '',
    ixLabels: labels,
    buyAmount: rule.buy_amount.toString(),
    takeProfit: rule.p_exit_take_profit.toString(),
    stopLoss: rule.p_exit_stop_loss.toString(),
    trailingStopPct: rule.p_exit_trailing_stop_pct?.toString() ?? '',
    timeStopSecs: rule.p_exit_time_stop_secs?.toString() ?? '',
    stallSecs: rule.p_exit_stall_secs?.toString() ?? '',
    liquidityDropPct: rule.p_exit_liquidity_drop_pct?.toString() ?? '',
  };
}

interface RuleFormModalProps {
  open: boolean;
  editRule: RuleRecord | null;
  loading: boolean;
  error: string | null;
  form: RuleFormData;
  onChange: (form: RuleFormData) => void;
  onClose: () => void;
  onSave: (allowParams: boolean) => void;
}

/** A labelled divider that groups the form fields by the param's ROLE
 *  (p_token_ fingerprint / sizing / p_exit_ exit), so each category is
 *  obvious at a glance. `right` hosts the section's action (e.g. the lock). */
function SectionHeader({
  title,
  hint,
  accent = 'text-primary',
  right,
}: {
  title: string;
  hint?: string;
  accent?: string;
  right?: ReactNode;
}) {
  return (
    <div className="mt-1 flex items-center justify-between border-t border-white/10 pt-3">
      <div className="flex items-baseline gap-2">
        <span className={cn('text-[11px] font-bold uppercase tracking-wider', accent)}>{title}</span>
        {hint && <span className="text-[10px] lowercase text-text-dim">{hint}</span>}
      </div>
      {right}
    </div>
  );
}

export function RuleFormModal({
  open,
  editRule,
  loading,
  error,
  form,
  onChange,
  onClose,
  onSave,
}: RuleFormModalProps) {
  const isEdit = editRule != null;
  const [allowEditParams, setAllowEditParams] = useState(false);
  const locked = isEdit && !allowEditParams;

  const set = (patch: Partial<RuleFormData>) => onChange({ ...form, ...patch });

  const fieldCls = (extra?: string) =>
    cn('font-mono', locked && 'cursor-not-allowed opacity-50', extra);

  return (
    <Modal title={isEdit ? 'Edit TPSL1 Rule' : 'New TPSL1 Rule'} open={open} onClose={onClose}>
      <div className="flex flex-col gap-4">
        <label className="flex flex-col gap-1.5">
          <span className="text-[10px] font-bold uppercase tracking-wider text-primary">Mode</span>
          <Select
            fieldSize="md"
            value={form.tradeMode}
            onChange={(e) => set({ tradeMode: e.target.value })}
            className={fieldCls()}
          >
            <option value="paper">Paper Test</option>
            <option value="real">Real Trading</option>
          </Select>
        </label>

        <label className="flex flex-col gap-1.5">
          <span className="text-[10px] font-bold uppercase tracking-wider text-primary">Rule Name</span>
          <Input
            type="text"
            fieldSize="md"
            value={form.ruleName}
            onChange={(e) => set({ ruleName: e.target.value })}
            placeholder="e.g. Sniper 0.5 SOL"
            className={fieldCls()}
          />
        </label>

        {/* ── Token fingerprint: which token this rule matches at creation
            (p_token_*). Locked behind the 🔓 toggle when editing. ── */}
        <SectionHeader
          title="Token Fingerprint"
          hint="which token to match"
          accent="text-info"
          right={
            isEdit ? (
              <button
                type="button"
                onClick={() => setAllowEditParams((v) => !v)}
                className="rounded-lg border border-white/12 bg-white/4 px-2 py-1 text-xs"
                title={allowEditParams ? 'Lock match criteria' : 'Unlock match criteria'}
              >
                {allowEditParams ? '🔒' : '🔓'}
              </button>
            ) : undefined
          }
        />
        <div className="grid grid-cols-2 gap-3">
          <label className="flex flex-col gap-1.5">
            <FieldLabel help="initialBuy">Initial Buy SOL</FieldLabel>
            <Input type="number" fieldSize="md" step="0.001" value={form.initialBuy} readOnly={locked}
              onChange={(e) => set({ initialBuy: e.target.value })} className={fieldCls()} />
          </label>
          <label className="flex flex-col gap-1.5">
            <FieldLabel help="tolerance">Tolerance %</FieldLabel>
            <Input type="number" fieldSize="md" step="0.1" value={form.tolerance} readOnly={locked}
              onChange={(e) => set({ tolerance: e.target.value })} className={fieldCls()} />
          </label>
          <label className="flex flex-col gap-1.5">
            <FieldLabel help="cuLimit">CU Limit</FieldLabel>
            <Input type="number" fieldSize="md" value={form.cuLimit} readOnly={locked}
              onChange={(e) => set({ cuLimit: e.target.value })} className={fieldCls()} />
          </label>
          <label className="flex flex-col gap-1.5">
            <FieldLabel help="cuPrice">CU Price</FieldLabel>
            <Input type="number" fieldSize="md" value={form.cuPrice} readOnly={locked}
              onChange={(e) => set({ cuPrice: e.target.value })} className={fieldCls()} />
          </label>
          <label className="flex flex-col gap-1.5">
            <FieldLabel help="maxSolCost">Max SOL Cost</FieldLabel>
            <Input type="number" fieldSize="md" step="0.001" value={form.maxSolCost} readOnly={locked}
              onChange={(e) => set({ maxSolCost: e.target.value })} className={fieldCls()} />
          </label>
          <label className="flex flex-col gap-1.5">
            <FieldLabel help="spendableSolIn">Spendable SOL In</FieldLabel>
            <Input type="number" fieldSize="md" step="0.001" value={form.spendableSolIn} readOnly={locked}
              onChange={(e) => set({ spendableSolIn: e.target.value })} className={fieldCls()} />
          </label>
        </div>

        <div className="flex flex-col gap-1.5">
          <div className="flex items-center justify-between">
            <FieldLabel help="ixLabels">Instruction Labels</FieldLabel>
            {!isEdit && (
              <button
                type="button"
                onClick={() => set({ ixLabels: EXAMPLE_IX_LABELS })}
                className="rounded-lg border border-white/12 bg-white/4 px-2 py-1 text-xs"
                title="Insert example labels"
              >
                ⎘
              </button>
            )}
          </div>
          <Textarea
            fieldSize="md"
            rows={4}
            value={form.ixLabels}
            readOnly={locked}
            onChange={(e) => set({ ixLabels: e.target.value })}
            className={fieldCls()}
            placeholder='["Pump.Fun: Buy"]'
          />
        </div>

        {/* ── Sizing & limits: position size + concurrency caps (unprefixed). ── */}
        <SectionHeader title="Sizing & Limits" hint="position size + concurrency" accent="text-text-dim" />
        <div className="grid grid-cols-3 gap-3">
          <label className="flex flex-col gap-1.5">
            <FieldLabel help="buyAmount" accent="text-primary">Buy Amount (SOL)</FieldLabel>
            <Input type="number" fieldSize="md" step="0.001" value={form.buyAmount}
              onChange={(e) => set({ buyAmount: e.target.value })} className={fieldCls()} />
          </label>
          <label className="flex flex-col gap-1.5">
            <FieldLabel help="maxConcurrentTokens">Max Concurrent Tokens</FieldLabel>
            <Input type="number" fieldSize="md" value={form.maxConcurrentTokens} readOnly={locked}
              onChange={(e) => set({ maxConcurrentTokens: e.target.value })} className={fieldCls()} />
          </label>
          <label className="flex flex-col gap-1.5">
            <FieldLabel help="maxTotalTokens">Max Total Tokens</FieldLabel>
            <Input type="number" fieldSize="md" value={form.maxTotalTokens} readOnly={locked}
              onChange={(e) => set({ maxTotalTokens: e.target.value })} className={fieldCls()} />
          </label>
        </div>

        {/* ── Exit gates: when to sell (p_exit_*). 0 = off. ── */}
        <SectionHeader title="Exit Gates" hint="when to sell" accent="text-warning" />
        <div className="grid grid-cols-3 gap-3">
          <label className="flex flex-col gap-1.5">
            <FieldLabel help="takeProfit" accent="text-primary">Take Profit %</FieldLabel>
            <Input type="number" fieldSize="md" step="1" value={form.takeProfit}
              onChange={(e) => set({ takeProfit: e.target.value })} className={fieldCls('focus:border-green')} />
          </label>
          <label className="flex flex-col gap-1.5">
            <FieldLabel help="stopLoss" accent="text-primary">Stop Loss %</FieldLabel>
            <Input type="number" fieldSize="md" step="1" value={form.stopLoss}
              onChange={(e) => set({ stopLoss: e.target.value })} className={fieldCls('focus:border-red')} />
          </label>
          <label className="flex flex-col gap-1.5">
            <FieldLabel help="trailingStopPct" accent="text-primary">Trailing Stop %</FieldLabel>
            <Input type="number" fieldSize="md" step="1" value={form.trailingStopPct}
              onChange={(e) => set({ trailingStopPct: e.target.value })}
              className={fieldCls('focus:border-warning')} placeholder="0 = off" />
          </label>
          <label className="flex flex-col gap-1.5">
            <FieldLabel help="timeStopSecs" accent="text-primary">Time Stop (s)</FieldLabel>
            <Input type="number" fieldSize="md" step="1" value={form.timeStopSecs}
              onChange={(e) => set({ timeStopSecs: e.target.value })}
              className={fieldCls('focus:border-info')} placeholder="0 = off" />
          </label>
          <label className="flex flex-col gap-1.5">
            <FieldLabel help="stallSecs" accent="text-primary">Stall (s)</FieldLabel>
            <Input type="number" fieldSize="md" step="1" value={form.stallSecs}
              onChange={(e) => set({ stallSecs: e.target.value })}
              className={fieldCls('focus:border-accent')} placeholder="0 = off" />
          </label>
          <label className="flex flex-col gap-1.5">
            <FieldLabel help="liquidityDropPct" accent="text-primary">Liquidity Drop %</FieldLabel>
            <Input type="number" fieldSize="md" step="1" value={form.liquidityDropPct}
              onChange={(e) => set({ liquidityDropPct: e.target.value })}
              className={fieldCls('focus:border-primary')} placeholder="0 = off" />
          </label>
        </div>

        {error && <InlineAlert variant="error">{error}</InlineAlert>}

        <div className="flex justify-end gap-2.5">
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" onClick={() => onSave(allowEditParams)} disabled={loading}>
            {loading ? 'Saving…' : 'Save Rule'}
          </Button>
        </div>
      </div>
    </Modal>
  );
}

export function buildCreatePayload(form: RuleFormData) {
  const parseOptF = (s: string) => (s.trim() ? parseFloat(s) : undefined);
  const parseOptU = (s: string) => (s.trim() ? parseInt(s, 10) : undefined);
  return {
    rule_name: form.ruleName,
    p_token_initial_buy_sol: parseOptF(form.initialBuy) ?? null,
    p_token_cu_limit: parseOptU(form.cuLimit) ?? null,
    p_token_cu_price: parseOptU(form.cuPrice) ?? null,
    p_token_max_sol_cost: parseOptF(form.maxSolCost) ?? null,
    p_token_spendable_sol_in: parseOptF(form.spendableSolIn) ?? null,
    p_max_concurrent_tokens: parseOptU(form.maxConcurrentTokens) ?? null,
    p_max_total_tokens: parseOptU(form.maxTotalTokens) ?? null,
    p_token_ix_labels: parseIxLabels(form.ixLabels),
    trade_mode: form.tradeMode,
    buy_amount: parseFloat(form.buyAmount),
    p_exit_take_profit: parseFloat(form.takeProfit),
    p_exit_stop_loss: parseFloat(form.stopLoss),
    p_exit_trailing_stop_pct: parseOptF(form.trailingStopPct) ?? null,
    p_exit_time_stop_secs: parseOptU(form.timeStopSecs) ?? null,
    p_exit_stall_secs: parseOptU(form.stallSecs) ?? null,
    p_exit_liquidity_drop_pct: parseOptF(form.liquidityDropPct) ?? null,
    tolerance_pct: form.tolerance.trim() ? parseFloat(form.tolerance) : null,
  };
}

export function buildUpdatePayload(form: RuleFormData, allowParams: boolean) {
  const base: Record<string, unknown> = {
    rule_name: form.ruleName,
    buy_amount: parseFloat(form.buyAmount),
    p_exit_take_profit: parseFloat(form.takeProfit),
    p_exit_stop_loss: parseFloat(form.stopLoss),
    // Exit params (always editable): 0 disables, per the ignore_zero convention.
    p_exit_trailing_stop_pct: form.trailingStopPct.trim() ? parseFloat(form.trailingStopPct) : 0,
    p_exit_time_stop_secs: form.timeStopSecs.trim() ? parseInt(form.timeStopSecs, 10) : 0,
    p_exit_stall_secs: form.stallSecs.trim() ? parseInt(form.stallSecs, 10) : 0,
    p_exit_liquidity_drop_pct: form.liquidityDropPct.trim() ? parseFloat(form.liquidityDropPct) : 0,
    trade_mode: form.tradeMode,
    tolerance_pct: form.tolerance.trim() ? parseFloat(form.tolerance) : undefined,
  };
  if (!allowParams) return base;
  return {
    ...base,
    p_token_initial_buy_sol: form.initialBuy.trim() ? parseFloat(form.initialBuy) : 0,
    p_token_cu_limit: form.cuLimit.trim() ? parseInt(form.cuLimit, 10) : 0,
    p_token_cu_price: form.cuPrice.trim() ? parseInt(form.cuPrice, 10) : 0,
    p_token_max_sol_cost: form.maxSolCost.trim() ? parseFloat(form.maxSolCost) : 0,
    p_token_spendable_sol_in: form.spendableSolIn.trim() ? parseFloat(form.spendableSolIn) : 0,
    p_max_concurrent_tokens: form.maxConcurrentTokens.trim()
      ? parseInt(form.maxConcurrentTokens, 10)
      : 0,
    p_max_total_tokens: form.maxTotalTokens.trim()
      ? parseInt(form.maxTotalTokens, 10)
      : 0,
    p_token_ix_labels: parseIxLabels(form.ixLabels),
  };
}
