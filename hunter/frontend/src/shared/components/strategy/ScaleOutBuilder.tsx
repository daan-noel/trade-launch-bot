// Scale-out (tranched exit) editor — a section of stage rows under the global
// Entry/Exit builder. Each stage reuses the exit condition grammar via
// ConditionBuilder (exit-only). Sell size is authored as % of the INITIAL bag
// and stored as `sell_bps`; blank % = remainder/`All` stage (must be last).
//
// Each stage carries the same ⏻ **park** toggle as a condition row: off keeps the
// stage (folded into `params.disabled.scale_out`, still validated per stage) but
// takes it out of the ladder, so it stops running AND stops consuming the ladder's
// stage-count / sell-% budget — the "try the ladder without that tranche" loop.

import { cn } from 'lib/cn';
import { Input } from 'components/ui/Input';
import { IconButton } from 'components/ui/IconButton';
import { PlusIcon } from 'components/ui/icons';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { RULE_FIELD_HELP } from 'lib/strategy/strategyHelp';
import {
  MAX_EXPLICIT_SCALE_STAGES,
  MAX_SCALE_SELL_BPS,
  type ExitStage,
} from 'lib/strategy/ruleParams';
import {
  rowsToSides,
  sidesToRows,
  type RuleConditionRow,
} from 'lib/strategy/ruleConditionRows';
import type { StrategyRegistry } from 'lib/strategy/registry';
import { ConditionBuilder } from './ConditionBuilder';

/** Editor draft for one scale-out stage (rows are the UI SSOT for conditions). */
export interface ScaleStageDraft {
  id: string;
  /** `false` = **parked**: the stage is kept and still validated per stage, but folds
   *  into `params.disabled.scale_out` instead of the live ladder, so the engine never
   *  compiles it and it stops consuming the ladder's stage/bps budget. `undefined`
   *  reads as enabled — a draft from before this field existed is live. */
  enabled?: boolean;
  /** Percent of initial bag (1–99). `null` = remainder stage. */
  sellPct: number | null;
  takeProfit: number | null;
  rows: RuleConditionRow[];
}

/** A stage's live/parked state. `undefined` = enabled (drafts predate the toggle). */
export function scaleStageEnabled(stage: ScaleStageDraft): boolean {
  return stage.enabled !== false;
}

function newId(): string {
  return `so-${Math.random().toString(36).slice(2, 10)}`;
}

export function emptyScaleStage(remainder = false): ScaleStageDraft {
  return {
    id: newId(),
    enabled: true,
    sellPct: remainder ? null : 70,
    takeProfit: remainder ? null : 50,
    rows: [],
  };
}

/**
 * Form `ExitStage[]` → editor drafts (conditions → exit-side rows). The parked
 * ladder (`params.disabled.scale_out`) comes back as drafts too, `enabled: false`,
 * appended after the live ones — the bag stores no ladder position, so a parked
 * stage returns to the end of the list rather than where it was authored.
 */
export function stagesToDrafts(
  stages: ExitStage[] | null | undefined,
  parked?: ExitStage[] | null,
): ScaleStageDraft[] {
  const toDraft = (s: ExitStage, enabled: boolean): ScaleStageDraft => ({
    id: newId(),
    enabled,
    sellPct: s.sell_bps == null ? null : s.sell_bps / 100,
    takeProfit: s.take_profit,
    rows: sidesToRows(undefined, s.conditions).filter((r) => r.side === 'exit'),
  });
  return [
    ...(stages ?? []).map((s) => toDraft(s, true)),
    ...(parked ?? []).map((s) => toDraft(s, false)),
  ];
}

/** Editor drafts → form `ExitStage[]` (empty → null). Pass `enabled: false` for the
 *  parked bag — same conversion, the toggle is only which drafts are picked. */
export function draftsToStages(
  drafts: ScaleStageDraft[],
  enabled = true,
): ExitStage[] | null {
  const stages: ExitStage[] = drafts
    .filter((d) => scaleStageEnabled(d) === enabled)
    .map((d) => {
      const { exit } = rowsToSides(d.rows);
      let sell_bps: number | null = null;
      if (d.sellPct != null && Number.isFinite(d.sellPct)) {
        // Integer bps from whole-percent UI (70 → 7000).
        sell_bps = Math.round(d.sellPct * 100);
      }
      return {
        sell_bps,
        take_profit: d.takeProfit,
        conditions: exit ?? {},
      };
    });
  return stages.length > 0 ? stages : null;
}

export interface ScaleOutBuilderProps {
  stages: ScaleStageDraft[];
  onChange: (stages: ScaleStageDraft[]) => void;
  registry: StrategyRegistry;
  disabled?: boolean;
  /** Allow the ⏻ park toggle. Default true. **The sweep config form passes `false`**:
   *  a sweep run stores its ladder as a bare `ExitStage[]` with no `disabled` bag, so
   *  a parked stage there would be silently dropped when the run is reloaded. */
  allowToggle?: boolean;
}

