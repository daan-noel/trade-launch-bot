//! Operator-triggered wallet sweeps (Wallet Pool page buttons) — two on-demand
//! reclaim passes that share the plain-transfer SSOT and a token-account closer:
//!
//!   1. [`sweep_used_and_retired`] ("Sweep & retire") — runs the same `used`-wallet
//!      dust sweep the hourly task does (skipping open-position holders that still
//!      need gas to sell), THEN reclaims already-`retired` wallets: sweeps any
//!      residual SOL back to the treasury and closes their **empty** SPL token
//!      accounts to reclaim the rent (non-empty accounts are reported, never
//!      force-closed — an SPL account can only be closed at a zero balance).
//!
//!   2. [`consolidate_all`] ("Consolidate → treasury") — drains SOL AND closes
//!      empty token accounts on EVERY managed wallet (all roles/statuses, incl.
//!      other treasuries) into one operator-chosen treasury wallet. Skips
//!      `funding`/`reserved` wallets — draining a mid-launch wallet would break the
//!      in-flight launch.
//!
//! The close-account tx uses the destination treasury as the **fee payer** (the
//! wallet owner only co-signs to authorize the close), so even a 0-SOL retired
//! wallet's rent can be reclaimed; the reclaimed rent lands in the treasury.

use std::str::FromStr;

use anyhow::{bail, Context, Result};
use platform_core::models::{ManagedWallet, WalletRole, WalletStatus};
use platform_core::storage::repositories::ManagedWalletRepo;
use pump_trader::protocol::{TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID};
use serde::Serialize;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::message::Message;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;
use solana_sdk::transaction::Transaction;
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

use crate::config::LauncherSettings;
use crate::dust_sweep::{sweep_used_wallets, SWEEP_MIN_LAMPORTS};
use crate::keystore::{self, EnvKek};
use crate::plan_exec::{execute_transfer, TransferMode};
use crate::wallet_pool::MIN_FUNDED_LAMPORTS;

/// SPL `CloseAccount` instruction discriminator (variant 9, no payload). Identical
/// for SPL-Token and Token-2022, so only the program id differs per account.
const CLOSE_ACCOUNT_IX: u8 = 9;
/// Empty token accounts closed per tx. Each close adds ~1 account key + a tiny ix;
/// kept well under the 1232-byte tx limit with room for the two signers.
const CLOSE_BATCH: usize = 12;

/// Outcome of sweeping/draining ONE wallet. `retired` = the wallet is `retired`
/// after this pass (the `used` dust sweep retires; the retired-reclaim path leaves
/// it retired; consolidate never retires). `note` carries a skip reason or error.
#[derive(Debug, Clone, Serialize)]
pub struct WalletSweepOutcome {
    pub wallet_id: Uuid,
    pub address: String,
    pub role: String,
    pub status: String,
    pub sol_swept_lamports: u64,
    pub rent_reclaimed_lamports: u64,
    pub accounts_closed: usize,
    pub accounts_nonempty_skipped: usize,
    pub retired: bool,
    pub signature: Option<String>,
    pub note: Option<String>,
}

impl WalletSweepOutcome {
    pub(crate) fn base(w: &ManagedWallet) -> Self {
        Self {
            wallet_id: w.id,
            address: w.address.clone(),
            role: w.role.clone(),
            status: w.status.clone(),
            sol_swept_lamports: 0,
            rent_reclaimed_lamports: 0,
            accounts_closed: 0,
            accounts_nonempty_skipped: 0,
            retired: false,
            signature: None,
            note: None,
        }
    }

    pub(crate) fn skipped(w: &ManagedWallet, reason: &str) -> Self {
        let mut o = Self::base(w);
        o.note = Some(reason.to_string());
        o
    }

    pub(crate) fn errored(w: &ManagedWallet, e: anyhow::Error) -> Self {
        let mut o = Self::base(w);
        o.note = Some(format!("{e:#}"));
        o
    }

