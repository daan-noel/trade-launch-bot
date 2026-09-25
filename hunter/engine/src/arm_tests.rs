//! Tests for [`super::CompiledRule`]: buffers, clock horizons, monotonic kills, the
//! pre-entry veto, entry verdicts and the one-step stage walk.

use super::*;
use crate::event::RuleId;
use crate::metrics::{Side, Span, TradeLite, WindowSpec};
use crate::rule_params::RuleParams;
use chrono::{Duration, TimeZone, Utc};
use serde_json::{json, Value};
use uuid::Uuid;

fn rule(params: Value) -> LoadedRule {
    LoadedRule {
        id: RuleId(Uuid::from_u128(1)),
        fingerprint_id: FingerprintId(Uuid::from_u128(2)),
        trade_mode: TradeMode::Paper,
        buy_amount_lamports: 1_000_000_000,
        max_concurrent_tokens: 1,
        max_total_tokens: 0,
        params: RuleParams::parse(&params).unwrap(),
        entry_enabled: true,
    }
}

fn compile(params: Value) -> CompiledRule {
    CompiledRule::compile(&rule(params))
}

fn c(metric: &str, op: &str, v: f64) -> Value {
    json!({ "metric": metric, "is": [{ "operator": op, "value": v }] })
}

fn cw(metric: &str, span: &str, op: &str, v: f64) -> Value {
    json!({ "metric": metric, "span": span, "is": [{ "operator": op, "value": v }] })
}

fn t0() -> Ts {
    Utc.timestamp_opt(1_700_000_000, 0).unwrap()
}

fn at(s: f64) -> Ts {
    t0() + Duration::milliseconds((s * 1000.0) as i64)
}

fn print(price: f64, reserve: f64, secs: f64) -> TradeLite {
    TradeLite { side: Side::Buy, sol: 1.0, price, reserve_sol: reserve, at: at(secs), ..Default::default() }
}

fn held(entry: f64, peak: f64, trough: f64, stage: u8, stage_since: f64) -> EnteredCtx {
    EnteredCtx {
        position: crate::event::PositionId(9),
        entry_price: entry,
        entered_at: t0(),
        peak_price: peak,
        trough_price: trough,
        stage,
        stage_since: at(stage_since),
        sold_bps: 0,
        entry_priced_reserve: f64::NAN,
    }
}

// ── Buffers and horizons ─────────────────────────────────────────────────────

#[test]
fn each_condition_registers_on_its_own_buffer_alone() {
    let r = compile(json!({ "enter": { "filters": [
        cw("m_flow.gross_sol", "10s", ">=", 1.0),
        cw("m_crowd.unique_wallets", "20s", ">=", 3.0),
        cw("m_price.trail_pct", "30s", ">=", 5.0),
        { "metric": "m_flow.buy_sol", "tag": "volume", "span": "40s", "is": [{ "operator": ">=", "value": 1.0 }] }
    ] } }));
    let b = &r.buffers;
    assert_eq!(b.flow_windows.as_slice(), &[WindowSpec::secs(10.0)]);
    assert_eq!(b.crowd_windows.as_slice(), &[WindowSpec::secs(20.0)]);
    assert_eq!(b.price_windows.as_slice(), &[WindowSpec::secs(30.0)]);
    assert_eq!(b.tags.len(), 1);
    assert!(b.tags[0].trade && !b.tags[0].template);
    assert_eq!(b.tags[0].windows.as_slice(), &[WindowSpec::secs(40.0)]);
    assert!(!b.needs_slot);
}

#[test]
fn a_sliced_span_registers_both_axes_and_widens_the_horizon() {
    let r = compile(json!({ "enter": { "filters": [
        { "metric": "m_flow.slice_trade_share_pct", "span": "60s", "slice": "3s", "is": [{ "operator": ">=", "value": 7.69 }] }
    ] } }));
    assert_eq!(r.buffers.flow_windows.len(), 2);
    assert!(r.clock_horizons.max_window_secs >= 60.0);
}

#[test]
fn slot_and_wave_reads_open_the_slot_state_and_the_template_tag() {
    let r = compile(json!({ "enter": {
        "event": [{ "metric": "m_wave.buy_count", "tag": "working", "is": [{ "operator": "=", "value": 2.0 }] }],
        "final_filters": [{ "metric": "m_crowd.unique_ix_templates", "tag": "working", "is": [{ "operator": ">=", "value": 4.0 }] }]
    } }));
    assert!(r.buffers.slot_state, "unique_ix_templates reads the slot state");
    assert!(r.buffers.needs_slot, "the wave needs the slot column");
    assert_eq!(r.buffers.tags.len(), 1);
    assert!(r.buffers.tags[0].template && !r.buffers.tags[0].trade);
}

