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
//! **Two shapes, and the client picks by what it is asking.** [`get_position_metrics`]
//! answers at **one instant** — it folds to the exit (or entry) fill and reads there,
//! which is what makes it cheap enough to poll. [`get_position_metric_series`] answers
//! at **every** row of the engine's decision grid in one pass, so a chart crosshair
//! indexes an array instead of re-folding per hover: the fold is O(all trades), and
//! multiplying that by a pointer's move rate on a 2vCPU box is the thing the series
//! exists to avoid.
//!
//! The two share their rule resolution and their flow context ([`resolve_rule`],
//! [`load_flow_ctx`]) rather than each doing it. Two copies drift, and the
//! run-snapshot preference is exactly the part that must not: a rule edited after the
//! position closed would otherwise draw thresholds that never applied to it. The
//! engine holds the other half of that line — a series row at an instant equals
//! [`replay_readout`] at that instant, pinned by a test.
//!
//! Contrast with the lab's `metric-series`, which computes the whole registry at
//! every row for a caller that evaluates the conditions itself. Here the backend
//! evaluates, so only the columns *this rule's* reqs read are folded and the client
//! declares no windows or horizons at all.
//!
//! Scope is deliberately **the rule's own conditions**. The full registry belongs to
//! the lab's inspect modal; here the question is "what is this position waiting on"
//! (open) or "what did the rule see when it closed" (replay).

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;
use uuid::Uuid;

use hunter_engine::arm::CompiledRule;
use hunter_engine::event::{LoadedRule, RuleId};
use hunter_engine::fingerprint::FingerprintId;
use hunter_engine::metrics::evaluator::ConditionExpr;
use hunter_engine::metrics::flow_ix::{
    ix_hash_from_labels_value, marker_bits_from_labels_value, wallet_hash, FlowPatterns,
};
use hunter_engine::metrics::{metric_spec, MetricId, Side, TradeLite};
use hunter_engine::readout::{
    replay_readout, replay_series, ConditionRead, ConditionSeries, ReadSide, ReadoutSource,
    ReplayCtx, ReplayFlow, RuleReadout,
};
use hunter_engine::rule_params::RuleParams;

use crate::state::deploy_state::DeployState;
use crate::strategies::engine::convert::{fp_to_engine, rule_to_loaded};
use crate::strategies::engine::EngineReloadError;
use trading_core::models::trade::{Trade, TradeType};
use trading_core::models::wallet::validate_solana_address;
use trading_core::models::StrategyPosition;

/// What a condition **is**, independent of any instant — the half of the wire shape
/// that does not change row to row, so the series sends it once and the point read
/// flattens it beside its single value.
///
/// `metric` is the registry **name**, not the engine's `MetricId` — the id is an
/// internal ordinal and must never reach a client (the lab's `metric-series` route
/// holds the same line).
#[derive(Debug, serde::Serialize)]
struct ConditionMetaOut {
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
    /// The full span - size, lag and unit. `window_size_sec` above stays for
    /// clients that only ever knew wall-clock windows.
    #[serde(skip_serializing_if = "Option::is_none")]
    window: Option<hunter_engine::metrics::WindowSpec>,
    /// The authored DNF, `OR` of `AND` arms, as `{operator, value}` objects.
    conditions: serde_json::Value,
    /// `authored` | `take_profit` | `stop_loss` — a desugared ladder req keeps its
    /// label so the UI does not render a TP as a raw `pnl` condition.
    origin: &'static str,
    /// PnL the trailing stop arms at, when gated.
    #[serde(skip_serializing_if = "Option::is_none")]
    arm_above_pct: Option<f64>,
}

/// One condition read at one instant.
#[derive(Debug, serde::Serialize)]
struct ConditionOut {
    #[serde(flatten)]
    meta: ConditionMetaOut,
    /// The live reading. Non-finite serializes `null` — the engine convention that
    /// an unreadable metric satisfies nothing, carried onto the wire rather than
    /// leaking a `NaN` that JSON cannot represent.
    value: Option<f64>,
    /// Whether this condition holds right now.
    ok: bool,
    /// The operator + threshold that matched, when one did.
    #[serde(skip_serializing_if = "Option::is_none")]
    matched_operator: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    matched_value: Option<f64>,
    /// The trail is gated and not yet armed, so the fold **skips** this condition.
    /// Distinct from `ok: false`: it is not being evaluated at all.
    disarmed: bool,
}

