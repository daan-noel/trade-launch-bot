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
use hunter_engine::metrics::{MetricGroupId, MetricId, Ts};
use hunter_engine::rule_params::{GroupConditions, RuleParams, SideConditions};

use trading_core::config::constants::sol_to_lamports;
use trading_core::strategies::kernel::{round_trip_with_costs, CostModel, ExitCode};

use crate::sweep::corpus::CorpusToken;
use crate::sweep::projection::CorpusTrade;
use crate::strategies::replay::{run_replay, PositionOutcome, ReplayConfig, ReplayToken};

use super::strategy::{build_series, columns_for, scan, sparse_grid_for};

const BUY_SOL: f64 = 1.0;
const FP_ID: uuid::Uuid = uuid::Uuid::from_u128(0x1234);

fn base() -> Ts {
    Utc.timestamp_opt(1_700_000_000, 0).unwrap()
}

fn at(secs: f64) -> Ts {
    base() + Duration::milliseconds((secs * 1000.0) as i64)
}

/// One synthetic trade. Reserves are REAL-reserve values (what deadness reads).
fn ct(secs: f64, is_buy: bool, sol: f64, price: f64, reserve: f64) -> CorpusTrade {
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
    }
}

/// A token that matches the test fingerprint (via `cu_limit`) and carries `trades`.
fn token(mint: &str, trades: Vec<CorpusTrade>) -> (CorpusToken, ReplayToken) {
    let tf = TokenFingerprint { cu_limit: Some(200_000), ..Default::default() };
    let trades = Arc::new(trades);
    let corpus = CorpusToken {
        mint: mint.to_string(),
        symbol: "SYM".to_string(),
        created_at: base(),
        trades: trades.clone(),
        fp: tf.clone(),
    };
    let replay = ReplayToken {
        mint: mint.to_string(),
        symbol: "SYM".to_string(),
        created_at: base(),
        tf,
        trades,
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
    gc.metrics.insert(metric, vec![Condition { operator: op, value }]);
    let mut side = SideConditions::default();
    side.0.insert(group, gc);
    RuleParams { take_profit: None, stop_loss: None, entry: None, exit: Some(side) }
}

fn exit_code_of(reason: Option<ExitReason>) -> ExitCode {
    match reason {
        None => ExitCode::Open,
        Some(ExitReason::TakeProfit) => ExitCode::TakeProfit,
        Some(ExitReason::StopLoss) => ExitCode::StopLoss,
        Some(ExitReason::Metrics) => ExitCode::Metrics,
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
            let (pnl_sol, pnl_pct) = round_trip_with_costs(o.entry_price, exit_price, BUY_SOL, cost);
            (true, exit_code_of(o.exit_reason), o.entry_price, exit_price, pnl_sol as f32, pnl_pct as f32)
        }
    }
}

/// Run `rule` over `corpus_tokens` two ways — full engine replay and the scan —
/// and assert the per-token outcomes match.
fn assert_parity(label: &str, params: RuleParams, tokens: &[(CorpusToken, ReplayToken)], as_of: Ts) {
    let rule = loaded(params);
    let fp = fingerprint();
    let cost = CostModel::pumpfun_default();

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
            ReplayConfig { as_of },
        );
        let po = replay_out.iter().find(|o| o.mint == corpus_tok.mint);
        let (r_fired, r_exit, r_entry, r_exit_price, r_pnl_sol, r_pnl_pct) = replay_tuple(po, &cost);

        let series = build_series(corpus_tok, cols.clone(), &grid, as_of);
        let outcome = scan(&series, &compiled, BUY_SOL, &cost);

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
        // TP: price doubles → +100% ≥ TP 50%.
        token("tp", vec![ct(0.0, true, 1.0, 1.0, 100.0), ct(3.0, true, 0.5, 2.0, 100.0)]),
        // SL: price halves → −50% ≤ SL 30%.
        token("sl", vec![ct(0.0, true, 1.0, 1.0, 100.0), ct(3.0, false, 0.5, 0.5, 100.0)]),
        // Metrics: peak 2.0 then 0.8 → 60% off peak (trail > 50).
        token(
            "metrics",
            vec![ct(0.0, true, 1.0, 1.0, 100.0), ct(1.0, true, 0.5, 2.0, 100.0), ct(3.0, false, 0.5, 0.8, 100.0)],
        ),
        // Dead: healthy price but liquidity gone (5 < 30) then silent → dead in the tail.
        token("dead", vec![ct(0.0, true, 1.0, 1.0, 5.0)]),
        // Open: mild drift, liquidity healthy, never triggers anything.
        token("open", vec![ct(0.0, true, 1.0, 1.0, 100.0), ct(3.0, true, 0.5, 1.1, 100.0)]),
    ]
}

