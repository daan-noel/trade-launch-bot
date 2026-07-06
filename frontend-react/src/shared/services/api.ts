import { API_BASE } from './config';
import { request } from './http';

// System reads (SOL price, live mode) and global app settings are served via
// RTK Query in `store/apiSlice.ts` (single deduped cache per endpoint) rather
// than these one-off fetch wrappers. `AppSettings` stays here as the shared
// shape the query/mutation are typed against.

/**
 * Global, server-wide app settings (persisted in `app_settings`). `timezone` and
 * `price_unit` are `null` until a client has set them. The shape mirrors the
 * backend `AppSettings` struct — add a field on both sides to add a setting.
 */
export interface AppSettings {
  track_mayhem: boolean;
  track_post_migration: boolean;
  /** Persist raw transaction blobs to `raw_transactions`. Off curbs DB growth. */
  persist_raw: boolean;
  timezone: string | null;
  price_unit: 'SOL' | 'USD' | null;
  /** Legacy combined slippage field; superseded by buy_slippage_bps / sell_slippage_bps. */
  slippage_bps: number | null;
  /** Buy-side slippage in bps (100 = 1%); null = use legacy slippage_bps then server default (5%). 0 = no floor. */
  buy_slippage_bps: number | null;
  /** Sell-side slippage in bps; null or 0 = no floor (always fills). Default is no floor. */
  sell_slippage_bps: number | null;
  /** Master switch for the ingest liveness watchdog (process-restart on stall). */
  watchdog_enabled: boolean;
  /** Stall window in seconds before the watchdog restarts a wedged ingest. */
  watchdog_stall_timeout_secs: number;
  /** How often (seconds) the watchdog checks the stall window. */
  watchdog_check_interval_secs: number;
  /** Hard ceiling (SOL) on total SOL committed to open real positions; null = no ceiling. */
  max_committed_sol: number | null;
  /** Enable gap-replay on LaserStream reconnect (default false — replayed creates get stale block_time). */
  gap_replay_on_reconnect: boolean;
  /** Max gap-replay window (seconds); gaps beyond this use a full re-subscribe. Default 300. */
  gap_replay_max_window_secs: number;
}

import type { TableRequestBody } from './tableRequest';

/**
 * POST one page of a server-side table (`{strategy}/rules/{id}/{table}`), reading
 * the full match `total` off the `X-Total-Count` header so the pager can size
 * itself without pulling the whole population. Kept as its own `fetch` (not the
 * shared `request` wrapper) because it needs the response header. `pluck` extracts
 * the row array from the JSON body — positions return a bare array, matched/sim
 * return `{tokens}`. Error + abort semantics mirror `request`.
 */
async function postTablePage<R>(
  path: string,
  body: TableRequestBody,
  pluck: (json: unknown) => R[],
  signal?: AbortSignal,
): Promise<{ items: R[]; total: number }> {
  const resp = await fetch(`${API_BASE}${path}`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
    signal,
  });
  if (!resp.ok) {
    const errBody = await resp.json().catch(() => null);
    const msg =
      errBody && typeof errBody === 'object' && 'error' in errBody
        ? String((errBody as { error: string }).error)
        : `HTTP ${resp.status}`;
    throw new Error(msg);
  }
  const items = pluck(await resp.json());
  const total = Number(resp.headers.get('X-Total-Count') ?? items.length);
  return { items, total: Number.isFinite(total) ? total : items.length };
}

/** Run-split scope for the by-rule positions/summary reads: `current` = the rule's
 *  latest run; `history` = every prior run. Omit for the legacy (unsplit) view. */
export type PositionScope = 'current' | 'history';

const scopeQuery = (scope?: PositionScope) => (scope ? `?scope=${scope}` : '');

/** POST one page of a rule's positions for `strategySeg` (`tpsl1`/`tpsl2`/`swing1`).
 *  Body is the unified `TableRequestBody`; response is a bare positions array.
 *  `scope` selects the current run vs. the run-history population. */
