// Scale-out (tranched exit) editor — a section of stage rows under the global
// Entry/Exit builder. Each stage reuses the exit condition grammar via
// ConditionBuilder (exit-only). Sell size is authored as % of the INITIAL bag
// and stored as `sell_bps`; blank % = remainder/`All` stage (must be last).

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
  /** Percent of initial bag (1–99). `null` = remainder stage. */
  sellPct: number | null;
  takeProfit: number | null;
  rows: RuleConditionRow[];
}

function newId(): string {
  return `so-${Math.random().toString(36).slice(2, 10)}`;
}

export function emptyScaleStage(remainder = false): ScaleStageDraft {
  return {
    id: newId(),
    sellPct: remainder ? null : 70,
    takeProfit: remainder ? null : 50,
    rows: [],
  };
}

/** Form `ExitStage[]` → editor drafts (conditions → exit-side rows). */
export function stagesToDrafts(stages: ExitStage[] | null | undefined): ScaleStageDraft[] {
  if (!stages || stages.length === 0) return [];
  return stages.map((s) => ({
    id: newId(),
    sellPct: s.sell_bps == null ? null : s.sell_bps / 100,
    takeProfit: s.take_profit,
    rows: sidesToRows(undefined, s.conditions).filter((r) => r.side === 'exit'),
  }));
}

/** Editor drafts → form `ExitStage[]` (empty → null). */
export function draftsToStages(drafts: ScaleStageDraft[]): ExitStage[] | null {
  if (drafts.length === 0) return null;
  const stages: ExitStage[] = drafts.map((d) => {
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
  return stages;
}

export interface ScaleOutBuilderProps {
  stages: ScaleStageDraft[];
  onChange: (stages: ScaleStageDraft[]) => void;
  registry: StrategyRegistry;
  disabled?: boolean;
}

/**
 * Ordered scale-out ladder. Cap: 3 explicit stages + optional remainder
 * (mirrors backend `MAX_EXPLICIT_SCALE_STAGES`).
 */
export function ScaleOutBuilder({ stages, onChange, registry, disabled }: ScaleOutBuilderProps) {
  const explicitCount = stages.filter((s) => s.sellPct != null).length;
  const hasRemainder = stages.some((s) => s.sellPct == null);
  const canAddExplicit = explicitCount < MAX_EXPLICIT_SCALE_STAGES;
  const canAddRemainder = !hasRemainder && stages.length > 0;

  const setStage = (id: string, patch: Partial<ScaleStageDraft>) =>
    onChange(stages.map((s) => (s.id === id ? { ...s, ...patch } : s)));
  const removeStage = (id: string) => onChange(stages.filter((s) => s.id !== id));
  const addExplicit = () => {
    if (!canAddExplicit || disabled) return;
    // Insert before a trailing remainder if present.
    if (hasRemainder) {
      const rem = stages[stages.length - 1];
      onChange([...stages.slice(0, -1), emptyScaleStage(false), rem]);
    } else {
      onChange([...stages, emptyScaleStage(false)]);
    }
  };
  const addRemainder = () => {
    if (!canAddRemainder || disabled) return;
    onChange([...stages, emptyScaleStage(true)]);
  };

  const sumBps = stages.reduce((acc, s) => {
    if (s.sellPct == null || !Number.isFinite(s.sellPct)) return acc;
    return acc + Math.round(s.sellPct * 100);
  }, 0);

  return (
    <div className="flex flex-col gap-2 rounded-md border border-white/10 bg-white/2 p-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span className="inline-flex items-center gap-1 text-[10px] font-bold uppercase tracking-wider text-text-dim/70">
          Scale-out
          <InfoTooltip title={RULE_FIELD_HELP.scaleOut.title} body={RULE_FIELD_HELP.scaleOut.body} />
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
          {stages.map((stage, i) => (
            <StageCard
              key={stage.id}
              index={i}
              stage={stage}
              registry={registry}
              disabled={disabled}
              onPatch={(patch) => setStage(stage.id, patch)}
              onRemove={() => removeStage(stage.id)}
            />
          ))}
          <p className="text-[10px] text-text-dim/60">
            Banked {Math.round(sumBps / 100)}% of initial
            {sumBps > MAX_SCALE_SELL_BPS ? (
              <span className="text-red"> — sum must be ≤ {MAX_SCALE_SELL_BPS / 100}%</span>
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
  onPatch,
  onRemove,
}: {
  index: number;
  stage: ScaleStageDraft;
  registry: StrategyRegistry;
  disabled?: boolean;
  onPatch: (patch: Partial<ScaleStageDraft>) => void;
  onRemove: () => void;
}) {
  const isRemainder = stage.sellPct == null;
  return (
    <div className="flex flex-col gap-1.5 rounded border border-white/10 bg-surface/40 p-2">
      <div className="flex flex-wrap items-end gap-2">
        <span className="pb-1.5 text-[10px] font-semibold uppercase text-text-dim/70">
          {isRemainder ? 'Remainder' : `Stage ${index + 1}`}
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
        <button
          type="button"
          onClick={onRemove}
          disabled={disabled}
          title="Remove stage"
          aria-label="Remove stage"
          className="ml-auto px-1 pb-1.5 text-text-dim hover:text-red disabled:opacity-40"
        >
          ✕
        </button>
      </div>
      <ConditionBuilder
        rows={stage.rows}
        onChange={(rows) => onPatch({ rows })}
        registry={registry}
        disabled={disabled}
        sides={['exit']}
        allowFlip={false}
        // No park toggle here: a stage's `conditions` have no `disabled` bag to fold
        // into, so a parked row would vanish on save. Remove the stage instead.
        allowToggle={false}
        sideTitles={{ exit: 'conditions' }}
      />
    </div>
  );
}
