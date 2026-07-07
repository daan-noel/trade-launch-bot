//! Fresh-wallet pool lifecycle (docs/wallet-pool-plan.md Phase 1): batch
//! generation, balance-driven funding detection, and the reservation TTL sweep.
//! The atomic claim / mark-used transitions live on `ManagedWalletRepo` itself
//! (`platform_core::storage::repositories::own_launch`) since they're plain SQL
//! state transitions, not launch-domain orchestration.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use platform_core::models::NewManagedWallet;
use platform_core::storage::repositories::ManagedWalletRepo;
use solana_sdk::signature::{Keypair, Signer};
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

use crate::config::LauncherSettings;
use crate::keystore::{self, EnvKek};

/// Balance a `generated` wallet must clear before it's promoted to `funded`.
/// Small enough to not gate on gas-only floats, large enough to reject dust.
const MIN_FUNDED_LAMPORTS: i64 = 1_000_000; // 0.001 SOL

const BALANCE_POLL_INTERVAL: Duration = Duration::from_secs(30);
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
    if !matches!(role, "dev" | "bundler" | "treasury" | "trading") {
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
        let mut tick = tokio::time::interval(BALANCE_POLL_INTERVAL);
        loop {
            tick.tick().await;
            if let Err(e) = poll_balances_once(&pool, &client, &rpc_url).await {
                warn!(?e, "wallet balance poll failed");
            }
        }
    })
}

async fn poll_balances_once(pool: &PgPool, client: &reqwest::Client, rpc_url: &str) -> Result<()> {
    let generated = ManagedWalletRepo::find_by_status(pool, "generated", None).await?;
    if generated.is_empty() {
        return Ok(());
    }

    for chunk in generated.chunks(RPC_BATCH_SIZE) {
        let addresses: Vec<String> = chunk.iter().map(|w| w.address.clone()).collect();
        let balances = fetch_balances(client, rpc_url, &addresses).await?;
        for (wallet, balance) in chunk.iter().zip(balances) {
            let balance = balance as i64;
            let updated =
                ManagedWalletRepo::record_balance(pool, wallet.id, balance, MIN_FUNDED_LAMPORTS)
                    .await?;
            if updated.status == "funded" {
                info!(
                    wallet_id = %wallet.id,
                    address = %wallet.address,
                    balance,
                    "wallet funded — promoted generated -> funded"
                );
            }
        }
    }
    Ok(())
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
