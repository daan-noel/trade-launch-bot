//! Live-only trading credentials + fee/tip tuning.
//!
//! The trader wallet key, nonce accounts, and Helius **Sender** endpoints are
//! required to move real SOL, so **only** the live bin loads them — lab has no
//! reason to hold trading secrets (and now can't accidentally require them). The
//! shared DB / server / Helius-endpoint config lives in
//! [`trading_core::config::Settings`], loaded by both bins. This split mirrors
//! forge's `Settings` (shared) + `LauncherSettings` (live-only).

use anyhow::Context;

/// Secrets and endpoints only the live trading path needs. Every field here is
/// **required** — a live bin with no wallet key or no Sender endpoint can't
/// trade, so a missing value is a hard boot failure, not a silent default.
#[derive(Debug, Clone)]
pub struct TradingSecrets {
    /// base58 trader wallet secret key. Never logged.
    pub wallet_private_key: String,
    /// Durable-nonce account pubkeys (parsed by the caller).
    pub nonce_accounts: Vec<String>,
    /// One or more Helius Sender endpoints. The signed tx is fanned out to all of
    /// them concurrently (same signature → on-chain dedup, tip paid once), so a
    /// slow/down endpoint can't gate the send. A single entry behaves exactly
    /// like the legacy single-endpoint path.
    pub helius_sender_urls: Vec<String>,
}

impl TradingSecrets {
    /// Load the live-only trading credentials from the environment. Call
    /// `dotenvy::dotenv()` (or load [`trading_core::config::Settings`]) first.
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            wallet_private_key: required("WALLET_PRIVATE_KEY")?,
            nonce_accounts: parse_required_list("NONCE_ACCOUNTS")?,
            helius_sender_urls: sender_urls()?,
        })
    }
}

/// Optional live fee/tip knobs applied onto `TraderConfig` at boot. Defaults match
/// `executor_core::config::{ComputeBudgetCfg,JitoTipCfg}` — raise tip for
/// contested slots, lower only when deliberately trading the SWQoS-only path.
#[derive(Debug, Clone)]
pub struct TraderFeeTuning {
    /// Helius Sender tip floor (SOL). Default `0.001` = Sender Max priority buffer.
    pub jito_min_tip_sol: f64,
    /// Hard per-trade tip ceiling (SOL).
    pub jito_max_tip_sol: f64,
    /// Landed-tip percentile for level-0 (25|50|75|95|99).
    pub jito_tip_percentile: u8,
    /// Compute-unit price (micro-lamports) — the priority-fee rate.
    pub cu_price_micro_lamports: u64,
}

impl TraderFeeTuning {
    pub fn from_env() -> anyhow::Result<Self> {
        let jito_min_tip_sol = env_f64("JITO_MIN_TIP_SOL", 0.001)?;
        let jito_max_tip_sol = env_f64("JITO_MAX_TIP_SOL", 0.005)?;
        if jito_min_tip_sol < 0.0 || jito_max_tip_sol < 0.0 {
            anyhow::bail!("JITO_MIN_TIP_SOL / JITO_MAX_TIP_SOL must be >= 0");
        }
        if jito_max_tip_sol < jito_min_tip_sol {
            anyhow::bail!(
                "JITO_MAX_TIP_SOL ({jito_max_tip_sol}) must be >= JITO_MIN_TIP_SOL ({jito_min_tip_sol})"
            );
        }
        let jito_tip_percentile = env_u64("JITO_TIP_PERCENTILE", 75)? as u8;
        if !matches!(jito_tip_percentile, 25 | 50 | 75 | 95 | 99) {
            anyhow::bail!(
                "JITO_TIP_PERCENTILE must be one of 25|50|75|95|99, got {jito_tip_percentile}"
            );
        }
        let cu_price_micro_lamports = env_u64("CU_PRICE_MICRO_LAMPORTS", 200_000)?;
        if cu_price_micro_lamports == 0 {
            anyhow::bail!(
                "CU_PRICE_MICRO_LAMPORTS must be > 0 (Helius Sender requires a priority fee)"
            );
        }
        Ok(Self {
            jito_min_tip_sol,
            jito_max_tip_sol,
            jito_tip_percentile,
            cu_price_micro_lamports,
        })
    }
}

/// Helius Sender endpoints, newest-form first. Prefer the plural
/// `HELIUS_FAST_SENDER_URLS` (comma-separated, fanned out concurrently); fall
/// back to the legacy singular `HELIUS_FAST_SENDER_URL` so existing single-
/// endpoint deployments keep working unchanged. At least one is required.
fn sender_urls() -> anyhow::Result<Vec<String>> {
    if let Ok(list) = std::env::var("HELIUS_FAST_SENDER_URLS") {
        let items: Vec<String> = list
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !items.is_empty() {
            return Ok(items);
        }
    }
    Ok(vec![required("HELIUS_FAST_SENDER_URL").context(
        "at least one Helius Sender endpoint (HELIUS_FAST_SENDER_URLS or HELIUS_FAST_SENDER_URL) is required",
    )?])
}

fn parse_required_list(key: &str) -> anyhow::Result<Vec<String>> {
    let value = required(key)?;
    let items = value
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();

    if items.is_empty() {
        anyhow::bail!("{key} must contain at least one value");
    }

    Ok(items)
}

fn required(key: &str) -> anyhow::Result<String> {
    std::env::var(key).map_err(|_| anyhow::anyhow!("Missing required env var: {key}"))
}

fn env_f64(key: &str, default: f64) -> anyhow::Result<f64> {
    match std::env::var(key) {
        Ok(val) => val
            .parse::<f64>()
            .map_err(|e| anyhow::anyhow!("Invalid value for {key}={val:?}: {e}")),
        Err(_) => Ok(default),
    }
}

fn env_u64(key: &str, default: u64) -> anyhow::Result<u64> {
    match std::env::var(key) {
        Ok(val) => val
            .parse::<u64>()
            .map_err(|e| anyhow::anyhow!("Invalid value for {key}={val:?}: {e}")),
        Err(_) => Ok(default),
    }
}
