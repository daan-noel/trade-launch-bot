// Trade-mode view filter for the rule boards (Rules live/lab + Simulate).
//
// Purely presentational, exactly like the tag filter next to it: narrowing to
// `paper` never changes what a rule *is*, only which rows a page shows. The
// scope deliberately rides the same shape as `useTagFilter` (URL param wins,
// localStorage restores the habit) so the two controls behave identically.
//
// The row paint that goes with this lives in `types.ts` (`ruleRowClass`) —
// filtering and painting are separate concerns and a page uses either or both.

import type { StrategyRule, TradeMode } from './types';

/** `all` = both modes (the default); otherwise show that one mode only. */
export type ModeFilter = 'all' | TradeMode;

export const DEFAULT_MODE_FILTER: ModeFilter = 'all';

/** `?mode=paper|real`. Deliberately the same key the Console uses for its own
 *  mode deep-link (`OPS_PARAMS.mode`) — different routes, same vocabulary. */
export const MODE_PARAM = 'mode';

export function isModeFilter(raw: string | null | undefined): raw is ModeFilter {
  return raw === 'all' || raw === 'paper' || raw === 'real';
}

/** Unknown/absent values fall back to `all` — a bad param must never blank the
 *  board (the same forgiving posture as `parseTagFilter`). */
export function parseModeFilter(raw: string | null | undefined): ModeFilter {
  return isModeFilter(raw) ? raw : DEFAULT_MODE_FILTER;
}

export function isDefaultModeFilter(f: ModeFilter): boolean {
  return f === DEFAULT_MODE_FILTER;
}

/** Does a rule survive the scope? */
export function matchesModeFilter(mode: TradeMode, f: ModeFilter): boolean {
  return f === 'all' || mode === f;
}

/** Per-mode rule counts for the chip labels. Pass the set filtered by
 *  everything EXCEPT the mode filter, so a chip's count doesn't collapse to the
 *  selection the moment you click it (same rule as `RuleTagFilter`). */
export function modeCounts(
  rules: Pick<StrategyRule, 'trade_mode'>[],
): Record<TradeMode, number> {
  const acc: Record<TradeMode, number> = { paper: 0, real: 0 };
  for (const r of rules) acc[r.trade_mode] += 1;
  return acc;
}
