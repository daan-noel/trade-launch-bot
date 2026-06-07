import { API_BASE } from './config';

async function request<T>(url: string, init?: RequestInit): Promise<T> {
  const resp = await fetch(url, init);
  if (!resp.ok) {
    const body = await resp.json().catch(() => null);
    const msg =
      body && typeof body === 'object' && 'error' in body
        ? String((body as { error: string }).error)
        : `HTTP ${resp.status}`;
    throw new Error(msg);
  }
  if (resp.status === 204) return undefined as T;
  return resp.json() as Promise<T>;
}

export async function fetchSolPrice(): Promise<number | null> {
  const data = await request<{ usd_rate: number | null }>(`${API_BASE}/api/system/price`);
  return data.usd_rate;
}

export async function fetchLiveMode(): Promise<boolean> {
  const data = await request<{ live: boolean }>(`${API_BASE}/api/system/live`);
  return data.live;
}

export async function setLiveMode(live: boolean): Promise<boolean> {
  const data = await request<{ live: boolean }>(`${API_BASE}/api/system/live`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ live }),
  });
  return data.live;
}

export async function fetchTokens(
  search: string,
  limit: number,
  offset: number,
): Promise<{ total: number; items: import('types').TokenRecord[] }> {
  let url = `${API_BASE}/api/tokens?limit=${limit}&offset=${offset}`;
  if (search) url += `&search=${encodeURIComponent(search)}`;
  return request(url);
}

export async function fetchTokenDetail(
  mint: string,
): Promise<import('types').TokenDetailRecord> {
  return request(`${API_BASE}/api/tokens/${mint}`);
}

export async function fetchTokenTrades(
  mint: string,
): Promise<import('types').TradeRecord[]> {
  return request(`${API_BASE}/api/tokens/${encodeURIComponent(mint)}/trades`);
}

export async function fetchTokenSwings(
  mint: string,
  params: import('types').SwingParams,
): Promise<import('types').SwingDetectionResult> {
  return request(`${API_BASE}/api/tokens/${encodeURIComponent(mint)}/swings`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(params),
  });
}

/**
 * Run swing detection over many mints in one request (shared params).
 *
 * `opts.startMs` / `opts.endMs` restrict detection to a time range expressed in
 * milliseconds relative to each token's first trade; a `null` bound is left
 * open. `opts.curveOnly` restricts detection to bonding-curve trades (the
 * token-creation → migration phase). Omit `opts` to run over full history.
 */
export async function fetchTokenSwingsBatch(
  mints: string[],
  params: import('types').SwingParams,
  opts?: { startMs: number | null; endMs: number | null; curveOnly?: boolean },
): Promise<import('types').SwingBatchResponse> {
  return request(`${API_BASE}/api/tokens/swings/batch`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      mints,
      params,
      window_start_ms: opts?.startMs ?? null,
      window_end_ms: opts?.endMs ?? null,
      curve_only: opts?.curveOnly ?? false,
    }),
  });
}

export async function fetchTpslRules(): Promise<import('types').RuleRecord[]> {
  return request(`${API_BASE}/api/strategies/tpsl/rules`);
}

export async function createTpslRule(
  req: Record<string, unknown>,
): Promise<import('types').RuleRecord> {
  return request(`${API_BASE}/api/strategies/tpsl/rules`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
}

export async function updateTpslRule(
  ruleId: string,
  req: Record<string, unknown>,
): Promise<import('types').RuleRecord> {
  return request(`${API_BASE}/api/strategies/tpsl/rules/${ruleId}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
}

export async function deleteTpslRule(ruleId: string): Promise<void> {
  await request(`${API_BASE}/api/strategies/tpsl/rules/${ruleId}`, { method: 'DELETE' });
}

export async function simulateTpslRule(
  ruleId: string,
): Promise<import('types').SimulatedTokenResult[]> {
  return request(`${API_BASE}/api/strategies/tpsl/rules/${ruleId}/simulate`);
}

export async function fetchMatchedTokens(
  ruleId: string,
): Promise<import('types').MatchedTokenRecord[]> {
  return request(`${API_BASE}/api/strategies/tpsl/rules/${ruleId}/matched`);
}

export async function fetchRulePositions(
  ruleId: string,
): Promise<import('types').RulePositionRecord[]> {
  return request(`${API_BASE}/api/strategies/tpsl/rules/${ruleId}/positions`);
}

export async function fetchWalletHoldings(): Promise<import('types').WalletHolding[]> {
  return request(`${API_BASE}/api/solana/wallet/tokens`);
}

export async function fetchAnalysis(
  limit: number,
  offset: number,
): Promise<{ total: number; items: import('types').AnalysisRecord[] }> {
  return request(`${API_BASE}/api/analysis?limit=${limit}&offset=${offset}`);
}

export async function fetchCreators(
  limit: number,
  offset: number,
): Promise<{ total: number; items: import('types').CreatorRecord[] }> {
  return request(`${API_BASE}/api/creators?limit=${limit}&offset=${offset}`);
}

export async function tradeBuy(req: {
  mint: string;
  sol_amount: number;
  token_program_id: string;
}): Promise<boolean> {
  await request(`${API_BASE}/api/solana/wallet/buy`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
  return true;
}

export async function tradeSell(req: {
  mint: string;
  token_amount: number;
  token_account: string;
}): Promise<boolean> {
  await request(`${API_BASE}/api/solana/wallet/sell`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
  return true;
}

// ---------------------------------------------------------------------------
// Profiles & Wallets
// ---------------------------------------------------------------------------

export async function fetchProfiles(): Promise<import('types').WalletProfile[]> {
  return request(`${API_BASE}/api/profiles`);
}

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

export function sseUrl(): string {
  return `${API_BASE}/api/stream`;
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
