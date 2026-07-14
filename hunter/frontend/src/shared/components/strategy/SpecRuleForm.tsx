import { useEffect, useState, type ReactNode } from 'react';
import type { RuleRecord } from 'types';
import { Button } from 'components/ui/Button';
import { Input, Textarea } from 'components/ui/Input';
import { InlineAlert } from 'components/ui/Modal';
import { Select } from 'components/ui/Select';
import { Accordion } from 'components/ui/Accordion';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { BucketChip } from 'components/ui/BucketChip';
import { TPSL_PARAM_HELP, type TpslParamKey } from 'lib/tpslParamHelp';
import { cn } from 'lib/cn';
import { EXAMPLE_IX_LABELS } from 'components/strategy/cellFormat';
import { PasteParamsSection } from 'components/strategy/PasteParamsSection';
import {
  applyBlob,
  freshLocks,
  isBucketedColumn,
  RULE_NAME_KEY,
  TRADE_MODE_KEY,
  type FormState,
  type LockState,
  type ParamField,
  type SpecSection,
  type StrategySpec,
} from 'lib/params';

/** One spec-driven rule form — inline on the page (NOT a modal), the same chrome
 *  for all 3 strategies. Renders the always-visible Mode + Rule Name row, the
 *  paste-params section, then one collapsible {@link Accordion} per
 *  `spec.sections`, each a labelled field grid. All field behavior (kind, unit,
 *  step, required/nullable, lock group, tooltip) is read off the {@link ParamField}.
 *
 *  Lock semantics (unchanged): every section starts LOCKED when editing; only the
 *  `liveEditable` section (sizing) is unlockable while the rule is running. A locked
 *  section contributes no keys on save (the caller's `buildUpdatePayload` honors the
 *  returned `unlocked` map). Create mode has no locks — every field editable. */
export interface SpecRuleFormProps {
  spec: StrategySpec;
  /** Whether the form is shown. Rendered inline; parent controls placement. */
  open: boolean;
  editRule: RuleRecord | null;
  loading: boolean;
  error: string | null;
  form: FormState;
  onChange: (form: FormState) => void;
  onClose: () => void;
  onSave: (unlocked: LockState) => void;
}

/** Field label + optional ⓘ tooltip (tpsl `helpKey`) or native `title` (swing1
 *  `help`). Matches the old modals' `FieldLabel` styling. Bucketed fingerprint
 *  fields also carry the shared {@link BucketChip} so it's explicit they match by
 *  range — the same marker the grouped-sweep picker shows. */
function FieldLabel({ field, accent }: { field: ParamField; accent?: string }) {
  const cls = cn(
    'inline-flex items-center gap-1 text-[10px] font-bold uppercase tracking-wider',
    accent ?? 'text-text-dim',
  );
  const bucketed = isBucketedColumn(field.column);
  if (field.helpKey && field.helpKey in TPSL_PARAM_HELP) {
    const h = TPSL_PARAM_HELP[field.helpKey as TpslParamKey];
    return (
      <span className={cls}>
        {field.label}
        {bucketed && <BucketChip />}
        <InfoTooltip title={h.title} body={h.body} />
      </span>
    );
  }
  return (
    <span className={cls} title={field.help}>
      {field.label}
      {bucketed && <BucketChip />}
    </span>
  );
}