function fetchRulePositionsPage(
  strategySeg: string,
  ruleId: string,
  body: TableRequestBody,
  signal?: AbortSignal,
  scope?: PositionScope,
): Promise<import('types').RulePositionsPage> {
  return postTablePage(
    `/api/strategies/${strategySeg}/rules/${ruleId}/positions${scopeQuery(scope)}`,
    body,
    (json) => json as import('types').RulePositionRecord[],
    signal,
  );
}

/** POST one page of a rule's matched tokens. Response body is `{tokens}`. */
export function fetchMatchedPage(
  strategySeg: string,
  ruleId: string,
  body: TableRequestBody,
  signal?: AbortSignal,
): Promise<{ items: import('types').MatchedTokenRecord[]; total: number }> {
  return postTablePage(
    `/api/strategies/${strategySeg}/rules/${ruleId}/matched`,
    body,
    (json) => (json as { tokens: import('types').MatchedTokenRecord[] }).tokens,
    signal,
  );
}

/** POST one page of a rule's finished-simulation tokens. Response body is `{tokens}`. */
export function fetchSimulatedPage(
  strategySeg: string,
  ruleId: string,
  body: TableRequestBody,
  signal?: AbortSignal,
): Promise<{ items: import('types').SimulatedTokenResult[]; total: number }> {
  return postTablePage(
    `/api/strategies/${strategySeg}/rules/${ruleId}/simulate/result`,
    body,
    (json) => (json as { tokens: import('types').SimulatedTokenResult[] }).tokens,
    signal,
  );
}

/** Whole-population roll-up for the Holdings summary bar (server-computed over the
 *  filtered set). Mirrors the Rust `HoldingsTableSummary`. */
export interface HoldingsTableSummary {
  positions: number;
  total_value_sol: number | null;
  total_value_usd: number | null;
  total_cost_basis_sol: number;
  total_unrealized_pnl_sol: number | null;
  change_24h_pct: number | null;
}

/** POST one page of the wallet Holdings table (server-side search/sort/filter/paging
 *  over the composed holdings). `fresh` busts the server's short-TTL scan cache — the
 *  page sends it once after a confirmed trade so the reload reflects the new balance.
 *  Response body is a bare holdings array (like positions). */
export function fetchHoldingsPage(
  body: TableRequestBody,
  signal?: AbortSignal,
  fresh = false,
): Promise<{ items: import('types').WalletHolding[]; total: number }> {
  return postTablePage(
    `/api/portfolio/holdings/query${fresh ? '?fresh=1' : ''}`,
    body,
    (json) => json as import('types').WalletHolding[],
    signal,
  );
}

/** One composed holding by mint (or `null` if unheld) — reuses the warm holdings
 *  scan via the `in` filter, so the manual buy/sell dialogs get the authoritative
 *  `managed_by` / `token_account` / balance without a separate RPC. */
export async function fetchHoldingByMint(
  mint: string,
): Promise<import('types').WalletHolding | null> {
  const { items } = await fetchHoldingsPage({
    pagination: { page: 1, pageSize: 1 },
    sorting: [],
    search: '',
    filters: { mint: { op: 'in', val: [mint] } },
  });
  return items[0] ?? null;
}

/** GET-shaped POST for the Holdings summary bar — same filter body as the table so
 *  the totals cover exactly the filtered population (pagination/sorting ignored). */
export function fetchHoldingsSummary(
  body: TableRequestBody,
  signal?: AbortSignal,
): Promise<HoldingsTableSummary> {
  return request(`${API_BASE}/api/portfolio/holdings/summary`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
    signal,
  });
}

/** GET the whole-run aggregate for a rule's Simulated summary card. */
export function fetchSimulatedSummary(
  strategySeg: string,
  ruleId: string,
  signal?: AbortSignal,
): Promise<import('types').SimulatedSummary> {
  return request(
    `${API_BASE}/api/strategies/${strategySeg}/rules/${ruleId}/simulate/result/summary`,
    { signal },
  );
}

/**
 * Run swing detection over a single token.
 *
 * `opts.startMs` / `opts.endMs` restrict detection to a time range expressed in
 * milliseconds relative to the token's first trade; a `null` bound is left open.
 * `opts.curveOnly` restricts detection to bonding-curve trades (the
 * token-creation → migration phase). Omit `opts` to run over full history.
 */
