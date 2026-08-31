// Row-based rule condition builder — the rule-editor analogue of the sweep's
// `GenericAxisBuilder`. Conditions are authored one row at a time (add with `+`,
// remove with `✕`). Buy is one decision: **Event** (completing-print AND) beside
// **Filters** (AND on that print), with **ways to sell** stacked as OR of AND.
// An empty Event is today's level-AND (filters every print).
//
// The trailing **window is a per-row field**, so the same metric at two windows is
// just two rows — they fold into two `GroupConditions` instances of the one group
// (`rowsToSide`), the engine's multi-window-per-group model. Scale-out stages pass
// `sides={['exit']}` and stay a flat object-form OR.

import { type CSSProperties, type ReactNode } from 'react';
import { cn } from 'lib/cn';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { IconButton } from 'components/ui/IconButton';
import { PlusIcon } from 'components/ui/icons';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { ToggleGroup } from 'components/ui/ToggleGroup';
import { type MetricUnit, type StrategyRegistry } from 'lib/strategy/registry';
import { metricColorStyle } from 'lib/strategy/metricColors';
import { isPnlAdvancedMetric } from 'lib/strategy/validate';
import {
  METRIC_HELP,
  metricHelpBody,
  groupHelpTip,
  SIDE_HELP,
  STRICT_PARAM_HELP,
} from 'lib/strategy/strategyHelp';
import {
  armAbovePctOrphanError,
  duplicateConditionRowError,
  exitClauseOrder,
  flipConditionSide,
  flipSideLabel,
  newExitClauseId,
  newExitClauseRow,
  newRuleConditionRow,
  parkedSideWarnings,
  ruleConditionRowError,
  ruleRowEnabled,
  ruleRowIsTrailing,
  ruleRowSliceSpec,
  ruleRowNeedsSlice,
  ruleRowNeedsWindow,
  ruleRowUnit,
  setRowInstanceStrict,
  type RuleConditionRow,
  type RuleConditionSide,
} from 'lib/strategy/ruleConditionRows';
import {
  sliceSizeParam,
  isDiscreteUnit,
  sizeParam as sizeParamOf,
  unitSuffix as windowUnitSuffix,
  WINDOW_LAG_PARAM,
  WINDOW_UNITS,
  withoutSliceAxis,
  type WindowUnit,
} from 'lib/strategy/windowSpec';
import { ConditionInput } from './ConditionInput';

export interface ConditionBuilderProps {
  rows: RuleConditionRow[];
  onChange: (rows: RuleConditionRow[]) => void;
  registry: StrategyRegistry;
  disabled?: boolean;
  /** Which side columns to show. Default both. Scale-out stages use `['exit']`. */
  sides?: RuleConditionSide[];
  /** Allow the ⇄ flip between columns. Default true when both sides are shown. */
  allowFlip?: boolean;
  /** Completing-print lock. `"slot"` = fire the event once per slot. `null` =
   *  event AND filters on every print. The builder auto-sets `"slot"` when the
   *  first event row is added and clears it when the last is removed. */
  entryLock?: 'slot' | null;
  onEntryLockChange?: (lock: 'slot' | null) => void;
  /** Allow the ⏻ park toggle. Default true. **Scale-out stages pass `false`**: a
   *  stage's `conditions` are their own nested `SideConditions` with no `disabled`
   *  bag, so a parked row there would be silently dropped on save. Park the whole
   *  stage instead (remove it) until the bag nests per stage. */
  allowToggle?: boolean;
  /** Optional column title override (e.g. exit → "conditions"). */
  sideTitles?: Partial<Record<RuleConditionSide, string>>;
  /**
   * Group exit rows into "ways to sell" (OR of AND). Default: on when both
   * entry and exit columns show (the rule editor); off for scale-out stages
   * (`sides={['exit']}`), which stay object-form.
   */
  exitWays?: boolean;
}

/**
 * The two-column (entry · exit) condition builder. Add a condition per column,
 * pick group → metric → window → grammar; the `⇄` flips a row to the other side and
 * the ⏻ **parks** it (kept and still validated, but folded into `params.disabled`
 * so the engine never compiles it — the "try this rule without that gate" loop).
 * Pass `sides={['exit']}` for a single exit-only column (scale-out stages).
 */
