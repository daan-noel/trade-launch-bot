import { useMemo, useState } from 'react';
import { cn } from 'lib/cn';
import { formatDecimalTrim } from 'utils/format';
import {
  buildTemporalSummary,
  countHeatBackground,
  formatWallSpan,
  formatWallTick,
  holdBarSegments,
  peakWallCell,
  pnlHeatBackground,
  WALL_GRAINS,
  wallGrainLabel,
  type HoldBinStats,
  type TemporalMetric,
  type TemporalRow,
  type TemporalSummaryData,
  type WallCellStats,
  type WallGrain,
  type WallGrainChoice,
  type WallTimeField,
} from 'lib/strategy/temporalSummary';
import { solText } from 'lib/strategy/runSummary';

export type TemporalSelection =
  | { kind: 'hold'; binId: string; mints: string[] }
  | { kind: 'wall'; cellId: string; mints: string[] }
  | null;

type WallColorMode = 'volume' | 'pnl';

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
  /**
   * Controlled grain choice. When `data` is server-built, the parent must refetch
   * on change (sim). With `rows`, the client fold respects this locally.
   */
  wallGrain?: WallGrainChoice;
  onWallGrainChange?: (g: WallGrainChoice) => void;
  className?: string;
}

/**
 * Temporal band under the scalar run summary — hold-duration stacked bars +
 * entry/create wall-clock volume timeline (auto or manual 30m…day grain).
 * Click a bin/cell to filter the positions table; click again / Clear to reset.
 */
export function TemporalSummary({
  data: dataProp,
  rows,
  selection = null,
  onSelect,
  wallField: wallFieldProp,
  onWallFieldChange,
  wallGrain: wallGrainProp,
  onWallGrainChange,
  className,
}: TemporalSummaryProps) {
  const [metric, setMetric] = useState<TemporalMetric>('exit_mix');
  const [wallColor, setWallColor] = useState<WallColorMode>('volume');
  const [localField, setLocalField] = useState<WallTimeField>('created_at');
  const [localGrain, setLocalGrain] = useState<WallGrainChoice>('auto');
  const wallField = wallFieldProp ?? localField;
  const grainChoice = wallGrainProp ?? localGrain;
  const setWallField = (f: WallTimeField) => {
    onWallFieldChange?.(f);
    if (wallFieldProp == null) setLocalField(f);
  };
  const setGrainChoice = (g: WallGrainChoice) => {
    onWallGrainChange?.(g);
    if (wallGrainProp == null) setLocalGrain(g);
  };

  const data = useMemo(() => {
    // Server payload already binned; grain override is applied by refetching.
    if (dataProp) return dataProp;
    if (rows) return buildTemporalSummary(rows, wallField, grainChoice);
    return null;
  }, [dataProp, rows, wallField, grainChoice]);

  if (!data || data.nFired === 0) return null;

  const maxHoldN = Math.max(1, ...data.hold.map((b) => b.n));
  const maxWallN = Math.max(1, ...data.wall.map((c) => c.n));
  const maxAbsPnl = Math.max(
    1e-9,
    ...data.hold.map((b) => Math.abs(b.pnl_sol)),
    ...data.wall.map((c) => Math.abs(c.pnl_sol)),
  );
  const peak = peakWallCell(data.wall);
  const wallTotal = data.wall.reduce((s, c) => s + c.n, 0);
  const autoGrain = data.wallGrainAuto ?? data.wallGrain;

  return (
    <div
      className={cn(
        'mt-4 rounded-lg border border-white/10 bg-white/2 px-3 py-3 sm:px-4 sm:py-4',
        className,
      )}
    >
      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:gap-6">
        <div className="shrink-0 sm:w-48">
          <div className="text-[11px] font-bold uppercase tracking-wider text-text">
            Temporal
          </div>
          <div className="mt-1 text-[11px] leading-snug text-text-mid">
            When tokens clustered and how long they were held
          </div>
          {wallTotal > 0 && (
            <div className="mt-2.5 flex flex-col gap-1.5">
              {peak && (
                <GlanceChip
                  label="peak"
                  value={`${formatWallTick(peak.start, data.wallGrain)} · n=${peak.n}`}
                  active={selection?.kind === 'wall' && selection.cellId === peak.id}
                  onClick={
                    onSelect
                      ? () =>
                          onSelect(
                            selection?.kind === 'wall' && selection.cellId === peak.id
                              ? null
                              : { kind: 'wall', cellId: peak.id, mints: peak.mints },
                          )
                      : undefined
                  }
                />
              )}
              <GlanceChip label="span" value={formatWallSpan(data.wallSpanMs ?? 0)} />
              <GlanceChip
                label="timed"
                value={`${wallTotal} · ${wallGrainLabel(data.wallGrain)}`}
              />
            </div>
          )}
        </div>

        <div className="flex min-w-0 flex-1 flex-col gap-5">
          <div className="flex flex-wrap items-center gap-2">
            <ToggleGroup
              value={metric}
              onChange={setMetric}
              options={[
                { id: 'exit_mix', label: 'Exit mix' },
                { id: 'pnl', label: 'Hold PnL' },
              ]}
            />
            {selection && onSelect && (
              <button
                type="button"
                className="ml-auto text-[11px] font-medium text-primary hover:underline"
                onClick={() => onSelect(null)}
              >
                Clear time filter
              </button>
            )}
          </div>

          <WallTimeline
            cells={data.wall}
            grain={data.wallGrain}
            autoGrain={autoGrain}
            grainChoice={grainChoice}
            onGrainChange={setGrainChoice}
            field={wallField}
            onFieldChange={setWallField}
            maxN={maxWallN}
            maxAbsPnl={maxAbsPnl}
            colorMode={wallColor}
            onColorModeChange={setWallColor}
            selection={selection}
            onSelect={onSelect}
          />

          <HoldBars
            bins={data.hold}
            maxN={maxHoldN}
            maxAbsPnl={maxAbsPnl}
            metric={metric}
            selection={selection}
            onSelect={onSelect}
          />
        </div>
      </div>
    </div>
  );
}

