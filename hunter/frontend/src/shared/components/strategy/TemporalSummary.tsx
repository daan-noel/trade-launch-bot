import { useMemo, useState } from 'react';
import { cn } from 'lib/cn';
import { formatDecimalTrim } from 'utils/format';
import {
  buildTemporalSummary,
  formatWallTick,
  holdBarSegments,
  pnlHeatBackground,
  type HoldBinStats,
  type TemporalMetric,
  type TemporalRow,
  type TemporalSummaryData,
  type WallCellStats,
  type WallTimeField,
} from 'lib/strategy/temporalSummary';
import { solText } from 'lib/strategy/runSummary';

export type TemporalSelection =
  | { kind: 'hold'; binId: string; mints: string[] }
  | { kind: 'wall'; cellId: string; mints: string[] }
  | null;

interface TemporalSummaryProps {
  /** Pre-built data (sim server path), or omit and pass `rows` for client fold. */
  data?: TemporalSummaryData | null;
  /** Client fold source (sweep drill-in). Ignored when `data` is set. */
  rows?: TemporalRow[];
  selection?: TemporalSelection;
  onSelect?: (sel: TemporalSelection) => void;
  /** Controlled wall-clock field; defaults to internal toggle when omitted. */
  wallField?: WallTimeField;
  onWallFieldChange?: (f: WallTimeField) => void;
  className?: string;
}

/**
 * Temporal band under the scalar run summary — hold-duration stacked bars +
 * entry/create wall-clock heatmap. Click a bin/cell to filter the positions
 * table (caller applies `selection`); click again / Clear to reset.
 */
export function TemporalSummary({
  data: dataProp,
  rows,
  selection = null,
  onSelect,
  wallField: wallFieldProp,
  onWallFieldChange,
  className,
}: TemporalSummaryProps) {
  const [metric, setMetric] = useState<TemporalMetric>('exit_mix');
  const [localField, setLocalField] = useState<WallTimeField>('entry_time');
  const wallField = wallFieldProp ?? localField;
  const setWallField = (f: WallTimeField) => {
    onWallFieldChange?.(f);
    if (wallFieldProp == null) setLocalField(f);
  };

  const data = useMemo(() => {
    if (dataProp) return dataProp;
    if (rows) return buildTemporalSummary(rows, wallField);
    return null;
  }, [dataProp, rows, wallField]);

  if (!data || data.nFired === 0) return null;

  const maxHoldN = Math.max(1, ...data.hold.map((b) => b.n));
  const maxAbsPnl = Math.max(
    1e-9,
    ...data.hold.map((b) => Math.abs(b.pnl_sol)),
    ...data.wall.map((c) => Math.abs(c.pnl_sol)),
  );

  return (
    <div className={cn('mt-4 border-t border-white/6 pt-4', className)}>
      <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:gap-6">
        <div className="shrink-0 sm:w-44">
          <div className="text-[10px] font-bold uppercase tracking-wider text-text-mid">
            Temporal
          </div>
          <div className="mt-0.5 text-[10px] leading-snug text-text-dim">
            Hold duration + when entries clustered
          </div>
        </div>

        <div className="flex min-w-0 flex-1 flex-col gap-4">
          <div className="flex flex-wrap items-center gap-2">
            <ToggleGroup
              value={metric}
              onChange={setMetric}
              options={[
                { id: 'exit_mix', label: 'Exit mix' },
                { id: 'pnl', label: 'Net PnL' },
              ]}
            />
            {selection && onSelect && (
              <button
                type="button"
                className="ml-auto text-[11px] text-text-dim hover:text-primary"
                onClick={() => onSelect(null)}
              >
                Clear time filter
              </button>
            )}
          </div>

          <HoldBars
            bins={data.hold}
            maxN={maxHoldN}
            maxAbsPnl={maxAbsPnl}
            metric={metric}
            selection={selection}
            onSelect={onSelect}
          />

          <WallHeatmap
            cells={data.wall}
            grain={data.wallGrain}
            field={wallField}
            onFieldChange={setWallField}
            maxAbsPnl={maxAbsPnl}
            selection={selection}
            onSelect={onSelect}
          />
        </div>
      </div>
    </div>
  );
}

