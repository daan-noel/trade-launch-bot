import { useMemo, useState, type CSSProperties, type ReactNode } from 'react';
import { cn } from 'lib/cn';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { IconButton } from 'components/ui/IconButton';
import { PlusIcon } from 'components/ui/icons';
import { Badge } from 'components/ui/Badge';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import {
  SWEEP_FIELD_HELP,
  METRIC_HELP,
  metricHelpBody,
  GROUP_HELP,
  SIDE_HELP,
  RULE_FIELD_HELP,
} from 'lib/strategy/strategyHelp';
import { unitSuffix, useStrategyRegistry, type StrategyRegistry } from 'lib/strategy/registry';
import { metricColorStyle } from 'lib/strategy/metricColors';
import { isPnlAdvancedMetric } from 'lib/strategy/validate';
import {
  axisRowError,
  comboCount,
  newAxisRow,
  pnlAxisSugarDuplicateError,
  rowNeedsWindow,
  sharedWindowError,
  type AxisKind,
  type GenericAxisRow,
  type MetricAxisSide,
} from './genericAxes';

const KIND_LABEL: Record<AxisKind, string> = {
  metric: 'metric',
  take_profit: 'TP %',
  stop_loss: 'SL %',
};

const DND_MIME = 'application/x-hunter-axis-id';

function isAxisDrag(dt: DataTransfer): boolean {
  const types = [...dt.types];
  return types.includes(DND_MIME) || types.includes('text/plain');
}

function axisDragId(dt: DataTransfer): string {
  return dt.getData(DND_MIME) || dt.getData('text/plain');
}

export interface GenericAxisBuilderProps {
  rows: GenericAxisRow[];
  onChange: (rows: GenericAxisRow[]) => void;
  /** Projected combos for this axis set (rendered as a badge; the config form
   *  reuses the same number for the over-cap gate). */
  projected?: number;
}

/** Insertion slot while dragging a metric: before `beforeId`, or append when null. */
type DropSlot = { side: MetricAxisSide; beforeId: string | null };

/**
 * The registry-driven sweep axis builder (redesign FE5.1). Layout:
 *  - TP / SL strip on top (one of each; empty slot = add)
 *  - Entry metrics (left) · Exit metrics (right)
 * Metric rows tint from the registry `hue` (+ fixed per-op shade). Drag a
 * metric row onto the other column to flip its side (or reorder within a column).
 */
