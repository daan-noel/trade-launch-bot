//! Nonce accounts for a registered wallet: create new ones, attach ones it
//! already owns, find them on chain, and register the env key with its nonces.

use std::collections::HashSet;
use std::str::FromStr;

use anyhow::{bail, Context, Result};
use serde::Serialize;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_request::RpcRequest;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::system_instruction;
use sqlx::PgPool;

use trading_core::storage::repositories::trader_wallet_repo::TraderWalletRepo;

use crate::trader::check_nonce_account;

use super::keystore::{self, key_ref_for, write_keypair, Kek};
use super::mover::{require_loaded, send};
use super::ops::free_name;
use super::runtime::{keystore_env, WalletRuntime};

/// One `getMultipleAccounts` reads at most 100 accounts; a wallet never needs more.
const MAX_ATTACH: usize = 32;
const MAX_READ: usize = 100;

/// Transactions per history page read for nonce discovery.
const HISTORY_PAGE: usize = 100;

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

/// Write the env key into the keystore and the registry, and attach the
/// `NONCE_ACCOUNTS` the chain confirms it owns. Runs once per boot while
/// `WALLET_PRIVATE_KEY` is set; a second run changes nothing. A row that
/// already exists keeps its name and status; it becomes primary only when no
/// active row is, as a new one does. An archived one stays archived. The process never signs with the env key directly.
pub async fn register_env_key(pool: &PgPool, rpc_url: &str, signer: &Keypair, nonces: &[Pubkey]) -> Result<()> {
    let (dir, kek) = keystore_env()?;
    let repo = TraderWalletRepo::new(pool.clone());
    let pubkey = signer.pubkey().to_string();
    let records = repo.records().await?;
    let name = match records.iter().find(|r| r.pubkey == pubkey) {
        Some(r) if r.status == "archived" => {
            tracing::warn!(wallet = %pubkey, "WALLET_PRIVATE_KEY is an archived wallet; it stays archived");
            return Ok(());
        }
        Some(r) => {
            // A row synced from the other box can arrive before its file.
            ensure_key_file(&dir, &r.key_ref, signer, &kek)?;
            // The env key signed as the primary; it stays primary when no row is.
            if !records.iter().any(|r| r.is_primary && r.status != "archived") && repo.set_primary(&pubkey).await? {
                let _ = repo.log_activity(&pubkey, "set_primary", serde_json::json!({}), &[]).await;
            }
            r.name.clone()
        }
        None => {
            let name = free_name(&records, "main");
            let key_ref = key_ref_for(&pubkey);
            ensure_key_file(&dir, &key_ref, signer, &kek)?;
            let primary = !records.iter().any(|r| r.is_primary && r.status != "archived");
            repo.insert(&pubkey, &name, &key_ref, primary).await?;
            let _ = repo.log_activity(&pubkey, "registered_env", serde_json::json!({ "name": name }), &[]).await;
            name
        }
    };
    if !nonces.is_empty() {
        let rpc = RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::confirmed());
        let registered = registered_nonces(&repo, &pubkey).await?;
        let (_, owned) = check_accounts(&rpc, &signer.pubkey(), nonces).await?;
        let new: Vec<&Pubkey> = owned.iter().filter(|pk| !registered.contains(pk)).collect();
        for pk in &new {
            repo.insert_nonce(&pk.to_string(), &pubkey).await?;
        }
        if !new.is_empty() {
            let _ = repo
                .log_activity(&pubkey, "nonces_attached", serde_json::json!({ "attached": new.len() }), &[])
                .await;
        }
    }
    tracing::warn!(
        wallet = %pubkey, name = %name,
        "WALLET_PRIVATE_KEY is registered in the keystore; remove WALLET_PRIVATE_KEY and NONCE_ACCOUNTS from .env"
    );
    Ok(())
}

/// The key file for `signer`, written only when it is absent. A file that
/// exists and holds another key, or does not decrypt, is never replaced.
fn ensure_key_file(dir: &std::path::Path, key_ref: &str, signer: &Keypair, kek: &Kek) -> Result<()> {
    match keystore::read_keypair(dir, key_ref, kek) {
        Ok(kp) if kp.pubkey() == signer.pubkey() => Ok(()),
        Ok(_) => bail!("keystore file {key_ref} holds another key"),
        Err(_) if !dir.join(key_ref).exists() => write_keypair(dir, key_ref, &signer.to_bytes(), kek),
        Err(e) => Err(e.context(format!("keystore file {key_ref} exists and does not decrypt"))),
    }
}