function GlanceChip({
  label,
  value,
  active,
  onClick,
}: {
  label: string;
  value: string;
  active?: boolean;
  onClick?: () => void;
}) {
  const Comp = onClick ? 'button' : 'div';
  return (
    <Comp
      type={onClick ? 'button' : undefined}
      onClick={onClick}
      className={cn(
        'flex w-full items-baseline gap-1.5 rounded-md border px-2 py-1 text-left transition',
        active
          ? 'border-primary/50 bg-primary/15'
          : 'border-white/8 bg-black/25',
        onClick && 'hover:border-white/20 hover:bg-white/5',
      )}
    >
      <span className="shrink-0 text-[9px] font-bold uppercase tracking-wider text-text-dim">
        {label}
      </span>
      <span className="min-w-0 truncate font-mono text-[11px] font-semibold text-text">
        {value}
      </span>
    </Comp>
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
    <div className="flex items-center gap-1 rounded-md bg-white/6 p-0.5">
      {options.map((o) => (
        <button
          key={o.id}
          type="button"
          onClick={() => onChange(o.id)}
          className={cn(
            'rounded px-2.5 py-1 text-[11px] font-medium transition',
            value === o.id ? 'bg-white/12 text-text' : 'text-text-dim hover:text-text-mid',
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
      <div className="mb-2 flex items-baseline justify-between gap-2">
        <span className="text-[11px] font-semibold uppercase tracking-wider text-text-mid">
          By hold duration
        </span>
        <span className="text-[10px] text-text-dim">bar height = count</span>
      </div>
      <div className="flex items-end gap-2">
        {bins.map((b) => {
          const active = selection?.kind === 'hold' && selection.binId === b.id;
          const h = b.n > 0 ? Math.max(14, Math.round((b.n / maxN) * 96)) : 6;
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
                'flex min-w-0 flex-1 flex-col items-center gap-1 rounded-md px-0.5 py-1.5 transition',
                active && 'bg-primary/15 ring-1 ring-primary/60',
                b.n > 0 && onSelect && 'hover:bg-white/5',
                b.n === 0 && 'opacity-40',
              )}
            >
              <span className="font-mono text-[10px] font-semibold text-text-mid">
                {b.n > 0 ? b.n : ''}
              </span>
              <div
                className="flex w-full max-w-16 flex-col justify-end overflow-hidden rounded-sm"
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
                          : 'rgba(255,255,255,0.08)',
                    }}
                  />
                )}
              </div>
              <span className="truncate font-mono text-[10px] text-text-mid">{b.label}</span>
              <span
                className={cn(
                  'font-mono text-[11px] font-bold',
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

function WallTimeline({
  cells,
  grain,
  autoGrain,
  grainChoice,
  onGrainChange,
  field,
  onFieldChange,
  maxN,
  maxAbsPnl,
  colorMode,
  onColorModeChange,
  selection,
  onSelect,
}: {
  cells: WallCellStats[];
  grain: WallGrain;
  autoGrain: WallGrain;
  grainChoice: WallGrainChoice;
  onGrainChange: (g: WallGrainChoice) => void;
  field: WallTimeField;
  onFieldChange: (f: WallTimeField) => void;
  maxN: number;
  maxAbsPnl: number;
  colorMode: WallColorMode;
  onColorModeChange: (m: WallColorMode) => void;
  selection: TemporalSelection;
  onSelect?: (sel: TemporalSelection) => void;
}) {
  const fieldLabel = field === 'entry_time' ? 'entry' : 'created';

  if (cells.length === 0) {
    return (
      <div>
        <WallToolbar
          fieldLabel={fieldLabel}
          field={field}
          onFieldChange={onFieldChange}
          grainChoice={grainChoice}
          autoGrain={autoGrain}
          onGrainChange={onGrainChange}
          colorMode={colorMode}
          onColorModeChange={onColorModeChange}
        />
        <p className="rounded-md border border-dashed border-white/10 px-3 py-4 text-[12px] text-text-dim">
          No timestamps on this cohort.
        </p>
      </div>
    );
  }

  const tickEvery = cells.length > 24 ? Math.ceil(cells.length / 12) : cells.length > 12 ? 2 : 1;
  const barMaxH = 120;
  // count + pnl labels above each bar
  const labelStackH = 28;

  return (
    <div>
      <WallToolbar
        fieldLabel={fieldLabel}
        field={field}
        onFieldChange={onFieldChange}
        grainChoice={grainChoice}
        autoGrain={autoGrain}
        onGrainChange={onGrainChange}
        colorMode={colorMode}
        onColorModeChange={onColorModeChange}
      />

      <div className="rounded-md border border-white/8 bg-black/20 px-2 pt-2 pb-1">
        <div className="mb-1 flex items-center justify-between px-0.5 font-mono text-[9px] text-text-dim">
          <span>peak {maxN}</span>
          <span>
            {cells.filter((c) => c.n > 0).length}/{cells.length} active
          </span>
        </div>
        <div className="overflow-x-auto pb-1">
          <div
            className="grid items-end gap-1"
            style={{
              gridTemplateColumns: `repeat(${cells.length}, minmax(1.6rem, 1fr))`,
              minWidth: Math.min(cells.length * 26, 920),
              height: barMaxH + labelStackH + 14,
            }}
          >
            {cells.map((c, i) => {
              const active = selection?.kind === 'wall' && selection.cellId === c.id;
              const h = c.n > 0 ? Math.max(10, Math.round((c.n / maxN) * barMaxH)) : 3;
              const fill =
                c.n === 0
                  ? 'rgba(255,255,255,0.04)'
                  : colorMode === 'pnl'
                    ? pnlHeatBackground(c.pnl_sol, maxAbsPnl, c.n)
                    : countHeatBackground(c.n, maxN);
              const pnlLabel =
                c.n > 0
                  ? `${c.pnl_sol >= 0 ? '+' : ''}${formatDecimalTrim(c.pnl_sol, 2)}`
                  : '';
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
                    'flex h-full flex-col items-center justify-end gap-0.5 rounded-sm px-0.5 transition',
                    active && 'bg-primary/20 ring-1 ring-primary/70',
                    c.n > 0 && onSelect && 'hover:bg-white/5',
                  )}
                >
                  <span
                    className={cn(
                      'w-full truncate text-center font-mono text-[9px] font-bold leading-none',
                      c.n === 0
                        ? 'text-transparent'
                        : c.pnl_sol > 0
                          ? 'text-green'
                          : c.pnl_sol < 0
                            ? 'text-red'
                            : 'text-text-mid',
                    )}
                  >
                    {pnlLabel || '0'}
                  </span>
                  <span
                    className={cn(
                      'font-mono text-[10px] font-bold leading-none',
                      c.n > 0 ? 'text-text' : 'text-transparent',
                    )}
                  >
                    {c.n > 0 ? c.n : '0'}
                  </span>
                  <div
                    className="w-full rounded-sm shadow-[inset_0_0_0_1px_rgba(255,255,255,0.06)]"
                    style={{ height: h, background: fill }}
                  />
                  {i % tickEvery === 0 ? (
                    <span className="w-full truncate text-center font-mono text-[8px] text-text-mid">
                      {formatWallTick(c.start, grain)}
                    </span>
                  ) : (
                    <span className="h-[10px]" />
                  )}
                </button>
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
}

function WallToolbar({
  fieldLabel,
  field,
  onFieldChange,
  grainChoice,
  autoGrain,
  onGrainChange,
  colorMode,
  onColorModeChange,
}: {
  fieldLabel: string;
  field: WallTimeField;
  onFieldChange: (f: WallTimeField) => void;
  grainChoice: WallGrainChoice;
  autoGrain: WallGrain;
  onGrainChange: (g: WallGrainChoice) => void;
  colorMode: WallColorMode;
  onColorModeChange: (m: WallColorMode) => void;
}) {
  return (
    <div className="mb-2 flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-[11px] font-semibold uppercase tracking-wider text-text">
          By {fieldLabel} time
        </span>
        <FieldToggle field={field} onChange={onFieldChange} />
        <ToggleGroup
          value={colorMode}
          onChange={onColorModeChange}
          options={[
            { id: 'volume', label: 'Volume' },
            { id: 'pnl', label: 'PnL color' },
          ]}
        />
      </div>
      <div className="flex flex-wrap items-center gap-1.5">
        <span className="text-[10px] font-semibold uppercase tracking-wider text-text-dim">
          Grain
        </span>
        <ToggleGroup
          value={grainChoice}
          onChange={onGrainChange}
          options={[
            { id: 'auto', label: `Auto (${wallGrainLabel(autoGrain)})` },
            ...WALL_GRAINS.map((g) => ({ id: g, label: wallGrainLabel(g) })),
          ]}
        />
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
        { id: 'created_at', label: 'Created at' },
        { id: 'entry_time', label: 'Entry time' },
      ]}
    />
  );
}
