//! **Scan ≡ `run_replay` drift lock** (plan decision 13 / step 5.5).
//!
//! The grouped sweep's per-combo scan (a cheap walk over a precomputed
//! [`MetricSeries`]) is an *optimization* of running the full `hunter_engine`
//! fold per combo. This test locks the two together: for a sample corpus and a
//! set of rules covering every exit path, a single-token
//! [`run_replay`](crate::strategies::replay::run_replay) (the real engine fold)
//! and the scan must produce identical per-token outcomes — same fired/exit
//! reason, same entry/exit price, same PnL. If they ever diverge, the fast path
//! silently lies; this test fails first.

use std::sync::Arc;

use chrono::{Duration, TimeZone, Utc};

use hunter_engine::arm::CompiledRule;
use hunter_engine::event::{ExitReason, LoadedRule, RuleId, TradeMode};
use hunter_engine::fingerprint::{Fingerprint, FingerprintId};
use hunter_engine::grouping::TokenFingerprint;
use hunter_engine::metrics::evaluator::{Condition, Operator};
use hunter_engine::metrics::flow_split::FlowPatterns;
use hunter_engine::metrics::{MetricGroupId, MetricId, Ts};
use hunter_engine::rule_params::{ExitStage, GroupConditions, RuleParams, SideConditions};

use trading_core::config::constants::sol_to_lamports;
use trading_core::strategies::kernel::{CostModel, ExitCode};
use trading_core::strategies::paper_fill::FillModel;

use crate::sweep::corpus::CorpusToken;
use crate::sweep::projection::CorpusTrade;
use crate::sweep::strategy::TokenOutcome;
use crate::strategies::replay::{run_replay, PositionOutcome, ReplayConfig, ReplayToken};

use super::axes::{AxisSide, AxisSpec};
use super::strategy::{
    build_series_with_flow, columns_for, scan, sparse_grid_for, wants_exit_index, Pricing,
};

const BUY_SOL: f64 = 1.0;
const FP_ID: uuid::Uuid = uuid::Uuid::from_u128(0x1234);

/// The sweep's legacy pricing — worst-case fills + `pumpfun_default`. The default
/// for tests that aren't about the fill model; [`pricing_for`] varies it.
fn pricing() -> Pricing {
    pricing_for(FillModel::WorstCase)
}

/// Legacy pricing repriced under `fill_model`. The cost model is deliberately held
/// at `pumpfun_default` here: these tests lock scan ≡ replay, and replay prices with
/// the same `CostModel` the assertion computes with, so varying it would only test
/// the arithmetic — the fill model is the half that changes *which trade* each leg
/// fills against, and so the half a parity guard has to cover.
fn pricing_for(fill_model: FillModel) -> Pricing {
    Pricing { buy_amount_sol: BUY_SOL, fill_model, cost: CostModel::pumpfun_default() }
}

/// Every selectable fill model — parity must hold under each, not just the one the
/// sweep used to hardcode.
const FILL_MODELS: [FillModel; 3] =
    [FillModel::WorstCase, FillModel::FirstInWindow, FillModel::SignalPrice];

fn base() -> Ts {
    Utc.timestamp_opt(1_700_000_000, 0).unwrap()
}

fn at(secs: f64) -> Ts {
    base() + Duration::milliseconds((secs * 1000.0) as i64)
}

/// One synthetic trade. Reserves are REAL-reserve values (what deadness reads).
fn ct(secs: f64, is_buy: bool, sol: f64, price: f64, reserve: f64) -> CorpusTrade {
    ct_flow(secs, is_buy, sol, price, reserve, None, None)
}

/// Like [`ct`], with optional `ix_labels` / `wallet` for flow-classifier fixtures.
fn ct_flow(
    secs: f64,
    is_buy: bool,
    sol: f64,
    price: f64,
    reserve: f64,
    ix_labels: Option<&[&str]>,
    wallet: Option<&str>,
) -> CorpusTrade {
    CorpusTrade {
        block_time: at(secs),
        amount_sol: sol,
        token_amount: 1.0,
        price_per_token: price,
        reserve_sol: Some(reserve),
        reserve_token: Some(1.0),
        real_reserve_sol: Some(reserve),
        real_token_reserves: Some(1.0),
        slot: secs as u64,
        leg_index: 0,
        is_buy,
        tx_signature: None,
        ix_labels: ix_labels
            .map(|labels| serde_json::to_string(labels).expect("labels json").into_boxed_str()),
        wallet: wallet.map(|w| w.to_string().into_boxed_str()),
    }
}

/// A token that matches the test fingerprint (via `cu_limit`) and carries `trades`,
/// created at `base()`.
fn token(mint: &str, trades: Vec<CorpusTrade>) -> (CorpusToken, ReplayToken) {
    token_at(mint, 0.0, trades)
}

/// Like [`token`], with an explicit creation offset (seconds past `base()`) — lets a
/// multi-token corpus stagger token ages so `time`-based clocks cross at different
/// absolute instants (the D1 frozen-tail fixtures).
fn token_at(mint: &str, created_secs: f64, trades: Vec<CorpusTrade>) -> (CorpusToken, ReplayToken) {
    let tf = TokenFingerprint { cu_limit: Some(200_000), ..Default::default() };
    let trades = Arc::new(trades);
    let created = at(created_secs);
    let corpus = CorpusToken {
        mint: mint.to_string(),
        symbol: "SYM".to_string(),
        created_at: created,
        trades: trades.clone(),
        fp: tf.clone(),
    };
    let replay = ReplayToken {
        mint: mint.to_string(),
        symbol: "SYM".to_string(),
        created_at: created,
        tf,
        trades,
        creator_wallet_hash: None,
    };
    (corpus, replay)
}

fn fingerprint() -> Fingerprint {
    Fingerprint {
        id: FingerprintId(FP_ID),
        cu_limit: Some(200_000),
        cu_price: None,
        ix_labels: None,
        init_buy_lamports: None,
        max_cost_lamports: None,
        spendable_lamports_in: None,
        first_slot_buy_lamports: None,
        first_slot_sell_lamports: None,
        bucket_size_amount: 0.1,
        metric_config: serde_json::json!({}),
    }
}

fn loaded(params: RuleParams) -> LoadedRule {
    LoadedRule {
        id: RuleId(uuid::Uuid::from_u128(0xABCD)),
        fingerprint_id: FingerprintId(FP_ID),
        trade_mode: TradeMode::Paper,
        buy_amount_lamports: sol_to_lamports(BUY_SOL) as u64,
        max_concurrent_tokens: u32::MAX,
        max_total_tokens: 0,
        params,
    }
}

/// `RuleParams` with a single exit metric condition.
fn exit_metric(group: MetricGroupId, metric: MetricId, op: Operator, value: f64) -> RuleParams {
    let mut gc = GroupConditions::default();
    gc.metrics.insert(metric, vec![vec![Condition { operator: op, value }]]);
    let mut side = SideConditions::default();
    side.0.insert(group, vec![gc]);
    RuleParams { take_profit: None, stop_loss: None, entry: None, exit: Some(side), ..RuleParams::default() }
}

/// One side's conditions for a single windowed (dynamic-group) metric — e.g. the
/// `m_price_window(30).trail >= 12` dip trigger.
fn window_metric_side(
    group: MetricGroupId,
    metric: MetricId,
    window: f64,
    op: Operator,
    value: f64,
) -> SideConditions {
    let mut gc = GroupConditions::default();
    gc.strict.insert("window_size_sec".to_string(), window);
    gc.metrics.insert(metric, vec![vec![Condition { operator: op, value }]]);
    let mut side = SideConditions::default();
    side.0.insert(group, vec![gc]);
    side
}

/// One side's conditions for a single static-group metric (e.g. `m_position.retrace`).
fn static_metric_side(group: MetricGroupId, metric: MetricId, op: Operator, value: f64) -> SideConditions {
    let mut gc = GroupConditions::default();
    gc.metrics.insert(metric, vec![vec![Condition { operator: op, value }]]);
    let mut side = SideConditions::default();
    side.0.insert(group, vec![gc]);
    side
}

fn exit_code_of(reason: Option<ExitReason>) -> ExitCode {
    match reason {
        None => ExitCode::Open,
        Some(ExitReason::TakeProfit) => ExitCode::TakeProfit,
        Some(ExitReason::StopLoss) => ExitCode::StopLoss,
        Some(ExitReason::Metrics { .. }) => ExitCode::Metrics,
        Some(ExitReason::Dead) => ExitCode::Dead,
        Some(ExitReason::Manual) => ExitCode::Open,
        Some(ExitReason::Migrated) => ExitCode::Open,
    }
}

/// The replay engine's outcome for one token as a comparable tuple:
/// `(fired, exit_code, entry_price, exit_price_for_pnl, pnl_sol, pnl_pct)`.
fn replay_tuple(po: Option<&PositionOutcome>, cost: &CostModel) -> (bool, ExitCode, f64, f64, f32, f32) {
    match po {
        None => (false, ExitCode::NoEntry, 0.0, 0.0, 0.0, 0.0),
        Some(o) => {
            let exit_price = o.exit_price.unwrap_or(o.last_price);
            let (pnl_sol, pnl_pct) = o.pnl_with_costs(BUY_SOL, cost);
            (true, exit_code_of(o.exit_reason), o.entry_price, exit_price, pnl_sol as f32, pnl_pct as f32)
        }
    }
}

