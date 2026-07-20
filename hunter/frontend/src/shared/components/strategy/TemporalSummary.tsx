import { useMemo, useState } from 'react';
import { cn } from 'lib/cn';
import { formatDecimalTrim } from 'utils/format';
import {
  alignHoldBins,
  alignWallCells,
  avgPnlSol,
  bestPnlWallCell,
  buildTemporalSummary,
  closedCount,
  countHeatBackground,
  formatWallClock,
  formatWallDate,
  formatWallRange,
  formatWallSpan,
  formatWallTick,
  holdBarSegments,
  holdSchemeLabel,
  HOLD_SCHEMES,
  isWallDayBreak,
  peakWallCell,
  pnlHeatBackground,
  rebucketHold,
  rebucketWall,
  WALL_GRAINS,
  wallGrainLabel,
  worstPnlWallCell,
  type HoldBinStats,
  type HoldScheme,
  type HoldSchemeChoice,
  type TemporalMetric,
  type TemporalRow,
  type TemporalSummaryData,
  type WallCellStats,
  type WallGrain,
  type WallGrainChoice,
  type WallTimeField,
} from 'lib/strategy/temporalSummary';
import { signedToneClass } from 'lib/signedTone';
import { solText } from 'lib/strategy/runSummary';

export type TemporalSelection =
  | { kind: 'hold'; binId: string; mints: string[] }
  | { kind: 'wall'; cellId: string; mints: string[] }
  | null;

type WallColorMode = 'volume' | 'pnl';

