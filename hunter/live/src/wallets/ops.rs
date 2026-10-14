//! Single-wallet actions from the Wallets page: send SOL, rename, archive an
//! idle empty wallet, restore an archived one. Each one is logged to
//! `wallet_activity`.

use std::str::FromStr;

use anyhow::{bail, Context, Result};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;
use solana_sdk::system_instruction;
use sqlx::PgPool;

use trading_core::models::trader_wallet::TraderWalletRecord;
use trading_core::storage::repositories::trader_wallet_repo::TraderWalletRepo;

use super::keystore::{same_name, valid_name};
use super::mover::{require_loaded, send};
use super::runtime::WalletRuntime;

/// Left on a wallet after a send, for that transaction's own fee.
const SEND_FEE_RESERVE: u64 = 10_000;
/// Above this a wallet still holds SOL worth sending before it is archived.
const ARCHIVE_DUST_LAMPORTS: u64 = 1_000_000;

pub const NAME_RULE: &str = "name must be 1 to 32 characters, with no leading or trailing space";

/// Why `name` cannot label a wallet (other than `except`), or `None`. The one
/// name check behind create, import and rename.
pub fn name_error(records: &[TraderWalletRecord], name: &str, except: Option<&str>) -> Option<String> {
    if !valid_name(name) {
        return Some(NAME_RULE.to_string());
    }
    records
        .iter()
        .find(|r| Some(r.pubkey.as_str()) != except && same_name(&r.name, name))
        .map(|r| format!("a wallet named {} already exists", r.name))
}

async fn record(repo: &TraderWalletRepo, pubkey: &str) -> Result<TraderWalletRecord> {
    repo.records()
        .await?
        .into_iter()
        .find(|r| r.pubkey == pubkey)
        .context("wallet is not registered")
}

async fn require_no_move(repo: &TraderWalletRepo, pubkey: &str) -> Result<()> {
    if let Some(mv) = repo.open_move().await? {
        if mv.from_wallet == pubkey || mv.to_wallet == pubkey {
            bail!("a move involving this wallet is in progress");
        }
    }
    Ok(())
}

/// Send `lamports` from a loaded wallet. `to` must be another registered wallet
/// unless `external` is set. SOL committed to in-flight buys and a fee reserve stay.
pub async fn send_sol(
    pool: &PgPool,
    runtime: &WalletRuntime,
    from: &str,
    to: &str,
    lamports: u64,
    external: bool,
) -> Result<String> {
    if lamports == 0 {
        bail!("amount must be above zero");
    }
    if from == to {
        bail!("source and destination are the same wallet");
    }
    let repo = TraderWalletRepo::new(pool.clone());
    let source = record(&repo, from).await?;
    if source.status != "active" {
        bail!("wallet is {}", source.status);
    }
    require_no_move(&repo, from).await?;
    require_loaded(runtime, from)?;
    let dest = Pubkey::from_str(to).context("destination is not a pubkey")?;
    let managed = repo.records().await?.into_iter().any(|r| r.pubkey == to && r.status != "archived");
    if !managed && !external {
        bail!("destination is not a managed wallet");
    }
    let trader = runtime.trader(from).context("wallet is not loaded")?;
    let balance = trader.get_sol_balance().await?;
    let spendable = balance
        .saturating_sub(SEND_FEE_RESERVE)
        .saturating_sub(trader.committed_lamports());
    if lamports > spendable {
        bail!("only {:.6} SOL is spendable", spendable as f64 / 1e9);
    }
    let key = runtime.signer(from).context("wallet key")?;
    let sig = send(
        runtime,
        &key,
        vec![system_instruction::transfer(&key.pubkey(), &dest, lamports)],
        &[],
    )
    .await?;
    let sigs = [sig.clone()];
    let detail = serde_json::json!({ "to": to, "lamports": lamports, "external": !managed });
    let _ = repo.log_activity(from, "sol_sent", detail, &sigs).await;
    if managed {
        let detail = serde_json::json!({ "from": from, "lamports": lamports });
        let _ = repo.log_activity(to, "sol_received", detail, &sigs).await;
    }
    Ok(sig)
}

