import type { ISeriesApi } from 'lightweight-charts';

/**
 * Per-series visibility for the cumulative `@tag` / `@!tag` flow overlay.
 *
 * Both curves sit on the candles' price axis at their cohort curve price, so a
 * toggle only shows or hides a line - it never rescales the other one relative to
 * the candles.
 */
export type FlowLineVisibility = {
  tagged: boolean;
  untagged: boolean;
};

export const DEFAULT_FLOW_LINE_VISIBILITY: FlowLineVisibility = { tagged: true, untagged: true };

/** True when at least one curve is drawn. */
export function anyFlowLineVisible(v: FlowLineVisibility): boolean {
  return v.tagged || v.untagged;
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
 * Show/hide the two overlay series. Call from an effect that also depends on the
 * structural series deps (style / grouping / interval), so the toggles survive a
 * series recreation.
 *
 * `available` is the classification gate (a tag with at least one matcher)
 * — it is per-chart, never per-series, so it forces both curves off together.
 */
export function applyFlowLineVisibility(args: {
  taggedSeries: ISeriesApi<'Line'> | null;
  untaggedSeries: ISeriesApi<'Line'> | null;
  visibility: FlowLineVisibility;
  available?: boolean;
}): void {
  const { taggedSeries, untaggedSeries, visibility, available = true } = args;
  taggedSeries?.applyOptions({ visible: available && visibility.tagged });
  untaggedSeries?.applyOptions({ visible: available && visibility.untagged });
}
