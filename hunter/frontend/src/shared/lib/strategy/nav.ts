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

/** Ops deep-link query keys used by notification click-through. */
export const OPS_PARAMS = {
  tab: 'tab',
  mode: 'mode',
  status: 'status',
  mint: 'mint',
  rule: 'rule',
  position: 'position',
} as const;

export type OpsTab = 'waiting' | 'open' | 'recent';

/** Map a notification status pill → Ops tab that holds that row. */
export function opsTabForNotifyStatus(status: string): OpsTab {
  switch (status) {
    case 'Armed':
    case 'Disarmed':
      return 'waiting';
    case 'End':
    case 'ExitFailed':
    case 'ExitUnconfirmed':
      return 'recent';
    default:
      return 'open';
  }
}

/**
 * Deep-link for a position/arm notification → Ops with the right tab, mode,
 * status filter, and row selection (`mint`+`rule` for waiting; `position` for
 * open/recent).
 */
export function opsNotifyHref(opts: {
  status: string;
  mode: string;
  mint: string;
  ruleId: string;
  positionId?: string | null;
}): string {
  const q = new URLSearchParams();
  q.set(OPS_PARAMS.tab, opsTabForNotifyStatus(opts.status));
  q.set(OPS_PARAMS.mode, opts.mode === 'paper' ? 'paper' : 'real');
  if (opts.status !== 'Armed' && opts.status !== 'Disarmed') {
    q.set(OPS_PARAMS.status, opts.status);
  }
  q.set(OPS_PARAMS.mint, opts.mint);
  if (opts.ruleId) q.set(OPS_PARAMS.rule, opts.ruleId);
  if (opts.positionId) q.set(OPS_PARAMS.position, opts.positionId);
  return `${STRATEGY_PATHS.ops}?${q.toString()}`;
}