    /// A `used` wallet retired without a sweep (balance at/below the dust floor).
    pub(crate) fn retired_no_sweep(w: &ManagedWallet, residual_lamports: u64) -> Self {
        let mut o = Self::base(w);
        o.retired = true;
        if residual_lamports > 0 {
            o.note = Some("below dust floor — retired, sub-fee residual left".to_string());
        }
        o
    }

    /// A `used` wallet swept to the treasury and retired.
    pub(crate) fn swept(
        w: &ManagedWallet,
        lamports: u64,
        signature: String,
        retired: bool,
    ) -> Self {
        let mut o = Self::base(w);
        o.sol_swept_lamports = lamports;
        o.signature = Some(signature);
        o.retired = retired;
        o
    }
}

/// Aggregate roll-up returned to the operator button. Totals fold the per-wallet
/// outcomes so the UI can show a one-line summary without re-summing.
#[derive(Debug, Serialize)]
pub struct SweepReport {
    pub treasury_id: Uuid,
    pub treasury_address: String,
    pub total_sol_swept_lamports: u64,
    pub total_rent_reclaimed_lamports: u64,
    pub wallets_retired: usize,
    pub accounts_closed: usize,
    pub outcomes: Vec<WalletSweepOutcome>,
}

impl SweepReport {
    fn new(treasury: &ManagedWallet, outcomes: Vec<WalletSweepOutcome>) -> Self {
        Self {
            treasury_id: treasury.id,
            treasury_address: treasury.address.clone(),
            total_sol_swept_lamports: outcomes.iter().map(|o| o.sol_swept_lamports).sum(),
            total_rent_reclaimed_lamports: outcomes.iter().map(|o| o.rent_reclaimed_lamports).sum(),
            wallets_retired: outcomes.iter().filter(|o| o.retired).count(),
            accounts_closed: outcomes.iter().map(|o| o.accounts_closed).sum(),
            outcomes,
        }
    }
}

/// Button 1: sweep `used` wallets (dust-sweep semantics — skips open-position
/// holders), reclaim already-`retired` wallets (residual SOL + close empty
/// token accounts), AND reclaim stranded `generated` dust (below the funded
/// floor, so it was never going to auto-promote — see the `generated` path
/// below). Everything lands in the first (oldest) non-retired treasury.
pub async fn sweep_used_and_retired(
    pool: &PgPool,
    settings: &LauncherSettings,
) -> Result<SweepReport> {
    let treasury = ManagedWalletRepo::by_role(pool, WalletRole::Treasury.as_str())
        .await?
        .into_iter()
        .next()
        .context("no treasury wallet configured (role=treasury) — nothing to sweep into")?;
    let treasury_addr =
        Pubkey::from_str(&treasury.address).context("parse treasury wallet address")?;

    let rpc = RpcClient::new_with_commitment(settings.rpc_url.clone(), CommitmentConfig::confirmed());
    let kek = EnvKek::from_passphrase(&settings.kek_passphrase);
    let http = reqwest::Client::new();
    let treasury_signer = keystore::resolve_signer(&settings.keystore_dir, &treasury.key_ref, &kek)?;

    // Snapshot the already-`retired` set BEFORE the used sweep runs — the used sweep
    // retires the wallets it drains, and reprocessing those here would double-count
    // them (and add a no-op residual sweep). Their empty ATAs get closed on the next
    // press; this pass reclaims the wallets that were retired going in.
    let retired =
        ManagedWalletRepo::find_by_status(pool, WalletStatus::Retired.as_str(), None).await?;

    // `generated` wallets below the funded floor: a `generated` wallet only
    // promotes to `funded` once its cached balance clears `MIN_FUNDED_LAMPORTS`
    // (wallet_pool.rs's balance poller), so anything stuck under that floor —
    // stray/partial deposit, or dust left behind after `record_balance` demotes a
    // drained `funded` wallet back to `generated` — is stranded forever otherwise.
    // Wallets at/above the floor are left alone: they're mid-promotion, not
    // stranded, and this button must never race the funding poller.
    let generated_dust = ManagedWalletRepo::find_by_status(pool, WalletStatus::Generated.as_str(), None)
        .await?
        .into_iter()
        .filter(|w| w.balance_lamports.unwrap_or(0) < MIN_FUNDED_LAMPORTS)
        .collect::<Vec<_>>();

    // `used` path — the exact hourly dust sweep (open-position holders held back).
    let mut outcomes =
        sweep_used_wallets(&rpc, pool, settings, &kek, treasury.id, treasury_addr).await?;

    // `retired` + `generated`-dust paths — reclaim residual SOL + close empty ATAs
    // on wallets that were never re-promoted by this sweep. (Neither set is ever
    // the picked treasury, which is non-retired and typically well-funded.)
    for wallet in retired.into_iter().chain(generated_dust) {
        let outcome = match drain_to_treasury(
            &rpc,
            &http,
            &settings.rpc_url,
            pool,
            settings,
            &kek,
            &wallet,
            treasury_signer.as_ref(),
            treasury_addr,
        )
        .await
        {
            Ok(o) => o,
            Err(e) => {
                warn!(managed_wallet_id = %wallet.id, %e, "stranded-wallet reclaim failed");
                WalletSweepOutcome::errored(&wallet, e)
            }
        };
        outcomes.push(outcome);
    }

    Ok(SweepReport::new(&treasury, outcomes))
}

