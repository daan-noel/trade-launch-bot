//! Launch-specific pump-trader tuning — longer confirm window + dual send paths.

use pump_trader::TraderConfig;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

use crate::config::LauncherSettings;

/// Build a [`TraderConfig`] tuned for launch workloads.
///
/// - Fans out to both Helius Sender **and** the RPC URL so a fast-sender drop
///   still has a standard `sendTransaction` landing path.
/// - Uses a longer confirmation poll than the snipe-buy defaults (~3s) — create
///   txs on mainnet routinely need 10–30s during congestion.
/// - Bumps CU price for create landing.
pub fn build_launch_trader_config(
    settings: &LauncherSettings,
    signer: Arc<dyn solana_sdk::signature::Signer + Send + Sync>,
    nonce_accounts: Vec<Pubkey>,
) -> Arc<TraderConfig> {
    let mut sender_urls = settings.sender_urls.clone();
    if !sender_urls.iter().any(|u| u == &settings.rpc_url) {
        sender_urls.push(settings.rpc_url.clone());
    }

    let mut config = TraderConfig::new(
        settings.rpc_url.clone(),
        sender_urls,
        signer,
        nonce_accounts,
    );
    // Launch ALT (when provisioned): the trader fetches it at init and compiles the
    // create + dev-buy as a v0 tx so create_v2 + dev-buy fits the 1232 B limit.
    config.launch_alt_address = settings.launch_alt;
    // Recent-blockhash trades, NOT durable nonce. Forge signs every buy/sell with a
    // different ephemeral managed wallet (dev/bundler/treasury), none of which is
    // the shared nonce pool's on-chain authority — a durable-nonce tx they can't
    // advance is silently dropped by the leader (never lands → confirm-timeout).
    // The create + dev-buy path already rides a recent blockhash; this makes the
    // post-launch manage buy/sell path do the same. (Durable nonce is a hunter
    // hot-path optimization for its single persistent signer.)
    config.durable_nonce = false;
    // Create-only v2 measures ~900–1100 B; give mainnet headroom to land.
    config.retry.confirm_max_retries = 40;
    config.retry.confirm_poll_ms = 1_500;
    config.retry.confirm_poll_schedule_ms = vec![500, 750, 1_000, 1_500, 2_000];
    config.compute.price_micro_lamports = 750_000;
    // Launch-tuned Jito tip bounds (env-overridable). The ceiling is higher than the
    // trade-path default so a contested launch's whole-bundle tip can climb far
    // enough to win the auction; a tip that never lands costs nothing.
    config.jito.min_sol = settings.jito_min_tip_sol;
    config.jito.max_sol = settings.jito_max_tip_sol;
    config.jito.percentile = settings.jito_tip_percentile;
    Arc::new(config)
}