export async function fetchTokenSwings(
  mint: string,
  params: import('types').SwingParams,
  opts?: { startMs: number | null; endMs: number | null; curveOnly?: boolean },
): Promise<import('types').SwingDetectionResult> {
  return request(`${API_BASE}/api/tokens/${encodeURIComponent(mint)}/swings`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      params,
      window_start_ms: opts?.startMs ?? null,
      window_end_ms: opts?.endMs ?? null,
      curve_only: opts?.curveOnly ?? false,
    }),
  });
}

/**
 * Start a "Swing Detection All" run as a detached background job (returns
 * immediately). The run scans the whole filtered token set (uncapped, can take
 * minutes), so its result is NOT delivered on this request — collect it via
 * {@link fetchSwingRunResult} once the `swing_detection_finished` SSE fires. This
 * decoupling is what stops a long run failing the client with `FETCH_ERROR`.
 *
 * `opts.runId` (required) keys the run's cancel flag, progress cell, result store,
 * and the raw legs the backend stashes so the server-side-paged tokens list can
 * sort by the chain columns. `opts.startMs` / `opts.endMs` restrict detection to a
 * time range in milliseconds relative to each token's first trade (`null` = open);
 * `opts.curveOnly` restricts to bonding-curve trades.
 */
export async function startSwingRun(
  mints: string[],
  params: import('types').SwingParams,
  opts: {
    runId: string;
    startMs: number | null;
    endMs: number | null;
    curveOnly?: boolean;
  },
): Promise<void> {
  await request(`${API_BASE}/api/tokens/swings/batch`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      mints,
      params,
      window_start_ms: opts.startMs ?? null,
      window_end_ms: opts.endMs ?? null,
      curve_only: opts.curveOnly ?? false,
      run_id: opts.runId,
    }),
  });
}

/** Collect a finished "Swing Detection All" run's result (single delivery — the
 *  backend removes the entry on read). Returns `{ cancelled: true }` if the run
 *  was cancelled. Call once the `swing_detection_finished` SSE for `runId` fires. */
export async function fetchSwingRunResult(
  runId: string,
): Promise<import('types').SwingBatchResponse | { cancelled: true }> {
  return request(`${API_BASE}/api/jobs/swings/${encodeURIComponent(runId)}/result`);
}

/** Cooperative cancel for an in-flight "Swing Detection All" run. No-op if none
 *  running for `runId`. */
export async function cancelSwingRun(runId: string): Promise<void> {
  await request(`${API_BASE}/api/jobs/swings/${encodeURIComponent(runId)}/cancel`, {
    method: 'POST',
  });
}

export async function fetchTpsl1Rules(): Promise<import('types').RuleRecord[]> {
  return request(`${API_BASE}/api/strategies/tpsl1/rules`);
}