/// Button 2: drain SOL + close empty token accounts on EVERY managed wallet into
/// `dest_treasury_id` (which must be a `treasury`-role wallet). Skips the
/// destination itself and any `funding`/`reserved` wallet (mid-launch — draining
/// it would break the in-flight launch). Does not retire — a pure fund reclaim.
pub async fn consolidate_all(
    pool: &PgPool,
    settings: &LauncherSettings,
    dest_treasury_id: Uuid,
) -> Result<SweepReport> {
    let dest = ManagedWalletRepo::get(pool, dest_treasury_id)
        .await?
        .context("destination wallet not found")?;
    if dest.role != WalletRole::Treasury.as_str() {
        bail!("destination must be a treasury-role wallet (got `{}`)", dest.role);
    }
    let dest_addr = Pubkey::from_str(&dest.address).context("parse destination treasury address")?;

    let rpc = RpcClient::new_with_commitment(settings.rpc_url.clone(), CommitmentConfig::confirmed());
    let kek = EnvKek::from_passphrase(&settings.kek_passphrase);
    let http = reqwest::Client::new();
    let dest_signer = keystore::resolve_signer(&settings.keystore_dir, &dest.key_ref, &kek)?;

    let all = ManagedWalletRepo::list_all(pool, None).await?;
    let mut outcomes = Vec::with_capacity(all.len());
    for wallet in all {
        if wallet.id == dest.id {
            continue;
        }
        // Never drain a mid-launch wallet: its SOL is committed to an in-flight
        // launch (funding send in transit, or reserved for a bundle leg).
        if matches!(
            WalletStatus::from_str(&wallet.status),
            Ok(WalletStatus::Funding | WalletStatus::Reserved)
        ) {
            outcomes.push(WalletSweepOutcome::skipped(
                &wallet,
                "mid-launch (funding/reserved) — skipped to avoid breaking an in-flight launch",
            ));
            continue;
        }

        let outcome = match drain_to_treasury(
            &rpc,
            &http,
            &settings.rpc_url,
            pool,
            settings,
            &kek,
            &wallet,
            dest_signer.as_ref(),
            dest_addr,
        )
        .await
        {
            Ok(o) => o,
            Err(e) => {
                warn!(managed_wallet_id = %wallet.id, %e, "consolidate drain failed");
                WalletSweepOutcome::errored(&wallet, e)
            }
        };
        outcomes.push(outcome);
    }

    Ok(SweepReport::new(&dest, outcomes))
}

