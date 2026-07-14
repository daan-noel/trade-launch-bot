//! Fresh-wallet pool lifecycle (docs/wallet-pool-plan.md Phase 1): batch
//! generation, balance-driven funding detection, and the reservation TTL sweep.
//! The atomic claim / mark-used transitions live on `ManagedWalletRepo` itself
//! (`platform_core::storage::repositories::own_launch`) since they're plain SQL
//! state transitions, not launch-domain orchestration.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use platform_core::models::{ManagedWallet, NewManagedWallet, WalletRole, WalletStatus};
use platform_core::storage::repositories::ManagedWalletRepo;
use std::str::FromStr;
use solana_sdk::signature::{Keypair, Signer};
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

use crate::config::LauncherSettings;
use crate::keystore::{self, EnvKek};

/// Balance a `generated` wallet must clear before it's promoted to `funded`.
/// Small enough to not gate on gas-only floats, large enough to reject dust.
/// `pub(crate)` so the funding write-back reuses the same threshold (SSOT).
pub(crate) const MIN_FUNDED_LAMPORTS: i64 = 1_000_000; // 0.001 SOL

/// Steady-state cadence when no wallet is awaiting SOL — keeps idle RPC cost on
/// the EC2 box unchanged (the 4GB/2vCPU guardrail: don't raise steady polling).
const BALANCE_POLL_IDLE_INTERVAL: Duration = Duration::from_secs(30);
/// Faster cadence WHILE wallets are mid-funding (`generated`/`funding`), so the
/// `funding -> funded` promotion — and a dev wallet becoming launch-ready —
/// doesn't lag up to 30s after a treasury send lands. Only active during the
/// short window a funding pass is settling, so steady-state cost is unaffected.
const BALANCE_POLL_ACTIVE_INTERVAL: Duration = Duration::from_secs(5);
const RESERVATION_SWEEP_INTERVAL: Duration = Duration::from_secs(30);
/// An aborted/crashed launch shouldn't strand claimed wallets forever.
const RESERVATION_TTL: chrono::Duration = chrono::Duration::minutes(15);
/// Solana JSON-RPC `getMultipleAccounts` caps at 100 pubkeys per call.
const RPC_BATCH_SIZE: usize = 100;

/// Generate `count` fresh ed25519 keypairs for `role`, envelope-encrypt each into
/// the keystore, and insert as `generated`. Server-side batch generation — the
/// caller never sees raw key material.
pub async fn generate_wallets(
    pool: &PgPool,
    settings: &LauncherSettings,
    role: &str,
    count: u32,
    label_prefix: Option<&str>,
) -> Result<Vec<platform_core::models::ManagedWallet>> {
    if count == 0 {
        bail!("wallet generation count must be positive");
    }
    if WalletRole::from_str(role).is_err() {
        bail!("unknown managed_wallets role: {role}");
    }

    let kek = EnvKek::from_passphrase(&settings.kek_passphrase);
    let mut wallets = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let kp = Keypair::new();
        let address = kp.pubkey().to_string();
        let key_ref = format!("{role}-{}.enc", Uuid::new_v4());
        keystore::write_envelope(&settings.keystore_dir.join(&key_ref), &kp.to_bytes(), &kek)
            .with_context(|| format!("envelope-encrypt generated wallet {address}"))?;

        let label = label_prefix.map(|p| format!("{p}-{}", &address[..8]));
        let wallet = ManagedWalletRepo::insert(
            pool,
            &NewManagedWallet {
                address,
                label,
                role: role.to_string(),
                key_ref,
                derivation_index: None,
            },
        )
        .await?;
        wallets.push(wallet);
    }

    info!(role, count, "generated fresh wallet batch");
    Ok(wallets)
}