export function GenericAxisBuilder({ rows, onChange, projected }: GenericAxisBuilderProps) {
  const { data: registry } = useStrategyRegistry();
  /** HTML5 DnD can't read custom MIME data during dragover — keep source id in state. */
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [dropSlot, setDropSlot] = useState<DropSlot | null>(null);

  const combos = useMemo(() => projected ?? comboCount(rows, registry), [projected, rows, registry]);
  const windowErr = useMemo(() => sharedWindowError(rows, registry), [rows, registry]);
  const pnlSugarErr = useMemo(() => pnlAxisSugarDuplicateError(rows), [rows]);

  const tpRow = useMemo(() => rows.find((r) => r.kind === 'take_profit'), [rows]);
  const slRow = useMemo(() => rows.find((r) => r.kind === 'stop_loss'), [rows]);
  const entryRows = useMemo(
    () => rows.filter((r) => r.kind === 'metric' && r.side === 'entry'),
    [rows],
  );
  const exitRows = useMemo(
    () => rows.filter((r) => r.kind === 'metric' && r.side === 'exit'),
    [rows],
  );

  const clearDrag = () => {
    setDraggingId(null);
    setDropSlot(null);
  };

  const setRow = (id: string, patch: Partial<GenericAxisRow>) =>
    onChange(rows.map((r) => (r.id === id ? { ...r, ...patch } : r)));
  const removeRow = (id: string) => onChange(rows.filter((r) => r.id !== id));
  /** At most one TP and one SL axis — a second of the same kind would overwrite
   *  in `RuleParams` while still multiplying combo count. */
  const addTpSl = (kind: 'take_profit' | 'stop_loss') => {
    if (rows.some((r) => r.kind === kind)) return;
    onChange([...rows, newAxisRow(kind, registry)]);
  };
  const addMetric = (side: MetricAxisSide) =>
    onChange([...rows, newAxisRow('metric', registry, side)]);

  /** Move a metric row to `targetSide`, optionally inserting before `beforeId`
   *  (same-column reorder or cross-column drop). Appends when `beforeId` is
   *  null / missing. */
  const moveMetric = (id: string, targetSide: MetricAxisSide, beforeId: string | null) => {
    const src = rows.find((r) => r.id === id);
    if (!src || src.kind !== 'metric') return;

    const moved: GenericAxisRow = { ...src, side: targetSide };
    const without = rows.filter((r) => r.id !== id);

    let insertAt = without.length;
    if (beforeId) {
      const bi = without.findIndex((r) => r.id === beforeId);
      if (bi >= 0) insertAt = bi;
    } else {
      let last = -1;
      for (let i = 0; i < without.length; i++) {
        const r = without[i];
        if (r.kind === 'metric' && r.side === targetSide) last = i;
      }
      insertAt = last >= 0 ? last + 1 : without.length;
    }
    onChange([...without.slice(0, insertAt), moved, ...without.slice(insertAt)]);
  };

  return (
    <div className="flex flex-col gap-2">
      <div className="grid grid-cols-[auto_1fr_auto] items-center gap-2">
        <span className="flex items-center gap-1 text-[10px] font-bold uppercase tracking-wider text-text-dim/80">
          Axes
          <InfoTooltip
            title="Sweep axes"
            body="TP/SL on top; entry metrics left, exit right. Each combo picks one value per axis. Same metric + crossed ops → OR; feasible opposing bounds → AND range. Drag a metric onto the other column to flip its side."
          />
        </span>
        <span />
        <Badge variant={combos === 0 ? 'neutral' : 'primary'} className="font-mono">
          ~{combos.toLocaleString()} combos
        </Badge>
      </div>

      {/* TP / SL strip — one slot each; empty slot is the add affordance. */}
      <div className="flex flex-col gap-1.5 rounded-md border border-white/10 bg-white/[0.02] p-2">
        <span className="inline-flex items-center gap-1 text-[10px] font-bold uppercase tracking-wider text-text-dim/70">
          TP / SL
          <InfoTooltip
            title="Take profit / Stop loss"
            body={`${RULE_FIELD_HELP.takeProfit.body} ${RULE_FIELD_HELP.stopLoss.body} Each is its own sweep axis (comma-separated % values).`}
          />
        </span>
        <div className="grid grid-cols-1 gap-1.5 sm:grid-cols-2">
          <TpSlSlot
            kind="take_profit"
            row={tpRow}
            registry={registry}
            onAdd={() => addTpSl('take_profit')}
            onPatch={(patch) => tpRow && setRow(tpRow.id, patch)}
            onRemove={() => tpRow && removeRow(tpRow.id)}
          />
          <TpSlSlot
            kind="stop_loss"
            row={slRow}
            registry={registry}
            onAdd={() => addTpSl('stop_loss')}
            onPatch={(patch) => slRow && setRow(slRow.id, patch)}
            onRemove={() => slRow && removeRow(slRow.id)}
          />
        </div>
      </div>

      {/* Entry (left) · Exit (right) */}
      <div className="grid grid-cols-1 gap-2 md:grid-cols-2">
        <MetricSideColumn
          side="entry"
          rows={entryRows}
          registry={registry}
          draggingId={draggingId}
          dropSlot={dropSlot?.side === 'entry' ? dropSlot : null}
          onDragStartRow={setDraggingId}
          onDragEnd={clearDrag}
          onDropSlot={setDropSlot}
          onPatch={setRow}
          onRemove={removeRow}
          onAdd={() => addMetric('entry')}
          onMove={moveMetric}
        />
        <MetricSideColumn
          side="exit"
          rows={exitRows}
          registry={registry}
          draggingId={draggingId}
          dropSlot={dropSlot?.side === 'exit' ? dropSlot : null}
          onDragStartRow={setDraggingId}
          onDragEnd={clearDrag}
          onDropSlot={setDropSlot}
          onPatch={setRow}
          onRemove={removeRow}
          onAdd={() => addMetric('exit')}
          onMove={moveMetric}
        />
      </div>

      {rows.length === 0 && (
        <p className="rounded-md border border-dashed border-white/10 px-3 py-2 text-[12px] text-text-dim">
          No axes yet — add a TP / SL axis above or a metric condition in either column.
        </p>
      )}

      {windowErr && <p className="text-[11px] text-red">{windowErr}</p>}
      {pnlSugarErr && <p className="text-[11px] text-red">{pnlSugarErr}</p>}
    </div>
  );
}