export function ConditionBuilder({
  rows,
  onChange,
  registry,
  disabled,
  sides = ['entry', 'exit'],
  allowFlip,
  allowToggle = true,
  sideTitles,
  exitWays,
  entryLock = null,
  onEntryLockChange,
}: ConditionBuilderProps) {
  const dupErr = duplicateConditionRowError(rows);
  const armErr = armAbovePctOrphanError(rows);
  // Parking the LAST condition of a side silently rewrites the rule (filters ⇒
  // fingerprint + event; event ⇒ no completing-print gate; exit ⇒ TP/SL/death) —
  // warn, don't block.
  // (Only for the columns this builder actually shows — a scale-out stage renders
  // the exit column alone and has its own "stage needs a way to fire" check.)
  const parkedWarnings = parkedSideWarnings(
    rows.filter((r) => sides.includes(r.side) || (sides.includes('entry') && r.side === 'entry_event')),
  );
  const canFlip = allowFlip ?? sides.length > 1;
  const groupExitWays = exitWays ?? (sides.includes('entry') && sides.includes('exit'));

  const setRow = (id: string, patch: Partial<RuleConditionRow>) =>
    onChange(rows.map((r) => (r.id === id ? { ...r, ...patch } : r)));
  // A strict param (`arm_above_pct`) belongs to the GROUP INSTANCE, not the row:
  // every row folding into that instance carries the same bag and `rowsToSide`
  // merges them all, so patching one row alone is undone on save by a sibling row
  // still holding the old value. Write the bag to the whole instance instead —
  // only the trailing row shows the control, and its siblings are exactly the rows
  // that would resurrect the value.
  const setInstanceStrict = (id: string, strict: Record<string, number>) =>
    onChange(setRowInstanceStrict(rows, id, strict));
  const addRow = (side: RuleConditionSide) => {
    if (side === 'entry_event' && !rows.some((r) => r.side === 'entry_event')) {
      onEntryLockChange?.('slot');
    }
    onChange([
      ...rows,
      side === 'exit' && groupExitWays ? newExitClauseRow() : newRuleConditionRow(side),
    ]);
  };
  const addToWay = (clauseId: string) => onChange([...rows, newExitClauseRow(clauseId)]);
  const toggleRow = (id: string) =>
    onChange(rows.map((r) => (r.id === id ? { ...r, enabled: !ruleRowEnabled(r) } : r)));
  // Park the whole way so an AND group round-trips through `disabled.exit` as one
  // clause. Per-row ⏻ inside a way still works in-session; save of a mixed way
  // splits the parked conjunct into its own spare (the wire has no clause id).
  const toggleWay = (clauseId: string) => {
    const way = rows.filter((r) => r.clauseId === clauseId);
    const anyOn = way.some(ruleRowEnabled);
    onChange(rows.map((r) => (r.clauseId === clauseId ? { ...r, enabled: !anyOn } : r)));
  };
  const removeRow = (id: string) => {
    const next = rows.filter((r) => r.id !== id);
    if (
      rows.some((r) => r.id === id && r.side === 'entry_event') &&
      !next.some((r) => r.side === 'entry_event')
    ) {
      onEntryLockChange?.(null);
    }
    onChange(next);
  };
  const flipSide = (id: string) =>
    onChange(
      rows.map((r) => {
        if (r.id !== id) return r;
        const next = flipConditionSide(r.side);
        if (next === 'exit') {
          return { ...r, side: next, clauseId: r.clauseId ?? newExitClauseId() };
        }
        return { ...r, side: next, clauseId: undefined };
      }),
    );

  const showEvent = sides.includes('entry');
  const showBuySell = showEvent && sides.includes('exit') && groupExitWays;
  const gridCls =
    sides.length === 1 && !showEvent
      ? 'grid grid-cols-1 gap-2'
      : 'grid grid-cols-1 gap-2 md:grid-cols-2';

  const eventColumn = (
    <ConditionColumn
      side="entry_event"
      title={sideTitles?.entry_event ?? 'event'}
      rows={rows.filter((r) => r.side === 'entry_event')}
      registry={registry}
      disabled={disabled}
      allowFlip={canFlip}
      allowToggle={allowToggle}
      emptyHint="No completing-print event — + to fire on a specific print this slot. Empty = every print that passes the filters."
      headerExtra={
        rows.some((r) => r.side === 'entry_event' && ruleRowEnabled(r)) ? (
          <span className={disabled ? 'pointer-events-none opacity-50' : undefined}>
            <ToggleGroup
              size="sm"
              tone="neutral"
              aria-label="How the event fires"
              value={entryLock === 'slot' ? 'slot' : 'every'}
              onChange={(v) => onEntryLockChange?.(v === 'slot' ? 'slot' : null)}
              options={[
                {
                  value: 'slot',
                  label: 'once / slot',
                  title: 'First print this slot that makes the event true is the only candidate',
                },
                {
                  value: 'every',
                  label: 'every print',
                  title: 'Event AND filters on every print — no slot spend',
                },
              ]}
            />
          </span>
        ) : undefined
      }
      onAdd={() => addRow('entry_event')}
      onPatch={setRow}
      onPatchStrict={setInstanceStrict}
      onRemove={removeRow}
      onFlip={flipSide}
      onToggle={toggleRow}
    />
  );

  const filterColumn = (
    <ConditionColumn
      side="entry"
      title={sideTitles?.entry ?? (showEvent ? 'filters' : 'entry')}
      rows={rows.filter((r) => r.side === 'entry')}
      registry={registry}
      disabled={disabled}
      allowFlip={canFlip}
      allowToggle={allowToggle}
      emptyHint={
        showEvent
          ? 'AND with the event on that same print — age, depth, crowd shape. Empty = fingerprint + event alone.'
          : undefined
      }
      onAdd={() => addRow('entry')}
      onPatch={setRow}
      onPatchStrict={setInstanceStrict}
      onRemove={removeRow}
      onFlip={flipSide}
      onToggle={toggleRow}
    />
  );

  const exitColumn =
    sides.includes('exit') && groupExitWays ? (
      <ExitWaysColumn
        title={sideTitles?.exit}
        rows={rows.filter((r) => r.side === 'exit')}
        registry={registry}
        disabled={disabled}
        allowFlip={canFlip}
        allowToggle={allowToggle}
        onAddWay={() => addRow('exit')}
        onAddToWay={addToWay}
        onPatch={setRow}
        onPatchStrict={setInstanceStrict}
        onRemove={removeRow}
        onFlip={flipSide}
        onToggle={toggleRow}
        onToggleWay={toggleWay}
      />
    ) : sides.includes('exit') ? (
      <ConditionColumn
        side="exit"
        title={sideTitles?.exit}
        rows={rows.filter((r) => r.side === 'exit')}
        registry={registry}
        disabled={disabled}
        allowFlip={canFlip}
        allowToggle={allowToggle}
        onAdd={() => addRow('exit')}
        onPatch={setRow}
        onPatchStrict={setInstanceStrict}
        onRemove={removeRow}
        onFlip={flipSide}
        onToggle={toggleRow}
      />
    ) : null;

  return (
    <div className="flex flex-col gap-2">
      {showBuySell ? (
        <div className="grid grid-cols-1 gap-2 xl:grid-cols-[minmax(0,7fr)_minmax(0,5fr)]">
          <div className="flex flex-col gap-1.5 rounded-md border border-accent/25 bg-accent/4 p-2">
            <div className="flex items-baseline justify-between gap-2">
              <span className="text-[10px] font-bold uppercase tracking-wider text-accent/80">
                Buy
              </span>
              <span className="text-[10px] font-normal normal-case tracking-normal text-text-dim/50">
                event AND filters · same print
              </span>
            </div>
            <div className="grid grid-cols-1 gap-2 lg:grid-cols-2">
              {eventColumn}
              {filterColumn}
            </div>
          </div>
          {exitColumn}
        </div>
      ) : (
        <div className={gridCls}>
          {showEvent && eventColumn}
          {sides.includes('entry') && filterColumn}
          {exitColumn}
        </div>
      )}
      {dupErr && <p className="text-[11px] text-red">{dupErr}</p>}
      {armErr && <p className="text-[11px] text-red">{armErr}</p>}
      {parkedWarnings.map((w) => (
        <p key={w} className="text-[11px] text-warning">
          ⚠ {w}
        </p>
      ))}
    </div>
  );
}