/// Run `rule` over `corpus_tokens` two ways — full engine replay and the scan — and
/// assert the per-token outcomes match, **under every [`FillModel`]**.
///
/// The fill model is not a display setting: it decides which trade in the window
/// prices each leg, so it moves entry/exit prices, PnL and (through the entry fill
/// row) every subsequent exit decision. `ReplayConfig` already threads it, so running
/// both sides under the same model is the real lock on the sweep's new fill selector
/// — a scan that only matched replay under `worst_case` would silently lie for the
/// two models the fill-sensitivity analysis actually reported on.
fn assert_parity(label: &str, params: RuleParams, tokens: &[(CorpusToken, ReplayToken)], as_of: Ts) {
    assert_parity_with_flow(label, params, tokens, as_of, None);
}

/// Like [`assert_parity`], but configures fingerprint `metric_config` + seeds the
/// scan's flow state from the same `volume_ix_patterns` (V2.3 drift lock).
fn assert_parity_with_flow(
    label: &str,
    params: RuleParams,
    tokens: &[(CorpusToken, ReplayToken)],
    as_of: Ts,
    volume_ix_patterns: Option<Vec<Vec<String>>>,
) {
    for fm in FILL_MODELS {
        assert_parity_under(
            &format!("{label}/{fm:?}"),
            params.clone(),
            tokens,
            as_of,
            volume_ix_patterns.clone(),
            fm,
        );
    }
}

/// One (`rule`, corpus, fill model) parity assertion — the body [`assert_parity`]
/// runs once per model.
fn assert_parity_under(
    label: &str,
    params: RuleParams,
    tokens: &[(CorpusToken, ReplayToken)],
    as_of: Ts,
    volume_ix_patterns: Option<Vec<Vec<String>>>,
    fill_model: FillModel,
) {
    let rule = loaded(params);
    let mut fp = fingerprint();
    if let Some(patterns) = &volume_ix_patterns {
        fp.metric_config = serde_json::json!({
            "m_flow_split": { "volume_ix_patterns": patterns }
        });
    }
    let flow_patterns = volume_ix_patterns
        .as_ref()
        .map(|p| FlowPatterns::from_label_sequences(p));
    let pricing = pricing_for(fill_model);
    let cost = pricing.cost;

    // Replay each token in isolation (single-token stream) — the sweep judges each
    // token independently, so single-token replay is the right reference.
    let compiled = CompiledRule::compile(&rule);
    let cols = columns_for(&compiled);
    let grid = sparse_grid_for(&compiled);

    for (corpus_tok, replay_tok) in tokens {
        let replay_out = run_replay(
            std::slice::from_ref(&rule),
            std::slice::from_ref(&fp),
            vec![replay_tok.clone()],
            ReplayConfig { as_of, fill_model },
        );
        let po = replay_out.iter().find(|o| o.mint == corpus_tok.mint);
        let (r_fired, r_exit, r_entry, r_exit_price, r_pnl_sol, r_pnl_pct) = replay_tuple(po, &cost);

        let series = build_series_with_flow(
            corpus_tok,
            cols.clone(),
            &grid,
            as_of,
            flow_patterns.as_ref(),
        );
        let outcome = scan(&corpus_tok.trades, &series, &compiled, &pricing);

        assert_eq!(
            outcome.fired, r_fired,
            "[{label}] {} fired mismatch (scan {} vs replay {})",
            corpus_tok.mint, outcome.fired, r_fired
        );
        if !r_fired {
            continue;
        }
        assert_eq!(
            outcome.exit, r_exit,
            "[{label}] {} exit-code mismatch (scan {:?} vs replay {:?})",
            corpus_tok.mint, outcome.exit, r_exit
        );
        let s_entry = outcome.entry_price.unwrap_or(0.0);
        assert!(
            (s_entry - r_entry).abs() < 1e-9,
            "[{label}] {} entry price mismatch (scan {s_entry} vs replay {r_entry})",
            corpus_tok.mint
        );
        // The scan marks an open position to last price internally; compare the
        // realized/marked exit price it priced against.
        let s_exit_price = outcome.exit_price.unwrap_or(r_exit_price);
        assert!(
            (s_exit_price - r_exit_price).abs() < 1e-9,
            "[{label}] {} exit price mismatch (scan {s_exit_price} vs replay {r_exit_price})",
            corpus_tok.mint
        );
        assert_eq!(
            outcome.pnl_sol, r_pnl_sol,
            "[{label}] {} pnl_sol mismatch",
            corpus_tok.mint
        );
        assert_eq!(
            outcome.pnl_percent, r_pnl_pct,
            "[{label}] {} pnl_pct mismatch",
            corpus_tok.mint
        );
    }
}

/// The sample corpus — one token per exit path.
fn corpus() -> Vec<(CorpusToken, ReplayToken)> {
    vec![
        // TP: trigger → adverse entry buy → later print (past fill window) clears TP.
        token(
            "tp",
            vec![
                ct(0.0, true, 1.0, 1.0, 100.0),
                ct(1.0, true, 0.5, 1.1, 100.0),
                ct(10.0, true, 0.5, 2.0, 100.0),
            ],
        ),
        // SL: trigger → entry fill → later sell clears −30% SL.
        token(
            "sl",
            vec![
                ct(0.0, true, 1.0, 1.0, 100.0),
                ct(1.0, true, 0.5, 1.05, 100.0),
                ct(10.0, false, 0.5, 0.5, 100.0),
            ],
        ),
        // Metrics: peak 2.0 then 0.8 → 60% off peak (trail > 50).
        token(
            "metrics",
            vec![
                ct(0.0, true, 1.0, 1.0, 100.0),
                ct(0.5, true, 0.5, 1.05, 100.0),
                ct(10.0, true, 0.5, 2.0, 100.0),
                ct(12.0, false, 0.5, 0.8, 100.0),
            ],
        ),
        // Dead: need an in-window entry fill, then liquidity gone + silent.
        token(
            "dead",
            vec![ct(0.0, true, 1.0, 1.0, 5.0), ct(1.0, true, 0.5, 1.05, 5.0)],
        ),
        // Open: mild drift, liquidity healthy, never triggers anything.
        token(
            "open",
            vec![
                ct(0.0, true, 1.0, 1.0, 100.0),
                ct(1.0, true, 0.5, 1.05, 100.0),
                ct(10.0, true, 0.5, 1.1, 100.0),
            ],
        ),
    ]
}

#[test]
fn scan_matches_replay_tp_sl_rule() {
    // TP 50% OR SL 30%. Enter on arm (no entry conditions).
    let params = RuleParams { take_profit: Some(50.0), stop_loss: Some(30.0), entry: None, exit: None, ..RuleParams::default() };
    assert_parity("tp_sl", params, &corpus(), at(1000.0));
}

#[test]
fn scan_matches_replay_metrics_exit_rule() {
    // Exit when trail (% off peak) exceeds 50 — no TP/SL, so only the metric fires.
    let params = exit_metric(MetricGroupId::PriceLifetime, MetricId::Trail, Operator::Gt, 50.0);
    assert_parity("trail_exit", params, &corpus(), at(1000.0));
}

#[test]
fn scan_matches_replay_entry_gated_rule() {
    // Entry gated on time > 2s (so entry defers past the first trade), TP 20%.
    let mut gc = GroupConditions::default();
    gc.metrics.insert(MetricId::Time, vec![vec![Condition { operator: Operator::Gt, value: 2.0 }]]);
    let mut entry = SideConditions::default();
    entry.0.insert(MetricGroupId::Snapshot, vec![gc]);
    let params =
        RuleParams { take_profit: Some(20.0), stop_loss: None, entry: Some(entry), exit: None, ..RuleParams::default() };
    assert_parity("entry_gate", params, &corpus(), at(1000.0));
}

#[test]
fn scan_matches_replay_pure_dead_open_rule() {
    // No TP/SL/exit metrics: every fired token rides to Dead or Open.
    let params = RuleParams { take_profit: None, stop_loss: None, entry: None, exit: None, ..RuleParams::default() };
    assert_parity("dead_open", params, &corpus(), at(1000.0));
}

/// Plan §5 / step 4 — 2-stage scale-out: bank 70% into strength (`take_profit`),
/// then close the stub on a pure time-stop (`held >= N`). Trades are spaced so
/// each fire's fill window collapses to the fire trade (no mid-flight deferred
/// fill), so the sweep's instantaneous stage advance matches replay's
/// same-batch `FillConfirmed`. Locked by `assert_parity` under every fill model.
#[test]
fn scan_matches_replay_scale_out_two_stage() {
    // Entry @ 1.0 exactly (fill window empty past slot 0 → market-fill) so
    // `initial_tokens = 1e6` and `7000 bps` is an exact integer — otherwise
    // replay's `sell_bps_of` truncates (6999) and multi-leg PnL drifts by float
    // dust under WorstCase. Gaps ≫ MAX_FILL_WAIT_SLOTS so every exit also
    // collapses to its fire print under every FillModel.
    let tokens = vec![token(
        "scale2",
        vec![
            ct(0.0, true, 1.0, 1.0, 100.0), // entry trigger + fill @ 1.0
            ct(10.0, true, 0.5, 1.50, 100.0), // +50% → stage-0 TP banks 7000 bps
            ct(40.0, true, 0.5, 1.60, 100.0), // held ≥ 30 from entry → remainder
        ],
    )];
    let remainder = static_metric_side(
        MetricGroupId::Position,
        MetricId::Held,
        Operator::Gte,
        30.0,
    );
    let params = RuleParams {
        scale_out: Some(vec![
            ExitStage {
                sell_bps: Some(7000),
                take_profit: Some(50.0),
                conditions: SideConditions::default(),
            },
            ExitStage {
                sell_bps: None,
                take_profit: None,
                conditions: remainder,
            },
        ]),
        ..RuleParams::default()
    };
    assert_parity("scale_out_2stage", params, &tokens, at(1000.0));
}

