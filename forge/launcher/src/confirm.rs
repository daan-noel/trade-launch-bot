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
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use chrono::Utc;
use sqlx::PgPool;
use tokio::sync::Notify;
use tracing::{info, warn};

use platform_core::models::{Bundle, BundleStatus, Launch, LaunchStatus, WalletRole};
use platform_core::storage::repositories::{
    BundleRepo, LaunchRepo, ManagedWalletRepo, TokenPositionRepo, TradeRepo,
};

use crate::bundle::legs_from_json;
use crate::config::LauncherSettings;
use crate::events::{EventSink, LaunchStatusEvent};
use crate::{execute_bundle, keystore};

/// Fallback re-check cadence. The watcher is normally woken by `trades_notify`
/// the instant the ingest DbWriter commits new trades (notify over poll); this
/// slow tick is only the backstop that still advances the time-driven
/// timeout → dropped → rebid path when no trades are flowing.
const FALLBACK_INTERVAL: Duration = Duration::from_secs(10);
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
///
/// `sink` (when the live bin wires one) receives each terminal launch/bundle
/// transition as a push event so the Launch Console updates without polling; a
/// `None` sink makes every emit a no-op (CLI paths, tests).
pub fn spawn_bundle_confirm_watcher(
    pool: PgPool,
    settings: Option<LauncherSettings>,
    trades_notify: Arc<Notify>,
    sink: Option<Arc<dyn EventSink>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(FALLBACK_INTERVAL);
        loop {
            // Wake on a fresh trade commit (fast-path landing detection) OR the
            // slow fallback tick (time-driven timeout/rebid). `notify_one` stores a
            // permit if we're mid-pass, so a commit during processing isn't missed.
            tokio::select! {
                _ = trades_notify.notified() => {}
                _ = tick.tick() => {}
            }
            if let Err(e) = confirm_pending(&pool, settings.as_ref(), sink.as_deref()).await {
                warn!(?e, "bundle confirm pass failed");
            }
        }
    })
}

async fn confirm_pending(
    pool: &PgPool,
    settings: Option<&LauncherSettings>,
    sink: Option<&dyn EventSink>,
) -> anyhow::Result<()> {
    let pending = BundleRepo::find_awaiting_confirmation(pool).await?;
    if pending.is_empty() {
        return Ok(());
    }

    // Batch-load the launches for this tick's pending bundles in ONE query instead
    // of a per-bundle `LaunchRepo::get` (audit §5). The set is tiny (only bundles
    // awaiting confirmation), so a HashMap by id is ample.
    let launch_ids: Vec<uuid::Uuid> = pending.iter().map(|b| b.launch_id).collect();
    let launches: std::collections::HashMap<uuid::Uuid, Launch> =
        LaunchRepo::get_many(pool, &launch_ids)
            .await?
            .into_iter()
            .map(|l| (l.id, l))
            .collect();

    for bundle in pending {
        let id = bundle.id;
        let Some(launch) = launches.get(&bundle.launch_id).cloned() else {
            warn!(bundle_id = %id, launch_id = %bundle.launch_id, "launch not found for bundle — skipping");
            continue;
        };
        if let Err(e) = confirm_one(pool, bundle, launch, settings, sink).await {
            warn!(bundle_id = %id, ?e, "bundle confirm check failed");
        }
    }
    Ok(())
}

