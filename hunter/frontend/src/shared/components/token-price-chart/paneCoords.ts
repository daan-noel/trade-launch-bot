import type { Coordinate, IChartApi } from 'lightweight-charts';
import type { OhlcBar } from './types';

/**
 * Pane-relative x (CSS px) for a pointer's `clientX`.
 *
 * lightweight-charts lays the chart out as `[left axis][pane][right axis]`
 * inside the container, and every time-scale coordinate is measured from the
 * PANE's left edge. A container-relative x is therefore off by the left price
 * scale's width whenever that scale is visible (it is, with the flow lines on),
 * which lands a drag-selected range that many pixels right of the pointer.
 * `width()` is 0 for a hidden scale, so this is exact in both states.
 */
export function paneX(chart: IChartApi, el: HTMLElement, clientX: number): Coordinate {
  const rect = el.getBoundingClientRect();
  return (clientX - rect.left - chart.priceScale('left').width()) as Coordinate;
}

/** Time of the bar nearest a pointer's `clientX`; null when there is nothing to hit. */
export function barTimeAtClientX(
  chart: IChartApi,
  el: HTMLElement,
  bars: readonly OhlcBar[],
  clientX: number,
): number | null {
  if (bars.length === 0) return null;
  const logical = chart.timeScale().coordinateToLogical(paneX(chart, el, clientX));
  if (logical == null) return null;
  const idx = Math.max(0, Math.min(bars.length - 1, Math.round(logical)));
  return bars[idx].time as number;
}
