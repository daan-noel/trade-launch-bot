//! Paper-run lifecycle helpers (the run-completion side; run start/stop live on
//! [`super::Tpsl2RuntimeCache`]).

use std::sync::Arc;

use chrono::Utc;
use sqlx::PgPool;
use tokio::sync::broadcast;
use tracing::{info, warn};
use uuid::Uuid;

use super::Tpsl2RuntimeCache;
use backend_core::models::ingest::SseEvent;
use backend_core::storage::repositories::tpsl2_strategy_rule_repo::Tpsl2StrategyRuleRepo;

/// After a paper position closes, finish the run if its total-token cap has
/// been reached and no positions remain open: auto-deactivate the rule, refresh
/// the rules cache, and broadcast a [`SseEvent::PaperTestFinished`] notification.
///
/// No-ops when the rule has no cap (`max_total` is `None`) — such a run only ends
/// on manual stop — or when the cap/holding conditions are not yet met.
pub(crate) async fn finish_paper_run_if_complete(
    pool: &PgPool,
    runtime: &Arc<Tpsl2RuntimeCache>,
    sse_tx: &broadcast::Sender<SseEvent>,
    rule_id: Uuid,
    rule_name: &str,
    max_total: Option<u64>,
) {
    let Some(cap) = max_total else { return };
    let total = runtime.total_count_by_rule(rule_id);
    let holding = runtime.holding_count_by_rule(rule_id);
    if total < cap as i64 || holding > 0 {
        return;
    }

    match runtime.finish_paper_run(pool, rule_id).await {
        Ok(Some(run)) => {
            // Auto-deactivate the rule so it stops cleanly, then refresh the cache.
            let rule_repo = Tpsl2StrategyRuleRepo::new(pool.clone());
            match rule_repo.find_by_id(rule_id).await {
                Ok(Some(mut rule)) if rule.is_active => {
                    rule.is_active = false;
                    if let Err(err) = rule_repo.update(&rule).await {
                        warn!("Failed to deactivate finished paper rule {rule_id}: {err}");
                    }
                }
                Ok(_) => {}
                Err(err) => warn!("Failed to load rule {rule_id} for paper finish: {err}"),
            }
            if let Err(err) = runtime.reload_rules(pool).await {
                warn!("Failed to reload rules after paper finish: {err}");
            }
            let _ = sse_tx.send(SseEvent::PaperTestFinished {
                rule_id,
                rule_name: rule_name.to_string(),
                run_seq: run.run_seq,
                tokens_traded: total,
                timestamp: Utc::now(),
            });
            info!(
                %rule_id, run_seq = run.run_seq, tokens = total,
                "[PAPER] run finished — rule auto-deactivated"
            );
        }
        Ok(None) => {} // already finished or stopped
        Err(err) => warn!("Failed to finish paper run for rule {rule_id}: {err}"),
    }
}
