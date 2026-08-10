//! Rule readout — each of a rule's conditions with the value behind it and whether
//! it holds, for one (token, rule).
//!
//! **Two sources, and the response says which.** An OPEN position is answered out of
//! the decision loop's own `TokenTrack` ([`hunter_engine::readout::read_state`]): the
//! exact state the engine is deciding on, so a condition shown as satisfied is one the
//! fold is acting on. A CLOSED one has no engine state left, so it is reconstructed by
//! folding stored trades back through a fresh track
//! ([`hunter_engine::readout::replay_readout`]) — close, but not the same thing, and
//! `source` carries that distinction to the client rather than blurring it.
//!
//! The replay is deliberately **one instant**, not a series: it folds to the exit (or
//! entry) fill and reads there. That is what makes it affordable on the deploy box —
//! the lab's `metric-series` computes every registry metric at every row of a sparse
//! tick grid, which is the right tool for scrubbing a chart and the wrong one for
//! answering "which condition closed this".
//!
//! Scope is deliberately **the rule's own conditions**. The full registry belongs to
//! the lab's inspect modal; here the question is "what is this position waiting on"
//! (open) or "what did the rule see when it closed" (replay).

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;
use uuid::Uuid;

use hunter_engine::arm::CompiledRule;
use hunter_engine::event::RuleId;
use hunter_engine::fingerprint::FingerprintId;
use hunter_engine::metrics::flow_split::{ix_hash_opt, wallet_hash, FlowPatterns};
use hunter_engine::metrics::{metric_spec, Side, TradeLite};
use hunter_engine::readout::{
    replay_readout, ConditionRead, ReadSide, ReadoutSource, ReplayCtx, ReplayFlow, RuleReadout,
};
use hunter_engine::rule_params::RuleParams;

use crate::state::deploy_state::DeployState;
use crate::strategies::engine::convert::{fp_to_engine, rule_to_loaded};
use crate::strategies::engine::EngineReloadError;
use trading_core::models::trade::{Trade, TradeType};
use trading_core::models::wallet::validate_solana_address;
use trading_core::models::StrategyPosition;

/// One condition on the wire. `metric` is the registry **name**, not the engine's
/// `MetricId` — the id is an internal ordinal and must never reach a client (the
/// `metric-series` route holds the same line).
#[derive(Debug, serde::Serialize)]
struct ConditionOut {
    /// `entry` | `exit` | `stage`.
    side: &'static str,
    /// Ladder index; present only on a `stage` read.
    #[serde(skip_serializing_if = "Option::is_none")]
    stage: Option<u8>,
    /// Whether the fold is currently evaluating this stage. Absent off a stage read.
    #[serde(skip_serializing_if = "Option::is_none")]
    stage_active: Option<bool>,
    metric: &'static str,
    group: &'static str,
    unit: &'static str,
    /// Trailing-window size for a dynamic metric; `null` for static ones.
    window_size_sec: Option<f64>,
    /// The live reading. Non-finite serializes `null` — the engine convention that
    /// an unreadable metric satisfies nothing, carried onto the wire rather than
    /// leaking a `NaN` that JSON cannot represent.
    value: Option<f64>,
    /// The authored DNF, `OR` of `AND` arms, as `{operator, value}` objects.
    conditions: serde_json::Value,
    /// Whether this condition holds right now.
    ok: bool,
    /// The operator + threshold that matched, when one did.
    #[serde(skip_serializing_if = "Option::is_none")]
    matched_operator: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    matched_value: Option<f64>,
    /// `authored` | `take_profit` | `stop_loss` — a desugared ladder req keeps its
    /// label so the UI does not render a TP as a raw `pnl` condition.
    origin: &'static str,
    /// PnL the trailing stop arms at, when gated.
    #[serde(skip_serializing_if = "Option::is_none")]
    arm_above_pct: Option<f64>,
    /// The trail is gated and not yet armed, so the fold **skips** this condition.
    /// Distinct from `ok: false`: it is not being evaluated at all.
    disarmed: bool,
}

/// The readout response.
#[derive(Debug, serde::Serialize)]
struct ReadoutOut {
    mint_address: String,
    rule_id: Uuid,
    /// `engine` = read out of the live fold (exact). `replay` = reconstructed from
    /// stored trades (close, but stored rows carry an approximated real-reserve value
    /// and any unpersisted trade is absent). A client must be able to tell them apart.
    source: &'static str,
    /// The arm's lifecycle state (`Armed`, `Entered`, `ExitPending`, …).
    /// `null` on a replay — the arm is long gone.
    arm: Option<&'static str>,
    /// Scale-out stage; `null` when no bag is held.
    stage: Option<u8>,
    /// The instant every value is read at — one instant for the whole response.
    at: chrono::DateTime<chrono::Utc>,
    conditions: Vec<ConditionOut>,
}