async fn confirm_one(
    pool: &PgPool,
    bundle: Bundle,
    launch: Launch,
    settings: Option<&LauncherSettings>,
    sink: Option<&dyn EventSink>,
) -> anyhow::Result<()> {
    // `leg_signatures` is the CO-BUY set (the create leg is `tx0`, its signature is
    // on the launch). The bundle is atomic, so all co-buys present in the feed ⟹ the
    // create landed too.
    let sigs: Vec<Vec<u8>> = bundle
        .leg_signatures
        .iter()
        .map(|s| bs58::decode(s).into_vec())
        .collect::<Result<_, _>>()
        .context("decode leg signature")?;
    if sigs.is_empty() {
        warn!(bundle_id = %bundle.id, "submitted bundle has no co-buy signatures — marking dropped");
        finalize_dropped(pool, &bundle, &launch, settings, sink).await;
        return Ok(());
    }

    let landed: HashSet<Vec<u8>> =
        TradeRepo::find_signatures_present(pool, &launch.mint_address, &sigs).await?;

    if landed.len() == sigs.len() {
        finalize_landed(pool, &bundle, &launch, settings, sink).await;
        info!(bundle_id = %bundle.id, legs = sigs.len(), "atomic launch bundle landed (create + co-buys)");
        return Ok(());
    }

    let submitted_at = bundle
        .submitted_at
        .context("submitted bundle missing submitted_at")?;
    if Utc::now() - submitted_at <= CONFIRM_TIMEOUT {
        return Ok(()); // still within the landing window, check again next tick
    }

    if landed.is_empty() {
        // Auto re-bid: the atomic bundle lost the Jito auction (accepted but not
        // included) — create AND co-buys, all-or-nothing, so nothing was created and
        // nothing spent. Re-submit the WHOLE bundle at a higher tip before conceding;
        // `submit_attempts` both bounds the retries and drives the escalation level.
        // Wallets stay `reserved` across the re-bid.
        if let Some(settings) = settings {
            if (bundle.submit_attempts as u32) <= settings.bundle_max_retries {
                // A successful re-bid returns the bundle to `submitted`; the watcher
                // picks it up again next tick. But if the re-bid itself can't
                // re-submit, `execute_bundle` leaves the bundle terminal `failed` —
                // and this watcher only ever revisits `submitted` bundles
                // (`find_awaiting_confirmation`), so nothing else would ever reconcile
                // the launch off `pending` or release its reserved wallets. Own the
                // terminal transition here, exactly as a clean drop would: fail the
                // launch + release the dev/bundler wallets.
                if let Err(e) = rebid_dropped(pool, settings, &bundle).await {
                    warn!(
                        bundle_id = %bundle.id,
                        attempts = bundle.submit_attempts,
                        ?e,
                        "atomic launch bundle re-bid failed to re-submit — finalizing as dropped"
                    );
                    finalize_dropped(pool, &bundle, &launch, Some(settings), sink).await;
                }
                return Ok(());
            }
        }
        warn!(
            bundle_id = %bundle.id,
            attempts = bundle.submit_attempts,
            "atomic launch bundle dropped — nothing landed within timeout (retries exhausted)"
        );
        finalize_dropped(pool, &bundle, &launch, settings, sink).await;
    } else {
        // Atomicity anomaly: a Jito bundle should never land partially. The create
        // co-landed (it's tx0), so the launch IS created — mark it so, but flag the
        // partial for investigation.
        warn!(
            bundle_id = %bundle.id,
            landed = landed.len(),
            total = sigs.len(),
            "atomic launch bundle partially landed — Jito atomicity anomaly, investigate"
        );
        finalize_partial(pool, &bundle, &launch, settings, sink).await;
    }
    Ok(())
}

/// Push one terminal launch/bundle transition to the frontend (no-op without a
/// sink). Called after the DB writes so a subscriber that refetches off it sees
/// the settled row.
fn emit_launch_status(
    sink: Option<&dyn EventSink>,
    launch: &Launch,
    launch_status: LaunchStatus,
    bundle: &Bundle,
    bundle_status: BundleStatus,
) {
    if let Some(sink) = sink {
        sink.launch_status(&LaunchStatusEvent {
            launch_id: launch.id,
            mint_address: launch.mint_address.clone(),
            launch_status: launch_status.as_str().to_string(),
            bundle_id: Some(bundle.id),
            bundle_status: Some(bundle_status.as_str().to_string()),
        });
    }
}

/// Terminal LANDED: the token is live and every co-buy filled. Flip the bundle +
/// launch to their success states, seed the dev + bundler cost-basis positions,
/// mark the wallets `used`, and delete the persisted mint key.
async fn finalize_landed(
    pool: &PgPool,
    bundle: &Bundle,
    launch: &Launch,
    settings: Option<&LauncherSettings>,
    sink: Option<&dyn EventSink>,
) {
    if let Err(e) = BundleRepo::set_confirmed(pool, bundle.id, BundleStatus::Landed.as_str()).await {
        warn!(bundle_id = %bundle.id, %e, "failed to set bundle landed");
    }
    // The create leg's signature was stamped on the launch at submit — promote the
    // launch to `created` now that it actually landed.
    let sig = launch.create_signature.clone().unwrap_or_default();
    if let Err(e) =
        LaunchRepo::set_created(pool, launch.id, &sig, LaunchStatus::Created.as_str()).await
    {
        warn!(launch_id = %launch.id, %e, "failed to set launch created after bundle landed");
    }
    seed_dev_position(pool, launch).await;
    seed_landed_bundle_positions(pool, &launch.mint_address, bundle).await;
    mark_bundle_wallets_used(pool, bundle).await;
    mark_dev_wallet_used(pool, launch).await;
    delete_mint_key(settings, launch.id);
    emit_launch_status(sink, launch, LaunchStatus::Created, bundle, BundleStatus::Landed);
}

