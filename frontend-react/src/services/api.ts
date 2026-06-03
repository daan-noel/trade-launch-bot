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
): Promise<{ total: number; items: import('../types').TokenRecord[] }> {
  let url = `${API_BASE}/api/tokens?limit=${limit}&offset=${offset}`;
  if (search) url += `&search=${encodeURIComponent(search)}`;
  return request(url);
}

export async function fetchTokenDetail(
  mint: string,
): Promise<import('../types').TokenDetailRecord> {
  return request(`${API_BASE}/api/tokens/${mint}`);
}

export async function fetchTokenTrades(
  mint: string,
): Promise<import('../types').TradeRecord[]> {
  return request(`${API_BASE}/api/tokens/${encodeURIComponent(mint)}/trades`);
}

export async function fetchTpslRules(): Promise<import('../types').RuleRecord[]> {
  return request(`${API_BASE}/api/strategies/tpsl/rules`);
}

export async function createTpslRule(
  req: Record<string, unknown>,
): Promise<import('../types').RuleRecord> {
  return request(`${API_BASE}/api/strategies/tpsl/rules`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
}

export async function updateTpslRule(
  ruleId: string,
  req: Record<string, unknown>,
): Promise<import('../types').RuleRecord> {
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
): Promise<import('../types').SimulatedTokenResult[]> {
  return request(`${API_BASE}/api/strategies/tpsl/rules/${ruleId}/simulate`);
}

export async function fetchMatchedTokens(
  ruleId: string,
): Promise<import('../types').MatchedTokenRecord[]> {
  return request(`${API_BASE}/api/strategies/tpsl/rules/${ruleId}/matched`);
}

export async function fetchRulePositions(
  ruleId: string,
): Promise<import('../types').RulePositionRecord[]> {
  return request(`${API_BASE}/api/strategies/tpsl/rules/${ruleId}/positions`);
}

export async function fetchWalletHoldings(): Promise<import('../types').WalletHolding[]> {
  return request(`${API_BASE}/api/solana/wallet/tokens`);
}

export async function fetchAnalysis(
  limit: number,
  offset: number,
): Promise<{ total: number; items: import('../types').AnalysisRecord[] }> {
  return request(`${API_BASE}/api/analysis?limit=${limit}&offset=${offset}`);
}

export async function fetchCreators(
  limit: number,
  offset: number,
): Promise<{ total: number; items: import('../types').CreatorRecord[] }> {
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

export function sseUrl(): string {
  return `${API_BASE}/api/stream`;
}
