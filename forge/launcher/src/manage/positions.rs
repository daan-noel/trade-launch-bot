//! The holdings read model: seed a mint's positions from its launch/bundle fills,
//! then reconcile each against chain (balance + canonical token account) and the
//! ingested feed (realized proceeds). Cold, operator-triggered path.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result};
use platform_core::models::{BundleStatus, PositionStatus, TokenPosition, WalletRole, WalletStatus};
use platform_core::storage::repositories::{
    BundleRepo, LaunchRepo, ManagedWalletRepo, TokenPositionRepo, TradeRepo,
};
use sqlx::PgPool;
use tokio::task::JoinSet;
use tracing::warn;

use crate::bundle::legs_from_json;
use crate::config::LauncherSettings;

/// Load the per-wallet holdings for a launched mint. Seeds any missing cost-basis
/// rows from the launch's dev buy + bundle legs (idempotent), then — when RPC is
/// configured — reconciles each row's on-chain balance + feed realized proceeds
/// best-effort (a reconcile failure is logged, never fatal: seeded rows still
/// return).
pub async fn load_positions(
    pool: &PgPool,
    settings: Option<&LauncherSettings>,
    mint_address: &str,
) -> Result<Vec<TokenPosition>> {
    seed_positions(pool, mint_address).await?;

    if let Some(settings) = settings {
        if let Err(e) = reconcile_positions(pool, settings, mint_address).await {
            warn!(%mint_address, ?e, "position reconcile failed — returning seeded rows");
        }
    }

    TokenPositionRepo::by_mint(pool, mint_address).await
}

/// Fast read for the operator holdings view: seed (idempotent) + return the
/// current snapshot WITHOUT the on-chain reconcile. The GET `/positions` handler
/// serves this immediately (O(1) DB) and kicks the reconcile off the request path
/// so a slow/failing RPC never blocks the response; the refreshed balances land
/// in the DB for the next read. The sell/preview paths still reconcile inline
/// (they must size off fresh balances) via [`load_positions`].
pub async fn read_positions(pool: &PgPool, mint_address: &str) -> Result<Vec<TokenPosition>> {
    seed_positions(pool, mint_address).await?;
    TokenPositionRepo::by_mint(pool, mint_address).await
}

/// Seed cost-basis rows for the dev wallet + every bundle leg of a mint's launch.
/// Idempotent, so it's safe to call on every read. No-op when the mint isn't one
/// of our launches.
async fn seed_positions(pool: &PgPool, mint_address: &str) -> Result<()> {
    let Some(launch) = LaunchRepo::find_by_mint(pool, mint_address).await? else {
        return Ok(());
    };

    if let Some(dev_wallet_id) = launch.dev_wallet_id {
        TokenPositionRepo::seed(
            pool,
            mint_address,
            dev_wallet_id,
            WalletRole::Dev.as_str(),
            launch.dev_buy_quote.unwrap_or(0),
        )
        .await?;
    }

    if let Some(bundle_id) = launch.bundle_id {
        if let Some(bundle) = BundleRepo::get(pool, bundle_id).await? {
            // A Jito bundle is atomic: `landed` ⇒ every leg bought; any other
            // TERMINAL outcome (dropped/failed) ⇒ no leg bought, so those wallets
            // hold nothing and spent nothing — seed them `dropped` at zero cost
            // (not a phantom closed buy). While the bundle is still non-terminal
            // (planned/submitting/submitted) we can't yet tell, so skip seeding
            // and let a later read (once the confirm watcher resolves it) seed the
            // correct row — `seed`/`seed_dropped` are DO-NOTHING idempotent, so the
            // first terminal read wins and sticks.
            match bundle.status.parse::<BundleStatus>() {
                Ok(BundleStatus::Landed) => {
                    for leg in legs_from_json(&bundle.legs)? {
                        TokenPositionRepo::seed(
                            pool,
                            mint_address,
                            leg.managed_wallet_id,
                            WalletRole::Bundler.as_str(),
                            leg.quote_amount,
                        )
                        .await?;
                    }
                }
                Ok(BundleStatus::Dropped) | Ok(BundleStatus::Failed) => {
                    for leg in legs_from_json(&bundle.legs)? {
                        TokenPositionRepo::seed_dropped(
                            pool,
                            mint_address,
                            leg.managed_wallet_id,
                            WalletRole::Bundler.as_str(),
                        )
                        .await?;
                    }
                }
                // Non-terminal (or an unrecognized status) — don't seed yet.
                _ => {}
            }
        }
    }

    Ok(())
}

