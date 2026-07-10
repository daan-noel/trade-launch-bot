//! The holdings read model: seed a mint's positions from its launch/bundle fills,
//! then reconcile each against chain (balance + canonical token account) and the
//! ingested feed (realized proceeds). Cold, operator-triggered path.

use std::time::Duration;

use anyhow::{Context, Result};
use platform_core::models::{BundleStatus, PositionStatus, TokenPosition, WalletRole};
use platform_core::storage::repositories::{
    BundleRepo, LaunchRepo, TokenPositionRepo, TradeRepo,
};
use sqlx::PgPool;
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

/// Reconcile every position's on-chain balance (`getTokenAccountsByOwner`) and its
/// feed-accurate realized proceeds (sum of the wallet's sell fills for the mint).
/// Public so the sell executor can refresh positions immediately after a sell.
pub async fn reconcile_positions(
    pool: &PgPool,
    settings: &LauncherSettings,
    mint_address: &str,
) -> Result<()> {
    let positions = TokenPositionRepo::by_mint(pool, mint_address).await?;
    if positions.is_empty() {
        return Ok(());
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("build reqwest client for position reconcile")?;

    for pos in &positions {
        // The canonical wallet identity is denormalized onto the row (no
        // managed_wallets join). A `dropped` leg never bought — skip its balance
        // RPC and keep it terminal (see `set_balance`'s dropped guard).
        if pos.status == PositionStatus::Dropped.as_str() {
            continue;
        }
        let owner = &pos.wallet_address;
        // On-chain balance + canonical account.
        let (balance_base, token_account) =
            fetch_token_holding(&client, &settings.rpc_url, owner, mint_address).await?;
        TokenPositionRepo::set_balance(pool, pos.id, balance_base, token_account.as_deref()).await?;

        // Feed-accurate realized proceeds (sum of this wallet's sells for the
        // mint) — authoritative from `trades`, never a fabricated estimate. Lags
        // one ingest cycle behind a just-fired sell; the next read picks it up.
        let realized = TradeRepo::sum_side_quote_by_address(pool, mint_address, owner, "sell")
            .await
            .unwrap_or(0);
        TokenPositionRepo::set_realized(pool, pos.id, realized).await?;
    }

    Ok(())
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
