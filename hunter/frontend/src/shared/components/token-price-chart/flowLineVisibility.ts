import type { IChartApi, ISeriesApi } from 'lightweight-charts';

/**
 * Per-series visibility for the cumulative vol / non-vol flow overlay.
 *
 * The two curves share the LEFT price scale, and vol normally dwarfs non-vol —
 * so with both drawn the non-vol curve is pinned to the axis floor and its shape
 * is unreadable. Hiding one lets the left scale autoscale to the other, which is
 * the point of splitting the toolbar toggle in two.
 */
export type FlowLineVisibility = {
  vol: boolean;
  nonVol: boolean;
};

export const DEFAULT_FLOW_LINE_VISIBILITY: FlowLineVisibility = { vol: true, nonVol: true };

/** True when at least one curve is drawn — the left price scale's visibility. */
export function anyFlowLineVisible(v: FlowLineVisibility): boolean {
  return v.vol || v.nonVol;
}

/** Stable key of what an axis MEANS, for the autoscale-reset guard. */
export function flowLineVisibilityKey(v: FlowLineVisibility): string {
  return `${v.vol}|${v.nonVol}`;
}

/**
 * Legacy persisted prefs stored ONE boolean (`showFlowLines`) for both curves.
 * Seed both flags from it when the split keys are absent, so an existing user's
 * saved state carries over instead of silently resetting.
 */
export function flowLineVisibilityFromPrefs(prefs: {
  showFlowVol?: boolean;
  showFlowNonVol?: boolean;
  showFlowLines?: boolean;
}): FlowLineVisibility {
  const legacy = prefs.showFlowLines ?? DEFAULT_FLOW_LINE_VISIBILITY.vol;
  return {
    vol: prefs.showFlowVol ?? legacy,
    nonVol: prefs.showFlowNonVol ?? legacy,
  };
}

/**
 * Show/hide the two overlay series and their shared left price scale. Call from
 * an effect that also depends on the structural series deps (style / grouping /
 * interval), so the toggles survive a series recreation.
 *
 * `available` is the classification gate (creator wallet or `volume_ix_patterns`)
 * — it is per-chart, never per-series, so it forces both curves off together.
 */
export function applyFlowLineVisibility(args: {
  volSeries: ISeriesApi<'Line'> | null;
  nonVolSeries: ISeriesApi<'Line'> | null;
  chart: IChartApi | null;
  visibility: FlowLineVisibility;
  available?: boolean;
}): void {
  const { volSeries, nonVolSeries, chart, visibility, available = true } = args;
  const vol = available && visibility.vol;
  const nonVol = available && visibility.nonVol;
  volSeries?.applyOptions({ visible: vol });
  nonVolSeries?.applyOptions({ visible: nonVol });
  chart?.priceScale('left').applyOptions({ visible: vol || nonVol });
}
