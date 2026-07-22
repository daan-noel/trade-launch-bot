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
use hunter_engine::rule_params::{GroupConditions, RuleParams, SideConditions};

use trading_core::config::constants::sol_to_lamports;
use trading_core::strategies::kernel::{round_trip_with_costs, CostModel, ExitCode};

use crate::sweep::corpus::CorpusToken;
use crate::sweep::projection::CorpusTrade;
use crate::sweep::strategy::TokenOutcome;
use crate::strategies::replay::{run_replay, PositionOutcome, ReplayConfig, ReplayToken};

use super::strategy::{build_series_with_flow, columns_for, scan, sparse_grid_for};

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
    RuleParams { take_profit: None, stop_loss: None, entry: None, exit: Some(side) }
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
            let (pnl_sol, pnl_pct) = round_trip_with_costs(o.entry_price, exit_price, BUY_SOL, cost);
            (true, exit_code_of(o.exit_reason), o.entry_price, exit_price, pnl_sol as f32, pnl_pct as f32)
        }
    }
}

/// Run `rule` over `corpus_tokens` two ways — full engine replay and the scan —
/// and assert the per-token outcomes match.
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
            ReplayConfig { as_of, ..Default::default() },
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
        let outcome = scan(&corpus_tok.trades, &series, &compiled, BUY_SOL, &cost);

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
    gc.metrics.insert(MetricId::Time, vec![vec![Condition { operator: Operator::Gt, value: 2.0 }]]);
    let mut entry = SideConditions::default();
    entry.0.insert(MetricGroupId::Snapshot, vec![gc]);
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
    let params = RuleParams { take_profit: None, stop_loss: None, entry: None, exit: None };
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

    let cost = CostModel::pumpfun_default();
    let as_of = at(100_000.0);
    // Two rule shapes (metric-exit and TP/SL) over both corpora, so the entry, exit
    // and mono-kill column sets are all exercised on tokens with differing series
    // lengths and gap structure.
    let rules = [
        exit_metric(MetricGroupId::PricePath, MetricId::Stall, Operator::Gte, 1800.0),
        RuleParams {
            take_profit: Some(50.0),
            stop_loss: Some(30.0),
            entry: None,
            exit: None,
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

            let entry = resolve_entry(&corpus_tok.trades, &series, &shared);
            let shared_out = resolve_exit(&corpus_tok.trades, &series, &shared, &entry, BUY_SOL, &cost);
            // `scan` binds against this token's own series — the reference.
            let per_token_out = scan(&corpus_tok.trades, &series, &compiled, BUY_SOL, &cost);

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
    exit.0.insert(MetricGroupId::TimeWindow, vec![gc]);
    let params = RuleParams { take_profit: None, stop_loss: None, entry: None, exit: Some(exit) };
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
    exit.0.insert(MetricGroupId::FlowWindow, vec![exit_gc]);

    let params = RuleParams {
        take_profit: None,
        stop_loss: None,
        entry: Some(entry),
        exit: Some(exit),
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
    use super::strategy::{resolve_entry, resolve_exit, resolve_exit_simd, BoundCombo};

    let cost = CostModel::pumpfun_default();
    let as_of = at(100_000.0);

    // Entry gated on time > 2 s so the fill defers past the first print — exercises a
    // non-zero `fill_row` (the SIMD scan starts at `fill_row + 1`).
    let entry_gate = {
        let mut gc = GroupConditions::default();
        gc.metrics.insert(MetricId::Time, vec![vec![Condition { operator: Operator::Gt, value: 2.0 }]]);
        let mut entry = SideConditions::default();
        entry.0.insert(MetricGroupId::Snapshot, vec![gc]);
        RuleParams { take_profit: Some(20.0), stop_loss: Some(60.0), entry: Some(entry), exit: None }
    };

    let rules = vec![
        // TP + SL together.
        RuleParams { take_profit: Some(50.0), stop_loss: Some(30.0), entry: None, exit: None },
        // TP only / SL only (one threshold absent → the `have_sl`/`have_tp` branches).
        RuleParams { take_profit: Some(20.0), stop_loss: None, entry: None, exit: None },
        RuleParams { take_profit: None, stop_loss: Some(40.0), entry: None, exit: None },
        // Neither → only Dead / Open can fire.
        RuleParams { take_profit: None, stop_loss: None, entry: None, exit: None },
        // Deferred entry (non-zero fill row).
        entry_gate,
        // Metrics exit → SIMD delegates to scalar; still must match.
        exit_metric(MetricGroupId::PricePath, MetricId::Trail, Operator::Gt, 50.0),
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
            let entry = resolve_entry(trades, &series, &bound);
            let scalar = resolve_exit(trades, &series, &bound, &entry, BUY_SOL, &cost);
            let simd = resolve_exit_simd(trades, &series, &bound, &entry, BUY_SOL, &cost);
            assert_eq!(
                outcome_tuple(&simd),
                outcome_tuple(&scalar),
                "rule[{i}] token '{}': AVX-512 exit scan diverged from scalar",
                corpus_tok.mint,
            );
        }
    }
}

#[test]
fn index_exit_scan_matches_scalar_across_paths() {
    use super::exit_index::ExitIndex;
    use super::strategy::{resolve_entry, resolve_exit, resolve_exit_indexed, BoundCombo};

    let cost = CostModel::pumpfun_default();
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
        }
    };

    let rules = vec![
        RuleParams { take_profit: Some(50.0), stop_loss: Some(30.0), entry: None, exit: None },
        RuleParams { take_profit: Some(20.0), stop_loss: None, entry: None, exit: None },
        RuleParams { take_profit: None, stop_loss: Some(40.0), entry: None, exit: None },
        RuleParams { take_profit: None, stop_loss: None, entry: None, exit: None },
        entry_gate,
        // Metrics → index falls back to scalar; still must match.
        exit_metric(MetricGroupId::PricePath, MetricId::Trail, Operator::Gt, 50.0),
    ];

    for (i, params) in rules.into_iter().enumerate() {
        let compiled = CompiledRule::compile(&loaded(params));
        let cols = columns_for(&compiled);
        let grid = sparse_grid_for(&compiled);
        let bound = BoundCombo::new(&cols, compiled.clone());

        for (corpus_tok, _) in corpus().iter().chain(gappy_corpus().iter()) {
            let series = build_series_with_flow(corpus_tok, cols.clone(), &grid, as_of, None);
            let trades = &corpus_tok.trades;
            let entry = resolve_entry(trades, &series, &bound);
            let mut idx = ExitIndex::default();
            match &entry {
                super::strategy::EntryResolution::Entered { fill_row, .. }
                    if !bound.rule.has_exit_metrics() =>
                {
                    idx.rebuild(&series, *fill_row);
                }
                _ => idx.clear(),
            }
            let scalar = resolve_exit(trades, &series, &bound, &entry, BUY_SOL, &cost);
            let indexed = resolve_exit_indexed(trades, &series, &bound, &entry, BUY_SOL, &cost, &idx);
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

    let cost = CostModel::pumpfun_default();
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
            };
            let compiled = CompiledRule::compile(&loaded(params));
            let bound = BoundCombo::new(series.columns(), compiled);
            let entry = resolve_entry(&trades, &series, &bound);
            let mut idx = ExitIndex::default();
            if let super::strategy::EntryResolution::Entered { fill_row, .. } = &entry {
                idx.rebuild(&series, *fill_row);
            }
            let scalar = resolve_exit(&trades, &series, &bound, &entry, BUY_SOL, &cost);
            let indexed = resolve_exit_indexed(&trades, &series, &bound, &entry, BUY_SOL, &cost, &idx);
            assert_outcomes_eq(
                &indexed,
                &scalar,
                &format!("rand case={case} grid[{ti}]: exit-index diverged from scalar"),
            );
        }
    }
}
