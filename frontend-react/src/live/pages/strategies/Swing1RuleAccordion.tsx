import { useEffect, useState, type ReactNode } from 'react';
import type { RuleRecord } from 'types';
import { Button } from 'components/ui/Button';
import { Input, Textarea } from 'components/ui/Input';
import { Modal, InlineAlert } from 'components/ui/Modal';
import { Select } from 'components/ui/Select';
import { Accordion } from 'components/ui/Accordion';
import { cn } from 'lib/cn';
import { EXAMPLE_IX_LABELS, parseIxLabels } from '@shared/components/tpsl2/utils';
import {
  SWING1_AXES,
  SWING1_INT_KEYS,
  SWING1_REQUIRED_KEYS,
  groupAxesBySubgroup,
  swing1AxisCol,
  type AxisDef,
  type AxisSubgroup,
} from '@shared/lib/swing1Axes';
import { PasteParamsSection } from 'components/strategy/PasteParamsSection';
import { applyBlobToSwing1Form } from 'lib/ruleParams';

/** Lock groups, aligned 1:1 with the accordion's sections. Each param subgroup is
 *  its own lock group; `sizing` covers the universal buy-amount + concurrency
 *  knobs. When editing, every group is locked by default (🔓 to unlock). `sizing`
 *  is the ONLY group that stays unlockable while the rule is live — the other
 *  groups shape entry/exit and are frozen until the rule is idle. */
export type LockGroup = 'fingerprint' | 'sizing' | AxisSubgroup;
export type LockGroupState = Record<LockGroup, boolean>;

function freshLocks(): LockGroupState {
  return {
    fingerprint: false,
    sizing: false,
    swing: false,
    kill: false,
    volume: false,
    confirm: false,
    ladder: false,
    'next-kill': false,
  };
}

// The axis⇄column map (`swing1AxisCol`), the required-axis set
// (`SWING1_REQUIRED_KEYS`), and the integer-axis set (`SWING1_INT_KEYS`) now live
// in `@shared/lib/swing1Axes` — shared with the params copy/paste round-trip so
// both sides map axis keys to the same `p_*` columns and parse them identically.
const axisCol = swing1AxisCol;
const REQUIRED_KEYS = SWING1_REQUIRED_KEYS;
const INT_KEYS = SWING1_INT_KEYS;

/** Universal (non-axis) form fields plus one string entry per swing1 axis, keyed
 *  by the axis `key`. String-typed like the tpsl2 form so blanks are preserved. */
export interface RuleFormData {
  ruleName: string;
  tradeMode: string;
  buyAmount: string;
  maxConcurrentTokens: string;
  maxTotalTokens: string;
  // Optional dev-fingerprint pre-filter (blank ⇒ unset; the swing latch is the
  // real gate). `tolerance` is the shared band for the SOL-valued knobs.
  initialBuy: string;
  tolerance: string;
  cuLimit: string;
  cuPrice: string;
  maxSolCost: string;
  spendableSolIn: string;
  ixLabels: string;
  /** One value per SWING1_AXES entry, keyed by axis `key`. */
  axes: Record<string, string>;
}

function emptyAxes(): Record<string, string> {
  const out: Record<string, string> = {};
  for (const a of SWING1_AXES) out[a.key] = '';
  return out;
}

export function emptyForm(): RuleFormData {
  return {
    ruleName: '',
    tradeMode: 'paper',
    buyAmount: '',
    maxConcurrentTokens: '',
    maxTotalTokens: '',
    initialBuy: '',
    tolerance: '0',
    cuLimit: '',
    cuPrice: '',
    maxSolCost: '',
    spendableSolIn: '',
    ixLabels: '',
    axes: emptyAxes(),
  };
}

export function formFromRule(rule: RuleRecord): RuleFormData {
  const axes: Record<string, string> = {};
  for (const a of SWING1_AXES) {
    const col = axisCol(a.key) as keyof RuleRecord;
    const v = rule[col] as number | null | undefined;
    axes[a.key] = v == null ? '' : v.toString();
  }
  const labels = Array.isArray(rule.p_token_ix_labels)
    ? JSON.stringify(rule.p_token_ix_labels)
    : '';
  return {
    ruleName: rule.rule_name,
    tradeMode: rule.trade_mode,
    buyAmount: rule.buy_amount.toString(),
    maxConcurrentTokens: rule.p_max_concurrent_tokens?.toString() ?? '',
    maxTotalTokens: rule.p_max_total_tokens?.toString() ?? '',
    initialBuy: rule.p_token_initial_buy_sol?.toString() ?? '',
    tolerance: rule.tolerance_pct.toString(),
    cuLimit: rule.p_token_cu_limit?.toString() ?? '',
    cuPrice: rule.p_token_cu_price?.toString() ?? '',
    maxSolCost: rule.p_token_max_sol_cost?.toString() ?? '',
    spendableSolIn: rule.p_token_spendable_sol_in?.toString() ?? '',
    ixLabels: labels,
    axes,
  };
}

