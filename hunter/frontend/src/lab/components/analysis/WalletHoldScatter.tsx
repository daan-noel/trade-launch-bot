import { memo, useMemo } from 'react';
import { formatDurationShort } from 'utils/format';
import { CHART_COLORS } from 'components/token-price-chart/constants';
import { buildHoldScatter } from './walletPnlStats';
import type { TraderTokenRow } from 'types';

interface WalletHoldScatterProps {
  rows: readonly TraderTokenRow[];
  width?: number;
  height?: number;
}

const PAD_L = 44;
const PAD_R = 12;
const PAD_T = 12;
const PAD_B = 28;

/** log10(seconds), floored at 1s so a sub-second hold doesn't go negative and
 *  blow up the x-scale. */
function logHold(seconds: number): number {
  return Math.log10(Math.max(1, seconds));
}

/**
 * Hold-time vs realized PnL% scatter — does this wallet make its money on fast
 * scalps or on rides? Hand-rolled SVG (no charting dep: lightweight-charts is
 * time-indexed only and can't do an x/y scatter; this mirrors the app's
 * existing "hand-roll the non-time-series views" convention). X is log-scaled
 * (hold spans 3+ orders of magnitude — seconds to hours); marker radius scales
 * with the position's total volume (sqrt scale, so a 4x bigger trade reads as
 * ~2x the radius, not 4x the area-perceived size).
 */
export const WalletHoldScatter = memo(function WalletHoldScatter({
  rows,
  width = 640,
  height = 280,
}: WalletHoldScatterProps) {
  const points = useMemo(() => buildHoldScatter(rows), [rows]);

  const { xMin, xMax, yMin, yMax, maxSize } = useMemo(() => {
    if (points.length === 0) return { xMin: 0, xMax: 1, yMin: -10, yMax: 10, maxSize: 1 };
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
    // Always include the zero line so a win/loss split is visible even when
    // every point is on one side.
    if (yMinV > 0) yMinV = 0;
    if (yMaxV < 0) yMaxV = 0;
    if (xMinV === xMaxV) {
      xMinV -= 0.5;
      xMaxV += 0.5;
    }
    return { xMin: xMinV, xMax: xMaxV, yMin: yMinV, yMax: yMaxV, maxSize: sizeMax || 1 };
  }, [points]);

  if (points.length === 0) {
    return (
      <p className="text-xs text-text-dim">
        No closed round trips with a positive hold span in this window to plot.
      </p>
    );
  }

  const plotW = width - PAD_L - PAD_R;
  const plotH = height - PAD_T - PAD_B;
  const xOf = (holdSeconds: number) => PAD_L + ((logHold(holdSeconds) - xMin) / (xMax - xMin)) * plotW;
  const yOf = (pnlPct: number) => PAD_T + (1 - (pnlPct - yMin) / (yMax - yMin)) * plotH;
  const rOf = (sizeSol: number) => 3 + 7 * Math.sqrt(Math.max(0, sizeSol) / maxSize);
  const zeroY = yOf(0);

  // A handful of round-ish tick values across the log-x range, in real seconds.
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

  return (
    <svg viewBox={`0 0 ${width} ${height}`} className="w-full" style={{ height }}>
      {/* Zero line */}
      <line x1={PAD_L} y1={zeroY} x2={width - PAD_R} y2={zeroY} stroke={CHART_COLORS.border} strokeWidth={1} />
      {/* Y axis */}
      <line x1={PAD_L} y1={PAD_T} x2={PAD_L} y2={height - PAD_B} stroke={CHART_COLORS.border} strokeWidth={1} />
      <text x={PAD_L - 6} y={PAD_T + 4} textAnchor="end" fontSize={9} fill={CHART_COLORS.text}>
        {yMax.toFixed(0)}%
      </text>
      <text x={PAD_L - 6} y={height - PAD_B} textAnchor="end" fontSize={9} fill={CHART_COLORS.text}>
        {yMin.toFixed(0)}%
      </text>
      <text x={PAD_L - 6} y={zeroY + 3} textAnchor="end" fontSize={9} fill={CHART_COLORS.text}>
        0%
      </text>
      {/* X ticks */}
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
          <text x={xOf(sec)} y={height - PAD_B + 15} textAnchor="middle" fontSize={9} fill={CHART_COLORS.text}>
            {formatDurationShort(sec)}
          </text>
        </g>
      ))}
      {/* Points */}
      {points.map((p) => (
        <circle
          key={p.mint_address}
          cx={xOf(p.holdSeconds)}
          cy={yOf(p.pnlPct)}
          r={rOf(p.sizeSol)}
          fill={p.isWin ? CHART_COLORS.up : CHART_COLORS.down}
          fillOpacity={0.55}
          stroke={p.isWin ? CHART_COLORS.up : CHART_COLORS.down}
          strokeWidth={1}
        >
          <title>
            {`${p.label}\nHold: ${formatDurationShort(p.holdSeconds)}\nPnL: ${p.pnlPct >= 0 ? '+' : ''}${p.pnlPct.toFixed(1)}%\nVolume: ${p.sizeSol.toFixed(2)} SOL`}
          </title>
        </circle>
      ))}
    </svg>
  );
});
