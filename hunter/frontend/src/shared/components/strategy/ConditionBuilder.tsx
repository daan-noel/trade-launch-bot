// Row-based rule condition builder — the rule-editor analogue of the sweep's
// `GenericAxisBuilder`. Conditions are authored one row at a time (add with `+`,
// remove with `✕`) across an Entry column and an Exit column, instead of the old
// full-registry wall that painted every group × metric whether used or not.
//
// The trailing **window is a per-row field**, so the same metric at two windows is
// just two rows — they fold into two `GroupConditions` instances of the one group
// (`rowsToSide`), the engine's multi-window-per-group model.

import { type CSSProperties, type ReactNode } from 'react';
import { cn } from 'lib/cn';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { IconButton } from 'components/ui/IconButton';
import { PlusIcon } from 'components/ui/icons';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { type MetricUnit, type StrategyRegistry } from 'lib/strategy/registry';
import { metricColorStyle } from 'lib/strategy/metricColors';
import { isPnlAdvancedMetric } from 'lib/strategy/validate';
import { GROUP_HELP, METRIC_HELP, metricHelpBody, SIDE_HELP } from 'lib/strategy/strategyHelp';
import {
  armAbovePctOrphanError,
  duplicateConditionRowError,
  newRuleConditionRow,
  ruleConditionRowError,
  ruleRowIsTrailing,
  ruleRowNeedsWindow,
  type RuleConditionRow,
  type RuleConditionSide,
} from 'lib/strategy/ruleConditionRows';
import { ConditionInput } from './ConditionInput';

export interface ConditionBuilderProps {
  rows: RuleConditionRow[];
  onChange: (rows: RuleConditionRow[]) => void;
  registry: StrategyRegistry;
  disabled?: boolean;
}

/**
 * The two-column (entry · exit) condition builder. Add a condition per column,
 * pick group → metric → window → grammar; the `⇄` flips a row to the other side.
 */
export function ConditionBuilder({ rows, onChange, registry, disabled }: ConditionBuilderProps) {
  const dupErr = duplicateConditionRowError(rows);
  const armErr = armAbovePctOrphanError(rows);

  const setRow = (id: string, patch: Partial<RuleConditionRow>) =>
    onChange(rows.map((r) => (r.id === id ? { ...r, ...patch } : r)));
  const removeRow = (id: string) => onChange(rows.filter((r) => r.id !== id));
  const addRow = (side: RuleConditionSide) => onChange([...rows, newRuleConditionRow(side)]);
  const flipSide = (id: string) =>
    onChange(
      rows.map((r) =>
        r.id === id ? { ...r, side: r.side === 'entry' ? 'exit' : 'entry' } : r,
      ),
    );

  return (
    <div className="flex flex-col gap-2">
      <div className="grid grid-cols-1 gap-2 md:grid-cols-2">
        {(['entry', 'exit'] as RuleConditionSide[]).map((side) => (
          <ConditionColumn
            key={side}
            side={side}
            rows={rows.filter((r) => r.side === side)}
            registry={registry}
            disabled={disabled}
            onAdd={() => addRow(side)}
            onPatch={setRow}
            onRemove={removeRow}
            onFlip={flipSide}
          />
        ))}
      </div>
      {dupErr && <p className="text-[11px] text-red">{dupErr}</p>}
      {armErr && <p className="text-[11px] text-red">{armErr}</p>}
    </div>
  );
}