const parseAxis = (key: string, s: string): number | null => {
  if (!s.trim()) return null;
  return INT_KEYS.has(key) ? parseInt(s, 10) : parseFloat(s);
};

/** Full create body — every axis param plus the universals. The token-fingerprint
 *  knobs are an **optional** pre-filter (blank ⇒ null ⇒ unset; the swing latch is
 *  the real gate); `p_token_ix_labels` parses to a JSON array. */
export function buildCreatePayload(form: RuleFormData): Record<string, unknown> {
  const parseOptU = (s: string) => (s.trim() ? parseInt(s, 10) : null);
  const parseOptF = (s: string) => (s.trim() ? parseFloat(s) : null);
  const payload: Record<string, unknown> = {
    rule_name: form.ruleName,
    trade_mode: form.tradeMode,
    buy_amount: parseFloat(form.buyAmount),
    p_max_concurrent_tokens: parseOptU(form.maxConcurrentTokens),
    p_max_total_tokens: parseOptU(form.maxTotalTokens),
    // Optional dev-fingerprint pre-filter.
    p_token_initial_buy_sol: parseOptF(form.initialBuy),
    p_token_cu_limit: parseOptU(form.cuLimit),
    p_token_cu_price: parseOptU(form.cuPrice),
    p_token_max_sol_cost: parseOptF(form.maxSolCost),
    p_token_spendable_sol_in: parseOptF(form.spendableSolIn),
    p_token_ix_labels: parseIxLabels(form.ixLabels),
    tolerance_pct: parseOptF(form.tolerance) ?? 0,
  };
  for (const a of SWING1_AXES) {
    payload[axisCol(a.key)] = parseAxis(a.key, form.axes[a.key] ?? '');
  }
  return payload;
}

/** PUT body from only the UNLOCKED groups (a locked group sends no keys, so the
 *  backend's Option-merge leaves it untouched). `rule_name` + `trade_mode` are
 *  administrative and always sent. Within an unlocked group, blank → 0 (disables
 *  per the ignore_zero convention); required TP/SL keep their parsed value. */
export function buildUpdatePayload(
  form: RuleFormData,
  unlocked: LockGroupState,
): Record<string, unknown> {
  const payload: Record<string, unknown> = {
    rule_name: form.ruleName,
    trade_mode: form.tradeMode,
  };
  if (unlocked.sizing) {
    payload.buy_amount = parseFloat(form.buyAmount);
    payload.p_max_concurrent_tokens = form.maxConcurrentTokens.trim()
      ? parseInt(form.maxConcurrentTokens, 10)
      : 0;
    payload.p_max_total_tokens = form.maxTotalTokens.trim()
      ? parseInt(form.maxTotalTokens, 10)
      : 0;
  }
  if (unlocked.fingerprint) {
    // Blank ⇒ 0 (the ignore_zero convention disables that criterion).
    const u = (s: string) => (s.trim() ? parseInt(s, 10) : 0);
    const f = (s: string) => (s.trim() ? parseFloat(s) : 0);
    payload.p_token_initial_buy_sol = f(form.initialBuy);
    payload.p_token_cu_limit = u(form.cuLimit);
    payload.p_token_cu_price = u(form.cuPrice);
    payload.p_token_max_sol_cost = f(form.maxSolCost);
    payload.p_token_spendable_sol_in = f(form.spendableSolIn);
    payload.p_token_ix_labels = parseIxLabels(form.ixLabels);
    payload.tolerance_pct = f(form.tolerance);
  }
  for (const a of SWING1_AXES) {
    const g = a.subgroup;
    if (!g || !unlocked[g]) continue;
    const raw = form.axes[a.key] ?? '';
    if (REQUIRED_KEYS.has(a.key)) {
      payload[axisCol(a.key)] = parseFloat(raw);
    } else {
      payload[axisCol(a.key)] = raw.trim()
        ? INT_KEYS.has(a.key)
          ? parseInt(raw, 10)
          : parseFloat(raw)
        : 0;
    }
  }
  return payload;
}

interface Swing1RuleAccordionProps {
  open: boolean;
  editRule: RuleRecord | null;
  loading: boolean;
  error: string | null;
  form: RuleFormData;
  onChange: (form: RuleFormData) => void;
  onClose: () => void;
  onSave: (unlocked: LockGroupState) => void;
}