fn side_name(side: ReadSide) -> &'static str {
    match side {
        ReadSide::Entry => "entry",
        ReadSide::Exit => "exit",
        ReadSide::Stage { .. } => "stage",
    }
}

fn origin_name(origin: hunter_engine::arm::ReqOrigin) -> &'static str {
    use hunter_engine::arm::ReqOrigin::*;
    match origin {
        Authored => "authored",
        TakeProfit => "take_profit",
        StopLoss => "stop_loss",
    }
}

fn condition_out(r: &ConditionRead) -> ConditionOut {
    let spec = metric_spec(r.metric);
    let (stage, stage_active) = match r.side {
        ReadSide::Stage { index, active } => (Some(index), Some(active)),
        _ => (None, None),
    };
    ConditionOut {
        side: side_name(r.side),
        stage,
        stage_active,
        metric: spec.name,
        group: hunter_engine::metrics::group_spec(hunter_engine::metrics::group_of(r.metric).id).name,
        unit: spec.unit.as_str(),
        window_size_sec: r.window,
        value: r.value.is_finite().then_some(r.value),
        conditions: hunter_engine::metrics::evaluator::condition_expr_to_value(&r.conds),
        ok: r.ok,
        matched_operator: r.matched.map(|c| c.operator.symbol()),
        matched_value: r.matched.map(|c| c.value),
        origin: origin_name(r.origin),
        arm_above_pct: r.arm_above_pct,
        disarmed: r.disarmed,
    }
}

fn readout_out(mint: String, rule_id: RuleId, readout: RuleReadout) -> ReadoutOut {
    ReadoutOut {
        mint_address: mint,
        rule_id: rule_id.0,
        source: match readout.source {
            ReadoutSource::Engine => "engine",
            ReadoutSource::Replay => "replay",
        },
        arm: readout.arm,
        stage: readout.stage,
        at: readout.at,
        conditions: readout.reads.iter().map(condition_out).collect(),
    }
}

/// Which instant a closed position's replay reads at.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReadAt {
    /// The exit fill — "why did this close". The default, and the question a
    /// post-mortem usually starts from.
    #[default]
    Exit,
    /// The entry fill — "what did the rule see when it bought".
    Entry,
}

#[derive(Debug, Default, Deserialize)]
pub struct PositionMetricsQuery {
    #[serde(default)]
    pub at: ReadAt,
}

/// Reconstruct a closed position's readout by folding its token's stored trades.
///
/// Every failure here is a `404` with a distinct reason, because the reasons are not
/// interchangeable to someone looking at an empty panel: a manual position never had
/// a rule, a `trades` table that aged the token out (the deploy box keeps a rolling
/// window) has nothing to fold, and a position that never filled has no instant to
/// read at. Collapsing them into one blank strip hides which it is.
async fn replay_for_position(
    app_state: &DeployState,
    position: &StrategyPosition,
    read_at: ReadAt,
) -> Result<(RuleReadout, RuleId), HttpResponse> {
    let not_found = |reason: &str| {
        HttpResponse::NotFound().json(serde_json::json!({ "error": reason }))
    };

    let Some(rule_uuid) = position.rule_id else {
        return Err(not_found("manual position — no rule conditions to replay"));
    };
    let rule_id = RuleId(rule_uuid);

    let rule_row = match app_state.rule_repo.find(rule_uuid).await {
        Ok(Some(r)) => r,
        Ok(None) => return Err(not_found("the rule that opened this position is deleted")),
        Err(e) => {
            tracing::error!(rule = %rule_uuid, "readout replay: rule load failed: {e}");
            return Err(HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "rule load failed" })));
        }
    };
    // Compiled through the SAME converter the decision loop reloads with, so the
    // replay evaluates the rule the engine would, not a parallel reading of `params`.
    let mut loaded = rule_to_loaded(&rule_row).map_err(|e| {
        tracing::warn!(rule = %rule_uuid, "readout replay: invalid rule params: {e}");
        not_found("the rule's params no longer parse")
    })?;

    // Prefer the RUN's frozen params over the rule's current ones. A rule edited
    // after this position closed would otherwise have the post-mortem drawing
    // thresholds that never applied to it — the single most misleading thing a
    // reconstruction can do, because every number around it is real.
    // `params_snapshot` is written at run launch (`strategy_runs`); an empty or
    // unparseable one (pre-snapshot rows) falls back to the live rule, which is the
    // best available answer rather than no answer.
    match app_state.strategy_repo.find_run(position.run_id).await {
        Ok(Some(run)) => match RuleParams::parse(&run.params_snapshot) {
            Ok(params) => loaded.params = params,
            Err(e) => tracing::warn!(
                run = %position.run_id,
                "readout replay: run params_snapshot unusable ({e}) — using the rule's current params",
            ),
        },
        Ok(None) => tracing::warn!(
            run = %position.run_id,
            "readout replay: run row missing — using the rule's current params",
        ),
        Err(e) => tracing::warn!(
            run = %position.run_id,
            "readout replay: run load failed ({e}) — using the rule's current params",
        ),
    }
    let compiled = CompiledRule::compile(&loaded);

    Ok((
        replay_at(app_state, position, &compiled, loaded.fingerprint_id, read_at).await?,
        rule_id,
    ))
}