interface TemporalSummaryProps {
  /** Pre-built base cohort (sim server path), or omit and pass `rows` for client fold. */
  data?: TemporalSummaryData | null;
  /**
   * Client fold source (sweep drill-in). Also used to compute the linked chart
   * when a bin/cell is selected. Ignored for the base fold when `data` is set.
   */
  rows?: TemporalRow[];
  /**
   * Server-built linked cohort (sim): same grain/scheme as `data`, filtered to the
   * selection's mints. Used when `rows` is absent so the other chart can rebin.
   */
  linkedData?: TemporalSummaryData | null;
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
  /**
   * Controlled hold-duration scheme. When `data` is server-built, the parent must
   * refetch on change (sim). With `rows`, the client fold respects this locally.
   */
  holdScheme?: HoldSchemeChoice;
  onHoldSchemeChange?: (s: HoldSchemeChoice) => void;
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
  linkedData,
  selection = null,
  onSelect,
  wallField: wallFieldProp,
  onWallFieldChange,
  wallGrain: wallGrainProp,
  onWallGrainChange,
  holdScheme: holdSchemeProp,
  onHoldSchemeChange,
  className,
}: TemporalSummaryProps) {
  const [metric, setMetric] = useState<TemporalMetric>('exit_mix');
  const [wallColor, setWallColor] = useState<WallColorMode>('volume');
  const [localField, setLocalField] = useState<WallTimeField>('created_at');
  const [localGrain, setLocalGrain] = useState<WallGrainChoice>('auto');
  const [localHold, setLocalHold] = useState<HoldSchemeChoice>('auto');
  const wallField = wallFieldProp ?? localField;
  const grainChoice = wallGrainProp ?? localGrain;
  const holdChoice = holdSchemeProp ?? localHold;
  const setWallField = (f: WallTimeField) => {
    onWallFieldChange?.(f);
    if (wallFieldProp == null) setLocalField(f);
  };
  const setGrainChoice = (g: WallGrainChoice) => {
    onWallGrainChange?.(g);
    if (wallGrainProp == null) setLocalGrain(g);
  };
  const setHoldChoice = (s: HoldSchemeChoice) => {
    onHoldSchemeChange?.(s);
    if (holdSchemeProp == null) setLocalHold(s);
    // Bin ids change with the scheme — drop a stale hold filter.
    if (selection?.kind === 'hold') onSelect?.(null);
  };

  const base = useMemo(() => {
    // Server payload already binned; grain/scheme overrides are applied by refetching.
    if (dataProp) {
      return {
        ...dataProp,
        holdScheme: dataProp.holdScheme ?? 'mid_30m',
        holdSchemeAuto: dataProp.holdSchemeAuto ?? dataProp.holdScheme ?? 'mid_30m',
        wallGrainAuto: dataProp.wallGrainAuto ?? dataProp.wallGrain,
        wallSpanMs: dataProp.wallSpanMs ?? 0,
      } satisfies TemporalSummaryData;
    }
    if (rows) return buildTemporalSummary(rows, wallField, grainChoice, holdChoice);
    return null;
  }, [dataProp, rows, wallField, grainChoice, holdChoice]);

  // Linked brush: selection on one chart rebins the other over the same mint set.
  const linked = useMemo(() => {
    if (!base || !selection?.mints.length) return null;
    if (rows) {
      const set = new Set(selection.mints);
      const filtered = rows.filter((r) => set.has(r.mint_address));
      if (selection.kind === 'wall') {
        return { hold: rebucketHold(base.holdScheme, filtered), wall: null as WallCellStats[] | null };
      }
      return {
        hold: null as HoldBinStats[] | null,
        wall: rebucketWall(base.wall, wallField, base.wallGrain, filtered),
      };
    }
    if (linkedData) {
      if (selection.kind === 'wall') {
        return { hold: alignHoldBins(base.hold, linkedData.hold), wall: null };
      }
      return { hold: null, wall: alignWallCells(base.wall, linkedData.wall) };
    }
    return null;
  }, [base, selection, rows, linkedData, wallField]);

  if (!base || base.nFired === 0) return null;

  const displayHold = linked?.hold ?? base.hold;
  const displayWall = linked?.wall ?? base.wall;
  const ghostHold = linked?.hold ? base.hold : null;
  const ghostWall = linked?.wall ? base.wall : null;

  const maxHoldN = Math.max(1, ...base.hold.map((b) => b.n));
  const maxWallN = Math.max(1, ...base.wall.map((c) => c.n));
  const maxAbsPnl = Math.max(
    1e-9,
    ...base.hold.map((b) => Math.abs(b.pnl_sol)),
    ...base.wall.map((c) => Math.abs(c.pnl_sol)),
  );
  const peak = peakWallCell(base.wall);
  const bestPnl = bestPnlWallCell(base.wall);
  const worstPnl = worstPnlWallCell(base.wall);
  const wallTotal = base.wall.reduce((s, c) => s + c.n, 0);
  const autoGrain = base.wallGrainAuto ?? base.wallGrain;
  const selectedSlice = resolveSelection(base, selection);
  const wallLinkHint = (() => {
    if (selection?.kind !== 'hold') return null;
    const bin = base.hold.find((b) => b.id === selection.binId);
    return bin ? `in hold ${bin.label}` : null;
  })();
  const holdLinkHint = (() => {
    if (selection?.kind !== 'wall') return null;
    const cell = base.wall.find((c) => c.id === selection.cellId);
    return cell ? `in ${formatWallTick(cell.start, base.wallGrain)}` : null;
  })();

  const selectWall = (cell: WallCellStats) => {
    if (!onSelect) return;
    onSelect(
      selection?.kind === 'wall' && selection.cellId === cell.id
        ? null
        : { kind: 'wall', cellId: cell.id, mints: cell.mints },
    );
  };

  return (
    <div
      className={cn(
        'mb-5 mt-5 rounded-xl border border-info/25 bg-linear-to-b from-info/10 via-white/4 to-transparent px-3 py-4 shadow-[inset_0_1px_0_rgba(56,189,248,0.12)] sm:px-5 sm:py-5',
        className,
      )}
    >
      <div className="mb-3 flex flex-wrap items-end gap-x-4 gap-y-2">
        <div className="flex min-w-0 items-center gap-2.5">
          <span className="h-5 w-1.5 shrink-0 rounded-full bg-info" />
          <div className="min-w-0">
            <h3 className="text-base font-bold tracking-tight text-text">Temporal pattern</h3>
            <p className="text-[11px] leading-snug text-text-mid">
              Click a time candle or hold bin — the other chart and the table follow
            </p>
          </div>
        </div>
        {selection && onSelect && (
          <button
            type="button"
            className="ml-auto rounded-md border border-primary/40 bg-primary/15 px-2.5 py-1 text-[11px] font-semibold text-primary hover:bg-primary/25"
            onClick={() => onSelect(null)}
          >
            Clear time filter
          </button>
        )}
      </div>

      {wallTotal > 0 && (
        <div className="mb-3 flex flex-wrap gap-2">
          {peak && (
            <InsightChip
              label="Peak volume"
              value={`${formatWallTick(peak.start, base.wallGrain)} · n=${peak.n}`}
              tone="info"
              active={selection?.kind === 'wall' && selection.cellId === peak.id}
              onClick={onSelect ? () => selectWall(peak) : undefined}
            />
          )}
          {bestPnl && (
            <InsightChip
              label="Best PnL"
              value={`${formatWallTick(bestPnl.start, base.wallGrain)} · ${pnlText(bestPnl.pnl_sol)}`}
              tone="good"
              active={selection?.kind === 'wall' && selection.cellId === bestPnl.id}
              onClick={onSelect ? () => selectWall(bestPnl) : undefined}
            />
          )}
          {worstPnl && worstPnl.id !== bestPnl?.id && (
            <InsightChip
              label="Worst PnL"
              value={`${formatWallTick(worstPnl.start, base.wallGrain)} · ${pnlText(worstPnl.pnl_sol)}`}
              tone="bad"
              active={selection?.kind === 'wall' && selection.cellId === worstPnl.id}
              onClick={onSelect ? () => selectWall(worstPnl) : undefined}
            />
          )}
          <InsightChip label="Span" value={formatWallSpan(base.wallSpanMs ?? 0)} />
          <InsightChip
            label="Timed"
            value={`${wallTotal} · ${wallGrainLabel(base.wallGrain)}`}
          />
        </div>
      )}

      {selectedSlice && (
        <SelectionInspector
          slice={selectedSlice}
          onClear={onSelect ? () => onSelect(null) : undefined}
        />
      )}

      <div className="rounded-lg border border-white/12 bg-black/30 px-3 py-3 sm:px-4 sm:py-4">
        <div className="flex flex-col gap-7">
          <WallTimeline
            cells={displayWall}
            ghostCells={ghostWall}
            linkHint={wallLinkHint}
            grain={base.wallGrain}
            autoGrain={autoGrain}
            grainChoice={grainChoice}
            onGrainChange={setGrainChoice}
            field={wallField}
            onFieldChange={setWallField}
            maxN={maxWallN}
            maxAbsPnl={maxAbsPnl}
            colorMode={wallColor}
            onColorModeChange={setWallColor}
            peakId={peak?.id ?? null}
            bestId={bestPnl?.id ?? null}
            worstId={worstPnl?.id ?? null}
            selection={selection}
            onSelect={onSelect}
          />

          <HoldBars
            bins={displayHold}
            ghostBins={ghostHold}
            linkHint={holdLinkHint}
            holdScheme={base.holdScheme}
            holdSchemeAuto={base.holdSchemeAuto}
            holdChoice={holdChoice}
            onHoldChoiceChange={setHoldChoice}
            maxN={maxHoldN}
            maxAbsPnl={maxAbsPnl}
            metric={metric}
            onMetricChange={setMetric}
            selection={selection}
            onSelect={onSelect}
          />
        </div>
      </div>
    </div>
  );
}

