// The two documents a grouped sweep carries beside its axes: the run's **tags** (what an
// axis `@tag` reads, written onto the promoted fingerprint) and the Pass-2 **stage
// plan**. Each edits its wire JSON through the rule editor's own components.

import { Input } from 'components/ui/Input';
import { Badge } from 'components/ui/Badge';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { TagsEditor } from 'components/strategy/TagsEditor';
import { StagesEditor } from 'components/strategy/rule/StagesEditor';
import type { CondContext } from 'components/strategy/rule/CondRow';
import { emptyRuleDoc, renameStage, type Stage } from 'lib/strategy/ruleDoc';
import type { StrategyRegistry } from 'lib/strategy/registry';
import { SWEEP_FIELD_HELP } from 'lib/strategy/strategyHelp';
import { tagsFromJson, tagsToJson, validateTags, type TagDef } from 'lib/strategy/tagsDoc';
import { STAGE_PLAN_TEMPLATE, stagePlanErrors, stagesFromWire, stagesToWire } from './stagePlan';
import { useWireDraft } from './useWireDraft';

/** The run's tags document. */
export function RunTagsEditor({
  tags,
  onChange,
  registry,
  readTags,
  disabled,
}: {
  /** Wire `tags` JSON. */
  tags: unknown;
  onChange: (tags: Record<string, unknown>) => void;
  registry: StrategyRegistry;
  /** The tag names the axes read: flags any the document lacks. */
  readTags: readonly string[];
  disabled?: boolean;
}) {
  const [draft, setDraft] = useWireDraft<TagDef[]>(tags, tagsFromJson, tagsToJson, (w) =>
    onChange(w as Record<string, unknown>),
  );
  const errors = validateTags(draft, registry);
  const missing = readTags.filter((t) => !draft.some((d) => d.name === t));
  return (
    <div className="flex flex-col gap-2">
      <span className="inline-flex items-center gap-1 text-[11px] text-text-dim">
        {SWEEP_FIELD_HELP.tags.title}
        <InfoTooltip title={SWEEP_FIELD_HELP.tags.title} body={SWEEP_FIELD_HELP.tags.body} />
        <span className="text-text-dim/60">
          {readTags.length ? `the axes read @${readTags.join(', @')}` : 'no axis reads a tag: the run sends none'}
        </span>
      </span>
      <TagsEditor tags={draft} onChange={setDraft} reg={registry} disabled={disabled} />
      {[...missing.map((t) => `An axis reads @${t}, which no tag here defines`), ...errors].map((e) => (
        <p key={e} className="text-[11px] text-red">
          {e}
        </p>
      ))}
    </div>
  );
}

/** The Pass-2 stage plan and its top-K. */
export function StagePlanEditor({
  plan,
  onChange,
  topK,
  onTopK,
  registry,
  disabled,
}: {
  /** Wire `stages` JSON; empty = Pass 2 off. */
  plan: unknown[];
  onChange: (plan: unknown[]) => void;
  topK: number;
  onTopK: (k: number) => void;
  registry: StrategyRegistry;
  disabled?: boolean;
}) {
  const [stages, setStages] = useWireDraft<Stage[]>(plan, stagesFromWire, stagesToWire, (w) => onChange(w as unknown[]));
  const doc = { ...emptyRuleDoc(), stages };
  // After the buy, with no signals: a plan line reads our position only.
  const ctx: CondContext = { reg: registry, tags: [], signals: [], beforeBuy: false, disabled };
  const errors = stagePlanErrors(stages, registry);
  return (
    <div className="flex flex-col gap-2">
      <span className="inline-flex items-center gap-1 text-[11px] text-text-dim">
        {SWEEP_FIELD_HELP.stagePlans.title}
        <InfoTooltip title={SWEEP_FIELD_HELP.stagePlans.title} body={SWEEP_FIELD_HELP.stagePlans.body} />
        <span className="text-text-dim/60">
          {stages.length ? 'each top-K combo keeps the better of this plan and its own exit' : 'off: add a stage to try one'}
        </span>
      </span>
      {stages.length === 0 && (
        <div>
          <button
            type="button"
            disabled={disabled}
            onClick={() => onChange(STAGE_PLAN_TEMPLATE.stages)}
            className="rounded border border-white/10 px-1.5 py-0.5 text-[10px] text-text-dim hover:text-text disabled:opacity-40"
          >
            Start from: {STAGE_PLAN_TEMPLATE.label}
          </button>
        </div>
      )}
      <StagesEditor
        doc={doc}
        ctx={ctx}
        onChange={setStages}
        onRename={(from, to) => setStages(renameStage(doc, from, to).stages)}
      />
      {errors.map((e) => (
        <p key={e} className="text-[11px] text-red">
          {e}
        </p>
      ))}
      {stages.length > 0 && (
        <div className="flex flex-wrap items-end gap-3">
          <label className="flex flex-col gap-0.5 text-[11px] text-text-dim">
            <span className="inline-flex items-center gap-1">
              Top-K / group
              <InfoTooltip title={SWEEP_FIELD_HELP.stagePlansTopK.title} body={SWEEP_FIELD_HELP.stagePlansTopK.body} />
            </span>
            <Input
              type="number"
              min={1}
              max={50}
              value={topK}
              onChange={(e) => onTopK(Math.max(1, Number(e.target.value) || 3))}
              disabled={disabled}
              className="w-20"
            />
          </label>
          <Badge variant="info">own exit vs. this plan, per combo</Badge>
        </div>
      )}
    </div>
  );
}
