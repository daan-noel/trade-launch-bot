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

// Fresh-wallet pool lifecycle (docs/wallet-pool-plan.md Phase 1):
// generated -> funded -> reserved -> used -> retired.
export type WalletStatus = 'generated' | 'funded' | 'reserved' | 'used' | 'retired';

// Full pool row (Wallet Management page) — note there is no key_ref field here:
// the backend model marks it #[serde(skip_serializing)], so it never reaches
// the frontend at all.
export interface ManagedWalletPool {
  id: string;
  address: string;
  label: string | null;
  role: string;
  derivation_index: number | null;
  status: WalletStatus;
  funding_source: string | null;
  reserved_by_launch_id: string | null;
  reserved_at: string | null;
  balance_lamports: number | null;
  balance_checked_at: string | null;
  created_at: string;
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
  walletPool: (role?: string) =>
    getJson<ManagedWalletPool[]>(
      role ? `/api/wallet_pool?role=${encodeURIComponent(role)}` : '/api/wallet_pool',
    ),
  generateWallets: (role: string, count: number, labelPrefix?: string) =>
    postJson<ManagedWalletPool[]>('/api/wallet_pool/generate', {
      role,
      count,
      label_prefix: labelPrefix || undefined,
    }),
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

export function formatSol(lamports: number | null): string {
  if (lamports == null) return '—';
  return `${(lamports / 1_000_000_000).toFixed(4)} SOL`;
}

export function formatAge(iso: string): string {
  const ms = Date.now() - new Date(iso).getTime();
  const minutes = Math.floor(ms / 60_000);
  if (minutes < 1) return 'just now';
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

const TERMINAL_BUNDLE = new Set(['landed', 'dropped', 'partial', 'failed']);

export function shouldKeepPolling(status: LaunchStatus | null): boolean {
  if (!status) return false;
  if (status.launch.status === 'pending') return true;
  const bundleStatus = status.bundle?.status;
  if (!bundleStatus) return false;
  return !TERMINAL_BUNDLE.has(bundleStatus);
}
