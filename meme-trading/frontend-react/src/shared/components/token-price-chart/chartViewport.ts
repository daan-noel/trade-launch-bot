import type {
  ISeriesApi,
  ITimeScaleApi,
  LogicalRange,
  Time,
  UTCTimestamp,
} from 'lightweight-charts';

/** Saved zoom/pan — logical indices plus extrapolated horizontal span (see LWC time-scale docs). */
export type ChartViewport = {
  logical: LogicalRange;
  from: UTCTimestamp;
  to: UTCTimestamp;
};

export function barsSignature(bars: { time: UTCTimestamp }[]): string {
  if (bars.length === 0) return '0';
  return `${bars.length}:${bars[0].time}:${bars[bars.length - 1].time}`;
}

function horzToTimestamp(value: number): UTCTimestamp {
  return value as UTCTimestamp;
}

/**
 * Captures the full visible viewport. `getVisibleRange()` only returns bar timestamps
 * and drops partial bars at the edges (~2s drift per restore on 1s candles).
 */
export function captureChartViewport(
  series: ISeriesApi<'Line' | 'Candlestick'>,
  logical: LogicalRange,
  horzStep: number,
): ChartViewport {
  const info = series.barsInLogicalRange(logical);
  if (info?.from == null || info.to == null) {
    const from = Number(logical.from);
    const to = Number(logical.to);
    return { logical, from: horzToTimestamp(from), to: horzToTimestamp(to) };
  }
  const step = horzStep > 0 ? horzStep : 1;
  const from = Number(info.from) - step * Math.abs(info.barsBefore);
  const to = Number(info.to) + step * Math.abs(info.barsAfter);
  return {
    logical,
    from: horzToTimestamp(from),
    to: horzToTimestamp(to),
  };
}

export function restoreChartViewport(
  ts: ITimeScaleApi<Time>,
  viewport: ChartViewport | null,
  mode: 'logical' | 'time',
): void {
  if (!viewport) return;
  if (mode === 'time') {
    ts.setVisibleRange({ from: viewport.from, to: viewport.to });
  } else {
    ts.setVisibleLogicalRange(viewport.logical);
  }
}

/** Re-apply after setData / overlay updates — LWC may adjust the range on the next frame. */
export function reapplyChartViewport(
  ts: ITimeScaleApi<Time>,
  viewport: ChartViewport | null,
  mode: 'logical' | 'time',
): void {
  if (!viewport) return;
  restoreChartViewport(ts, viewport, mode);
  requestAnimationFrame(() => {
    restoreChartViewport(ts, viewport, mode);
  });
}