/// What the chain says about a wallet right now.
#[derive(Debug, Default, Serialize)]
pub struct SyncReport {
    pub sol_lamports: u64,
    /// Live nonce accounts the registry holds for the wallet after the sync.
    pub nonces: usize,
    pub attached: usize,
    pub retired: usize,
    /// Why the transaction history could not be read. The nonces already
    /// known were still checked.
    pub history_error: Option<String>,
}

/// The wallet's balance and the candidates the chain names it the authority of.
struct ChainState {
    lamports: u64,
    nonces: Vec<Pubkey>,
    history_error: Option<String>,
}

/// Read the wallet's balance and every nonce account it is the authority of,
/// store the result, and run its trader on that set. Import and the Wallets
/// page's Refresh land here. Costs two history pages and one
/// `getMultipleAccounts`, so it runs on a click, never on a timer.
pub async fn sync_wallet(pool: &PgPool, runtime: &WalletRuntime, wallet: &str) -> Result<SyncReport> {
    let repo = TraderWalletRepo::new(pool.clone());
    require_registered(&repo, wallet).await?;
    let wallet_pk = Pubkey::from_str(wallet).context("wallet pubkey")?;
    let registered = registered_nonces(&repo, wallet).await?;
    let chain = read_chain(runtime, &wallet_pk, &registered).await?;
    let report = store_chain(&repo, wallet, &registered, chain).await?;
    if let Some(trader) = runtime.trader(wallet) {
        trader.update_balance_lamports_cache(report.sol_lamports);
        sync_trader(runtime, &repo, wallet).await?;
    }
    Ok(report)
}

/// A wallet just inserted into the registry and not loaded yet: its chain
/// state is stored first, so its trader is built once, on its real nonce set.
pub async fn sync_new(pool: &PgPool, runtime: &WalletRuntime, wallet: &Pubkey) -> Result<(Vec<Pubkey>, SyncReport)> {
    let repo = TraderWalletRepo::new(pool.clone());
    let chain = read_chain(runtime, wallet, &[]).await?;
    let report = store_chain(&repo, &wallet.to_string(), &[], chain).await?;
    Ok((registered_nonces(&repo, &wallet.to_string()).await?, report))
}

async fn registered_nonces(repo: &TraderWalletRepo, wallet: &str) -> Result<Vec<Pubkey>> {
    let rows = repo.nonces_for(wallet).await?;
    Ok(rows.into_iter().map(|n| n.pubkey.parse()).collect::<Result<Vec<Pubkey>, _>>()?)
}

/// Attach every confirmed nonce the registry lacks and retire every
/// registered one the chain no longer confirms.
async fn store_chain(
    repo: &TraderWalletRepo,
    wallet: &str,
    registered: &[Pubkey],
    chain: ChainState,
) -> Result<SyncReport> {
    let mut report = SyncReport {
        sol_lamports: chain.lamports,
        nonces: chain.nonces.len(),
        history_error: chain.history_error,
        ..SyncReport::default()
    };
    for pk in chain.nonces.iter().filter(|pk| !registered.contains(pk)) {
        repo.insert_nonce(&pk.to_string(), wallet).await?;
        report.attached += 1;
    }
    for pk in registered.iter().filter(|pk| !chain.nonces.contains(pk)) {
        repo.retire_nonce(&pk.to_string()).await?;
        report.retired += 1;
    }
    if report.attached > 0 || report.retired > 0 {
        let detail = serde_json::json!({ "attached": report.attached, "retired": report.retired });
        let _ = repo.log_activity(wallet, "nonces_synced", detail, &[]).await;
    }
    Ok(report)
}

/// The known candidates plus the ones history names, checked with the wallet
/// itself in one `getMultipleAccounts`. Read at `confirmed`, so a nonce
/// created seconds ago is already visible and never retired.
async fn read_chain(runtime: &WalletRuntime, wallet: &Pubkey, known: &[Pubkey]) -> Result<ChainState> {
    let rpc = RpcClient::new_with_commitment(runtime.rpc_url().to_string(), CommitmentConfig::confirmed());
    let mut candidates: Vec<Pubkey> = Vec::new();
    let mut push = |pk: Pubkey| {
        if !candidates.contains(&pk) {
            candidates.push(pk);
        }
    };
    known.iter().copied().for_each(&mut push);
    let history_error = match history_nonces(&rpc, wallet).await {
        Ok(found) => {
            found.into_iter().for_each(&mut push);
            None
        }
        Err(e) => Some(format!("{e:#}")),
    };
    let (lamports, nonces) = check_accounts(&rpc, wallet, &candidates).await?;
    Ok(ChainState { lamports, nonces, history_error })
}

