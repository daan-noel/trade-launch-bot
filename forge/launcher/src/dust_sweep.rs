//! Dust sweep (wallet-pool Phase 4): scan `used` wallets, sweep any balance
//! above a floor back to a `treasury` wallet, retire the swept wallet.
//!
//! Deliberately a plain SOL transfer via `solana-client`'s nonblocking RPC —
//! not routed through pump-trader's Jito/multi-sender/retry machinery, since a
//! dust sweep has no landing urgency (unlike a snipe buy).
//!
//! The per-wallet pass ([`sweep_used_wallets`]) is shared: the hourly background
//! task drives it here, and the operator "Sweep & retire" button drives it (plus
//! the retired-wallet reclaim) via [`crate::wallet_sweep`].

use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Result};
use platform_core::models::{ManagedWallet, WalletRole, WalletStatus};
use platform_core::storage::repositories::{ManagedWalletRepo, TokenPositionRepo};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

use crate::config::LauncherSettings;
use crate::keystore::{self, EnvKek};
use crate::wallet_sweep::WalletSweepOutcome;

/// Only sweep wallets holding more than this — not worth a signed tx + fee for dust.
/// Also the floor a `Max` operator transfer reuses (see `wallet_transfer`).
pub(crate) const SWEEP_MIN_LAMPORTS: u64 = 100_000; // 0.0001 SOL
/// Hourly dust-sweep cadence — now gated inside the unified wallet-lifecycle tick
/// (`wallet_lifecycle.rs`) rather than its own task.
pub(crate) const DUST_SWEEP_INTERVAL: Duration = Duration::from_secs(3600);

/// One hourly pass: resolve the treasury, then sweep every eligible `used` wallet.
/// Driven by the unified wallet-lifecycle tick. Logs a one-line summary itself so
/// the orchestrator just fires it on cadence.
pub(crate) async fn run_dust_sweep_pass(pool: &PgPool, settings: &LauncherSettings) {
    match sweep_once(pool, settings).await {
        Ok(outcomes) => {
            let swept = outcomes.iter().filter(|o| o.retired).count();
            if swept > 0 {
                info!(swept, "dust sweep pass complete");
            }
        }
        Err(e) => warn!(?e, "dust sweep pass failed"),
    }
}

/// One hourly pass: resolve the treasury, then sweep every eligible `used` wallet.
async fn sweep_once(pool: &PgPool, settings: &LauncherSettings) -> Result<Vec<WalletSweepOutcome>> {
    let Some(treasury) = ManagedWalletRepo::by_role(pool, WalletRole::Treasury.as_str())
        .await?
        .into_iter()
        .next()
    else {
        warn!("dust sweep: no treasury wallet configured (role=treasury) — skipping");
        return Ok(Vec::new());
    };
    let treasury_address =
        Pubkey::from_str(&treasury.address).context("parse treasury wallet address")?;

    let rpc = RpcClient::new_with_commitment(settings.rpc_url.clone(), CommitmentConfig::confirmed());
    let kek = EnvKek::from_passphrase(&settings.kek_passphrase);
    sweep_used_wallets(&rpc, pool, settings, &kek, treasury.id, treasury_address).await
}

/// Sweep every `used` wallet's balance to `treasury`, retiring each. Skips wallets
/// with an OPEN token position (their SOL is the gas they need to sell — draining
/// them here strands the tokens). Returns one [`WalletSweepOutcome`] per wallet
/// touched (including the held-back ones, so the operator sees why they were kept).
///
/// Shared by the hourly [`spawn_dust_sweep`] task and the operator sweep button.
pub(crate) async fn sweep_used_wallets(
    rpc: &RpcClient,
    pool: &PgPool,
    settings: &LauncherSettings,
    kek: &EnvKek,
    treasury_id: Uuid,
    treasury_address: Pubkey,
) -> Result<Vec<WalletSweepOutcome>> {
    let used = ManagedWalletRepo::find_by_status(pool, WalletStatus::Used.as_str(), None).await?;
    if used.is_empty() {
        return Ok(Vec::new());
    }

    // Never sweep a wallet that still holds an OPEN token position: its SOL is the
    // gas it needs to sell that position later. Draining + retiring it here strands
    // the tokens — the sell can't pay its fee, so the leader drops the tx and the
    // manage action fails with "confirmation timed out". Such a wallet stays `used`
    // and is re-checked next pass; once the position is sold (`closed`) or was never
    // bought (`dropped`), it becomes sweep-eligible again.
    let holding = TokenPositionRepo::managed_wallet_ids_with_open_positions(pool).await?;
    let (sweepable, held): (Vec<_>, Vec<_>) =
        used.into_iter().partition(|w| !holding.contains(&w.id));
    if !held.is_empty() {
        info!(
            kept = held.len(),
            "dust sweep: keeping wallets with open token positions (need gas to sell) — not swept"
        );
    }

    let mut outcomes = Vec::with_capacity(sweepable.len() + held.len());
    for wallet in &held {
        // Never a treasury-drain candidate: surfaced so the operator understands the
        // count. `sol_swept`/`retired` stay zero/false.
        outcomes.push(WalletSweepOutcome::skipped(
            wallet,
            "holds an open token position — keeping gas to sell it",
        ));
    }

    for wallet in sweepable {
        let outcome =
            match sweep_wallet(rpc, pool, settings, kek, &wallet, treasury_id, treasury_address)
                .await
            {
                Ok(o) => o,
                Err(e) => {
                    warn!(managed_wallet_id = %wallet.id, %e, "dust sweep failed for wallet");
                    WalletSweepOutcome::errored(&wallet, e)
                }
            };
        outcomes.push(outcome);
    }
    Ok(outcomes)
}