/// One condition across a whole series: the same metadata, then one entry per row of
/// [`SeriesOut::at`].
#[derive(Debug, serde::Serialize)]
struct ConditionSeriesOut {
    #[serde(flatten)]
    meta: ConditionMetaOut,
    /// Per row; non-finite serializes `null`, as on the point read.
    values: Vec<Option<f64>>,
    ok: Vec<bool>,
    /// Per row, and **omitted entirely** unless this condition is a gated trail —
    /// the only kind the fold ever skips. Per-row rather than a single flag because
    /// a trail arms and disarms as PnL crosses `arm_above_pct`, which is exactly the
    /// distinction `disarmed` exists to carry.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    disarmed: Vec<bool>,
}

/// The series response. `at` is **epoch millis**, not RFC3339: at one row per
/// decision tick, 25 bytes of quoted timestamp per row is the single largest line
/// item in the payload and buys the client nothing it cannot do with `new Date(ms)`.
#[derive(Debug, serde::Serialize)]
struct SeriesOut {
    /// Absent on the **armed** series: a Waiting row has no position yet, and
    /// inventing a nil UUID would read as one.
    #[serde(skip_serializing_if = "Option::is_none")]
    position_id: Option<Uuid>,
    mint_address: String,
    rule_id: Uuid,
    /// Always `replay` — a series is a reconstruction by construction, including for
    /// a position the engine still holds. Carried anyway so the client's honesty
    /// rendering keys off the payload rather than off which route it called.
    source: &'static str,
    at: Vec<i64>,
    conditions: Vec<ConditionSeriesOut>,
    /// The row cap cut the fold short; coverage ends at `covered_until`. A silently
    /// short series reads exactly like a token that stopped trading.
    truncated: bool,
    covered_until: chrono::DateTime<chrono::Utc>,
    /// Where coverage *starts*. On an entered position the budget is spent around the
    /// entry rather than around token creation, so this is not the first trade and the
    /// client cannot assume it is — a crosshair left of here has no row either.
    covered_from: chrono::DateTime<chrono::Utc>,
    /// The window the server asked for, or `null` when it recorded the whole history.
    /// `covered_from` alone cannot tell the client *why* the head starts where it does,
    /// and the two cases read oppositely: with a window, rows left of it exist and were
    /// withheld; without one, the token simply had not traded yet.
    record_from: Option<chrono::DateTime<chrono::Utc>>,
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

/// The instant-independent half of a condition, from the parts every read carries.
/// ONE mapper for both shapes: the strip renders a hovered series row with exactly
/// the chips it renders a pinned point read with, so the two cannot describe the
/// same condition differently.
fn condition_meta(
    side: ReadSide,
    metric: MetricId,
    window: Option<hunter_engine::metrics::WindowSpec>,
    conds: &ConditionExpr,
    origin: hunter_engine::arm::ReqOrigin,
    arm_above_pct: Option<f64>,
) -> ConditionMetaOut {
    let spec = metric_spec(metric);
    let (stage, stage_active) = match side {
        ReadSide::Stage { index, active } => (Some(index), Some(active)),
        _ => (None, None),
    };
    ConditionMetaOut {
        side: side_name(side),
        stage,
        stage_active,
        metric: spec.name,
        group: hunter_engine::metrics::group_spec(hunter_engine::metrics::group_of(metric).id).name,
        unit: spec.unit.as_str(),
        // Kept as the legacy key for a wall-clock window so no client breaks; a
        // slot window reports `null` there and names itself in `window` instead.
        window_size_sec: window
            .filter(|w| w.unit == hunter_engine::metrics::WindowUnit::Sec)
            .map(|w| w.size),
        window,
        conditions: hunter_engine::metrics::evaluator::condition_expr_to_value(conds),
        origin: origin_name(origin),
        arm_above_pct,
    }
}

fn condition_out(r: &ConditionRead) -> ConditionOut {
    ConditionOut {
        meta: condition_meta(r.side, r.metric, r.window, &r.conds, r.origin, r.arm_above_pct),
        value: r.value.is_finite().then_some(r.value),
        ok: r.ok,
        matched_operator: r.matched.map(|c| c.operator.symbol()),
        matched_value: r.matched.map(|c| c.value),
        disarmed: r.disarmed,
    }
}

fn condition_series_out(c: &ConditionSeries) -> ConditionSeriesOut {
    let r = &c.req;
    ConditionSeriesOut {
        meta: condition_meta(c.side, r.metric, r.window.primary, &r.conds, r.origin, r.arm_above_pct),
        values: c.values.iter().map(|v| v.is_finite().then_some(*v)).collect(),
        ok: c.ok.clone(),
        // Only a gated trail is ever skipped, so every other condition ships an
        // empty vec and the field vanishes from its JSON object.
        disarmed: if r.arm_above_pct.is_some() { c.disarmed.clone() } else { Vec::new() },
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

/// A `404` whose body names which absence it is.
fn not_found(reason: &str) -> HttpResponse {
    HttpResponse::NotFound().json(serde_json::json!({ "error": reason }))
}

/// The rule a position's reconstruction must be judged against, compiled.
struct ResolvedRule {
    id: RuleId,
    compiled: CompiledRule,
    fingerprint_id: FingerprintId,
}

/// Load one rule row and convert it the way the decision loop does.
///
/// Every failure is a `404` with a distinct reason, because the reasons are not
/// interchangeable to someone looking at an empty panel: a deleted rule has nothing
/// to compile and unparseable params are a different problem. Collapsing them into
/// one blank strip hides which it is.
async fn load_rule(
    app_state: &DeployState,
    rule_uuid: Uuid,
) -> Result<LoadedRule, HttpResponse> {
    let rule_row = match app_state.rule_repo.find(rule_uuid).await {
        Ok(Some(r)) => r,
        Ok(None) => return Err(not_found("that rule is deleted")),
        Err(e) => {
            tracing::error!(rule = %rule_uuid, "readout replay: rule load failed: {e}");
            return Err(HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "rule load failed" })));
        }
    };
    // Converted through the SAME function the decision loop reloads with, so the
    // replay evaluates the rule the engine would, not a parallel reading of `params`.
    rule_to_loaded(&rule_row).map_err(|e| {
        tracing::warn!(rule = %rule_uuid, "readout replay: invalid rule params: {e}");
        not_found("the rule's params no longer parse")
    })
}

