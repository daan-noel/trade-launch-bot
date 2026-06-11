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
  // Scalp-continuation gates (0/blank = disabled).
  minAgeSecs: string;
  minAliveSol: string;
  minOrganicSol: string;
  pullbackPct: string;
  higherLowSecs: string;
  maxCohortHeld: string;
  minLiquiditySol: string;
  minOrganicLiq: string;
  cohortExitRatio: string;
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
    minAgeSecs: '',
    minAliveSol: '',
    minOrganicSol: '',
    pullbackPct: '',
    higherLowSecs: '',
    maxCohortHeld: '',
    minLiquiditySol: '',
    minOrganicLiq: '',
    cohortExitRatio: '',
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
    maxConcurrentTokens: rule.p_max_concurrent_tokens?.toString() ?? '',
    maxTotalTokens: rule.p_max_total_tokens?.toString() ?? '',
    ixLabels: labels,
    buyAmount: rule.buy_amount.toString(),
    takeProfit: rule.take_profit.toString(),
    stopLoss: rule.stop_loss.toString(),
    trailingStopPct: rule.p_trailing_stop_pct?.toString() ?? '',
    timeStopSecs: rule.p_time_stop_secs?.toString() ?? '',
    stallSecs: rule.p_stall_secs?.toString() ?? '',
    liquidityDropPct: rule.p_liquidity_drop_pct?.toString() ?? '',
    minAgeSecs: rule.p_min_age_secs?.toString() ?? '',
    minAliveSol: rule.p_min_alive_sol?.toString() ?? '',
    minOrganicSol: rule.p_min_organic_sol?.toString() ?? '',
    pullbackPct: rule.p_pullback_pct?.toString() ?? '',
    higherLowSecs: rule.p_higher_low_secs?.toString() ?? '',
    maxCohortHeld: rule.p_max_cohort_held?.toString() ?? '',
    minLiquiditySol: rule.p_min_liquidity_sol?.toString() ?? '',
    minOrganicLiq: rule.p_min_organic_liq?.toString() ?? '',
    cohortExitRatio: rule.p_cohort_exit_ratio?.toString() ?? '',
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
    <Modal title={isEdit ? 'Edit TPSL2 Rule' : 'New TPSL2 Rule'} open={open} onClose={onClose}>
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
            <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">Max Concurrent Tokens</span>
            <Input type="number" fieldSize="md" value={form.maxConcurrentTokens} readOnly={locked}
              onChange={(e) => set({ maxConcurrentTokens: e.target.value })} className={fieldCls()} />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">Max Total Tokens</span>
            <Input type="number" fieldSize="md" value={form.maxTotalTokens} readOnly={locked}
              onChange={(e) => set({ maxTotalTokens: e.target.value })} className={fieldCls()} />
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
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-primary">Trailing Stop %</span>
            <Input type="number" fieldSize="md" step="1" value={form.trailingStopPct}
              onChange={(e) => set({ trailingStopPct: e.target.value })}
              className={fieldCls('focus:border-warning')} placeholder="0 = off" />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-primary">Time Stop (s)</span>
            <Input type="number" fieldSize="md" step="1" value={form.timeStopSecs}
              onChange={(e) => set({ timeStopSecs: e.target.value })}
              className={fieldCls('focus:border-info')} placeholder="0 = off" />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-primary">Stall (s)</span>
            <Input type="number" fieldSize="md" step="1" value={form.stallSecs}
              onChange={(e) => set({ stallSecs: e.target.value })}
              className={fieldCls('focus:border-accent')} placeholder="0 = off" />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-primary">Liquidity Drop %</span>
            <Input type="number" fieldSize="md" step="1" value={form.liquidityDropPct}
              onChange={(e) => set({ liquidityDropPct: e.target.value })}
              className={fieldCls('focus:border-primary')} placeholder="0 = off" />
          </label>
        </div>

        {/* Scalp-continuation gates (tpsl2 only). All inert at blank/0; entry
            gates decide the buy on the trade stream, cohort-dump is exit E5. */}
        <div className="flex flex-col gap-2">
          <span className="text-[10px] font-bold uppercase tracking-wider text-accent">
            Scalp Gates · entry shape + cohort
          </span>
          <div className="grid grid-cols-3 gap-3">
            <label className="flex flex-col gap-1.5">
              <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">Min Age (s)</span>
              <Input type="number" fieldSize="md" step="1" value={form.minAgeSecs}
                onChange={(e) => set({ minAgeSecs: e.target.value })} className={fieldCls()} placeholder="0 = off" />
            </label>
            <label className="flex flex-col gap-1.5">
              <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">Min Alive SOL</span>
              <Input type="number" fieldSize="md" step="0.01" value={form.minAliveSol}
                onChange={(e) => set({ minAliveSol: e.target.value })} className={fieldCls()} placeholder="0 = off" />
            </label>
            <label className="flex flex-col gap-1.5">
              <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">Min Organic SOL</span>
              <Input type="number" fieldSize="md" step="0.01" value={form.minOrganicSol}
                onChange={(e) => set({ minOrganicSol: e.target.value })} className={fieldCls()} placeholder="0 = off" />
            </label>
            <label className="flex flex-col gap-1.5">
              <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">Pullback %</span>
              <Input type="number" fieldSize="md" step="1" value={form.pullbackPct}
                onChange={(e) => set({ pullbackPct: e.target.value })} className={fieldCls()} placeholder="0 = off" />
            </label>
            <label className="flex flex-col gap-1.5">
              <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">Higher-Low (s)</span>
              <Input type="number" fieldSize="md" step="1" value={form.higherLowSecs}
                onChange={(e) => set({ higherLowSecs: e.target.value })} className={fieldCls()} placeholder="0 = off" />
            </label>
            <label className="flex flex-col gap-1.5">
              <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">Max Cohort Held</span>
              <Input type="number" fieldSize="md" step="0.05" value={form.maxCohortHeld}
                onChange={(e) => set({ maxCohortHeld: e.target.value })} className={fieldCls()} placeholder="0 = off" />
            </label>
            <label className="flex flex-col gap-1.5">
              <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">Min Liquidity SOL</span>
              <Input type="number" fieldSize="md" step="0.1" value={form.minLiquiditySol}
                onChange={(e) => set({ minLiquiditySol: e.target.value })} className={fieldCls()} placeholder="0 = off" />
            </label>
            <label className="flex flex-col gap-1.5">
              <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">Min Organic Liq</span>
              <Input type="number" fieldSize="md" step="0.1" value={form.minOrganicLiq}
                onChange={(e) => set({ minOrganicLiq: e.target.value })} className={fieldCls()} placeholder="0 = off" />
            </label>
            <label className="flex flex-col gap-1.5">
              <span className="text-[10px] font-bold uppercase tracking-wider text-primary">Cohort Exit Ratio</span>
              <Input type="number" fieldSize="md" step="0.01" value={form.cohortExitRatio}
                onChange={(e) => set({ cohortExitRatio: e.target.value })}
                className={fieldCls('focus:border-red')} placeholder="0 = off" />
            </label>
          </div>
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
    p_max_concurrent_tokens: parseOptU(form.maxConcurrentTokens) ?? null,
    p_max_total_tokens: parseOptU(form.maxTotalTokens) ?? null,
    p_ix_labels: parseIxLabels(form.ixLabels),
    trade_mode: form.tradeMode,
    buy_amount: parseFloat(form.buyAmount),
    take_profit: parseFloat(form.takeProfit),
    stop_loss: parseFloat(form.stopLoss),
    p_trailing_stop_pct: parseOptF(form.trailingStopPct) ?? null,
    p_time_stop_secs: parseOptU(form.timeStopSecs) ?? null,
    p_stall_secs: parseOptU(form.stallSecs) ?? null,
    p_liquidity_drop_pct: parseOptF(form.liquidityDropPct) ?? null,
    // Scalp-continuation gates.
    p_min_age_secs: parseOptU(form.minAgeSecs) ?? null,
    p_min_alive_sol: parseOptF(form.minAliveSol) ?? null,
    p_min_organic_sol: parseOptF(form.minOrganicSol) ?? null,
    p_pullback_pct: parseOptF(form.pullbackPct) ?? null,
    p_higher_low_secs: parseOptU(form.higherLowSecs) ?? null,
    p_max_cohort_held: parseOptF(form.maxCohortHeld) ?? null,
    p_min_liquidity_sol: parseOptF(form.minLiquiditySol) ?? null,
    p_min_organic_liq: parseOptF(form.minOrganicLiq) ?? null,
    p_cohort_exit_ratio: parseOptF(form.cohortExitRatio) ?? null,
    tolerance_pct: form.tolerance.trim() ? parseFloat(form.tolerance) : null,
  };
}

