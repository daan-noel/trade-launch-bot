//! Launch-specific pump-trader tuning — longer confirm window + dual send paths.

use pump_trader::TraderConfig;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

use crate::config::{LauncherSettings, ManageConfig};

/// Build a [`TraderConfig`] tuned for launch workloads.
///
/// - Fans out to both Helius Sender **and** the RPC URL so a fast-sender drop
///   still has a standard `sendTransaction` landing path.
/// - Uses a longer confirmation poll than the snipe-buy defaults (~3s) — create
///   txs on mainnet routinely need 10–30s during congestion.
/// - Sets a modest CU price for create landing. The launch *bundle* is ordered by
///   the Jito **tip**, not by priority fee, so this fee only has to cover the
///   validator's per-CU charge — not out-bid a slot. (Was 750k; the create leg's
///   fee at that rate cost ~0.00026 SOL for no inclusion benefit.)
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
    config.compute.price_micro_lamports = 100_000;
    // Launch-tuned Jito tip bounds (env-overridable). The ceiling bounds the
    // whole-bundle create-leg tip; a tip that never lands costs nothing.
    config.jito.min_sol = settings.jito_min_tip_sol;
    config.jito.max_sol = settings.jito_max_tip_sol;
    config.jito.percentile = settings.jito_tip_percentile;
    Arc::new(config)
}

/// Build a [`TraderConfig`] tuned for the **manage** path (operator-timed buys /
/// sells / re-gas), decoupled from the launch fee/tip budget.
///
/// Manage actions are cold and manual — the operator chooses when to fire, so there
/// is no slot race to win. That lets this path be far cheaper than launch:
///
/// - **Plain RPC only** (no Helius Sender fan-out). The Sender path charges its own
///   tip semantics (min 0.001 SOL to enter the priority buffer, credited only to a
///   *Sender* tip account) and buys latency we don't need. A normal `sendTransaction`
///   lands a non-contested manage trade fine.
/// - **Zeroed Jito tip** by default ([`ManageConfig::jito_min_tip_sol`]/`max` = 0).
///   On plain RPC a Jito tip is a no-op transfer to a tip account no bundle reads —
///   pure waste. The always-appended `jito_tip_ix` becomes a 0-lamport no-op.
/// - **Low CU price** ([`ManageConfig::cu_price_micro_lamports`], default 50k) — the
///   only landing lever that matters on plain RPC, sized well below the launch fee.
///
/// Shares the launch path's recent-blockhash (non-durable-nonce) + confirm tuning:
/// the manage wallets aren't the nonce pool's authority, and a manual sell wants the
/// same generous confirm window a create does.
pub fn build_manage_trader_config(
    settings: &LauncherSettings,
    manage: &ManageConfig,
    signer: Arc<dyn solana_sdk::signature::Signer + Send + Sync>,
    nonce_accounts: Vec<Pubkey>,
) -> Arc<TraderConfig> {
    // Plain RPC only — latency isn't a race here, and the Sender path's tip would be
    // wasted (see the doc comment). One landing path, no fan-out.
    let mut config = TraderConfig::new(
        settings.rpc_url.clone(),
        vec![settings.rpc_url.clone()],
        signer,
        nonce_accounts,
    );
    config.launch_alt_address = settings.launch_alt;
    config.durable_nonce = false;
    config.retry.confirm_max_retries = 40;
    config.retry.confirm_poll_ms = 1_500;
    config.retry.confirm_poll_schedule_ms = vec![500, 750, 1_000, 1_500, 2_000];
    config.compute.price_micro_lamports = manage.cu_price_micro_lamports;
    // Zero (default) → the appended tip ix is a no-op transfer; no tip paid on RPC.
    config.jito.min_sol = manage.jito_min_tip_sol;
    config.jito.max_sol = manage.jito_max_tip_sol;
    config.jito.percentile = settings.jito_tip_percentile;
    Arc::new(config)
}