/// Same ladder plus a global SL — after the bank, a crash must close the stub
/// via the global side (not another stage), matching `decide_arm` priority.
#[test]
fn scan_matches_replay_scale_out_global_sl_mid_ladder() {
    let tokens = vec![token(
        "scale_sl",
        vec![
            ct(0.0, true, 1.0, 1.0, 100.0), // exact 1.0 entry (see two_stage note)
            ct(10.0, true, 0.5, 1.50, 100.0), // bank 70% at +50%
            ct(20.0, false, 0.5, 0.60, 100.0), // −40% from entry → global SL
        ],
    )];
    let params = RuleParams {
        stop_loss: Some(30.0),
        scale_out: Some(vec![ExitStage {
            sell_bps: Some(7000),
            take_profit: Some(50.0),
            conditions: SideConditions::default(),
        }]),
        ..RuleParams::default()
    };
    assert_parity("scale_out_sl_mid", params, &tokens, at(1000.0));
}

// ───────────── flow-scalper metrics parity (Phase 6 sweep wiring) ─────────────
//
// The two new metric classes the flow scalper needs: the windowed price-extrema
// entry (`m_price_window.trail`, a real precompute column) and the position-scoped
// trailing-stop exit (`m_position.{retrace,pnl}`, evaluated against the per-entry
// PositionCtx during the scan — NOT a token series column). These lock the scan's
// position-aware exit + price-window entry against the real fold.

/// A token that rises then dips ≥ 12% below its rolling high (so the price-window
/// entry triggers) with a later print to fill against, plus a run-up and a peak-then-
/// dump so both TP and the trailing stop have something to fire on.
fn pw_dip_corpus() -> Vec<(CorpusToken, ReplayToken)> {
    vec![
        token(
            "pw_dip",
            vec![
                ct(0.0, true, 2.0, 1.0, 100.0),
                ct(1.0, true, 2.0, 1.2, 100.0), // 30 s rolling high = 1.2
                ct(2.0, false, 1.0, 1.0, 100.0), // 16.7% below high → dip entry triggers
                ct(3.0, true, 2.0, 1.05, 100.0), // worst-case entry fill lands around here
                ct(10.0, true, 2.0, 1.8, 100.0), // run-up (peak since entry)
                ct(12.0, false, 1.0, 1.0, 100.0), // ~44% off the 1.8 peak → trailing stop
            ],
        ),
        // Never dips 12% off its rolling high → the window entry never fires.
        token(
            "pw_flat",
            vec![
                ct(0.0, true, 2.0, 1.0, 100.0),
                ct(1.0, true, 2.0, 1.02, 100.0),
                ct(2.0, true, 2.0, 1.05, 100.0),
            ],
        ),
    ]
}

#[test]
fn scan_matches_replay_position_retrace_exit() {
    // Trailing stop: exit ≥ 3% below the since-entry peak (enter-on-arm). The
    // `metrics` token peaks at 2.0 then dips to 0.8 → fires; the position-scoped
    // metric must read the running PositionCtx, not a (NaN) series column.
    assert_parity(
        "position_retrace",
        RuleParams {
            take_profit: None,
            stop_loss: None,
            entry: None,
            exit: Some(static_metric_side(MetricGroupId::Position, MetricId::Retrace, Operator::Gte, 3.0)),
            ..RuleParams::default()
        },
        &corpus(),
        at(1000.0),
    );
    assert_parity(
        "position_retrace_gappy",
        RuleParams {
            take_profit: None,
            stop_loss: None,
            entry: None,
            exit: Some(static_metric_side(MetricGroupId::Position, MetricId::Retrace, Operator::Gte, 3.0)),
            ..RuleParams::default()
        },
        &gappy_corpus(),
        at(100_000.0),
    );
}

#[test]
fn scan_matches_replay_armed_trailing_exit() {
    // `m_position.arm_above_pct` — the trailing stop held off until the position is
    // in profit. Two things must hold at once, which is why this is a parity test
    // and not just an engine unit test:
    //
    //   1. the scan's `exit_req_fires` must skip a disarmed trailing req exactly as
    //      `CompiledRule::exit_fired` does, and
    //   2. an armed req must NOT take the `ExitClass::Trailing` prefix-extrema hull
    //      — arming is a conjunction of retrace and pnl, which the hull does not
    //      index, so `classify_exit_req` sends it to the scalar walk. If that
    //      classification ever regresses the hull answers a question it cannot see
    //      the gate on, and only a parity assertion catches it.
    //
    // Gates on both sides of the corpus's price path: 0 (arm at break-even) and a
    // gate high enough that the trail never arms and the stop-loss has to close it.
    for gate in [0.0, 5.0, 40.0] {
        let mut gc = GroupConditions::default();
        gc.metrics.insert(
            MetricId::Retrace,
            vec![vec![Condition { operator: Operator::Gte, value: 3.0 }]],
        );
        gc.strict.insert("arm_above_pct".into(), gate);
        let mut side = SideConditions::default();
        side.0.insert(MetricGroupId::Position, vec![gc]);

        let params = RuleParams {
            take_profit: None,
            stop_loss: Some(20.0),
            entry: None,
            exit: Some(side),
            ..RuleParams::default()
        };
        assert_parity(&format!("armed_trailing_gate_{gate}"), params.clone(), &corpus(), at(1000.0));
        assert_parity(
            &format!("armed_trailing_gappy_gate_{gate}"),
            params,
            &gappy_corpus(),
            at(100_000.0),
        );
    }
}

#[test]
fn scan_matches_replay_position_pnl_exit() {
    // Authored `m_position.pnl <= -20` — a metric stop (stamps Metrics, distinct from
    // the desugared `stop_loss` which stamps StopLoss). Position-scoped, so the scan
    // evaluates it against the PositionCtx pnl at each row.
    let params = exit_metric(MetricGroupId::Position, MetricId::Pnl, Operator::Lte, -20.0);
    assert_parity("position_pnl", params, &corpus(), at(1000.0));
}

#[test]
fn scan_matches_replay_position_held_exit() {
    // `m_position.held >= 5` — a time stop. New fast-path class (binary search on the
    // series' `at` column), and one no sweep path could resolve without walking every
    // row before item B; locked against the fold on both a dense and a gappy corpus.
    let params = exit_metric(MetricGroupId::Position, MetricId::Held, Operator::Gte, 5.0);
    assert_parity("position_held", params.clone(), &corpus(), at(1000.0));
    assert_parity("position_held_gappy", params, &gappy_corpus(), at(100_000.0));
}

#[test]
fn scan_matches_replay_tp_sl_plus_authored_metric() {
    // Mixed classes AND mixed origins on one rule: the desugared SL/TP must still
    // outrank the authored trailing stop when both fire on the same row, and the
    // labels must stay `StopLoss`/`TakeProfit` rather than collapsing to `Metrics`.
    let params = RuleParams {
        take_profit: Some(50.0),
        stop_loss: Some(30.0),
        entry: None,
        exit: Some(static_metric_side(MetricGroupId::Position, MetricId::Retrace, Operator::Gte, 3.0)),
        ..RuleParams::default()
    };
    assert_parity("tp_sl_plus_retrace", params, &corpus(), at(1000.0));
}

#[test]
fn scan_matches_replay_price_window_dip_entry() {
    // Entry on `m_price_window(30).trail >= 12` (buy a 12%+ dip off the rolling high),
    // exit via TP. Exercises the windowed price-extrema entry column end-to-end.
    let entry = window_metric_side(MetricGroupId::PriceWindow, MetricId::WinTrail, 30.0, Operator::Gte, 12.0);
    let params =
        RuleParams { take_profit: Some(20.0), stop_loss: None, entry: Some(entry), exit: None, ..RuleParams::default() };
    assert_parity("pw_dip_entry", params, &pw_dip_corpus(), at(1000.0));
}

#[test]
fn position_retrace_actually_fires_the_trailing_stop() {
    // Non-vacuity guard for the parity tests above: a scan≡replay assertion passes
    // trivially if NEITHER side ever enters, so prove the two new metrics really do
    // drive an entry and a close on this fixture.
    let compiled = CompiledRule::compile(&loaded(RuleParams {
        take_profit: None,
        stop_loss: None,
        entry: Some(window_metric_side(
            MetricGroupId::PriceWindow,
            MetricId::WinTrail,
            30.0,
            Operator::Gte,
            12.0,
        )),
        exit: Some(static_metric_side(MetricGroupId::Position, MetricId::Retrace, Operator::Gte, 3.0)),
        ..RuleParams::default()
    }));
    let cols = columns_for(&compiled);
    // The dip trigger IS a precomputed column; the position metric is NOT (it reads
    // the per-entry PositionCtx — a static column could only ever record NaN).
    assert!(
        cols.contains(&hunter_engine::metrics::series::SeriesColumn::Window(MetricId::WinTrail, 30.0)),
        "m_price_window.trail must precompute as a windowed column: {cols:?}"
    );
    assert!(
        !cols.iter().any(|c| matches!(
            c,
            hunter_engine::metrics::series::SeriesColumn::Static(MetricId::Retrace)
        )),
        "m_position must not become a series column: {cols:?}"
    );
    // The price-window decay region must be sized by the price window, not just flows.
    assert_eq!(sparse_grid_for(&compiled).max_window_secs, 30.0);

    let grid = sparse_grid_for(&compiled);
    let pricing = pricing();
    let toks = pw_dip_corpus();

    // The dipping token: enters on the 16.7% dip, closes on the post-peak retrace.
    let (dip, _) = &toks[0];
    let series = build_series_with_flow(dip, cols.clone(), &grid, at(1000.0), None);
    let out = scan(&dip.trades, &series, &compiled, &pricing);
    assert!(out.fired, "the 12% dip entry must fire");
    assert_eq!(out.exit, ExitCode::Metrics, "the retrace trailing stop must close it");
    assert!(
        out.exit_price.is_some_and(|p| p < 1.8),
        "exit must price off the post-peak dump, got {:?}",
        out.exit_price
    );

    // The flat token never dips 12% off its rolling high → never enters.
    let (flat, _) = &toks[1];
    let flat_series = build_series_with_flow(flat, cols, &grid, at(1000.0), None);
    let flat_out = scan(&flat.trades, &flat_series, &compiled, &pricing);
    assert!(!flat_out.fired, "no 12% dip → no entry");
}

