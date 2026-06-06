import { useState } from 'react';
import type { RuleRecord } from 'types';
import { Button } from 'components/ui/Button';
import { Input, Textarea } from 'components/ui/Input';
import { Modal, InlineAlert } from 'components/ui/Modal';
import { Select } from 'components/ui/Select';
import { cn } from 'lib/cn';
import { EXAMPLE_IX_LABELS, parseIxLabels } from './utils';

export interface RuleFormData {
  ruleName: string;
  tradeMode: string;
  initialBuy: string;
  tolerance: string;
  cuLimit: string;
  cuPrice: string;
  maxSolCost: string;
  spendableSolIn: string;
  maxHoldingTokens: string;
  totalMaxTradeTokens: string;
  ixLabels: string;
  buyAmount: string;
  takeProfit: string;
  stopLoss: string;
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
    maxHoldingTokens: '',
    totalMaxTradeTokens: '',
    ixLabels: '',
    buyAmount: '',
    takeProfit: '',
    stopLoss: '',
  };
}

export function formFromRule(rule: RuleRecord): RuleFormData {
  const labels = Array.isArray(rule.p_ix_labels)
    ? JSON.stringify(rule.p_ix_labels)
    : '';
  return {
    ruleName: rule.rule_name,
    tradeMode: rule.trade_mode,
    initialBuy: rule.p_initial_buy_sol?.toString() ?? '',
    tolerance: rule.tolerance_pct.toString(),
    cuLimit: rule.p_cu_limit?.toString() ?? '',
    cuPrice: rule.p_cu_price?.toString() ?? '',
    maxSolCost: rule.p_max_sol_cost?.toString() ?? '',
    spendableSolIn: rule.p_spendable_sol_in?.toString() ?? '',
    maxHoldingTokens: rule.p_max_holding_tokens?.toString() ?? '',
    totalMaxTradeTokens: rule.p_total_max_trade_tokens?.toString() ?? '',
    ixLabels: labels,
    buyAmount: rule.buy_amount.toString(),
    takeProfit: rule.take_profit.toString(),
    stopLoss: rule.stop_loss.toString(),
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
    <Modal title={isEdit ? 'Edit TPSL Rule' : 'New TPSL Rule'} open={open} onClose={onClose}>
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

        <div className="grid grid-cols-2 gap-3">
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">Initial Buy SOL</span>
            <Input type="number" fieldSize="md" step="0.001" value={form.initialBuy} readOnly={locked}
              onChange={(e) => set({ initialBuy: e.target.value })} className={fieldCls()} />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">Tolerance %</span>
            <Input type="number" fieldSize="md" step="0.1" value={form.tolerance} readOnly={locked}
              onChange={(e) => set({ tolerance: e.target.value })} className={fieldCls()} />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">CU Limit</span>
            <Input type="number" fieldSize="md" value={form.cuLimit} readOnly={locked}
              onChange={(e) => set({ cuLimit: e.target.value })} className={fieldCls()} />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">CU Price</span>
            <Input type="number" fieldSize="md" value={form.cuPrice} readOnly={locked}
              onChange={(e) => set({ cuPrice: e.target.value })} className={fieldCls()} />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">Max SOL Cost</span>
            <Input type="number" fieldSize="md" step="0.001" value={form.maxSolCost} readOnly={locked}
              onChange={(e) => set({ maxSolCost: e.target.value })} className={fieldCls()} />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">Spendable SOL In</span>
            <Input type="number" fieldSize="md" step="0.001" value={form.spendableSolIn} readOnly={locked}
              onChange={(e) => set({ spendableSolIn: e.target.value })} className={fieldCls()} />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">Max Holding Tokens</span>
            <Input type="number" fieldSize="md" value={form.maxHoldingTokens} readOnly={locked}
              onChange={(e) => set({ maxHoldingTokens: e.target.value })} className={fieldCls()} />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">Total Max Trade Tokens</span>
            <Input type="number" fieldSize="md" value={form.totalMaxTradeTokens} readOnly={locked}
              onChange={(e) => set({ totalMaxTradeTokens: e.target.value })} className={fieldCls()} />
          </label>
        </div>

        <div className="flex flex-col gap-1.5">
          <div className="flex items-center justify-between">
            <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
              Instruction Labels
            </span>
            <div className="flex gap-1">
              {isEdit && (
                <button
                  type="button"
                  onClick={() => setAllowEditParams((v) => !v)}
                  className="rounded-lg border border-white/12 bg-white/4 px-2 py-1 text-xs"
                  title={allowEditParams ? 'Lock criteria' : 'Unlock criteria'}
                >
                  {allowEditParams ? '🔒' : '🔓'}
                </button>
              )}
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

        <div className="grid grid-cols-3 gap-3">
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-primary">Buy Amount (SOL)</span>
            <Input type="number" fieldSize="md" step="0.001" value={form.buyAmount}
              onChange={(e) => set({ buyAmount: e.target.value })} className={fieldCls()} />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-primary">Take Profit %</span>
            <Input type="number" fieldSize="md" step="1" value={form.takeProfit}
              onChange={(e) => set({ takeProfit: e.target.value })} className={fieldCls('focus:border-green')} />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-primary">Stop Loss %</span>
            <Input type="number" fieldSize="md" step="1" value={form.stopLoss}
              onChange={(e) => set({ stopLoss: e.target.value })} className={fieldCls('focus:border-red')} />
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
    p_initial_buy_sol: parseOptF(form.initialBuy) ?? null,
    p_cu_limit: parseOptU(form.cuLimit) ?? null,
    p_cu_price: parseOptU(form.cuPrice) ?? null,
    p_max_sol_cost: parseOptF(form.maxSolCost) ?? null,
    p_spendable_sol_in: parseOptF(form.spendableSolIn) ?? null,
    p_max_holding_tokens: parseOptU(form.maxHoldingTokens) ?? null,
    p_total_max_trade_tokens: parseOptU(form.totalMaxTradeTokens) ?? null,
    p_ix_labels: parseIxLabels(form.ixLabels),
    trade_mode: form.tradeMode,
    buy_amount: parseFloat(form.buyAmount),
    take_profit: parseFloat(form.takeProfit),
    stop_loss: parseFloat(form.stopLoss),
    tolerance_pct: form.tolerance.trim() ? parseFloat(form.tolerance) : null,
  };
}

