//! Orchestrated admin reseed of DB-backed in-memory caches on the live box.
//!
//! Does **not** wipe feed-sourced state (armed registry, producer cursors,
//! blockhash/nonce caches). For a full cold rebuild, restart the process.

use std::sync::Arc;

use serde::Serialize;
use tracing::info;

use trading_core::models::ingest::SseEvent;
use trading_core::storage::repositories::wallet_dict_repo;

use crate::seed;
use crate::services::amm_pool_facts;
use crate::state::deploy_state::DeployState;
use crate::strategies::engine::EngineReloadError;

#[derive(Debug, Clone, Serialize)]
pub struct ReloadStep {
    pub name: &'static str,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReloadCachesResponse {
    pub ok: bool,
    pub steps: Vec<ReloadStep>,
}

fn step_ok(name: &'static str, detail: Option<String>) -> ReloadStep {
    ReloadStep { name, ok: true, detail }
}

fn step_err(name: &'static str, detail: String) -> ReloadStep {
    ReloadStep {
        name,
        ok: false,
        detail: Some(detail),
    }
}

fn engine_reload_detail(e: EngineReloadError) -> String {
    e.to_string()
}

/// Re-read every PG-backed cache the live process can safely hot-reseed.
pub async fn reload_all(state: &DeployState) -> ReloadCachesResponse {
    let mut steps = Vec::new();

    // 1. Settings document (also keeps live_mode watch in sync).
    match state.settings_repo().load_all().await {
        Ok(loaded) => {
            let live = loaded.live;
            let _ = state.settings.send_replace(loaded);
            if live != state.is_live() {
                state.set_live(live);
            }
            steps.push(step_ok("settings", None));
        }
        Err(e) => steps.push(step_err("settings", e.to_string())),
    }

    // 2. Strategy engine: rules + PG position adopt + episode counters.
    match state.engine.reseed_from_db().await {
        Ok(report) => steps.push(step_ok(
            "engine",
            Some(format!(
                "rules={} holdings={} buy_submitted={} episodes={}",
                report.rules,
                report.holdings_adopted,
                report.buy_submitted_adopted,
                report.episodes_seeded,
            )),
        )),
        Err(e) => steps.push(step_err("engine", engine_reload_detail(e))),
    }

    // 3. Token cache seed (merge — never clobbers live-created mints).
    match seed::seed_token_cache(&state.db, state.token_cache.clone()).await {
        Ok(outcome) => {
            for mint in &outcome.held_mints {
                state.held_pools.note(mint);
            }
            state
                .held_pools
                .track_migrated_many(&outcome.held_migrated_mints);
            amm_pool_facts::seed_from_db(
                &state.trader,
                &state.db,
                &outcome.held_migrated_mints,
            )
            .await;
            steps.push(step_ok(
                "token_cache",
                Some(format!(
                    "held_mints={} held_migrated={}",
                    outcome.held_mints.len(),
                    outcome.held_migrated_mints.len(),
                )),
            ));
        }
        Err(e) => steps.push(step_err("token_cache", e.to_string())),
    }

    // 4. Wallet interning perf cache (self-healing on next miss).
    wallet_dict_repo::clear_wallet_id_cache();
    steps.push(step_ok("wallet_dict", None));

    // 5. Short-TTL API composition caches.
    state.holdings_cache.invalidate().await;
    state.cashback_cache.invalidate().await;
    steps.push(step_ok("portfolio_caches", None));

    let ok = steps.iter().all(|s| s.ok);
    if ok {
        let _ = state.sse_tx.send(SseEvent::TpslRulesChanged {
            strategy: "generic".into(),
        });
        info!("admin reload-caches: all steps ok");
    } else {
        tracing::warn!(
            failed = steps.iter().filter(|s| !s.ok).count(),
            "admin reload-caches: completed with failures"
        );
    }

    ReloadCachesResponse { ok, steps }
}

/// Type alias for the handler injection point.
pub type DeployStateData = actix_web::web::Data<Arc<DeployState>>;