type SliceView = {
  kind: 'hold' | 'wall';
  title: string;
  subtitle: string;
  n: number;
  pnl_sol: number;
  win_rate: number | null;
  dominant: string | null;
  exits: Record<string, number>;
};

function resolveSelection(
  data: TemporalSummaryData,
  selection: TemporalSelection,
): SliceView | null {
  if (!selection) return null;
  if (selection.kind === 'hold') {
    const bin = data.hold.find((b) => b.id === selection.binId);
    if (!bin || bin.n === 0) return null;
    return {
      kind: 'hold',
      title: `Hold ${bin.label}`,
      subtitle: 'Hold-duration slice',
      n: bin.n,
      pnl_sol: bin.pnl_sol,
      win_rate: null,
      dominant: dominantFromExits(bin.exits),
      exits: bin.exits,
    };
  }
  const cell = data.wall.find((c) => c.id === selection.cellId);
  if (!cell || cell.n === 0) return null;
  return {
    kind: 'wall',
    title: formatWallTick(cell.start, data.wallGrain),
    subtitle: `Wall-clock · ${wallGrainLabel(data.wallGrain)}`,
    n: cell.n,
    pnl_sol: cell.pnl_sol,
    win_rate: cell.win_rate,
    dominant: cell.dominant,
    exits: cell.exits,
  };
}

