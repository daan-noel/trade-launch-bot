import { useMemo, useState, type ReactNode } from 'react';

import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { Button } from 'components/ui/Button';
import { Tabs, TabsList, TabsTrigger, TabsPanel } from 'components/ui/Tabs';
import { cn } from 'lib/cn';
import { useStrategyRegistry, type StrategyRegistry } from 'lib/strategy/registry';
import {
  emptyRuleParams,
  ruleParamsFromJson,
  ruleParamsToJson,
  type RuleParams,
  type SideConditions,
} from 'lib/strategy/ruleParams';
import { validateRuleParams } from 'lib/strategy/validate';
import { solToLamports, lamportsToSol, type StrategyRule, type TradeMode } from 'lib/strategy/types';
import { ConditionSideEditor } from './ConditionSideEditor';
import { FingerprintPicker } from './FingerprintPicker';

/** The normalized draft the editor emits (matches the create body; the page maps
 *  it to a create or an update patch). */
export interface RuleEditorDraft {
  rule_name: string;
  fingerprint_id: string;
  trade_mode: TradeMode;
  buy_amount_lamports: number;
  max_concurrent_tokens: number;
  max_total_tokens: number;
  params: Record<string, unknown>;
}

export interface RuleEditorProps {
  /** Existing rule to edit; omit to create. */
  initial?: StrategyRule;
  onSubmit: (draft: RuleEditorDraft) => void;
  onCancel?: () => void;
  submitting?: boolean;
  error?: string | null;
  /** Lab-only dry-run panel (FE3); rendered beneath the builder when provided. */
  dryRunSlot?: ReactNode;
}

/** Wrapper that waits for the registry (the editor renders entirely from it). */
export function RuleEditor(props: RuleEditorProps) {
  const { data: registry, isLoading } = useStrategyRegistry();
  if (isLoading || !registry) {
    return <p className="p-3 text-[12px] text-text-dim">loading registry…</p>;
  }
  return <RuleEditorInner {...props} registry={registry} />;
}

