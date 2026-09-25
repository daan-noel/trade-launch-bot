import { useMemo, useState, type CSSProperties, type ReactNode } from 'react';
import { cn } from 'lib/cn';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { IconButton } from 'components/ui/IconButton';
import { PlusIcon } from 'components/ui/icons';
import { Badge } from 'components/ui/Badge';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { RefFields, type CondContext } from 'components/strategy/rule/CondRow';
import { SWEEP_FIELD_HELP } from 'lib/strategy/strategyHelp';
import {
  familyName,
  findMetric,
  rulePart,
  unitSuffix,
  useStrategyRegistry,
  type Operator,
  type StrategyRegistry,
} from 'lib/strategy/registry';
import { metricColorStyle } from 'lib/strategy/metricColors';
import { readPhrase } from 'lib/strategy/sentences';
import {
  axisRowError,
  axisSummary,
  comboCount,
  newAxisRow,
  type GenericAxisRow,
  type MetricAxisSide,
} from './genericAxes';

const DND_MIME = 'application/x-hunter-axis-id';

function isAxisDrag(dt: DataTransfer): boolean {
  const types = [...dt.types];
  return types.includes(DND_MIME) || types.includes('text/plain');
}

function axisDragId(dt: DataTransfer): string {
  return dt.getData(DND_MIME) || dt.getData('text/plain');
}

/** The rule part a side writes into, for its heading and help. */
const SIDE_PART: Record<MetricAxisSide, string> = { entry: 'enter.filters', exit: 'always' };

export interface GenericAxisBuilderProps {
  rows: GenericAxisRow[];
  onChange: (rows: GenericAxisRow[]) => void;
  /** The tag names the run's tags document defines: what a tag select offers. */
  tags: readonly string[];
  /** Projected combos (the badge); the form reuses the number for its cap gate. */
  projected?: number;
}

/** Insertion slot while dragging a metric row: before `beforeId`, or append. */
type DropSlot = { side: MetricAxisSide; beforeId: string | null };

/**
 * The sweep axis builder: a TP / SL strip on top, entry filters (left) and exit sell
 * lines (right) below. Each metric row is one read (the rule editor's own metric, tag
 * and span controls), an operator and the values to try. Drag a row onto the other
 * column to flip its side.
 */
