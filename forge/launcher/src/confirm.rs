//! Feed-based bundle-landing confirmation (LIVE box).
//!
//! Jito `sendBundle` only reports acceptance into the block-engine queue, not
//! landing — the same gap hunter's sell-confirm closes by trusting the
//! ingested feed over a fresh RPC poll. This watcher does the equivalent for
//! launch bundles: it never calls the chain itself, it only checks whether each
//! leg's signature has shown up in the already-ingested `trades` table.
//!
//! A true Jito bundle is atomic (all legs land or none do), so the expected
//! outcomes are `landed` / `dropped`; `partial` is logged as an anomaly rather
//! than designed around.

use std::collections::HashSet;
use std::time::Duration;

use anyhow::Context;
use chrono::Utc;
use sqlx::PgPool;
use tracing::{info, warn};

use platform_core::models::{Bundle, BundleStatus, WalletRole};
use platform_core::storage::repositories::{
    BundleRepo, LaunchRepo, ManagedWalletRepo, TokenPositionRepo, TradeRepo,
};

use crate::bundle::legs_from_json;
use crate::config::LauncherSettings;
use crate::execute_bundle;

/// How often to re-check bundles awaiting confirmation.
const POLL_INTERVAL: Duration = Duration::from_secs(3);
/// How long to wait for a bundle to land before declaring it dropped.
const CONFIRM_TIMEOUT: chrono::Duration = chrono::Duration::seconds(90);

/// Spawn the watcher as a long-lived task. Cheap when idle — the query is
/// bounded to the tiny `status = 'submitted'` set (see the partial index in
/// migration `0003`), never a full-table scan.
///
/// `settings` enables auto re-bid on a `dropped` verdict (re-submitting the
/// bundle at a higher Jito tip up to `bundle_max_retries`); `None` (the launcher
/// env failed to parse at boot) keeps the watcher read-only — it still confirms
/// landings and marks drops, it just can't re-bid.
pub fn spawn_bundle_confirm_watcher(
    pool: PgPool,
    settings: Option<LauncherSettings>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(POLL_INTERVAL);
        loop {
            tick.tick().await;
            if let Err(e) = confirm_pending(&pool, settings.as_ref()).await {
                warn!(?e, "bundle confirm pass failed");
            }
        }
    })
}

async fn confirm_pending(pool: &PgPool, settings: Option<&LauncherSettings>) -> anyhow::Result<()> {
    let pending = BundleRepo::find_awaiting_confirmation(pool).await?;
    for bundle in pending {
        let id = bundle.id;
        if let Err(e) = confirm_one(pool, bundle, settings).await {
            warn!(bundle_id = %id, ?e, "bundle confirm check failed");
        }
    }
    Ok(())
}

async fn confirm_one(
    pool: &PgPool,
    bundle: Bundle,
    settings: Option<&LauncherSettings>,
) -> anyhow::Result<()> {
    let launch = LaunchRepo::get(pool, bundle.launch_id)
        .await?
        .context("launch not found for bundle")?;

    let sigs: Vec<Vec<u8>> = bundle
        .leg_signatures
        .iter()
        .map(|s| bs58::decode(s).into_vec())
        .collect::<Result<_, _>>()
        .context("decode leg signature")?;
    if sigs.is_empty() {
        warn!(bundle_id = %bundle.id, "submitted bundle has no leg signatures — marking dropped");
        BundleRepo::set_confirmed(pool, bundle.id, BundleStatus::Dropped.as_str()).await?;
        mark_bundle_wallets_used(pool, &bundle).await;
        return Ok(());
    }

    let landed: HashSet<Vec<u8>> =
        TradeRepo::find_signatures_present(pool, &launch.mint_address, &sigs).await?;

    if landed.len() == sigs.len() {
        BundleRepo::set_confirmed(pool, bundle.id, BundleStatus::Landed.as_str()).await?;
        seed_landed_bundle_positions(pool, &launch.mint_address, &bundle).await;
        mark_bundle_wallets_used(pool, &bundle).await;
        info!(bundle_id = %bundle.id, legs = sigs.len(), "bundle landed");
        return Ok(());
    }

    let submitted_at = bundle
        .submitted_at
        .context("submitted bundle missing submitted_at")?;
    if Utc::now() - submitted_at <= CONFIRM_TIMEOUT {
        return Ok(()); // still within the landing window, check again next tick
    }

    if landed.is_empty() {
        // Auto re-bid: the bundle lost the Jito auction (accepted but not
        // included). Re-submit at a higher tip before conceding — `submit_attempts`
        // both bounds the retries and drives the escalation level (bundle_execute
        // reads it as p95/p99/…). Wallets stay `reserved` across the re-bid; they
        // only transition to `used` on a terminal outcome below.
        if let Some(settings) = settings {
            if (bundle.submit_attempts as u32) <= settings.bundle_max_retries {
                return rebid_dropped(pool, settings, &bundle).await;
            }
        }
        BundleRepo::set_confirmed(pool, bundle.id, BundleStatus::Dropped.as_str()).await?;
        warn!(
            bundle_id = %bundle.id,
            attempts = bundle.submit_attempts,
            "bundle dropped — no legs landed within timeout (retries exhausted)"
        );
    } else {
        // Atomicity anomaly: a Jito bundle should never land partially.
        BundleRepo::set_confirmed(pool, bundle.id, BundleStatus::Partial.as_str()).await?;
        warn!(
            bundle_id = %bundle.id,
            landed = landed.len(),
            total = sigs.len(),
            "bundle partially landed — Jito atomicity anomaly, investigate"
        );
    }
    mark_bundle_wallets_used(pool, &bundle).await;
    Ok(())
}