/// Close a wallet's empty token accounts (rent → `dest`, paid for by `dest`) then
/// sweep its residual SOL to `dest`. Shared by the retired-reclaim and consolidate
/// paths. Never retires; leaves the wallet's lifecycle status untouched (only the
/// cached balance is refreshed). `dest` is the fee payer + rent destination.
#[allow(clippy::too_many_arguments)]
async fn drain_to_treasury(
    rpc: &RpcClient,
    http: &reqwest::Client,
    rpc_url: &str,
    pool: &PgPool,
    settings: &LauncherSettings,
    kek: &EnvKek,
    wallet: &ManagedWallet,
    dest_signer: &(dyn Signer + Send + Sync),
    dest: Pubkey,
) -> Result<WalletSweepOutcome> {
    let mut o = WalletSweepOutcome::base(wallet);
    let owner = Pubkey::from_str(&wallet.address).context("parse wallet address")?;
    let owner_signer = keystore::resolve_signer(&settings.keystore_dir, &wallet.key_ref, kek)?;

    // 1) Close empty token accounts, reclaiming their rent into the treasury.
    let close = close_empty_token_accounts(
        rpc,
        http,
        rpc_url,
        dest_signer,
        owner_signer.as_ref(),
        owner,
        dest,
    )
    .await
    .context("close empty token accounts")?;
    o.rent_reclaimed_lamports = close.rent_reclaimed_lamports;
    o.accounts_closed = close.accounts_closed;
    o.accounts_nonempty_skipped = close.nonempty_skipped;

    // 2) Sweep residual SOL to the treasury (source signs + pays its own fee). A
    // 0-balance / sub-floor wallet just no-ops here (`None`).
    let swept = execute_transfer(
        rpc,
        owner_signer.as_ref(),
        owner,
        dest,
        TransferMode::SweepAll { min_lamports: SWEEP_MIN_LAMPORTS },
        true,
    )
    .await
    .context("sweep residual SOL")?;
    if let Some((sig, lamports)) = swept {
        o.sol_swept_lamports = lamports;
        o.signature = Some(sig.to_string());
    }

    // Refresh the cached balance so the pool table reflects the drain at once.
    // `record_balance` also demotes `funded` → `generated` when the drain took the
    // balance back below the funded floor (so a drained wallet can't be handed to
    // the next launch as if it still had gas); `used`/`reserved`/`retired` are left
    // alone — it never un-retires a wallet. Best-effort — the drain already succeeded.
    match rpc.get_balance(&owner).await {
        Ok(bal) => {
            if let Err(e) =
                ManagedWalletRepo::record_balance(pool, wallet.id, bal as i64, MIN_FUNDED_LAMPORTS)
                    .await
            {
                warn!(?e, managed_wallet_id = %wallet.id, "sweep: cached-balance refresh failed");
            }
        }
        Err(e) => warn!(?e, managed_wallet_id = %wallet.id, "sweep: post-drain balance read failed"),
    }

    o.retired = wallet.status == WalletStatus::Retired.as_str();
    if o.accounts_closed > 0 || o.sol_swept_lamports > 0 {
        info!(
            managed_wallet_id = %wallet.id, address = %wallet.address,
            closed = o.accounts_closed, rent = o.rent_reclaimed_lamports, sol = o.sol_swept_lamports,
            "wallet drained to treasury"
        );
    }
    Ok(o)
}

struct CloseSummary {
    accounts_closed: usize,
    rent_reclaimed_lamports: u64,
    nonempty_skipped: usize,
}

