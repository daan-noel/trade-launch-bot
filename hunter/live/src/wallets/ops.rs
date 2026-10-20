//! Single-wallet actions from the Wallets page: send SOL, rename, archive an
//! idle empty wallet, restore an archived one. Each one is logged to
//! `wallet_activity`.

use std::str::FromStr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;
use solana_sdk::system_instruction;
use sqlx::PgPool;
use zeroize::Zeroizing;

use trading_core::models::trader_wallet::TraderWalletRecord;
use trading_core::storage::repositories::trader_wallet_repo::TraderWalletRepo;

use super::keystore::{read_keypair, same_name, valid_name};
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
    let free = spendable(runtime, from).await?;
    if lamports > free {
        bail!("only {:.6} SOL is spendable", free as f64 / 1e9);
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

/// Wallets one fund transaction pays; each is one transfer instruction.
const FUND_MAX_TARGETS: usize = 16;
/// A swept wallet with open positions keeps at least this, for its sells' fees.
const SWEEP_OPEN_KEEP: u64 = 10_000_000;

/// SOL a loaded wallet can send now: its balance less a fee reserve and the
/// SOL committed to buys in flight.
async fn spendable(runtime: &WalletRuntime, pubkey: &str) -> Result<u64> {
    let trader = runtime.trader(pubkey).context("wallet is not loaded")?;
    let balance = trader.get_sol_balance().await?;
    Ok(balance.saturating_sub(SEND_FEE_RESERVE).saturating_sub(trader.committed_lamports()))
}

/// One transaction from `from` that pays each managed wallet its amount.
pub async fn fund(pool: &PgPool, runtime: &WalletRuntime, from: &str, targets: &[(String, u64)]) -> Result<String> {
    if targets.is_empty() || targets.len() > FUND_MAX_TARGETS {
        bail!("fund 1 to {FUND_MAX_TARGETS} wallets at a time");
    }
    let repo = TraderWalletRepo::new(pool.clone());
    let records = repo.records().await?;
    let source = records.iter().find(|r| r.pubkey == from).context("wallet is not registered")?;
    if source.status != "active" {
        bail!("wallet is {}", source.status);
    }
    require_no_move(&repo, from).await?;
    require_loaded(runtime, from)?;
    let mut ixs = Vec::with_capacity(targets.len());
    let key = runtime.signer(from).context("wallet key")?;
    for (to, lamports) in targets {
        if *lamports == 0 {
            bail!("every amount must be above zero");
        }
        if to == from {
            bail!("source and destination are the same wallet");
        }
        if !records.iter().any(|r| r.pubkey == *to && r.status != "archived") {
            bail!("{to} is not a managed wallet");
        }
        ixs.push(system_instruction::transfer(&key.pubkey(), &Pubkey::from_str(to)?, *lamports));
    }
    let total: u64 = targets.iter().map(|(_, l)| l).sum();
    let free = spendable(runtime, from).await?;
    if total > free {
        bail!("only {:.6} SOL is spendable", free as f64 / 1e9);
    }
    let sig = send(runtime, &key, ixs, &[]).await?;
    let sigs = [sig.clone()];
    for (to, lamports) in targets {
        let detail = serde_json::json!({ "to": to, "lamports": lamports, "external": false });
        let _ = repo.log_activity(from, "sol_sent", detail, &sigs).await;
        let detail = serde_json::json!({ "from": from, "lamports": lamports });
        let _ = repo.log_activity(to, "sol_received", detail, &sigs).await;
    }
    Ok(sig)
}

/// One wallet's sweep: sent, or why it was not.
#[derive(serde::Serialize)]
pub struct SweepResult {
    pub wallet: String,
    pub lamports: u64,
    pub signature: Option<String>,
    pub error: Option<String>,
}

/// Send each wallet's spendable SOL above `keep` to `to`, one transaction per
/// wallet. A wallet with open positions keeps at least [`SWEEP_OPEN_KEEP`] for
/// its sells. A failure is that wallet's entry, never the whole sweep.
pub async fn sweep(pool: &PgPool, runtime: &WalletRuntime, wallets: &[String], to: &str, keep: u64) -> Result<Vec<SweepResult>> {
    let repo = TraderWalletRepo::new(pool.clone());
    let records = repo.records().await?;
    if !records.iter().any(|r| r.pubkey == to && r.status != "archived") {
        bail!("destination is not a managed wallet");
    }
    let dest = Pubkey::from_str(to)?;
    let mut out = Vec::with_capacity(wallets.len());
    for from in wallets {
        let res = sweep_one(&repo, runtime, &records, from, to, &dest, keep).await;
        out.push(match res {
            Ok((lamports, signature)) => SweepResult { wallet: from.clone(), lamports, signature, error: None },
            Err(e) => SweepResult { wallet: from.clone(), lamports: 0, signature: None, error: Some(format!("{e:#}")) },
        });
    }
    Ok(out)
}

async fn sweep_one(
    repo: &TraderWalletRepo,
    runtime: &WalletRuntime,
    records: &[TraderWalletRecord],
    from: &str,
    to: &str,
    dest: &Pubkey,
    keep: u64,
) -> Result<(u64, Option<String>)> {
    if from == to {
        bail!("is the destination");
    }
    let rec = records.iter().find(|r| r.pubkey == from).context("not registered")?;
    if rec.status != "active" {
        bail!("wallet is {}", rec.status);
    }
    require_no_move(repo, from).await?;
    require_loaded(runtime, from)?;
    let keep = if repo.open_positions(from).await?.is_empty() { keep } else { keep.max(SWEEP_OPEN_KEEP) };
    let lamports = spendable(runtime, from).await?.saturating_sub(keep);
    if lamports == 0 {
        return Ok((0, None));
    }
    let key = runtime.signer(from).context("wallet key")?;
    let sig = send(runtime, &key, vec![system_instruction::transfer(&key.pubkey(), dest, lamports)], &[]).await?;
    let sigs = [sig.clone()];
    let detail = serde_json::json!({ "to": to, "lamports": lamports, "external": false });
    let _ = repo.log_activity(from, "sol_sent", detail, &sigs).await;
    let detail = serde_json::json!({ "from": from, "lamports": lamports });
    let _ = repo.log_activity(to, "sol_received", detail, &sigs).await;
    Ok((lamports, Some(sig)))
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

/// Stop or resume new entries on a wallet. Open positions keep exiting either way.
pub async fn set_paused(pool: &PgPool, runtime: &WalletRuntime, pubkey: &str, paused: bool) -> Result<()> {
    let repo = TraderWalletRepo::new(pool.clone());
    if !repo.set_paused(pubkey, paused).await? {
        bail!("wallet is not registered or is archived");
    }
    runtime.directory.set_paused(pubkey, paused);
    let kind = if paused { "paused" } else { "resumed" };
    let _ = repo.log_activity(pubkey, kind, serde_json::json!({}), &[]).await;
    Ok(())
}

/// Wrong passphrases a reveal takes before it locks, and how long the lock holds.
const REVEAL_MAX_FAILURES: u32 = 5;
const REVEAL_LOCKOUT: Duration = Duration::from_secs(15 * 60);

/// Wrong-passphrase count across every wallet; a lock refuses every reveal.
struct RevealGate {
    failures: u32,
    locked_until: Option<Instant>,
}

impl RevealGate {
    const fn new() -> Self {
        Self { failures: 0, locked_until: None }
    }

    /// Seconds the lock still holds at `now`, or `None` when open.
    fn locked(&mut self, now: Instant) -> Option<u64> {
        match self.locked_until {
            Some(until) if until > now => Some((until - now).as_secs().max(1)),
            Some(_) => {
                *self = Self::new();
                None
            }
            None => None,
        }
    }

    fn fail(&mut self, now: Instant) {
        self.failures += 1;
        if self.failures >= REVEAL_MAX_FAILURES {
            self.locked_until = Some(now + REVEAL_LOCKOUT);
        }
    }
}

static REVEAL_GATE: Mutex<RevealGate> = Mutex::new(RevealGate::new());

/// A wallet's secret key, decrypted from its file.
pub struct RevealedKey {
    pub base58: Zeroizing<String>,
    pub bytes: Zeroizing<Vec<u8>>,
}

/// Decrypt one wallet's key for the page, from its file, so an archived or
/// unloaded wallet reveals too. `passphrase` must be `WALLET_KEK`: the bearer
/// token is the proxy's, so it does not prove who is at the page.
pub async fn reveal_key(pool: &PgPool, runtime: &WalletRuntime, pubkey: &str, passphrase: &str) -> Result<RevealedKey> {
    let dir = runtime.keystore_dir().context("WALLET_KEYSTORE is not set")?;
    let kek = runtime.kek().context("WALLET_KEK is not set")?;
    let repo = TraderWalletRepo::new(pool.clone());
    let rec = record(&repo, pubkey).await?;
    let matched = {
        let mut gate = REVEAL_GATE.lock().expect("reveal gate");
        let now = Instant::now();
        if let Some(secs) = gate.locked(now) {
            bail!("too many wrong passphrases; try again in {} min", secs.div_ceil(60));
        }
        let matched = kek.matches(passphrase);
        if matched {
            *gate = RevealGate::new();
        } else {
            gate.fail(now);
        }
        matched
    };
    if !matched {
        let _ = repo.log_activity(pubkey, "key_reveal_refused", serde_json::json!({}), &[]).await;
        bail!("wrong passphrase");
    }
    let keypair = read_keypair(dir, &rec.key_ref, kek)?;
    if keypair.pubkey().to_string() != pubkey {
        bail!("keystore file {} does not match pubkey {pubkey}", rec.key_ref);
    }
    let _ = repo.log_activity(pubkey, "key_revealed", serde_json::json!({}), &[]).await;
    tracing::warn!(wallet = %pubkey, "private key revealed on the Wallets page");
    Ok(RevealedKey {
        base58: Zeroizing::new(keypair.to_base58_string()),
        bytes: Zeroizing::new(keypair.to_bytes().to_vec()),
    })
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
            paused: false,
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

    #[test]
    fn the_reveal_gate_locks_after_the_failure_cap_and_reopens_after_the_lockout() {
        let mut gate = RevealGate::new();
        let t0 = Instant::now();
        for _ in 0..REVEAL_MAX_FAILURES - 1 {
            gate.fail(t0);
        }
        assert!(gate.locked(t0).is_none());
        gate.fail(t0);
        assert!(gate.locked(t0 + Duration::from_secs(60)).is_some());
        assert!(gate.locked(t0 + REVEAL_LOCKOUT).is_none());
        assert_eq!(gate.failures, 0);
    }
}