export function buildUpdatePayload(form: RuleFormData, allowParams: boolean) {
  const base: Record<string, unknown> = {
    rule_name: form.ruleName,
    buy_amount: parseFloat(form.buyAmount),
    take_profit: parseFloat(form.takeProfit),
    stop_loss: parseFloat(form.stopLoss),
    trade_mode: form.tradeMode,
    tolerance_pct: form.tolerance.trim() ? parseFloat(form.tolerance) : undefined,
  };
  if (!allowParams) return base;
  return {
    ...base,
    p_initial_buy_sol: form.initialBuy.trim() ? parseFloat(form.initialBuy) : 0,
    p_cu_limit: form.cuLimit.trim() ? parseInt(form.cuLimit, 10) : 0,
    p_cu_price: form.cuPrice.trim() ? parseInt(form.cuPrice, 10) : 0,
    p_max_sol_cost: form.maxSolCost.trim() ? parseFloat(form.maxSolCost) : 0,
    p_spendable_sol_in: form.spendableSolIn.trim() ? parseFloat(form.spendableSolIn) : 0,
    p_max_holding_tokens: form.maxHoldingTokens.trim()
      ? parseInt(form.maxHoldingTokens, 10)
      : 0,
    p_total_max_trade_tokens: form.totalMaxTradeTokens.trim()
      ? parseInt(form.totalMaxTradeTokens, 10)
      : 0,
    p_ix_labels: parseIxLabels(form.ixLabels),
  };
}
