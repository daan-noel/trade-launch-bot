//! Launcher runtime settings (Helius RPC/sender, nonce accounts, keystore path).

use anyhow::{bail, Context, Result};
use std::path::PathBuf;

/// Env-backed settings for the launch executor.
#[derive(Debug, Clone)]
pub struct LauncherSettings {
    pub rpc_url: String,
    pub sender_urls: Vec<String>,
    pub nonce_accounts: Vec<String>,
    /// Directory containing envelope-encrypted wallet blobs (`key_ref` is relative).
    pub keystore_dir: PathBuf,
    /// Passphrase for [`super::keystore::EnvKek`] (wraps ed25519 secrets at rest).
    pub kek_passphrase: String,
    /// Jito block-engine JSON-RPC base (defaults to mainnet).
    pub jito_block_engine_url: String,
    /// Wallet-pool backup root (wallet-pool Phase 4) — `None` disables the
    /// post-generation backup entirely; there's no safe default location to
    /// assume, so this stays opt-in rather than required.
    pub backup_dir: Option<PathBuf>,
    /// Pinata JWT for pinning token-metadata images/JSON to IPFS (see
    /// `metadata_upload`) — `None` disables metadata-template authoring with a
    /// clear error rather than a required-at-boot var; nothing else needs it.
    pub pinata_jwt: Option<String>,
    /// Automated treasury→pool funding (docs/wallet-funding-plan.md) — `None`
    /// (the default: `FUND_ENABLED` unset/false) disables the background funder
    /// AND the manual `POST /api/wallet_pool/fund` endpoint. The kill switch.
    pub funding: Option<FundingConfig>,
    /// Shared secret gating `POST /api/wallet_pool/{id}/export` (raw private-key
    /// export). `None` (the default: `WALLET_EXPORT_SECRET` unset) hard-disables
    /// the endpoint — it returns 403. Opt-in per deployment; the endpoint hands
    /// out spendable keys, so it is off unless a secret is set. Serve over TLS.
    pub export_secret: Option<String>,
    /// Post-launch token management (token-management-plan.md) — `None` (the
    /// default: `MANAGE_ENABLED` unset/false) hard-disables the destructive
    /// `POST /api/tokens/{mint}/manage/execute` endpoint (503). The kill switch:
    /// previewing a plan and reading holdings are always allowed; firing real
    /// sells/buys is not, unless explicitly enabled. Mirrors `funding`.
    pub manage: Option<ManageConfig>,
}

/// Config for executing post-launch management actions (real sells/buys). Only
/// constructed when `MANAGE_ENABLED=true`.
#[derive(Debug, Clone)]
pub struct ManageConfig {
    /// Slippage floor (bps) applied to each management sell — protects proceeds
    /// against a thin curve. Default 10% (managed tokens are often low-liquidity).
    pub sell_slippage_bps: u64,
    /// Log intended actions and place NO real trades. Test before live.
    pub dry_run: bool,
}

impl ManageConfig {
    /// `None` unless `MANAGE_ENABLED=true`.
    pub fn from_env() -> Option<Self> {
        if !env_flag("MANAGE_ENABLED", false) {
            return None;
        }
        Some(Self {
            sell_slippage_bps: env_u64("MANAGE_SELL_SLIPPAGE_BPS", 1_000),
            dry_run: env_flag("MANAGE_DRY_RUN", false),
        })
    }
}

/// Safety-railed config for autonomous real-SOL funding (docs/wallet-funding-plan.md
/// P3). Every field is a guard against draining the treasury; all overridable via
/// env, with conservative defaults. Only constructed when `FUND_ENABLED=true`.
#[derive(Debug, Clone)]
pub struct FundingConfig {
    /// Never spend the treasury below this floor (lamports).
    pub treasury_reserve_lamports: u64,
    /// Hard stop mid-batch once this much has been sent in one funding pass (lamports).
    pub max_spend_per_interval_lamports: u64,
    /// Per-wallet target amount by role (jittered at send time).
    pub amount_dev_lamports: u64,
    pub amount_bundler_lamports: u64,
    /// Amount jitter fraction: each transfer is `amount * (1 ± jitter)`.
    pub amount_jitter_pct: f64,
    /// Max random inter-send delay (ms) — timing de-correlation.
    pub max_delay_ms: u64,
    /// Keep at least this many `funded` wallets warm per role (top-up target).
    pub target_funded_dev: i64,
    pub target_funded_bundler: i64,
    /// Log intended transfers and send NOTHING (revert claims). Test before live.
    pub dry_run: bool,
}