export async function createTpsl1Rule(
  req: Record<string, unknown>,
): Promise<import('types').RuleRecord> {
  return request(`${API_BASE}/api/strategies/tpsl1/rules`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
}

export async function updateTpsl1Rule(
  ruleId: string,
  req: Record<string, unknown>,
): Promise<import('types').RuleRecord> {
  return request(`${API_BASE}/api/strategies/tpsl1/rules/${ruleId}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
}

export async function deleteTpsl1Rule(ruleId: string): Promise<void> {
  await request(`${API_BASE}/api/strategies/tpsl1/rules/${ruleId}`, { method: 'DELETE' });
}

/**
 * Lifecycle transitions (the single source of truth on the backend is
 * `tpsl_sniper_1::lifecycle`). Each returns the updated rule.
 *
 * `activate` — entries on. For paper rules, `paperRun` chooses a fresh run vs
 * continuing the prior one (ignored for real rules).
 */
export async function activateTpsl1Rule(
  ruleId: string,
  paperRun: 'fresh' | 'continue' = 'fresh',
): Promise<import('types').RuleRecord> {
  return request(`${API_BASE}/api/strategies/tpsl1/rules/${ruleId}/activate`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ paper_run: paperRun }),
  });
}

/** `pause` — entries off; open positions drain via the exit ladder. */
export async function pauseTpsl1Rule(ruleId: string): Promise<import('types').RuleRecord> {
  return request(`${API_BASE}/api/strategies/tpsl1/rules/${ruleId}/pause`, { method: 'POST' });
}

/** `stop` — pause and force-close every open position now (real = on-chain sells). */
export async function stopTpsl1Rule(ruleId: string): Promise<import('types').RuleRecord> {
  return request(`${API_BASE}/api/strategies/tpsl1/rules/${ruleId}/stop`, { method: 'POST' });
}

/** Bulk `pause` — every currently active `mode` rule (Real or Paper section). */
export async function pauseAllTpsl1Rules(mode: 'real' | 'paper'): Promise<import('types').BulkRuleResult> {
  return request(`${API_BASE}/api/strategies/tpsl1/rules/pause-all?mode=${mode}`, { method: 'POST' });
}

/** Bulk `stop` — every active-or-draining `mode` rule (Real or Paper section). */
export async function stopAllTpsl1Rules(mode: 'real' | 'paper'): Promise<import('types').BulkRuleResult> {
  return request(`${API_BASE}/api/strategies/tpsl1/rules/stop-all?mode=${mode}`, { method: 'POST' });
}

/** Clear a paper rule's recorded run history (runs + positions). Paper-only and
 *  only valid while the rule is idle; the backend rejects live rules. */
export async function clearTpsl1PaperResult(ruleId: string): Promise<void> {
  await request(`${API_BASE}/api/strategies/tpsl1/rules/${ruleId}/paper-result`, {
    method: 'DELETE',
  });
}

export function fetchTpsl1RulePositions(
  ruleId: string,
  body: TableRequestBody,
  signal?: AbortSignal,
  scope?: PositionScope,
): Promise<import('types').RulePositionsPage> {
  return fetchRulePositionsPage('tpsl1', ruleId, body, signal, scope);
}

/** Run/rule-wide position aggregates for the tpsl1 Positions Summary panel. */
export async function fetchTpsl1RulePositionsSummary(
  ruleId: string,
  signal?: AbortSignal,
  scope?: PositionScope,
): Promise<import('types').PositionsSummary> {
  return request(
    `${API_BASE}/api/strategies/tpsl1/rules/${ruleId}/positions/summary${scopeQuery(scope)}`,
    { signal },
  );
}

// ---------------------------------------------------------------------------
// Strategy: tpsl_sniper_2 (clone of tpsl, separate /strategies/tpsl2 endpoints)
// ---------------------------------------------------------------------------

export async function fetchTpsl2Rules(): Promise<import('types').RuleRecord[]> {
  return request(`${API_BASE}/api/strategies/tpsl2/rules`);
}

export async function createTpsl2Rule(
  req: Record<string, unknown>,
): Promise<import('types').RuleRecord> {
  return request(`${API_BASE}/api/strategies/tpsl2/rules`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
}

export async function updateTpsl2Rule(
  ruleId: string,
  req: Record<string, unknown>,
): Promise<import('types').RuleRecord> {
  return request(`${API_BASE}/api/strategies/tpsl2/rules/${ruleId}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
}

export async function deleteTpsl2Rule(ruleId: string): Promise<void> {
  await request(`${API_BASE}/api/strategies/tpsl2/rules/${ruleId}`, { method: 'DELETE' });
}

/**
 * Lifecycle transitions (the single source of truth on the backend is
 * `tpsl_sniper_2::lifecycle`). Each returns the updated rule.
 *
 * `activate` — entries on. For paper rules, `paperRun` chooses a fresh run vs
 * continuing the prior one (ignored for real rules).
 */
export async function activateTpsl2Rule(
  ruleId: string,
  paperRun: 'fresh' | 'continue' = 'fresh',
): Promise<import('types').RuleRecord> {
  return request(`${API_BASE}/api/strategies/tpsl2/rules/${ruleId}/activate`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ paper_run: paperRun }),
  });
}

/** `pause` — entries off; open positions drain via the exit ladder. */
export async function pauseTpsl2Rule(ruleId: string): Promise<import('types').RuleRecord> {
  return request(`${API_BASE}/api/strategies/tpsl2/rules/${ruleId}/pause`, { method: 'POST' });
}

