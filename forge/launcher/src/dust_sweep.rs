//! Dust sweep (wallet-pool Phase 4): scan `used` wallets, sweep any balance
//! above a floor back to a `treasury` wallet, retire the swept wallet.
//!
//! Deliberately a plain SOL transfer via `solana-client`'s nonblocking RPC —
//! not routed through pump-trader's Jito/multi-sender/retry machinery, since a
//! dust sweep has no landing urgency (unlike a snipe buy).

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

use crate::config::LauncherSettings;
use crate::keystore::{self, EnvKek};

/// Only sweep wallets holding more than this — not worth a signed tx + fee for dust.
/// Also the floor a `Max` operator transfer reuses (see `wallet_transfer`).
pub(crate) const SWEEP_MIN_LAMPORTS: u64 = 100_000; // 0.0001 SOL
const SWEEP_INTERVAL: Duration = Duration::from_secs(3600);

/// Spawn the dust sweep as a long-lived task. Cheap when idle — bounded to the
/// `used` set (typically small; every entrant here is already terminal).
pub fn spawn_dust_sweep(pool: PgPool, settings: LauncherSettings) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(SWEEP_INTERVAL);
        loop {
            tick.tick().await;
            if let Err(e) = sweep_once(&pool, &settings).await {
                warn!(?e, "dust sweep pass failed");
            }
        }
    })
}

async fn sweep_once(pool: &PgPool, settings: &LauncherSettings) -> Result<()> {
    let Some(treasury) = ManagedWalletRepo::by_role(pool, WalletRole::Treasury.as_str())
        .await?
        .into_iter()
        .next()
    else {
        warn!("dust sweep: no treasury wallet configured (role=treasury) — skipping");
        return Ok(());
    };
    let treasury_address =
        Pubkey::from_str(&treasury.address).context("parse treasury wallet address")?;

    let used = ManagedWalletRepo::find_by_status(pool, WalletStatus::Used.as_str(), None).await?;
    if used.is_empty() {
        return Ok(());
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
    if sweepable.is_empty() {
        return Ok(());
    }

    let rpc = RpcClient::new_with_commitment(settings.rpc_url.clone(), CommitmentConfig::confirmed());
    let kek = EnvKek::from_passphrase(&settings.kek_passphrase);

    for wallet in sweepable {
        let id = wallet.id;
        if let Err(e) = sweep_wallet(&rpc, pool, settings, &kek, &wallet, treasury_address).await {
            warn!(managed_wallet_id = %id, %e, "dust sweep failed for wallet");
        }
    }
    Ok(())
}

async fn sweep_wallet(
    rpc: &RpcClient,
    pool: &PgPool,
    settings: &LauncherSettings,
    kek: &EnvKek,
    wallet: &ManagedWallet,
    treasury: Pubkey,
) -> Result<()> {
    let address = Pubkey::from_str(&wallet.address).context("parse wallet address")?;
    let balance = rpc.get_balance(&address).await.context("fetch wallet balance")?;

    if balance <= SWEEP_MIN_LAMPORTS {
        // Not worth a signed tx + fee — retire directly, dust left behind. Stamp the
        // real (sub-floor) residual so the row reflects what's actually on-chain.
        ManagedWalletRepo::retire(pool, wallet.id, balance as i64).await?;
        info!(managed_wallet_id = %wallet.id, balance, "wallet below dust floor — retired without sweep");
        return Ok(());
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
        Some((sig, _lamports)) => info!(
            managed_wallet_id = %wallet.id, address = %wallet.address, sig = %sig,
            "dust swept to treasury, wallet retired"
        ),
        None => info!(
            managed_wallet_id = %wallet.id, balance,
            "wallet balance below fee at send — retired without sweep"
        ),
    }
    Ok(())
}
