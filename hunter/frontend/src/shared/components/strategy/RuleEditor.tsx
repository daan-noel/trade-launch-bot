import { useMemo, useState, type ReactNode } from 'react';

import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { IconButton } from 'components/ui/IconButton';
import { LockIcon, SaveIcon, SpinnerIcon, UnlockIcon } from 'components/ui/icons';
import { Button } from 'components/ui/Button';
import { Tabs, TabsList, TabsTrigger, TabsPanel } from 'components/ui/Tabs';
import { cn } from 'lib/cn';
import { useStrategyRegistry, type StrategyRegistry } from 'lib/strategy/registry';
import { copyEditorRegistry, copyEpisodeDoc, copyViewParams, withCopyFlag } from 'lib/strategy/copyRule';
import { emptyRuleDoc, ruleDocFromJson, ruleDocToJson, type RuleDoc } from 'lib/strategy/ruleDoc';
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
import { RuleChain } from './rule/RuleChain';
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
  /** Copy board: the picker is the copy readings, and a sell can mirror his bag. */
  scope?: 'metric' | 'copy';
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
 *  reason shown, so it is never silently overwritten by a save. A copy shortcut
 *  opens as the episode it expands to. */
function initialDoc(initial: StrategyRule | undefined, scope: 'metric' | 'copy'): { doc: RuleDoc; error: string | null } {
  if (!initial) return { doc: scope === 'copy' ? copyEpisodeDoc() : emptyRuleDoc(), error: null };
  try {
    const raw = scope === 'copy' ? copyViewParams(initial.params) : initial.params;
    return { doc: ruleDocFromJson(raw), error: null };
  } catch (e) {
    return { doc: emptyRuleDoc(), error: e instanceof Error ? e.message : String(e) };
  }
}

function RuleEditorInner({
  initial,
  onSubmit,
  onCancel,
  submitting,
  error,
  renderDryRun,
  registry: fullRegistry,
  scope = 'metric',
}: RuleEditorProps & { registry: StrategyRegistry }) {
  const registry = scope === 'copy' ? copyEditorRegistry(fullRegistry) : fullRegistry;
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

  const [{ doc: firstDoc, error: loadError }] = useState(() => initialDoc(initial, scope));
  const [doc, setDoc] = useState<RuleDoc>(firstDoc);
  const [tab, setTab] = useState<'builder' | 'json' | 'words'>('builder');
  const storedParams = (d: RuleDoc) => (scope === 'copy' ? withCopyFlag(ruleDocToJson(d)) : ruleDocToJson(d));
  const [jsonText, setJsonText] = useState(() => JSON.stringify(storedParams(firstDoc), null, 2));
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

  const syncFromJson = (text: string) => {
    setJsonText(text);
    try {
      setDoc(ruleDocFromJson(scope === 'copy' ? copyViewParams(JSON.parse(text)) : JSON.parse(text)));
      setJsonError(null);
    } catch (e) {
      setJsonError(e instanceof Error ? e.message : 'invalid JSON');
    }
  };
  const switchTab = (next: string) => {
    if (next === 'json') setJsonText(JSON.stringify(storedParams(doc), null, 2));
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
        params: storedParams(doc),
        tags: labels,
      }
    : null;

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-start gap-4 rounded-md border border-white/10 bg-white/3 px-3 py-2">
        <div className="flex min-w-0 flex-1 flex-col gap-2">
          <div className="flex items-end gap-3">
            <label className="flex min-w-0 flex-1 flex-col gap-1 text-[11px] text-text-dim">
              <LabelTip tip={RULE_FIELD_HELP.name}>Name</LabelTip>
              <Input fieldSize="sm" value={ruleName} onChange={(e) => setRuleName(e.target.value)} />
            </label>
            {scope === 'copy' && (
              <label className="flex w-52 shrink-0 flex-col gap-1 text-[11px] text-text-dim">
                <span>Follow</span>
                <Select
                  fieldSize="sm"
                  value={doc.follow ?? 'all'}
                  disabled={conditionsLocked}
                  onChange={(e) =>
                    patch((d) => ({ ...d, follow: e.target.value === 'bought' ? 'bought' : 'all' }))
                  }
                >
                  <option value="bought">the wallet we bought</option>
                  <option value="all">every target wallet</option>
                </Select>
              </label>
            )}
            <div className="flex w-36 shrink-0 flex-col gap-1 text-[11px] text-text-dim">
              <LabelTip tip={RULE_FIELD_HELP.mode}>Mode</LabelTip>
              <div className="flex items-center gap-1">
                <Select fieldSize="sm" className="w-24" value={mode} onChange={(e) => setMode(e.target.value as TradeMode)} disabled={modeLocked}>
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
          </div>
          <div className="flex min-w-0 flex-col gap-1 text-[11px] text-text-dim">
            <LabelTip tip={RULE_FIELD_HELP.tags}>Labels</LabelTip>
            <RuleTagsInput value={labels} onChange={setLabels} suggestions={labelSuggestions} />
          </div>
        </div>
        <div className="flex min-w-0 flex-1 flex-col gap-1 text-[11px] text-text-dim">
          <LabelTip tip={RULE_FIELD_HELP.fingerprint}>
            Watch {conditionsLocked && <span className="text-text-dim/60">(locked: rule live)</span>}
          </LabelTip>
          <FingerprintPicker
            value={fingerprintId}
            onChange={setFingerprintId}
            disabled={conditionsLocked}
            pool={scope === 'copy' ? 'copy' : 'rules'}
          />
          <FingerprintParamsById id={fingerprintId} />
          {fingerprintId && (
            <p className="text-[11px] text-text-dim">
              Tags:{' '}
              {fpTags.length ? fpTags.map((t) => <code key={t} className="mr-1">@{t}</code>) : <span className="italic">none (a metric that needs a tag reads nothing)</span>}
            </p>
          )}
        </div>
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
          <RuleChain
            doc={doc}
            patch={patch}
            registry={registry}
            locked={conditionsLocked}
            buySol={buySol}
            onBuySol={setBuySol}
            maxConcurrent={maxConcurrent}
            onMaxConcurrent={setMaxConcurrent}
            maxTotal={maxTotal}
            onMaxTotal={setMaxTotal}
            tags={fpTags}
            bagMirror={scope === 'copy'}
          />
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
          <RuleSentences doc={doc} reg={registry} watch={fingerprints.find((f) => f.id === fingerprintId)?.name} />
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
