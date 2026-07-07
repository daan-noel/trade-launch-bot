import bs58 from 'bs58';

export interface LaunchTemplate {
  id: string;
  template_name: string;
  variant: string;
  params: Record<string, unknown>;
}

export interface ManagedWallet {
  id: string;
  address: string;
  label: string | null;
  role: string;
}

export interface LaunchResult {
  launch_id: string;
  mint_address: string;
  create_signature: string;
  bundle?: {
    bundle_id: string;
    jito_bundle_id: string;
    leg_signatures: string[];
  };
}

export interface Launch {
  id: string;
  mint_address: string;
  status: string;
  create_signature: string | null;
  bundle_id: string | null;
}

export interface Bundle {
  id: string;
  status: string;
  jito_bundle_id: string | null;
  leg_signatures: string[];
  submitted_at: string | null;
  confirmed_at: string | null;
}

export interface TradePriced {
  trade_type: string;
  slot: number;
  amount_quote_display: number | null;
  amount_quote: number;
  wallet_id: number;
  tx_signature: number[] | string;
}

export interface LaunchStatus {
  launch: Launch;
  bundle: Bundle | null;
  trade_count: number;
  trades: TradePriced[];
}

async function getJson<T>(path: string): Promise<T> {
  const res = await fetch(path);
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`${res.status} ${path}: ${text}`);
  }
  return res.json() as Promise<T>;
}

async function postJson<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(path, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`${res.status} ${path}: ${text}`);
  }
  return res.json() as Promise<T>;
}

export const api = {
  templates: () => getJson<LaunchTemplate[]>('/api/launch_templates'),
  wallets: (role?: string) =>
    getJson<ManagedWallet[]>(
      role ? `/api/managed_wallets?role=${encodeURIComponent(role)}` : '/api/managed_wallets',
    ),
  executeLaunch: (template_id: string, dev_wallet_id: string) =>
    postJson<LaunchResult>('/api/launches/execute', { template_id, dev_wallet_id }),
  launchStatus: (id: string) => getJson<LaunchStatus>(`/api/launches/${id}/status`),
};

export function solscanTx(sig: string) {
  return `https://solscan.io/tx/${sig}`;
}

export function solscanMint(mint: string) {
  return `https://solscan.io/token/${mint}`;
}

export function formatSig(tx: TradePriced['tx_signature']): string {
  if (typeof tx === 'string') return tx;
  if (!tx?.length) return '—';
  return bs58.encode(Uint8Array.from(tx));
}

const TERMINAL_BUNDLE = new Set(['landed', 'dropped', 'partial', 'failed']);

export function shouldKeepPolling(status: LaunchStatus | null): boolean {
  if (!status) return false;
  if (status.launch.status === 'pending') return true;
  const bundleStatus = status.bundle?.status;
  if (!bundleStatus) return false;
  return !TERMINAL_BUNDLE.has(bundleStatus);
}