export function GenericAxisBuilder({ rows, onChange, tags, projected }: GenericAxisBuilderProps) {
  const { data: registry } = useStrategyRegistry();
  /** HTML5 DnD cannot read custom MIME data during dragover: keep the source id here. */
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [dropSlot, setDropSlot] = useState<DropSlot | null>(null);

  const combos = useMemo(() => projected ?? comboCount(rows), [projected, rows]);
  const tpRow = rows.find((r) => r.kind === 'take_profit');
  const slRow = rows.find((r) => r.kind === 'stop_loss');
  const entryRows = rows.filter((r) => r.kind === 'metric' && r.side === 'entry');
  const exitRows = rows.filter((r) => r.kind === 'metric' && r.side === 'exit');

  const clearDrag = () => {
    setDraggingId(null);
    setDropSlot(null);
  };
  const setRow = (id: string, patch: Partial<GenericAxisRow>) =>
    onChange(rows.map((r) => (r.id === id ? { ...r, ...patch } : r)));
  const removeRow = (id: string) => onChange(rows.filter((r) => r.id !== id));
  /** One TP and one SL axis at most: a second would overwrite the first in the rule
   *  while still multiplying the combo count. */
  const addTpSl = (kind: 'take_profit' | 'stop_loss') => {
    if (rows.some((r) => r.kind === kind)) return;
    onChange([...rows, newAxisRow(kind, registry)]);
  };
  const addMetric = (side: MetricAxisSide) => onChange([...rows, newAxisRow('metric', registry, side)]);

  /** Move a metric row to `side`, before `beforeId` (or after that side's last row). */
  const moveMetric = (id: string, side: MetricAxisSide, beforeId: string | null) => {
    const src = rows.find((r) => r.id === id);
    if (!src || src.kind !== 'metric') return;
    const without = rows.filter((r) => r.id !== id);
    let at = without.length;
    if (beforeId) {
      const bi = without.findIndex((r) => r.id === beforeId);
      if (bi >= 0) at = bi;
    } else {
      let last = -1;
      without.forEach((r, i) => {
        if (r.kind === 'metric' && r.side === side) last = i;
      });
      at = last >= 0 ? last + 1 : without.length;
    }
    onChange([...without.slice(0, at), { ...src, side }, ...without.slice(at)]);
  };

  if (!registry) return <p className="text-[11px] text-text-dim/60">Loading strategy registry…</p>;
  const tpPart = rulePart(registry, 'take_profit');
  const slPart = rulePart(registry, 'stop_loss');

  return (
    <div className="flex flex-col gap-2">
      <div className="grid grid-cols-[auto_1fr_auto] items-center gap-2">
        <span className="flex items-center gap-1 text-[10px] font-bold uppercase tracking-wider text-text-dim/80">
          Axes
          <InfoTooltip title={SWEEP_FIELD_HELP.axes.title} body={SWEEP_FIELD_HELP.axes.body} />
        </span>
        <span />
        <Badge variant={combos === 0 ? 'neutral' : 'primary'} className="font-mono">
          ~{combos.toLocaleString()} combos
        </Badge>
      </div>

      {/* TP / SL strip: one slot each; an empty slot is the add button. */}
      <div className="flex flex-col gap-1.5 rounded-md border border-white/10 bg-white/2 p-2">
        <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim/70">TP / SL</span>
        <div className="grid grid-cols-1 gap-1.5 sm:grid-cols-2">
          {(['take_profit', 'stop_loss'] as const).map((kind) => {
            const row = kind === 'take_profit' ? tpRow : slRow;
            const part = kind === 'take_profit' ? tpPart : slPart;
            return (
              <TpSlSlot
                key={kind}
                kind={kind}
                row={row}
                help={part ? `${part.summary}\n\nExample: ${part.example}` : undefined}
                registry={registry}
                onAdd={() => addTpSl(kind)}
                onPatch={(patch) => row && setRow(row.id, patch)}
                onRemove={() => row && removeRow(row.id)}
              />
            );
          })}
        </div>
      </div>

      <div className="grid grid-cols-1 gap-2 xl:grid-cols-2">
        {(['entry', 'exit'] as const).map((side) => (
          <MetricSideColumn
            key={side}
            side={side}
            rows={side === 'entry' ? entryRows : exitRows}
            registry={registry}
            tags={tags}
            draggingId={draggingId}
            dropSlot={dropSlot?.side === side ? dropSlot : null}
            onDragStartRow={setDraggingId}
            onDragEnd={clearDrag}
            onDropSlot={setDropSlot}
            onPatch={setRow}
            onRemove={removeRow}
            onAdd={() => addMetric(side)}
            onMove={moveMetric}
          />
        ))}
      </div>

      {rows.length === 0 && (
        <p className="rounded-md border border-dashed border-white/10 px-3 py-2 text-[12px] text-text-dim">
          No axes yet: add a TP / SL axis above or a metric axis in either column.
        </p>
      )}
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

/** One TP or SL axis slot: empty = the add button; filled = its values box. */
function TpSlSlot({
  kind,
  row,
  help,
  registry,
  onAdd,
  onPatch,
  onRemove,
}: {
  kind: 'take_profit' | 'stop_loss';
  row: GenericAxisRow | undefined;
  /** The registry's text for this rule part. */
  help: string | undefined;
  registry: StrategyRegistry;
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
          'flex min-h-13 items-center justify-center gap-1.5 rounded-md border px-3 py-2 text-[11px] font-semibold tracking-wide transition-colors',
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
    <div className={cn('flex flex-col gap-1 rounded-md border px-2.5 py-2', err ? 'border-red/40 bg-red/5' : style.filled)}>
      <div className="flex items-center gap-2">
        <span
          className={cn(
            'inline-flex shrink-0 items-center gap-0.5 text-[10px] font-bold uppercase tracking-wider',
            err ? 'text-red' : style.accent,
          )}
        >
          {style.short}
          {help && <InfoTooltip title={style.label} body={help} side="top" />}
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
            className={cn('font-mono tabular-nums', err ? 'border-red/70 focus:border-red' : style.input)}
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
      <span className={cn('text-[10px]', err ? 'text-red' : 'text-text-dim')}>{err ?? axisSummary(row)}</span>
    </div>
  );
}

/** Insert-slot line, absolutely placed so it does not shift row hit-testing. */
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
  tags,
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
  registry: StrategyRegistry;
  tags: readonly string[];
  draggingId: string | null;
  dropSlot: DropSlot | null;
  onDragStartRow: (id: string) => void;
  onDragEnd: () => void;
  onDropSlot: (slot: DropSlot | null) => void;
  onPatch: (id: string, patch: Partial<GenericAxisRow>) => void;
  onRemove: (id: string) => void;
  onAdd: () => void;
  onMove: (id: string, side: MetricAxisSide, beforeId: string | null) => void;
}) {
  const part = rulePart(registry, SIDE_PART[side]);
  const showEndIndicator = dropSlot != null && dropSlot.beforeId == null && rows.length > 0;

  /** Insert slot from the pointer Y against each row's midpoint. */
  const slotFromPointerY = (clientY: number, columnEl: HTMLElement): DropSlot => {
    for (const node of columnEl.querySelectorAll<HTMLElement>('[data-axis-row]')) {
      const rect = node.getBoundingClientRect();
      if (clientY < rect.top + rect.height / 2) return { side, beforeId: node.dataset.axisRow || null };
    }
    return { side, beforeId: null };
  };

  /** `null` for a no-op slot (where the dragged row already is). */
  const normalizeSlot = (slot: DropSlot): DropSlot | null => {
    if (!draggingId) return slot;
    const from = rows.findIndex((r) => r.id === draggingId);
    if (from < 0) return slot;
    const to = slot.beforeId ? rows.findIndex((r) => r.id === slot.beforeId) : rows.length;
    return (from < to ? to - 1 : to) === from ? null : slot;
  };

  return (
    <div
      className={cn(
        'flex flex-col gap-1.5 rounded-md border p-2 transition-colors',
        dropSlot ? 'border-primary/50 bg-primary/5' : 'border-white/10 bg-white/2',
      )}
      onDragOver={(e) => {
        if (!isAxisDrag(e.dataTransfer)) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = 'move';
        onDropSlot(normalizeSlot(slotFromPointerY(e.clientY, e.currentTarget)));
      }}
      onDragLeave={(e) => {
        if (e.currentTarget.contains(e.relatedTarget as Node)) return;
        onDropSlot(null);
      }}
      onDrop={(e) => {
        e.preventDefault();
        const id = axisDragId(e.dataTransfer);
        const slot = normalizeSlot(slotFromPointerY(e.clientY, e.currentTarget));
        onDropSlot(null);
        onDragEnd();
        if (id && slot) onMove(id, side, slot.beforeId);
      }}
    >
      <div className="flex items-start justify-between gap-1.5">
        <div className="flex min-w-0 flex-col">
          <span className="inline-flex items-center gap-1 text-[10px] font-bold uppercase tracking-wider text-text-dim/70">
            {side === 'entry' ? 'Entry filters' : 'Exit lines'}
            {part && <InfoTooltip title={part.title} body={`${part.summary}\n\nExample: ${part.example}`} />}
          </span>
          {part && <span className="text-[10px] text-text-dim/60">{part.summary}</span>}
        </div>
        <IconButton variant="success" size="md" onClick={onAdd} title="Add metric axis" aria-label="Add metric axis">
          <PlusIcon />
        </IconButton>
      </div>

      {rows.length === 0 ? (
        <div
          className={cn(
            'rounded border border-dashed px-2 py-3 text-center text-[11px] transition-colors',
            dropSlot ? 'border-primary/50 bg-primary/10 text-primary/80' : 'border-white/10 text-text-dim/50',
          )}
        >
          {dropSlot ? 'Drop here' : 'Drop here or + metric'}
        </div>
      ) : (
        <div className="flex flex-col gap-1.5">
          {rows.map((row, i) => (
            <div key={row.id} className="relative" data-axis-row={row.id}>
              {dropSlot?.beforeId === row.id && <DropIndicator at="before" />}
              {showEndIndicator && i === rows.length - 1 && <DropIndicator at="after" />}
              <MetricAxisRow
                row={row}
                registry={registry}
                tags={tags}
                dragging={draggingId === row.id}
                onDragStartRow={() => onDragStartRow(row.id)}
                onDragEnd={onDragEnd}
                onPatch={(patch) => onPatch(row.id, patch)}
                onRemove={() => onRemove(row.id)}
              />
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

/** One metric axis: the read, the operator, the values, then what it does in words. */
function MetricAxisRow({
  row,
  registry,
  tags,
  dragging,
  onDragStartRow,
  onDragEnd,
  onPatch,
  onRemove,
}: {
  row: GenericAxisRow;
  registry: StrategyRegistry;
  tags: readonly string[];
  dragging: boolean;
  onDragStartRow: () => void;
  onDragEnd: () => void;
  onPatch: (patch: Partial<GenericAxisRow>) => void;
  onRemove: () => void;
}) {
  const err = axisRowError(row, registry, tags);
  const spec = findMetric(registry, row.ref.metric);
  // Before the buy our position does not exist, so an entry axis does not offer it.
  const ctx: CondContext = { reg: registry, tags, signals: [], beforeBuy: row.side === 'entry' };
  const tint: CSSProperties | undefined =
    !err && spec
      ? metricColorStyle({ hue: spec.hue, group: familyName(spec.path), metric: spec.name, operator: row.operator }).style
      : undefined;
  return (
    <div
      className={cn(
        'flex flex-col gap-1 rounded-md border px-2 py-1.5 transition-opacity',
        err ? 'border-red/40 bg-red/5' : tint ? '' : 'border-white/10 bg-surface',
        dragging && 'opacity-40',
      )}
      style={tint ? { borderColor: tint.borderColor, backgroundColor: tint.backgroundColor } : undefined}
    >
      <div className="flex flex-wrap items-center gap-1.5">
        <span
          draggable
          onDragStart={(e) => {
            e.dataTransfer.setData(DND_MIME, row.id);
            e.dataTransfer.setData('text/plain', row.id);
            e.dataTransfer.effectAllowed = 'move';
            onDragStartRow();
          }}
          onDragEnd={onDragEnd}
          className="shrink-0 cursor-grab select-none px-0.5 text-[12px] leading-none text-text-dim/40 active:cursor-grabbing"
          title="Drag to the other column (or reorder)"
          aria-label="Drag axis"
        >
          ⠿
        </span>
        <RefFields r={row.ref} onChange={(ref) => onPatch({ ref })} ctx={ctx} />
        <Cell label="op" tip={SWEEP_FIELD_HELP.axisOp}>
          <Select
            fieldSize="sm"
            value={row.operator}
            onChange={(e) => onPatch({ operator: e.target.value as Operator })}
            className="w-14"
          >
            {registry.operators.map((op) => (
              <option key={op} value={op}>
                {op}
              </option>
            ))}
          </Select>
        </Cell>
        <Cell label="values" grow tip={SWEEP_FIELD_HELP.axisValues}>
          <Input
            fieldSize="sm"
            value={row.valuesText}
            onChange={(e) => onPatch({ valuesText: e.target.value })}
            placeholder="off, 5, 10  ·  10..40 step 10"
            unit={spec ? unitSuffix(spec.unit) || undefined : undefined}
            aria-invalid={!!err}
            className={cn('min-w-32', err && 'border-red/70 focus:border-red')}
          />
        </Cell>
        <button type="button" onClick={onRemove} title="Remove axis" className="ml-auto shrink-0 px-1 text-text-dim hover:text-red">
          ✕
        </button>
      </div>
      {err ? (
        <span className="text-[11px] text-red">{err}</span>
      ) : (
        <span className="text-[11px] text-text-dim">
          {axisSummary(row)}
          <span className="text-text-dim/60"> · reads {readPhrase(registry, row.ref)}</span>
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
    <div className={cn('flex flex-col gap-0.5', grow && 'flex-1')}>
      <span className="inline-flex items-center gap-0.5 text-[8px] uppercase tracking-wider text-text-dim/50">
        {label}
        {tip && <InfoTooltip title={tip.title} body={tip.body} side="top" />}
      </span>
      {children}
    </div>
  );
}
