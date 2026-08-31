import { useMemo, useState, type ReactNode } from 'react';

import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { IconButton } from 'components/ui/IconButton';
import { LockIcon, SaveIcon, SpinnerIcon, UnlockIcon } from 'components/ui/icons';
import { Button } from 'components/ui/Button';
import { Tabs, TabsList, TabsTrigger, TabsPanel } from 'components/ui/Tabs';
import { cn } from 'lib/cn';
import { useStrategyRegistry, type StrategyRegistry } from 'lib/strategy/registry';
import {
  emptyRuleParams,
  ruleParamsFromJson,
  ruleParamsToJson,
  type RuleParams,
} from 'lib/strategy/ruleParams';
import {
  paramsToConditionRows,
  rowsToSides,
  type RuleConditionRow,
} from 'lib/strategy/ruleConditionRows';
import { validateRuleParams } from 'lib/strategy/validate';
import { solToLamports, lamportsToSol, type StrategyRule, type TradeMode } from 'lib/strategy/types';
import { ConditionBuilder } from './ConditionBuilder';
import {
  draftsToStages,
  stagesToDrafts,
  ScaleOutBuilder,
  type ScaleStageDraft,
} from './ScaleOutBuilder';
import { FingerprintParamsById } from './FingerprintParamsSummary';
import { FingerprintPicker } from './FingerprintPicker';
import { LabelTip } from './LabelTip';
import { RuleTagsInput } from './RuleTagsInput';
import { allTags } from 'lib/strategy/tags';
import { useGetStrategyRulesQuery } from 'store/sharedEndpoints';
import { RULE_FIELD_HELP } from 'lib/strategy/strategyHelp';

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
  /** Presentational labels — the server canonicalizes them on save. */
  tags: string[];
}

export interface RuleEditorProps {
  /** Existing rule to edit; omit to create. */
  initial?: StrategyRule;
  onSubmit: (draft: RuleEditorDraft) => void;
  onCancel?: () => void;
  submitting?: boolean;
  error?: string | null;
  /** Lab-only dry-run panel (FE3): given the editor's live draft (or `null` when
   *  no fingerprint is chosen) and whether it is valid enough to simulate,
   *  returns the panel rendered beneath the builder. Injected by the lab page so
   *  the shared editor never imports the lab-only simulate endpoints. */
  renderDryRun?: (draft: RuleEditorDraft | null, canRun: boolean) => ReactNode;
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
  renderDryRun,
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
  const [tags, setTags] = useState<string[]>(initial?.tags ?? []);
  // Autocomplete over the tags already in use, so the vocabulary converges
  // instead of sprouting a near-duplicate per rule.
  const { data: allRules = [] } = useGetStrategyRulesQuery();
  const tagSuggestions = useMemo(() => allTags(allRules), [allRules]);

