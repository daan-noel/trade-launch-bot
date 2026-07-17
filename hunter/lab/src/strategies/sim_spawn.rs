//! Detached simulation job spawn — shared by tpsl1/tpsl2/swing1 handlers.
//!
//! Fast path: when the rule's [`sim_key`](trading_core::strategies::match_keys::sim_key)
//! and analysis window match a cached [`SimOutcome::Done`], fire
//! `simulation_finished` immediately (no scan, no lake load, no trade walk).

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use actix_web::{web, HttpResponse};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::state::local_state::LocalState;
use crate::state::sim_results::SimOutcome;
use crate::state::sim_summary::SimSummary;
use trading_core::models::ingest::SseEvent;
use trading_core::models::LegacyStrategyRule;
use trading_core::strategies::match_keys::sim_key;

type BacktestFut =
    Pin<Box<dyn Future<Output = Result<Vec<serde_json::Value>, anyhow::Error>> + Send>>;

/// Start (or instantly replay) a rule simulation. `run_backtest` runs on a cache
/// miss; on a hit the stored rows are reused and the SSE fires at once.
pub async fn spawn_rule_simulation(
    app_state: web::Data<Arc<LocalState>>,
    rid: Uuid,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    strategy_rule: &LegacyStrategyRule,
    run_backtest: impl FnOnce(
        web::Data<Arc<LocalState>>,
        Uuid,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
        Arc<std::sync::atomic::AtomicBool>,
        Arc<crate::state::job_progress::ProgressCell>,
    ) -> BacktestFut
    + Send
    + 'static,
) -> HttpResponse {
    let key = sim_key(strategy_rule);
    if let Some(SimOutcome::Done(_)) =
        app_state
            .sim_results
            .peek_if(&rid, &key, since, until)
    {
        tracing::info!(%rid, "simulation cache hit — skipping backtest");
        let sse = app_state.sse_tx.clone();
        actix_web::rt::spawn(async move {
            let _ = sse.send(SseEvent::SimulationFinished {
                rule_id: rid,
                cancelled: false,
            });
        });
        return HttpResponse::Accepted().json(serde_json::json!({ "started": true, "cached": true }));
    }

    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cell = Arc::new(crate::state::job_progress::ProgressCell::default());
    app_state.sim_cancels.insert(rid, cancel.clone());
    app_state.sim_progress.insert(rid, cell.clone());
    app_state.sim_results.clear(&rid);

    let sim_key_owned = key;
    actix_web::rt::spawn(async move {
        struct SimGuard {
            state: web::Data<Arc<LocalState>>,
            rule_id: Uuid,
            cancel: Arc<std::sync::atomic::AtomicBool>,
        }
        impl Drop for SimGuard {
            fn drop(&mut self) {
                self.state.sim_cancels.remove(&self.rule_id);
                self.state.sim_progress.remove(&self.rule_id);
                let _ = self.state.sse_tx.send(SseEvent::SimulationFinished {
                    rule_id: self.rule_id,
                    cancelled: self.cancel.load(Ordering::Acquire),
                });
            }
        }
        let _guard = SimGuard {
            state: app_state.clone(),
            rule_id: rid,
            cancel: cancel.clone(),
        };

        let outcome = match run_backtest(
            app_state.clone(),
            rid,
            since,
            until,
            cancel,
            cell,
        )
        .await
        {
            Ok(rows) => {
                // Roll up before the rows move into the outcome — a handful of
                // scalars kept long-lived on `last_sim_summary`, decoupled from
                // `sim_results`' 60-minute TTL so the rules table's column
                // doesn't blink out mid-session. See `state::sim_summary`.
                let r = crate::strategies::sim_query::summarize(&rows);
                app_state.last_sim_summary.insert(
                    rid,
                    SimSummary {
                        n_fired: r.n_fired,
                        closed: r.closed,
                        win_rate: r.win_rate,
                        avg_pnl_pct: r.avg_pnl_pct,
                        total_pnl_sol: r.total_pnl_sol,
                        computed_at: Utc::now(),
                        sim_key: sim_key_owned.clone(),
                    },
                );
                SimOutcome::Done(Arc::new(rows))
            }
            Err(e) => classify_backtest_error(&e),
        };
        app_state
            .sim_results
            .insert(rid, sim_key_owned, since, until, outcome);
    });

    HttpResponse::Accepted().json(serde_json::json!({ "started": true }))
}

fn classify_backtest_error(e: &anyhow::Error) -> SimOutcome {
    let msg = e.to_string();
    if msg.contains("cancelled") {
        SimOutcome::Cancelled
    } else if msg.contains("Rule not found") {
        SimOutcome::Failed { status: 404, message: msg }
    } else if msg.contains("All rule criteria are empty")
        || msg.contains("Rule configures no scalp entry gate")
    {
        SimOutcome::Failed { status: 400, message: msg }
    } else {
        tracing::error!("Simulation failed: {e}");
        SimOutcome::Failed { status: 500, message: msg }
    }
}

/// Serialize a backtest row vector to JSON values for storage.
pub fn rows_to_json<T: serde::Serialize>(rows: Vec<T>) -> Result<Vec<serde_json::Value>, anyhow::Error> {
    match serde_json::to_value(&rows) {
        Ok(serde_json::Value::Array(vals)) => Ok(vals),
        Ok(_) => anyhow::bail!("unexpected sim result shape (not an array)"),
        Err(e) => Err(anyhow::Error::from(e)),
    }
}