const TPSL_SLOT: Record<
  'take_profit' | 'stop_loss',
  { short: string; label: string; accent: string; empty: string; filled: string; input: string }
> = {
  take_profit: {
    short: 'TP',
    label: 'Take profit',
    accent: 'text-green',
    empty:
      'border-dashed border-green/25 bg-green/[0.03] text-green/70 hover:border-green/45 hover:bg-green/[0.08] hover:text-green',
    filled: 'border-green/30 bg-green/[0.07]',
    input: 'border-green/20 bg-green/[0.06] focus:border-green/50 focus:bg-green/[0.1]',
  },
  stop_loss: {
    short: 'SL',
    label: 'Stop loss',
    accent: 'text-red',
    empty:
      'border-dashed border-red/25 bg-red/[0.03] text-red/70 hover:border-red/45 hover:bg-red/[0.08] hover:text-red',
    filled: 'border-red/30 bg-red/[0.07]',
    input: 'border-red/20 bg-red/[0.06] focus:border-red/50 focus:bg-red/[0.1]',
  },
};

/** One TP or SL axis slot — empty = add affordance; filled = tinted values editor. */
function TpSlSlot({
  kind,
  row,
  registry,
  onAdd,
  onPatch,
  onRemove,
}: {
  kind: 'take_profit' | 'stop_loss';
  row: GenericAxisRow | undefined;
  registry: StrategyRegistry | undefined;
  onAdd: () => void;
  onPatch: (patch: Partial<GenericAxisRow>) => void;
  onRemove: () => void;
}) {
  const style = TPSL_SLOT[kind];

  if (!row) {
    return (
      <button
        type="button"
        onClick={onAdd}
        className={cn(
          'flex min-h-[3.25rem] items-center justify-center gap-1.5 rounded-md border px-3 py-2 text-[11px] font-semibold tracking-wide transition-colors',
          style.empty,
        )}
      >
        <span className="text-[13px] leading-none opacity-70">+</span>
        {style.label} %
      </button>
    );
  }

  const err = axisRowError(row, registry);

  return (
    <div
      className={cn(
        'flex flex-col gap-1 rounded-md border px-2.5 py-2',
        err ? 'border-red/40 bg-red/5' : style.filled,
      )}
    >
      <div className="flex items-center gap-2">
        <span
          className={cn(
            'inline-flex shrink-0 items-center gap-0.5 text-[10px] font-bold uppercase tracking-wider',
            err ? 'text-red' : style.accent,
          )}
        >
          {style.short}
          <InfoTooltip
            title={kind === 'take_profit' ? RULE_FIELD_HELP.takeProfit.title : RULE_FIELD_HELP.stopLoss.title}
            body={kind === 'take_profit' ? RULE_FIELD_HELP.takeProfit.body : RULE_FIELD_HELP.stopLoss.body}
            side="top"
          />
        </span>
        <div className="min-w-0 flex-1">
          <Input
            fieldSize="sm"
            value={row.valuesText}
            onChange={(e) => onPatch({ valuesText: e.target.value })}
            placeholder="50, 100, 200"
            unit="%"
            aria-label={`${style.label} values`}
            aria-invalid={!!err}
            className={cn(
              'font-mono tabular-nums',
              err ? 'border-red/70 focus:border-red' : style.input,
            )}
          />
        </div>
        <button
          type="button"
          onClick={onRemove}
          title={`Remove ${style.label.toLowerCase()}`}
          className="shrink-0 px-1 text-text-dim/50 transition-colors hover:text-red"
        >
          ✕
        </button>
      </div>
      {err && <span className="text-[10px] text-red">{err}</span>}
    </div>
  );
}

