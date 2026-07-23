//! Live-only trading credentials.
//!
//! The trader wallet key, nonce accounts, and Helius **Sender** endpoints are
//! required to move real SOL, so **only** the live bin loads them — lab has no
//! reason to hold trading secrets (and now can't accidentally require them). The
//! shared DB / server / Helius-endpoint config lives in
//! [`trading_core::config::Settings`], loaded by both bins. Tip / CU fee knobs
//! are also shared ([`trading_core::config::FeeTuning`]) so lab's CostModel
//! prices the same floor the live trader bids. This split mirrors forge's
//! `Settings` (shared) + `LauncherSettings` (live-only).

use anyhow::Context;

/// Re-export the shared tip/CU knobs under the historical live name so existing
/// call sites (`TraderFeeTuning::from_env`) keep compiling. Prefer
/// [`trading_core::config::FeeTuning`] in new code.
pub use trading_core::config::FeeTuning as TraderFeeTuning;

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
