import { memo, useMemo } from 'react';
import { CHART_COLORS } from 'components/token-price-chart/constants';

interface PnlSparklineProps {
  /** Per-period values in chronological order (e.g. daily realized PnL). */
  values: number[];
  width?: number;
  height?: number;
  /** Render the running cumulative sum instead of the per-period values — the
   *  equity-curve reading of the same series. */
  cumulative?: boolean;
  title?: string;
}

/**
 * A table-cell-sized PnL trend line. Inline SVG (no charting dep, no layout
 * effect) because it renders once per row of a scoreboard — a `lightweight-charts`
 * instance per row would be absurd.
 *
 * Colored by the *final* value on the SSOT candle palette, with a zero baseline
 * so "ends above water" is readable at 14px tall.
 */
export const PnlSparkline = memo(function PnlSparkline({
  values,
  width = 84,
  height = 20,
  cumulative = true,
  title,
}: PnlSparklineProps) {
  const { path, zeroY, last } = useMemo(() => {
    const series: number[] = [];
    let cum = 0;
    for (const v of values) {
      cum += v;
      series.push(cumulative ? cum : v);
    }
    if (series.length === 0) return { path: '', zeroY: height / 2, last: 0 };

    const min = Math.min(0, ...series);
    const max = Math.max(0, ...series);
    const span = max - min || 1;
    const pad = 1.5;
    const usable = height - pad * 2;
    const y = (v: number) => pad + (1 - (v - min) / span) * usable;
    const x = (i: number) => (series.length === 1 ? width / 2 : (i / (series.length - 1)) * width);

    const d = series.map((v, i) => `${i === 0 ? 'M' : 'L'}${x(i).toFixed(2)},${y(v).toFixed(2)}`).join(' ');
    return { path: d, zeroY: y(0), last: series[series.length - 1]! };
  }, [values, width, height, cumulative]);

  if (!path) {
    return <span className="text-text-dim">—</span>;
  }

  const stroke = last >= 0 ? CHART_COLORS.up : CHART_COLORS.down;
  return (
    <svg
      width={width}
      height={height}
      viewBox={`0 0 ${width} ${height}`}
      role="img"
      aria-label={title ?? 'PnL trend'}
      className="overflow-visible"
    >
      {title && <title>{title}</title>}
      <line
        x1={0}
        x2={width}
        y1={zeroY}
        y2={zeroY}
        stroke="currentColor"
        strokeWidth={0.5}
        className="text-white/15"
      />
      <path d={path} fill="none" stroke={stroke} strokeWidth={1.25} strokeLinejoin="round" />
    </svg>
  );
});