#[test]
fn scan_matches_replay_minimal_flow_scalper_core() {
    // The irreducible 2-metric core: dip entry (price_window trail >= 12) + trailing-
    // stop exit (position retrace >= 3), no TP/SL. Both new metric classes together.
    let entry = window_metric_side(MetricGroupId::PriceWindow, MetricId::WinTrail, 30.0, Operator::Gte, 12.0);
    let exit = static_metric_side(MetricGroupId::Position, MetricId::Retrace, Operator::Gte, 3.0);
    let params =
        RuleParams { take_profit: None, stop_loss: None, entry: Some(entry), exit: Some(exit), ..RuleParams::default() };
    assert_parity("flow_scalper_core", params, &pw_dip_corpus(), at(1000.0));
}

// ───────────────────── sparse-grid parity (plan §P2) ─────────────────────
//
// The sparse series omits provably-static ticks in long quiet gaps. These
// fixtures put the decision points *inside* those gaps — a long silence, a
// dead-flip mid-gap, a mid-gap time/stall threshold (with `=` tolerance), a
// window flow that decays across the gap, and a token that revives after the
// gap — so a dropped or misplaced tick surfaces as a scan≠replay divergence.

/// Fixtures with multi-hour gaps between trades — the sparse grid's stress case.
fn gappy_corpus() -> Vec<(CorpusToken, ReplayToken)> {
    vec![
        // Revive: early worst-case entry fill, 2 h silence, then live prints (also
        // enough post-gap buys for a deferred time-gate entry to fill).
        token(
            "revive",
            vec![
                ct(0.0, true, 2.0, 1.0, 100.0),
                ct(1.0, true, 2.0, 1.05, 100.0),
                ct(7200.0, true, 2.0, 1.2, 100.0),
                ct(7201.0, true, 2.0, 1.25, 100.0),
            ],
        ),
        // Dead mid-gap: enter via in-window fill, liquidity gone, silent past the
        // 300 s quiet window inside a long gap, then a late trade.
        token(
            "dead_midgap",
            vec![
                ct(0.0, true, 1.0, 1.0, 5.0),
                ct(1.0, true, 0.5, 1.05, 5.0),
                ct(7200.0, true, 1.0, 1.0, 5.0),
            ],
        ),
        // Healthy but idle: trigger + fill, reserves fine, rides to Open in the tail.
        token(
            "idle",
            vec![ct(0.0, true, 2.0, 1.0, 100.0), ct(1.0, true, 2.0, 1.05, 100.0)],
        ),
    ]
}

#[test]
fn scan_matches_replay_gappy_dead_open() {
    // Enter-on-arm, no exits: dead_midgap → Dead mid-gap, revive/idle → Open.
    let params = RuleParams { take_profit: None, stop_loss: None, entry: None, exit: None, ..RuleParams::default() };
    assert_parity("gappy_dead_open", params, &gappy_corpus(), at(100_000.0));
}

#[test]
fn scan_matches_replay_time_gate_across_gap() {
    // Entry gated on time > 3600 s — qualifies mid-gap, so the fill lands on a tick
    // deep inside the quiet span the sparse grid must still emit.
    let mut gc = GroupConditions::default();
    gc.metrics.insert(MetricId::Time, vec![vec![Condition { operator: Operator::Gt, value: 3600.0 }]]);
    let mut entry = SideConditions::default();
    entry.0.insert(MetricGroupId::Snapshot, vec![gc]);
    let params =
        RuleParams { take_profit: Some(5.0), stop_loss: None, entry: Some(entry), exit: None, ..RuleParams::default() };
    assert_parity("time_gate_gap", params, &gappy_corpus(), at(100_000.0));
}

#[test]
fn scan_matches_replay_stall_eq_exit_across_gap() {
    // Exit when stall ≈ 1800 s (`=` with the metric's tolerance) — a tolerance-edged
    // threshold reached only inside the gap. Exercises both region boundaries.
    let params = exit_metric(MetricGroupId::PriceLifetime, MetricId::Stall, Operator::Eq, 1800.0);
    assert_parity("stall_eq_gap", params, &gappy_corpus(), at(100_000.0));
}

/// A combo bound **once** and reused across every token must produce exactly what
/// binding per token produces.
///
/// This is the guard for resolving series-column indices at bind time
/// (`BoundCombo`). The sweep binds a combo once per shard/group and scans every
/// token with it, which is only sound because every token's series is built from the
/// same fixed column set. The `scan_matches_replay_*` tests above cannot catch a
/// break: each binds against the very series it then scans, so a stale or mismatched
/// index would agree with itself. Here the bind is deliberately detached from the
/// tokens — the shape the sweep actually runs.
#[test]
fn shared_bind_matches_per_token_bind() {
    use super::strategy::{resolve_entry, resolve_exit, BoundCombo};

    let pricing = pricing();
    let as_of = at(100_000.0);
    // Two rule shapes (metric-exit and TP/SL) over both corpora, so the entry, exit
    // and mono-kill column sets are all exercised on tokens with differing series
    // lengths and gap structure.
    let rules = [
        exit_metric(MetricGroupId::PriceLifetime, MetricId::Stall, Operator::Gte, 1800.0),
        RuleParams {
            take_profit: Some(50.0),
            stop_loss: Some(30.0),
            entry: None,
            exit: None,
            ..RuleParams::default()
        },
    ];

    for params in rules {
        let compiled = CompiledRule::compile(&loaded(params));
        let cols = columns_for(&compiled);
        let grid = sparse_grid_for(&compiled);
        // Bound ONCE, against the run's column set — never re-derived per token.
        let shared = BoundCombo::new(&cols, compiled.clone());

        for (corpus_tok, _) in corpus().iter().chain(gappy_corpus().iter()) {
            let series = build_series_with_flow(corpus_tok, cols.clone(), &grid, as_of, None);

            let entry = resolve_entry(&corpus_tok.trades, &series, &shared, &pricing);
            let shared_out = resolve_exit(&corpus_tok.trades, &series, &shared, &entry, &pricing, None);
            // `scan` binds against this token's own series — the reference.
            let per_token_out = scan(&corpus_tok.trades, &series, &compiled, &pricing);

            let m = &corpus_tok.mint;
            assert_eq!(shared_out.fired, per_token_out.fired, "{m}: fired");
            assert_eq!(shared_out.exit, per_token_out.exit, "{m}: exit code");
            assert_eq!(shared_out.entry_price, per_token_out.entry_price, "{m}: entry price");
            assert_eq!(shared_out.exit_price, per_token_out.exit_price, "{m}: exit price");
            assert_eq!(shared_out.pnl_sol, per_token_out.pnl_sol, "{m}: pnl_sol");
            assert_eq!(shared_out.pnl_percent, per_token_out.pnl_percent, "{m}: pnl_pct");
            assert_eq!(shared_out.holding_secs, per_token_out.holding_secs, "{m}: holding");
        }
    }
}

#[test]
fn scan_matches_replay_window_flow_across_gap() {
    // Exit when the 60 s gross-flow window drops to 0 — flows decay to 0 partway
    // through the gap, so the decay-region ticks must be present and exact.
    let mut gc = GroupConditions::default();
    gc.strict.insert("window_size_sec".to_string(), 60.0);
    gc.metrics.insert(MetricId::GrossFlow, vec![vec![Condition { operator: Operator::Lte, value: 0.0 }]]);
    let mut exit = SideConditions::default();
    exit.0.insert(MetricGroupId::FlowWindow, vec![gc]);
    let params =
        RuleParams { take_profit: None, stop_loss: None, entry: None, exit: Some(exit), ..RuleParams::default() };
    assert_parity("flow_decay_gap", params, &gappy_corpus(), at(100_000.0));
}

/// Corpus for volume/organic flow split — labeled trades + configured patterns.
fn flow_corpus() -> Vec<(CorpusToken, ReplayToken)> {
    vec![
        // Enters on volume-side buy (vol_net≥3), exits when organic window goes quiet.
        token(
            "vol_entry",
            vec![
                // Organic first — not enough vol_net to enter.
                ct_flow(1.0, true, 1.0, 1.0, 100.0, None, Some("org1")),
                // Volume-side pattern match → vol_net=3 → entry.
                ct_flow(2.0, true, 3.0, 1.1, 103.0, Some(&["vol"]), Some("vol1")),
            ],
        ),
        // Never matches the volume pattern → never enters.
        token(
            "organic_only",
            vec![
                ct_flow(1.0, true, 5.0, 1.0, 100.0, Some(&["Pump.Fun: Buy"]), Some("org2")),
                ct_flow(3.0, true, 2.0, 1.2, 102.0, None, Some("org3")),
            ],
        ),
    ]
}