/** `stop` — pause and force-close every open position now (real = on-chain sells). */
export async function stopTpsl2Rule(ruleId: string): Promise<import('types').RuleRecord> {
  return request(`${API_BASE}/api/strategies/tpsl2/rules/${ruleId}/stop`, { method: 'POST' });
}

/** Bulk `pause` — every currently active `mode` rule (Real or Paper section). */
export async function pauseAllTpsl2Rules(mode: 'real' | 'paper'): Promise<import('types').BulkRuleResult> {
  return request(`${API_BASE}/api/strategies/tpsl2/rules/pause-all?mode=${mode}`, { method: 'POST' });
}

/** Bulk `stop` — every active-or-draining `mode` rule (Real or Paper section). */
export async function stopAllTpsl2Rules(mode: 'real' | 'paper'): Promise<import('types').BulkRuleResult> {
  return request(`${API_BASE}/api/strategies/tpsl2/rules/stop-all?mode=${mode}`, { method: 'POST' });
}

/** Clear a paper rule's recorded run history (runs + positions). Paper-only and
 *  only valid while the rule is idle; the backend rejects live rules. */
export async function clearTpsl2PaperResult(ruleId: string): Promise<void> {
  await request(`${API_BASE}/api/strategies/tpsl2/rules/${ruleId}/paper-result`, {
    method: 'DELETE',
  });
}

export function fetchTpsl2RulePositions(
  ruleId: string,
  body: TableRequestBody,
  signal?: AbortSignal,
  scope?: PositionScope,
): Promise<import('types').RulePositionsPage> {
  return fetchRulePositionsPage('tpsl2', ruleId, body, signal, scope);
}

/** Run/rule-wide position aggregates for the tpsl2 Positions Summary panel. */
export async function fetchTpsl2RulePositionsSummary(
  ruleId: string,
  signal?: AbortSignal,
  scope?: PositionScope,
): Promise<import('types').PositionsSummary> {
  return request(
    `${API_BASE}/api/strategies/tpsl2/rules/${ruleId}/positions/summary${scopeQuery(scope)}`,
    { signal },
  );
}

// ---------------------------------------------------------------------------
// swing1 rules + positions (live) — mirrors the tpsl2 block over the generic
// `/api/strategies/{strategy}/…` live routes (`strategy` seg = `swing1`).
// ---------------------------------------------------------------------------

export async function fetchSwing1Rules(): Promise<import('types').RuleRecord[]> {
  return request(`${API_BASE}/api/strategies/swing1/rules`);
}