/// The rule as the engine is evaluating it **right now** — the armed (pre-entry)
/// path, where there is no run to freeze against.
///
/// Deliberately no `params_snapshot` step: a Waiting row is being judged by the
/// live rule this instant, so its current params ARE the right thresholds. The
/// position path below is the one that must reach back for a frozen copy.
async fn resolve_live_rule(
    app_state: &DeployState,
    rule_uuid: Uuid,
) -> Result<ResolvedRule, HttpResponse> {
    let loaded = load_rule(app_state, rule_uuid).await?;
    Ok(ResolvedRule {
        id: RuleId(rule_uuid),
        compiled: CompiledRule::compile(&loaded),
        fingerprint_id: loaded.fingerprint_id,
    })
}

/// Resolve and compile the rule behind a position — **shared by the point readout
/// and the series**, because the run-`params_snapshot` preference below is the one
/// thing two copies must never disagree about.
async fn resolve_rule(
    app_state: &DeployState,
    position: &StrategyPosition,
) -> Result<ResolvedRule, HttpResponse> {
    let Some(rule_uuid) = position.rule_id else {
        return Err(not_found("manual position — no rule conditions to replay"));
    };
    let mut loaded = load_rule(app_state, rule_uuid).await?;

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
    Ok(ResolvedRule {
        id: RuleId(rule_uuid),
        compiled: CompiledRule::compile(&loaded),
        fingerprint_id: loaded.fingerprint_id,
    })
}