/// Cadence knobs for the unified wallet-lifecycle tick (`wallet_lifecycle.rs`),
/// which now drives the balance poll (this crate no longer spawns a standalone
/// balance-poller task). Exposed so the orchestrator picks the same active/idle
/// base cadence this poll always used.
pub(crate) const BALANCE_POLL_IDLE: Duration = BALANCE_POLL_IDLE_INTERVAL;
pub(crate) const BALANCE_POLL_ACTIVE: Duration = BALANCE_POLL_ACTIVE_INTERVAL;
pub(crate) const RESERVATION_SWEEP: Duration = RESERVATION_SWEEP_INTERVAL;

/// Freshness window for reusing the cached `balance_lamports` in place of a fresh
/// RPC read (audit Phase C2). Comfortably longer than the lifecycle tick's active
/// 5s cadence — so a wallet the poller just refreshed counts as fresh — but short
/// enough that a reused figure is never meaningfully stale for a spend decision.
pub(crate) const BALANCE_FRESH_WINDOW: chrono::Duration = chrono::Duration::seconds(15);

/// The wallet's cached balance IFF it was checked on-chain within
/// [`BALANCE_FRESH_WINDOW`], else `None` (the caller must RPC). Lets funding / dust
/// reuse the balance the unified lifecycle poll wrote moments earlier instead of
/// issuing a second identical `get_balance`. Pre-send correctness is unchanged: a
/// fresh cache value is a confirmed-commitment read from seconds ago, and the
/// reserve/cap rails still apply to whichever figure is used.
pub(crate) fn fresh_cached_balance(w: &ManagedWallet) -> Option<u64> {
    let checked = w.balance_checked_at?;
    if Utc::now() - checked <= BALANCE_FRESH_WINDOW {
        u64::try_from(w.balance_lamports?).ok()
    } else {
        None
    }
}

/// Build the shared reqwest client the balance poll uses (10s timeout).
pub(crate) fn balance_poll_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("build reqwest client for wallet balance poller")
}

/// One reservation + funding TTL sweep (extracted from the former standalone
/// task so the unified lifecycle tick can drive it on the same 30s cadence).
/// Cheap — bounded to the small `reserved`/`funding` sets by the partial indexes
/// in migration `0004`.
pub(crate) async fn sweep_reservations_once(pool: &PgPool) {
    let cutoff = Utc::now() - RESERVATION_TTL;
    match ManagedWalletRepo::release_expired_reservations(pool, cutoff).await {
        Ok(0) => {}
        Ok(released) => info!(released, "reservation TTL sweep released stranded wallets"),
        Err(e) => warn!(?e, "reservation TTL sweep failed"),
    }
    match ManagedWalletRepo::revert_stale_funding(pool, cutoff).await {
        Ok(0) => {}
        Ok(reverted) => info!(reverted, "funding TTL sweep reverted stranded wallets"),
        Err(e) => warn!(?e, "funding TTL sweep failed"),
    }
}

/// Returns the number of pollable (`generated`/`funding`) wallets this pass saw,
/// so the caller can pick the active vs idle cadence.
pub(crate) async fn poll_balances_once(
    pool: &PgPool,
    client: &reqwest::Client,
    rpc_url: &str,
) -> Result<usize> {
    // Both `generated` (manual funding) and `funding` (an automated treasury send
    // in flight) are watched — either promotes to `funded` when SOL lands. Only
    // this set drives the fast/idle cadence (`pending`).
    let pollable = ManagedWalletRepo::find_pollable(pool).await?;
    let pending = pollable.len();

    // Also refresh the treasury's cached balance every pass so the Wallet Pool
    // page reflects outbound spends (funding sends, launch bundles) instead of a
    // stale pre-send number. The treasury is the SOL *source* — never promoted —
    // so it stays OUT of `pending`: a single always-present wallet must not pin
    // the poller to the 5s active interval (EC2 idle-cost guardrail).
    let mut wallets = pollable;
    for t in ManagedWalletRepo::by_role(pool, WalletRole::Treasury.as_str()).await? {
        // Skip if it's mid-funding and thus already in `pollable`.
        if !wallets.iter().any(|w| w.id == t.id) {
            wallets.push(t);
        }
    }
    if wallets.is_empty() {
        return Ok(0);
    }

    for chunk in wallets.chunks(RPC_BATCH_SIZE) {
        let addresses: Vec<String> = chunk.iter().map(|w| w.address.clone()).collect();
        let balances = fetch_balances(client, rpc_url, &addresses).await?;
        for (wallet, balance) in chunk.iter().zip(balances) {
            let balance = balance as i64;
            let updated =
                ManagedWalletRepo::record_balance(pool, wallet.id, balance, MIN_FUNDED_LAMPORTS)
                    .await?;
            if updated.status == WalletStatus::Funded.as_str()
                && wallet.status != WalletStatus::Funded.as_str()
            {
                info!(
                    managed_wallet_id = %wallet.id,
                    address = %wallet.address,
                    prev = %wallet.status,
                    balance,
                    "wallet funded — promoted to funded"
                );
            }
        }
    }
    Ok(pending)
}

