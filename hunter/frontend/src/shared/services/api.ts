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

/** POST one page of a generic-engine run's matched tokens (`POST /api/strategies/simulate/{run_id}/matched`).
 *  The fingerprint-matched candidate pool the run's positions are a subset of;
 *  `run_id` is the rule id for a saved-rule run. Response body is `{tokens}`. */
export function fetchEngineMatchedPage(
  runId: string,
  body: TableRequestBody,
  signal?: AbortSignal,
): Promise<{ items: import('types').MatchedTokenRecord[]; total: number }> {
  return postTablePage(
    `/api/strategies/simulate/${encodeURIComponent(runId)}/matched`,
    body,
    (json) => (json as { tokens: import('types').MatchedTokenRecord[] }).tokens,
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

/** POST hold + wall-clock bins for the Temporal summary band. */
export function fetchEngineSimTimeSummary(
  runId: string,
  body: TableRequestBody,
  wallField: 'entry_time' | 'created_at' = 'entry_time',
  signal?: AbortSignal,
): Promise<import('types').TemporalSummaryPayload> {
  const q = new URLSearchParams({ wall_field: wallField });
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

/** POST the filtered aggregate for a rule's Simulated summary card. */
export function fetchSimulatedSummary(
  strategySeg: string,
  ruleId: string,
  body: TableRequestBody,
  signal?: AbortSignal,
): Promise<import('types').SimulatedSummary> {
  return request(
    `${API_BASE}/api/strategies/${strategySeg}/rules/${ruleId}/simulate/result/summary`,
    {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(body),
      signal,
    },
  );
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

/** Start a backtest for every rule of `strategy` whose last-simulation rollup is
 *  missing or stale (params edited since that run) — rules already fresh for
 *  `range` are skipped server-side, so repeated clicks are cheap. Each job runs
 *  through the same detached pipeline as a single `startSimulation`; watch for
 *  their `simulation_finished` SSE frames (or just wait for the rules list to
 *  refresh) rather than this call. `started_ids` are exactly the rules a backtest
 *  was queued for (freshly-cached ones are omitted, and never fire a progress /
 *  finished frame), so the caller can `markStarting` those and only those for the
 *  per-row progress bar without leaving a phantom bar on a skipped rule. */
export async function startSimulateAll(
  strategy: 'tpsl1' | 'tpsl2' | 'swing1',
  range: { from?: string; to?: string },
): Promise<{ started: number; started_ids: string[]; skipped: number; total: number }> {
  const qs = new URLSearchParams();
  if (range.from) qs.set('from', range.from);
  if (range.to) qs.set('to', range.to);
  const s = qs.toString();
  const url = `${API_BASE}/api/strategies/${strategy}/rules/simulate-all${s ? `?${s}` : ''}`;
  return request(url, { method: 'POST' });
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