/// The flow context a replay classifies volume vs organic with — the rule's
/// fingerprint patterns and the token's creator wallet.
///
/// Without it every `m_flow_ix` condition reads `NaN` and — worse — the creator's
/// dev buy and dev dump, usually a token's two largest single flows, classify as
/// organic. Neither half is fatal on its own: an unconfigured fingerprint just omits
/// the flow columns, and a missing creator is logged rather than silently folded.
/// See [`ReplayFlow`].
async fn load_flow_ctx(
    app_state: &DeployState,
    mint: &str,
    fingerprint_id: FingerprintId,
) -> (Option<FlowPatterns>, Option<u64>) {
    let patterns = match app_state.fingerprint_repo.find(fingerprint_id.0).await {
        Ok(Some(fp)) => FlowPatterns::from_metric_config(&fp_to_engine(&fp).metric_config),
        Ok(None) => None,
        Err(e) => {
            tracing::warn!(fp = %fingerprint_id.0, "readout replay: fingerprint load failed: {e}");
            None
        }
    };
    // Only pay for the creator lookup on the flow path — a rule with no flow
    // conditions never needs it.
    if patterns.is_none() {
        return (None, None);
    }
    let creator_wallet_hash = match app_state.core.token_repo().find_by_mint(mint).await {
        Ok(Some(t)) if !t.creator_wallet.is_empty() => Some(wallet_hash(&t.creator_wallet)),
        _ => {
            tracing::warn!(mint, "readout replay: no creator wallet — flow split unseeded");
            None
        }
    };
    (patterns, creator_wallet_hash)
}

/// The token's stored trades up to `until`, as the engine's `TradeLite`.
///
/// Bounded in SQL: a memecoin that keeps trading for hours past a position's exit
/// would otherwise transfer and walk rows that cannot affect the answer.
async fn load_trades(
    app_state: &DeployState,
    mint: &str,
    until: chrono::DateTime<chrono::Utc>,
) -> Result<Vec<Trade>, HttpResponse> {
    let trades = app_state
        .core
        .trade_repo()
        .find_by_mint_until(mint, until)
        .await
        .map_err(|e| {
            tracing::error!(mint, "readout replay: trade fetch failed: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "trade fetch failed" }))
        })?;
    if trades.is_empty() {
        // Almost always retention, not a bug: the deploy box keeps a rolling ingest
        // window, so a position older than it has no history left to fold.
        return Err(not_found(
            "no trade history retained for this token — replay needs the lab",
        ));
    }
    Ok(trades)
}

/// The instant a replay's metric clock starts — the token's own creation time, the
/// same anchor the fold uses live (`reduce`'s `TokenCreated`).
///
/// It is NOT the first retained trade. `time` is measured from this instant, `stall`
/// and the lifetime price/flow state are re-based on it, and the 200 ms tick grid is
/// phased from it — so anchoring on `trades[0]` shifts every one of them by however
/// much of the token's head has aged out of the rolling ingest window. On a token
/// whose whole history is still retained the two are within a slot of each other,
/// which is exactly why the difference goes unnoticed until it is large.
///
/// Falls back to the first trade when the token row is gone (retention drops trades
/// and tokens independently): a clock anchored a little late still beats refusing to
/// answer at all, and it is the reading this had before.
async fn replay_created_at(
    app_state: &DeployState,
    mint: &str,
    trades: &[Trade],
) -> chrono::DateTime<chrono::Utc> {
    let first_trade = trades[0].block_time;
    match app_state.core.token_repo().find_by_mint(mint).await {
        Ok(Some(token)) => token.created_at,
        Ok(None) => first_trade,
        Err(e) => {
            tracing::warn!(
                mint,
                "readout replay: token lookup failed, anchoring on first trade: {e}"
            );
            first_trade
        }
    }
}

/// The entry fill `(time, price)` a `PositionCtx` anchors on; `None` for a position
/// that never filled, whose `m_position` reads then have no anchor.
fn entry_fill(position: &StrategyPosition) -> Option<(chrono::DateTime<chrono::Utc>, f64)> {
    position
        .entry_time
        .zip(position.entry_price)
        .filter(|(_, p)| p.is_finite() && *p > 0.0)
}

/// Reconstruct a closed position's readout at one instant by folding its token's
/// stored trades.
///
/// A position that never filled has no instant to read at — its own `404`, distinct
/// from the ones [`resolve_rule`] and [`load_trades`] raise.
async fn replay_for_position(
    app_state: &DeployState,
    position: &StrategyPosition,
    read_at: ReadAt,
) -> Result<(RuleReadout, RuleId), HttpResponse> {
    let rule = resolve_rule(app_state, position).await?;

    // The instant to read. Exit falls back to entry for a row that never closed
    // cleanly, so a stuck/unconfirmed position still answers "what did it see".
    let at = match read_at {
        ReadAt::Entry => position.entry_time,
        ReadAt::Exit => position.exit_time.or(position.entry_time),
    };
    let Some(at) = at else {
        return Err(not_found("this position never filled — no instant to read at"));
    };

    let trades = load_trades(app_state, &position.mint_address, at).await?;
    let (patterns, creator_wallet_hash) =
        load_flow_ctx(app_state, &position.mint_address, rule.fingerprint_id).await;

    let created_at = replay_created_at(app_state, &position.mint_address, &trades).await;
    let lites: Vec<TradeLite> = trades.iter().map(trade_lite).collect();
    let entry = entry_fill(position);
    let stage = Some(position.scale_stage);
    let ResolvedRule { id: rule_id, compiled, fingerprint_id } = rule;

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
            &ReplayCtx {
                created_at, entry, stage, flow,
            },
            at,
        )
    })
    .await;

    out.map(|readout| (readout, rule_id)).map_err(|e| {
        tracing::error!("readout replay: fold task failed: {e}");
        HttpResponse::InternalServerError()
            .json(serde_json::json!({ "error": "replay failed" }))
    })
}

