//! Operator wallet-to-wallet SOL transfer (docs/wallet-transfer-plan.md): move SOL
//! between two managed wallets from the dashboard — no Phantom, no raw private
//! keys. Primary driver: a managed wallet at 0 SOL can't pay the fee for its own
//! buy/sell; top it up in two clicks.
//!
//! No new signing / tx-assembly here: this routes through the ONE audited plain
//! SOL-transfer SSOT ([`crate::plan_exec::execute_transfer`]) that funding and the
//! dust sweep also use. It's the dust sweep's shape with an operator-chosen source,
//! destination, and amount — and *without* retiring the source.

use std::str::FromStr;

use anyhow::{bail, Context, Result};
use platform_core::models::{WalletRole, WalletStatus};
use platform_core::storage::repositories::ManagedWalletRepo;
use platform_core::units::sol_to_lamports;
use serde::Serialize;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use sqlx::PgPool;
use tracing::warn;
use uuid::Uuid;

use crate::config::LauncherSettings;
use crate::dust_sweep::SWEEP_MIN_LAMPORTS;
use crate::keystore::{self, EnvKek};
use crate::plan_exec::{execute_transfer, TransferMode};
use crate::wallet_pool::MIN_FUNDED_LAMPORTS;

/// How much of the source to move.
#[derive(Debug, Clone, Copy)]
pub enum TransferAmount {
    /// Move exactly this many whole SOL (source pays the fee on top).
    Exact(f64),
    /// Sweep the source to ~0 (send `balance − fee`), reusing the dust-sweep floor.
    Max,
}

/// Outcome of one operator transfer. `signature`/`lamports_sent` are `None`/`0`
/// only when a `Max` sweep found nothing worth sending.
#[derive(Debug, Serialize)]
pub struct TransferReport {
    pub from_id: Uuid,
    pub to_id: Uuid,
    pub from_address: String,
    pub to_address: String,
    pub lamports_sent: u64,
    pub signature: Option<String>,
}

/// Move SOL from one managed wallet to another. The **source** signs and pays the
/// fee. Rejects a source that's mid-launch (`funding`/`reserved`) or `retired`;
/// `funded`/`used`/`treasury`/`generated` are valid sources (a 0-balance source
/// just no-ops on `Max` or errors on `Exact` — insufficient funds). On success,
/// both wallets' cached balances are refreshed so the pool table updates at once
/// (this also auto-promotes an empty `generated` destination to `funded` once the
/// top-up clears `MIN_FUNDED_LAMPORTS`).
pub async fn transfer_between_wallets(
    pool: &PgPool,
    settings: &LauncherSettings,
    from_id: Uuid,
    to_id: Uuid,
    amount: TransferAmount,
) -> Result<TransferReport> {
    if from_id == to_id {
        bail!("source and destination wallets must differ");
    }

    let from = ManagedWalletRepo::get(pool, from_id)
        .await?
        .context("source wallet not found")?;
    let to = ManagedWalletRepo::get(pool, to_id)
        .await?
        .context("destination wallet not found")?;

    // Status guard on the source. A `funding`/`reserved` wallet has a launch
    // in-flight against it; `retired` is shredded/backed out — none is a safe
    // ad-hoc source.
    match WalletStatus::from_str(&from.status).map_err(|e| anyhow::anyhow!(e))? {
        WalletStatus::Funding | WalletStatus::Reserved | WalletStatus::Retired => bail!(
            "source wallet is `{}` — not a valid transfer source (a launch is mid-flight, or it's retired)",
            from.status
        ),
        WalletStatus::Generated | WalletStatus::Funded | WalletStatus::Used => {}
    }

    let from_addr = Pubkey::from_str(&from.address).context("parse source wallet address")?;
    let to_addr = Pubkey::from_str(&to.address).context("parse destination wallet address")?;

    let mode = match amount {
        TransferAmount::Exact(sol) => {
            let lamports = sol_to_lamports(sol);
            if lamports <= 0 {
                bail!("transfer amount must be greater than 0 SOL");
            }
            TransferMode::Exact(lamports as u64)
        }
        TransferAmount::Max => TransferMode::SweepAll { min_lamports: SWEEP_MIN_LAMPORTS },
    };

    let rpc =
        RpcClient::new_with_commitment(settings.rpc_url.clone(), CommitmentConfig::confirmed());
    let kek = EnvKek::from_passphrase(&settings.kek_passphrase);
    let signer = keystore::resolve_signer(&settings.keystore_dir, &from.key_ref, &kek)?;

    // Concurrency: a treasury on EITHER side of this move could race a funding pass
    // that snapshots treasury balance for its reserve/cap math — so serialize on the
    // same lock those passes hold. Non-treasury ↔ non-treasury moves are independent
    // and skip it.
    let treasury = WalletRole::Treasury.as_str();
    let touches_treasury = from.role == treasury || to.role == treasury;
    let _guard = if touches_treasury {
        Some(crate::wallet_funding::FUNDING_LOCK.lock().await)
    } else {
        None
    };

    let sent = execute_transfer(&rpc, signer.as_ref(), from_addr, to_addr, mode, true).await?;
    let (signature, lamports_sent) = match sent {
        Some((sig, lamports)) => (Some(sig.to_string()), lamports),
        None => (None, 0),
    };

    // Refresh both cached balances so the pool table reflects the move immediately
    // (and promotes an empty destination). Best-effort: the transfer already
    // succeeded, so a balance re-read failure only leaves a stale number until the
    // next poll — never fail the request over it.
    if signature.is_some() {
        // One `getMultipleAccounts` for both wallets instead of two `getBalance`.
        let addrs = [from_addr, to_addr];
        match rpc.get_multiple_accounts(&addrs).await {
            Ok(accounts) => {
                for (id, acct) in [from.id, to.id].into_iter().zip(accounts) {
                    // A missing (e.g. fully-swept) account reads as 0 lamports.
                    let bal = acct.map(|a| a.lamports).unwrap_or(0) as i64;
                    if let Err(e) =
                        ManagedWalletRepo::record_balance(pool, id, bal, MIN_FUNDED_LAMPORTS).await
                    {
                        warn!(?e, managed_wallet_id = %id, "wallet transfer: cached-balance refresh failed");
                    }
                }
            }
            Err(e) => {
                warn!(?e, "wallet transfer: post-send balance read failed");
            }
        }
    }

    Ok(TransferReport {
        from_id: from.id,
        to_id: to.id,
        from_address: from.address,
        to_address: to.address,
        lamports_sent,
        signature,
    })
}