/// The wallet's balance and the candidates whose nonce authority is the
/// wallet, read in one `getMultipleAccounts` with the wallet itself.
async fn check_accounts(rpc: &RpcClient, wallet: &Pubkey, candidates: &[Pubkey]) -> Result<(u64, Vec<Pubkey>)> {
    let candidates = &candidates[..candidates.len().min(MAX_READ - 1)];
    let keys: Vec<Pubkey> = std::iter::once(*wallet).chain(candidates.iter().copied()).collect();
    let accounts = rpc.get_multiple_accounts(&keys).await.context("read wallet and nonce accounts")?;
    let lamports = accounts.first().and_then(|a| a.as_ref()).map_or(0, |a| a.lamports);
    let owned = candidates
        .iter()
        .zip(accounts.iter().skip(1))
        .filter(|(pk, account)| check_nonce_account(**pk, account.as_ref(), wallet).matches_wallet)
        .map(|(pk, _)| *pk)
        .collect();
    Ok((lamports, owned))
}

/// Nonce accounts named by the wallet's newest and oldest transactions. A
/// trading wallet advances every nonce it rotates through, so the newest page
/// carries the live set; the oldest page carries the ones used from its start.
/// Helius `getTransactionsForAddress`: `getProgramAccounts` cannot filter the
/// System Program by authority there. A name here is only a candidate.
async fn history_nonces(rpc: &RpcClient, wallet: &Pubkey) -> Result<Vec<Pubkey>> {
    let mut found = Vec::new();
    for order in ["desc", "asc"] {
        let params = serde_json::json!([wallet.to_string(), {
            "transactionDetails": "full",
            "encoding": "jsonParsed",
            "maxSupportedTransactionVersion": 0,
            "sortOrder": order,
            "limit": HISTORY_PAGE,
        }]);
        let page: serde_json::Value = rpc
            .send(RpcRequest::Custom { method: "getTransactionsForAddress" }, params)
            .await
            .context("getTransactionsForAddress")?;
        collect_nonce_accounts(&page, &mut found);
        // The whole history fit in the first page.
        if page.get("paginationToken").is_none_or(|t| t.is_null()) {
            break;
        }
    }
    Ok(found)
}

/// Every `nonceAccount` a parsed System instruction names (advance, initialize,
/// authorize, withdraw), at any depth, inner instructions included.
fn collect_nonce_accounts(value: &serde_json::Value, out: &mut Vec<Pubkey>) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(pk) = map.get("nonceAccount").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()) {
                if !out.contains(&pk) {
                    out.push(pk);
                }
            }
            map.values().for_each(|v| collect_nonce_accounts(v, out));
        }
        serde_json::Value::Array(items) => items.iter().for_each(|v| collect_nonce_accounts(v, out)),
        _ => {}
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_names_every_nonce_once_at_any_depth() {
        let a = Pubkey::new_unique();
        let b = Pubkey::new_unique();
        let page = serde_json::json!({ "data": [
            { "transaction": { "message": { "instructions": [
                { "program": "system", "parsed": { "type": "advanceNonce", "info": { "nonceAccount": a.to_string() } } },
                { "program": "system", "parsed": { "type": "transfer", "info": { "source": b.to_string() } } },
            ] } } },
            { "meta": { "innerInstructions": [ { "instructions": [
                { "parsed": { "type": "initializeNonce", "info": { "nonceAccount": b.to_string() } } },
                { "parsed": { "type": "advanceNonce", "info": { "nonceAccount": a.to_string() } } },
                { "parsed": { "type": "advanceNonce", "info": { "nonceAccount": "not-a-pubkey" } } },
            ] } ] } },
        ], "paginationToken": null });
        let mut found = Vec::new();
        collect_nonce_accounts(&page, &mut found);
        assert_eq!(found, vec![a, b]);
    }
}
