//! Feed-based bundle-landing confirmation (LIVE box).
//!
//! Jito `sendBundle` only reports acceptance into the block-engine queue, not
//! landing — the same gap meme-trading's sell-confirm closes by trusting the
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

use platform_core::models::Bundle;
use platform_core::storage::repositories::{BundleRepo, LaunchRepo, TradeRepo};

/// How often to re-check bundles awaiting confirmation.
const POLL_INTERVAL: Duration = Duration::from_secs(3);
/// How long to wait for a bundle to land before declaring it dropped.
const CONFIRM_TIMEOUT: chrono::Duration = chrono::Duration::seconds(90);

/// Spawn the watcher as a long-lived task. Cheap when idle — the query is
/// bounded to the tiny `status = 'submitted'` set (see the partial index in
/// migration `0003`), never a full-table scan.
pub fn spawn_bundle_confirm_watcher(pool: PgPool) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(POLL_INTERVAL);
        loop {
            tick.tick().await;
            if let Err(e) = confirm_pending(&pool).await {
                warn!(?e, "bundle confirm pass failed");
            }
        }
    })
}

async fn confirm_pending(pool: &PgPool) -> anyhow::Result<()> {
    let pending = BundleRepo::find_awaiting_confirmation(pool).await?;
    for bundle in pending {
        let id = bundle.id;
        if let Err(e) = confirm_one(pool, bundle).await {
            warn!(bundle_id = %id, ?e, "bundle confirm check failed");
        }
    }
    Ok(())
}

async fn confirm_one(pool: &PgPool, bundle: Bundle) -> anyhow::Result<()> {
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
        BundleRepo::set_confirmed(pool, bundle.id, "dropped").await?;
        return Ok(());
    }

    let landed: HashSet<Vec<u8>> =
        TradeRepo::find_signatures_present(pool, &launch.mint_address, &sigs).await?;

    if landed.len() == sigs.len() {
        BundleRepo::set_confirmed(pool, bundle.id, "landed").await?;
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
        BundleRepo::set_confirmed(pool, bundle.id, "dropped").await?;
        warn!(bundle_id = %bundle.id, "bundle dropped — no legs landed within timeout");
    } else {
        // Atomicity anomaly: a Jito bundle should never land partially.
        BundleRepo::set_confirmed(pool, bundle.id, "partial").await?;
        warn!(
            bundle_id = %bundle.id,
            landed = landed.len(),
            total = sigs.len(),
            "bundle partially landed — Jito atomicity anomaly, investigate"
        );
    }
    Ok(())
}