pub async fn rename(pool: &PgPool, runtime: &WalletRuntime, pubkey: &str, name: &str) -> Result<()> {
    let repo = TraderWalletRepo::new(pool.clone());
    let records = repo.records().await?;
    let current = records.iter().find(|r| r.pubkey == pubkey).context("wallet is not registered")?;
    if current.name == name {
        return Ok(());
    }
    if let Some(e) = name_error(&records, name, Some(pubkey)) {
        bail!(e);
    }
    // The key file keeps its filename (`key_ref`); only the label changes.
    repo.rename(pubkey, name).await?;
    runtime.rename(pubkey, name);
    let detail = serde_json::json!({ "from": current.name, "to": name });
    let _ = repo.log_activity(pubkey, "renamed", detail, &[]).await;
    Ok(())
}

/// Archive a wallet nothing uses and that holds nothing: not primary, no rule,
/// no open position, no nonce account, no token, no SOL above dust. A wallet
/// that still holds something is moved instead.
pub async fn archive(pool: &PgPool, runtime: &WalletRuntime, pubkey: &str) -> Result<()> {
    let repo = TraderWalletRepo::new(pool.clone());
    let rec = record(&repo, pubkey).await?;
    if rec.status != "active" {
        bail!("wallet is {}", rec.status);
    }
    if rec.is_primary {
        bail!("set another wallet as primary first");
    }
    require_no_move(&repo, pubkey).await?;
    let rules = repo.rules_for(pubkey).await?;
    if !rules.is_empty() {
        bail!("{} rule(s) still use this wallet", rules.len());
    }
    let open = repo.open_positions(pubkey).await?;
    if !open.is_empty() {
        bail!("{} open position(s) on this wallet", open.len());
    }
    let nonces = repo.nonces_for(pubkey).await?;
    if !nonces.is_empty() {
        bail!("{} nonce account(s) still name this wallet; move the wallet instead", nonces.len());
    }
    if let Some(trader) = runtime.trader(pubkey) {
        let held = trader.get_all_token_accounts().await?.into_iter().filter(|h| h.amount > 0).count();
        if held > 0 {
            bail!("{held} token balance(s) on this wallet; move the wallet instead");
        }
        let sol = trader.get_sol_balance().await?;
        if sol > ARCHIVE_DUST_LAMPORTS {
            bail!("{:.6} SOL is still on this wallet; send it first", sol as f64 / 1e9);
        }
    }
    repo.set_status(pubkey, "archived").await?;
    runtime.unload(pubkey);
    let _ = repo.log_activity(pubkey, "archived", serde_json::json!({}), &[]).await;
    Ok(())
}

pub async fn restore(pool: &PgPool, runtime: &WalletRuntime, pubkey: &str) -> Result<()> {
    let repo = TraderWalletRepo::new(pool.clone());
    let rec = record(&repo, pubkey).await?;
    if rec.status != "archived" {
        bail!("wallet is not archived");
    }
    let nonces = repo
        .nonces_for(pubkey)
        .await?
        .into_iter()
        .map(|n| n.pubkey.parse::<Pubkey>())
        .collect::<Result<Vec<_>, _>>()?;
    // Load first: a key file that is missing or does not decrypt leaves the row archived.
    runtime.restore(pubkey, &rec.name, &rec.key_ref, nonces).await?;
    repo.restore(pubkey).await?;
    let _ = repo.log_activity(pubkey, "restored", serde_json::json!({}), &[]).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pubkey: &str, name: &str) -> TraderWalletRecord {
        TraderWalletRecord {
            pubkey: pubkey.to_string(),
            name: name.to_string(),
            status: "active".to_string(),
            key_ref: format!("{pubkey}.enc"),
            is_primary: false,
        }
    }

    #[test]
    fn a_name_is_taken_whatever_its_case_except_by_itself() {
        let rows = vec![row("A", "Main"), row("B", "desk")];
        assert!(name_error(&rows, "main", None).is_some());
        assert!(name_error(&rows, "MAIN", Some("A")).is_none());
        assert!(name_error(&rows, "Desk", Some("A")).is_some());
        assert!(name_error(&rows, " x", None).is_some());
        assert!(name_error(&rows, "Main Sniper", None).is_none());
    }
}