/**
 * Ordered scale-out ladder. Cap: 3 explicit stages + optional remainder
 * (mirrors backend `MAX_EXPLICIT_SCALE_STAGES`) — counted over the LIVE stages, so
 * parking one frees its slot and its share of the bag.
 */
export function ScaleOutBuilder({
  stages,
  onChange,
  registry,
  disabled,
  allowToggle = true,
}: ScaleOutBuilderProps) {
  // Every budget question is about the LIVE ladder — a parked stage costs no bag,
  // no stage slot, and no remainder. That is what makes the toggle worth having:
  // park a stage and its 70% is free for the one you are trying instead.
  const live = stages.filter(scaleStageEnabled);
  const parkedCount = stages.length - live.length;
  const explicitCount = live.filter((s) => s.sellPct != null).length;
  const hasRemainder = live.some((s) => s.sellPct == null);
  const canAddExplicit = explicitCount < MAX_EXPLICIT_SCALE_STAGES;
  const canAddRemainder = !hasRemainder && stages.length > 0;

  const setStage = (id: string, patch: Partial<ScaleStageDraft>) =>
    onChange(stages.map((s) => (s.id === id ? { ...s, ...patch } : s)));
  const removeStage = (id: string) => onChange(stages.filter((s) => s.id !== id));
  /** Park / un-park a stage. Un-parking an explicit stage re-inserts it **before** a
   *  live remainder — the ladder's one ordering rule ("remainder is last") must not
   *  be broken by the position a parked stage happened to be sitting in. */
  const toggleStage = (id: string) => {
    const target = stages.find((s) => s.id === id);
    if (!target) return;
    const next = { ...target, enabled: !scaleStageEnabled(target) };
    const rest = stages.filter((s) => s.id !== id);
    // Parking, or un-parking a remainder (which belongs at the end anyway): append.
    const remainderAt =
      next.enabled && next.sellPct != null
        ? rest.findIndex((s) => scaleStageEnabled(s) && s.sellPct == null)
        : -1;
    if (remainderAt < 0) {
      onChange([...rest, next]);
      return;
    }
    onChange([...rest.slice(0, remainderAt), next, ...rest.slice(remainderAt)]);
  };
  const addExplicit = () => {
    if (!canAddExplicit || disabled) return;
    // Insert before a live trailing remainder if present.
    const remainderAt = stages.findIndex((s) => scaleStageEnabled(s) && s.sellPct == null);
    if (remainderAt < 0) {
      onChange([...stages, emptyScaleStage(false)]);
      return;
    }
    onChange([
      ...stages.slice(0, remainderAt),
      emptyScaleStage(false),
      ...stages.slice(remainderAt),
    ]);
  };
  const addRemainder = () => {
    if (!canAddRemainder || disabled) return;
    onChange([...stages, emptyScaleStage(true)]);
  };

  // 1-based ordinal of each live explicit stage (parked stages are absent).
  const liveExplicitIndex = new Map<string, number>();
  live
    .filter((s) => s.sellPct != null)
    .forEach((s, i) => liveExplicitIndex.set(s.id, i + 1));

  const sumBps = live.reduce((acc, s) => {
    if (s.sellPct == null || !Number.isFinite(s.sellPct)) return acc;
    return acc + Math.round(s.sellPct * 100);
  }, 0);

  return (
    <div className="flex flex-col gap-2 rounded-md border border-white/10 bg-white/2 p-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span className="inline-flex items-center gap-1 text-[10px] font-bold uppercase tracking-wider text-text-dim/70">
          Scale-out
          <InfoTooltip title={RULE_FIELD_HELP.scaleOut.title} body={RULE_FIELD_HELP.scaleOut.body} />
          {/* A parked stage still renders, so say plainly how many of the visible
              stages the engine will actually run. */}
          {parkedCount > 0 && (
            <span className="font-normal normal-case tracking-normal text-warning/80">
              {live.length} live · {parkedCount} off
            </span>
          )}
        </span>
        <div className="flex items-center gap-1">
          <IconButton
            variant="success"
            size="md"
            onClick={addExplicit}
            disabled={disabled || !canAddExplicit}
            title={
              canAddExplicit
                ? 'Add partial stage'
                : `At most ${MAX_EXPLICIT_SCALE_STAGES} partial stages`
            }
            aria-label="Add partial stage"
          >
            <PlusIcon />
          </IconButton>
          {stages.length > 0 && (
            <button
              type="button"
              disabled={disabled || !canAddRemainder}
              onClick={addRemainder}
              title="Add remainder stage (different exit on the stub)"
              className="rounded border border-white/10 px-1.5 py-0.5 text-[10px] text-text-dim hover:text-text disabled:opacity-40"
            >
              + remainder
            </button>
          )}
        </div>
      </div>

      {stages.length === 0 ? (
        <p className="text-[11px] text-text-dim/60">
          No scale-out — exits close 100%. + to bank a tranche into strength.
        </p>
      ) : (
        <div className="flex flex-col gap-2">
          {stages.map((stage) => (
            <StageCard
              key={stage.id}
              // Numbered by LIVE ladder position — a parked stage has none, because
              // it has no place in the ladder until it is switched back on.
              index={liveExplicitIndex.get(stage.id) ?? null}
              stage={stage}
              registry={registry}
              disabled={disabled}
              allowToggle={allowToggle}
              onPatch={(patch) => setStage(stage.id, patch)}
              onRemove={() => removeStage(stage.id)}
              onToggle={() => toggleStage(stage.id)}
            />
          ))}
          <p className="text-[10px] text-text-dim/60">
            Banked {Math.round(sumBps / 100)}% of initial
            {sumBps > MAX_SCALE_SELL_BPS ? (
              <span className="text-red"> — sum must be ≤ {MAX_SCALE_SELL_BPS / 100}%</span>
            ) : live.length === 0 ? (
              <span className="text-warning"> — every stage is off; exits close 100%</span>
            ) : (
              <> · stub closes via {hasRemainder ? 'remainder stage' : 'global exit'}</>
            )}
          </p>
        </div>
      )}
    </div>
  );
}