/// Re-submit a dropped bundle at the next tip-escalation level. Resets it to
/// `planned` (clearing the stale Jito id / leg signatures so this pass — and the
/// next watcher tick — can't re-pick it mid-flight), then re-runs the standard
/// execute path: it rebuilds the legs from the persisted plan, sizes the tip to
/// the live floor at level = `submit_attempts` (higher than the attempt that just
/// lost), and re-submits. The leg wallets stay `reserved` throughout — a re-bid is
/// not a terminal outcome. On a build/submit error `execute_bundle` leaves the
/// bundle `failed` (its own rollback); the reservation TTL sweep later reclaims the
/// wallets. Errors surface to the caller's `warn!`.
async fn rebid_dropped(
    pool: &PgPool,
    settings: &LauncherSettings,
    bundle: &Bundle,
) -> anyhow::Result<()> {
    warn!(
        bundle_id = %bundle.id,
        attempts = bundle.submit_attempts,
        max_retries = settings.bundle_max_retries,
        "bundle dropped — re-bidding at a higher Jito tip"
    );
    BundleRepo::reset_for_rebid(pool, bundle.id).await?;
    let res = execute_bundle(pool, settings, bundle.id).await?;
    info!(
        bundle_id = %bundle.id,
        jito_bundle_id = %res.jito_bundle_id,
        "dropped bundle re-submitted at a higher tip"
    );
    Ok(())
}

/// Wallet-pool lifecycle: a bundle's leg wallets are terminal (`used`) once the
/// bundle reaches ANY terminal outcome (landed/dropped/partial) — a dropped or
/// partial bundle still spent its legs' fees/nonces on a real submit attempt, so
/// they're not safely re-claimable. A no-op (`WHERE status = 'reserved'` guard in
/// `mark_used`) for wallets not currently reserved, i.e. today's free-form
/// template-selected legs, until launch execution claims via `claim_funded`.
/// Seed the cost-basis position for every leg of a LANDED bundle (idempotent),
/// mirroring the dev-buy seed at launch. This gives the dust sweep's
/// open-position guard a row to protect the instant the bundle lands, rather than
/// waiting for the first holdings-page read to lazily seed it — otherwise these
/// token-holding bundler wallets, now `used`, could be drained + retired by the
/// hourly sweep before the token is ever viewed, stranding their sell (no gas to
/// pay the fee → "confirmation timed out"). Only landed legs bought tokens; a
/// dropped/partial bundle's legs hold nothing and stay sweep-eligible.
async fn seed_landed_bundle_positions(pool: &PgPool, mint_address: &str, bundle: &Bundle) {
    let legs = match legs_from_json(&bundle.legs) {
        Ok(legs) => legs,
        Err(e) => {
            warn!(bundle_id = %bundle.id, %e, "failed to parse bundle legs for position seed");
            return;
        }
    };
    for leg in legs {
        if let Err(e) = TokenPositionRepo::seed(
            pool,
            mint_address,
            leg.managed_wallet_id,
            WalletRole::Bundler.as_str(),
            leg.quote_amount,
        )
        .await
        {
            warn!(bundle_id = %bundle.id, managed_wallet_id = %leg.managed_wallet_id, %e, "failed to seed bundler position");
        }
    }
}

async fn mark_bundle_wallets_used(pool: &PgPool, bundle: &Bundle) {
    let wallet_ids = match legs_from_json(&bundle.legs) {
        Ok(legs) => legs.iter().map(|leg| leg.managed_wallet_id).collect::<Vec<_>>(),
        Err(e) => {
            warn!(bundle_id = %bundle.id, %e, "failed to parse bundle legs for wallet-used transition");
            return;
        }
    };
    if let Err(e) = ManagedWalletRepo::mark_used(pool, &wallet_ids).await {
        warn!(bundle_id = %bundle.id, %e, "failed to mark bundler wallets used");
    }
}