#[test]
fn deadlines_and_clock_conditions_widen_their_horizons() {
    let r = compile(json!({
        "always": [{ "if": [c("m_position.held_sec", ">=", 1000.0)], "sell": "clock" }],
        "stages": [
            { "name": "early", "ends": { "age_sec": 20.0 } },
            { "name": "ride", "ends": { "stage_sec": 30.0 }, "then": "early" }
        ]
    }));
    let h = r.clock_horizons;
    assert!(h.held_secs >= 1000.0);
    assert!(h.time_secs >= 20.0);
    assert!(h.stage_secs >= 30.0);
}

// ── Monotonic kills and blockers ─────────────────────────────────────────────

#[test]
fn an_age_upper_bound_is_a_mono_kill_and_names_what_was_short() {
    let r = compile(json!({ "enter": { "filters": [
        { "metric": "m_state.age_sec", "is": [{ "operator": ">", "value": 10.0 }, { "operator": "<", "value": 50.0 }] },
        cw("m_flow.gross_sol", "60s", ">=", 40.0)
    ] } }));
    assert_eq!(r.mono_kills.len(), 1);
    let mut track = TokenTrack::new(t0());
    for w in &r.buffers.flow_windows {
        track.ensure_window(*w);
    }
    track.on_trade(TradeLite { sol: 20.0, ..print(1.0, 20.0, 0.0) });
    assert!(r.entry_unsatisfiable(&track, at(30.0)).is_none());
    let killed = r.entry_unsatisfiable(&track, at(50.0)).expect("age < 50 crossed");
    assert_eq!(killed.r.metric, Metric::AgeSec);
    assert_eq!(killed.threshold, 50.0);
    let b = r.entry_blockers(&track, at(50.0), killed);
    assert_eq!(b.unmet.len(), 1, "{:?}", b.unmet);
    assert_eq!(b.unmet[0].r.metric, Metric::GrossSol);
    assert_eq!(b.unmet[0].r.span, Span::secs(60.0));
    assert_eq!(b.unmet[0].value, 20.0);
}

#[test]
fn an_or_with_an_open_arm_never_kills_and_a_window_is_never_monotonic() {
    let open = compile(json!({ "enter": { "filters": [
        { "metric": "m_state.age_sec", "is": [[{ "operator": "<", "value": 30.0 }], [{ "operator": ">=", "value": 70.0 }]] }
    ] } }));
    assert!(!open.mono_kills[0].permanently_false(100.0));
    let windowed = compile(json!({ "enter": { "filters": [cw("m_flow.buy_sol", "10s", "<", 5.0)] } }));
    assert!(windowed.mono_kills.is_empty(), "a window decays, so its bound can come back");
    let life = compile(json!({ "enter": { "filters": [c("m_flow.buy_sol", "<", 5.0)] } }));
    assert_eq!(life.mono_kills.len(), 1, "a lifetime sum only grows");
}

// ── Entry ────────────────────────────────────────────────────────────────────

/// The veto: no buy while an `always` sell line or a first-stage sell line already
/// holds on the coin. A line on our position never vetoes.
#[test]
fn no_buy_while_a_sell_line_already_holds() {
    let r = compile(json!({
        "enter": { "filters": [c("m_state.liquidity_sol", ">", 50.0)] },
        "always": [
            { "if": [c("m_state.liquidity_sol", ">", 40.0)], "sell": "deep" },
            { "if": [c("m_position.pnl_pct", "<=", -30.0)], "sell": "stop" }
        ]
    }));
    let mut deep = TokenTrack::new(t0());
    deep.on_trade(print(1.0, 60.0, 1.0));
    assert_eq!(r.sell_line_holding_before_entry(&deep, at(1.0)), Some(ExitReason::Line("deep")));
    assert!(!r.can_enter(&deep, at(1.0)));

    let staged = compile(json!({
        "enter": { "filters": [c("m_state.liquidity_sol", ">", 50.0)] },
        "stages": [
            { "name": "a", "on": [{ "if": [c("m_state.liquidity_sol", ">", 40.0)], "sell": "early" }] },
            { "name": "b", "on": [{ "if": [c("m_state.liquidity_sol", ">", 45.0)], "sell": "late" }] }
        ]
    }));
    assert_eq!(staged.sell_line_holding_before_entry(&deep, at(1.0)), Some(ExitReason::Line("early")));
    let only_later = compile(json!({
        "enter": { "filters": [c("m_state.liquidity_sol", ">", 50.0)] },
        "stages": [
            { "name": "a", "on": [{ "if": [c("m_state.liquidity_sol", ">", 90.0)], "go": "b" }] },
            { "name": "b", "on": [{ "if": [c("m_state.liquidity_sol", ">", 40.0)], "sell": "late" }] }
        ]
    }));
    assert!(only_later.can_enter(&deep, at(1.0)), "a later stage cannot act right after the buy");
}