/// Terminal DROPPED (retries exhausted): the atomic bundle never landed, so the
/// token was never created and NO SOL was spent (a dropped Jito bundle executes no
/// txs). Fail the launch and RELEASE the dev + bundler wallets back to `funded`
/// (they're fully reusable), and delete the persisted mint key.
async fn finalize_dropped(
    pool: &PgPool,
    bundle: &Bundle,
    launch: &Launch,
    settings: Option<&LauncherSettings>,
    sink: Option<&dyn EventSink>,
) {
    if let Err(e) = BundleRepo::set_confirmed(pool, bundle.id, BundleStatus::Dropped.as_str()).await
    {
        warn!(bundle_id = %bundle.id, %e, "failed to set bundle dropped");
    }
    if let Err(e) = LaunchRepo::set_failed(pool, launch.id, LaunchStatus::Failed.as_str()).await {
        warn!(launch_id = %launch.id, %e, "failed to set launch failed after bundle dropped");
    }
    release_bundle_wallets(pool, bundle, launch).await;
    delete_mint_key(settings, launch.id);
    emit_launch_status(sink, launch, LaunchStatus::Failed, bundle, BundleStatus::Dropped);
}

/// PARTIAL (Jito atomicity anomaly — should not happen): some co-buys landed, so
/// the create (tx0) landed too. Treat the launch as created, seed what landed, mark
/// the wallets `used` (they spent), and delete the mint key. Left flagged by the
/// caller's `warn!` for investigation.
async fn finalize_partial(
    pool: &PgPool,
    bundle: &Bundle,
    launch: &Launch,
    settings: Option<&LauncherSettings>,
    sink: Option<&dyn EventSink>,
) {
    if let Err(e) = BundleRepo::set_confirmed(pool, bundle.id, BundleStatus::Partial.as_str()).await
    {
        warn!(bundle_id = %bundle.id, %e, "failed to set bundle partial");
    }
    let sig = launch.create_signature.clone().unwrap_or_default();
    if let Err(e) =
        LaunchRepo::set_created(pool, launch.id, &sig, LaunchStatus::Created.as_str()).await
    {
        warn!(launch_id = %launch.id, %e, "failed to set launch created after partial bundle");
    }
    seed_dev_position(pool, launch).await;
    seed_landed_bundle_positions(pool, &launch.mint_address, bundle).await;
    mark_bundle_wallets_used(pool, bundle).await;
    mark_dev_wallet_used(pool, launch).await;
    delete_mint_key(settings, launch.id);
    emit_launch_status(sink, launch, LaunchStatus::Created, bundle, BundleStatus::Partial);
}

/// Seed the dev cost-basis position for a landed launch (idempotent) — the dev-buy
/// fused into the create leg. No dev-buy ⇒ nothing held ⇒ nothing to seed.
async fn seed_dev_position(pool: &PgPool, launch: &Launch) {
    let (Some(dev_wallet_id), Some(dev_buy)) = (launch.dev_wallet_id, launch.dev_buy_quote) else {
        return;
    };
    if dev_buy <= 0 {
        return;
    }
    if let Err(e) = TokenPositionRepo::seed(
        pool,
        &launch.mint_address,
        dev_wallet_id,
        WalletRole::Dev.as_str(),
        dev_buy,
    )
    .await
    {
        warn!(launch_id = %launch.id, %e, "failed to seed dev position on landing");
    }
}

/// Mark the dev wallet `used` on a terminal spend (landed/partial).
async fn mark_dev_wallet_used(pool: &PgPool, launch: &Launch) {
    if let Some(dev_wallet_id) = launch.dev_wallet_id {
        if let Err(e) = ManagedWalletRepo::mark_used(pool, &[dev_wallet_id]).await {
            warn!(launch_id = %launch.id, %e, "failed to mark dev wallet used");
        }
    }
}

/// Release the dev + bundler wallets back to `funded` (a dropped atomic bundle spent
/// nothing — they're reusable).
async fn release_bundle_wallets(pool: &PgPool, bundle: &Bundle, launch: &Launch) {
    let mut ids = match legs_from_json(&bundle.legs) {
        Ok(legs) => legs.iter().map(|leg| leg.managed_wallet_id).collect::<Vec<_>>(),
        Err(e) => {
            warn!(bundle_id = %bundle.id, %e, "failed to parse bundle legs for wallet release");
            Vec::new()
        }
    };
    if let Some(dev_wallet_id) = launch.dev_wallet_id {
        ids.push(dev_wallet_id);
    }
    if !ids.is_empty() {
        if let Err(e) = ManagedWalletRepo::release_reservation(pool, &ids).await {
            warn!(bundle_id = %bundle.id, %e, "failed to release bundle wallets after drop");
        }
    }
}

