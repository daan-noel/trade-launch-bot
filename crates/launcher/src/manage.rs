//! Post-launch token management (token-management-plan.md).
//!
//! Phase 1 is the read model only: resolve a launched token back to the wallets
//! that hold it (dev + bundle legs), seed their cost-basis rows, and reconcile
//! each against the on-chain SPL token balance. No trades are placed here — the
//! sell/buy/consolidate primitives land in later phases on top of these rows.

use std::time::Duration;

use anyhow::{Context, Result};
use platform_core::models::{TokenPosition, WalletRole};
use platform_core::storage::repositories::{
    BundleRepo, LaunchRepo, ManagedWalletRepo, TokenPositionRepo,
};
use sqlx::PgPool;
use tracing::warn;
use uuid::Uuid;

use crate::bundle::legs_from_json;
use crate::config::LauncherSettings;

/// Load the per-wallet holdings for a launched mint. Seeds any missing cost-basis
/// rows from the launch's dev buy + bundle legs (idempotent), then — when RPC is
/// configured — reconciles each row's on-chain token balance best-effort (a
/// reconcile failure is logged, never fatal: seeded rows still return).
///
/// Cold, operator-triggered path (the token detail panel), not the hot ingest
/// loop — a handful of RPC calls per token is fine here.
pub async fn load_positions(
    pool: &PgPool,
    settings: Option<&LauncherSettings>,
    mint_address: &str,
) -> Result<Vec<TokenPosition>> {
    seed_positions(pool, mint_address).await?;

    if let Some(settings) = settings {
        if let Err(e) = reconcile_positions(pool, settings, mint_address).await {
            warn!(%mint_address, ?e, "position balance reconcile failed — returning seeded rows");
        }
    }

    TokenPositionRepo::by_mint(pool, mint_address).await
}

/// Seed cost-basis rows for the dev wallet + every bundle leg of a mint's launch.
/// Idempotent (each `seed` is `ON CONFLICT DO NOTHING`), so it's safe to call on
/// every read. No-op when the mint isn't one of our launches.
async fn seed_positions(pool: &PgPool, mint_address: &str) -> Result<()> {
    let Some(launch) = LaunchRepo::find_by_mint(pool, mint_address).await? else {
        return Ok(());
    };

    // Dev wallet: cost basis = its dev-buy amount (quote base units).
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

    // Bundle legs: each leg's wallet + planned quote amount (cost basis).
    if let Some(bundle_id) = launch.bundle_id {
        if let Some(bundle) = BundleRepo::get(pool, bundle_id).await? {
            for leg in legs_from_json(&bundle.legs)? {
                TokenPositionRepo::seed(
                    pool,
                    mint_address,
                    leg.wallet_id,
                    WalletRole::Bundler.as_str(),
                    leg.quote_amount,
                )
                .await?;
            }
        }
    }

    Ok(())
}

/// Reconcile every open position's `balance_base` + canonical token account
/// against chain via `getTokenAccountsByOwner` (one call per holder wallet).
async fn reconcile_positions(
    pool: &PgPool,
    settings: &LauncherSettings,
    mint_address: &str,
) -> Result<()> {
    let positions = TokenPositionRepo::by_mint(pool, mint_address).await?;
    if positions.is_empty() {
        return Ok(());
    }

    // Resolve holder wallet_id -> address in one round trip.
    let wallet_ids: Vec<Uuid> = positions.iter().map(|p| p.wallet_id).collect();
    let wallets = ManagedWalletRepo::get_many(pool, &wallet_ids).await?;
    let addr_of = |id: Uuid| wallets.iter().find(|w| w.id == id).map(|w| w.address.clone());

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("build reqwest client for position reconcile")?;

    for pos in &positions {
        let Some(owner) = addr_of(pos.wallet_id) else {
            warn!(wallet_id = %pos.wallet_id, "position wallet missing from managed_wallets — skipped");
            continue;
        };
        let (balance_base, token_account) =
            fetch_token_holding(&client, &settings.rpc_url, &owner, mint_address).await?;
        TokenPositionRepo::set_balance(pool, pos.id, balance_base, token_account.as_deref()).await?;
    }

    Ok(())
}

/// Sum an owner's SPL balance for a mint, and pick the largest token account as
/// the canonical one. Works across the SPL Token + Token-2022 programs (the RPC
/// resolves the program from the mint filter). A never-created ATA comes back as
/// an empty account list → `(0, None)`.
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