#[test]
fn a_token_lock_decides_on_the_first_event_print_only() {
    let r = compile(json!({ "enter": {
        "event": [c("m_state.age_sec", ">=", 1.0)],
        "filters": [c("m_state.liquidity_sol", ">=", 14.0)],
        "lock": "token"
    } }));
    let mut shallow = TokenTrack::new(t0());
    shallow.on_trade(print(1.0, 10.0, 2.0));
    assert_eq!(r.try_enter(&shallow, at(2.0), None, false), EntryVerdict::No, "a tick never decides");
    assert_eq!(r.try_enter(&shallow, at(2.0), None, true), EntryVerdict::Exhaust, "the one chance fails");
    let mut deep = TokenTrack::new(t0());
    deep.on_trade(print(1.0, 20.0, 2.0));
    assert_eq!(r.try_enter(&deep, at(2.0), None, true), EntryVerdict::Enter);
}

#[test]
fn a_final_filter_failure_ends_the_coin_and_a_filter_failure_does_not() {
    let r = compile(json!({ "enter": {
        "filters": [c("m_state.liquidity_sol", ">=", 14.0)],
        "final_filters": [c("m_state.age_sec", "<=", 5.0)]
    } }));
    let mut late = TokenTrack::new(t0());
    late.on_trade(print(1.0, 20.0, 9.0));
    assert_eq!(r.try_enter(&late, at(9.0), None, true), EntryVerdict::Exhaust);
    let mut shallow = TokenTrack::new(t0());
    shallow.on_trade(print(1.0, 10.0, 2.0));
    assert_eq!(r.try_enter(&shallow, at(2.0), None, true), EntryVerdict::No);
}

// ── Held side ────────────────────────────────────────────────────────────────

/// Stop loss first, then take profit, then the authored always lines.
#[test]
fn the_shortcuts_come_first_in_their_own_order() {
    let r = compile(json!({
        "take_profit": 100.0,
        "stop_loss": 30.0,
        "always": [{ "if": [c("m_position.retrace_pct", ">=", 3.0)], "sell": "trail" }]
    }));
    assert_eq!(r.always.len(), 3);
    assert_eq!(r.always[0].sell.unwrap().reason, ExitReason::StopLoss);
    assert_eq!(r.always[1].sell.unwrap().reason, ExitReason::TakeProfit);
    assert_eq!(r.always[2].sell.unwrap().reason, ExitReason::Line("trail"));
}

/// The 7ix plan, one step per evaluation: a signal before age 20 sells; at 20 the rule
/// moves to `late`; in `late` the signal moves to `ride` (and the ride's own line is read
/// from the next evaluation, never on the move); a ride deadline moves to `hold`.
#[test]
fn a_stage_plan_walks_one_step_per_evaluation() {
    let r = compile(json!({
        "signals": { "cashout": [[c("m_state.liquidity_sol", ">=", 30.0)]] },
        "always": [{ "if": [c("m_state.liquidity_sol", ">=", 80.0)], "sell": "top" }],
        "stages": [
            { "name": "early", "ends": { "age_sec": 20.0 }, "on": [{ "if": [{ "signal": "cashout" }], "sell": "spike" }] },
            { "name": "late", "on": [{ "if": [{ "signal": "cashout" }], "go": "ride" }] },
            { "name": "ride", "ends": { "stage_sec": 30.0 }, "then": "hold",
              "on": [{ "if": [c("m_state.liquidity_sol", ">=", 30.0)], "sell": "burst" }] },
            { "name": "hold" }
        ]
    }));
    let mut hot = TokenTrack::new(t0());
    hot.on_trade(print(1.0, 35.0, 5.0));
    assert_eq!(
        r.held_step(&hot, &held(1.0, 1.0, 1.0, 0, 0.0), at(10.0)),
        HeldAction::Sell { reason: ExitReason::Line("spike"), bps: None, then_stage: None }
    );
    assert_eq!(r.held_step(&hot, &held(1.0, 1.0, 1.0, 0, 0.0), at(20.0)), HeldAction::Move { stage: 1 }, "the deadline");
    assert_eq!(r.held_step(&hot, &held(1.0, 1.0, 1.0, 1, 20.0), at(25.0)), HeldAction::Move { stage: 2 });
    assert_eq!(
        r.held_step(&hot, &held(1.0, 1.0, 1.0, 2, 25.0), at(25.2)),
        HeldAction::Sell { reason: ExitReason::Line("burst"), bps: None, then_stage: None },
        "the ride's line is read from the next evaluation"
    );
    assert_eq!(r.held_step(&hot, &held(1.0, 1.0, 1.0, 2, 25.0), at(55.0)), HeldAction::Move { stage: 3 });
    assert_eq!(r.held_step(&hot, &held(1.0, 1.0, 1.0, 3, 55.0), at(60.0)), HeldAction::None);
    let mut top = TokenTrack::new(t0());
    top.on_trade(print(1.0, 85.0, 5.0));
    assert_eq!(
        r.held_step(&top, &held(1.0, 1.0, 1.0, 3, 55.0), at(60.0)),
        HeldAction::Sell { reason: ExitReason::Line("top"), bps: None, then_stage: None },
        "always lines apply in every stage"
    );
}