#[test]
fn scan_matches_replay_flow_split_entry_and_window_exit() {
    // Mirror the engine golden: enter on vol_net > 2, exit when trailing
    // nonvol_gross (5 s) ≈ 0. Patterns + scan FP id must stay aligned with replay.
    let mut entry_gc = GroupConditions::default();
    entry_gc.metrics.insert(
        MetricId::VolNet,
        vec![vec![Condition { operator: Operator::Gt, value: 2.0 }]],
    );
    let mut entry = SideConditions::default();
    entry.0.insert(MetricGroupId::FlowSplit, vec![entry_gc]);

    let mut exit_gc = GroupConditions::default();
    exit_gc.strict.insert("window_size_sec".to_string(), 5.0);
    exit_gc.metrics.insert(
        MetricId::WinNonvolGross,
        vec![vec![Condition { operator: Operator::Eq, value: 0.0 }]],
    );
    let mut exit = SideConditions::default();
    exit.0.insert(MetricGroupId::FlowSplitWindow, vec![exit_gc]);

    let params = RuleParams {
        take_profit: None,
        stop_loss: None,
        entry: Some(entry),
        exit: Some(exit),
        ..RuleParams::default()
    };
    let patterns = vec![vec!["vol".to_string()]];
    assert_parity_with_flow(
        "flow_split",
        params,
        &flow_corpus(),
        at(1000.0),
        Some(patterns),
    );
}

// ───────────────── scalar ≡ AVX-512 exit-scan parity (plan §P3) ─────────────────
//
// `resolve_exit_simd` (the vector path the frontend toggle selects) must produce a
// **byte-identical** `TokenOutcome` to the scalar `resolve_exit` — same exit code,
// entry/exit prices, times, slots and PnL — for every rule shape and token. This is
// the SSOT safety net the toggle rests on (locked design decision 2): the strategy.rs
// unit tests prove the kernel's first-exit-row search; this proves the whole outcome,
// end to end, on the real sparse series (including the metrics-fallback branch, where
// `resolve_exit_simd` delegates straight to scalar). On a non-AVX-512 host the SIMD
// entry point falls back to scalar, so the assertion holds there too.

/// Every exit-outcome field, as a comparable tuple.
fn outcome_tuple(
    o: &TokenOutcome,
) -> (bool, ExitCode, Option<f64>, Option<f64>, Option<u64>, Option<u64>, i64, f32, f32) {
    (
        o.fired,
        o.exit,
        o.entry_price,
        o.exit_price,
        o.entry_slot,
        o.exit_slot,
        o.holding_secs,
        o.pnl_sol,
        o.pnl_percent,
    )
}

/// `assert_eq` on `outcome_tuple` fails when both sides carry `NaN` (NaN ≠ NaN).
/// Dead exits on a NaN-priced row produce that shape — compare bit-patterns instead.
fn assert_outcomes_eq(a: &TokenOutcome, b: &TokenOutcome, msg: &str) {
    let ta = outcome_tuple(a);
    let tb = outcome_tuple(b);
    let eq_opt = |x: Option<f64>, y: Option<f64>| match (x, y) {
        (Some(x), Some(y)) => x.to_bits() == y.to_bits(),
        (None, None) => true,
        _ => false,
    };
    let ok = ta.0 == tb.0
        && ta.1 == tb.1
        && eq_opt(ta.2, tb.2)
        && eq_opt(ta.3, tb.3)
        && ta.4 == tb.4
        && ta.5 == tb.5
        && ta.6 == tb.6
        && ta.7.to_bits() == tb.7.to_bits()
        && ta.8.to_bits() == tb.8.to_bits();
    assert!(ok, "{msg}:\n  left:  {ta:?}\n  right: {tb:?}");
}

#[test]
fn simd_exit_scan_matches_scalar_across_paths() {
    use super::exit_index::ExitIndex;
    use super::strategy::{resolve_entry, resolve_exit, resolve_exit_simd, BoundCombo};

    let pricing = pricing();
    let as_of = at(100_000.0);

    // Entry gated on time > 2 s so the fill defers past the first print — exercises a
    // non-zero `fill_row` (the SIMD scan starts at `fill_row + 1`).
    let entry_gate = {
        let mut gc = GroupConditions::default();
        gc.metrics.insert(MetricId::Time, vec![vec![Condition { operator: Operator::Gt, value: 2.0 }]]);
        let mut entry = SideConditions::default();
        entry.0.insert(MetricGroupId::Snapshot, vec![gc]);
        RuleParams { take_profit: Some(20.0), stop_loss: Some(60.0), entry: Some(entry), exit: None, ..RuleParams::default() }
    };

    let rules = vec![
        // TP + SL together.
        RuleParams { take_profit: Some(50.0), stop_loss: Some(30.0), entry: None, exit: None, ..RuleParams::default() },
        // TP only / SL only (one threshold absent → the `have_sl`/`have_tp` branches).
        RuleParams { take_profit: Some(20.0), stop_loss: None, entry: None, exit: None, ..RuleParams::default() },
        RuleParams { take_profit: None, stop_loss: Some(40.0), entry: None, exit: None, ..RuleParams::default() },
        // Neither → only Dead / Open can fire.
        RuleParams { take_profit: None, stop_loss: None, entry: None, exit: None, ..RuleParams::default() },
        // Deferred entry (non-zero fill row).
        entry_gate,
        // Metrics exit → SIMD delegates to scalar; still must match.
        exit_metric(MetricGroupId::PriceLifetime, MetricId::Trail, Operator::Gt, 50.0),
    ];

    for (i, params) in rules.into_iter().enumerate() {
        let compiled = CompiledRule::compile(&loaded(params));
        let cols = columns_for(&compiled);
        let grid = sparse_grid_for(&compiled);
        // Bind once against the run's column set — the shape the sweep actually runs.
        let bound = BoundCombo::new(&cols, compiled.clone());

        for (corpus_tok, _) in corpus().iter().chain(gappy_corpus().iter()) {
            let series = build_series_with_flow(corpus_tok, cols.clone(), &grid, as_of, None);
            let trades = &corpus_tok.trades;
            let entry = resolve_entry(trades, &series, &bound, &pricing);
            let mut idx = ExitIndex::default();
            match &entry {
                super::strategy::EntryResolution::Entered { fill_row, .. }
                    if wants_exit_index(&bound, &entry) =>
                {
                    idx.rebuild(&series, *fill_row);
                }
                _ => idx.clear(),
            }
            let scalar = resolve_exit(trades, &series, &bound, &entry, &pricing, None);
            let simd = resolve_exit_simd(trades, &series, &bound, &entry, &pricing, &idx, None);
            assert_eq!(
                outcome_tuple(&simd),
                outcome_tuple(&scalar),
                "rule[{i}] token '{}': AVX-512 exit scan diverged from scalar",
                corpus_tok.mint,
            );
        }
    }
}

/// **Reachability lock** (item B's regression, not its correctness).
///
/// From Phase 2 until this change, the exit index and the AVX-512 scan were dead for
/// every TP/SL rule: desugaring turned `take_profit`/`stop_loss` into `pnl` exit reqs,
/// which made `has_exit_metrics()` — the flag both fast paths gated on — true. Nothing
/// failed, because the scalar fallback they dropped into is the correct reference. The
/// absence of *this* assertion is what let that rot for a whole phase, so it asserts
/// the gate is open, on a real entered token, for the exact rule shape it was built for.
#[test]
fn tp_sl_rules_actually_reach_the_exit_index() {
    use super::exit_index::ExitIndex;
    use super::strategy::{resolve_entry, BoundCombo, EntryResolution};

    let pricing = pricing();
    let as_of = at(1000.0);
    // Every pure price-ladder shape: TP+SL, TP only, SL only.
    let ladders = [
        (Some(50.0), Some(30.0)),
        (Some(20.0), None),
        (None, Some(40.0)),
    ];

    let mut entered_tokens = 0;
    for (tp, sl) in ladders {
        let compiled = CompiledRule::compile(&loaded(RuleParams {
            take_profit: tp,
            stop_loss: sl,
            entry: None,
            exit: None,
            ..RuleParams::default()
        }));
        // The shape the old gate got wrong: desugared reqs exist, so the pre-fix
        // branch would have refused the index here.
        assert!(
            compiled.has_exit_metrics(),
            "TP/SL desugars into exit reqs — that is what broke the old gate"
        );
        let cols = columns_for(&compiled);
        let grid = sparse_grid_for(&compiled);
        let bound = BoundCombo::new(&cols, compiled.clone());
        assert!(bound.fast_exit, "a pure TP/SL rule must classify entirely");

        for (corpus_tok, _) in corpus().iter() {
            let series = build_series_with_flow(corpus_tok, cols.clone(), &grid, as_of, None);
            let entry = resolve_entry(&corpus_tok.trades, &series, &bound, &pricing);
            let EntryResolution::Entered { fill_row, .. } = entry else { continue };
            entered_tokens += 1;
            assert!(
                wants_exit_index(&bound, &entry),
                "{}: TP/SL entry must open the index gate",
                corpus_tok.mint
            );
            let mut idx = ExitIndex::default();
            idx.rebuild(&series, fill_row);
            assert!(idx.is_ready(), "{}: the rebuilt index must be usable", corpus_tok.mint);
        }
    }
    // Non-vacuity: the loop above must actually have entered somewhere.
    assert!(entered_tokens > 0, "no token entered — the assertions above proved nothing");
}