  // `params` carries TP/SL/re-entry/flags; entry/exit conditions live in `rows`
  // and scale-out stages in `scaleStages` — both folded back at compose time.
  const [params, setParams] = useState<RuleParams>(() =>
    initial ? ruleParamsFromJson(initial.params, registry) : emptyRuleParams(),
  );
  const [rows, setRows] = useState<RuleConditionRow[]>(() => {
    const p = initial ? ruleParamsFromJson(initial.params, registry) : emptyRuleParams();
    // Parked conditions come back as rows too (muted, `enabled: false`) — that is the
    // point of storing them in `params.disabled` rather than dropping them on save.
    return paramsToConditionRows(p);
  });
  const [scaleStages, setScaleStages] = useState<ScaleStageDraft[]>(() => {
    const p = initial ? ruleParamsFromJson(initial.params, registry) : emptyRuleParams();
    // Parked stages come back as drafts too (muted, `enabled: false`) — same reason
    // as the parked condition rows above.
    return stagesToDrafts(p.scale_out, p.disabled?.scale_out);
  });
  // The full rule params = TP/SL/re-entry + conditions + scale-out. Parked rows and
  // parked stages share the one `disabled` bag, so they merge here rather than one
  // overwriting the other.
  const composedParams: RuleParams = useMemo(() => {
    const sides = rowsToSides(rows);
    const parkedStages = draftsToStages(scaleStages, false);
    return {
      take_profit: params.take_profit,
      stop_loss: params.stop_loss,
      reentry: params.reentry,
      exclusive: params.exclusive,
      priority: params.priority,
      buy_pct_of_vsol: params.buy_pct_of_vsol,
      ...sides,
      entry_lock: Object.keys(sides.entry_event).length ? params.entry_lock : null,
      disabled:
        sides.disabled || parkedStages
          ? { ...sides.disabled, scale_out: parkedStages }
          : null,
      scale_out: draftsToStages(scaleStages),
    };
  }, [
    params.take_profit,
    params.stop_loss,
    params.reentry,
    params.exclusive,
    params.priority,
    params.buy_pct_of_vsol,
    params.entry_lock,
    rows,
    scaleStages,
  ]);
  const [tab, setTab] = useState<'builder' | 'json'>('builder');
  const [jsonText, setJsonText] = useState(() =>
    JSON.stringify(ruleParamsToJson(composedParams), null, 2),
  );
  const [jsonError, setJsonError] = useState<string | null>(null);
  // Persisted rules start with mode locked; unlock via the padlock to flip paper↔real.
  // Duplicate / promote pass an id-less `initial` — no lock control (create path).
  const [modeUnlocked, setModeUnlocked] = useState(false);

  // Conditions (fingerprint + params) are locked once the rule is live — only
  // sizing/caps stay editable. Open positions keep their snapshotted trade_mode;
  // flipping mode here only affects future entries.
  const conditionsLocked = Boolean(initial?.is_active);
  const modeCanLock = Boolean(initial?.id);
  const modeLocked = modeCanLock && !modeUnlocked;

  const toggleModeLock = () => {
    if (modeUnlocked) {
      setModeUnlocked(false);
      return;
    }
    if (
      !window.confirm(
        'Unlock trade mode? Changing paper↔real affects future entries for this rule. Open positions keep their original mode.',
      )
    ) {
      return;
    }
    setModeUnlocked(true);
  };

  // Re-entry: absent ⇒ one-shot. Toggling on seeds the family-observed defaults
  // (cooldown 5s, cap 10/token); toggling off drops the block entirely. Editing a
  // field to blank stores NaN so validation flags it (kept a number for the type).
  const reentryOn = params.reentry != null;
  const toggleReentry = (on: boolean) =>
    setParams((p) => ({
      ...p,
      reentry: on ? (p.reentry ?? { cooldown_sec: 5, max_episodes_per_token: 10 }) : null,
    }));
  const setReentryField = (field: 'cooldown_sec' | 'max_episodes_per_token', n: number | null) =>
    setParams((p) => ({
      ...p,
      reentry: {
        cooldown_sec: p.reentry?.cooldown_sec ?? NaN,
        max_episodes_per_token: p.reentry?.max_episodes_per_token ?? NaN,
        [field]: n ?? NaN,
      },
    }));
  const finiteOrNull = (v: number | null | undefined) =>
    v != null && Number.isFinite(v) ? v : null;

  // When editing the JSON tab, fold it back into TP/SL/re-entry, condition rows,
  // and scale-out stages so the builder stays in sync.
  const syncFromJson = (text: string) => {
    setJsonText(text);
    try {
      const parsed = ruleParamsFromJson(JSON.parse(text), registry);
      setParams((p) => ({
        ...p,
        take_profit: parsed.take_profit,
        stop_loss: parsed.stop_loss,
        reentry: parsed.reentry,
        exclusive: parsed.exclusive,
        priority: parsed.priority,
        buy_pct_of_vsol: parsed.buy_pct_of_vsol,
        entry_event: parsed.entry_event,
        entry_lock: parsed.entry_lock,
      }));
      setRows(paramsToConditionRows(parsed));
      setScaleStages(stagesToDrafts(parsed.scale_out, parsed.disabled?.scale_out));
      setJsonError(null);
    } catch (e) {
      setJsonError(e instanceof Error ? e.message : 'invalid JSON');
    }
  };
  const switchTab = (next: string) => {
    if (next === 'json') setJsonText(JSON.stringify(ruleParamsToJson(composedParams), null, 2));
    setTab(next as 'builder' | 'json');
  };