/// Delete the persisted (envelope-encrypted) mint key on a terminal outcome.
/// A no-op when the watcher is running without settings (can't reach the keystore
/// dir) — the file is small and the reservation sweep is the wider backstop.
fn delete_mint_key(settings: Option<&LauncherSettings>, launch_id: uuid::Uuid) {
    let Some(settings) = settings else { return };
    let key_ref = keystore::mint_key_ref(launch_id);
    if let Err(e) = keystore::delete_mint_key(&settings.keystore_dir, &key_ref) {
        warn!(%launch_id, %e, "failed to delete mint key on terminal outcome");
    }
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

/// Seed the cost-basis position for every leg of a LANDED (or partial) bundle
/// (idempotent), mirroring the dev-buy seed. This gives the dust sweep's
/// open-position guard a row to protect the instant the bundle lands, rather than
/// waiting for the first holdings-page read to lazily seed it — otherwise these
/// token-holding bundler wallets, now `used`, could be drained + retired by the
/// hourly sweep before the token is ever viewed, stranding their sell (no gas to
/// pay the fee → "confirmation timed out"). Only landed legs bought tokens; a fully
/// dropped atomic bundle's legs hold nothing and are released back to `funded`
/// (see [`release_bundle_wallets`]), not seeded here.
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

/// Terminal `reserved` -> `used` for a bundle's LEG (bundler) wallets on a spend
/// (landed/partial) — they bought tokens, so they're not re-claimable. The dev
/// wallet is transitioned separately ([`mark_dev_wallet_used`]). A no-op for wallets
/// not currently `reserved`.
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

#[cfg(test)]
mod tests {
    use super::*;
    use platform_core::config::Settings;
    use platform_core::models::{NewManagedWallet, WalletStatus};
    use platform_core::storage::connect;
    use std::path::PathBuf;
    use uuid::Uuid;

    /// A minimal `LauncherSettings` for the confirm pass. The re-bid it triggers
    /// bails inside `execute_bundle` (the bundle has no `create_args`) BEFORE any
    /// keystore / RPC / Jito call, so these values are never dereferenced — only
    /// `bundle_max_retries` matters (it must admit the re-bid).
    fn dummy_settings() -> LauncherSettings {
        LauncherSettings {
            rpc_url: "http://127.0.0.1:1".to_string(),
            sender_urls: vec![],
            nonce_accounts: vec![],
            keystore_dir: PathBuf::from("/nonexistent-keystore"),
            kek_passphrase: "test".to_string(),
            jito_block_engine_url: "http://127.0.0.1:1".to_string(),
            jito_block_engine_urls: vec!["http://127.0.0.1:1".to_string()],
            bundle_max_retries: 2,
            jito_min_tip_sol: 0.001,
            jito_max_tip_sol: 0.05,
            jito_tip_percentile: 50,
            leader_gate: crate::config::LeaderGateConfig {
                enabled: false,
                max_wait_ms: 0,
                send_within_slots: 2,
            },
            launch_alt: None,
            backup_dir: None,
            pinata_jwt: None,
            funding: None,
            export_secret: None,
            manage: None,
            allow_fingerprint: true,
        }
    }

    fn mk_wallet(addr: &str, role: WalletRole) -> NewManagedWallet {
        NewManagedWallet {
            address: addr.to_string(),
            label: None,
            role: role.as_str().to_string(),
            key_ref: format!("{addr}.enc"),
            derivation_index: None,
        }
    }

    /// Regression (Fix 1): when the background re-bid can't re-submit a dropped
    /// atomic bundle, `execute_bundle` leaves the bundle terminal `failed`. The
    /// confirm watcher only ever revisits `submitted` bundles
    /// (`find_awaiting_confirmation`), so nothing else would touch it again —
    /// previously stranding the launch at `pending` with its wallets still
    /// `reserved` (the exact bug seen in prod: launch=pending, bundle=failed, dev
    /// buy "open"). The confirm pass must instead OWN the terminal transition: fail
    /// the launch and release the dev + bundler wallets back to `funded`.
    ///
    /// DB-gated: set `PLATFORM_TEST_DATABASE_URL` to a DEDICATED throwaway DB (runs
    /// migrations + mutates rows). Self-skips otherwise.
    #[tokio::test]
    async fn rebid_failure_finalizes_launch_and_releases_wallets() {
        let Ok(url) = std::env::var("PLATFORM_TEST_DATABASE_URL") else {
            eprintln!("SKIP confirm rebid_failure: set PLATFORM_TEST_DATABASE_URL to run it");
            return;
        };
        std::env::set_var("DATABASE_URL", &url);
        let settings = Settings::from_env().expect("settings");
        let pools = connect(&settings).await.expect("connect + migrate");
        let pool = &pools.hot;

        let tag = Uuid::new_v4();
        let mint = format!("REBIDTEST_{tag}");

        // dev + one bundler wallet, both reserved to the launch below.
        let dev = ManagedWalletRepo::insert(pool, &mk_wallet(&format!("REBIDTEST_dev_{tag}"), WalletRole::Dev))
            .await
            .unwrap();
        let bundler =
            ManagedWalletRepo::insert(pool, &mk_wallet(&format!("REBIDTEST_bnd_{tag}"), WalletRole::Bundler))
                .await
                .unwrap();

        let launch_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO launches (id, mint_address, launchpad_id, variant, quote_asset_id, \
             dev_wallet_id, dev_buy_quote, status) \
             VALUES ($1, $2, 1, 'pumpfun.create_v2', 1, $3, 20000000, 'pending')",
        )
        .bind(launch_id)
        .bind(&mint)
        .bind(dev.id)
        .execute(pool)
        .await
        .unwrap();

        for w in [dev.id, bundler.id] {
            sqlx::query(
                "UPDATE managed_wallets SET status='reserved', reserved_by_launch_id=$2, \
                 reserved_at=now() WHERE id=$1",
            )
            .bind(w)
            .bind(launch_id)
            .execute(pool)
            .await
            .unwrap();
        }

        // A `submitted` bundle past the confirm timeout, with a co-buy signature that
        // never appears in `trades` (no fills) → the pass declares it dropped and
        // re-bids. `create_args` is NULL, so `execute_bundle` bails immediately —
        // the deterministic "re-bid can't re-submit" failure this test guards.
        // `submit_attempts=0` keeps it under `bundle_max_retries`, so the re-bid
        // branch is taken rather than the retries-exhausted one.
        let bundle_id = Uuid::new_v4();
        let legs = serde_json::json!([{
            "structure": {"layout": ["create_ata","core","cu_price","tip"], "variant": "buy_exact_sol_in",
                "cu_limit": 1, "cu_price": 1, "ix_order": 0, "tip_quote": 0, "slippage_bps": 500},
            "quote_amount": 10_000_000,
            "managed_wallet_id": bundler.id,
        }]);
        sqlx::query(
            "INSERT INTO bundles (id, launch_id, status, legs, leg_signatures, submitted_at, submit_attempts) \
             VALUES ($1, $2, 'submitted', $3, $4, now() - interval '10 minutes', 0)",
        )
        .bind(bundle_id)
        .bind(launch_id)
        .bind(&legs)
        // Any valid base58 string that is not a real landed signature.
        .bind(vec!["4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi".to_string()])
        .execute(pool)
        .await
        .unwrap();
        sqlx::query("UPDATE launches SET bundle_id=$2 WHERE id=$1")
            .bind(launch_id)
            .bind(bundle_id)
            .execute(pool)
            .await
            .unwrap();

        // One confirm pass: timeout → drop → re-bid → re-bid fails → finalize dropped.
        confirm_pending(pool, Some(&dummy_settings()), None).await.unwrap();

        let launch = LaunchRepo::get(pool, launch_id).await.unwrap().unwrap();
        assert_eq!(
            launch.status,
            LaunchStatus::Failed.as_str(),
            "launch must be finalized `failed`, not stranded at `pending`"
        );
        let bundle = BundleRepo::get(pool, bundle_id).await.unwrap().unwrap();
        assert_eq!(
            bundle.status,
            BundleStatus::Dropped.as_str(),
            "bundle must be terminal `dropped` after the failed re-bid"
        );
        for id in [dev.id, bundler.id] {
            let w = ManagedWalletRepo::get(pool, id).await.unwrap().unwrap();
            assert_eq!(
                w.status,
                WalletStatus::Funded.as_str(),
                "wallet must be released back to `funded`"
            );
            assert!(w.reserved_by_launch_id.is_none(), "reservation must be cleared");
        }

        // Cleanup — the bundle cascades on the launch delete.
        sqlx::query("DELETE FROM launches WHERE id=$1")
            .bind(launch_id)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM managed_wallets WHERE id = ANY($1)")
            .bind(vec![dev.id, bundler.id])
            .execute(pool)
            .await
            .unwrap();
    }
}
