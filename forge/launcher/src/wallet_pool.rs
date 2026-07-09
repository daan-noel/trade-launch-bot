//! Fresh-wallet pool lifecycle (docs/wallet-pool-plan.md Phase 1): batch
//! generation, balance-driven funding detection, and the reservation TTL sweep.
//! The atomic claim / mark-used transitions live on `ManagedWalletRepo` itself
//! (`platform_core::storage::repositories::own_launch`) since they're plain SQL
//! state transitions, not launch-domain orchestration.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use platform_core::models::{NewManagedWallet, WalletRole, WalletStatus};
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

/// Spawn the balance poller as a long-lived task. Cheap when idle — bounded to
/// the (small) `generated` set via the partial index in migration `0004`.
pub fn spawn_balance_poller(pool: PgPool, rpc_url: String) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("build reqwest client for wallet balance poller");
        loop {
            let pending = match poll_balances_once(&pool, &client, &rpc_url).await {
                Ok(n) => n,
                Err(e) => {
                    warn!(?e, "wallet balance poll failed");
                    0
                }
            };
            // Poll fast only while wallets are actively awaiting SOL; fall back to
            // the idle cadence the moment the pool settles.
            let wait = if pending > 0 {
                BALANCE_POLL_ACTIVE_INTERVAL
            } else {
                BALANCE_POLL_IDLE_INTERVAL
            };
            tokio::time::sleep(wait).await;
        }
    })
}

/// Returns the number of pollable (`generated`/`funding`) wallets this pass saw,
/// so the caller can pick the active vs idle cadence.
async fn poll_balances_once(pool: &PgPool, client: &reqwest::Client, rpc_url: &str) -> Result<usize> {
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
                    wallet_id = %wallet.id,
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

/// Spawn the reservation TTL sweep as a long-lived task. Cheap when idle —
/// bounded to the (small) `reserved` set via the partial index in migration `0004`.
pub fn spawn_reservation_sweep(pool: PgPool) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(RESERVATION_SWEEP_INTERVAL);
        loop {
            tick.tick().await;
            let cutoff = Utc::now() - RESERVATION_TTL;
            match ManagedWalletRepo::release_expired_reservations(&pool, cutoff).await {
                Ok(0) => {}
                Ok(released) => info!(released, "reservation TTL sweep released stranded wallets"),
                Err(e) => warn!(?e, "reservation TTL sweep failed"),
            }
        }
    })
}