export async function createSwing1Rule(
  req: Record<string, unknown>,
): Promise<import('types').RuleRecord> {
  return request(`${API_BASE}/api/strategies/swing1/rules`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
}

export async function updateSwing1Rule(
  ruleId: string,
  req: Record<string, unknown>,
): Promise<import('types').RuleRecord> {
  return request(`${API_BASE}/api/strategies/swing1/rules/${ruleId}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
}

export async function deleteSwing1Rule(ruleId: string): Promise<void> {
  await request(`${API_BASE}/api/strategies/swing1/rules/${ruleId}`, { method: 'DELETE' });
}

/** `activate` — entries on. For paper rules, `paperRun` chooses fresh vs continue. */
export async function activateSwing1Rule(
  ruleId: string,
  paperRun: 'fresh' | 'continue' = 'fresh',
): Promise<import('types').RuleRecord> {
  return request(`${API_BASE}/api/strategies/swing1/rules/${ruleId}/activate`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ paper_run: paperRun }),
  });
}

/** `pause` — entries off; open positions drain via the exit ladder. */
export async function pauseSwing1Rule(ruleId: string): Promise<import('types').RuleRecord> {
  return request(`${API_BASE}/api/strategies/swing1/rules/${ruleId}/pause`, { method: 'POST' });
}

/** `stop` — pause and force-close every open position now. */
export async function stopSwing1Rule(ruleId: string): Promise<import('types').RuleRecord> {
  return request(`${API_BASE}/api/strategies/swing1/rules/${ruleId}/stop`, { method: 'POST' });
}

/** Bulk `pause` — every currently active `mode` rule (Real or Paper section). */
export async function pauseAllSwing1Rules(mode: 'real' | 'paper'): Promise<import('types').BulkRuleResult> {
  return request(`${API_BASE}/api/strategies/swing1/rules/pause-all?mode=${mode}`, { method: 'POST' });
}

/** Bulk `stop` — every active-or-draining `mode` rule (Real or Paper section). */
export async function stopAllSwing1Rules(mode: 'real' | 'paper'): Promise<import('types').BulkRuleResult> {
  return request(`${API_BASE}/api/strategies/swing1/rules/stop-all?mode=${mode}`, { method: 'POST' });
}

export function fetchSwing1RulePositions(
  ruleId: string,
  body: TableRequestBody,
  signal?: AbortSignal,
  scope?: PositionScope,
): Promise<import('types').RulePositionsPage> {
  return fetchRulePositionsPage('swing1', ruleId, body, signal, scope);
}

/** Run/rule-wide position aggregates for the swing1 Positions Summary panel. */
export async function fetchSwing1RulePositionsSummary(
  ruleId: string,
  signal?: AbortSignal,
  scope?: PositionScope,
): Promise<import('types').PositionsSummary> {
  return request(
    `${API_BASE}/api/strategies/swing1/rules/${ruleId}/positions/summary${scopeQuery(scope)}`,
    { signal },
  );
}

/** Clear a swing1 paper rule's recorded run history (runs + positions). Paper-only
 *  and only valid while the rule is idle; the backend rejects live rules. */
export async function clearSwing1PaperResult(ruleId: string): Promise<void> {
  await request(`${API_BASE}/api/strategies/swing1/rules/${ruleId}/paper-result`, {
    method: 'DELETE',
  });
}

// ---------------------------------------------------------------------------
// Profiles & Wallets
// ---------------------------------------------------------------------------

export async function createProfile(req: {
  name: string;
  profile_type: import('types').ProfileType;
}): Promise<import('types').WalletProfile> {
  return request(`${API_BASE}/api/profiles`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
}

export async function updateProfile(
  id: string,
  req: { name: string; profile_type: import('types').ProfileType },
): Promise<void> {
  await request(`${API_BASE}/api/profiles/${id}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
}

export async function deleteProfile(id: string): Promise<void> {
  await request(`${API_BASE}/api/profiles/${id}`, { method: 'DELETE' });
}

export async function createWalletEntry(
  profileId: string,
  req: { address: string; is_tracked?: boolean; comment?: string },
): Promise<import('types').WalletEntry> {
  return request(`${API_BASE}/api/profiles/${profileId}/wallets`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
}

export async function updateWalletEntry(
  id: string,
  req: { is_tracked: boolean; comment: string | null },
): Promise<void> {
  await request(`${API_BASE}/api/wallets/${id}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
}

export async function deleteWalletEntry(id: string): Promise<void> {
  await request(`${API_BASE}/api/wallets/${id}`, { method: 'DELETE' });
}

// ---------------------------------------------------------------------------
// Tags
// ---------------------------------------------------------------------------

export async function fetchTags(): Promise<import('types').WalletProfileTag[]> {
  return request(`${API_BASE}/api/tags`);
}

export async function createTag(req: {
  name: string;
  color: string;
  comment?: string;
}): Promise<import('types').WalletProfileTag> {
  return request(`${API_BASE}/api/tags`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
}

export async function updateTag(
  id: string,
  req: { name: string; color: string; comment?: string },
): Promise<void> {
  await request(`${API_BASE}/api/tags/${id}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
}

export async function deleteTag(id: string): Promise<void> {
  await request(`${API_BASE}/api/tags/${id}`, { method: 'DELETE' });
}

export async function updateProfileTags(profileId: string, tagIds: string[]): Promise<void> {
  await request(`${API_BASE}/api/profiles/${profileId}/tags`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ tag_ids: tagIds }),
  });
}

/** Request cancellation of the in-flight grouped sweep (cooperative — the engine
 *  polls the flag between groups and bails). No-op if none is running. */
export async function cancelGroupedSweep(): Promise<void> {
  await request(`${API_BASE}/api/strategies/sweeps/cancel`, { method: 'POST' });
}

/** Start a rule's backtest as a detached background job (returns immediately).
 *  The run is uncapped and may take minutes, so the result is NOT delivered on
 *  this request — collect it via the result endpoint (`getStrategySimulateResult`)
 *  once the `simulation_finished` SSE fires. `range` scopes the candidate scan
 *  (empty = all-time). This decoupling is what stops a long sim failing the client
 *  with `FETCH_ERROR`. */
export async function startSimulation(
  strategy: 'tpsl1' | 'tpsl2' | 'swing1',
  ruleId: string,
  range: { from?: string; to?: string },
): Promise<void> {
  const qs = new URLSearchParams();
  if (range.from) qs.set('from', range.from);
  if (range.to) qs.set('to', range.to);
  const s = qs.toString();
  const url = `${API_BASE}/api/strategies/${strategy}/rules/${encodeURIComponent(ruleId)}/simulate${
    s ? `?${s}` : ''
  }`;
  await request(url, { method: 'POST' });
}

/** Strategy-agnostic cancel for a rule's in-flight simulation (the backend keys
 *  the cancel flag by rule_id across both tpsl snipers). No-op if none running. */
export async function cancelSimulation(ruleId: string): Promise<void> {
  await request(`${API_BASE}/api/jobs/simulations/${encodeURIComponent(ruleId)}/cancel`, {
    method: 'POST',
  });
}

/** Snapshot of every running background job (sweep + simulations) for recovering
 *  the progress UI after a page load — SSE only delivers future frames. */
export async function getJobsStatus(): Promise<import('types').JobsStatus> {
  return request(`${API_BASE}/api/jobs/status`);
}

/** Estimate (signatures only) how many transactions a sync would download. */
export interface SyncPreview {
  /** Transactions "Fetch New" would download (newer than the watermark). */
  new_count: number;
  /** True if the new-count hit the page cap (display as "N+"). */
  new_capped: boolean;
  /** Transactions "Fetch All" would download (full history, capped). */
  total_count: number;
  /** True if the total hit the page cap (display as "N+"). */
  total_capped: boolean;
  is_migrated: boolean;
}

export async function fetchSyncPreview(
  mintAddress: string,
  includePostMigrate: boolean,
  signal?: AbortSignal,
): Promise<SyncPreview> {
  return request(`${API_BASE}/api/token/sync/preview`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      mint_address: mintAddress.trim(),
      include_post_migrate: includePostMigrate,
    }),
    signal,
  });
}