#[test]
fn index_exit_scan_matches_scalar_across_paths() {
    use super::exit_index::ExitIndex;
    use super::strategy::{resolve_entry, resolve_exit, resolve_exit_indexed, BoundCombo};

    let pricing = pricing();
    let as_of = at(100_000.0);

    let entry_gate = {
        let mut gc = GroupConditions::default();
        gc.metrics.insert(
            MetricId::Time,
            vec![vec![Condition { operator: Operator::Gt, value: 2.0 }]],
        );
        let mut entry = SideConditions::default();
        entry.0.insert(MetricGroupId::Snapshot, vec![gc]);
        RuleParams {
            take_profit: Some(20.0),
            stop_loss: Some(60.0),
            entry: Some(entry),
            exit: None,
            ..RuleParams::default()
        }
    };

    let rules = vec![
        RuleParams { take_profit: Some(50.0), stop_loss: Some(30.0), entry: None, exit: None, ..RuleParams::default() },
        RuleParams { take_profit: Some(20.0), stop_loss: None, entry: None, exit: None, ..RuleParams::default() },
        RuleParams { take_profit: None, stop_loss: Some(40.0), entry: None, exit: None, ..RuleParams::default() },
        RuleParams { take_profit: None, stop_loss: None, entry: None, exit: None, ..RuleParams::default() },
        entry_gate,
        // Metrics → index falls back to scalar; still must match.
        exit_metric(MetricGroupId::PriceLifetime, MetricId::Trail, Operator::Gt, 50.0),
        // The three position classes the fast path now resolves itself: a trailing
        // stop (O(n) running peak), a time stop (binary search on `at`), and an
        // authored `pnl` bound (hull, but labelled `Metrics` not `StopLoss`).
        exit_metric(MetricGroupId::Position, MetricId::Retrace, Operator::Gte, 3.0),
        exit_metric(MetricGroupId::Position, MetricId::Held, Operator::Gte, 5.0),
        exit_metric(MetricGroupId::Position, MetricId::Pnl, Operator::Lte, -20.0),
        // Mixed classes on one rule — the earliest row across hull + scan wins, and
        // the tie-break must keep the desugared SL/TP ahead of the authored metric.
        RuleParams {
            take_profit: Some(50.0),
            stop_loss: Some(30.0),
            entry: None,
            exit: Some(static_metric_side(
                MetricGroupId::Position,
                MetricId::Retrace,
                Operator::Gte,
                3.0,
            )),
            ..RuleParams::default()
        },
    ];

    for (i, params) in rules.into_iter().enumerate() {
        let compiled = CompiledRule::compile(&loaded(params));
        let cols = columns_for(&compiled);
        let grid = sparse_grid_for(&compiled);
        let bound = BoundCombo::new(&cols, compiled.clone());

        for (corpus_tok, _) in corpus().iter().chain(gappy_corpus().iter()) {
            let series = build_series_with_flow(corpus_tok, cols.clone(), &grid, as_of, None);
            let trades = &corpus_tok.trades;
            let entry = resolve_entry(trades, &series, &bound, &pricing);
            let mut idx = ExitIndex::default();
            match &entry {
                super::strategy::EntryResolution::Entered { fill_row, .. }
                    if wants_exit_index(&bound, &entry) =>
                {
                    idx.rebuild(&series, *fill_row);
                }
                _ => idx.clear(),
            }
            let scalar = resolve_exit(trades, &series, &bound, &entry, &pricing, None);
            let indexed = resolve_exit_indexed(trades, &series, &bound, &entry, &pricing, &idx, None);
            assert_outcomes_eq(
                &indexed,
                &scalar,
                &format!("rule[{i}] token '{}': exit-index diverged from scalar", corpus_tok.mint),
            );
        }
    }
}

#[test]
fn index_exit_scan_matches_scalar_on_randomized_walks() {
    use super::exit_index::ExitIndex;
    use super::strategy::{resolve_entry, resolve_exit, resolve_exit_indexed, BoundCombo};
    use hunter_engine::metrics::series::MetricSeries;
    use hunter_engine::metrics::{Side, TradeLite};

    let pricing = pricing();
    // Deterministic LCG — no rand dep needed in this module.
    let mut seed: u64 = 0xC0FFEE_u64;
    let mut next = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        seed
    };

    let tp_sl_grid: &[(Option<f64>, Option<f64>)] = &[
        (Some(20.0), Some(30.0)),
        (Some(50.0), None),
        (None, Some(40.0)),
        (Some(10.0), Some(10.0)),
        (None, None),
    ];

    for case in 0..32 {
        let n = 8 + (next() % 40) as usize;
        let t0 = base();
        let mut series = MetricSeries::new(t0, vec![]);
        let mut trades: Vec<CorpusTrade> = Vec::with_capacity(n);
        let mut price = 1.0_f64;
        for i in 0..n {
            let r = next();
            // ~12% NaN-like hole via a tick with no price change from a prior NaN overwrite.
            let is_hole = r % 8 == 0;
            let shock = ((r >> 8) % 21) as i64 - 10; // -10..+10
            if !is_hole {
                price = (price * (1.0 + shock as f64 * 0.03)).max(0.01);
            }
            let at = t0 + Duration::milliseconds((i as i64) * 500);
            let is_buy = r % 2 == 0;
            let sol = 1.0 + (r % 5) as f64;
            let slot = i as u64;
            series.push_trade_at(
                TradeLite {
                    side: if is_buy { Side::Buy } else { Side::Sell },
                    sol,
                    price,
                    reserve_sol: 50.0,
                    at,
                    ..Default::default()
                },
                Some(slot),
            );
            trades.push(CorpusTrade {
                block_time: at,
                amount_sol: sol,
                token_amount: 1.0,
                price_per_token: price,
                reserve_sol: Some(50.0),
                reserve_token: Some(1.0),
                real_reserve_sol: Some(50.0),
                real_token_reserves: Some(1.0),
                slot,
                leg_index: 0,
                is_buy,
                tx_signature: None,
                ix_labels: None,
                wallet: None,
            });
            if is_hole {
                // Overwrite to NaN after push so the hull carry path is exercised.
                let last = series.n_rows() - 1;
                series.price[last] = f64::NAN;
            }
            // Sparse dead flags near the tail.
            if i + 3 >= n && (r % 5 == 0) {
                let last = series.n_rows() - 1;
                series.dead[last] = true;
            }
        }

        for (ti, (tp, sl)) in tp_sl_grid.iter().enumerate() {
            let params = RuleParams {
                take_profit: *tp,
                stop_loss: *sl,
                entry: None,
                exit: None,
                ..RuleParams::default()
            };
            let compiled = CompiledRule::compile(&loaded(params));
            let bound = BoundCombo::new(series.columns(), compiled);
            let entry = resolve_entry(&trades, &series, &bound, &pricing);
            let mut idx = ExitIndex::default();
            if let super::strategy::EntryResolution::Entered { fill_row, .. } = &entry {
                idx.rebuild(&series, *fill_row);
            }
            let scalar = resolve_exit(&trades, &series, &bound, &entry, &pricing, None);
            let indexed = resolve_exit_indexed(&trades, &series, &bound, &entry, &pricing, &idx, None);
            assert_outcomes_eq(
                &indexed,
                &scalar,
                &format!("rand case={case} grid[{ti}]: exit-index diverged from scalar"),
            );
        }
    }
}

// ─────────────── multi-token aggregate parity — the D1 backstop ────────────────
//
// **This is the guard the single-token locks above cannot be.** They replay each token
// in ISOLATION, so `run_replay`'s tail (bounded by the CORPUS-wide last trade) and the
// sweep's per-token tail truncate at the exact same instant — the D1 asymmetry is
// invisible by construction. The fix (`resolve_frozen_tail`) is therefore also a no-op
// there. Here a MULTI-token corpus makes the corpus tail outrun a quiet token's own
// cut, so a deterministic clock exit that lands in that gap must close in the sweep
// exactly as it does in a multi-token simulate. The `legacy` control folds the same
// corpus with the resolve OFF and asserts it DIVERGES — i.e. this test fails if Task 1
// is reverted, which is the whole point (parity plan Task 2).

/// A ≥3-token corpus with overlapping lifetimes and one quiet-but-still-liquid token
/// (`quiet`) whose exit lands after its own last trade. All created at `base()` except
/// `young`. Under `time > 1000`:
/// * `quiet` (last @50, own cut ≈410) — the D1 shape: `time` crosses 1000 in the corpus
///   tail, past its own cut. Open without the fix, closed with it.
/// * `active` (last @1200) — crosses INSIDE its own dense/`time`-horizon region, so it
///   closes in-series either way (a normal closed token that also extends the corpus
///   tail to 1200 → horizon 1560).
/// * `young` (created @800) — at horizon 1560 its `time` is only 760, so it never
///   crosses: genuinely Open in BOTH sweep and simulate.
fn d1_time_corpus() -> Vec<(CorpusToken, ReplayToken)> {
    vec![
        token(
            "quiet",
            vec![
                ct(0.0, true, 1.0, 1.0, 100.0),
                ct(1.0, true, 0.5, 1.05, 100.0),
                ct(50.0, true, 0.5, 1.1, 100.0),
            ],
        ),
        token(
            "active",
            vec![
                ct(0.0, true, 1.0, 1.0, 100.0),
                ct(1.0, true, 0.5, 1.02, 100.0),
                ct(1200.0, true, 0.5, 1.3, 100.0),
            ],
        ),
        token_at(
            "young",
            800.0,
            vec![
                ct(800.0, true, 1.0, 1.0, 100.0),
                ct(801.0, true, 0.5, 1.05, 100.0),
                ct(850.0, true, 0.5, 1.1, 100.0),
            ],
        ),
    ]
}

/// Fold a `run_replay` outcome into the same core aggregate the sweep uses, pricing the
/// round-trip through the shared kernel (open ⇒ marked to last price, as the sweep does).
fn replay_outcome_to_token(po: &PositionOutcome, cost: &CostModel) -> TokenOutcome {
    let (pnl_sol, pnl_pct) = po.pnl_with_costs(BUY_SOL, cost);
    TokenOutcome {
        fired: true,
        holding_secs: po.exit_time.map(|t| (t - po.entry_time).num_seconds()).unwrap_or(0),
        pnl_percent: pnl_pct as f32,
        pnl_sol: pnl_sol as f32,
        exit: exit_code_of(po.exit_reason),
        exit_metric: None,
        exit_operator: None,
        exit_metric_value: None,
        exit_metric_slot: None,
        entry_time: None,
        entry_price: None,
        entry_slot: None,
        exit_time: None,
        exit_price: None,
        exit_slot: None,
    }
}

