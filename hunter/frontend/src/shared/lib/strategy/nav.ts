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
  /** Live Floor — waiting / open / attention / recent. */
  floor: '/floor',
  /** @deprecated Prefer `floor`; kept for any lingering `/ops` string refs. */
  ops: '/floor',
  /** Live Portfolio — cross-rule money. */
  portfolio: '/portfolio',
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

/** Live per-rule Evidence page (positions summary + traded history). */
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

export function portfolioHref(range?: 'today' | '7d' | 'all'): string {
  if (!range) return STRATEGY_PATHS.portfolio;
  return `${STRATEGY_PATHS.portfolio}?range=${range}`;
}

/** Floor deep-link query keys (notification click-through + Home). */
export const OPS_PARAMS = {
  tab: 'tab',
  mode: 'mode',
  status: 'status',
  mint: 'mint',
  rule: 'rule',
  position: 'position',
} as const;

export type OpsTab = 'waiting' | 'open' | 'attention' | 'recent';

/** Map a notification status pill → Floor tab that holds that row. */
export function opsTabForNotifyStatus(status: string): OpsTab {
  switch (status) {
    case 'Armed':
    case 'Disarmed':
      return 'waiting';
    case 'ExitFailed':
    case 'ExitUnconfirmed':
    case 'ExitPending':
      return 'attention';
    case 'End':
      return 'recent';
    default:
      return 'open';
  }
}

export function floorHref(opts?: {
  tab?: OpsTab;
  mode?: string;
  status?: string;
  mint?: string;
  ruleId?: string;
  positionId?: string | null;
}): string {
  if (!opts) return STRATEGY_PATHS.floor;
  const q = new URLSearchParams();
  if (opts.tab) q.set(OPS_PARAMS.tab, opts.tab);
  if (opts.mode) q.set(OPS_PARAMS.mode, opts.mode === 'paper' ? 'paper' : 'real');
  if (opts.status) q.set(OPS_PARAMS.status, opts.status);
  if (opts.mint) q.set(OPS_PARAMS.mint, opts.mint);
  if (opts.ruleId) q.set(OPS_PARAMS.rule, opts.ruleId);
  if (opts.positionId) q.set(OPS_PARAMS.position, opts.positionId);
  const s = q.toString();
  return s ? `${STRATEGY_PATHS.floor}?${s}` : STRATEGY_PATHS.floor;
}

/**
 * Deep-link for a position/arm notification.
 * Clean `End` → Rules Evidence (history). Stuck exits → Floor attention.
 * Open/waiting → Floor inventory.
 */
export function opsNotifyHref(opts: {
  status: string;
  mode: string;
  mint: string;
  ruleId: string;
  positionId?: string | null;
}): string {
  if (opts.status === 'End' && opts.ruleId) {
    return rulesHref(opts.ruleId);
  }
  return floorHref({
    tab: opsTabForNotifyStatus(opts.status),
    mode: opts.mode,
    status:
      opts.status !== 'Armed' && opts.status !== 'Disarmed' ? opts.status : undefined,
    mint: opts.mint,
    ruleId: opts.ruleId || undefined,
    positionId: opts.positionId,
  });
}