/** Insert-slot line — absolutely placed so it doesn't shift row hit-testing. */
function DropIndicator({ at }: { at: 'before' | 'after' }) {
  return (
    <div
      aria-hidden
      className={cn(
        'pointer-events-none absolute inset-x-0 z-10 flex items-center gap-1',
        at === 'before' ? '-top-1.5' : '-bottom-1.5',
      )}
    >
      <span className="size-1.5 shrink-0 rounded-full bg-primary ring-2 ring-primary/35" />
      <span className="h-1 flex-1 rounded-full bg-primary shadow-[0_0_10px_color-mix(in_srgb,var(--color-primary)_55%,transparent)]" />
      <span className="size-1.5 shrink-0 rounded-full bg-primary ring-2 ring-primary/35" />
    </div>
  );
}

function MetricSideColumn({
  side,
  rows,
  registry,
  draggingId,
  dropSlot,
  onDragStartRow,
  onDragEnd,
  onDropSlot,
  onPatch,
  onRemove,
  onAdd,
  onMove,
}: {
  side: MetricAxisSide;
  rows: GenericAxisRow[];
  registry: StrategyRegistry | undefined;
  draggingId: string | null;
  dropSlot: DropSlot | null;
  onDragStartRow: (id: string) => void;
  onDragEnd: () => void;
  onDropSlot: (slot: DropSlot | null) => void;
  onPatch: (id: string, patch: Partial<GenericAxisRow>) => void;
  onRemove: (id: string) => void;
  onAdd: () => void;
  onMove: (id: string, targetSide: MetricAxisSide, beforeId: string | null) => void;
}) {
  const showEndIndicator = dropSlot != null && dropSlot.beforeId == null && rows.length > 0;

  /** Insert slot from pointer Y vs each row's midpoint (stable across flex gaps). */
  const slotFromPointerY = (clientY: number, columnEl: HTMLElement): DropSlot => {
    const nodes = columnEl.querySelectorAll<HTMLElement>('[data-axis-row]');
    for (const node of nodes) {
      const rect = node.getBoundingClientRect();
      if (clientY < rect.top + rect.height / 2) {
        return { side, beforeId: node.dataset.axisRow || null };
      }
    }
    return { side, beforeId: null };
  };

  /** Skip a no-op slot (same place the dragged row already occupies). */
  const normalizeSlot = (slot: DropSlot): DropSlot | null => {
    if (!draggingId) return slot;
    const fromIdx = rows.findIndex((r) => r.id === draggingId);
    if (fromIdx < 0) return slot; // cross-column — always a real move
    const toIdx = slot.beforeId
      ? rows.findIndex((r) => r.id === slot.beforeId)
      : rows.length;
    // Removing the source shifts later indices down by one.
    const adjustedTo = fromIdx < toIdx ? toIdx - 1 : toIdx;
    if (adjustedTo === fromIdx) return null;
    return slot;
  };

  const previewSlot = (clientY: number, columnEl: HTMLElement) => {
    onDropSlot(normalizeSlot(slotFromPointerY(clientY, columnEl)));
  };

  const commitDrop = (id: string, slot: DropSlot | null) => {
    onDropSlot(null);
    onDragEnd();
    if (!id || !slot) return;
    onMove(id, side, slot.beforeId);
  };

  return (
    <div
      className={cn(
        'flex flex-col gap-1.5 rounded-md border p-2 transition-colors',
        dropSlot ? 'border-primary/50 bg-primary/5' : 'border-white/10 bg-white/[0.02]',
      )}
      onDragOver={(e) => {
        if (!isAxisDrag(e.dataTransfer)) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = 'move';
        previewSlot(e.clientY, e.currentTarget);
      }}
      onDragLeave={(e) => {
        if (e.currentTarget.contains(e.relatedTarget as Node)) return;
        onDropSlot(null);
      }}
      onDrop={(e) => {
        e.preventDefault();
        const id = axisDragId(e.dataTransfer);
        const slot = normalizeSlot(slotFromPointerY(e.clientY, e.currentTarget));
        commitDrop(id, slot);
      }}
    >
      <div className="flex items-center justify-between gap-1.5">
        <span className="inline-flex items-center gap-1 text-[10px] font-bold uppercase tracking-wider text-text-dim/70">
          {side}
          <InfoTooltip title={SIDE_HELP[side].title} body={SIDE_HELP[side].body} />
        </span>
        <IconButton
          variant="success"
          size="md"
          onClick={onAdd}
          title="Add metric"
          aria-label="Add metric"
        >
          <PlusIcon />
        </IconButton>
      </div>

      {rows.length === 0 ? (
        <div
          className={cn(
            'rounded border border-dashed px-2 py-3 text-center text-[11px] transition-colors',
            dropSlot
              ? 'border-primary/50 bg-primary/10 text-primary/80'
              : 'border-white/10 text-text-dim/50',
          )}
        >
          {dropSlot ? 'Drop here' : 'Drop here or + metric'}
        </div>
      ) : (
        <div className="flex flex-col gap-1.5">
          {rows.map((row, i) => {
            const showBefore = dropSlot?.beforeId === row.id;
            const showAfter = showEndIndicator && i === rows.length - 1;
            return (
              <div key={row.id} className="relative" data-axis-row={row.id}>
                {showBefore && <DropIndicator at="before" />}
                {showAfter && <DropIndicator at="after" />}
                <AxisRow
                  row={row}
                  registry={registry}
                  dragHandle
                  dragging={draggingId === row.id}
                  onDragStartRow={() => onDragStartRow(row.id)}
                  onDragEnd={onDragEnd}
                  onPatch={(patch) => onPatch(row.id, patch)}
                  onRemove={() => onRemove(row.id)}
                />
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

/** One editable axis row. Metric rows omit the side select (column owns side). */
function AxisRow({
  row,
  registry,
  dragHandle,
  dragging,
  onDragStartRow,
  onDragEnd,
  onPatch,
  onRemove,
}: {
  row: GenericAxisRow;
  registry: StrategyRegistry | undefined;
  /** When set, a ⠿ handle starts the drag (inputs stay selectable). */
  dragHandle?: boolean;
  dragging?: boolean;
  onDragStartRow?: () => void;
  onDragEnd?: () => void;
  onPatch: (patch: Partial<GenericAxisRow>) => void;
  onRemove: () => void;
}) {
  const err = axisRowError(row, registry);
  const isMetric = row.kind === 'metric';
  const group = registry?.groups.find((g) => g.name === row.group);
  const metric = group?.metrics.find((m) => m.name === row.metric);
  const needsWindow = rowNeedsWindow(row, registry);
  const valueUnit = metric ? unitSuffix(metric.unit) : row.kind !== 'metric' ? '%' : '';

  const onGroup = (name: string) => {
    const g = registry?.groups.find((gg) => gg.name === name);
    onPatch({ group: name, metric: g?.metrics[0]?.name ?? '' });
  };

  const tintStyle: CSSProperties | undefined =
    !err && isMetric && row.metric
      ? (() => {
          const c = metricColorStyle({
            hue: metric?.hue,
            group: row.group,
            metric: row.metric,
            operator: row.operator,
          });
          return { borderColor: c.border, backgroundColor: c.background };
        })()
      : undefined;

  return (
    <div
      className={cn(
        'flex flex-wrap items-center gap-1.5 rounded-md border px-2 py-1.5 transition-opacity',
        err ? 'border-red/40 bg-red/5' : tintStyle ? '' : 'border-white/10 bg-surface',
        dragging && 'opacity-40',
      )}
      style={tintStyle}
    >
      {dragHandle && (
        <span
          draggable
          onDragStart={(e) => {
            e.dataTransfer.setData(DND_MIME, row.id);
            e.dataTransfer.setData('text/plain', row.id);
            e.dataTransfer.effectAllowed = 'move';
            onDragStartRow?.();
          }}
          onDragEnd={() => onDragEnd?.()}
          className="shrink-0 cursor-grab select-none px-0.5 text-[12px] leading-none text-text-dim/40 active:cursor-grabbing"
          title="Drag to the other column (or reorder)"
          aria-label="Drag axis"
        >
          ⠿
        </span>
      )}

      <span className="w-14 shrink-0 text-[10px] font-bold uppercase tracking-wider text-text-dim/70">
        {KIND_LABEL[row.kind]}
      </span>

      {isMetric ? (
        <>
          <Cell
            label="group"
            tip={row.group && GROUP_HELP[row.group] ? GROUP_HELP[row.group] : undefined}
          >
            <Select
              fieldSize="sm"
              value={row.group}
              onChange={(e) => onGroup(e.target.value)}
              className="w-32"
            >
              <option value="">group…</option>
              {registry?.groups.map((g) => (
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
                    body: metricHelpBody(
                      row.metric,
                      group?.metrics.find((m) => m.name === row.metric),
                    ),
                  }
                : undefined
            }
          >
            <Select
              fieldSize="sm"
              value={row.metric}
              onChange={(e) => onPatch({ metric: e.target.value })}
              className="w-28"
              disabled={!group}
            >
              <option value="">metric…</option>
              {group?.metrics.map((m) => (
                <option key={m.name} value={m.name}>
                  {isPnlAdvancedMetric(group.name, m.name) ? `${m.name} (advanced)` : m.name}
                </option>
              ))}
            </Select>
          </Cell>
          <Cell label="op" tip={SWEEP_FIELD_HELP.axisOp}>
            <Select
              fieldSize="sm"
              value={row.operator}
              onChange={(e) => onPatch({ operator: e.target.value as GenericAxisRow['operator'] })}
              className="w-14"
            >
              {registry?.operators.map((op) => (
                <option key={op} value={op}>
                  {op}
                </option>
              ))}
            </Select>
          </Cell>
          {needsWindow && (
            <Cell label="window s" tip={SWEEP_FIELD_HELP.axisWindow}>
              <Input
                fieldSize="sm"
                type="number"
                min={0.5}
                step={0.5}
                value={row.window}
                onChange={(e) => onPatch({ window: e.target.value })}
                placeholder="10"
                className="w-16"
              />
            </Cell>
          )}
        </>
      ) : null}

      <Cell label="values" grow tip={SWEEP_FIELD_HELP.axisValues}>
        <Input
          fieldSize="sm"
          value={row.valuesText}
          onChange={(e) => onPatch({ valuesText: e.target.value })}
          placeholder={isMetric ? 'off, 5, 10  ·  10..40 step 10' : '50, 100, 200'}
          unit={valueUnit || undefined}
          aria-invalid={!!err}
          className={cn('min-w-[8rem]', err && 'border-red/70 focus:border-red')}
        />
      </Cell>

      <button
        type="button"
        onClick={onRemove}
        title="Remove axis"
        className="ml-auto shrink-0 px-1 text-text-dim hover:text-red"
      >
        ✕
      </button>

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
    <div className={cn('flex flex-col gap-0.5', grow && 'flex-1')}>
      <span className="inline-flex items-center gap-0.5 text-[8px] uppercase tracking-wider text-text-dim/50">
        {label}
        {tip && <InfoTooltip title={tip.title} body={tip.body} side="top" />}
      </span>
      {children}
    </div>
  );
}