/// Reconcile a mint's positions against chain + feed, **discovering** any managed
/// wallet that holds the mint but was never seeded a row. Public so the sell
/// executor can refresh positions immediately after a sell.
///
/// The probe set is every non-retired managed wallet (discovery) UNION every
/// existing non-`dropped` position owner (so a sold-out row on a since-retired
/// wallet still reconciles to `closed`). Each is queried for its on-chain balance
/// (`getTokenAccountsByOwner`) concurrently; a wallet found holding the mint with
/// no position row gets one seeded (cost basis 0 — the row exists only to make the
/// holding visible + sellable) before the batch balance write. This is what makes
/// "sell all" correct for a manual manage-buy or any out-of-band holding, not just
/// the launch dev/bundle wallets.
///
/// Shape (audit 2026-07-13): realized proceeds for ALL wallets come from ONE
/// grouped query (not a per-position N+1); the per-wallet balance RPCs run
/// **concurrently** (a slow/failing wallet logs + skips instead of `?`-aborting
/// the whole pass); and every reconciled row is written in ONE `UNNEST` batch
/// update (not two UPDATEs × N rows). Cold, operator-triggered path (preview /
/// sell / holdings read) — the per-wallet scan is acceptable here and must never
/// run on an ingest hot path.
pub async fn reconcile_positions(
    pool: &PgPool,
    settings: &LauncherSettings,
    mint_address: &str,
) -> Result<()> {
    let existing = TokenPositionRepo::by_mint(pool, mint_address).await?;
    let existing_owners: std::collections::HashSet<String> =
        existing.iter().map(|p| p.wallet_address.clone()).collect();

    // Probe set: owner_address -> (managed_wallet_id, role). Seed from every
    // non-retired managed wallet (discovery), then add any existing non-dropped
    // position owner. A `dropped` leg never bought — never probe or seed it.
    let mut probe: HashMap<String, (uuid::Uuid, String)> = HashMap::new();
    for w in ManagedWalletRepo::list_all(pool, None).await? {
        if w.status == WalletStatus::Retired.as_str() {
            continue;
        }
        probe.insert(w.address, (w.id, w.role));
    }
    for p in &existing {
        if p.status == PositionStatus::Dropped.as_str() {
            continue;
        }
        probe
            .entry(p.wallet_address.clone())
            .or_insert((p.managed_wallet_id, p.role.clone()));
    }
    if probe.is_empty() {
        return Ok(());
    }

    // Feed-accurate realized proceeds for every wallet that has sold — one scan +
    // GROUP BY, defaulting the rest to 0. Lags one ingest cycle behind a
    // just-fired sell; the next read picks it up.
    let realized_by_addr: HashMap<String, i64> =
        TradeRepo::sum_sells_by_address_for_mint(pool, mint_address)
            .await?
            .into_iter()
            .collect();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("build reqwest client for position reconcile")?;

    // Fire every probed wallet's balance RPC concurrently.
    let mut set = JoinSet::new();
    for (owner, (wid, role)) in &probe {
        let client = client.clone();
        let rpc_url = settings.rpc_url.clone();
        let owner = owner.clone();
        let role = role.clone();
        let wid = *wid;
        let mint = mint_address.to_string();
        set.spawn(async move {
            let res = fetch_token_holding(&client, &rpc_url, &owner, &mint).await;
            (owner, wid, role, res)
        });
    }

    // Collect each wallet's balance; seed a row for any DISCOVERED holder (holds
    // the mint, no existing row) so the batch update below has an id to write. A
    // failed wallet is logged and skipped — it keeps its last-known row rather than
    // aborting the pass.
    let mut holdings: HashMap<String, (i64, Option<String>)> = HashMap::new();
    while let Some(joined) = set.join_next().await {
        let (owner, wid, role, res) = match joined {
            Ok(t) => t,
            Err(e) => {
                warn!(?e, "position balance task panicked — skipping");
                continue;
            }
        };
        match res {
            Ok((balance_base, token_account)) => {
                if balance_base > 0 && !existing_owners.contains(&owner) {
                    if let Err(e) =
                        TokenPositionRepo::seed(pool, mint_address, wid, &role, 0).await
                    {
                        warn!(%owner, ?e, "seed discovered holder position failed — skipping");
                        continue;
                    }
                }
                holdings.insert(owner, (balance_base, token_account));
            }
            Err(e) => warn!(%owner, ?e, "position balance RPC failed — skipping this wallet"),
        }
    }

    // Re-read so freshly-seeded discovery rows carry an id, then write every
    // non-dropped row we have a fresh balance for in ONE `UNNEST` batch update.
    let positions = TokenPositionRepo::by_mint(pool, mint_address).await?;
    let mut ids = Vec::new();
    let mut balances = Vec::new();
    let mut token_accounts: Vec<Option<String>> = Vec::new();
    let mut realized = Vec::new();
    for pos in &positions {
        if pos.status == PositionStatus::Dropped.as_str() {
            continue;
        }
        let Some((balance_base, token_account)) = holdings.get(&pos.wallet_address) else {
            continue;
        };
        ids.push(pos.id);
        balances.push(*balance_base);
        token_accounts.push(token_account.clone());
        realized.push(realized_by_addr.get(&pos.wallet_address).copied().unwrap_or(0));
    }

    TokenPositionRepo::reconcile_batch(pool, &ids, &balances, &token_accounts, &realized).await
}

/// Sum an owner's SPL balance for a mint, and pick the largest token account as
/// the canonical one. Works across SPL Token + Token-2022 (the RPC resolves the
/// program from the mint filter). A never-created ATA ⇒ `(0, None)`.
async fn fetch_token_holding(
    client: &reqwest::Client,
    rpc_url: &str,
    owner: &str,
    mint: &str,
) -> Result<(i64, Option<String>)> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getTokenAccountsByOwner",
        "params": [owner, { "mint": mint }, { "encoding": "jsonParsed", "commitment": "confirmed" }],
    });
    let v: serde_json::Value = client
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
        anyhow::bail!("getTokenAccountsByOwner RPC error: {err}");
    }
    let accounts = v
        .get("result")
        .and_then(|r| r.get("value"))
        .and_then(|value| value.as_array())
        .context("getTokenAccountsByOwner response missing result.value")?;

    let mut total: i64 = 0;
    let mut best_account: Option<String> = None;
    let mut best_amount: i64 = -1;
    for acc in accounts {
        let amount = acc
            .pointer("/account/data/parsed/info/tokenAmount/amount")
            .and_then(|a| a.as_str())
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);
        total = total.saturating_add(amount);
        if amount > best_amount {
            best_amount = amount;
            best_account = acc.get("pubkey").and_then(|p| p.as_str()).map(String::from);
        }
    }
    Ok((total, best_account))
}