function ExitWaysColumn({
  title,
  rows,
  registry,
  disabled,
  allowFlip,
  allowToggle,
  onAddWay,
  onAddToWay,
  onPatch,
  onPatchStrict,
  onRemove,
  onFlip,
  onToggle,
  onToggleWay,
}: {
  title?: string;
  rows: RuleConditionRow[];
  registry: StrategyRegistry;
  disabled?: boolean;
  allowFlip: boolean;
  allowToggle: boolean;
  onAddWay: () => void;
  onAddToWay: (clauseId: string) => void;
  onPatch: (id: string, patch: Partial<RuleConditionRow>) => void;
  onPatchStrict: (id: string, strict: Record<string, number>) => void;
  onRemove: (id: string) => void;
  onFlip: (id: string) => void;
  onToggle: (id: string) => void;
  onToggleWay: (clauseId: string) => void;
}) {
  const label = title ?? 'exit';
  const wayIds = exitClauseOrder(rows);
  const parked = rows.filter((r) => !ruleRowEnabled(r)).length;
  const showAndChrome = wayIds.some((id) => rows.filter((r) => r.clauseId === id).length > 1);

  return (
    <div className="flex flex-col gap-1.5 rounded-md border border-white/10 bg-white/2 p-2">
      <div className="flex items-center justify-between gap-1.5">
        <span className="inline-flex items-center gap-1 text-[10px] font-bold uppercase tracking-wider text-text-dim/70">
          {label}
          <InfoTooltip title={SIDE_HELP.exit.title} body={SIDE_HELP.exit.body} />
          {parked > 0 && (
            <span className="font-normal normal-case tracking-normal text-warning/80">
              {rows.length - parked} live · {parked} off
            </span>
          )}
        </span>
        <IconButton
          variant="success"
          size="md"
          onClick={onAddWay}
          disabled={disabled}
          title="Add another way to sell"
          aria-label="Add another way to sell"
        >
          <PlusIcon />
        </IconButton>
      </div>

      {wayIds.length === 0 ? (
        <div className="rounded border border-dashed border-white/10 px-2 py-3 text-center text-[11px] text-text-dim/50">
          No exit conditions — + to add a way to sell
        </div>
      ) : (
        <div className="flex flex-col gap-1.5">
          {wayIds.map((id, i) => {
            const wayRows = rows.filter((r) => r.clauseId === id);
            const hasTrail = wayRows.some(ruleRowIsTrailing);
            const hasArmed = wayRows.some((r) => r.metric === 'armed');
            const hasArmPct = wayRows.some((r) => r.strict?.arm_above_pct != null);
            const multi = wayRows.length > 1;
            const anyOn = wayRows.some(ruleRowEnabled);
            return (
              <div key={id}>
                {i > 0 && (
                  <div className="flex items-center gap-2 py-0.5">
                    <span className="h-px flex-1 bg-white/10" />
                    <span className="text-[9px] font-bold uppercase tracking-wider text-text-dim/55">
                      or
                    </span>
                    <span className="h-px flex-1 bg-white/10" />
                  </div>
                )}
                <div
                  className={cn(
                    'flex flex-col gap-1 rounded-md p-1.5',
                    (showAndChrome || wayIds.length > 1) && 'border border-white/8 bg-black/10',
                    !anyOn && 'opacity-60',
                  )}
                >
                  <div className="flex items-center justify-between gap-1">
                    <span className="text-[9px] font-bold uppercase tracking-wider text-text-dim/60">
                      {wayIds.length > 1 || multi
                        ? `Way ${i + 1}${multi ? ' · all of these' : ''}`
                        : 'Any of these sells'}
                    </span>
                    <span className="inline-flex items-center gap-0.5">
                      {allowToggle && (
                        <button
                          type="button"
                          onClick={() => onToggleWay(id)}
                          disabled={disabled}
                          title={
                            anyOn
                              ? 'Turn this way off — keep it but stop evaluating it'
                              : 'Turn this way on — evaluate it again'
                          }
                          aria-label="Toggle way"
                          aria-pressed={anyOn}
                          className={cn(
                            'px-1 text-[11px] transition-colors disabled:opacity-40',
                            anyOn ? 'text-text-dim/60 hover:text-warning' : 'text-warning hover:text-text',
                          )}
                        >
                          ⏻
                        </button>
                      )}
                      <IconButton
                        variant="ghost"
                        size="sm"
                        onClick={() => onAddToWay(id)}
                        disabled={disabled}
                        title="AND another condition into this way"
                        aria-label="Add condition to this way"
                      >
                        <PlusIcon />
                      </IconButton>
                    </span>
                  </div>
                  {wayRows.length === 0 ? (
                    <div className="rounded border border-dashed border-white/10 px-2 py-2 text-center text-[11px] text-text-dim/50">
                      Empty way — + to add a condition
                    </div>
                  ) : (
                    wayRows.map((row) => (
                      <ConditionRow
                        key={row.id}
                        row={row}
                        registry={registry}
                        disabled={disabled}
                        allowFlip={allowFlip}
                        allowToggle={allowToggle}
                        onPatch={(patch) => onPatch(row.id, patch)}
                        onPatchStrict={(strict) => onPatchStrict(row.id, strict)}
                        onRemove={() => onRemove(row.id)}
                        onFlip={() => onFlip(row.id)}
                        onToggle={() => onToggle(row.id)}
                      />
                    ))
                  )}
                  {hasTrail && hasArmPct && multi && !hasArmed && (
                    <p className="px-1 text-[10px] text-text-dim/70">
                      Grouped ways do not skip under arm ≥ %. Add <code>armed = 1</code> in
                      this way to latch the trail after PnL falls.
                    </p>
                  )}
                  {hasTrail && hasArmPct && hasArmed && (
                    <p className="px-1 text-[10px] text-text-dim/70">
                      <code>armed</code> latches at arm ≥ % and stays on after PnL falls.
                    </p>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function ConditionColumn({
  side,
  title,
  rows,
  registry,
  disabled,
  allowFlip,
  allowToggle,
  emptyHint,
  headerExtra,
  onAdd,
  onPatch,
  onPatchStrict,
  onRemove,
  onFlip,
  onToggle,
}: {
  side: RuleConditionSide;
  title?: string;
  rows: RuleConditionRow[];
  registry: StrategyRegistry;
  disabled?: boolean;
  allowFlip: boolean;
  allowToggle: boolean;
  emptyHint?: string;
  headerExtra?: ReactNode;
  onAdd: () => void;
  onPatch: (id: string, patch: Partial<RuleConditionRow>) => void;
  onPatchStrict: (id: string, strict: Record<string, number>) => void;
  onRemove: (id: string) => void;
  onFlip: (id: string) => void;
  onToggle: (id: string) => void;
}) {
  const label = title ?? side;
  const parked = rows.filter((r) => !ruleRowEnabled(r)).length;
  return (
    <div className="flex flex-col gap-1.5 rounded-md border border-white/10 bg-white/2 p-2">
      <div className="flex items-center justify-between gap-1.5">
        <span className="inline-flex flex-wrap items-center gap-1.5 text-[10px] font-bold uppercase tracking-wider text-text-dim/70">
          {label}
          <InfoTooltip title={SIDE_HELP[side].title} body={SIDE_HELP[side].body} />
          {headerExtra}
          {/* A parked row still renders, so say plainly how many of the visible rows
              the engine will actually evaluate. */}
          {parked > 0 && (
            <span className="font-normal normal-case tracking-normal text-warning/80">
              {rows.length - parked} live · {parked} off
            </span>
          )}
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
          {emptyHint ?? `No ${label} conditions — + to add one`}
        </div>
      ) : (
        <div className="flex flex-col gap-1.5">
          {rows.map((row) => (
            <ConditionRow
              key={row.id}
              row={row}
              registry={registry}
              disabled={disabled}
              allowFlip={allowFlip}
              allowToggle={allowToggle}
              onPatch={(patch) => onPatch(row.id, patch)}
              onPatchStrict={(strict) => onPatchStrict(row.id, strict)}
              onRemove={() => onRemove(row.id)}
              onFlip={() => onFlip(row.id)}
              onToggle={() => onToggle(row.id)}
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
  allowFlip,
  allowToggle,
  onPatch,
  onPatchStrict,
  onRemove,
  onFlip,
  onToggle,
}: {
  row: RuleConditionRow;
  registry: StrategyRegistry;
  disabled?: boolean;
  allowFlip: boolean;
  allowToggle: boolean;
  onPatch: (patch: Partial<RuleConditionRow>) => void;
  onPatchStrict: (strict: Record<string, number>) => void;
  onRemove: () => void;
  onFlip: () => void;
  onToggle: () => void;
}) {
  const on = ruleRowEnabled(row);
  // A parked row is still validated (its problem must be visible BEFORE it goes
  // live again) but is shown muted, and the editor's save gate ignores it.
  const err = ruleConditionRowError(row, registry);
  const group = registry.groups.find((g) => g.name === row.group);
  const metric = group?.metrics.find((m) => m.name === row.metric);
  const needsWindow = ruleRowNeedsWindow(row, registry);
  // A second trailing-window axis (`m_flow_window`) — asked of the registry, not
  // hardcoded per group, so a future two-window basis gets its control for free
  // rather than silently round-tripping with no way to edit it.
  const needsSlice = ruleRowNeedsSlice(row, registry);
  // Both axes count in the ONE unit the row picks: a slice in slots over a
  // reference in seconds is a ratio across two clocks, which the backend rejects.
  const windowUnit = ruleRowUnit(row);
  const uSuffix = windowUnitSuffix(windowUnit);
  // A discrete axis counts whole buckets — slots and prints have no halves — so its
  // inputs step by 1 from 1. Seconds keep the half-second grain they always had.
  const uStep = isDiscreteUnit(windowUnit) ? 1 : 0.5;
  const uMin = isDiscreteUnit(windowUnit) ? 1 : 0.5;
  // Placeholders name the canonical span of each basis: 10s of tape, 30 slots, the
  // 20 prints behind this one. The slice placeholder is the span inside it, and on
  // a print row `1` is this transaction alone — the span "10 SOL in one trade" needs.
  const windowHint = windowUnit === 'slot' ? '30' : windowUnit === 'print' ? '20' : '10';
  const sliceHint = windowUnit === 'sec' ? '3' : '1';
  const sliceValue = row.strict?.[sliceSizeParam(windowUnit)];
  const sliceText = sliceValue == null ? '' : String(sliceValue);
  const isTrailing = ruleRowIsTrailing(row);
  const armValue = row.strict?.arm_above_pct;
  const armText = armValue == null ? '' : String(armValue);

  const onArm = (text: string) => {
    // Empty = off: drop the key rather than write 0 — `arm_above_pct: 0` is a real
    // value (arm at break-even), so the two stay distinguishable.
    if (text.trim() === '') {
      const { arm_above_pct: _drop, ...rest } = row.strict ?? {};
      onPatchStrict(rest);
      return;
    }
    const v = Number(text);
    if (!Number.isFinite(v)) return;
    onPatchStrict({ ...row.strict, arm_above_pct: v });
  };
  const onSlice = (text: string) => {
    // Only ONE slice size param may survive, in the row's own unit — leaving a
    // sibling behind is the "two spans claiming one axis" the backend rejects at save.
    const rest = withoutSliceAxis(row.strict);
    if (text.trim() === '') {
      onPatchStrict(rest);
      return;
    }
    const v = Number(text);
    if (!Number.isFinite(v)) return;
    onPatchStrict({ ...rest, [sliceSizeParam(windowUnit)]: v });
  };
  // Flipping the unit RE-SPELLS the slice param rather than reinterpreting it: the
  // number the user typed is the span they meant, and it now counts in the new unit.
  const onUnit = (next: WindowUnit) => {
    const slice = ruleRowSliceSpec(row)?.size;
    const rest = withoutSliceAxis(row.strict);
    onPatch({
      windowUnit: next,
      strict: slice == null ? rest : { ...rest, [sliceSizeParam(next)]: slice },
    });
  };
  // Only drives the input's unit adornment; the field is disabled until a metric is
  // chosen, so the fallback is never user-visible.
  const unit: MetricUnit = metric?.unit ?? 'seconds';

  const onGroup = (name: string) => {
    const g = registry.groups.find((gg) => gg.name === name);
    // Auto-pick the group's first metric + reset every window field so a static pick
    // drops them. The slice axis goes with them: carried over from the previous
    // group it is a param the new group does not declare, which the backend rejects
    // as unknown at save rather than ignoring.
    const rest = withoutSliceAxis(row.strict);
    onPatch({
      group: name,
      metric: g?.metrics[0]?.name ?? '',
      window: '',
      lag: '',
      arms: [],
      strict: rest,
    });
  };

  // Tint the row border from the metric hue (+ op shade) once fully picked, matching
  // the sweep builder's per-metric coloring. A parked row drops the tint entirely —
  // the hue is what says "this one is live".
  const tint: CSSProperties | undefined =
    on && !err && row.metric
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
        err && on ? 'border-red/40 bg-red/5' : tint ? '' : 'border-white/10 bg-surface',
        // Parked: readable but plainly inert. Controls stay enabled on purpose —
        // tweaking a value before switching it back on is the whole workflow.
        !on && 'border-dashed opacity-45',
      )}
      style={tint}
    >
      <Cell label="group" tip={row.group ? groupHelpTip(row.group, group) : undefined}>
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
            .filter((g) => !(row.side !== 'exit' && g.scope === 'position'))
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
        <Cell label={`window ${uSuffix}`} tip={STRICT_PARAM_HELP[sizeParamOf(windowUnit)]}>
          <Input
            fieldSize="sm"
            type="number"
            min={uMin}
            step={uStep}
            value={row.window}
            disabled={disabled}
            onChange={(e) => onPatch({ window: e.target.value })}
            placeholder={windowHint}
            className="w-16"
          />
        </Cell>
      )}

      {/* The unit tip follows the SELECTION: with three bases, one fixed help text
          would explain a unit the row is not counting in. */}
      {needsWindow && (
        <Cell label="unit" tip={STRICT_PARAM_HELP[sizeParamOf(windowUnit)]}>
          <Select
            fieldSize="sm"
            value={windowUnit}
            disabled={disabled}
            onChange={(e) => onUnit(e.target.value as WindowUnit)}
            className="w-20"
          >
            {/* Straight off WINDOW_UNITS, so a new basis gets its option rather than
                round-tripping through the JSON view with no way to pick it. */}
            {WINDOW_UNITS.map((u) => (
              <option key={u} value={u}>
                {u}
              </option>
            ))}
          </Select>
        </Cell>
      )}

      {needsSlice && (
        <Cell label={`slice ${uSuffix}`} tip={STRICT_PARAM_HELP[sliceSizeParam(windowUnit)]}>
          <Input
            fieldSize="sm"
            type="number"
            min={uMin}
            step={uStep}
            value={sliceText}
            disabled={disabled}
            onChange={(e) => onSlice(e.target.value)}
            placeholder={sliceHint}
            className="w-16"
          />
        </Cell>
      )}

      {needsWindow && (
        <Cell label={`lag ${uSuffix}`} tip={STRICT_PARAM_HELP[WINDOW_LAG_PARAM]}>
          <Input
            fieldSize="sm"
            type="number"
            min={0}
            step={uStep}
            value={row.lag ?? ''}
            disabled={disabled}
            onChange={(e) => onPatch({ lag: e.target.value })}
            placeholder="0"
            className="w-14"
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
        {allowToggle && (
        <button
          type="button"
          onClick={onToggle}
          disabled={disabled}
          title={
            on
              ? 'Turn off — keep the condition but stop evaluating it'
              : 'Turn on — evaluate this condition again'
          }
          aria-label="Toggle condition"
          aria-pressed={on}
          className={cn(
            'px-1 transition-colors disabled:opacity-40',
            on ? 'text-text-dim/60 hover:text-warning' : 'text-warning hover:text-text',
          )}
        >
          ⏻
        </button>
        )}
        {allowFlip && (
          <button
            type="button"
            onClick={onFlip}
            disabled={disabled}
            title={`Move to ${flipSideLabel(row.side)}`}
            aria-label="Flip side"
            className="px-1 text-text-dim/60 transition-colors hover:text-text disabled:opacity-40"
          >
            ⇄
          </button>
        )}
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

      {err && (
        <span className={cn('w-full text-[11px]', on ? 'text-red' : 'text-text-dim/70')}>
          {on ? err : `off — ${err} (fix before turning it back on)`}
        </span>
      )}
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
