import { useMemo, useState, type ReactNode } from 'react';

import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { IconButton } from 'components/ui/IconButton';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { LockIcon, SaveIcon, SpinnerIcon, UnlockIcon } from 'components/ui/icons';
import { Button } from 'components/ui/Button';
import { Tabs, TabsList, TabsTrigger, TabsPanel } from 'components/ui/Tabs';
import { cn } from 'lib/cn';
import { rulePart, useStrategyRegistry, type StrategyRegistry } from 'lib/strategy/registry';
import {
  emptyRuleDoc,
  renameSignal,
  renameStage,
  ruleDocFromJson,
  ruleDocToJson,
  type RuleDoc,
} from 'lib/strategy/ruleDoc';
import { tagNames } from 'lib/strategy/tagsDoc';
import { validateRuleDoc } from 'lib/strategy/validate';
import { solToLamports, lamportsToSol, type StrategyRule, type TradeMode } from 'lib/strategy/types';
import { FingerprintParamsById } from './FingerprintParamsSummary';
import { FingerprintPicker } from './FingerprintPicker';
import { LabelTip } from './LabelTip';
import { RuleTagsInput } from './RuleTagsInput';
import { allTags } from 'lib/strategy/tags';
import { useGetFingerprintsQuery, useGetStrategyRulesQuery } from 'store/sharedEndpoints';
import { RULE_FIELD_HELP } from 'lib/strategy/strategyHelp';
import type { CondContext } from './rule/CondRow';
import { CondList, LineList, PartHeader } from './rule/parts';
import { SignalsEditor } from './rule/SignalsEditor';
import { StagesEditor } from './rule/StagesEditor';
import { RuleSentences } from './rule/RuleSentences';
import { GuideButton } from './StrategyGuide';

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
  /** Presentational labels; the server canonicalizes them on save. */
  tags: string[];
}

