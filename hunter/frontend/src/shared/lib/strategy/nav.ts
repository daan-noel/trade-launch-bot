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
  /** The unified real-trade Console — attention / open / manual trade / waiting /
   *  recent. Replaces the old Floor + Trade pages (both redirect here). */
  console: '/console',
  /** @deprecated Prefer `console`; kept for lingering `/floor` string refs. */
  floor: '/console',
  /** @deprecated Prefer `console`; kept for any lingering `/ops` string refs. */
  ops: '/console',
  /** Live Portfolio — cross-rule money. */
  portfolio: '/portfolio',
  /** Lab app only. */
  simulate: '/strategies/simulate',
  /** Lab app only. */
  flowDiscovery: '/strategies/flow-discovery',
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

/** Lab-only. Deep-link to Flow discovery scoped to a saved fingerprint —
 *  `?fp=<id>` seeds it as the discovery seed fingerprint on arrival. */
export function flowDiscoveryHref(fpId?: string | null): string {
  if (!fpId) return STRATEGY_PATHS.flowDiscovery;
  return `${STRATEGY_PATHS.flowDiscovery}?${STRATEGY_PARAMS.fingerprint}=${encodeURIComponent(fpId)}`;
}

/** The calendar windows the portfolio/history surfaces share. `custom` means the
 *  explicit `from`/`to` params carry the window instead. */
export type HistoryRange = 'today' | '7d' | '30d' | 'all' | 'custom';

export function portfolioHref(range?: HistoryRange): string {
  if (!range) return STRATEGY_PATHS.portfolio;
  return `${STRATEGY_PATHS.portfolio}?range=${range}`;
}

/** Console deep-link query keys (notification click-through + Home + the History
 *  cohort filter bar). `tab` is legacy (the Console is one page of lanes) —
 *  accepted but ignored.
 *
 *  The `h*` keys drive the History section's single cohort (charts deck **and**
 *  table read the same ones), so a Portfolio "History" link lands on exactly the
 *  cohort it promised. */
export const OPS_PARAMS = {
  tab: 'tab',
  mode: 'mode',
  status: 'status',
  mint: 'mint',
  rule: 'rule',
  position: 'position',
  /** History: calendar window preset (`today|7d|30d|all|custom`). */
  range: 'range',
  /** History: custom window bounds (UTC wall-clock), used when `range=custom`. */
  from: 'from',
  to: 'to',
  /** History: rule filter (independent of the lane `rule` param). */
  hRule: 'hrule',
  /** History: mode filter (`real|paper|all`). */
  hMode: 'hmode',
  /** History: position status filter (`End`, `EntryFailed`, …). */
  hStatus: 'hstatus',
  /** History: exit-reason filter. */
  hExit: 'hexit',
  /** History: chart drill-down focus (`day:…` / `heat:…` / `pct:…` / `rule:…`). */
  hFocus: 'hfocus',
  /** History: summary Win%/Worst% tile lens (`win|loss`) — realized SOL sign. */
  hOutcome: 'houtcome',
  /** History: summary Fired/Closed/Open tile lens (the entered partitions). */
  hLane: 'hlane',
  /** History: summary Migrated tile lens (`1|0` — graduated to AMM or not). */
  hMigrated: 'hmigrated',
  // Arms section (the durable arm ledger). Its OWN `a*` channel, deliberately
  // not shared with History's `h*`: narrowing a PnL review must not silently
  // narrow the arm funnel, which describes a different population.
  /** Arms: calendar window preset (`today|7d|30d|all|custom`). */
  aRange: 'arange',
  /** Arms: custom window bounds (UTC wall-clock), used when `arange=custom`. */
  aFrom: 'afrom',
  aTo: 'ato',
  /** Arms: rule filter. */
  aRule: 'arule',
  /** Arms: mode filter (`real|paper|all`). */
  aMode: 'amode',
  /** Arms: end-reason filter (`entered|dead|…`, or `waiting` for a live episode). */
  aReason: 'areason',
} as const;

/** Deep-link into the Console **History** section with a preset cohort — the
 *  Portfolio scoreboard's per-rule "History" link. `scroll=history` tells the
 *  Console to bring the section into view on arrival. */
export function consoleHistoryHref(opts: {
  ruleId?: string | null;
  range?: HistoryRange;
  mode?: 'real' | 'paper' | 'all';
  from?: string;
  to?: string;
}): string {
  const q = new URLSearchParams();
  if (opts.ruleId) q.set(OPS_PARAMS.hRule, opts.ruleId);
  if (opts.range) q.set(OPS_PARAMS.range, opts.range);
  if (opts.mode) q.set(OPS_PARAMS.hMode, opts.mode);
  if (opts.from) q.set(OPS_PARAMS.from, opts.from);
  if (opts.to) q.set(OPS_PARAMS.to, opts.to);
  q.set('scroll', 'history');
  return `${STRATEGY_PATHS.console}?${q.toString()}`;
}

/** @deprecated The Console has no tabs — kept only for old link compatibility. */
export type OpsTab = 'waiting' | 'open' | 'attention' | 'recent';

export function consoleHref(opts?: {
  /** Legacy tab hint — ignored by the Console (lanes are always visible). */
  tab?: OpsTab;
  mode?: string;
  status?: string;
  mint?: string;
  ruleId?: string;
  positionId?: string | null;
}): string {
  if (!opts) return STRATEGY_PATHS.console;
  const q = new URLSearchParams();
  if (opts.mode) q.set(OPS_PARAMS.mode, opts.mode === 'paper' ? 'paper' : 'real');
  if (opts.status) q.set(OPS_PARAMS.status, opts.status);
  if (opts.mint) q.set(OPS_PARAMS.mint, opts.mint);
  if (opts.ruleId) q.set(OPS_PARAMS.rule, opts.ruleId);
  if (opts.positionId) q.set(OPS_PARAMS.position, opts.positionId);
  const s = q.toString();
  return s ? `${STRATEGY_PATHS.console}?${s}` : STRATEGY_PATHS.console;
}

/** @deprecated Prefer {@link consoleHref}. */
export const floorHref = consoleHref;

/**
 * Deep-link for a position/arm notification — always the Console with the
 * position focused; whichever lane the row lives in, it is on this one page
 * (defect #3 fix: there is no wrong tab to land in).
 */
export function opsNotifyHref(opts: {
  status: string;
  mode: string;
  mint: string;
  ruleId: string;
  positionId?: string | null;
}): string {
  return consoleHref({
    mode: opts.mode,
    mint: opts.mint,
    ruleId: opts.ruleId || undefined,
    positionId: opts.positionId,
  });
}