  const paramErrors = useMemo(
    () => validateRuleParams(composedParams, registry),
    [composedParams, registry],
  );
  const buyLamports = solToLamports(buySol) ?? 0;
  const errors: string[] = [];
  if (!ruleName.trim()) errors.push('rule_name must not be empty');
  if (!fingerprintId) errors.push('a fingerprint is required');
  if (buyLamports <= 0) errors.push('buy amount must be > 0');
  // Blank ⇒ `null` ⇒ saved as the `0 = unlimited` sentinel on both caps, so only
  // a negative typed value is an error.
  if ((maxConcurrent ?? 0) < 0) errors.push('max concurrent must be ≥ 0 (blank = ∞)');
  if ((maxTotal ?? 0) < 0) errors.push('max total must be ≥ 0 (blank = ∞)');
  errors.push(...paramErrors);
  if (jsonError) errors.push(`JSON: ${jsonError}`);
  const canSubmit = errors.length === 0 && !submitting;

  // The editor's live draft — shared by submit and the dry-run render-prop.
  const currentDraft: RuleEditorDraft | null = fingerprintId
    ? {
        rule_name: ruleName.trim(),
        fingerprint_id: fingerprintId,
        trade_mode: mode,
        buy_amount_lamports: buyLamports,
        max_concurrent_tokens: maxConcurrent ?? 0,
        max_total_tokens: maxTotal ?? 0,
        params: ruleParamsToJson(composedParams),
        tags,
      }
    : null;

  const submit = () => {
    if (currentDraft) onSubmit(currentDraft);
  };

