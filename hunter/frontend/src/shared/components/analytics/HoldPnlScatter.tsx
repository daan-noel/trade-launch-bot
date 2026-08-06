import { memo, useCallback, useMemo, useRef, useState } from 'react';
import { formatDurationShort } from 'utils/format';
import { CHART_COLORS } from 'components/token-price-chart/constants';
import type { HoldScatterPoint } from './pnlSeries';

export interface HoldPnlDomain {
  /** Hold seconds (linear; chart x is log10 of these). */
  holdLo: number;
  holdHi: number;
  pctLo: number;
  pctHi: number;
}

interface HoldPnlScatterProps {
  points: readonly HoldScatterPoint[];
  width?: number;
  height?: number;
  emptyMessage?: string;
  /** Selected point key (position id on History), if any. */
  selectedKey?: string | null;
  onSelectPoint?: (key: string) => void;
  /** Controlled axis window — when set, the plot zooms to this band. */
  domain?: HoldPnlDomain | null;
  /** Drag-brush finished → new domain (or null to reset). */
  onDomainChange?: (domain: HoldPnlDomain | null) => void;
}

const PAD_L = 44;
const PAD_R = 12;
const PAD_T = 12;
const PAD_B = 28;
/** Pointer travel below this (svg units) counts as a click, not a brush. */
const CLICK_SLOP = 6;

function logHold(seconds: number): number {
  return Math.log10(Math.max(1, seconds));
}

function unlogHold(log: number): number {
  return 10 ** log;
}

/**
 * Hold-time vs realized PnL% scatter. Drag on the plot to zoom into a band
 * (and optionally focus the table via `onDomainChange`); double-click resets.
 */