export async function syncToken(
  mintAddress: string,
  includePostMigrate: boolean,
  onProgress: (event: import('types').SyncProgressEvent) => void,
  signal?: AbortSignal,
  incremental = false,
): Promise<import('types').SyncCompleteEvent> {
  const resp = await fetch(`${API_BASE}/api/token/sync`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      Accept: 'application/x-ndjson',
    },
    body: JSON.stringify({
      mint_address: mintAddress.trim(),
      include_post_migrate: includePostMigrate,
      incremental,
    }),
    signal,
  });

  if (!resp.ok) {
    const body = (await resp.json().catch(() => null)) as { message?: string } | null;
    throw new Error(
      body?.message ?? `Sync failed (HTTP ${resp.status})`,
    );
  }

  const reader = resp.body?.getReader();
  if (!reader) {
    throw new Error('Sync response has no body');
  }

  const decoder = new TextDecoder();
  let buffer = '';

  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });

    let newline: number;
    while ((newline = buffer.indexOf('\n')) >= 0) {
      const line = buffer.slice(0, newline).trim();
      buffer = buffer.slice(newline + 1);
      if (!line) continue;

      const event = JSON.parse(line) as import('types').SyncStreamEvent;
      if (event.type === 'progress') {
        onProgress(event);
      } else if (event.type === 'error') {
        throw new Error(event.message);
      } else if (event.type === 'complete') {
        return event;
      }
    }
  }

  const tail = buffer.trim();
  if (tail) {
    const event = JSON.parse(tail) as import('types').SyncStreamEvent;
    if (event.type === 'complete') return event;
    if (event.type === 'error') throw new Error(event.message);
    if (event.type === 'progress') onProgress(event);
  }

  throw new Error('Sync ended without a complete response');
}
