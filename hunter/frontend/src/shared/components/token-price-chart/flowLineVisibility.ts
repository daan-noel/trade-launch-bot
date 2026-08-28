import type { IChartApi, ISeriesApi } from 'lightweight-charts';

/**
 * Per-series visibility for the cumulative tagged / non-tagged flow overlay.
 *
 * The two curves share the LEFT price scale, and tagged normally dwarfs non-tagged —
 * so with both drawn the non-tagged curve is pinned to the axis floor and its shape
 * is unreadable. Hiding one lets the left scale autoscale to the other, which is
 * the point of splitting the toolbar toggle in two.
 */
export type FlowLineVisibility = {
  tagged: boolean;
  untagged: boolean;
};

export const DEFAULT_FLOW_LINE_VISIBILITY: FlowLineVisibility = { tagged: true, untagged: true };

/** True when at least one curve is drawn — the left price scale's visibility. */
export function anyFlowLineVisible(v: FlowLineVisibility): boolean {
  return v.tagged || v.untagged;
}

/** Stable key of what an axis MEANS, for the autoscale-reset guard. */
export function flowLineVisibilityKey(v: FlowLineVisibility): string {
  return `${v.tagged}|${v.untagged}`;
}

/**
 * Legacy persisted prefs stored ONE boolean (`showFlowLines`) for both curves.
 * Seed both flags from it when the split keys are absent, so an existing user's
 * saved state carries over instead of silently resetting.
 */
export function flowLineVisibilityFromPrefs(prefs: {
  showFlowTagged?: boolean;
  showFlowUntagged?: boolean;
  showFlowLines?: boolean;
}): FlowLineVisibility {
  const legacy = prefs.showFlowLines ?? DEFAULT_FLOW_LINE_VISIBILITY.tagged;
  return {
    tagged: prefs.showFlowTagged ?? legacy,
    untagged: prefs.showFlowUntagged ?? legacy,
  };
}

/**
 * Show/hide the two overlay series and their shared left price scale. Call from
 * an effect that also depends on the structural series deps (style / grouping /
 * interval), so the toggles survive a series recreation.
 *
 * `available` is the classification gate (creator wallet or `ix_patterns`)
 * — it is per-chart, never per-series, so it forces both curves off together.
 */
export function applyFlowLineVisibility(args: {
  taggedSeries: ISeriesApi<'Line'> | null;
  untaggedSeries: ISeriesApi<'Line'> | null;
  chart: IChartApi | null;
  visibility: FlowLineVisibility;
  available?: boolean;
}): void {
  const { taggedSeries, untaggedSeries, chart, visibility, available = true } = args;
  const tagged = available && visibility.tagged;
  const untagged = available && visibility.untagged;
  taggedSeries?.applyOptions({ visible: tagged });
  untaggedSeries?.applyOptions({ visible: untagged });
  chart?.priceScale('left').applyOptions({ visible: tagged || untagged });
}