  return (
    <div className="flex flex-col gap-3">
      {/* Header: identity + sizing */}
      <div className="flex flex-wrap items-end gap-3">
        <label className="flex flex-col gap-1 text-[11px] text-text-dim">
          <LabelTip tip={RULE_FIELD_HELP.name}>Name</LabelTip>
          <Input fieldSize="sm" value={ruleName} onChange={(e) => setRuleName(e.target.value)} />
        </label>
        <div className="flex flex-col gap-1 text-[11px] text-text-dim">
          <LabelTip tip={RULE_FIELD_HELP.mode}>Mode</LabelTip>
          <div className="flex items-center gap-1">
            <Select
              fieldSize="sm"
              value={mode}
              onChange={(e) => setMode(e.target.value as TradeMode)}
              disabled={modeLocked}
            >
              <option value="paper">paper</option>
              <option value="real">real</option>
            </Select>
            {modeCanLock && (
              <IconButton
                variant="ghost"
                size="sm"
                active={modeUnlocked}
                onClick={toggleModeLock}
                title={modeUnlocked ? 'Lock trade mode' : 'Unlock trade mode'}
                aria-label={modeUnlocked ? 'Lock trade mode' : 'Unlock trade mode'}
              >
                {modeUnlocked ? <UnlockIcon /> : <LockIcon />}
              </IconButton>
            )}
          </div>
        </div>
        <label className="flex flex-col gap-1 text-[11px] text-text-dim">
          <LabelTip tip={RULE_FIELD_HELP.buy}>Buy (◎)</LabelTip>
          <Input
            fieldSize="sm"
            numeric
            unit="◎"
            numericValue={buySol}
            onNumericChange={setBuySol}
            className="w-24"
          />
        </label>
        {/* The two governance caps are the genuine `0 = off` sentinels left in the
            rule form: `blankZero` renders a stored 0 as an empty field so "no cap"
            reads as blank/∞ instead of "capped at zero". Display-only — an untouched
            0 still saves as 0, which the engine decodes to `Cap::UNLIMITED`. A NEW
            rule still opens at 1 concurrent, so unbounded is always an explicit
            authoring act, never a default. */}
        <label className="flex flex-col gap-1 text-[11px] text-text-dim">
          <LabelTip tip={RULE_FIELD_HELP.maxConcurrent}>Max concurrent</LabelTip>
          <Input
            fieldSize="sm"
            numeric
            integer
            blankZero
            placeholder="∞"
            numericValue={maxConcurrent}
            onNumericChange={setMaxConcurrent}
            className="w-20"
          />
        </label>
        <label className="flex flex-col gap-1 text-[11px] text-text-dim">
          <LabelTip tip={RULE_FIELD_HELP.maxTotal}>Max total</LabelTip>
          <Input
            fieldSize="sm"
            numeric
            integer
            blankZero
            placeholder="∞"
            numericValue={maxTotal}
            onNumericChange={setMaxTotal}
            className="w-20"
          />
        </label>
      </div>

      {/* Tags — a label, not a condition, so it stays editable while the rule is
          live (unlike the fingerprint / entry / exit block below). */}
      <div className="flex flex-col gap-1 text-[11px] text-text-dim">
        <LabelTip tip={RULE_FIELD_HELP.tags}>Tags</LabelTip>
        <RuleTagsInput value={tags} onChange={setTags} suggestions={tagSuggestions} />
      </div>

      {/* Fingerprint + TP/SL — controls on one row; axis chips below full width */}
      <div className="flex flex-col gap-1.5">
        <div className="grid grid-cols-[minmax(0,1fr)_5rem_5rem] items-end gap-x-3">
          <div className="flex min-w-0 flex-col gap-1 text-[11px] text-text-dim">
            <LabelTip tip={RULE_FIELD_HELP.fingerprint}>
              Fingerprint{' '}
              {conditionsLocked && <span className="text-text-dim/60">(locked — rule live)</span>}
            </LabelTip>
            <FingerprintPicker
              value={fingerprintId}
              onChange={setFingerprintId}
              disabled={conditionsLocked}
            />
          </div>
          <label className="flex flex-col gap-1 text-[11px] text-text-dim">
            <LabelTip tip={RULE_FIELD_HELP.takeProfit}>TP (%)</LabelTip>
            <Input
              fieldSize="sm"
              numeric
              unit="%"
              numericValue={params.take_profit}
              onNumericChange={(n) => setParams((p) => ({ ...p, take_profit: n }))}
              disabled={conditionsLocked}
              className="w-full"
            />
          </label>
          <label className="flex flex-col gap-1 text-[11px] text-text-dim">
            <LabelTip tip={RULE_FIELD_HELP.stopLoss}>SL (%)</LabelTip>
            <Input
              fieldSize="sm"
              numeric
              unit="%"
              numericValue={params.stop_loss}
              onNumericChange={(n) => setParams((p) => ({ ...p, stop_loss: n }))}
              disabled={conditionsLocked}
              className="w-full"
            />
          </label>
        </div>
        <FingerprintParamsById id={fingerprintId} />
      </div>

      {/* Re-entry (optional): re-arm after each normal exit. Absent ⇒ one-shot. */}
      <div className="flex flex-wrap items-end gap-3">
        <label className="flex h-8 items-center gap-1.5 text-[11px] text-text-dim">
          <input
            type="checkbox"
            checked={reentryOn}
            disabled={conditionsLocked}
            onChange={(e) => toggleReentry(e.target.checked)}
            className="accent-accent"
          />
          <LabelTip tip={RULE_FIELD_HELP.reentry}>Re-entry</LabelTip>
        </label>
        {reentryOn && (
          <>
            <label className="flex flex-col gap-1 text-[11px] text-text-dim">
              <LabelTip tip={RULE_FIELD_HELP.reentryCooldown}>Cooldown (s)</LabelTip>
              <Input
                fieldSize="sm"
                numeric
                unit="s"
                numericValue={finiteOrNull(params.reentry?.cooldown_sec)}
                onNumericChange={(n) => setReentryField('cooldown_sec', n)}
                disabled={conditionsLocked}
                className="w-20"
              />
            </label>
            <label className="flex flex-col gap-1 text-[11px] text-text-dim">
              <LabelTip tip={RULE_FIELD_HELP.reentryMaxEpisodes}>Max episodes/token</LabelTip>
              <Input
                fieldSize="sm"
                numeric
                integer
                numericValue={finiteOrNull(params.reentry?.max_episodes_per_token)}
                onNumericChange={(n) => setReentryField('max_episodes_per_token', n)}
                disabled={conditionsLocked}
                className="w-24"
              />
            </label>
          </>
        )}
      </div>

      {/* Exclusivity (optional): skip entry while any other rule holds this token. */}
      <div className="flex flex-wrap items-end gap-3">
        <label className="flex h-8 items-center gap-1.5 text-[11px] text-text-dim">
          <input
            type="checkbox"
            checked={params.exclusive}
            disabled={conditionsLocked}
            onChange={(e) => setParams((p) => ({ ...p, exclusive: e.target.checked }))}
            className="accent-accent"
          />
          <LabelTip tip={RULE_FIELD_HELP.exclusive}>Exclusive</LabelTip>
        </label>
        {params.exclusive && (
          <label className="flex flex-col gap-1 text-[11px] text-text-dim">
            <LabelTip tip={RULE_FIELD_HELP.exclusivePriority}>Priority</LabelTip>
            <Input
              fieldSize="sm"
              numeric
              integer
              numericValue={params.priority}
              onNumericChange={(n) => setParams((p) => ({ ...p, priority: n ?? 0 }))}
              disabled={conditionsLocked}
              className="w-20"
            />
          </label>
        )}
        {/* Blank = the rule's fixed buy size. Set = a percent of the pool at entry,
            which is what holds our own impact constant across a liquidity band. */}
        <label className="flex flex-col gap-1 text-[11px] text-text-dim">
          <LabelTip tip={RULE_FIELD_HELP.buyPctOfVsol}>Buy % of vSOL</LabelTip>
          <Input
            fieldSize="sm"
            numeric
            unit="%"
            numericValue={finiteOrNull(params.buy_pct_of_vsol)}
            onNumericChange={(n) => setParams((p) => ({ ...p, buy_pct_of_vsol: n }))}
            disabled={conditionsLocked}
            className="w-24"
          />
        </label>
      </div>

      <Tabs value={tab} onValueChange={switchTab}>
        <TabsList>
          <TabsTrigger value="builder">Builder</TabsTrigger>
          <TabsTrigger value="json">JSON</TabsTrigger>
        </TabsList>
        <TabsPanel value="builder">
          <div className="flex flex-col gap-3">
            <ConditionBuilder
              rows={rows}
              onChange={setRows}
              registry={registry}
              disabled={conditionsLocked}
              entryLock={params.entry_lock}
              onEntryLockChange={(lock) => setParams((p) => ({ ...p, entry_lock: lock }))}
            />
            <ScaleOutBuilder
              stages={scaleStages}
              onChange={setScaleStages}
              registry={registry}
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
          <p className="mt-1 flex items-center gap-1 text-[11px] text-text-dim/70">
            Raw <code>params</code> JSON — registry-validated. Order normalizes on save.
            <LabelTip tip={RULE_FIELD_HELP.paramsJson} />
          </p>
        </TabsPanel>
      </Tabs>

      {renderDryRun?.(currentDraft, canSubmit)}

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
        <IconButton
          variant="primary"
          size="lg"
          disabled={!canSubmit}
          onClick={submit}
          label={submitting ? 'Saving…' : initial?.id ? 'Save rule' : 'Create rule'}
          title={submitting ? 'Saving…' : initial?.id ? 'Save rule' : 'Create rule'}
        >
          {submitting ? <SpinnerIcon /> : <SaveIcon />}
        </IconButton>
      </div>
    </div>
  );
}