/// Fold the token's trades up to the requested instant and read the rule there.
async fn replay_at(
    app_state: &DeployState,
    position: &StrategyPosition,
    compiled: &CompiledRule,
    fingerprint_id: FingerprintId,
    read_at: ReadAt,
) -> Result<RuleReadout, HttpResponse> {
    let not_found = |reason: &str| {
        HttpResponse::NotFound().json(serde_json::json!({ "error": reason }))
    };

    // The instant to read. Exit falls back to entry for a row that never closed
    // cleanly, so a stuck/unconfirmed position still answers "what did it see".
    let at = match read_at {
        ReadAt::Entry => position.entry_time,
        ReadAt::Exit => position.exit_time.or(position.entry_time),
    };
    let Some(at) = at else {
        return Err(not_found("this position never filled — no instant to read at"));
    };

    let trades = match app_state
        .core
        .trade_repo()
        .find_by_mint_until(&position.mint_address, at)
        .await
    {
        Ok(t) => t,
        Err(e) => {
            tracing::error!(mint = %position.mint_address, "readout replay: trade fetch failed: {e}");
            return Err(HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "trade fetch failed" })));
        }
    };
    if trades.is_empty() {
        // Almost always retention, not a bug: the deploy box keeps a rolling ingest
        // window, so a position older than it has no history left to fold.
        return Err(not_found(
            "no trade history retained for this token — replay needs the lab",
        ));
    }

    // The flow context, without which every `m_flow_split` condition reads NaN and —
    // worse — the creator's dev buy/dump would classify as organic. See `ReplayFlow`.
    let patterns = match app_state.fingerprint_repo.find(fingerprint_id.0).await {
        Ok(Some(fp)) => FlowPatterns::from_metric_config(&fp_to_engine(&fp).metric_config),
        Ok(None) => None,
        Err(e) => {
            tracing::warn!(fp = %fingerprint_id.0, "readout replay: fingerprint load failed: {e}");
            None
        }
    };
    let creator_wallet_hash = match app_state
        .core
        .token_repo()
        .find_by_mint(&position.mint_address)
        .await
    {
        Ok(Some(t)) if !t.creator_wallet.is_empty() => Some(wallet_hash(&t.creator_wallet)),
        _ => {
            tracing::warn!(
                mint = %position.mint_address,
                "readout replay: no creator wallet — flow split unseeded",
            );
            None
        }
    };

    let created_at = trades[0].block_time;
    let lites: Vec<TradeLite> = trades.iter().map(trade_lite).collect();
    let entry = position
        .entry_time
        .zip(position.entry_price)
        .filter(|(_, p)| p.is_finite() && *p > 0.0);
    let stage = Some(position.scale_stage);
    let compiled = compiled.clone();

    // Off the reactor: this walks every trade the token made before `at`, which for a
    // busy token is tens of thousands of folds. Small next to the lab's full series,
    // still not something to run on an async worker of a 2vCPU box.
    let out = web::block(move || {
        let flow = patterns.as_ref().map(|p| ReplayFlow {
            fingerprint: fingerprint_id,
            patterns: p,
            creator_wallet_hash,
        });
        replay_readout(
            &compiled,
            lites,
            &ReplayCtx { created_at, entry, stage, flow },
            at,
        )
    })
    .await;

    out.map_err(|e| {
        tracing::error!("readout replay: fold task failed: {e}");
        HttpResponse::InternalServerError()
            .json(serde_json::json!({ "error": "replay failed" }))
    })
}

/// One stored trade as the engine's `TradeLite` — the offline mirror of the live
/// `producers::trade_lite` and the lab's `to_trade_lite`. Same three choices as both:
/// the canonical `price_per_token`, REAL reserves (absent ⇒ `NaN`, which reads as
/// "alive" rather than dead), and the flow hashes.
fn trade_lite(t: &Trade) -> TradeLite {
    TradeLite {
        side: if t.trade_type == TradeType::Buy { Side::Buy } else { Side::Sell },
        sol: t.amount_sol,
        price: t.price_per_token,
        reserve_sol: t.real_reserve_sol.unwrap_or(f64::NAN),
        at: t.block_time,
        ix_hash: ix_hash_of(&t.instruction_labels),
        wallet_hash: wallet_hash(&t.wallet_address),
    }
}