function dominantFromExits(exits: Record<string, number>): string | null {
  const segs = holdBarSegments(exits);
  if (segs.length === 0) return null;
  let best = segs[0];
  for (const s of segs) if (s.n > best.n) best = s;
  return best.label;
}

function SelectionInspector({
  slice,
  onClear,
}: {
  slice: SliceView;
  onClear?: () => void;
}) {
  const avg = avgPnlSol(slice.pnl_sol, slice.n);
  const closed = closedCount(slice.n, slice.exits);
  const segs = holdBarSegments(slice.exits);

  return (
    <div className="mb-3 rounded-lg border border-primary/35 bg-primary/10 px-3 py-3 sm:px-4">
      <div className="flex flex-wrap items-start gap-x-6 gap-y-3">
        <div className="min-w-[9rem]">
          <div className="text-[10px] font-bold uppercase tracking-wider text-primary">
            Selected · {slice.subtitle}
          </div>
          <div className="mt-0.5 font-mono text-sm font-bold text-text">{slice.title}</div>
          {slice.dominant && (
            <div className="mt-1 text-[11px] text-text-mid">
              Dominant exit · <span className="font-semibold text-text">{slice.dominant}</span>
            </div>
          )}
        </div>

        <Stat label="n" value={String(slice.n)} />
        <Stat
          label="PnL"
          value={pnlText(slice.pnl_sol)}
          cls={signedToneClass(slice.pnl_sol)}
        />
        <Stat
          label="Avg"
          value={avg == null ? '—' : pnlText(avg)}
          cls={avg == null ? undefined : signedToneClass(avg)}
        />
        {slice.win_rate != null && closed > 0 && (
          <Stat label="Win" value={`${Math.round(slice.win_rate * 100)}%`} />
        )}
        <Stat label="Closed" value={`${closed}/${slice.n}`} />

        <div className="min-w-[12rem] flex-1">
          <div className="mb-1 text-[10px] font-semibold uppercase tracking-wider text-text-dim">
            Exit mix
          </div>
          {segs.length > 0 ? (
            <>
              <div className="flex h-2.5 overflow-hidden rounded-sm bg-black/30">
                {segs.map((s) => (
                  <div
                    key={s.key}
                    className={cn('h-full', s.bar)}
                    style={{ flexGrow: s.n, flexBasis: 0 }}
                    title={`${s.label}: ${s.n}`}
                  />
                ))}
              </div>
              <ExitLegend segments={segs} className="mt-1.5" />
            </>
          ) : (
            <div className="text-[11px] text-text-dim">No exits in slice</div>
          )}
        </div>

        {onClear && (
          <button
            type="button"
            className="ml-auto self-start text-[11px] font-medium text-primary hover:underline"
            onClick={onClear}
          >
            Clear
          </button>
        )}
      </div>
    </div>
  );
}

function Stat({
  label,
  value,
  cls,
}: {
  label: string;
  value: string;
  cls?: string;
}) {
  return (
    <div className="flex flex-col gap-0.5">
      <span className="text-[10px] font-semibold uppercase tracking-wider text-text-dim">
        {label}
      </span>
      <span className={cn('font-mono text-base font-bold leading-none text-text', cls)}>
        {value}
      </span>
    </div>
  );
}