export function Swing1RuleAccordion({
  open,
  editRule,
  loading,
  error,
  form,
  onChange,
  onClose,
  onSave,
}: Swing1RuleAccordionProps) {
  const isEdit = editRule != null;
  // A live rule (running, or still draining open positions) freezes everything
  // except `sizing`, so its detection/entry/exit shape can't shift mid-run.
  const live = isEdit && (editRule.is_active || editRule.open_positions > 0);
  const [unlocked, setUnlocked] = useState<LockGroupState>(freshLocks);

  // Every group starts LOCKED on each (re)open — an unlock from a prior edit never
  // carries over.
  useEffect(() => {
    if (open) setUnlocked(freshLocks());
  }, [open]);

  const set = (patch: Partial<RuleFormData>) => onChange({ ...form, ...patch });
  const setAxis = (key: string, value: string) =>
    onChange({ ...form, axes: { ...form.axes, [key]: value } });

  // Create mode has no lock concept: every field is freely editable.
  const editable = (g: LockGroup) => !isEdit || unlocked[g];
  const canUnlock = (g: LockGroup) => !live || g === 'sizing';

  const fieldCls = (g: LockGroup, extra?: string) =>
    cn('font-mono', !editable(g) && 'cursor-not-allowed opacity-50', extra);

  /** Per-section 🔓/🔒 toggle. Only rendered when editing; disabled when the group
   *  is frozen because the rule is live. */
  const lockToggle = (g: LockGroup): ReactNode =>
    isEdit ? (
      <button
        type="button"
        onClick={() => canUnlock(g) && setUnlocked((u) => ({ ...u, [g]: !u[g] }))}
        disabled={!canUnlock(g)}
        className="rounded-lg border border-white/12 bg-white/4 px-2 py-1 text-xs disabled:cursor-not-allowed disabled:opacity-40"
        title={
          !canUnlock(g)
            ? 'Locked while the rule is running or holding positions'
            : unlocked[g]
              ? 'Lock to prevent edits'
              : 'Unlock to edit'
        }
      >
        {unlocked[g] ? '🔒' : '🔓'}
      </button>
    ) : undefined;

  const buckets = groupAxesBySubgroup(SWING1_AXES);

  const renderAxis = (a: AxisDef, g: LockGroup) => (
    <label key={a.key} className="flex flex-col gap-1.5">
      <span className="inline-flex items-center gap-1 text-[10px] font-bold uppercase tracking-wider text-text-dim">
        {a.label}
      </span>
      <Input
        type="number"
        fieldSize="md"
        value={form.axes[a.key] ?? ''}
        readOnly={!editable(g)}
        blankZero={!REQUIRED_KEYS.has(a.key)}
        onChange={(e) => setAxis(a.key, e.target.value)}
        className={fieldCls(g)}
        placeholder={a.nullable ? '0 = off' : undefined}
        title={a.desc}
      />
    </label>
  );

  return (
    <Modal
      title={isEdit ? 'Edit Swing 1 Rule' : 'New Swing 1 Rule'}
      open={open}
      onClose={onClose}
    >
      <div className="flex flex-col gap-4">
        {/* ── Universals (outside the accordion) ── */}
        <div className="grid grid-cols-2 gap-3">
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-primary">Mode</span>
            <Select
              fieldSize="md"
              value={form.tradeMode}
              onChange={(e) => set({ tradeMode: e.target.value })}
              disabled={live}
              className={cn('font-mono', live && 'cursor-not-allowed opacity-50')}
            >
              <option value="paper">Paper Test</option>
            </Select>
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-primary">Rule Name</span>
            <Input
              type="text"
              fieldSize="md"
              value={form.ruleName}
              onChange={(e) => set({ ruleName: e.target.value })}
              placeholder="e.g. Swing 0.5 SOL"
              className="font-mono"
            />
          </label>
        </div>

        {/* ── Paste params (rule ⎘ / sweep combo ⎘ / detect ⎘ → this form) ── */}
        <PasteParamsSection
          strategy="swing_1"
          live={live}
          onApply={(blob, mode) => {
            // Split the typed form into its universal fields (everything except
            // `axes`) and the axis map — the shared apply routes blob columns to
            // one half or the other, then we reassemble the typed form.
            const { axes: curAxes, ...universals } = form;
            const { universals: nextU, axes: nextA, result } = applyBlobToSwing1Form(
              universals as unknown as Record<string, string>,
              () => {
                const { axes: _a, ...u } = emptyForm();
                void _a;
                return u as unknown as Record<string, string>;
              },
              curAxes,
              emptyAxes,
              blob,
              live,
              mode,
            );
            onChange({ ...(nextU as unknown as Omit<RuleFormData, 'axes'>), axes: nextA });
            return result;
          }}
        />

        {/* ── Sizing & limits (universal · editable while live) ── */}
        <div className="flex items-center justify-between border-t border-white/10 pt-3">
          <div className="flex items-baseline gap-2">
            <span className="text-[11px] font-bold uppercase tracking-wider text-text-dim">Sizing &amp; Limits</span>
            <span className="text-[10px] lowercase text-text-dim">position size + concurrency · editable while live</span>
          </div>
          {lockToggle('sizing')}
        </div>
        <div className="grid grid-cols-3 gap-3">
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-primary">Buy Amount (SOL)</span>
            <Input type="number" fieldSize="md" step="0.001" unit="◎" value={form.buyAmount} readOnly={!editable('sizing')}
              onChange={(e) => set({ buyAmount: e.target.value })} className={fieldCls('sizing')} />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">Max Concurrent Tokens</span>
            <Input type="number" fieldSize="md" value={form.maxConcurrentTokens} readOnly={!editable('sizing')}
              onChange={(e) => set({ maxConcurrentTokens: e.target.value })} className={fieldCls('sizing')} />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">Max Total Tokens</span>
            <Input type="number" fieldSize="md" value={form.maxTotalTokens} readOnly={!editable('sizing')}
              onChange={(e) => set({ maxTotalTokens: e.target.value })} className={fieldCls('sizing')} />
          </label>
        </div>

        {/* ── Token fingerprint (optional pre-filter · p_token_*) ── */}
        <div className="flex items-center justify-between border-t border-white/10 pt-3">
          <div className="flex items-baseline gap-2">
            <span className="text-[11px] font-bold uppercase tracking-wider text-info">Token Fingerprint</span>
            <span className="text-[10px] lowercase text-text-dim">optional · which token to match · blank = off</span>
          </div>
          {lockToggle('fingerprint')}
        </div>
        <div className="grid grid-cols-6 gap-3 items-end">
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">Initial Buy SOL</span>
            <Input type="number" fieldSize="md" step="0.001" unit="◎" value={form.initialBuy} readOnly={!editable('fingerprint')} blankZero
              onChange={(e) => set({ initialBuy: e.target.value })} className={fieldCls('fingerprint')} />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">Tolerance %</span>
            <Input type="number" fieldSize="md" step="0.1" min={0} max={100} unit="%" value={form.tolerance} readOnly={!editable('fingerprint')} blankZero
              onChange={(e) => set({ tolerance: e.target.value })} className={fieldCls('fingerprint')} />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">CU Limit</span>
            <Input type="number" fieldSize="md" value={form.cuLimit} readOnly={!editable('fingerprint')} blankZero
              onChange={(e) => set({ cuLimit: e.target.value })} className={fieldCls('fingerprint')} />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">CU Price</span>
            <Input type="number" fieldSize="md" value={form.cuPrice} readOnly={!editable('fingerprint')} blankZero
              onChange={(e) => set({ cuPrice: e.target.value })} className={fieldCls('fingerprint')} />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">Max SOL Cost</span>
            <Input type="number" fieldSize="md" step="0.001" unit="◎" value={form.maxSolCost} readOnly={!editable('fingerprint')} blankZero
              onChange={(e) => set({ maxSolCost: e.target.value })} className={fieldCls('fingerprint')} />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">Spendable SOL In</span>
            <Input type="number" fieldSize="md" step="0.001" unit="◎" value={form.spendableSolIn} readOnly={!editable('fingerprint')} blankZero
              onChange={(e) => set({ spendableSolIn: e.target.value })} className={fieldCls('fingerprint')} />
          </label>
        </div>
        <label className="flex flex-col gap-1.5">
          <div className="flex items-center justify-between">
            <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">Instruction Labels</span>
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
            readOnly={!editable('fingerprint')}
            onChange={(e) => set({ ixLabels: e.target.value })}
            placeholder='blank = off · e.g. ["CreateIdempotent","Create","ExtendAccount","Buy"]'
            className={fieldCls('fingerprint', 'text-xs')}
          />
        </label>

        {/* ── Param subgroups — one accordion section each, pipeline order ── */}
        <div className="flex flex-col gap-2">
          {buckets.map(({ meta, axes }) => {
            if (!meta) return null;
            const g = meta.key as LockGroup;
            return (
              <Accordion
                key={meta.key}
                padding="sm"
                header={
                  <div className="flex min-w-0 flex-1 items-center justify-between gap-2">
                    <div className="flex items-baseline gap-2">
                      <span className={cn('text-[11px] font-bold uppercase tracking-wider', meta.accent)}>{meta.label}</span>
                      <span className="text-[10px] lowercase text-text-dim">{meta.hint}</span>
                    </div>
                    {lockToggle(g)}
                  </div>
                }
              >
                <div className="grid grid-cols-4 gap-3">
                  {axes.map((a) => renderAxis(a, g))}
                </div>
              </Accordion>
            );
          })}
        </div>

        {error && <InlineAlert variant="error">{error}</InlineAlert>}

        <div className="flex justify-end gap-2.5">
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" onClick={() => onSave(unlocked)} disabled={loading}>
            {loading ? 'Saving…' : 'Save Rule'}
          </Button>
        </div>
      </div>
    </Modal>
  );
}