impl FundingConfig {
    /// `None` unless `FUND_ENABLED=true`. Reads every `FUND_*` var with a
    /// conservative default so a partial config can't silently over-spend.
    pub fn from_env() -> Option<Self> {
        if !env_flag("FUND_ENABLED", false) {
            return None;
        }
        Some(Self {
            treasury_reserve_lamports: env_u64("FUND_TREASURY_RESERVE_LAMPORTS", 50_000_000),
            max_spend_per_interval_lamports: env_u64(
                "FUND_MAX_SPEND_PER_INTERVAL_LAMPORTS",
                1_000_000_000,
            ),
            amount_dev_lamports: env_u64("FUND_AMOUNT_DEV_LAMPORTS", 50_000_000),
            amount_bundler_lamports: env_u64("FUND_AMOUNT_BUNDLER_LAMPORTS", 30_000_000),
            amount_jitter_pct: env_f64("FUND_AMOUNT_JITTER_PCT", 0.15),
            max_delay_ms: env_u64("FUND_MAX_DELAY_MS", 8_000),
            target_funded_dev: env_u64("FUND_TARGET_FUNDED_DEV", 2) as i64,
            target_funded_bundler: env_u64("FUND_TARGET_FUNDED_BUNDLER", 5) as i64,
            dry_run: env_flag("FUND_DRY_RUN", false),
        })
    }
}

fn env_flag(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(v) => matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"),
        Err(_) => default,
    }
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key).ok().and_then(|v| v.trim().parse().ok()).unwrap_or(default)
}

fn env_f64(key: &str, default: f64) -> f64 {
    std::env::var(key).ok().and_then(|v| v.trim().parse().ok()).unwrap_or(default)
}

impl LauncherSettings {
    pub fn from_env() -> Result<Self> {
        let rpc_url = std::env::var("HELIUS_RPC_URL")
            .or_else(|_| std::env::var("RPC_URL"))
            .context("HELIUS_RPC_URL (or RPC_URL) required for launcher")?;
        let sender_urls = sender_urls_from_env()?;
        if sender_urls.is_empty() {
            bail!("at least one sender URL required (HELIUS_FAST_SENDER_URL or HELIUS_SENDER_URLS)");
        }
        let nonce_raw = std::env::var("NONCE_ACCOUNTS")
            .context("NONCE_ACCOUNTS required for launcher (comma-separated pubkeys)")?;
        let nonce_accounts: Vec<String> = nonce_raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        if nonce_accounts.is_empty() {
            bail!("NONCE_ACCOUNTS parsed empty");
        }
        let keystore_dir = std::env::var("WALLET_KEYSTORE")
            .map(PathBuf::from)
            .context("WALLET_KEYSTORE required (directory of envelope-encrypted wallet blobs)")?;
        let kek_passphrase = std::env::var("LAUNCHER_KEK_PASSPHRASE")
            .or_else(|_| std::env::var("WALLET_KEK_PASSPHRASE"))
            .context("LAUNCHER_KEK_PASSPHRASE (or WALLET_KEK_PASSPHRASE) required")?;
        let jito_block_engine_url = std::env::var("JITO_BLOCK_ENGINE_URL").unwrap_or_else(|_| {
            "https://mainnet.block-engine.jito.wtf/api/v1/bundles".to_string()
        });
        let backup_dir = std::env::var("WALLET_BACKUP_DIR").ok().map(PathBuf::from);
        let pinata_jwt = std::env::var("PINATA_JWT").ok().filter(|s| !s.is_empty());
        let funding = FundingConfig::from_env();
        let export_secret = std::env::var("WALLET_EXPORT_SECRET")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let manage = ManageConfig::from_env();
        Ok(Self {
            rpc_url,
            sender_urls,
            nonce_accounts,
            keystore_dir,
            kek_passphrase,
            jito_block_engine_url,
            backup_dir,
            pinata_jwt,
            funding,
            export_secret,
            manage,
        })
    }
}

fn sender_urls_from_env() -> Result<Vec<String>> {
    if let Ok(list) = std::env::var("HELIUS_SENDER_URLS") {
        let urls: Vec<String> = list
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        if !urls.is_empty() {
            return Ok(urls);
        }
    }
    if let Ok(one) = std::env::var("HELIUS_FAST_SENDER_URL") {
        if !one.is_empty() {
            return Ok(vec![one]);
        }
    }
    Ok(Vec::new())
}