/// One stored trade as the engine's `TradeLite` — the offline mirror of the live
/// `producers::trade_lite` and the lab's `to_trade_lite`. Same four choices as both:
/// the canonical `price_per_token`, REAL reserves (absent ⇒ `NaN`, which reads as
/// "alive" rather than dead), the PRICED reserve for impact, and the flow hashes.
fn trade_lite(t: &Trade) -> TradeLite {
    TradeLite {
        side: if t.trade_type == TradeType::Buy { Side::Buy } else { Side::Sell },
        sol: t.amount_sol,
        price: t.price_per_token,
        reserve_sol: t.real_reserve_sol.unwrap_or(f64::NAN),
        priced_reserve_sol: t.reserve_sol.unwrap_or(f64::NAN),
        at: t.block_time,
        // The ONE shape-complete reader of `trades.ix_labels` — the same
        // `normalize_labels` + hash the live `CachedTrade` uses, so both persisted
        // shapes (bare array and `{"instructions":[…]}`) hash alike. A local reader
        // that only accepts the bare array books every object-shaped row as organic,
        // silently, and the whole flow split downstream of it goes with it.
        ix_hash: ix_hash_from_labels_value(&t.instruction_labels),
        slot: t.slot,
        marker_bits: marker_bits_from_labels_value(&t.instruction_labels),
        wallet_hash: wallet_hash(&t.wallet_address),
        // Which instruction of its transaction this is. A bundle selling several
        // wallets' bags emits one trade per leg, all carrying the same `ix_labels`,
        // so `m_dump_ix`'s transaction count reads leg 0 only. Saturates: the byte is
        // only ever compared against 0, and a tx never carries 255 legs.
        leg_index: t.leg_index.min(u8::MAX as u32) as u8,
    }
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

/// Row ceiling for one series response.
///
/// Deliberately well under the lab's 40k: this ships to the 2vCPU box, and the
/// payload is `rows × conditions` numbers rather than the lab's `rows × registry`.
/// Hitting it **truncates in time** rather than coarsening the grid — every returned
/// row stays bit-identical to the engine's `TICK_MS` decision grid, which is the
/// whole reason to fold on that grid at all — and the response flags the short
/// coverage so the strip can say so.
const MAX_READOUT_SERIES_ROWS: usize = 8_000;

/// GET /api/strategies/{strategy}/positions/{position_id}/metric-series
///
/// Every condition of the position's rule, at every row of the engine's decision
/// grid — the crosshair's answer to "what did the rule see *here*".
///
/// Always a reconstruction, including for a position the engine still holds: the
/// engine keeps one instant of state, not a history, so an instant the loop decided
/// at live can only be reproduced by folding stored trades. That is not the same
/// arithmetic — stored rows carry an approximated real-reserve value and an
/// unpersisted trade is simply absent — which is why `source` is `replay` here
/// unconditionally and the strip labels a hover accordingly.
///
/// One fold per modal, not per hover: the fold is O(all trades) and a pointer moves
/// at frame rate.
pub async fn get_position_metric_series(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, Uuid)>,
) -> impl Responder {
    let (_strategy, position_id) = path.into_inner();

    let position = match app_state.strategy_repo.find_position(position_id).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return HttpResponse::NotFound()
                .json(serde_json::json!({ "error": "Position not found" }));
        }
        Err(e) => {
            tracing::error!(position = %position_id, "readout series: position load failed: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "Failed to get position" }));
        }
    };

    let rule = match resolve_rule(&app_state, &position).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    series_response(
        &app_state,
        position.mint_address.clone(),
        rule,
        SeriesAnchor {
            position_id: Some(position_id),
            // Spend the row budget on the position's own window rather than on the
            // token's first N rows. The budget is a fixed *span* of grid (~22 min at
            // 8k), so on a token entered later than that the whole position would
            // otherwise fall outside coverage — the one case a post-mortem is opened
            // for.
            //
            // The entry *time*, not `entry_fill` — centring needs an instant, and a
            // row that stamped an entry with an unusable price (a failed or stuck
            // fill) is precisely the one worth covering. The fold's anchor below
            // still demands a real price.
            centre: position.entry_time,
            entry: entry_fill(&position),
            stage: Some(position.scale_stage),
        },
    )
    .await
}

