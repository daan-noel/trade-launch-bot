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
  /** Buy-side slippage in bps (100 = 1%), used exactly as typed; null (blank) = the
   *  server's buy default. `0` is rejected with a 400 — blank is how you say "no limit". */
  buy_slippage_bps: number | null;
  /** Sell-side slippage in bps, used exactly as typed; null (blank) = no floor
   *  (min_out = 1, sell all). `0` is rejected with a 400. */
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
  /** Copycat guard: skip a token whose (name, symbol) was already traded on a
   *  different mint inside the window. Default false. */
  skip_duplicate_identity: boolean;
  /** Copycat-guard memory horizon in hours. Default 168 (7 days); `0` is a 400. */
  duplicate_identity_window_hours: number;
  /** RFC3339 instant the copycat guard was first enabled — the floor of its boot
   *  rebuild, so enabling it starts from an empty memory. Read-only; null = never. */
  duplicate_identity_since: string | null;
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

/**
 * POST one page of a rule's live/paper positions.
 * `scope`: `current` | `history` | `all` | `run` (with `runSeq`).
 * Response body is a bare array; total from `X-Total-Count`.
 * Strategy path segment is ignored by the generic engine — pass `"generic"`.
 */
export type PositionFetchScope =
  | { kind: 'current' }
  | { kind: 'history' }
  | { kind: 'all' }
  /** `mode` disambiguates `runSeq`, which is monotonic per `(rule, mode)` — a rule
   *  that has traded in both has two runs numbered `#1`. Omit for the rule's own
   *  `trade_mode`. */
  | { kind: 'run'; runSeq: number; mode?: string }
  | { kind: 'legacy' };

function positionScopeQuery(scope: PositionFetchScope | undefined): string {
  if (!scope || scope.kind === 'legacy') return '';
  if (scope.kind === 'run') {
    const mode = scope.mode ? `&mode=${encodeURIComponent(scope.mode)}` : '';
    return `?scope=run&run_seq=${encodeURIComponent(String(scope.runSeq))}${mode}`;
  }
  return `?scope=${scope.kind}`;
}

export function fetchRulePositionsPage(
  strategySeg: string,
  ruleId: string,
  body: TableRequestBody,
  scope: PositionFetchScope | 'current' | 'history' | undefined,
  signal?: AbortSignal,
): Promise<{ items: import('types').RulePositionRecord[]; total: number }> {
  const normalized: PositionFetchScope | undefined =
    scope == null
      ? undefined
      : typeof scope === 'string'
        ? { kind: scope }
        : scope;
  const q = positionScopeQuery(normalized);
  return postTablePage(
    `/api/strategies/${strategySeg}/rules/${encodeURIComponent(ruleId)}/positions${q}`,
    body,
    (json) => json as import('types').RulePositionRecord[],
    signal,
  );
}

/**
 * POST one page of the **cross-rule** position history (Console History, B1).
 * Same wire contract and server SQL as {@link fetchRulePositionsPage}; the cohort
 * narrows only through the body's filters (`mode` / `rule_id` / `status` /
 * `exit_reason`) and its `range` window. Body is a bare array; total from
 * `X-Total-Count`.
 */
export function fetchPortfolioPositionsPage(
  body: TableRequestBody,
  signal?: AbortSignal,
): Promise<{ items: import('types').RulePositionRecord[]; total: number }> {
  return postTablePage(
    '/api/portfolio/positions/query',
    body,
    (json) => json as import('types').RulePositionRecord[],
    signal,
  );
}

/**
 * POST one page of the **arm ledger** (`strategy_arms`) — the Console Arms
 * section. Same {@link TableRequestBody} contract as the positions pair, so the
 * table's paging/sort/filters and the section's date range travel in one body.
 *
 * This is the DURABLE read. `GET /api/strategies/armed` is its live twin (the
 * in-RAM registry behind the Waiting lane) and answers a different question —
 * see `docs/plans/strategies/arm-ledger.md`.
 */
export function fetchArmsPage(
  body: TableRequestBody,
  signal?: AbortSignal,
): Promise<{ items: import('lib/strategy/types').StrategyArmRecord[]; total: number }> {
  return postTablePage(
    '/api/strategies/arms/query',
    body,
    (json) => json as import('lib/strategy/types').StrategyArmRecord[],
    signal,
  );
}

/**
 * POST the arm funnel over the same cohort {@link fetchArmsPage} pages
 * (pagination/sort ignored). Aggregated in Postgres, so the counts stay exact
 * past the page size instead of re-stating themselves on every page turn.
 */
export function fetchArmsSummary(
  body: TableRequestBody,
  signal?: AbortSignal,
): Promise<import('lib/strategy/types').ArmFunnel> {
  return request(`${API_BASE}/api/strategies/arms/summary`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
    signal,
  });
}

