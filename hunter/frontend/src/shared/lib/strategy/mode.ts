// Trade-mode view filter for the rule boards (Rules live/lab + Simulate).
//
// Purely presentational, exactly like the tag filter next to it: narrowing to
// `paper` never changes what a rule *is*, only which rows a page shows. The
// scope deliberately rides the same shape as `useTagFilter` (URL param wins,
// localStorage restores the habit) so the two controls behave identically.
//
// The row paint that goes with this lives in `types.ts` (`ruleRowClass`) —
// filtering and painting are separate concerns and a page uses either or both.

import type { BadgeVariant } from 'components/ui/Badge';
import type { StrategyRule, TradeMode } from './types';

/** `all` = both modes (the default); otherwise show that one mode only. */
export type ModeFilter = 'all' | TradeMode;

/**
 * Badge hue for a trade mode — SSOT with ModeToggle / ModeBadge / row rails.
 * `paper` = info (blue), `real` = warning (amber). Never `neutral` for paper.
 */
export function modeBadgeVariant(mode: TradeMode): BadgeVariant {
  return mode === 'real' ? 'warning' : 'info';
}

/** Selected-segment washes for `ModeToggle` — literal Tailwind for the scanner. */
export const MODE_TOGGLE_ACTIVE: Record<ModeFilter, string> = {
  all: 'bg-primary/20 text-primary',
  paper: 'bg-info/20 text-info',
  real: 'bg-warning/20 text-warning',
};

/** Left rail swatches on Paper / Real segments (same hues as row rails). */
export const MODE_TOGGLE_SWATCH: Record<TradeMode, string> = {
  paper: 'bg-info/60',
  real: 'bg-warning',
};

/**
 * Segment order for `ModeToggle`:
 * - `filter` — All · Paper · Real (rule boards; scope first)
 * - `ops` — Real · Paper · All (Console / History; money mode first)
 */
export type ModeToggleLayout = 'filter' | 'ops';

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
