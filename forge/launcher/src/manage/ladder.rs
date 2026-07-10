//! Simple-threshold sell ladders (token-management-plan.md Phase 4) — the
//! subsystem's first automation.
//!
//! A ladder is a set of rungs `{ metric, threshold, pct }`. A background evaluator
//! reads each armed mint's market state and, when a metric crosses a rung's
//! threshold, fires a sell of `pct` across the ladder's wallet selection — through
//! the **Phase 2 sell pipeline** (audit + feed-confirm), gated by the same
//! `MANAGE_ENABLED` kill switch. Deliberately NOT hunter's tpsl engine: a
//! handful of price/market-cap thresholds authored here, evaluated off the
//! already-ingested `token_overview`.

use std::time::Duration;

use anyhow::{bail, Result};
use platform_core::models::LadderStatus;
use platform_core::storage::repositories::{SellLadderRepo, TokenRepo};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{info, warn};

use super::execute::execute_action;
use super::model::{ManageRequest, WalletSelection};
use crate::config::LauncherSettings;

/// How often the evaluator scans armed ladders. Cheap — bounded to the tiny armed
/// set by the partial index; a slow cadence is fine for take-profit milestones.
const LADDER_EVAL_INTERVAL: Duration = Duration::from_secs(15);

/// The metrics a rung can trigger on — read from the `token_overview` view.
const METRIC_MARKET_CAP_USD: &str = "market_cap_usd";
const METRIC_PRICE_USD: &str = "price_usd";

/// One rung: sell `pct`% of holdings when `metric` first reaches `threshold`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LadderRung {
    /// `market_cap_usd` | `price_usd`.
    pub metric: String,
    pub threshold: f64,
    /// Percent of current holdings to sell when crossed (0–100).
    pub pct: f64,
    /// Flipped by the evaluator once this rung has fired (its trade attempted).
    #[serde(default)]
    pub fired: bool,
}

/// Arm a new ladder for `mint`. Validates the rungs (known metric, sane pct /
/// threshold) up front so a bad rung fails at arm time, not silently mid-run.
pub async fn arm_ladder(
    pool: &PgPool,
    mint: &str,
    selection: WalletSelection,
    rungs: Vec<LadderRung>,
) -> Result<platform_core::models::SellLadder> {
    if rungs.is_empty() {
        bail!("a ladder needs at least one rung");
    }
    for r in &rungs {
        if r.metric != METRIC_MARKET_CAP_USD && r.metric != METRIC_PRICE_USD {
            bail!("unknown rung metric '{}' (use {METRIC_MARKET_CAP_USD} | {METRIC_PRICE_USD})", r.metric);
        }
        if !(0.0..=100.0).contains(&r.pct) || r.pct <= 0.0 {
            bail!("rung pct must be in (0, 100] (got {})", r.pct);
        }
        if !(r.threshold > 0.0) {
            bail!("rung threshold must be positive (got {})", r.threshold);
        }
    }
    SellLadderRepo::insert(
        pool,
        mint,
        serde_json::to_value(&selection)?,
        serde_json::to_value(&rungs)?,
    )
    .await
}

/// Spawn the ladder evaluator as a long-lived task. Cheap when idle.
pub fn spawn_ladder_evaluator(
    pool: PgPool,
    settings: LauncherSettings,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(LADDER_EVAL_INTERVAL);
        loop {
            tick.tick().await;
            if let Err(e) = evaluate_once(&pool, &settings).await {
                warn!(?e, "ladder evaluation pass failed");
            }
        }
    })
}

/// One evaluation pass: for each armed ladder, fire any rung whose metric has
/// crossed. No-op (ladders stay armed) when management is disabled — so arming a
/// ladder while `MANAGE_ENABLED` is off never silently burns rungs without selling.
async fn evaluate_once(pool: &PgPool, settings: &LauncherSettings) -> Result<()> {
    if settings.manage.is_none() {
        return Ok(()); // disabled — leave ladders armed until it's turned on
    }

    for ladder in SellLadderRepo::list_armed(pool).await? {
        let Some(ov) = TokenRepo::overview(pool, &ladder.mint_address).await? else {
            continue; // not ingested yet — nothing to evaluate against
        };
        let mut rungs: Vec<LadderRung> = match serde_json::from_value(ladder.rungs.clone()) {
            Ok(r) => r,
            Err(e) => {
                warn!(ladder_id = %ladder.id, %e, "ladder rungs unparseable — skipping");
                continue;
            }
        };
        let selection: WalletSelection =
            serde_json::from_value(ladder.selection.clone()).unwrap_or_default();

        let mut changed = false;
        for rung in &mut rungs {
            if rung.fired {
                continue;
            }
            let value = match rung.metric.as_str() {
                METRIC_MARKET_CAP_USD => ov.market_cap_usd,
                METRIC_PRICE_USD => ov.price_usd,
                _ => None,
            };
            let Some(v) = value else { continue };
            if v < rung.threshold {
                continue;
            }

            // Crossed — fire a sell of pct% across the ladder's selection through
            // the Phase 2 pipeline. Mark fired regardless of the trade outcome so
            // a transient failure can't re-fire every 15s tick (the audit row
            // records success/failure); the operator re-arms if a fire is lost.
            let req = ManageRequest {
                kind: "sell".to_string(),
                sizing: "pct_of_holdings".to_string(),
                size: rung.pct,
                selection: selection.clone(),
            };
            match execute_action(pool, settings, &ladder.mint_address, &req).await {
                Ok(a) => info!(
                    ladder_id = %ladder.id, mint = %ladder.mint_address, metric = %rung.metric,
                    threshold = rung.threshold, value = v, action = %a.status,
                    "ladder rung fired"
                ),
                Err(e) => warn!(
                    ladder_id = %ladder.id, mint = %ladder.mint_address, %e,
                    "ladder rung fire failed (marked fired to avoid re-fire loop)"
                ),
            }
            rung.fired = true;
            changed = true;
        }

        if changed {
            let status = if rungs.iter().all(|r| r.fired) {
                LadderStatus::Done
            } else {
                LadderStatus::Armed
            };
            SellLadderRepo::update(
                pool,
                ladder.id,
                serde_json::to_value(&rungs)?,
                status.as_str(),
            )
            .await?;
        }
    }
    Ok(())
}
