import { useMemo, type ReactNode } from 'react';
import { cn } from 'lib/cn';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { Button } from 'components/ui/Button';
import { Badge } from 'components/ui/Badge';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { unitSuffix, useStrategyRegistry, type StrategyRegistry } from 'lib/strategy/registry';
import {
  axisRowError,
  comboCount,
  newAxisRow,
  rowNeedsWindow,
  sharedWindowError,
  type AxisKind,
  type GenericAxisRow,
  type MetricAxisSide,
} from './genericAxes';

/** The three axis kinds, in the order the "+ add" menu offers them. */
const AXIS_KINDS: { value: AxisKind; label: string }[] = [
  { value: 'metric', label: 'metric condition' },
  { value: 'take_profit', label: 'take profit %' },
  { value: 'stop_loss', label: 'stop loss %' },
];

const KIND_LABEL: Record<AxisKind, string> = {
  metric: 'metric',
  take_profit: 'TP %',
  stop_loss: 'SL %',
};

export interface GenericAxisBuilderProps {
  rows: GenericAxisRow[];
  onChange: (rows: GenericAxisRow[]) => void;
  /** Projected combos for this axis set (rendered as a badge; the config form
   *  reuses the same number for the over-cap gate). */
  projected?: number;
}

/**
 * The registry-driven sweep axis builder (redesign FE5.1). Each row is one swept
 * dimension: a `(side, group, metric, operator, values[, window])` condition, or
 * a TP / SL % axis. Groups / metrics / operators come from
 * `GET /api/meta/strategy-registry`, so a metric added in Rust appears here on
 * the next load. Values accept a comma list (`5, 10, 15`) and ranges
 * (`10..40 step 10`). Replaces the three static `*_AXES` grids.
 */
export function GenericAxisBuilder({ rows, onChange, projected }: GenericAxisBuilderProps) {
  const { data: registry } = useStrategyRegistry();

  const combos = useMemo(() => projected ?? comboCount(rows, registry), [projected, rows, registry]);
  const windowErr = useMemo(() => sharedWindowError(rows, registry), [rows, registry]);

  const setRow = (id: string, patch: Partial<GenericAxisRow>) =>
    onChange(rows.map((r) => (r.id === id ? { ...r, ...patch } : r)));
  const removeRow = (id: string) => onChange(rows.filter((r) => r.id !== id));
  const addRow = (kind: AxisKind) => onChange([...rows, newAxisRow(kind, registry)]);

  return (
    <div className="flex flex-col gap-2">
      <div className="grid grid-cols-[auto_1fr_auto] items-center gap-2">
        <span className="flex items-center gap-1 text-[10px] font-bold uppercase tracking-wider text-text-dim/80">
          Axes
          <InfoTooltip
            title="Sweep axes"
            body="Each row is one swept dimension. A combo picks one value per axis; the values assemble one rule. Values accept a comma list (5, 10, 15), ranges (10..40 step 10), and — on metric rows — 'off' (that combo omits the condition, sweeping with-vs-without). Entry axes vary slowest so combos stay contiguous."
          />
        </span>
        <span />
        <Badge variant={combos === 0 ? 'neutral' : 'primary'} className="font-mono">
          ~{combos.toLocaleString()} combos
        </Badge>
      </div>

      {rows.length === 0 && (
        <p className="rounded-md border border-dashed border-white/10 px-3 py-2 text-[12px] text-text-dim">
          No axes yet — add a metric condition or a TP / SL axis below.
        </p>
      )}

      <div className="flex flex-col gap-1.5">
        {rows.map((row) => (
          <AxisRow
            key={row.id}
            row={row}
            registry={registry}
            onPatch={(patch) => setRow(row.id, patch)}
            onRemove={() => removeRow(row.id)}
          />
        ))}
      </div>

      {windowErr && <p className="text-[11px] text-red">{windowErr}</p>}

      <div className="flex flex-wrap items-center gap-1.5">
        {AXIS_KINDS.map((k) => (
          <Button key={k.value} variant="subtle" size="xs" onClick={() => addRow(k.value)}>
            + {k.label}
          </Button>
        ))}
      </div>
    </div>
  );
}