export interface RuleEditorProps {
  /** Existing rule to edit; omit to create. */
  initial?: StrategyRule;
  onSubmit: (draft: RuleEditorDraft) => void;
  onCancel?: () => void;
  submitting?: boolean;
  error?: string | null;
  /** Lab-only dry-run panel: given the editor's live draft (or `null` when no
   *  fingerprint is chosen) and whether it is valid enough to simulate, returns the
   *  panel rendered beneath the builder. Injected by the lab page so the shared editor
   *  never imports the lab-only simulate endpoints. */
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

/** Read the stored params; a document the editor cannot read opens empty with the
 *  reason shown, so it is never silently overwritten by a save. */
function initialDoc(initial: StrategyRule | undefined): { doc: RuleDoc; error: string | null } {
  if (!initial) return { doc: emptyRuleDoc(), error: null };
  try {
    return { doc: ruleDocFromJson(initial.params), error: null };
  } catch (e) {
    return { doc: emptyRuleDoc(), error: e instanceof Error ? e.message : String(e) };
  }
}

function Section({ n, title, children }: { n: number; title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-3 rounded-lg border border-white/10 bg-white/2 p-3">
      <h3 className="text-[13px] font-semibold text-text">
        <span className="mr-1.5 text-accent">{n}.</span>
        {title}
      </h3>
      {children}
    </section>
  );
}

function NumberField({
  reg,
  part,
  label,
  value,
  onChange,
  unit,
  integer,
  disabled,
  className = 'w-24',
  placeholder,
}: {
  reg: StrategyRegistry;
  part?: string;
  label: string;
  value: number | null;
  onChange: (n: number | null) => void;
  unit?: string;
  integer?: boolean;
  disabled?: boolean;
  className?: string;
  placeholder?: string;
}) {
  const p = part ? rulePart(reg, part) : undefined;
  return (
    <label className="flex flex-col gap-1 text-[11px] text-text-dim">
      <span className="inline-flex items-center gap-1">
        {label}
        {p && <InfoTooltip title={p.title} body={`${p.summary}\n\nExample: ${p.example}`} />}
      </span>
      <Input
        fieldSize="sm"
        numeric
        integer={integer}
        unit={unit}
        placeholder={placeholder ?? 'off'}
        numericValue={value != null && Number.isFinite(value) ? value : null}
        onNumericChange={onChange}
        disabled={disabled}
        className={className}
      />
    </label>
  );
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
  const [maxConcurrent, setMaxConcurrent] = useState<number | null>(initial?.max_concurrent_tokens ?? 1);
  const [maxTotal, setMaxTotal] = useState<number | null>(initial?.max_total_tokens ?? 0);
  const [fingerprintId, setFingerprintId] = useState<string | null>(initial?.fingerprint_id ?? null);
  const [labels, setLabels] = useState<string[]>(initial?.tags ?? []);
  const { data: allRules = [] } = useGetStrategyRulesQuery();
  const labelSuggestions = useMemo(() => allTags(allRules), [allRules]);
  const { data: fingerprints = [] } = useGetFingerprintsQuery();
  const fpTags = useMemo(
    () => tagNames(fingerprints.find((f) => f.id === fingerprintId)?.tags),
    [fingerprints, fingerprintId],
  );

  const [{ doc: firstDoc, error: loadError }] = useState(() => initialDoc(initial));
  const [doc, setDoc] = useState<RuleDoc>(firstDoc);
  const [tab, setTab] = useState<'builder' | 'json' | 'words'>('builder');
  const [jsonText, setJsonText] = useState(() => JSON.stringify(ruleDocToJson(firstDoc), null, 2));
  const [jsonError, setJsonError] = useState<string | null>(loadError);
  const [modeUnlocked, setModeUnlocked] = useState(false);

  // A live rule's conditions are locked; sizing and caps stay editable. Open positions
  // keep their snapshotted trade mode, so flipping mode only affects future buys.
  const conditionsLocked = Boolean(initial?.is_active);
  const modeCanLock = Boolean(initial?.id);
  const modeLocked = modeCanLock && !modeUnlocked;
  const toggleModeLock = () => {
    if (modeUnlocked) {
      setModeUnlocked(false);
      return;
    }
    if (!window.confirm('Unlock trade mode? Changing paper↔real affects future buys for this rule. Open positions keep their original mode.')) {
      return;
    }
    setModeUnlocked(true);
  };

  const patch = (f: (d: RuleDoc) => RuleDoc) => setDoc((d) => f(d));
  const setEnter = (e: Partial<RuleDoc['enter']>) => patch((d) => ({ ...d, enter: { ...d.enter, ...e } }));

  const syncFromJson = (text: string) => {
    setJsonText(text);
    try {
      setDoc(ruleDocFromJson(JSON.parse(text)));
      setJsonError(null);
    } catch (e) {
      setJsonError(e instanceof Error ? e.message : 'invalid JSON');
    }
  };
  const switchTab = (next: string) => {
    if (next === 'json') setJsonText(JSON.stringify(ruleDocToJson(doc), null, 2));
    setTab(next as 'builder' | 'json' | 'words');
  };

  const issues = useMemo(
    () => validateRuleDoc(doc, registry, fingerprintId ? fpTags : undefined),
    [doc, registry, fingerprintId, fpTags],
  );
  const buyLamports = solToLamports(buySol) ?? 0;
  const errors: string[] = [];
  if (!ruleName.trim()) errors.push('Give the rule a name');
  if (!fingerprintId) errors.push('Pick a fingerprint: it decides which coins the rule watches');
  if (buyLamports <= 0) errors.push('The buy amount must be above 0');
  if ((maxConcurrent ?? 0) < 0) errors.push('Max concurrent must be 0 or more (blank = no cap)');
  if ((maxTotal ?? 0) < 0) errors.push('Max total must be 0 or more (blank = no cap)');
  errors.push(...issues.errors);
  if (jsonError) errors.push(`JSON: ${jsonError}`);
  const canSubmit = errors.length === 0 && !submitting;

  const currentDraft: RuleEditorDraft | null = fingerprintId
    ? {
        rule_name: ruleName.trim(),
        fingerprint_id: fingerprintId,
        trade_mode: mode,
        buy_amount_lamports: buyLamports,
        max_concurrent_tokens: maxConcurrent ?? 0,
        max_total_tokens: maxTotal ?? 0,
        params: ruleDocToJson(doc),
        tags: labels,
      }
    : null;

  const signalNames = doc.signals.map((s) => s.name);
  const stageNames = doc.stages.map((s) => s.name);
  const buyCtx: CondContext = { reg: registry, tags: fpTags, signals: signalNames, beforeBuy: true, disabled: conditionsLocked };
  const sellCtx: CondContext = { ...buyCtx, beforeBuy: false };

  return (
    <div className="flex flex-col gap-3">
      {/* Identity + sizing */}
      <div className="flex flex-wrap items-end gap-3">
        <label className="flex flex-col gap-1 text-[11px] text-text-dim">
          <LabelTip tip={RULE_FIELD_HELP.name}>Name</LabelTip>
          <Input fieldSize="sm" value={ruleName} onChange={(e) => setRuleName(e.target.value)} />
        </label>
        <div className="flex flex-col gap-1 text-[11px] text-text-dim">
          <LabelTip tip={RULE_FIELD_HELP.mode}>Mode</LabelTip>
          <div className="flex items-center gap-1">
            <Select fieldSize="sm" value={mode} onChange={(e) => setMode(e.target.value as TradeMode)} disabled={modeLocked}>
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
          <Input fieldSize="sm" numeric unit="◎" numericValue={buySol} onNumericChange={setBuySol} className="w-24" />
        </label>
        {/* The two caps are the genuine `0 = off` sentinels: a stored 0 renders blank so
            "no cap" reads as ∞ instead of "capped at zero". A new rule opens at 1
            concurrent, so unbounded is always an explicit choice. */}
        <label className="flex flex-col gap-1 text-[11px] text-text-dim">
          <LabelTip tip={RULE_FIELD_HELP.maxConcurrent}>Max concurrent</LabelTip>
          <Input fieldSize="sm" numeric integer blankZero placeholder="∞" numericValue={maxConcurrent} onNumericChange={setMaxConcurrent} className="w-20" />
        </label>
        <label className="flex flex-col gap-1 text-[11px] text-text-dim">
          <LabelTip tip={RULE_FIELD_HELP.maxTotal}>Max total</LabelTip>
          <Input fieldSize="sm" numeric integer blankZero placeholder="∞" numericValue={maxTotal} onNumericChange={setMaxTotal} className="w-20" />
        </label>
      </div>

      <div className="flex flex-col gap-1 text-[11px] text-text-dim">
        <LabelTip tip={RULE_FIELD_HELP.tags}>Labels</LabelTip>
        <RuleTagsInput value={labels} onChange={setLabels} suggestions={labelSuggestions} />
      </div>

      <div className="flex flex-col gap-1.5">
        <div className="flex min-w-0 flex-col gap-1 text-[11px] text-text-dim">
          <LabelTip tip={RULE_FIELD_HELP.fingerprint}>
            Fingerprint {conditionsLocked && <span className="text-text-dim/60">(locked: rule live)</span>}
          </LabelTip>
          <FingerprintPicker value={fingerprintId} onChange={setFingerprintId} disabled={conditionsLocked} />
        </div>
        <FingerprintParamsById id={fingerprintId} />
        {fingerprintId && (
          <p className="text-[11px] text-text-dim">
            Tags this fingerprint defines:{' '}
            {fpTags.length ? fpTags.map((t) => <code key={t} className="mr-1">@{t}</code>) : <span className="italic">none (metrics that need a tag read nothing)</span>}
          </p>
        )}
      </div>

      <Tabs value={tab} onValueChange={switchTab}>
        <div className="flex items-center justify-between gap-2">
          <TabsList>
            <TabsTrigger value="builder">Builder</TabsTrigger>
            <TabsTrigger value="json">JSON</TabsTrigger>
            <TabsTrigger value="words">In words</TabsTrigger>
          </TabsList>
          <GuideButton />
        </div>
        <TabsPanel value="builder">
          <div className="flex flex-col gap-3">
            <Section n={1} title="Buy">
              <PartHeader ctx={buyCtx} part="enter.event" />
              <CondList conds={doc.enter.event} onChange={(event) => setEnter({ event })} ctx={buyCtx} empty="No trigger: the rule buys on the first print or tick where the filters below hold." />
              <PartHeader ctx={buyCtx} part="enter.filters" />
              <CondList conds={doc.enter.filters} onChange={(filters) => setEnter({ filters })} ctx={buyCtx} />
              <PartHeader ctx={buyCtx} part="enter.final_filters" />
              <CondList conds={doc.enter.final_filters} onChange={(final_filters) => setEnter({ final_filters })} ctx={buyCtx} />
              <div className="flex flex-wrap items-end gap-3">
                <label className="flex flex-col gap-1 text-[11px] text-text-dim">
                  <span className="inline-flex items-center gap-1">
                    {rulePart(registry, 'enter.lock')?.title ?? 'One chance'}
                    <InfoTooltip body={`${rulePart(registry, 'enter.lock')?.summary ?? ''}\n\nExample: ${rulePart(registry, 'enter.lock')?.example ?? ''}`} />
                  </span>
                  <Select
                    className="w-48"
                    value={doc.enter.lock ?? ''}
                    disabled={conditionsLocked}
                    onChange={(e) => setEnter({ lock: (e.target.value || null) as RuleDoc['enter']['lock'] })}
                  >
                    <option value="">no: any print may buy</option>
                    <option value="token">once per coin</option>
                    <option value="slot">once per slot</option>
                  </Select>
                </label>
                <NumberField
                  reg={registry}
                  part="enter.size_pct_of_pool"
                  label="Size as % of pool"
                  unit="%"
                  placeholder="fixed"
                  value={doc.enter.size_pct_of_pool}
                  onChange={(n) => setEnter({ size_pct_of_pool: n })}
                  disabled={conditionsLocked}
                />
              </div>
            </Section>

            <Section n={2} title="Signals">
              <SignalsEditor
                doc={doc}
                ctx={sellCtx}
                onChange={(signals) => patch((d) => ({ ...d, signals }))}
                onRename={(from, to) => patch((d) => renameSignal(d, from, to))}
              />
            </Section>

            <Section n={3} title="Sell">
              <div className="flex flex-wrap items-end gap-3">
                <NumberField reg={registry} part="stop_loss" label="Stop loss" unit="%" value={doc.stop_loss} onChange={(n) => patch((d) => ({ ...d, stop_loss: n }))} disabled={conditionsLocked} />
                <NumberField reg={registry} part="take_profit" label="Take profit" unit="%" value={doc.take_profit} onChange={(n) => patch((d) => ({ ...d, take_profit: n }))} disabled={conditionsLocked} />
              </div>
              <PartHeader ctx={sellCtx} part="always" />
              <LineList lines={doc.always} onChange={(always) => patch((d) => ({ ...d, always }))} ctx={sellCtx} stages={stageNames} addLabel="always line" />
              <StagesEditor
                doc={doc}
                ctx={sellCtx}
                onChange={(stages) => patch((d) => ({ ...d, stages }))}
                onRename={(from, to) => patch((d) => renameStage(d, from, to))}
              />
            </Section>

            <Section n={4} title="Settings">
              <div className="flex flex-wrap items-end gap-3">
                <label className="flex h-8 items-center gap-1.5 text-[11px] text-text-dim">
                  <input
                    type="checkbox"
                    className="accent-accent"
                    checked={doc.reentry != null}
                    disabled={conditionsLocked}
                    onChange={(e) => patch((d) => ({ ...d, reentry: e.target.checked ? (d.reentry ?? { cooldown_sec: 5, max_per_coin: 10 }) : null }))}
                  />
                  {rulePart(registry, 'reentry')?.title ?? 'Buy again'}
                  <InfoTooltip body={`${rulePart(registry, 'reentry')?.summary ?? ''}\n\nExample: ${rulePart(registry, 'reentry')?.example ?? ''}`} />
                </label>
                {doc.reentry && (
                  <>
                    <NumberField reg={registry} label="Wait after a sell" unit="s" placeholder="" value={doc.reentry.cooldown_sec} disabled={conditionsLocked} className="w-20"
                      onChange={(n) => patch((d) => ({ ...d, reentry: { cooldown_sec: n ?? NaN, max_per_coin: d.reentry?.max_per_coin ?? NaN } }))} />
                    <NumberField reg={registry} label="Most buys per coin" integer placeholder="" value={doc.reentry.max_per_coin} disabled={conditionsLocked} className="w-20"
                      onChange={(n) => patch((d) => ({ ...d, reentry: { cooldown_sec: d.reentry?.cooldown_sec ?? NaN, max_per_coin: n ?? NaN } }))} />
                  </>
                )}
              </div>
              <div className="flex flex-wrap items-end gap-3">
                <label className="flex h-8 items-center gap-1.5 text-[11px] text-text-dim">
                  <input
                    type="checkbox"
                    className="accent-accent"
                    checked={doc.exclusive}
                    disabled={conditionsLocked}
                    onChange={(e) => patch((d) => ({ ...d, exclusive: e.target.checked }))}
                  />
                  {rulePart(registry, 'exclusive')?.title ?? 'Exclusive'}
                  <InfoTooltip body={`${rulePart(registry, 'exclusive')?.summary ?? ''}\n\nExample: ${rulePart(registry, 'exclusive')?.example ?? ''}`} />
                </label>
                {doc.exclusive && (
                  <NumberField reg={registry} label="Priority" integer placeholder="0" value={doc.priority} disabled={conditionsLocked} className="w-20"
                    onChange={(n) => patch((d) => ({ ...d, priority: n ?? 0 }))} />
                )}
              </div>
            </Section>
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
            The rule's <code>params</code> as stored (format 2). Edits here update the builder.
            <LabelTip tip={RULE_FIELD_HELP.paramsJson} />
          </p>
        </TabsPanel>
        <TabsPanel value="words">
          <div className="rounded-md border border-white/10 bg-bg-card p-3">
            <RuleSentences doc={doc} reg={registry} />
          </div>
        </TabsPanel>
      </Tabs>

      {renderDryRun?.(currentDraft, canSubmit)}

      {issues.warnings.length > 0 && (
        <ul className="flex flex-col gap-0.5 text-[11px] text-amber-300">
          {issues.warnings.map((w, i) => (
            <li key={i}>⚠ {w}</li>
          ))}
        </ul>
      )}
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
          onClick={() => currentDraft && onSubmit(currentDraft)}
          label={submitting ? 'Saving…' : initial?.id ? 'Save rule' : 'Create rule'}
          title={submitting ? 'Saving…' : initial?.id ? 'Save rule' : 'Create rule'}
        >
          {submitting ? <SpinnerIcon /> : <SaveIcon />}
        </IconButton>
      </div>
    </div>
  );
}
