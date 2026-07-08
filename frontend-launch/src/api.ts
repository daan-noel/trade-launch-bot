import bs58 from 'bs58';

export interface LaunchTemplate {
  id: string;
  template_name: string;
  launchpad_id: number;
  quote_asset_id: number;
  variant: string;
  // Token identity (name/symbol/uri) resolves from this metadata_templates row —
  // the single source of truth. null only for an unlinked legacy row.
  metadata_template_id: string | null;
  params: PumpfunTemplateParams;
  created_at: string;
  updated_at: string;
}

// Audited pump.fun buy discriminator surface (`crates/launcher/src/bundle.rs`
// `BuyVariant`) — never an arbitrary account list.
export type BuyVariant = 'buy' | 'buy_exact_sol_in' | 'buy_v2' | 'buy_exact_quote_in';

// One entry in `params.leg_structures` — ranges the composer randomizes within
// per leg (`crates/launcher/src/bundle.rs` `LegStructureRecipe`). All amounts are
// quote base units (lamports for SOL).
export interface LegStructureRecipe {
  variant: BuyVariant;
  slippage_bps_min?: number;
  slippage_bps_max?: number;
  cu_limit_min?: number;
  cu_limit_max?: number;
  cu_price_min?: number;
  cu_price_max?: number;
  tip_quote_min?: number;
  tip_quote_max?: number;
}

// `crates/launcher/src/service.rs` `PumpfunTemplateParams` — the parsed
// `launch_templates.params` JSONB brain, server-validated on create/update
// against this exact shape.
export interface PumpfunTemplateParams {
  dev_buy_quote?: number;
  slippage_bps?: number;
  is_mayhem_mode?: boolean;
  cashback_enabled?: boolean;
  bundle_leg_count?: number;
  bundle_quote_per_leg?: number;
  bundle_tip_quote?: number;
  leg_structures?: LegStructureRecipe[];
}

// Create/update body — full-replace, mirrors the backend's
// `NewLaunchTemplate`/`UpdateLaunchTemplate` (same 5-field shape).
export interface NewLaunchTemplateInput {
  template_name: string;
  launchpad_id: number;
  variant: string;
  quote_asset_id: number;
  metadata_template_id: string | null;
  params: PumpfunTemplateParams;
}

export interface Launchpad {
  id: number;
  key: string;
  display_name: string;
  default_quote_asset_id: number;
}

export interface QuoteAsset {
  id: number;
  mint: string;
  symbol: string;
  decimals: number;
  is_native: boolean;
}

// Fresh-wallet pool lifecycle (docs/wallet-pool-plan.md Phase 1 +
// wallet-funding-plan.md): generated -> funding -> funded -> reserved -> used
// -> retired. `funding` = a treasury->wallet SOL send is in flight.
export type WalletStatus =
  | 'generated'
  | 'funding'
  | 'funded'
  | 'reserved'
  | 'used'
  | 'retired';

// Per-wallet result of a funding pass (POST /api/wallet_pool/fund) — mirrors the
// backend `launcher::WalletFundOutcome`.
export interface WalletFundOutcome {
  wallet_id: string;
  address: string;
  role: string;
  amount_lamports: number;
  result: 'sent' | 'dry_run' | 'failed' | 'skipped_cap' | 'skipped_bad_address';
  signature: string | null;
  error: string | null;
}

export interface FundReport {
  spent_lamports: number;
  outcomes: WalletFundOutcome[];
}

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

// Authored token-metadata content, pinned to IPFS via Pinata — the resulting
// `uri` is the same shape a launch template's `params.uri` / a per-launch
// override consumes.
export interface MetadataTemplate {
  id: string;
  template_name: string;
  name: string;
  symbol: string;
  description: string | null;
  twitter: string | null;
  telegram: string | null;
  website: string | null;
  // null for a preset authored outside the pin flow (e.g. one backfilled from a
  // legacy launch template) — the image is embedded in the JSON at `uri`.
  image_uri: string | null;
  uri: string;
  created_at: string;
}

export interface NewMetadataTemplate {
  template_name: string;
  name: string;
  symbol: string;
  description?: string;
  twitter?: string;
  telegram?: string;
  website?: string;
  image_base64: string;
  image_filename: string;
  image_content_type: string;
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

// Per-launch overrides + "use N bundlers" — unset fields fall back to the
// template's own metadata_template_id / bundle_leg_count (wallet-pool Phase 3).
export interface LaunchOverrides {
  metadata_template_id?: string;
  bundler_count?: number;
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

export interface IngestStatus {
  // `false` when the box booted without Helius creds — nothing to toggle.
  configured: boolean;
  live: boolean;
}

// The launch console's initial load in one round trip (GET /api/bootstrap) —
// replaces three separate GETs on mount. `dev_wallets` is the pool filtered to
// the `dev` role server-side (all the console's wallet picker needs).
export interface BootstrapPayload {
  templates: LaunchTemplate[];
  dev_wallets: ManagedWalletPool[];
  metadata_templates: MetadataTemplate[];
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

async function putJson<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(path, {
    method: 'PUT',
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
  bootstrap: () => getJson<BootstrapPayload>('/api/bootstrap'),
  templates: () => getJson<LaunchTemplate[]>('/api/launch_templates'),
  createLaunchTemplate: (req: NewLaunchTemplateInput) =>
    postJson<LaunchTemplate>('/api/launch_templates', req),
  updateLaunchTemplate: (id: string, req: NewLaunchTemplateInput) =>
    putJson<LaunchTemplate>(`/api/launch_templates/${id}`, req),
  launchpads: () => getJson<Launchpad[]>('/api/launchpads'),
  quoteAssets: () => getJson<QuoteAsset[]>('/api/quote_assets'),
  executeLaunch: (template_id: string, dev_wallet_id: string, overrides?: LaunchOverrides) =>
    postJson<LaunchResult>('/api/launches/execute', { template_id, dev_wallet_id, ...overrides }),
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
  // On-demand treasury -> pool funding. Unset role/count = top every fundable
  // role (dev + bundler) up to its warm target. Requires FUND_ENABLED on the box.
  fundPool: (role?: string, count?: number) =>
    postJson<FundReport>('/api/wallet_pool/fund', {
      role: role || undefined,
      count: count ?? undefined,
    }),
  metadataTemplates: () => getJson<MetadataTemplate[]>('/api/metadata_templates'),
  createMetadataTemplate: (req: NewMetadataTemplate) =>
    postJson<MetadataTemplate>('/api/metadata_templates', req),
  ingestStatus: () => getJson<IngestStatus>('/api/ingest'),
  setIngest: (live: boolean) => putJson<IngestStatus>('/api/ingest', { live }),
};

// Reads a File as base64 (strips the `data:<mime>;base64,` prefix) for the
// metadata-template image field — kept a plain JSON POST rather than adding
// multipart handling for this one low-volume admin form.
export function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = reader.result as string;
      resolve(result.slice(result.indexOf(',') + 1));
    };
    reader.onerror = () => reject(reader.error ?? new Error('file read failed'));
    reader.readAsDataURL(file);
  });
}

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
