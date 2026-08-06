import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
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
/** Above this count, paint dots on canvas so brush-drag never remounts N circles. */
const CANVAS_DOT_THRESHOLD = 250;

function logHold(seconds: number): number {
  return Math.log10(Math.max(1, seconds));
}

function unlogHold(log: number): number {
  return 10 ** log;
}

interface LaidOutPoint {
  key: string;
  label: string;
  holdSeconds: number;
  pnlPct: number;
  sizeSol: number;
  isWin: boolean;
  cx: number;
  cy: number;
  r: number;
}

/**
 * Hold-time vs realized PnL% scatter. Drag on the plot to zoom into a band
 * (and optionally focus the table via `onDomainChange`); double-click resets.
 *
 * Brush geometry is updated via DOM refs during pointermove so a drag never
 * re-renders every marker. Large cohorts paint on canvas; small ones keep SVG
 * circles (native `<title>` tooltips).
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
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const dragRectRef = useRef<SVGRectElement>(null);
  const dragRef = useRef<{ x0: number; y0: number; x1: number; y1: number } | null>(null);
  const [hover, setHover] = useState<LaidOutPoint | null>(null);

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

  const plotW = width - PAD_L - PAD_R;
  const plotH = height - PAD_T - PAD_B;

  const laidOut = useMemo((): LaidOutPoint[] => {
    const xSpan = xMax - xMin || 1;
    const ySpan = yMax - yMin || 1;
    const out: LaidOutPoint[] = [];
    for (const p of points) {
      const cx = PAD_L + ((logHold(p.holdSeconds) - xMin) / xSpan) * plotW;
      const cy = PAD_T + (1 - (p.pnlPct - yMin) / ySpan) * plotH;
      if (cx < PAD_L || cx > width - PAD_R || cy < PAD_T || cy > height - PAD_B) continue;
      out.push({
        key: p.key,
        label: p.label,
        holdSeconds: p.holdSeconds,
        pnlPct: p.pnlPct,
        sizeSol: p.sizeSol,
        isWin: p.isWin,
        cx,
        cy,
        r: 3 + 7 * Math.sqrt(Math.max(0, p.sizeSol) / maxSize),
      });
    }
    return out;
  }, [points, xMin, xMax, yMin, yMax, maxSize, plotW, plotH, width, height]);

  const useCanvas = laidOut.length >= CANVAS_DOT_THRESHOLD;

  const xTicks = useMemo(() => {
    const ticks: number[] = [];
    const startDecade = Math.floor(xMin);
    const endDecade = Math.ceil(xMax);
    for (let d = startDecade; d <= endDecade; d++) {
      const sec = 10 ** d;
      const lx = logHold(sec);
      if (lx >= xMin - 0.01 && lx <= xMax + 0.01) ticks.push(sec);
    }
    return ticks.length > 0 ? ticks : [10 ** Math.round((xMin + xMax) / 2)];
  }, [xMin, xMax]);

  const zeroY = yMin <= 0 && yMax >= 0
    ? PAD_T + (1 - (0 - yMin) / (yMax - yMin || 1)) * plotH
    : null;

  const xOf = useCallback(
    (holdSeconds: number) => PAD_L + ((logHold(holdSeconds) - xMin) / (xMax - xMin || 1)) * plotW,
    [xMin, xMax, plotW],
  );

  // Paint canvas dots whenever layout / selection changes — never on brush drag.
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !useCanvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const dpr = typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1;
    canvas.width = Math.max(1, Math.floor(width * dpr));
    canvas.height = Math.max(1, Math.floor(height * dpr));
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);
    for (const p of laidOut) {
      const selected = selectedKey === p.key;
      const r = p.r + (selected ? 1.5 : 0);
      ctx.beginPath();
      ctx.arc(p.cx, p.cy, r, 0, Math.PI * 2);
      ctx.fillStyle = p.isWin ? CHART_COLORS.up : CHART_COLORS.down;
      ctx.globalAlpha = selected ? 0.9 : 0.55;
      ctx.fill();
      ctx.globalAlpha = 1;
      ctx.lineWidth = selected ? 2 : 1;
      ctx.strokeStyle = selected ? CHART_COLORS.line : p.isWin ? CHART_COLORS.up : CHART_COLORS.down;
      ctx.stroke();
    }
  }, [laidOut, selectedKey, useCanvas, width, height]);

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

  const setDragRectVisible = (box: { x0: number; y0: number; x1: number; y1: number } | null) => {
    const el = dragRectRef.current;
    if (!el) return;
    if (!box) {
      el.setAttribute('visibility', 'hidden');
      return;
    }
    const x = Math.min(box.x0, box.x1);
    const y = Math.min(box.y0, box.y1);
    el.setAttribute('visibility', 'visible');
    el.setAttribute('x', String(x));
    el.setAttribute('y', String(y));
    el.setAttribute('width', String(Math.abs(box.x1 - box.x0)));
    el.setAttribute('height', String(Math.abs(box.y1 - box.y0)));
  };

  const hitTest = useCallback(
    (x: number, y: number): LaidOutPoint | null => {
      let best: LaidOutPoint | null = null;
      let bestDist = 14;
      for (const pt of laidOut) {
        const d = Math.hypot(pt.cx - x, pt.cy - y);
        if (d < bestDist) {
          bestDist = d;
          best = pt;
        }
      }
      return best;
    },
    [laidOut],
  );

  if (points.length === 0) {
    return <p className="text-xs text-text-dim">{emptyMessage}</p>;
  }

  const handlePointerDown = (e: React.PointerEvent<SVGSVGElement>) => {
    if (!onDomainChange && !onSelectPoint) return;
    const raw = svgPoint(e.clientX, e.clientY);
    const p = clampPlot(raw.x, raw.y);
    dragRef.current = { x0: p.x, y0: p.y, x1: p.x, y1: p.y };
    setDragRectVisible(dragRef.current);
    e.currentTarget.setPointerCapture(e.pointerId);
  };

  const handlePointerMove = (e: React.PointerEvent<SVGSVGElement>) => {
    if (dragRef.current) {
      const raw = svgPoint(e.clientX, e.clientY);
      const p = clampPlot(raw.x, raw.y);
      dragRef.current = { ...dragRef.current, x1: p.x, y1: p.y };
      setDragRectVisible(dragRef.current);
      return;
    }
    if (!useCanvas) return;
    const raw = svgPoint(e.clientX, e.clientY);
    const next = hitTest(raw.x, raw.y);
    setHover((prev) => (prev?.key === next?.key ? prev : next));
  };

  const handlePointerUp = (e: React.PointerEvent<SVGSVGElement>) => {
    const drag = dragRef.current;
    dragRef.current = null;
    setDragRectVisible(null);
    if (!drag) return;
    const dx = Math.abs(drag.x1 - drag.x0);
    const dy = Math.abs(drag.y1 - drag.y0);

    if (dx < CLICK_SLOP && dy < CLICK_SLOP) {
      if (!onSelectPoint) return;
      const raw = svgPoint(e.clientX, e.clientY);
      const best = hitTest(raw.x, raw.y);
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

  const tipPoint = useCanvas ? hover : null;

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
      <div className="relative w-full" style={{ height }}>
        {tipPoint && (
          <div
            className="pointer-events-none absolute z-10 max-w-[14rem] rounded border border-white/15 bg-bg-panel/95 px-1.5 py-1 text-[10px] text-text shadow-md"
            style={{
              left: `${(tipPoint.cx / width) * 100}%`,
              top: `${(tipPoint.cy / height) * 100}%`,
              transform: 'translate(-50%, calc(-100% - 6px))',
            }}
          >
            {tipPoint.label}
            <br />
            Hold: {formatDurationShort(tipPoint.holdSeconds)}
            <br />
            PnL: {tipPoint.pnlPct >= 0 ? '+' : ''}
            {tipPoint.pnlPct.toFixed(1)}%
            <br />
            Size: {tipPoint.sizeSol.toFixed(2)} SOL
          </div>
        )}
        {useCanvas && (
          <canvas
            ref={canvasRef}
            className="pointer-events-none absolute inset-0 h-full w-full"
            aria-hidden
          />
        )}
        <svg
          ref={svgRef}
          viewBox={`0 0 ${width} ${height}`}
          className="absolute inset-0 h-full w-full touch-none"
          style={{ cursor: onDomainChange ? 'crosshair' : 'default' }}
          onPointerDown={handlePointerDown}
          onPointerMove={handlePointerMove}
          onPointerUp={handlePointerUp}
          onPointerLeave={() => {
            if (!dragRef.current) setHover(null);
          }}
          onPointerCancel={() => {
            dragRef.current = null;
            setDragRectVisible(null);
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
          {!useCanvas &&
            laidOut.map((p) => {
              const selected = selectedKey === p.key;
              return (
                <circle
                  key={p.key}
                  cx={p.cx}
                  cy={p.cy}
                  r={p.r + (selected ? 1.5 : 0)}
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
          <rect
            ref={dragRectRef}
            visibility="hidden"
            fill={CHART_COLORS.line}
            fillOpacity={0.12}
            stroke={CHART_COLORS.line}
            strokeWidth={1}
            strokeDasharray="4 2"
            style={{ pointerEvents: 'none' }}
          />
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
    </div>
  );
});