/// Where a series spends its row budget, and what position context the fold folds
/// under. The two handlers differ ONLY in this — everything downstream is shared, so
/// an armed series and a position series can never disagree about a row.
struct SeriesAnchor {
    /// Echoed to the client; `None` on the armed path.
    position_id: Option<Uuid>,
    /// The instant coverage is centred on (recording starts one pre-roll before it).
    /// `None` records from the token's first trade.
    centre: Option<chrono::DateTime<chrono::Utc>>,
    /// Entry fill anchoring the `m_position` metrics; `None` pre-entry, whose
    /// position-scoped reads then stay `NaN` — exactly what `can_enter` sees.
    entry: Option<(chrono::DateTime<chrono::Utc>, f64)>,
    stage: Option<u8>,
}

/// Fold one token's retained history through one compiled rule and answer with the
/// whole series. **The one body behind both series routes.**
async fn series_response(
    app_state: &DeployState,
    mint: String,
    rule: ResolvedRule,
    anchor: SeriesAnchor,
) -> HttpResponse {
    // The whole retained history, not just up to the exit: the chart draws past the
    // close, so the crosshair can reach there and must get an answer rather than the
    // last row repeated.
    let as_of = chrono::Utc::now();
    let trades = match load_trades(app_state, &mint, as_of).await {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    let (patterns, creator_wallet_hash) = load_flow_ctx(app_state, &mint, rule.fingerprint_id).await;

    let created_at = replay_created_at(app_state, &mint, &trades).await;
    let lites: Vec<TradeLite> = trades.iter().map(trade_lite).collect();
    let SeriesAnchor { position_id, centre, entry, stage } = anchor;
    let ResolvedRule { id: rule_id, compiled, fingerprint_id } = rule;

    // The fold still runs from creation; only recording starts here. Pre-roll is the
    // rule's own widest metric window, floored at a minute, because the question a
    // readout is opened on is "why did this arm fire" / "why has it not" — which is
    // answered by the approach, not by the anchor instant alone.
    let record_from = centre.map(|at| {
        let pre_roll = compiled.clock_horizons.max_window_secs.max(60.0);
        at - chrono::Duration::milliseconds((pre_roll * 1000.0) as i64)
    });

    // Off the reactor, for the same reason the point replay is — more so: this folds
    // the token's whole retained history and evaluates every condition at every row.
    let out = web::block(move || {
        let flow = patterns.as_ref().map(|p| ReplayFlow {
            fingerprint: fingerprint_id,
            patterns: p,
            creator_wallet_hash,
        });
        replay_series(
            &compiled,
            lites,
            &ReplayCtx {
                created_at, entry, stage, flow,
            },
            as_of,
            Some(MAX_READOUT_SERIES_ROWS),
            record_from,
        )
    })
    .await;

    let series = match out {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("readout series: fold task failed: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "replay failed" }));
        }
    };
    if series.truncated {
        tracing::warn!(
            mint = %mint,
            cap = MAX_READOUT_SERIES_ROWS,
            covered_from = %series.covered_from,
            covered_until = %series.covered_until,
            "readout series hit the row ceiling — truncating coverage in time",
        );
    }

    HttpResponse::Ok().json(SeriesOut {
        position_id,
        mint_address: mint,
        rule_id: rule_id.0,
        source: "replay",
        at: series.at.iter().map(|t| t.timestamp_millis()).collect(),
        conditions: series.conditions.iter().map(condition_series_out).collect(),
        truncated: series.truncated,
        covered_until: series.covered_until,
        covered_from: series.covered_from,
        record_from,
    })
}

/// Query for the armed (pre-entry) readout — a Waiting row has no position id.
#[derive(Debug, Deserialize)]
pub struct ArmedReadoutQuery {
    pub mint: String,
    pub rule: Uuid,
}

/// Query for the armed series: the point query plus where to centre coverage.
#[derive(Debug, Deserialize)]
pub struct ArmedSeriesQuery {
    pub mint: String,
    pub rule: Uuid,
    /// The instant the arm was raised. Coverage centres one pre-roll before it, so a
    /// token that has been tracked longer than the row budget still covers *why it is
    /// still waiting* rather than its first 22 minutes. Omit to record from the
    /// token's first trade.
    #[serde(default)]
    pub armed_at: Option<chrono::DateTime<chrono::Utc>>,
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

/// GET /api/strategies/armed/metric-series?mint=&rule=[&armed_at=]
///
/// The series twin of [`get_armed_metrics`]: every condition of the rule at every row
/// of the decision grid, for a token it is **armed on but has not entered**. What the
/// Waiting modal's chart crosshair and condition timeline read.
///
/// Same fold, same grid and same honesty as the position series — only the anchor
/// differs. Pre-entry there is no entry fill and no ladder stage, so the
/// position-scoped conditions read `null` throughout, which is exactly what the
/// `can_enter` gate itself sees.
///
/// The rule is taken **live**, not from a run snapshot: nothing has been decided yet,
/// so the thresholds this row is being judged by are the current ones.
///
/// Lazily fetched by the client (first crosshair move or timeline toggle), never
/// polled: the fold is O(all trades) and this box has two cores.
pub async fn get_armed_metric_series(
    app_state: web::Data<Arc<DeployState>>,
    query: web::Query<ArmedSeriesQuery>,
) -> impl Responder {
    let ArmedSeriesQuery { mint, rule, armed_at } = query.into_inner();
    if let Err(e) = validate_solana_address(&mint) {
        return HttpResponse::BadRequest().json(serde_json::json!({ "error": e }));
    }
    let resolved = match resolve_live_rule(&app_state, rule).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    series_response(
        &app_state,
        mint,
        resolved,
        SeriesAnchor { position_id: None, centre: armed_at, entry: None, stage: None },
    )
    .await
}