function StageCard({
  index,
  stage,
  registry,
  disabled,
  allowToggle,
  onPatch,
  onRemove,
  onToggle,
}: {
  /** 1-based position in the LIVE ladder; `null` for a parked stage (no position). */
  index: number | null;
  stage: ScaleStageDraft;
  registry: StrategyRegistry;
  disabled?: boolean;
  allowToggle: boolean;
  onPatch: (patch: Partial<ScaleStageDraft>) => void;
  onRemove: () => void;
  onToggle: () => void;
}) {
  const isRemainder = stage.sellPct == null;
  const on = scaleStageEnabled(stage);
  return (
    <div
      className={cn(
        'flex flex-col gap-1.5 rounded border border-white/10 bg-surface/40 p-2',
        // Parked: readable but plainly inert. Controls stay enabled on purpose —
        // tweaking a stage before switching it back on is the whole workflow.
        !on && 'border-dashed opacity-45',
      )}
    >
      <div className="flex flex-wrap items-end gap-2">
        <span className="pb-1.5 text-[10px] font-semibold uppercase text-text-dim/70">
          {isRemainder ? 'Remainder' : index != null ? `Stage ${index}` : 'Stage'}
          {!on && <span className="ml-1 normal-case text-warning/80">off</span>}
        </span>
        {!isRemainder && (
          <label className="flex flex-col gap-0.5 text-[10px] text-text-dim">
            Sell %
            <Input
              fieldSize="sm"
              numeric
              unit="%"
              numericValue={stage.sellPct}
              onNumericChange={(n) => onPatch({ sellPct: n })}
              disabled={disabled}
              className="w-20"
            />
          </label>
        )}
        <label className="flex flex-col gap-0.5 text-[10px] text-text-dim">
          Stage TP %
          <Input
            fieldSize="sm"
            numeric
            unit="%"
            numericValue={stage.takeProfit}
            onNumericChange={(n) => onPatch({ takeProfit: n })}
            disabled={disabled}
            className="w-20"
          />
        </label>
        <div className="ml-auto flex shrink-0 items-center gap-0.5 pb-1.5">
          {allowToggle && (
          <button
            type="button"
            onClick={onToggle}
            disabled={disabled}
            title={
              on
                ? 'Turn off — keep the stage but stop running it (frees its % and stage slot)'
                : 'Turn on — put this stage back in the ladder'
            }
            aria-label="Toggle stage"
            aria-pressed={on}
            className={cn(
              'px-1 transition-colors disabled:opacity-40',
              on ? 'text-text-dim/60 hover:text-warning' : 'text-warning hover:text-text',
            )}
          >
            ⏻
          </button>
          )}
          <button
            type="button"
            onClick={onRemove}
            disabled={disabled}
            title="Remove stage"
            aria-label="Remove stage"
            className="px-1 text-text-dim hover:text-red disabled:opacity-40"
          >
            ✕
          </button>
        </div>
      </div>
      <ConditionBuilder
        rows={stage.rows}
        onChange={(rows) => onPatch({ rows })}
        registry={registry}
        disabled={disabled}
        sides={['exit']}
        allowFlip={false}
        // No per-ROW park toggle here: a stage's `conditions` have no `disabled` bag
        // of their own, so a parked row would vanish on save. Park the whole stage
        // with the ⏻ above instead.
        allowToggle={false}
        sideTitles={{ exit: 'conditions' }}
      />
    </div>
  );
}