function InsightChip({
  label,
  value,
  tone,
  active,
  onClick,
}: {
  label: string;
  value: string;
  tone?: 'info' | 'good' | 'bad';
  active?: boolean;
  onClick?: () => void;
}) {
  const Comp = onClick ? 'button' : 'div';
  return (
    <Comp
      type={onClick ? 'button' : undefined}
      onClick={onClick}
      className={cn(
        'flex min-w-0 items-baseline gap-1.5 rounded-md border px-2.5 py-1.5 text-left transition',
        active
          ? 'border-primary/55 bg-primary/18 ring-1 ring-primary/40'
          : tone === 'good'
            ? 'border-green/25 bg-green/10'
            : tone === 'bad'
              ? 'border-red/25 bg-red/10'
              : tone === 'info'
                ? 'border-info/30 bg-info/10'
                : 'border-white/10 bg-black/30',
        onClick && !active && 'hover:border-white/25 hover:bg-white/5',
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

function ExitLegend({
  segments,
  className,
}: {
  segments: Array<{ key: string; n: number; bar: string; label: string }>;
  className?: string;
}) {
  if (segments.length === 0) return null;
  return (
    <div className={cn('flex flex-wrap gap-x-2.5 gap-y-1', className)}>
      {segments.map((s) => (
        <span key={s.key} className="inline-flex items-center gap-1 text-[10px] text-text-mid">
          <span className={cn('h-1.5 w-1.5 shrink-0 rounded-sm', s.bar)} />
          <span>
            {s.label} {s.n}
          </span>
        </span>
      ))}
    </div>
  );
}

function HoldBars({
  bins,
  ghostBins,
  linkHint,
  holdScheme,
  holdSchemeAuto,
  holdChoice,
  onHoldChoiceChange,
  maxN,
  maxAbsPnl,
  metric,
  onMetricChange,
  selection,
  onSelect,
}: {
  bins: HoldBinStats[];
  ghostBins?: HoldBinStats[] | null;
  linkHint?: string | null;
  holdScheme: HoldScheme;
  holdSchemeAuto: HoldScheme;
  holdChoice: HoldSchemeChoice;
  onHoldChoiceChange: (s: HoldSchemeChoice) => void;
  maxN: number;
  maxAbsPnl: number;
  metric: TemporalMetric;
  onMetricChange: (m: TemporalMetric) => void;
  selection: TemporalSelection;
  onSelect?: (sel: TemporalSelection) => void;
}) {
  const totalPnlAbs = bins.reduce((s, b) => s + Math.abs(b.pnl_sol), 0) || 1;
  const ghostById = useMemo(() => {
    if (!ghostBins) return null;
    return new Map(ghostBins.map((b) => [b.id, b]));
  }, [ghostBins]);
  const legendSegs = useMemo(() => {
    const merged: Record<string, number> = {};
    for (const b of bins) {
      for (const s of holdBarSegments(b.exits)) {
        merged[s.key] = (merged[s.key] ?? 0) + s.n;
      }
    }
    return holdBarSegments(merged);
  }, [bins]);
  const barMaxH = 112;

  return (
    <div>
      <div className="mb-2 flex flex-wrap items-center gap-2">
        <span className="text-[11px] font-semibold uppercase tracking-wider text-text">
          By hold duration
        </span>
        {linkHint && (
          <span className="rounded-md border border-primary/35 bg-primary/15 px-2 py-0.5 font-mono text-[10px] font-semibold text-primary">
            {linkHint}
          </span>
        )}
        <ToggleGroup
          value={holdChoice === 'auto' ? 'auto' : 'manual'}
          onChange={(v) => {
            if (v === 'auto') onHoldChoiceChange('auto');
            else if (holdChoice === 'auto') onHoldChoiceChange(holdSchemeAuto);
          }}
          options={[
            { id: 'auto', label: `Auto (${holdSchemeLabel(holdSchemeAuto)})` },
            { id: 'manual', label: 'Manual' },
          ]}
        />
        {holdChoice !== 'auto' && (
          <select
            value={holdScheme}
            onChange={(e) => onHoldChoiceChange(e.target.value as HoldScheme)}
            className="rounded-md border border-white/12 bg-black/40 px-2 py-1 font-mono text-[11px] text-text"
          >
            {HOLD_SCHEMES.map((s) => (
              <option key={s} value={s}>
                {holdSchemeLabel(s)}
              </option>
            ))}
          </select>
        )}
        <ToggleGroup
          value={metric}
          onChange={onMetricChange}
          options={[
            { id: 'exit_mix', label: 'Exit mix' },
            { id: 'pnl', label: 'Hold PnL' },
          ]}
        />
        <span className="text-[10px] text-text-dim">
          {ghostById ? 'faint = full cohort · solid = selection' : 'height = count · click to filter'}
        </span>
      </div>
      {metric === 'exit_mix' && <ExitLegend segments={legendSegs} className="mb-2" />}
      <div className="flex items-end gap-2">
        {bins.map((b) => {
          const active = selection?.kind === 'hold' && selection.binId === b.id;
          const ghost = ghostById?.get(b.id);
          const ghostH =
            ghost && ghost.n > 0 ? Math.max(18, Math.round((ghost.n / maxN) * barMaxH)) : 0;
          const h = b.n > 0 ? Math.max(14, Math.round((b.n / maxN) * barMaxH)) : ghostH > 0 ? 0 : 6;
          const stackH = Math.max(ghostH, h, 6);
          const segs = holdBarSegments(b.exits);
          const avg = avgPnlSol(b.pnl_sol, b.n);
          const share = b.n > 0 ? Math.round((Math.abs(b.pnl_sol) / totalPnlAbs) * 100) : 0;
          const title = [
            b.label,
            `n=${b.n}`,
            ghost ? `cohort n=${ghost.n}` : null,
            `pnl=${solText(b.pnl_sol)}`,
            avg != null ? `avg=${solText(avg)}` : null,
            ...segs.map((s) => `${s.label}: ${s.n}`),
          ]
            .filter(Boolean)
            .join(' · ');
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
              <div className="relative w-full max-w-16" style={{ height: stackH }}>
                {ghostH > 0 && (
                  <div
                    className="absolute bottom-0 w-full rounded-sm bg-white/15"
                    style={{ height: ghostH }}
                  />
                )}
                {h > 0 && (
                  <div
                    className="absolute bottom-0 flex w-full flex-col justify-end overflow-hidden rounded-sm"
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
                              : 'rgba(56,189,248,0.75)',
                        }}
                      />
                    )}
                  </div>
                )}
              </div>
              <span className="truncate font-mono text-[10px] text-text-mid">{b.label}</span>
              <span
                className={cn(
                  'font-mono text-[11px] font-bold',
                  b.n === 0 ? 'text-text-dim' : signedToneClass(b.pnl_sol),
                )}
              >
                {b.n === 0 ? '—' : pnlText(b.pnl_sol)}
              </span>
              {b.n > 0 && (
                <span className="font-mono text-[9px] text-text-dim">
                  {metric === 'pnl' ? `${share}% |pnl|` : avg != null ? `avg ${pnlText(avg)}` : ''}
                </span>
              )}
            </button>
          );
        })}
      </div>
    </div>
  );
}

