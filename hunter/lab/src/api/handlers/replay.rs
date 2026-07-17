//! Event-log replay inspector (plan 6.1) — the time-travel debugger backend. Loads a
//! recorded live event log, re-runs the pure engine [`reduce`](hunter_engine::reduce)
//! over it, and returns every `event → effects` decision as JSON so a past live run
//! can be reproduced and inspected offline (frontend viewer = FE plan FE6).
//!
//! Rules come from PG (the log omits `RulesReloaded`), so an inspection replays the
//! recorded events against the *current* rule set — see the module docs on
//! [`crate::strategies::replay_inspect`] for the slicing/parity caveats.

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;
use uuid::Uuid;

use hunter_engine::event::LoadedRule;
use hunter_engine::fingerprint::Fingerprint as EngineFingerprint;
use hunter_engine::metrics::Ts;

use trading_core::storage::repositories::fingerprint_repo::FingerprintRepo;
use trading_core::storage::repositories::rule_repo::RuleRepo;
use trading_core::strategies::fingerprint_axes::{fp_to_engine, rule_to_loaded};

use crate::state::local_state::LocalState;
use crate::strategies::replay_inspect::{self, InspectConfig, DEFAULT_MAX_STEPS};

/// Request body for `POST /api/replay/inspect`.
#[derive(Debug, Deserialize)]
pub struct InspectRequest {
    /// Log directory (default: `EVENT_LOG_DIR` env, else `event_log`).
    #[serde(default)]
    pub dir: Option<String>,
    /// A single `YYYY-MM-DD` day-file to load; omitted ⇒ every day-file in `dir`.
    #[serde(default)]
    pub date: Option<String>,
    /// Only dump steps touching this mint (the whole log is still folded).
    #[serde(default)]
    pub mint: Option<String>,
    /// Only dump steps at/after this instant (RFC3339).
    #[serde(default)]
    pub since: Option<Ts>,
    /// Only dump steps at/before this instant (RFC3339).
    #[serde(default)]
    pub until: Option<Ts>,
    /// Interleave synthetic 500 ms ticks so tick-driven decisions reproduce (default
    /// `true`; set `false` to see only the logged events' direct effects).
    #[serde(default = "default_true")]
    pub synthetic_ticks: bool,
    /// Replay against only active rules (default `false` ⇒ all rules, so a since-paused
    /// rule that fired in the log still arms).
    #[serde(default)]
    pub active_only: bool,
    /// Restrict the loaded rule set to these ids (default: all).
    #[serde(default)]
    pub rule_ids: Option<Vec<Uuid>>,
    /// Cap on dumped steps (default [`DEFAULT_MAX_STEPS`]).
    #[serde(default)]
    pub max_steps: Option<usize>,
}

fn default_true() -> bool {
    true
}

/// `POST /api/replay/inspect` — replay a recorded event log through the engine and
/// dump every `event → effects` decision.
pub async fn inspect_replay(
    state: web::Data<Arc<LocalState>>,
    body: web::Json<InspectRequest>,
) -> impl Responder {
    let req = body.into_inner();

    // Load rules + fingerprints from PG (the log carries neither).
    let rule_repo = RuleRepo::new(state.db.clone());
    let fp_repo = FingerprintRepo::new(state.db.clone());

    let rule_rows = match if req.active_only { rule_repo.list_active().await } else { rule_repo.list().await } {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("replay-inspect: load rules failed: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": format!("load rules failed: {e}") }));
        }
    };
    let fp_rows = match fp_repo.list().await {
        Ok(f) => f,
        Err(e) => {
            tracing::error!("replay-inspect: load fingerprints failed: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": format!("load fingerprints failed: {e}") }));
        }
    };

    let wanted: Option<std::collections::HashSet<Uuid>> =
        req.rule_ids.as_ref().map(|ids| ids.iter().copied().collect());
    let rules: Vec<LoadedRule> = rule_rows
        .iter()
        .filter(|r| wanted.as_ref().is_none_or(|w| w.contains(&r.id)))
        .filter_map(|r| match rule_to_loaded(r) {
            Ok(l) => Some(l),
            Err(e) => {
                tracing::warn!(rule = %r.id, "replay-inspect: skipping rule with invalid params: {e}");
                None
            }
        })
        .collect();
    let fps: Vec<EngineFingerprint> = fp_rows.iter().map(fp_to_engine).collect();

    let cfg = InspectConfig {
        mint: req.mint,
        since: req.since,
        until: req.until,
        synthetic_ticks: req.synthetic_ticks,
        max_steps: req.max_steps.unwrap_or(DEFAULT_MAX_STEPS).max(1),
    };
    let dir = replay_inspect::resolve_dir(req.dir.as_deref());
    let dir_str = dir.display().to_string();
    let date = req.date;

    // File read + the reduce fold are CPU/IO-bound — run off the async worker.
    let outcome = web::block(move || {
        let (files, events) = replay_inspect::read_logs(&dir, date.as_deref())?;
        let run = replay_inspect::inspect(&rules, &fps, events, &cfg);
        Ok::<_, std::io::Error>((files, run))
    })
    .await;

    match outcome {
        Ok(Ok((files, run))) => {
            let mut value = serde_json::to_value(&run).unwrap_or_else(|_| serde_json::json!({}));
            if let Some(obj) = value.as_object_mut() {
                obj.insert("dir".to_string(), serde_json::json!(dir_str));
                obj.insert("files".to_string(), serde_json::json!(files));
            }
            HttpResponse::Ok().json(value)
        }
        Ok(Err(e)) => {
            tracing::error!("replay-inspect: read log dir {dir_str:?} failed: {e}");
            HttpResponse::BadRequest()
                .json(serde_json::json!({ "error": format!("read log dir failed: {e}"), "dir": dir_str }))
        }
        Err(e) => {
            tracing::error!("replay-inspect: fold task failed: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "replay fold failed" }))
        }
    }
}
