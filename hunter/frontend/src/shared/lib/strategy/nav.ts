/**
 * Cross-page deep links for strategy surfaces (Rules ↔ Fingerprints ↔ Simulate…).
 * Selection lives in the query string so navigation (same-tab or new-tab) keeps
 * the target selected.
 *
 * Params: `?rule=<id>` on Rules/Simulate, `?fp=<id>` on Fingerprints — same shape
 * as Tokens `?mint=` / Sweep `?run=`. Prefer Router `Link` (same-tab); Ctrl/middle-click
 * still opens a new tab with the param intact.
 *
 * Simulate is lab-only (`/strategies/simulate`); don't link to it from the live app.
 */

export const STRATEGY_PATHS = {
  rules: '/strategies/rules',
  fingerprints: '/strategies/fingerprints',
  /** Live: per-rule positions + summary (traded history). */
  ops: '/ops',
  /** Lab app only. */
  simulate: '/strategies/simulate',
} as const;

export const STRATEGY_PARAMS = {
  rule: 'rule',
  fingerprint: 'fp',
} as const;

export function rulesHref(ruleId?: string | null): string {
  if (!ruleId) return STRATEGY_PATHS.rules;
  return `${STRATEGY_PATHS.rules}?${STRATEGY_PARAMS.rule}=${encodeURIComponent(ruleId)}`;
}

/** Live per-rule Analyze page (positions summary + traded history). */
export function ruleAnalyzeHref(ruleId: string): string {
  return `${STRATEGY_PATHS.rules}/${encodeURIComponent(ruleId)}`;
}

export function fingerprintsHref(fpId?: string | null): string {
  if (!fpId) return STRATEGY_PATHS.fingerprints;
  return `${STRATEGY_PATHS.fingerprints}?${STRATEGY_PARAMS.fingerprint}=${encodeURIComponent(fpId)}`;
}

/** Lab-only. */
export function simulateHref(ruleId?: string | null): string {
  if (!ruleId) return STRATEGY_PATHS.simulate;
  return `${STRATEGY_PATHS.simulate}?${STRATEGY_PARAMS.rule}=${encodeURIComponent(ruleId)}`;
}
