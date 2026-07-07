//! Dust sweep (wallet-pool Phase 4): scan `used` wallets, sweep any balance
//! above a floor back to a `treasury` wallet, retire the swept wallet.
//!
//! Deliberately a plain SOL transfer via `solana-client`'s nonblocking RPC —
//! not routed through pump-trader's Jito/multi-sender/retry machinery, since a
//! dust sweep has no landing urgency (unlike a snipe buy).

use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Result};
use platform_core::models::ManagedWallet;
use platform_core::storage::repositories::ManagedWalletRepo;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::message::Message;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;
use solana_sdk::system_instruction;
use solana_sdk::transaction::Transaction;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::config::LauncherSettings;
use crate::keystore::{self, EnvKek};

/// Only sweep wallets holding more than this — not worth a signed tx + fee for dust.
const SWEEP_MIN_LAMPORTS: u64 = 100_000; // 0.0001 SOL
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
    let Some(treasury) = ManagedWalletRepo::by_role(pool, "treasury")
        .await?
        .into_iter()
        .next()
    else {
        warn!("dust sweep: no treasury wallet configured (role=treasury) — skipping");
        return Ok(());
    };
    let treasury_address =
        Pubkey::from_str(&treasury.address).context("parse treasury wallet address")?;

    let used = ManagedWalletRepo::find_by_status(pool, "used", None).await?;
    if used.is_empty() {
        return Ok(());
    }

    let rpc = RpcClient::new_with_commitment(settings.rpc_url.clone(), CommitmentConfig::confirmed());
    let kek = EnvKek::from_passphrase(&settings.kek_passphrase);

    for wallet in used {
        let id = wallet.id;
        if let Err(e) = sweep_wallet(&rpc, pool, settings, &kek, &wallet, treasury_address).await {
            warn!(wallet_id = %id, %e, "dust sweep failed for wallet");
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
        // Not worth a signed tx + fee — retire directly, dust left behind.
        ManagedWalletRepo::retire(pool, wallet.id).await?;
        info!(wallet_id = %wallet.id, balance, "wallet below dust floor — retired without sweep");
        return Ok(());
    }

    let blockhash = rpc.get_latest_blockhash().await.context("fetch blockhash")?;

    // The transfer amount is a fixed-width u64 in the instruction data, so a
    // probe message (any amount) has the exact same serialized size — and
    // therefore the exact same fee — as the final one. Query it first so we
    // can send the true remainder and land the source account at exactly 0
    // lamports: leaving any non-zero balance below the rent-exempt minimum
    // makes the runtime reject the transfer outright.
    let probe_ix = system_instruction::transfer(&address, &treasury, balance);
    let probe_msg = Message::new_with_blockhash(&[probe_ix], Some(&address), &blockhash);
    let fee = rpc
        .get_fee_for_message(&probe_msg)
        .await
        .context("fetch dust-sweep transfer fee")?;

    if balance <= fee {
        ManagedWalletRepo::retire(pool, wallet.id).await?;
        info!(wallet_id = %wallet.id, balance, fee, "wallet balance below fee — retired without sweep");
        return Ok(());
    }
    let send_lamports = balance - fee;

    let signer = keystore::resolve_signer(&settings.keystore_dir, &wallet.key_ref, kek)?;
    let ix = system_instruction::transfer(&address, &treasury, send_lamports);
    let msg = Message::new_with_blockhash(&[ix], Some(&address), &blockhash);
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[signer.as_ref() as &dyn Signer], blockhash)
        .context("sign dust-sweep transfer")?;

    let sig = rpc
        .send_and_confirm_transaction(&tx)
        .await
        .context("send dust-sweep transfer")?;

    ManagedWalletRepo::retire(pool, wallet.id).await?;
    info!(
        wallet_id = %wallet.id,
        address = %wallet.address,
        swept_lamports = send_lamports,
        sig = %sig,
        "dust swept to treasury, wallet retired"
    );
    Ok(())
}