export function SpecRuleForm({
  spec,
  open,
  editRule,
  loading,
  error,
  form,
  onChange,
  onClose,
  onSave,
}: SpecRuleFormProps) {
  const isEdit = editRule != null;
  // A live rule (running, or still draining open positions) freezes everything
  // except the `liveEditable` section, so its match/entry/exit shape can't shift
  // mid-run.
  const live = isEdit && (editRule.is_active || editRule.open_positions > 0);
  const [unlocked, setUnlocked] = useState<LockState>(() => freshLocks(spec));

  // Every section starts LOCKED on each (re)open — an unlock from a prior edit (or
  // a just-saved rule) never carries over. Keyed on `open` so re-opening resets.
  useEffect(() => {
    if (open) setUnlocked(freshLocks(spec));
  }, [open, spec]);

  if (!open) return null;

  const set = (col: string, value: string) => onChange({ ...form, [col]: value });

  // Create mode has no lock concept: every field is freely editable.
  const editable = (s: SpecSection) => !isEdit || unlocked[s.key];
  const canUnlock = (s: SpecSection) => !live || s.liveEditable;

  const fieldCls = (s: SpecSection, extra?: string) =>
    cn('font-mono', !editable(s) && 'cursor-not-allowed opacity-50', extra);

  /** Per-section 🔓/🔒 toggle. Only when editing; disabled when frozen (live). */
  const lockToggle = (s: SpecSection): ReactNode =>
    isEdit ? (
      <button
        type="button"
        onClick={() => canUnlock(s) && setUnlocked((u) => ({ ...u, [s.key]: !u[s.key] }))}
        disabled={!canUnlock(s)}
        className="rounded-lg border border-white/12 bg-white/4 px-2 py-1 text-xs disabled:cursor-not-allowed disabled:opacity-40"
        title={
          !canUnlock(s)
            ? 'Locked while the rule is running or holding positions'
            : unlocked[s.key]
              ? 'Lock to prevent edits'
              : 'Unlock to edit'
        }
      >
        {unlocked[s.key] ? '🔒' : '🔓'}
      </button>
    ) : undefined;

  const renderField = (f: ParamField, s: SpecSection) => {
    if (f.kind === 'array') {
      // Instruction-labels textarea + example-insert button (create only).
      return (
        <div key={f.column} className="col-span-full flex flex-col gap-1.5">
          <div className="flex items-center justify-between">
            <FieldLabel field={f} />
            {!isEdit && (
              <button
                type="button"
                onClick={() => set(f.column, EXAMPLE_IX_LABELS)}
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
            value={form[f.column] ?? ''}
            readOnly={!editable(s)}
            onChange={(e) => set(f.column, e.target.value)}
            className={fieldCls(s, 'text-xs')}
            placeholder='["Pump.Fun: Buy"]'
          />
        </div>
      );
    }
    return (
      <label key={f.column} className="flex flex-col gap-1.5">
        <FieldLabel field={f} accent={f.required ? 'text-primary' : undefined} />
        <Input
          type="number"
          fieldSize="md"
          step={f.step}
          min={f.min}
          max={f.max}
          unit={f.unit}
          value={form[f.column] ?? ''}
          readOnly={!editable(s)}
          blankZero={!f.required}
          onChange={(e) => set(f.column, e.target.value)}
          className={fieldCls(s, f.inputClass)}
          placeholder={f.nullable ? (f.placeholder ?? '0 = off') : undefined}
        />
      </label>
    );
  };

  const modeOnly = spec.modeOptions.length === 1;

  return (
    <div className="flex flex-col gap-4 rounded-xl border border-white/10 bg-surface/40 p-4">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-bold text-text">
          {isEdit ? `Edit ${spec.title} Rule` : `New ${spec.title} Rule`}
        </h3>
        <button
          type="button"
          onClick={onClose}
          className="rounded-md border border-white/10 px-2 py-0.5 text-xs text-text-dim hover:text-text"
          title="Close form"
        >
          ✕
        </button>
      </div>

      {/* ── Mode + Rule Name (always visible) ── */}
      <div className="grid grid-cols-2 gap-3">
        <label className="flex flex-col gap-1.5">
          <span className="text-[10px] font-bold uppercase tracking-wider text-primary">Mode</span>
          <Select
            fieldSize="md"
            value={form[TRADE_MODE_KEY] ?? spec.modeOptions[0]}
            onChange={(e) => set(TRADE_MODE_KEY, e.target.value)}
            disabled={live || modeOnly}
            title={live ? 'Mode is frozen while the rule is running or holding positions' : undefined}
            className={cn('font-mono', (live || modeOnly) && 'cursor-not-allowed opacity-50')}
          >
            {spec.modeOptions.includes('paper') && <option value="paper">Paper Test</option>}
            {spec.modeOptions.includes('real') && <option value="real">Real Trading</option>}
          </Select>
        </label>
        <label className="flex flex-col gap-1.5">
          <span className="text-[10px] font-bold uppercase tracking-wider text-primary">Rule Name</span>
          <Input
            type="text"
            fieldSize="md"
            value={form[RULE_NAME_KEY] ?? ''}
            onChange={(e) => set(RULE_NAME_KEY, e.target.value)}
            placeholder="e.g. Sniper 0.5 SOL"
            className="font-mono"
          />
        </label>
      </div>

      {/* ── Paste params (rule ⎘ / sweep combo ⎘ / detect ⎘ → this form) ── */}
      <PasteParamsSection
        strategy={spec.strategy}
        live={live}
        onApply={(blob, mode) => {
          const { next, result } = applyBlob(spec, form, blob, { live, mode });
          onChange(next);
          return result;
        }}
      />

      {/* ── One collapsible accordion section per spec.sections ── */}
      <div className="flex flex-col gap-2">
        {spec.sections.map((s) => {
          const fields = spec.fields.filter((f) => f.section === s.key);
          if (!fields.length) return null;
          return (
            <Accordion
              key={s.key}
              padding="sm"
              header={
                <div className="flex min-w-0 flex-1 items-center justify-between gap-2">
                  <div className="flex items-baseline gap-2">
                    <span className={cn('text-[11px] font-bold uppercase tracking-wider', s.accent)}>
                      {s.label}
                    </span>
                    <span className="text-[10px] lowercase text-text-dim">{s.hint}</span>
                  </div>
                  {lockToggle(s)}
                </div>
              }
            >
              <div
                className="grid gap-3"
                style={{ gridTemplateColumns: `repeat(${s.cols}, minmax(0, 1fr))` }}
              >
                {fields.map((f) => renderField(f, s))}
              </div>
            </Accordion>
          );
        })}
      </div>

      {error && <InlineAlert variant="error">{error}</InlineAlert>}

      <div className="flex justify-end gap-2.5">
        <Button variant="ghost" onClick={onClose}>Cancel</Button>
        <Button variant="primary" onClick={() => onSave(unlocked)} disabled={loading}>
          {loading ? 'Saving…' : 'Save Rule'}
        </Button>
      </div>
    </div>
  );
}