export const HoldPnlScatter = memo(function HoldPnlScatter({
  points,
  width = 640,
  height = 280,
  emptyMessage = 'No closed round trips with a positive hold span in this window to plot.',
  selectedKey = null,
  onSelectPoint,
  domain = null,
  onDomainChange,
}: HoldPnlScatterProps) {
  const svgRef = useRef<SVGSVGElement>(null);
  const dragRef = useRef<{ x0: number; y0: number; x1: number; y1: number } | null>(null);
  const [dragBox, setDragBox] = useState<{
    x0: number;
    y0: number;
    x1: number;
    y1: number;
  } | null>(null);

  const auto = useMemo(() => {
    if (points.length === 0) {
      return { xMin: 0, xMax: 1, yMin: -10, yMax: 10, maxSize: 1 };
    }
    let xMinV = Infinity;
    let xMaxV = -Infinity;
    let yMinV = 0;
    let yMaxV = 0;
    let sizeMax = 0;
    for (const p of points) {
      const lx = logHold(p.holdSeconds);
      if (lx < xMinV) xMinV = lx;
      if (lx > xMaxV) xMaxV = lx;
      if (p.pnlPct < yMinV) yMinV = p.pnlPct;
      if (p.pnlPct > yMaxV) yMaxV = p.pnlPct;
      if (p.sizeSol > sizeMax) sizeMax = p.sizeSol;
    }
    if (yMinV > 0) yMinV = 0;
    if (yMaxV < 0) yMaxV = 0;
    if (xMinV === xMaxV) {
      xMinV -= 0.5;
      xMaxV += 0.5;
    }
    if (yMinV === yMaxV) {
      yMinV -= 1;
      yMaxV += 1;
    }
    return { xMin: xMinV, xMax: xMaxV, yMin: yMinV, yMax: yMaxV, maxSize: sizeMax || 1 };
  }, [points]);

  const xMin = domain ? logHold(domain.holdLo) : auto.xMin;
  const xMax = domain ? logHold(domain.holdHi) : auto.xMax;
  const yMin = domain ? domain.pctLo : auto.yMin;
  const yMax = domain ? domain.pctHi : auto.yMax;
  const maxSize = auto.maxSize;

  const xTicks = useMemo(() => {
    const ticks: number[] = [];
    const startDecade = Math.floor(xMin);
    const endDecade = Math.ceil(xMax);
    for (let d = startDecade; d <= endDecade; d++) {
      const sec = 10 ** d;
      if (logHold(sec) >= xMin - 0.01 && logHold(sec) <= xMax + 0.01) ticks.push(sec);
    }
    return ticks.length > 0 ? ticks : [10 ** Math.round((xMin + xMax) / 2)];
  }, [xMin, xMax]);

  const plotW = width - PAD_L - PAD_R;
  const plotH = height - PAD_T - PAD_B;

  const xOf = useCallback(
    (holdSeconds: number) => PAD_L + ((logHold(holdSeconds) - xMin) / (xMax - xMin || 1)) * plotW,
    [xMin, xMax, plotW],
  );
  const yOf = useCallback(
    (pnlPct: number) => PAD_T + (1 - (pnlPct - yMin) / (yMax - yMin || 1)) * plotH,
    [yMin, yMax, plotH],
  );
  const rOf = (sizeSol: number) => 3 + 7 * Math.sqrt(Math.max(0, sizeSol) / maxSize);
  const zeroY = yMin <= 0 && yMax >= 0 ? yOf(0) : null;

  const svgPoint = useCallback((clientX: number, clientY: number) => {
    const svg = svgRef.current;
    if (!svg) return { x: 0, y: 0 };
    const pt = svg.createSVGPoint();
    pt.x = clientX;
    pt.y = clientY;
    const ctm = svg.getScreenCTM();
    if (!ctm) return { x: 0, y: 0 };
    const local = pt.matrixTransform(ctm.inverse());
    return { x: local.x, y: local.y };
  }, []);

  const clampPlot = useCallback(
    (x: number, y: number) => ({
      x: Math.min(width - PAD_R, Math.max(PAD_L, x)),
      y: Math.min(height - PAD_B, Math.max(PAD_T, y)),
    }),
    [width, height],
  );

  if (points.length === 0) {
    return <p className="text-xs text-text-dim">{emptyMessage}</p>;
  }

  const handlePointerDown = (e: React.PointerEvent<SVGSVGElement>) => {
    if (!onDomainChange && !onSelectPoint) return;
    const raw = svgPoint(e.clientX, e.clientY);
    const p = clampPlot(raw.x, raw.y);
    dragRef.current = { x0: p.x, y0: p.y, x1: p.x, y1: p.y };
    setDragBox({ ...dragRef.current });
    e.currentTarget.setPointerCapture(e.pointerId);
  };

  const handlePointerMove = (e: React.PointerEvent<SVGSVGElement>) => {
    if (!dragRef.current) return;
    const raw = svgPoint(e.clientX, e.clientY);
    const p = clampPlot(raw.x, raw.y);
    dragRef.current = { ...dragRef.current, x1: p.x, y1: p.y };
    setDragBox({ ...dragRef.current });
  };

  const handlePointerUp = (e: React.PointerEvent<SVGSVGElement>) => {
    const drag = dragRef.current;
    dragRef.current = null;
    setDragBox(null);
    if (!drag) return;
    const dx = Math.abs(drag.x1 - drag.x0);
    const dy = Math.abs(drag.y1 - drag.y0);

    if (dx < CLICK_SLOP && dy < CLICK_SLOP) {
      if (!onSelectPoint) return;
      const raw = svgPoint(e.clientX, e.clientY);
      let best: HoldScatterPoint | null = null;
      let bestDist = 14;
      for (const pt of points) {
        const cx = xOf(pt.holdSeconds);
        const cy = yOf(pt.pnlPct);
        if (cx < PAD_L - 1 || cx > width - PAD_R + 1 || cy < PAD_T - 1 || cy > height - PAD_B + 1) {
          continue;
        }
        const d = Math.hypot(cx - raw.x, cy - raw.y);
        if (d < bestDist) {
          bestDist = d;
          best = pt;
        }
      }
      if (best) onSelectPoint(best.key);
      return;
    }

    if (!onDomainChange) return;
    const xA = Math.min(drag.x0, drag.x1);
    const xB = Math.max(drag.x0, drag.x1);
    const yA = Math.min(drag.y0, drag.y1);
    const yB = Math.max(drag.y0, drag.y1);
    const holdLo = unlogHold(xMin + ((xA - PAD_L) / plotW) * (xMax - xMin));
    const holdHi = unlogHold(xMin + ((xB - PAD_L) / plotW) * (xMax - xMin));
    const pctHi = yMax - ((yA - PAD_T) / plotH) * (yMax - yMin);
    const pctLo = yMax - ((yB - PAD_T) / plotH) * (yMax - yMin);
    if (!(holdLo < holdHi) || !(pctLo < pctHi)) return;
    onDomainChange({ holdLo, holdHi, pctLo, pctHi });
  };

  return (
    <div className="relative w-full">
      {domain && onDomainChange && (
        <button
          type="button"
          className="absolute right-0 top-0 z-10 rounded border border-white/15 bg-bg-panel/90 px-1.5 py-0.5 text-[10px] font-semibold text-text-dim hover:border-white/30 hover:text-text"
          onClick={() => onDomainChange(null)}
          title="Reset scale to fit all closes"
        >
          Reset scale
        </button>
      )}
      <svg
        ref={svgRef}
        viewBox={`0 0 ${width} ${height}`}
        className="w-full touch-none"
        style={{ height, cursor: onDomainChange ? 'crosshair' : 'default' }}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onPointerCancel={() => {
          dragRef.current = null;
          setDragBox(null);
        }}
        onDoubleClick={() => onDomainChange?.(null)}
      >
        {zeroY != null && (
          <line
            x1={PAD_L}
            y1={zeroY}
            x2={width - PAD_R}
            y2={zeroY}
            stroke={CHART_COLORS.border}
            strokeWidth={1}
          />
        )}
        <line
          x1={PAD_L}
          y1={PAD_T}
          x2={PAD_L}
          y2={height - PAD_B}
          stroke={CHART_COLORS.border}
          strokeWidth={1}
        />
        <text x={PAD_L - 6} y={PAD_T + 4} textAnchor="end" fontSize={9} fill={CHART_COLORS.text}>
          {yMax.toFixed(0)}%
        </text>
        <text x={PAD_L - 6} y={height - PAD_B} textAnchor="end" fontSize={9} fill={CHART_COLORS.text}>
          {yMin.toFixed(0)}%
        </text>
        {zeroY != null && (
          <text x={PAD_L - 6} y={zeroY + 3} textAnchor="end" fontSize={9} fill={CHART_COLORS.text}>
            0%
          </text>
        )}
        {xTicks.map((sec) => (
          <g key={sec}>
            <line
              x1={xOf(sec)}
              y1={height - PAD_B}
              x2={xOf(sec)}
              y2={height - PAD_B + 4}
              stroke={CHART_COLORS.border}
              strokeWidth={1}
            />
            <text
              x={xOf(sec)}
              y={height - PAD_B + 15}
              textAnchor="middle"
              fontSize={9}
              fill={CHART_COLORS.text}
            >
              {formatDurationShort(sec)}
            </text>
          </g>
        ))}
        {points.map((p) => {
          const cx = xOf(p.holdSeconds);
          const cy = yOf(p.pnlPct);
          if (cx < PAD_L || cx > width - PAD_R || cy < PAD_T || cy > height - PAD_B) return null;
          const selected = selectedKey === p.key;
          return (
            <circle
              key={p.key}
              cx={cx}
              cy={cy}
              r={rOf(p.sizeSol) + (selected ? 1.5 : 0)}
              fill={p.isWin ? CHART_COLORS.up : CHART_COLORS.down}
              fillOpacity={selected ? 0.9 : 0.55}
              stroke={selected ? CHART_COLORS.line : p.isWin ? CHART_COLORS.up : CHART_COLORS.down}
              strokeWidth={selected ? 2 : 1}
              style={{ pointerEvents: 'none' }}
            >
              <title>
                {`${p.label}\nHold: ${formatDurationShort(p.holdSeconds)}\nPnL: ${
                  p.pnlPct >= 0 ? '+' : ''
                }${p.pnlPct.toFixed(1)}%\nSize: ${p.sizeSol.toFixed(2)} SOL`}
              </title>
            </circle>
          );
        })}
        {dragBox && (
          <rect
            x={Math.min(dragBox.x0, dragBox.x1)}
            y={Math.min(dragBox.y0, dragBox.y1)}
            width={Math.abs(dragBox.x1 - dragBox.x0)}
            height={Math.abs(dragBox.y1 - dragBox.y0)}
            fill={CHART_COLORS.line}
            fillOpacity={0.12}
            stroke={CHART_COLORS.line}
            strokeWidth={1}
            strokeDasharray="4 2"
            style={{ pointerEvents: 'none' }}
          />
        )}
        {onDomainChange && (
          <text
            x={width - PAD_R}
            y={PAD_T + 10}
            textAnchor="end"
            fontSize={9}
            fill={CHART_COLORS.text}
            style={{ pointerEvents: 'none' }}
          >
            drag to zoom · double-click reset
          </text>
        )}
      </svg>
    </div>
  );
});