/// One-shot LIVE balance refresh across the WHOLE managed-wallet set (every role,
/// every status incl. `funded`/`reserved`/`used`/`retired`), so the operator can
/// read an exact "how much SOL is in my wallets right now" figure on demand.
///
/// Deliberately NOT wired into the steady poller: that stays bounded to
/// `generated`/`funding` + treasury to hold EC2 idle RPC cost flat (the 4GB/2vCPU
/// guardrail). This is an operator-triggered burst instead — batched
/// `getMultipleAccounts` (100 pubkeys/call), one cache write per wallet. Returns
/// the freshly-stamped rows (balance + `balance_checked_at`), optionally scoped to
/// one role, in list order. Races the steady poller harmlessly: both write via
/// `record_balance` (last-writer-wins on balance/checked_at, no corruption).
pub async fn refresh_all_balances(
    pool: &PgPool,
    rpc_url: &str,
    role: Option<&str>,
) -> Result<Vec<platform_core::models::ManagedWallet>> {
    let wallets = ManagedWalletRepo::list_all(pool, role).await?;
    if wallets.is_empty() {
        return Ok(wallets);
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("build reqwest client for balance refresh")?;
    for chunk in wallets.chunks(RPC_BATCH_SIZE) {
        let addresses: Vec<String> = chunk.iter().map(|w| w.address.clone()).collect();
        let balances = fetch_balances(&client, rpc_url, &addresses).await?;
        for (wallet, balance) in chunk.iter().zip(balances) {
            ManagedWalletRepo::record_balance(pool, wallet.id, balance as i64, MIN_FUNDED_LAMPORTS)
                .await?;
        }
    }
    info!(count = wallets.len(), ?role, "operator live balance refresh");
    // Re-read so the caller gets the freshly-stamped rows.
    ManagedWalletRepo::list_all(pool, role).await
}

/// `getMultipleAccounts` lamport balances, in the same order as `addresses`.
/// Missing/never-funded accounts come back `null` from the RPC — treated as 0.
async fn fetch_balances(
    client: &reqwest::Client,
    rpc_url: &str,
    addresses: &[String],
) -> Result<Vec<u64>> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getMultipleAccounts",
        "params": [addresses, { "commitment": "confirmed" }],
    });
    let resp = client
        .post(rpc_url)
        .json(&body)
        .send()
        .await
        .context("getMultipleAccounts HTTP")?
        .error_for_status()
        .context("getMultipleAccounts HTTP status")?;
    let v: serde_json::Value = resp.json().await.context("parse getMultipleAccounts body")?;
    if let Some(err) = v.get("error") {
        bail!("getMultipleAccounts RPC error: {err}");
    }
    let accounts = v
        .get("result")
        .and_then(|r| r.get("value"))
        .and_then(|value| value.as_array())
        .context("getMultipleAccounts response missing result.value")?;
    Ok(accounts
        .iter()
        .map(|a| a.get("lamports").and_then(|l| l.as_u64()).unwrap_or(0))
        .collect())
}