/**
 * POST the **cross-rule** Positions Summary aggregate (Console History's summary
 * strip). Aggregate twin of {@link fetchPortfolioPositionsPage}: identical body,
 * identical cohort, pagination/sort ignored — so the strip totals exactly the
 * population the table pages, past the current page and without shipping rows.
 */
export function fetchPortfolioPositionsSummary(
  body: TableRequestBody,
  signal?: AbortSignal,
): Promise<import('types').PositionsSummary> {
  return request(`${API_BASE}/api/portfolio/positions/summary`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
    signal,
  });
}

/** POST the filtered Positions Summary aggregate for a rule (same scope/filters as the table). */
export function fetchRulePositionsSummary(
  strategySeg: string,
  ruleId: string,
  body: TableRequestBody,
  scope: PositionFetchScope | 'current' | 'history' | undefined,
  signal?: AbortSignal,
): Promise<import('types').PositionsSummary> {
  const normalized: PositionFetchScope | undefined =
    scope == null
      ? undefined
      : typeof scope === 'string'
        ? { kind: scope }
        : scope;
  const q = positionScopeQuery(normalized);
  return request(
    `${API_BASE}/api/strategies/${strategySeg}/rules/${encodeURIComponent(ruleId)}/positions/summary${q}`,
    {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(body),
      signal,
    },
  );
}

/** POST one page of a generic-engine simulate run (`POST /api/strategies/simulate/{run_id}/result`).
 *  `run_id` is the rule id for a saved-rule run (same key as the rules table). */
export function fetchEngineSimPage(
  runId: string,
  body: TableRequestBody,
  signal?: AbortSignal,
): Promise<{ items: import('types').SimulatedTokenResult[]; total: number }> {
  return postTablePage(
    `/api/strategies/simulate/${encodeURIComponent(runId)}/result`,
    body,
    (json) => (json as { tokens: import('types').SimulatedTokenResult[] }).tokens,
    signal,
  );
}

/** POST the filtered aggregate for a generic-engine simulate run. */
export function fetchEngineSimSummary(
  runId: string,
  body: TableRequestBody,
  signal?: AbortSignal,
): Promise<import('types').SimulatedSummary> {
  return request(
    `${API_BASE}/api/strategies/simulate/${encodeURIComponent(runId)}/result/summary`,
    {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(body),
      signal,
    },
  );
}

/**
 * POST hold + wall-clock bins for the Temporal summary band.
 *
 * `timeZone` is not cosmetic: the wall bins are civil-time buckets, and the
 * server floors them in this zone so they line up with the Timing calendar +
 * heatmap the client folds locally. Omit it and the server buckets in UTC.
 */
export function fetchEngineSimTimeSummary(
  runId: string,
  body: TableRequestBody,
  wallField: import('lib/strategy/temporalSummary').WallTimeField = 'exit_time',
  wallGrain: import('lib/strategy/temporalSummary').WallGrainChoice = 'auto',
  holdScheme: import('lib/strategy/temporalSummary').HoldSchemeChoice = 'auto',
  timeZone = 'UTC',
  signal?: AbortSignal,
): Promise<import('types').TemporalSummaryPayload> {
  const q = new URLSearchParams({
    wall_field: wallField,
    wall_grain: wallGrain,
    hold_scheme: holdScheme,
    tz: timeZone,
  });
  return request(
    `${API_BASE}/api/strategies/simulate/${encodeURIComponent(runId)}/result/time-summary?${q}`,
    {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(body),
      signal,
    },
  );
}

/** One cash line for the Wallet cash strip. Mirrors Rust `CashHoldingSummary`. */
export interface CashHoldingSummary {
  mint_address: string;
  symbol: string;
  ui_amount: number;
  value_usd: number;
  value_sol: number | null;
}

/** Roll-up for the Holdings summary bar + cash strip. Cash is unfiltered; position
 *  metrics match the filtered meme table. Native SOL is the System-account
 *  balance (push-fed cache), separate from USDC cash. Mirrors Rust
 *  `HoldingsTableSummary`. */
export interface HoldingsTableSummary {
  positions: number;
  /** Native wallet SOL; `null` until the live bin's balance cache is seeded. */
  sol_balance_sol: number | null;
  sol_balance_usd: number | null;
  cash_holdings: CashHoldingSummary[];
  cash_value_usd: number | null;
  cash_value_sol: number | null;
  positions_value_sol: number | null;
  positions_value_usd: number | null;
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
    `/api/portfolio/holdings/query${fresh ? '?fresh=true' : ''}`,
    body,
    (json) => json as import('types').WalletHolding[],
    signal,
  );
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

/** Cooperative cancel for the in-flight flow-discovery job. */
export async function cancelFlowDiscovery(): Promise<void> {
  await request(`${API_BASE}/api/strategies/flow-discovery/cancel`, { method: 'POST' });
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