function ToggleGroup<T extends string>({
  value,
  onChange,
  options,
}: {
  value: T;
  onChange: (v: T) => void;
  options: Array<{ id: T; label: string }>;
}) {
  return (
    <div className="flex items-center gap-1 rounded-md bg-white/4 p-0.5">
      {options.map((o) => (
        <button
          key={o.id}
          type="button"
          onClick={() => onChange(o.id)}
          className={cn(
            'rounded px-2 py-0.5 text-[11px] font-medium transition',
            value === o.id ? 'bg-white/10 text-text' : 'text-text-dim hover:text-text-mid',
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

function HoldBars({
  bins,
  maxN,
  maxAbsPnl,
  metric,
  selection,
  onSelect,
}: {
  bins: HoldBinStats[];
  maxN: number;
  maxAbsPnl: number;
  metric: TemporalMetric;
  selection: TemporalSelection;
  onSelect?: (sel: TemporalSelection) => void;
}) {
  return (
    <div>
      <div className="mb-1.5 text-[10px] font-semibold uppercase tracking-wider text-text-dim">
        By hold
      </div>
      <div className="flex items-end gap-2">
        {bins.map((b) => {
          const active = selection?.kind === 'hold' && selection.binId === b.id;
          const h = b.n > 0 ? Math.max(8, Math.round((b.n / maxN) * 72)) : 4;
          const segs = holdBarSegments(b.exits);
          const title = [
            b.label,
            `n=${b.n}`,
            `pnl=${solText(b.pnl_sol)}`,
            ...segs.map((s) => `${s.label}: ${s.n}`),
          ].join(' · ');
          return (
            <button
              key={b.id}
              type="button"
              title={title}
              disabled={b.n === 0 || !onSelect}
              onClick={() =>
                onSelect?.(active ? null : { kind: 'hold', binId: b.id, mints: b.mints })
              }
              className={cn(
                'flex min-w-0 flex-1 flex-col items-center gap-1 rounded-sm px-0.5 py-1 transition',
                active && 'bg-white/8 ring-1 ring-primary/50',
                b.n > 0 && onSelect && 'hover:bg-white/4',
                b.n === 0 && 'opacity-40',
              )}
            >
              <div
                className="flex w-full max-w-14 flex-col justify-end overflow-hidden rounded-sm"
                style={{ height: h }}
              >
                {metric === 'exit_mix' && b.n > 0 ? (
                  <div className="flex h-full w-full flex-col-reverse">
                    {segs.map((s) => (
                      <div
                        key={s.key}
                        className={cn('w-full', s.bar)}
                        style={{ flexGrow: s.n, flexBasis: 0, minHeight: 2 }}
                      />
                    ))}
                  </div>
                ) : (
                  <div
                    className="h-full w-full rounded-sm"
                    style={{
                      background:
                        metric === 'pnl'
                          ? pnlHeatBackground(b.pnl_sol, maxAbsPnl, b.n)
                          : 'rgba(255,255,255,0.06)',
                    }}
                  />
                )}
              </div>
              <span className="truncate font-mono text-[9px] text-text-dim">{b.label}</span>
              <span
                className={cn(
                  'font-mono text-[10px] font-bold',
                  b.n === 0
                    ? 'text-text-dim'
                    : b.pnl_sol > 0
                      ? 'text-green'
                      : b.pnl_sol < 0
                        ? 'text-red'
                        : 'text-text-mid',
                )}
              >
                {b.n === 0 ? '—' : `${b.pnl_sol >= 0 ? '+' : ''}${formatDecimalTrim(b.pnl_sol, 2)}`}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

function WallHeatmap({
  cells,
  grain,
  field,
  onFieldChange,
  maxAbsPnl,
  selection,
  onSelect,
}: {
  cells: WallCellStats[];
  grain: 'hour' | 'day';
  field: WallTimeField;
  onFieldChange: (f: WallTimeField) => void;
  maxAbsPnl: number;
  selection: TemporalSelection;
  onSelect?: (sel: TemporalSelection) => void;
}) {
  if (cells.length === 0) {
    return (
      <div>
        <div className="mb-1.5 flex flex-wrap items-center gap-2">
          <span className="text-[10px] font-semibold uppercase tracking-wider text-text-dim">
            By time
          </span>
          <FieldToggle field={field} onChange={onFieldChange} />
        </div>
        <p className="text-[11px] text-text-dim">No timestamps on this cohort.</p>
      </div>
    );
  }

  // Dense hour grids get wide — cap visible tick labels.
  const tickEvery = cells.length > 24 ? Math.ceil(cells.length / 12) : cells.length > 12 ? 2 : 1;

  return (
    <div>
      <div className="mb-1.5 flex flex-wrap items-center gap-2">
        <span className="text-[10px] font-semibold uppercase tracking-wider text-text-dim">
          By {field === 'entry_time' ? 'entry' : 'created'} ({grain})
        </span>
        <FieldToggle field={field} onChange={onFieldChange} />
      </div>
      <div className="overflow-x-auto pb-1">
        <div
          className="grid gap-0.5"
          style={{
            gridTemplateColumns: `repeat(${cells.length}, minmax(1.1rem, 1fr))`,
            minWidth: Math.min(cells.length * 18, 720),
          }}
        >
          {cells.map((c, i) => {
            const active = selection?.kind === 'wall' && selection.cellId === c.id;
            const title = [
              formatWallTick(c.start, grain),
              `n=${c.n}`,
              c.n > 0 ? `win ${(c.win_rate * 100).toFixed(0)}%` : null,
              c.n > 0 ? `pnl ${solText(c.pnl_sol)}` : null,
              c.dominant ? `dom ${c.dominant}` : null,
            ]
              .filter(Boolean)
              .join(' · ');
            return (
              <button
                key={c.id}
                type="button"
                title={title}
                disabled={c.n === 0 || !onSelect}
                onClick={() =>
                  onSelect?.(active ? null : { kind: 'wall', cellId: c.id, mints: c.mints })
                }
                className={cn(
                  'flex flex-col items-center gap-0.5',
                  active && 'ring-1 ring-primary/60 ring-offset-1 ring-offset-transparent',
                )}
              >
                <div
                  className="h-7 w-full rounded-sm"
                  style={{ background: pnlHeatBackground(c.pnl_sol, maxAbsPnl, c.n) }}
                />
                {i % tickEvery === 0 && (
                  <span className="truncate font-mono text-[8px] text-text-dim">
                    {formatWallTick(c.start, grain)}
                  </span>
                )}
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}

function FieldToggle({
  field,
  onChange,
}: {
  field: WallTimeField;
  onChange: (f: WallTimeField) => void;
}) {
  return (
    <ToggleGroup
      value={field}
      onChange={onChange}
      options={[
        { id: 'entry_time', label: 'Entry time' },
        { id: 'created_at', label: 'Created at' },
      ]}
    />
  );
}
