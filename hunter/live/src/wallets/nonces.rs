//! Nonce accounts for a registered wallet: create new ones, attach ones it
//! already owns, and bring the env wallet with its nonces into the registry.

use std::collections::HashSet;
use std::str::FromStr;

use anyhow::{bail, Context, Result};
use serde::Serialize;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::system_instruction;
use sqlx::PgPool;

use trading_core::storage::repositories::trader_wallet_repo::TraderWalletRepo;

use crate::trader::check_nonce_account;

use super::keystore::{key_ref_for, valid_name, write_keypair};
use super::mover::{require_loaded, send};
use super::runtime::WalletRuntime;

/// One `getMultipleAccounts` reads at most 100 accounts; a wallet never needs more.
const MAX_ATTACH: usize = 32;

#[derive(Debug, Default, Serialize)]
pub struct AttachReport {
    pub attached: usize,
    pub already: usize,
    pub rejected: Vec<Rejected>,
}

#[derive(Debug, Serialize)]
pub struct Rejected {
    pub pubkey: String,
    pub reason: String,
}

async fn require_registered(repo: &TraderWalletRepo, wallet: &str) -> Result<()> {
    match repo.records().await?.into_iter().find(|r| r.pubkey == wallet) {
        Some(r) if r.status == "active" => Ok(()),
        Some(_) => bail!("wallet is not active"),
        None => bail!("wallet is not in the registry; bring the env wallet in first"),
    }
}

pub async fn provision_nonces(
    pool: &PgPool,
    runtime: &WalletRuntime,
    wallet: &str,
    count: u8,
) -> Result<u8> {
    if !runtime.can_manage_keys() {
        bail!("wallet keystore is not configured");
    }
    if count == 0 || count > 16 {
        bail!("nonce count must be 1 to 16");
    }
    let repo = TraderWalletRepo::new(pool.clone());
    // Checked before any SOL is spent: a nonce created for an unregistered
    // wallet has no row to land in.
    require_registered(&repo, wallet).await?;
    require_loaded(runtime, wallet)?;
    let owner = runtime.signer(wallet).context("wallet key")?;
    let rpc = RpcClient::new(runtime.rpc_url().to_string());
    let rent = rpc.get_minimum_balance_for_rent_exemption(80).await?;
    let mut made = 0u8;
    let mut sigs = Vec::with_capacity(count as usize);
    let mut failed = None;
    for _ in 0..count {
        let nonce = Keypair::new();
        let ixs = system_instruction::create_nonce_account(
            &owner.pubkey(),
            &nonce.pubkey(),
            &owner.pubkey(),
            rent,
        );
        match send(runtime, &owner, ixs, &[&nonce]).await {
            Ok(sig) => sigs.push(sig),
            Err(e) => {
                failed = Some(e);
                break;
            }
        }
        repo.insert_nonce(&nonce.pubkey().to_string(), wallet).await?;
        made += 1;
    }
    // The ones that landed are recorded and running even when a later one failed.
    if made > 0 {
        let _ = repo
            .log_activity(wallet, "nonces_created", serde_json::json!({ "count": made }), &sigs)
            .await;
        sync_trader(runtime, &repo, wallet).await?;
    }
    match failed {
        Some(e) => Err(e.context(format!("created {made} of {count} nonce accounts"))),
        None => Ok(made),
    }
}

/// One registered nonce account and what the chain says about it.
#[derive(Debug, Serialize)]
pub struct NonceView {
    pub pubkey: String,
    /// The chain names this wallet as the authority.
    pub ok: bool,
    pub error: Option<String>,
}

/// The wallet's registered nonce accounts, checked in one `getMultipleAccounts`.
pub async fn list_nonces(pool: &PgPool, runtime: &WalletRuntime, wallet: &str) -> Result<Vec<NonceView>> {
    let repo = TraderWalletRepo::new(pool.clone());
    let wallet_pk = Pubkey::from_str(wallet).context("wallet pubkey")?;
    let rows = repo.nonces_for(wallet).await?;
    let pks = rows.iter().map(|n| Pubkey::from_str(&n.pubkey)).collect::<Result<Vec<_>, _>>()?;
    if pks.is_empty() {
        return Ok(Vec::new());
    }
    let rpc = RpcClient::new(runtime.rpc_url().to_string());
    let accounts = rpc.get_multiple_accounts(&pks).await.context("read nonce accounts")?;
    Ok(pks
        .iter()
        .zip(accounts.iter())
        .map(|(pk, account)| {
            let check = check_nonce_account(*pk, account.as_ref(), &wallet_pk);
            let error = match (&check.error, check.authority) {
                (Some(e), _) => Some(e.clone()),
                (None, Some(other)) if !check.matches_wallet => Some(format!("authority is {other}")),
                _ => None,
            };
            NonceView { pubkey: pk.to_string(), ok: check.matches_wallet, error }
        })
        .collect())
}