export function buildUpdatePayload(form: RuleFormData, allowParams: boolean) {
  const base: Record<string, unknown> = {
    rule_name: form.ruleName,
    buy_amount: parseFloat(form.buyAmount),
    take_profit: parseFloat(form.takeProfit),
    stop_loss: parseFloat(form.stopLoss),
    // Exit params (always editable): 0 disables, per the ignore_zero convention.
    p_trailing_stop_pct: form.trailingStopPct.trim() ? parseFloat(form.trailingStopPct) : 0,
    p_time_stop_secs: form.timeStopSecs.trim() ? parseInt(form.timeStopSecs, 10) : 0,
    p_stall_secs: form.stallSecs.trim() ? parseInt(form.stallSecs, 10) : 0,
    p_liquidity_drop_pct: form.liquidityDropPct.trim() ? parseFloat(form.liquidityDropPct) : 0,
    // Scalp-continuation gates (always editable; 0 disables, per ignore_zero).
    p_min_age_secs: form.minAgeSecs.trim() ? parseInt(form.minAgeSecs, 10) : 0,
    p_min_alive_sol: form.minAliveSol.trim() ? parseFloat(form.minAliveSol) : 0,
    p_min_organic_sol: form.minOrganicSol.trim() ? parseFloat(form.minOrganicSol) : 0,
    p_pullback_pct: form.pullbackPct.trim() ? parseFloat(form.pullbackPct) : 0,
    p_higher_low_secs: form.higherLowSecs.trim() ? parseInt(form.higherLowSecs, 10) : 0,
    p_max_cohort_held: form.maxCohortHeld.trim() ? parseFloat(form.maxCohortHeld) : 0,
    p_min_liquidity_sol: form.minLiquiditySol.trim() ? parseFloat(form.minLiquiditySol) : 0,
    p_min_organic_liq: form.minOrganicLiq.trim() ? parseFloat(form.minOrganicLiq) : 0,
    p_cohort_exit_ratio: form.cohortExitRatio.trim() ? parseFloat(form.cohortExitRatio) : 0,
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
    p_max_concurrent_tokens: form.maxConcurrentTokens.trim()
      ? parseInt(form.maxConcurrentTokens, 10)
      : 0,
    p_max_total_tokens: form.maxTotalTokens.trim()
      ? parseInt(form.maxTotalTokens, 10)
      : 0,
    p_ix_labels: parseIxLabels(form.ixLabels),
  };
}