#[test]
fn scan_matches_replay_multi_token_frozen_tail() {
    use super::exit_index::ExitIndex;
    use super::strategy::{
        frozen_tail_horizon, resolve_entry, resolve_exit, resolve_exit_indexed, resolve_exit_simd,
        wants_exit_index, BoundCombo, EntryResolution,
    };
    use crate::sweep::aggregate::ComboAgg;

    let as_of = at(100_000.0);
    let corpus = d1_time_corpus();
    let corpus_tokens: Vec<CorpusToken> = corpus.iter().map(|(c, _)| c.clone()).collect();
    let replay_tokens: Vec<ReplayToken> = corpus.iter().map(|(_, r)| r.clone()).collect();
    // The corpus-wide tail cap `run_replay` bounds its tick loop by — the horizon the
    // sweep's per-token series never reaches on its own. D2 out of the way: caps are ∞
    // on both sides (`loaded` sets `max_concurrent_tokens: u32::MAX`, `run_replay`
    // honors it), so only the tail asymmetry is under test.
    let tail_horizon = frozen_tail_horizon(as_of, &corpus_tokens);
    assert!(tail_horizon.is_some(), "corpus has trades");

    // `time > 1000`: the plan's canonical deterministic clock (token-scoped ⇒ the scalar
    // path, and grid-horizon-aware so no mid-history tick is dropped). The `held` clock
    // is locked separately in `held_frozen_tail_matches_across_exit_paths` (its position
    // scope needs a tail crossing, not a mid-gap one).
    let params = exit_metric(MetricGroupId::Snapshot, MetricId::Time, Operator::Gt, 1000.0);

    for fm in FILL_MODELS {
        let pricing = pricing_for(fm);
        let cost = pricing.cost;
        let compiled = CompiledRule::compile(&loaded(params.clone()));
        let cols = columns_for(&compiled);
        let grid = sparse_grid_for(&compiled);
        let bound = BoundCombo::new(&cols, compiled.clone());

        let mut sweep = ComboAgg::default();
        let mut legacy = ComboAgg::default();

        for (corpus_tok, _) in &corpus {
            let series = build_series_with_flow(corpus_tok, cols.clone(), &grid, as_of, None);
            let trades = &corpus_tok.trades;
            let entry = resolve_entry(trades, &series, &bound, &pricing);
            let mut idx = ExitIndex::default();
            match &entry {
                EntryResolution::Entered { fill_row, .. } if wants_exit_index(&bound, &entry) => {
                    idx.rebuild(&series, *fill_row);
                }
                _ => idx.clear(),
            }
            let scalar = resolve_exit(trades, &series, &bound, &entry, &pricing, tail_horizon);
            // All three exit paths must resolve the frozen tail identically (time is a
            // `General` req, so index/simd delegate to scalar — asserted, not assumed).
            let indexed =
                resolve_exit_indexed(trades, &series, &bound, &entry, &pricing, &idx, tail_horizon);
            let simd =
                resolve_exit_simd(trades, &series, &bound, &entry, &pricing, &idx, tail_horizon);
            assert_outcomes_eq(&indexed, &scalar, &format!("{fm:?}/{} index", corpus_tok.mint));
            assert_outcomes_eq(&simd, &scalar, &format!("{fm:?}/{} simd", corpus_tok.mint));
            sweep.record(&scalar);
            legacy.record(&resolve_exit(trades, &series, &bound, &entry, &pricing, None));
        }

        let replay_out = run_replay(
            std::slice::from_ref(&loaded(params.clone())),
            std::slice::from_ref(&fingerprint()),
            replay_tokens.clone(),
            ReplayConfig { as_of, fill_model: fm },
        );
        let mut replay = ComboAgg::default();
        for po in &replay_out {
            replay.record(&replay_outcome_to_token(po, &cost));
        }

        let s = sweep.finalize(0);
        let r = replay.finalize(0);
        let l = legacy.finalize(0);

        assert_eq!(s.n_fired, r.n_fired, "{fm:?}: fired count");
        assert_eq!(
            s.n_closed, r.n_closed,
            "{fm:?}: closed count — the quiet-but-liquid token must close in the corpus tail (D1)"
        );
        assert_eq!(s.n_open, r.n_open, "{fm:?}: open count");
        assert!(
            (s.total_pnl_sol - r.total_pnl_sol).abs() < 1e-9,
            "{fm:?}: realized pnl (sweep {} vs replay {})",
            s.total_pnl_sol,
            r.total_pnl_sol
        );

        // Control (Task 2's raison d'être): with the resolve OFF the sweep leaves `quiet`
        // Open where simulate closes it, so the aggregate MUST diverge. If this ever
        // stops differing, the corpus no longer exercises D1 and every assert above is
        // vacuous — this is the assertion that fails when Task 1 is reverted.
        assert_ne!(
            (l.n_open, l.n_closed),
            (r.n_open, r.n_closed),
            "{fm:?}: control — the legacy per-token tail must diverge from simulate (D1)"
        );
    }
}

// ───────────── fold ≡ per-combo scan — the entry-cache poisoning lock ─────────────
//
// Every guard above drives ONE combo at a time, so the fold's entry cache is invisible
// to them by construction — which is exactly why the poisoning bug (2026-07-26) lived
// through the whole guard suite. The engine's `can_enter` veto makes the resolved entry
// **exit-dependent**, but the cache was keyed on the entry picks alone, so the first
// combo of an entry class donated its entered set to every sibling: wrong `n_fired`,
// wrong rows, wrong prices, on any sweep with exit-side metric axes.
//
// The lock below folds a real multi-combo `AxesModel` through the real fold
// (`fill_outcomes_with_state`) and asserts every combo's outcome equals its own
// single-combo `scan` AND its own `run_replay` — with a non-vacuity assert that the
// two exit variants really do resolve to different entries, so the test can never pass
// by measuring nothing.

/// One token whose liquidity steps up mid-history: an exit bound between the two
/// levels vetoes every early entry (the `can_enter` gate), while a looser bound on the
/// same axis vetoes nothing. Reserves stay above `DEAD_MAX_LIQUIDITY_SOL` (30) so the
/// two combos differ by the veto only, never by deadness.
fn veto_corpus() -> Vec<(CorpusToken, ReplayToken)> {
    vec![token(
        "veto",
        vec![
            ct(0.0, true, 1.0, 1.0, 50.0),
            ct(1.0, true, 0.5, 1.05, 50.0),
            ct(5.0, true, 0.5, 1.1, 50.0),
            ct(10.0, true, 0.5, 1.2, 150.0),
            ct(20.0, true, 0.5, 1.3, 150.0),
        ],
    )]
}

fn metric_axis(
    side: AxisSide,
    group: &str,
    metric: &str,
    operator: Operator,
    values: Vec<Option<f64>>,
) -> AxisSpec {
    AxisSpec {
        kind: "metric".to_string(),
        side: Some(side),
        group: Some(group.to_string()),
        metric: Some(metric.to_string()),
        operator: Some(operator),
        window: None,
        values,
    }
}