#[test]
fn scan_matches_replay_tp_sl_rule() {
    // TP 50% OR SL 30%. Enter on arm (no entry conditions).
    let params = RuleParams { take_profit: Some(50.0), stop_loss: Some(30.0), entry: None, exit: None };
    assert_parity("tp_sl", params, &corpus(), at(1000.0));
}

#[test]
fn scan_matches_replay_metrics_exit_rule() {
    // Exit when trail (% off peak) exceeds 50 — no TP/SL, so only the metric fires.
    let params = exit_metric(MetricGroupId::PricePath, MetricId::Trail, Operator::Gt, 50.0);
    assert_parity("trail_exit", params, &corpus(), at(1000.0));
}

#[test]
fn scan_matches_replay_entry_gated_rule() {
    // Entry gated on time > 2s (so entry defers past the first trade), TP 20%.
    let mut gc = GroupConditions::default();
    gc.metrics.insert(MetricId::Time, vec![Condition { operator: Operator::Gt, value: 2.0 }]);
    let mut entry = SideConditions::default();
    entry.0.insert(MetricGroupId::Snapshot, gc);
    let params =
        RuleParams { take_profit: Some(20.0), stop_loss: None, entry: Some(entry), exit: None };
    assert_parity("entry_gate", params, &corpus(), at(1000.0));
}

#[test]
fn scan_matches_replay_pure_dead_open_rule() {
    // No TP/SL/exit metrics: every fired token rides to Dead or Open.
    let params = RuleParams { take_profit: None, stop_loss: None, entry: None, exit: None };
    assert_parity("dead_open", params, &corpus(), at(1000.0));
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
        // Revive: 2 h silence (flows decay to 0, time/stall grow) then a live trade.
        token("revive", vec![ct(0.0, true, 2.0, 1.0, 100.0), ct(7200.0, true, 2.0, 1.2, 100.0)]),
        // Dead mid-gap: liquidity gone, silent past the 300 s quiet window inside a
        // long gap, then a late trade (the verdict is booked before it lands).
        token("dead_midgap", vec![ct(0.0, true, 1.0, 1.0, 5.0), ct(7200.0, true, 1.0, 1.0, 5.0)]),
        // Healthy but idle: one trade, reserves fine, rides to Open in the tail.
        token("idle", vec![ct(0.0, true, 2.0, 1.0, 100.0)]),
    ]
}

#[test]
fn scan_matches_replay_gappy_dead_open() {
    // Enter-on-arm, no exits: dead_midgap → Dead mid-gap, revive/idle → Open.
    let params = RuleParams { take_profit: None, stop_loss: None, entry: None, exit: None };
    assert_parity("gappy_dead_open", params, &gappy_corpus(), at(100_000.0));
}

#[test]
fn scan_matches_replay_time_gate_across_gap() {
    // Entry gated on time > 3600 s — qualifies mid-gap, so the fill lands on a tick
    // deep inside the quiet span the sparse grid must still emit.
    let mut gc = GroupConditions::default();
    gc.metrics.insert(MetricId::Time, vec![Condition { operator: Operator::Gt, value: 3600.0 }]);
    let mut entry = SideConditions::default();
    entry.0.insert(MetricGroupId::Snapshot, gc);
    let params =
        RuleParams { take_profit: Some(5.0), stop_loss: None, entry: Some(entry), exit: None };
    assert_parity("time_gate_gap", params, &gappy_corpus(), at(100_000.0));
}

#[test]
fn scan_matches_replay_stall_eq_exit_across_gap() {
    // Exit when stall ≈ 1800 s (`=` with the metric's tolerance) — a tolerance-edged
    // threshold reached only inside the gap. Exercises both region boundaries.
    let params = exit_metric(MetricGroupId::PricePath, MetricId::Stall, Operator::Eq, 1800.0);
    assert_parity("stall_eq_gap", params, &gappy_corpus(), at(100_000.0));
}

#[test]
fn scan_matches_replay_window_flow_across_gap() {
    // Exit when the 60 s gross-flow window drops to 0 — flows decay to 0 partway
    // through the gap, so the decay-region ticks must be present and exact.
    let mut gc = GroupConditions::default();
    gc.strict.insert("window_size_sec".to_string(), 60.0);
    gc.metrics.insert(MetricId::GrossFlow, vec![Condition { operator: Operator::Lte, value: 0.0 }]);
    let mut exit = SideConditions::default();
    exit.0.insert(MetricGroupId::TimeWindow, gc);
    let params = RuleParams { take_profit: None, stop_loss: None, entry: None, exit: Some(exit) };
    assert_parity("flow_decay_gap", params, &gappy_corpus(), at(100_000.0));
}