function RuleEditorInner({
  initial,
  onSubmit,
  onCancel,
  submitting,
  error,
  dryRunSlot,
  registry,
}: RuleEditorProps & { registry: StrategyRegistry }) {
  const [ruleName, setRuleName] = useState(initial?.rule_name ?? '');
  const [mode, setMode] = useState<TradeMode>(initial?.trade_mode ?? 'paper');
  const [buySol, setBuySol] = useState<number | null>(lamportsToSol(initial?.buy_amount_lamports));
  const [maxConcurrent, setMaxConcurrent] = useState<number | null>(
    initial?.max_concurrent_tokens ?? 1,
  );
  const [maxTotal, setMaxTotal] = useState<number | null>(initial?.max_total_tokens ?? 0);
  const [fingerprintId, setFingerprintId] = useState<string | null>(initial?.fingerprint_id ?? null);

  const [params, setParams] = useState<RuleParams>(() =>
    initial ? ruleParamsFromJson(initial.params, registry) : emptyRuleParams(),
  );
  const [tab, setTab] = useState<'builder' | 'json'>('builder');
  const [jsonText, setJsonText] = useState(() => JSON.stringify(ruleParamsToJson(params), null, 2));
  const [jsonError, setJsonError] = useState<string | null>(null);

  // Conditions (fingerprint + params) are locked once the rule is live — only
  // sizing/caps stay editable (ported lock semantics). Create is fully open.
  const conditionsLocked = Boolean(initial?.is_active);

  const setSide = (side: 'entry' | 'exit', next: SideConditions) =>
    setParams((p) => ({ ...p, [side]: next }));

  // When leaving the JSON tab, fold edited JSON back into the form model.
  const syncFromJson = (text: string) => {
    setJsonText(text);
    try {
      const parsed = JSON.parse(text);
      setParams(ruleParamsFromJson(parsed, registry));
      setJsonError(null);
    } catch (e) {
      setJsonError(e instanceof Error ? e.message : 'invalid JSON');
    }
  };
  const switchTab = (next: string) => {
    if (next === 'json') setJsonText(JSON.stringify(ruleParamsToJson(params), null, 2));
    setTab(next as 'builder' | 'json');
  };

  const paramErrors = useMemo(() => validateRuleParams(params, registry), [params, registry]);
  const buyLamports = solToLamports(buySol) ?? 0;
  const errors: string[] = [];
  if (!ruleName.trim()) errors.push('rule_name must not be empty');
  if (!fingerprintId) errors.push('a fingerprint is required');
  if (buyLamports <= 0) errors.push('buy amount must be > 0');
  if ((maxConcurrent ?? 0) < 1) errors.push('max concurrent must be ≥ 1');
  if ((maxTotal ?? 0) < 0) errors.push('max total must be ≥ 0');
  errors.push(...paramErrors);
  if (jsonError) errors.push(`JSON: ${jsonError}`);
  const canSubmit = errors.length === 0 && !submitting;

  const submit = () => {
    if (!fingerprintId) return;
    onSubmit({
      rule_name: ruleName.trim(),
      fingerprint_id: fingerprintId,
      trade_mode: mode,
      buy_amount_lamports: buyLamports,
      max_concurrent_tokens: maxConcurrent ?? 1,
      max_total_tokens: maxTotal ?? 0,
      params: ruleParamsToJson(params),
    });
  };

  return (
    <div className="flex flex-col gap-3">
      {/* Header: identity + sizing */}
      <div className="flex flex-wrap items-end gap-3">
        <label className="flex flex-col gap-1 text-[11px] text-text-dim">
          Name
          <Input fieldSize="sm" value={ruleName} onChange={(e) => setRuleName(e.target.value)} />
        </label>
        <label className="flex flex-col gap-1 text-[11px] text-text-dim">
          Mode
          <Select fieldSize="sm" value={mode} onChange={(e) => setMode(e.target.value as TradeMode)}>
            <option value="paper">paper</option>
            <option value="real">real</option>
          </Select>
        </label>
        <label className="flex flex-col gap-1 text-[11px] text-text-dim">
          Buy (◎)
          <Input
            fieldSize="sm"
            numeric
            unit="◎"
            numericValue={buySol}
            onNumericChange={setBuySol}
            className="w-24"
          />
        </label>
        <label className="flex flex-col gap-1 text-[11px] text-text-dim">
          Max concurrent
          <Input
            fieldSize="sm"
            numeric
            integer
            numericValue={maxConcurrent}
            onNumericChange={setMaxConcurrent}
            className="w-20"
          />
        </label>
        <label className="flex flex-col gap-1 text-[11px] text-text-dim">
          Max total (0=∞)
          <Input
            fieldSize="sm"
            numeric
            integer
            numericValue={maxTotal}
            onNumericChange={setMaxTotal}
            className="w-20"
          />
        </label>
      </div>

      {/* Fingerprint + TP/SL */}
      <div className="flex flex-wrap items-end gap-3">
        <div className="flex flex-col gap-1 text-[11px] text-text-dim">
          Fingerprint {conditionsLocked && <span className="text-text-dim/60">(locked — rule live)</span>}
          <FingerprintPicker value={fingerprintId} onChange={setFingerprintId} disabled={conditionsLocked} />
        </div>
        <label className="flex flex-col gap-1 text-[11px] text-text-dim">
          TP (%)
          <Input
            fieldSize="sm"
            numeric
            unit="%"
            numericValue={params.take_profit}
            onNumericChange={(n) => setParams((p) => ({ ...p, take_profit: n }))}
            disabled={conditionsLocked}
            className="w-20"
          />
        </label>
        <label className="flex flex-col gap-1 text-[11px] text-text-dim">
          SL (%)
          <Input
            fieldSize="sm"
            numeric
            unit="%"
            numericValue={params.stop_loss}
            onNumericChange={(n) => setParams((p) => ({ ...p, stop_loss: n }))}
            disabled={conditionsLocked}
            className="w-20"
          />
        </label>
      </div>

      <Tabs value={tab} onValueChange={switchTab}>
        <TabsList>
          <TabsTrigger value="builder">Builder</TabsTrigger>
          <TabsTrigger value="json">JSON</TabsTrigger>
        </TabsList>
        <TabsPanel value="builder">
          <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
            <ConditionSideEditor
              side="entry"
              registry={registry}
              value={params.entry ?? {}}
              onChange={(next) => setSide('entry', next)}
              disabled={conditionsLocked}
            />
            <ConditionSideEditor
              side="exit"
              registry={registry}
              value={params.exit ?? {}}
              onChange={(next) => setSide('exit', next)}
              disabled={conditionsLocked}
            />
          </div>
        </TabsPanel>
        <TabsPanel value="json">
          <textarea
            className={cn(
              'h-72 w-full rounded-md border border-white/10 bg-bg-card p-2 font-mono text-[12px] text-text outline-none',
              jsonError && 'border-red/70',
            )}
            value={jsonText}
            spellCheck={false}
            disabled={conditionsLocked}
            onChange={(e) => syncFromJson(e.target.value)}
          />
          <p className="mt-1 text-[11px] text-text-dim/70">
            Raw <code>params</code> JSON — registry-validated. Order normalizes on save.
          </p>
        </TabsPanel>
      </Tabs>

      {dryRunSlot}

      {errors.length > 0 && (
        <ul className="flex flex-col gap-0.5 text-[11px] text-red">
          {errors.map((e, i) => (
            <li key={i}>• {e}</li>
          ))}
        </ul>
      )}
      {error && <p className="text-[11px] text-red">{error}</p>}
      <div className="flex justify-end gap-2">
        {onCancel && (
          <Button variant="ghost" size="sm" onClick={onCancel} disabled={submitting}>
            Cancel
          </Button>
        )}
        <Button variant="primary" size="sm" disabled={!canSubmit} onClick={submit}>
          {initial ? 'Save rule' : 'Create rule'}
        </Button>
      </div>
    </div>
  );
}