/// Record nonce accounts `wallet` already owns on chain. One `getMultipleAccounts`
/// checks every authority; an account owned by another key is rejected, never stored.
pub async fn attach_nonces(
    pool: &PgPool,
    runtime: &WalletRuntime,
    wallet: &str,
    nonces: &[String],
) -> Result<AttachReport> {
    let repo = TraderWalletRepo::new(pool.clone());
    require_registered(&repo, wallet).await?;
    let wallet_pk = Pubkey::from_str(wallet).context("wallet pubkey")?;
    let mut report = AttachReport::default();
    let mut seen = HashSet::new();
    let mut pks = Vec::new();
    for raw in nonces {
        let raw = raw.trim();
        if raw.is_empty() || !seen.insert(raw.to_string()) {
            continue;
        }
        match Pubkey::from_str(raw) {
            Ok(pk) => pks.push(pk),
            Err(_) => report.rejected.push(Rejected { pubkey: raw.to_string(), reason: "not a pubkey".into() }),
        }
    }
    if pks.len() > MAX_ATTACH {
        bail!("at most {MAX_ATTACH} nonce accounts at once");
    }
    if pks.is_empty() {
        return Ok(report);
    }
    let rpc = RpcClient::new(runtime.rpc_url().to_string());
    let accounts = rpc.get_multiple_accounts(&pks).await.context("read nonce accounts")?;
    let known: HashSet<String> = repo.nonces_for(wallet).await?.into_iter().map(|n| n.pubkey).collect();
    for (pk, account) in pks.iter().zip(accounts.iter()) {
        let check = check_nonce_account(*pk, account.as_ref(), &wallet_pk);
        if !check.matches_wallet {
            let reason = match (check.error, check.authority) {
                (Some(e), _) => e,
                (None, Some(other)) => format!("authority is {other}"),
                (None, None) => "not a nonce account".into(),
            };
            report.rejected.push(Rejected { pubkey: pk.to_string(), reason });
        } else if known.contains(&pk.to_string()) {
            report.already += 1;
        } else {
            repo.insert_nonce(&pk.to_string(), wallet).await?;
            report.attached += 1;
        }
    }
    if report.attached > 0 {
        let detail = serde_json::json!({ "attached": report.attached, "rejected": report.rejected.len() });
        let _ = repo.log_activity(wallet, "nonces_attached", detail, &[]).await;
        sync_trader(runtime, &repo, wallet).await?;
    }
    Ok(report)
}

/// Write the env key to `name.enc`, register it, and attach `NONCE_ACCOUNTS`.
/// A row for the env pubkey that already exists is kept; only its nonces are
/// attached. The running trader is rebuilt only when its nonce set changes.
pub async fn adopt_env(pool: &PgPool, runtime: &WalletRuntime, name: &str) -> Result<AttachReport> {
    let env = runtime.env_wallet().context("WALLET_PRIVATE_KEY is not set")?;
    let repo = TraderWalletRepo::new(pool.clone());
    let records = repo.records().await?;
    match records.iter().find(|r| r.pubkey == env.pubkey) {
        Some(r) if r.status != "active" => bail!("the env wallet is {} in the registry", r.status),
        Some(_) => {}
        None => {
            if !valid_name(name) {
                bail!("name must be lowercase letters, digits, and dashes");
            }
            if records.iter().any(|r| r.name == name) {
                bail!("a wallet named {name} already exists");
            }
            let dir = runtime.keystore_dir().context("WALLET_KEYSTORE is not set")?;
            let kek = runtime.kek().context("WALLET_KEK is not set")?;
            require_loaded(runtime, &env.pubkey)?;
            let signer = runtime.signer(&env.pubkey).context("env key")?;
            let key_ref = key_ref_for(name);
            write_keypair(dir, &key_ref, &signer.to_bytes(), kek)?;
            let primary = runtime.primary_pubkey() == env.pubkey;
            repo.insert(&env.pubkey, name, &key_ref, primary).await?;
            runtime.rename(&env.pubkey, name);
            let _ = repo.log_activity(&env.pubkey, "registered_env", serde_json::json!({ "name": name }), &[]).await;
        }
    }
    let nonces: Vec<String> = env.nonces.iter().map(|n| n.to_string()).collect();
    attach_nonces(pool, runtime, &env.pubkey, &nonces).await
}

/// Rebuild `wallet`'s trader on the registry's nonce set, unless it already runs on it.
async fn sync_trader(runtime: &WalletRuntime, repo: &TraderWalletRepo, wallet: &str) -> Result<()> {
    let nonces = repo
        .nonces_for(wallet)
        .await?
        .into_iter()
        .map(|n| n.pubkey.parse())
        .collect::<Result<Vec<Pubkey>, _>>()?;
    let running: HashSet<Pubkey> = runtime.nonces(wallet).unwrap_or_default().into_iter().collect();
    if running == nonces.iter().copied().collect::<HashSet<_>>() && runtime.init_error(wallet).is_none() {
        return Ok(());
    }
    runtime.rebuild(wallet, nonces).await
}