/// Sweep one `used` wallet to the treasury and retire it. `treasury_id` guards
/// against a treasury wallet somehow entering the `used` set — never sweep it to
/// itself (an empty ix set would fail the send anyway, but skip it cleanly).
async fn sweep_wallet(
    rpc: &RpcClient,
    pool: &PgPool,
    settings: &LauncherSettings,
    kek: &EnvKek,
    wallet: &ManagedWallet,
    treasury_id: Uuid,
    treasury: Pubkey,
) -> Result<WalletSweepOutcome> {
    if wallet.id == treasury_id {
        return Ok(WalletSweepOutcome::skipped(wallet, "is the destination treasury"));
    }
    let address = Pubkey::from_str(&wallet.address).context("parse wallet address")?;
    // Reuse a fresh cached balance when one exists (audit Phase C2) — usually a
    // `used` wallet isn't poller-refreshed, so this falls through to a live read;
    // it only skips the RPC right after an operator "Refresh balances" stamped it.
    // The actual sweep re-reads on-chain (SweepAll probe), so this read is only the
    // above-floor pre-check.
    let balance = match crate::wallet_pool::fresh_cached_balance(wallet) {
        Some(b) => b,
        None => rpc.get_balance(&address).await.context("fetch wallet balance")?,
    };

    if balance <= SWEEP_MIN_LAMPORTS {
        // Not worth a signed tx + fee — retire directly, dust left behind. Stamp the
        // real (sub-floor) residual so the row reflects what's actually on-chain.
        ManagedWalletRepo::retire(pool, wallet.id, balance as i64).await?;
        info!(managed_wallet_id = %wallet.id, balance, "wallet below dust floor — retired without sweep");
        return Ok(WalletSweepOutcome::retired_no_sweep(wallet, balance));
    }

    // Phase 2.F: the sweep is a typed `TransferSol` move executed through the SSOT
    // [`crate::plan_exec::execute_transfer`] (the ONE place a plain SOL transfer is
    // assembled — probe-fee then `balance − fee` so the source lands at exactly 0).
    // Still a plain transfer (no Jito): a dust sweep has no landing urgency.
    let signer = keystore::resolve_signer(&settings.keystore_dir, &wallet.key_ref, kek)?;
    let swept = crate::plan_exec::execute_transfer(
        rpc,
        signer.as_ref(),
        address,
        treasury,
        crate::plan_exec::TransferMode::SweepAll { min_lamports: SWEEP_MIN_LAMPORTS },
        true, // confirm
    )
    .await?;

    // Retire regardless: a `None` means the balance fell to/below the tx fee since
    // we read it — nothing worth a signed tx, same terminal outcome as a sweep.
    // The `SweepAll` transfer lands the source at 0 (any `None`/post-fee residual is
    // sub-fee dust), so stamp 0 — the wallet holds nothing after this.
    ManagedWalletRepo::retire(pool, wallet.id, 0).await?;
    match swept {
        Some((sig, lamports)) => {
            info!(
                managed_wallet_id = %wallet.id, address = %wallet.address, sig = %sig,
                "dust swept to treasury, wallet retired"
            );
            Ok(WalletSweepOutcome::swept(wallet, lamports, sig.to_string(), true))
        }
        None => {
            info!(
                managed_wallet_id = %wallet.id, balance,
                "wallet balance below fee at send — retired without sweep"
            );
            Ok(WalletSweepOutcome::retired_no_sweep(wallet, 0))
        }
    }
}