#[test]
fn fold_gives_each_exit_variant_its_own_entry() {
    use crate::sweep::engine::fill_outcomes_with_state;
    use crate::sweep::generic::axes::{AxesModel, AxesRequest};
    use crate::sweep::generic::strategy::{scan_with_horizon, GenericSweepStrategy};
    use crate::sweep::progress::NoopObserver;
    use crate::sweep::strategy::{ParamSpace, Strategy, SweepMethod};

    let as_of = at(100_000.0);
    let corpus = veto_corpus();

    // One entry axis (`time >= 1`, satisfied on every row from 1 s on — so the veto has
    // somewhere to move the entry TO) × one exit axis on a token-scoped metric. All
    // combos therefore share an `entry_key` and differ only on the exit side: the class
    // the old cache collapsed. The three exit picks veto at three different depths —
    // `< 40` never, `< 100` until reserves step up at t=10, `< 200` forever (no entry at
    // all) — so the class spans every shape the shared walk has to serve.
    let model = AxesModel::resolve(&AxesRequest {
        axes: vec![
            metric_axis(AxisSide::Entry, "m_snapshot", "time", Operator::Gte, vec![Some(1.0)]),
            metric_axis(
                AxisSide::Exit,
                "m_snapshot",
                "liquidity",
                Operator::Lt,
                vec![Some(40.0), Some(100.0), Some(200.0)],
            ),
        ],
    })
    .expect("axes resolve");

    for fm in FILL_MODELS {
        let pricing = pricing_for(fm);
        let strategy = GenericSweepStrategy::new(model.clone(), pricing, as_of, None);
        let mut params = strategy.sample(SweepMethod::Grid);
        strategy.order_for_entry_cache(&mut params);
        assert_eq!(params.len(), 3, "one entry pick × three exit picks");
        assert!(
            params.iter().all(|p| strategy.entry_key(p) == strategy.entry_key(&params[0])),
            "the fixture must put every combo in ONE entry class — otherwise the cache \
             is never even consulted and this test proves nothing"
        );

        for (corpus_tok, replay_tok) in &corpus {
            let series = strategy.prepare_token(corpus_tok);
            let trades = &corpus_tok.trades;

            // Fold the class BOTH ways. The shared walk is resumable state carried
            // across the combo loop, so a leak between siblings (a stale candidate
            // list, a sticky stop row) shows up as an order-dependent outcome — and
            // the reversed order also drives the deepest-vetoing combo first, which is
            // the case where later combos read candidates the walk already passed.
            for order in ["forward", "reversed"] {
                let params: Vec<_> = if order == "forward" {
                    params.clone()
                } else {
                    params.iter().rev().copied().collect()
                };
                let bound: Vec<_> = params.iter().map(|p| strategy.bind_param(p)).collect();

                // The real fold — the path a grouped sweep actually runs.
                let mut folded = Vec::new();
                fill_outcomes_with_state(
                    &strategy,
                    &params,
                    &bound,
                    corpus_tok,
                    &series,
                    &NoopObserver,
                    &mut folded,
                )
                .expect("fold");
                assert_eq!(folded.len(), params.len());

                for (i, p) in params.iter().enumerate() {
                    let rule = loaded(model.combo_params(p.idx));
                    let compiled = CompiledRule::compile(&rule);
                    let label = format!("{fm:?}/{}/{order}/combo{}", corpus_tok.mint, p.idx);

                    // (a) fold ≡ the uncached single-combo scan (the drill-in's path).
                    // The strategy carries no corpus horizon, so `None` is its tail too.
                    let solo = scan_with_horizon(trades, &series, &compiled, &pricing, None);
                    assert_outcomes_eq(&folded[i], &solo, &format!("{label}: fold vs scan"));

                    // (b) fold ≡ the engine fold itself, on the same single-token stream.
                    let replay_out = run_replay(
                        std::slice::from_ref(&rule),
                        std::slice::from_ref(&fingerprint()),
                        vec![replay_tok.clone()],
                        ReplayConfig { as_of, fill_model: fm },
                    );
                    let po = replay_out.iter().find(|o| o.mint == corpus_tok.mint);
                    let (r_fired, r_exit, r_entry, _r_exit_price, r_pnl_sol, _) =
                        replay_tuple(po, &pricing.cost);
                    assert_eq!(folded[i].fired, r_fired, "{label}: fired vs replay");
                    if r_fired {
                        assert_eq!(folded[i].exit, r_exit, "{label}: exit code vs replay");
                        let s_entry = folded[i].entry_price.unwrap_or(0.0);
                        assert!(
                            (s_entry - r_entry).abs() < 1e-9,
                            "{label}: entry price (fold {s_entry} vs replay {r_entry}) — a \
                             donated entry shows up here first"
                        );
                        assert_eq!(folded[i].pnl_sol, r_pnl_sol, "{label}: pnl vs replay");
                    }
                }

                // Non-vacuity: the veto must actually move the entry across the class —
                // one combo entering early, one late, one not at all. If this ever stops
                // holding, the fixture no longer exercises the poisoning shape and every
                // assert above is trivially satisfiable.
                let entries: std::collections::BTreeSet<_> =
                    folded.iter().map(|o| (o.fired, o.entry_time)).collect();
                assert_eq!(
                    entries.len(),
                    3,
                    "{fm:?}/{order}: the exit variants must resolve THREE different \
                     entries for this guard to mean anything, got {entries:?}"
                );
            }
        }
    }
}

#[test]
fn held_frozen_tail_matches_across_exit_paths() {
    // The `held` clock is position-scoped, so it takes the fast HeldBound index path
    // (not the scalar walk `time` forces). Its frozen-tail Open branch must resolve
    // byte-identically across scalar / index / simd. `held > 500` crosses ~500 s after
    // entry — past every `corpus()` token's own ≈370 s cut, so the crossing lands in the
    // frozen tail (not a sparse mid-history gap), and an explicit far horizon fires it.
    use super::exit_index::ExitIndex;
    use super::strategy::{
        resolve_entry, resolve_exit, resolve_exit_indexed, resolve_exit_simd, wants_exit_index,
        BoundCombo, EntryResolution,
    };

    let as_of = at(100_000.0);
    let tail_horizon = Some(at(2000.0)); // ≫ entry + 500, so the tail crossing fires
    let params = exit_metric(MetricGroupId::Position, MetricId::Held, Operator::Gt, 500.0);

    let mut saw_frozen_close = false;
    for fm in FILL_MODELS {
        let pricing = pricing_for(fm);
        let compiled = CompiledRule::compile(&loaded(params.clone()));
        let cols = columns_for(&compiled);
        let grid = sparse_grid_for(&compiled);
        let bound = BoundCombo::new(&cols, compiled.clone());
        assert!(bound.fast_exit, "a lone held bound must classify (fast path)");

        for (corpus_tok, _) in corpus().iter().chain(gappy_corpus().iter()) {
            let series = build_series_with_flow(corpus_tok, cols.clone(), &grid, as_of, None);
            let trades = &corpus_tok.trades;
            let entry = resolve_entry(trades, &series, &bound, &pricing);
            let mut idx = ExitIndex::default();
            match &entry {
                EntryResolution::Entered { fill_row, .. } if wants_exit_index(&bound, &entry) => {
                    idx.rebuild(&series, *fill_row);
                }
                _ => idx.clear(),
            }
            let scalar = resolve_exit(trades, &series, &bound, &entry, &pricing, tail_horizon);
            let indexed =
                resolve_exit_indexed(trades, &series, &bound, &entry, &pricing, &idx, tail_horizon);
            let simd =
                resolve_exit_simd(trades, &series, &bound, &entry, &pricing, &idx, tail_horizon);
            assert_outcomes_eq(&indexed, &scalar, &format!("{fm:?}/{} held index", corpus_tok.mint));
            assert_outcomes_eq(&simd, &scalar, &format!("{fm:?}/{} held simd", corpus_tok.mint));

            // Non-vacuity: the horizon-off resolve leaves a healthy token Open where the
            // horizon-on resolve closes it via the frozen-tail held crossing.
            let off = resolve_exit(trades, &series, &bound, &entry, &pricing, None);
            if scalar.exit == ExitCode::Metrics && off.exit == ExitCode::Open {
                saw_frozen_close = true;
            }
        }
    }
    assert!(
        saw_frozen_close,
        "no token exercised the held frozen-tail close — the fixture proved nothing"
    );
}

/// **Exclusivity is NOT enforced by the sweep** (documented divergence, alongside D2's
/// stripped concurrency caps — `docs/plans/sweep/sim-parity.md`). The scan judges each
/// combo/token independently in a parallel fan-out with no shared cross-combo state,
/// while `exclusive` needs cross-*rule* state at the same instant. This test keeps the
/// gap visible and re-verifiable rather than assumed away: the two exclusive rules both
/// fire in the sweep, and only one of them does through `run_replay`.
#[test]
fn sweep_ignores_exclusivity_but_the_engine_enforces_it() {
    use hunter_engine::event::{Effect, Event, Mint};
    use hunter_engine::reduce::reduce;
    use hunter_engine::EngineState;

    let as_of = at(1000.0);
    // One token, enter-on-arm, healthy — nothing but exclusivity can stop either rule.
    let (corpus_tok, replay_tok) = token(
        "excl",
        vec![
            ct(0.0, true, 1.0, 1.0, 100.0),
            ct(1.0, true, 0.5, 1.05, 100.0),
            ct(10.0, true, 0.5, 1.1, 100.0),
        ],
    );
    let fp = fingerprint();
    let pricing = pricing();

    let excl_rule = |id: u128, tp: f64| LoadedRule {
        id: RuleId(uuid::Uuid::from_u128(id)),
        params: RuleParams { take_profit: Some(tp), exclusive: true, ..RuleParams::default() },
        ..loaded(RuleParams::default())
    };
    let (a, b) = (excl_rule(0xA, 500.0), excl_rule(0xB, 900.0));

    // Sweep: each combo is scanned on its own → BOTH enter, un-deconflicted.
    for rule in [&a, &b] {
        let compiled = CompiledRule::compile(rule);
        let series = build_series_with_flow(
            &corpus_tok,
            columns_for(&compiled),
            &sparse_grid_for(&compiled),
            as_of,
            None,
        );
        let outcome = scan(&corpus_tok.trades, &series, &compiled, &pricing);
        assert!(
            outcome.fired,
            "sweep must still fire every exclusive combo independently ({:?})",
            rule.id
        );
    }

    // Engine: one shared fold over both rules → the lower-id rule claims the token and
    // the other stands down. Driven through `reduce` rather than `run_replay` because
    // the lab replay driver keys in-flight fills by *mint*, so it cannot carry two
    // concurrent positions on one token regardless of what the engine decides.
    let buys_under = |exclusive: bool| -> Vec<RuleId> {
        let rules: Vec<LoadedRule> = [&a, &b]
            .into_iter()
            .map(|r| LoadedRule {
                params: RuleParams { exclusive, ..r.params.clone() },
                ..r.clone()
            })
            .collect();
        let mut state = EngineState::new();
        reduce(
            &mut state,
            Event::RulesReloaded { rules: rules.into(), fps: vec![fp.clone()].into() },
        );
        let fx = reduce(
            &mut state,
            Event::TokenCreated {
                mint: Mint::from(replay_tok.mint.as_str()),
                fp: Box::new(replay_tok.tf.clone()),
                at: replay_tok.created_at,
                creator_wallet_hash: None,
            },
        );
        fx.iter()
            .filter_map(|e| match e {
                Effect::SubmitBuy { rule, .. } => Some(*rule),
                _ => None,
            })
            .collect()
    };
    assert_eq!(buys_under(true), vec![a.id], "reduce enforces exclusivity; the sweep does not");
    // Non-vacuity: drop the flag and the same fold enters both — the single entry above
    // is exclusivity, not a cap or a fingerprint quirk.
    assert_eq!(
        buys_under(false),
        vec![a.id, b.id],
        "without the flag both rules stack on the token"
    );
}