/// Enumerate `owner`'s SPL token accounts and close the EMPTY ones (zero token
/// balance), reclaiming each account's rent lamports to `rent_dest`. `fee_payer`
/// pays the tx fee and `owner` co-signs to authorize the close, so a 0-SOL wallet's
/// accounts can still be closed. Non-empty accounts are counted, never closed.
async fn close_empty_token_accounts(
    rpc: &RpcClient,
    http: &reqwest::Client,
    rpc_url: &str,
    fee_payer: &(dyn Signer + Send + Sync),
    owner: &(dyn Signer + Send + Sync),
    owner_pubkey: Pubkey,
    rent_dest: Pubkey,
) -> Result<CloseSummary> {
    let accounts = list_token_accounts(http, rpc_url, &owner_pubkey.to_string()).await?;
    let (empty, nonempty): (Vec<_>, Vec<_>) = accounts.into_iter().partition(|a| a.amount == 0);

    let mut summary = CloseSummary {
        accounts_closed: 0,
        rent_reclaimed_lamports: 0,
        nonempty_skipped: nonempty.len(),
    };
    if empty.is_empty() {
        return Ok(summary);
    }

    for chunk in empty.chunks(CLOSE_BATCH) {
        let ixs: Vec<Instruction> = chunk
            .iter()
            .map(|acc| Instruction {
                program_id: acc.program_id,
                accounts: vec![
                    AccountMeta::new(acc.pubkey, false),
                    AccountMeta::new(rent_dest, false),
                    AccountMeta::new_readonly(owner_pubkey, true),
                ],
                data: vec![CLOSE_ACCOUNT_IX],
            })
            .collect();

        let blockhash = rpc.get_latest_blockhash().await.context("fetch blockhash")?;
        let msg = Message::new_with_blockhash(&ixs, Some(&fee_payer.pubkey()), &blockhash);
        let mut tx = Transaction::new_unsigned(msg);
        tx.try_sign(&[fee_payer as &dyn Signer, owner as &dyn Signer], blockhash)
            .context("sign close-account tx")?;
        rpc.send_and_confirm_transaction(&tx).await.context("send close-account tx")?;

        summary.accounts_closed += chunk.len();
        summary.rent_reclaimed_lamports += chunk.iter().map(|a| a.lamports).sum::<u64>();
    }
    Ok(summary)
}

struct TokenAccountInfo {
    pubkey: Pubkey,
    program_id: Pubkey,
    amount: u64,
    lamports: u64,
}

/// List every SPL token account owned by `owner`, across both the SPL-Token and
/// Token-2022 programs (a `getTokenAccountsByOwner` call per program — the raw
/// JSON-RPC path already used by `manage::positions`). Returns zero-balance
/// accounts too (they're the close-and-reclaim targets).
async fn list_token_accounts(
    http: &reqwest::Client,
    rpc_url: &str,
    owner: &str,
) -> Result<Vec<TokenAccountInfo>> {
    let mut out = Vec::new();
    for program in [TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID] {
        let program_id = Pubkey::from_str(program).context("parse token program id")?;
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTokenAccountsByOwner",
            "params": [owner, { "programId": program }, { "encoding": "jsonParsed", "commitment": "confirmed" }],
        });
        let v: serde_json::Value = http
            .post(rpc_url)
            .json(&body)
            .send()
            .await
            .context("getTokenAccountsByOwner HTTP")?
            .error_for_status()
            .context("getTokenAccountsByOwner HTTP status")?
            .json()
            .await
            .context("parse getTokenAccountsByOwner body")?;
        if let Some(err) = v.get("error") {
            bail!("getTokenAccountsByOwner RPC error: {err}");
        }
        let accounts = v
            .get("result")
            .and_then(|r| r.get("value"))
            .and_then(|value| value.as_array())
            .context("getTokenAccountsByOwner response missing result.value")?;
        for acc in accounts {
            let Some(pubkey) = acc.get("pubkey").and_then(|p| p.as_str()) else {
                continue;
            };
            let pubkey = Pubkey::from_str(pubkey).context("parse token account pubkey")?;
            let amount = acc
                .pointer("/account/data/parsed/info/tokenAmount/amount")
                .and_then(|a| a.as_str())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let lamports = acc
                .pointer("/account/lamports")
                .and_then(|l| l.as_u64())
                .unwrap_or(0);
            out.push(TokenAccountInfo { pubkey, program_id, amount, lamports });
        }
    }
    Ok(out)
}