function WallTimeline({
  cells,
  ghostCells,
  linkHint,
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
  peakId,
  bestId,
  worstId,
  selection,
  onSelect,
}: {
  cells: WallCellStats[];
  ghostCells?: WallCellStats[] | null;
  linkHint?: string | null;
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
  peakId: string | null;
  bestId: string | null;
  worstId: string | null;
  selection: TemporalSelection;
  onSelect?: (sel: TemporalSelection) => void;
}) {
  const fieldLabel = field === 'entry_time' ? 'entry' : 'created';
  const ghostById = useMemo(() => {
    if (!ghostCells) return null;
    return new Map(ghostCells.map((c) => [c.id, c]));
  }, [ghostCells]);

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

  // Aim for ~14 readable ticks; always label day breaks + first/last.
  const tickEvery =
    cells.length <= 14 ? 1 : cells.length <= 28 ? 2 : Math.ceil(cells.length / 14);
  const barMaxH = 132;
  const colMinPx = grain === 'day' ? 44 : 40;
  const gridMinWidth = Math.max(cells.length * colMinPx, 320);
  const rangeSource = ghostCells ?? cells;
  const rangeLabel = formatWallRange(rangeSource, grain);
  const selectedCell =
    selection?.kind === 'wall'
      ? (ghostCells ?? cells).find((c) => c.id === selection.cellId)
      : undefined;

  return (
    <div>
      {linkHint && (
        <div className="mb-2">
          <span className="rounded-md border border-primary/35 bg-primary/15 px-2 py-0.5 font-mono text-[10px] font-semibold text-primary">
            Linked · {linkHint}
          </span>
        </div>
      )}
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

      <div className="rounded-md border border-white/10 bg-black/25 px-2.5 pt-2.5 pb-2">
        <div className="mb-2 flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1 px-0.5">
          <div className="min-w-0">
            <div className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
              Time range · {wallGrainLabel(grain)} buckets
              {ghostById ? ' · faint = full cohort' : ''}
            </div>
            <div className="mt-0.5 truncate font-mono text-[13px] font-bold tracking-tight text-text">
              {rangeLabel ?? '—'}
            </div>
          </div>
          <div className="font-mono text-[10px] text-text-dim">
            peak {maxN} · {cells.filter((c) => c.n > 0).length}/{cells.length} active
          </div>
        </div>

        {selectedCell && (
          <div className="mb-2 rounded border border-primary/30 bg-primary/10 px-2 py-1 font-mono text-[12px] font-semibold text-text">
            Focus · {formatWallTick(selectedCell.start, grain)}
            <span className="ml-2 font-normal text-text-mid">
              n={selectedCell.n} · {pnlText(selectedCell.pnl_sol)}
            </span>
          </div>
        )}

        <div className="overflow-x-auto pb-1">
          <div style={{ minWidth: gridMinWidth }}>
            {/* Bars */}
            <div
              className="grid items-end gap-1"
              style={{
                gridTemplateColumns: `repeat(${cells.length}, minmax(${colMinPx}px, 1fr))`,
                height: barMaxH + 34,
              }}
            >
              {cells.map((c) => {
                const active = selection?.kind === 'wall' && selection.cellId === c.id;
                const ghost = ghostById?.get(c.id);
                const ghostH =
                  ghost && ghost.n > 0
                    ? Math.max(12, Math.round((ghost.n / maxN) * barMaxH))
                    : 0;
                const h =
                  c.n > 0 ? Math.max(12, Math.round((c.n / maxN) * barMaxH)) : ghostH > 0 ? 0 : 3;
                const fill =
                  c.n === 0
                    ? 'transparent'
                    : colorMode === 'pnl'
                      ? pnlHeatBackground(c.pnl_sol, maxAbsPnl, c.n)
                      : countHeatBackground(c.n, maxN);
                const closed = closedCount(c.n, c.exits);
                const winLabel =
                  c.n > 0 && closed > 0 ? `${Math.round(c.win_rate * 100)}%` : '';
                const mark =
                  !ghostById &&
                  (c.id === peakId ? '▲' : c.id === bestId ? '+' : c.id === worstId ? '−' : '');
                const title = [
                  formatWallTick(c.start, grain),
                  `n=${c.n}`,
                  ghost ? `cohort n=${ghost.n}` : null,
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
                      'relative flex h-full flex-col items-center justify-end gap-0.5 rounded-sm px-0.5 transition',
                      active && 'bg-primary/20 ring-1 ring-primary/70',
                      c.n > 0 && onSelect && 'hover:bg-white/5',
                    )}
                  >
                    {mark && c.n > 0 && (
                      <span
                        className={cn(
                          'absolute top-0 font-mono text-[9px] font-bold leading-none',
                          c.id === peakId
                            ? 'text-info'
                            : c.id === bestId
                              ? 'text-green'
                              : 'text-red',
                        )}
                      >
                        {mark}
                      </span>
                    )}
                    <span
                      className={cn(
                        'w-full truncate text-center font-mono text-[9px] font-bold leading-none',
                        c.n === 0 ? 'text-transparent' : signedToneClass(c.pnl_sol),
                      )}
                    >
                      {c.n > 0 ? pnlText(c.pnl_sol) : '0'}
                    </span>
                    <span
                      className={cn(
                        'font-mono text-[10px] font-bold leading-none',
                        c.n > 0 ? 'text-text' : 'text-transparent',
                      )}
                    >
                      {c.n > 0 ? c.n : '0'}
                    </span>
                    {winLabel ? (
                      <span className="font-mono text-[8px] leading-none text-text-dim">
                        {winLabel}
                      </span>
                    ) : (
                      <span className="h-[10px]" />
                    )}
                    <div className="relative w-full" style={{ height: Math.max(ghostH, h, 3) }}>
                      {ghostH > 0 && (
                        <div
                          className="absolute bottom-0 w-full rounded-sm bg-white/15"
                          style={{ height: ghostH }}
                        />
                      )}
                      {h > 0 && (
                        <div
                          className="absolute bottom-0 w-full rounded-sm shadow-[inset_0_0_0_1px_rgba(255,255,255,0.08)]"
                          style={{ height: h, background: fill }}
                        />
                      )}
                    </div>
                  </button>
                );
              })}
            </div>

            {/* Dedicated time axis — clock-first, date on day breaks */}
            <div
              className="mt-1 grid gap-1 border-t border-white/12 pt-1.5"
              style={{
                gridTemplateColumns: `repeat(${cells.length}, minmax(${colMinPx}px, 1fr))`,
              }}
            >
              {cells.map((c, i) => {
                const prev = i > 0 ? cells[i - 1]!.start : null;
                const dayBreak = grain !== 'day' && isWallDayBreak(c.start, prev);
                const show =
                  i === 0 ||
                  i === cells.length - 1 ||
                  dayBreak ||
                  i % tickEvery === 0 ||
                  (selection?.kind === 'wall' && selection.cellId === c.id);
                const active = selection?.kind === 'wall' && selection.cellId === c.id;
                if (!show) {
                  return (
                    <div key={c.id} className="flex flex-col items-center">
                      <span className="h-1 w-px bg-white/15" />
                    </div>
                  );
                }
                return (
                  <div
                    key={c.id}
                    className={cn(
                      'flex min-w-0 flex-col items-center gap-0.5',
                      active && 'rounded-sm bg-primary/15',
                    )}
                    title={formatWallTick(c.start, grain)}
                  >
                    <span className="h-1.5 w-px bg-white/35" />
                    {grain === 'day' ? (
                      <span
                        className={cn(
                          'w-full text-center font-mono text-[11px] font-bold leading-tight',
                          active ? 'text-primary' : 'text-text',
                        )}
                      >
                        {formatWallDate(c.start)}
                      </span>
                    ) : (
                      <>
                        <span
                          className={cn(
                            'w-full text-center font-mono text-[11px] font-bold leading-tight tabular-nums',
                            active ? 'text-primary' : 'text-text',
                          )}
                        >
                          {formatWallClock(c.start, grain)}
                        </span>
                        {(dayBreak || i === 0) && (
                          <span className="w-full text-center font-mono text-[9px] font-semibold leading-tight text-text-mid">
                            {formatWallDate(c.start)}
                          </span>
                        )}
                      </>
                    )}
                  </div>
                );
              })}
            </div>
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
    <div className="mb-2 flex flex-wrap items-center gap-2">
      <span className="text-[11px] font-semibold uppercase tracking-wider text-text">
        By {fieldLabel} time
      </span>
      <ToggleGroup
        value={field}
        onChange={onFieldChange}
        options={[
          { id: 'created_at', label: 'Created at' },
          { id: 'entry_time', label: 'Entry time' },
        ]}
      />
      <ToggleGroup
        value={colorMode}
        onChange={onColorModeChange}
        options={[
          { id: 'volume', label: 'Volume' },
          { id: 'pnl', label: 'PnL color' },
        ]}
      />
      <div className="flex items-center gap-1.5">
        <span className="text-[10px] font-semibold uppercase tracking-wider text-text-dim">
          Grain
        </span>
        <ToggleGroup
          value={grainChoice === 'auto' ? 'auto' : 'manual'}
          onChange={(v) => {
            if (v === 'auto') onGrainChange('auto');
            else if (grainChoice === 'auto') onGrainChange(autoGrain);
          }}
          options={[
            { id: 'auto', label: `Auto (${wallGrainLabel(autoGrain)})` },
            { id: 'manual', label: 'Manual' },
          ]}
        />
        {grainChoice !== 'auto' && (
          <select
            value={grainChoice}
            onChange={(e) => onGrainChange(e.target.value as WallGrain)}
            className="rounded-md border border-white/12 bg-black/40 px-2 py-1 font-mono text-[11px] text-text"
          >
            {WALL_GRAINS.map((g) => (
              <option key={g} value={g}>
                {wallGrainLabel(g)}
              </option>
            ))}
          </select>
        )}
      </div>
    </div>
  );
}

function pnlText(v: number): string {
  return `${v >= 0 ? '+' : ''}${formatDecimalTrim(v, 2)}`;
}