/// A checkpoint: at the deadline the first `at_end` line that holds acts, and only
/// otherwise does the rule move on.
#[test]
fn a_checkpoint_acts_at_its_deadline() {
    let r = compile(json!({ "stages": [
        { "name": "watch", "ends": { "held_sec": 60.0 },
          "at_end": [{ "if": [c("m_position.pnl_pct", "<", 0.0)], "sell": "weak at 60 s" }] },
        { "name": "run" }
    ] }));
    let mut t = TokenTrack::new(t0());
    t.on_trade(print(0.9, 20.0, 1.0));
    assert_eq!(r.held_step(&t, &held(1.0, 1.0, 0.9, 0, 0.0), at(30.0)), HeldAction::None, "before the deadline");
    assert_eq!(
        r.held_step(&t, &held(1.0, 1.0, 0.9, 0, 0.0), at(60.0)),
        HeldAction::Sell { reason: ExitReason::Line("weak at 60 s"), bps: None, then_stage: None }
    );
    let mut up = TokenTrack::new(t0());
    up.on_trade(print(1.2, 20.0, 1.0));
    assert_eq!(r.held_step(&up, &held(1.0, 1.2, 1.0, 0, 0.0), at(60.0)), HeldAction::Move { stage: 1 });
}

/// A partial sell carries its percent and the stage it moves to when the fill lands.
#[test]
fn a_partial_sell_carries_its_share_and_next_stage() {
    let r = compile(json!({ "stages": [
        { "name": "first", "on": [{ "if": [c("m_position.pnl_pct", ">=", 50.0)], "sell": "half", "sell_pct": 50.0, "go": "rest" }] },
        { "name": "rest", "on": [{ "if": [c("m_position.retrace_pct", ">=", 20.0)], "sell": "trail" }] }
    ] }));
    let mut t = TokenTrack::new(t0());
    t.on_trade(print(1.6, 20.0, 1.0));
    assert_eq!(
        r.held_step(&t, &held(1.0, 1.6, 1.0, 0, 0.0), at(2.0)),
        HeldAction::Sell { reason: ExitReason::Line("half"), bps: Some(5000), then_stage: Some(1) }
    );
}

/// A line without a label is labelled from its first condition.
#[test]
fn an_unlabelled_line_names_itself_from_its_first_condition() {
    let r = compile(json!({ "always": [
        { "if": [{ "metric": "m_flow.buy_sol", "tag": "!volume", "span": "10s", "is": [{ "operator": ">=", "value": 2.0 }] }], "sell": true }
    ] }));
    assert_eq!(r.always[0].sell.unwrap().reason, ExitReason::Line("m_flow.buy_sol @!volume [10s] >= 2"));
}

/// Parked lines and conditions compile to nothing.
#[test]
fn off_items_are_not_compiled() {
    let r = compile(json!({
        "enter": { "filters": [c("m_state.age_sec", ">", 1.0), { "metric": "m_state.age_sec", "is": [{ "operator": "<", "value": 30.0 }], "off": true }] },
        "always": [
            { "if": [c("m_position.pnl_pct", ">=", 50.0)], "sell": "tp" },
            { "if": [c("m_position.pnl_pct", "<=", -20.0)], "sell": "sl", "off": true }
        ]
    }));
    assert_eq!(r.filters.len(), 1);
    assert_eq!(r.always.len(), 1);
    assert!(r.mono_kills.is_empty(), "the parked upper bound is not a kill");
}