/// [`ix_hash_opt`] over `trades.ix_labels` in its stored JSON form, borrowing rather
/// than rebuilding a `Vec<String>` per trade.
///
/// Anything that is not a flat array of strings yields `None` — the same
/// "unparseable ⇒ `None` ⇒ organic" answer `ix_hash_from_labels_json` gives, and the
/// reason this rejects a non-string element instead of skipping it: skipping would
/// hash a *different* label sequence and silently reclassify the trade.
fn ix_hash_of(labels: &serde_json::Value) -> Option<u64> {
    let arr = labels.as_array()?;
    let mut out: Vec<&str> = Vec::with_capacity(arr.len());
    for v in arr {
        out.push(v.as_str()?);
    }
    ix_hash_opt(&out)
}

/// Map an engine-handle failure to a response. A wedged or shutting-down loop is a
/// `503`, not a `500`: nothing is wrong with the request and a retry is the right
/// move, which is exactly what the polling UI does.
fn engine_error(e: EngineReloadError) -> HttpResponse {
    tracing::warn!(error = %e, "rule readout: engine unavailable");
    HttpResponse::ServiceUnavailable()
        .json(serde_json::json!({ "error": "engine unavailable" }))
}

/// GET /api/strategies/{strategy}/positions/{position_id}/metrics[?at=exit|entry]
///
/// ONE route for open and closed alike — the client asks the same question and the
/// response's `source` says how it was answered. Live engine state first (exact);
/// falling back to a replay of stored trades when the engine has no arm, which is
/// every closed position and any row the engine dropped while the modal was open.
///
/// `at` selects the replay instant and is ignored on the engine path, where the only
/// meaningful instant is now.
pub async fn get_position_metrics(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, Uuid)>,
    query: web::Query<PositionMetricsQuery>,
) -> impl Responder {
    let (_strategy, position_id) = path.into_inner();

    // The live engine holds this position ⇒ answer from the fold itself.
    if let Some(meta) = app_state
        .positions
        .engine_id(position_id)
        .and_then(|id| app_state.positions.get(id))
    {
        match app_state.engine.read_rule(&meta.mint, meta.rule_id).await {
            Ok(Some(readout)) => {
                return HttpResponse::Ok().json(readout_out(meta.mint, meta.rule_id, readout));
            }
            // Registered but no arm — it is closing, or it is a tracked-only manual
            // episode. Either way the durable row can still be replayed below.
            Ok(None) => {}
            Err(e) => return engine_error(e),
        }
    }

    // Closed (or engine-less): reconstruct from the durable row + stored trades.
    let position = match app_state.strategy_repo.find_position(position_id).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return HttpResponse::NotFound()
                .json(serde_json::json!({ "error": "Position not found" }));
        }
        Err(e) => {
            tracing::error!(position = %position_id, "readout: position load failed: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "Failed to get position" }));
        }
    };
    match replay_for_position(&app_state, &position, query.at).await {
        Ok((readout, rule_id)) => {
            HttpResponse::Ok().json(readout_out(position.mint_address, rule_id, readout))
        }
        Err(resp) => resp,
    }
}

/// Query for the armed (pre-entry) readout — a Waiting row has no position id.
#[derive(Debug, Deserialize)]
pub struct ArmedReadoutQuery {
    pub mint: String,
    pub rule: Uuid,
}

/// GET /api/strategies/armed/metrics?mint=&rule=
///
/// The same readout for an **armed, not yet entered** (token, rule) pair: entry
/// conditions with live values, so a Waiting row can answer "what is it waiting on".
/// Exit conditions come back too, position-scoped ones reading `null` — exactly what
/// the pre-entry `can_enter` gate sees.
pub async fn get_armed_metrics(
    app_state: web::Data<Arc<DeployState>>,
    query: web::Query<ArmedReadoutQuery>,
) -> impl Responder {
    let ArmedReadoutQuery { mint, rule } = query.into_inner();
    // Reject a malformed mint here rather than forwarding it to the decision loop.
    // It would only miss the token map, but the loop is the one thread that must
    // never do avoidable work on a caller's behalf.
    if let Err(e) = validate_solana_address(&mint) {
        return HttpResponse::BadRequest().json(serde_json::json!({ "error": e }));
    }
    let rule_id = RuleId(rule);
    match app_state.engine.read_rule(&mint, rule_id).await {
        Ok(Some(readout)) => HttpResponse::Ok().json(readout_out(mint, rule_id, readout)),
        Ok(None) => HttpResponse::NotFound()
            .json(serde_json::json!({ "error": "token not tracked for this rule" })),
        Err(e) => engine_error(e),
    }
}