/** One editable axis row. */
function AxisRow({
  row,
  registry,
  onPatch,
  onRemove,
}: {
  row: GenericAxisRow;
  registry: StrategyRegistry | undefined;
  onPatch: (patch: Partial<GenericAxisRow>) => void;
  onRemove: () => void;
}) {
  const err = axisRowError(row, registry);
  const isMetric = row.kind === 'metric';
  const group = registry?.groups.find((g) => g.name === row.group);
  const metric = group?.metrics.find((m) => m.name === row.metric);
  const needsWindow = rowNeedsWindow(row, registry);
  const valueUnit = metric ? unitSuffix(metric.unit) : row.kind !== 'metric' ? '%' : '';

  // Changing the group resets the metric (metrics are group-scoped).
  const onGroup = (name: string) => {
    const g = registry?.groups.find((gg) => gg.name === name);
    onPatch({ group: name, metric: g?.metrics[0]?.name ?? '' });
  };

  return (
    <div
      className={cn(
        'flex flex-wrap items-center gap-1.5 rounded-md border px-2 py-1.5',
        err ? 'border-red/40 bg-red/5' : 'border-white/10 bg-surface',
      )}
    >
      {/* Kind badge (fixed once added — remove + re-add to change kind). */}
      <span className="w-14 shrink-0 text-[10px] font-bold uppercase tracking-wider text-text-dim/70">
        {KIND_LABEL[row.kind]}
      </span>

      {isMetric ? (
        <>
          <Cell label="side">
            <Select
              fieldSize="sm"
              value={row.side}
              onChange={(e) => onPatch({ side: e.target.value as MetricAxisSide })}
              className="w-20"
            >
              <option value="entry">entry</option>
              <option value="exit">exit</option>
            </Select>
          </Cell>
          <Cell label="group">
            <Select
              fieldSize="sm"
              value={row.group}
              onChange={(e) => onGroup(e.target.value)}
              className="w-36"
            >
              <option value="">group…</option>
              {registry?.groups.map((g) => (
                <option key={g.name} value={g.name}>
                  {g.name}
                </option>
              ))}
            </Select>
          </Cell>
          <Cell label="metric">
            <Select
              fieldSize="sm"
              value={row.metric}
              onChange={(e) => onPatch({ metric: e.target.value })}
              className="w-32"
              disabled={!group}
            >
              <option value="">metric…</option>
              {group?.metrics.map((m) => (
                <option key={m.name} value={m.name}>
                  {m.name}
                </option>
              ))}
            </Select>
          </Cell>
          <Cell label="op">
            <Select
              fieldSize="sm"
              value={row.operator}
              onChange={(e) => onPatch({ operator: e.target.value as GenericAxisRow['operator'] })}
              className="w-16"
            >
              {registry?.operators.map((op) => (
                <option key={op} value={op}>
                  {op}
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
                onChange={(e) => onPatch({ window: e.target.value })}
                placeholder="10"
                className="w-20"
              />
            </Cell>
          )}
        </>
      ) : null}

      <Cell label="values" grow>
        <Input
          fieldSize="sm"
          value={row.valuesText}
          onChange={(e) => onPatch({ valuesText: e.target.value })}
          placeholder={isMetric ? 'off, 5, 10  ·  10..40 step 10' : '50, 100, 200'}
          unit={valueUnit || undefined}
          aria-invalid={!!err}
          className={cn('min-w-[10rem]', err && 'border-red/70 focus:border-red')}
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
function Cell({ label, grow, children }: { label: string; grow?: boolean; children: ReactNode }) {
  return (
    <div className={cn('flex flex-col gap-0.5', grow && 'flex-1')}>
      <span className="text-[8px] uppercase tracking-wider text-text-dim/50">{label}</span>
      {children}
    </div>
  );
}