function ConditionColumn({
  side,
  rows,
  registry,
  disabled,
  onAdd,
  onPatch,
  onRemove,
  onFlip,
}: {
  side: RuleConditionSide;
  rows: RuleConditionRow[];
  registry: StrategyRegistry;
  disabled?: boolean;
  onAdd: () => void;
  onPatch: (id: string, patch: Partial<RuleConditionRow>) => void;
  onRemove: (id: string) => void;
  onFlip: (id: string) => void;
}) {
  return (
    <div className="flex flex-col gap-1.5 rounded-md border border-white/10 bg-white/2 p-2">
      <div className="flex items-center justify-between gap-1.5">
        <span className="inline-flex items-center gap-1 text-[10px] font-bold uppercase tracking-wider text-text-dim/70">
          {side}
          <InfoTooltip title={SIDE_HELP[side].title} body={SIDE_HELP[side].body} />
        </span>
        <IconButton
          variant="success"
          size="md"
          onClick={onAdd}
          disabled={disabled}
          title="Add condition"
          aria-label="Add condition"
        >
          <PlusIcon />
        </IconButton>
      </div>

      {rows.length === 0 ? (
        <div className="rounded border border-dashed border-white/10 px-2 py-3 text-center text-[11px] text-text-dim/50">
          No {side} conditions — + to add one
        </div>
      ) : (
        <div className="flex flex-col gap-1.5">
          {rows.map((row) => (
            <ConditionRow
              key={row.id}
              row={row}
              registry={registry}
              disabled={disabled}
              onPatch={(patch) => onPatch(row.id, patch)}
              onRemove={() => onRemove(row.id)}
              onFlip={() => onFlip(row.id)}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function ConditionRow({
  row,
  registry,
  disabled,
  onPatch,
  onRemove,
  onFlip,
}: {
  row: RuleConditionRow;
  registry: StrategyRegistry;
  disabled?: boolean;
  onPatch: (patch: Partial<RuleConditionRow>) => void;
  onRemove: () => void;
  onFlip: () => void;
}) {
  const err = ruleConditionRowError(row, registry);
  const group = registry.groups.find((g) => g.name === row.group);
  const metric = group?.metrics.find((m) => m.name === row.metric);
  const needsWindow = ruleRowNeedsWindow(row, registry);
  const isTrailing = ruleRowIsTrailing(row);
  const armValue = row.strict?.arm_above_pct;
  const armText = armValue == null ? '' : String(armValue);

  const onArm = (text: string) => {
    if (text.trim() === '') {
      if (!row.strict) return;
      const { arm_above_pct: _drop, ...rest } = row.strict;
      onPatch({ strict: rest });
      return;
    }
    const v = Number(text);
    if (!Number.isFinite(v)) return;
    onPatch({ strict: { ...row.strict, arm_above_pct: v } });
  };
  // Only drives the input's unit adornment; the field is disabled until a metric is
  // chosen, so the fallback is never user-visible.
  const unit: MetricUnit = metric?.unit ?? 'seconds';

  const onGroup = (name: string) => {
    const g = registry.groups.find((gg) => gg.name === name);
    // Auto-pick the group's first metric + reset window so a static pick drops it.
    onPatch({ group: name, metric: g?.metrics[0]?.name ?? '', window: '', arms: [] });
  };

  // Tint the row border from the metric hue (+ op shade) once fully picked, matching
  // the sweep builder's per-metric coloring.
  const tint: CSSProperties | undefined =
    !err && row.metric
      ? (() => {
          const c = metricColorStyle({
            hue: metric?.hue,
            group: row.group,
            metric: row.metric,
            operator: row.arms[0]?.[0]?.operator,
          });
          return { borderColor: c.border, backgroundColor: c.background };
        })()
      : undefined;

  return (
    <div
      className={cn(
        'flex flex-wrap items-center gap-1.5 rounded-md border px-2 py-1.5',
        err ? 'border-red/40 bg-red/5' : tint ? '' : 'border-white/10 bg-surface',
      )}
      style={tint}
    >
      <Cell label="group" tip={row.group && GROUP_HELP[row.group] ? GROUP_HELP[row.group] : undefined}>
        <Select
          fieldSize="sm"
          value={row.group}
          disabled={disabled}
          onChange={(e) => onGroup(e.target.value)}
          className="w-32"
        >
          <option value="">group…</option>
          {registry.groups
            // Hide position-scoped (exit-only) groups from an entry row.
            .filter((g) => !(row.side === 'entry' && g.scope === 'position'))
            .map((g) => (
              <option key={g.name} value={g.name}>
                {g.name}
              </option>
            ))}
        </Select>
      </Cell>

      <Cell
        label="metric"
        tip={
          row.metric
            ? {
                title: METRIC_HELP[row.metric]?.title ?? row.metric,
                body: metricHelpBody(row.metric, metric),
              }
            : undefined
        }
      >
        <Select
          fieldSize="sm"
          value={row.metric}
          disabled={disabled || !group}
          onChange={(e) => onPatch({ metric: e.target.value })}
          className="w-28"
        >
          <option value="">metric…</option>
          {group?.metrics.map((m) => (
            <option key={m.name} value={m.name}>
              {isPnlAdvancedMetric(group.name, m.name) ? `${m.name} (advanced)` : m.name}
            </option>
          ))}
        </Select>
      </Cell>

      {needsWindow && (
        <Cell label="window s">
          <Input
            fieldSize="sm"
            type="number"
            min={0.5}
            step={0.5}
            value={row.window}
            disabled={disabled}
            onChange={(e) => onPatch({ window: e.target.value })}
            placeholder="10"
            className="w-16"
          />
        </Cell>
      )}

      {isTrailing && (
        <Cell
          label="arm ≥ %"
          tip={METRIC_HELP.arm_above_pct ? { title: METRIC_HELP.arm_above_pct.title, body: METRIC_HELP.arm_above_pct.body } : undefined}
        >
          <Input
            fieldSize="sm"
            type="number"
            min={0}
            step={0.5}
            value={armText}
            disabled={disabled}
            onChange={(e) => onArm(e.target.value)}
            placeholder="off"
            className="w-16"
          />
        </Cell>
      )}

      <Cell label="condition" grow>
        <ConditionInput
          value={row.arms}
          onChange={(arms) => onPatch({ arms })}
          unit={unit}
          eqTolerance={metric?.eq_tolerance}
          disabled={disabled || !metric}
          className="min-w-40"
        />
      </Cell>

      <div className="ml-auto flex shrink-0 items-center gap-0.5">
        <button
          type="button"
          onClick={onFlip}
          disabled={disabled}
          title={`Move to ${row.side === 'entry' ? 'exit' : 'entry'}`}
          aria-label="Flip side"
          className="px-1 text-text-dim/60 transition-colors hover:text-text disabled:opacity-40"
        >
          ⇄
        </button>
        <button
          type="button"
          onClick={onRemove}
          disabled={disabled}
          title="Remove condition"
          aria-label="Remove condition"
          className="px-1 text-text-dim transition-colors hover:text-red disabled:opacity-40"
        >
          ✕
        </button>
      </div>

      {err && <span className="w-full text-[11px] text-red">{err}</span>}
    </div>
  );
}

/** A labelled control cell (tiny caption above the control). */
function Cell({
  label,
  grow,
  tip,
  children,
}: {
  label: string;
  grow?: boolean;
  tip?: { title: string; body: string };
  children: ReactNode;
}) {
  return (
    <div className={cn('flex flex-col gap-0.5', grow && 'min-w-40 flex-1')}>
      <span className="inline-flex items-center gap-0.5 text-[8px] uppercase tracking-wider text-text-dim/50">
        {label}
        {tip && <InfoTooltip title={tip.title} body={tip.body} side="top" />}
      </span>
      {children}
    </div>
  );
}
