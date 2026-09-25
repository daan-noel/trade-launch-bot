//! Tests for the rule readout: it agrees with the fold, reads entry with no position,
//! places every condition, and its replay and series routes agree with each other.

use super::*;
use crate::arm::HeldAction;
use crate::event::{LoadedRule, RuleId, TradeMode};
use crate::metrics::{Metric, Side};
use crate::rule_params::RuleParams;
use chrono::{Duration, TimeZone, Utc};
use serde_json::{json, Value};
use uuid::Uuid;

fn ts(secs: i64) -> Ts {
    Utc.timestamp_opt(1_700_000_000, 0).unwrap() + Duration::seconds(secs)
}

fn rule(params: Value) -> CompiledRule {
    CompiledRule::compile(&LoadedRule {
        id: RuleId(Uuid::nil()),
        fingerprint_id: FingerprintId(Uuid::nil()),
        trade_mode: TradeMode::Paper,
        buy_amount_lamports: 100_000_000,
        max_concurrent_tokens: 1,
        max_total_tokens: 0,
        entry_enabled: true,
        params: RuleParams::parse(&params).expect("params parse"),
    })
}

fn trade(price: f64, secs: i64) -> TradeLite {
    TradeLite { side: Side::Buy, sol: 1.0, price, reserve_sol: 30.0, at: ts(secs), ..Default::default() }
}

fn track_at(price: f64, secs: i64) -> TokenTrack {
    let mut t = TokenTrack::new(ts(0));
    t.on_trade(trade(price, secs));
    t
}

fn c(metric: &str, op: &str, v: f64) -> Value {
    json!({ "metric": metric, "is": [{ "operator": op, "value": v }] })
}

fn entered(peak: f64) -> crate::arm::EnteredCtx {
    let mut e = crate::arm::EnteredCtx::at_fill(crate::event::PositionId(1), 1.0, ts(0), f64::NAN);
    e.peak_price = peak;
    e
}

/// A condition's `ok` agrees with the fold's own step, where it sells and where not.
#[test]
fn a_sell_line_reads_what_the_fold_decides() {
    let r = rule(json!({ "always": [{ "if": [c("m_position.retrace_pct", ">=", 20.0)], "sell": "trail" }] }));
    let held = entered(2.0);
    let calm = track_at(1.8, 10);
    let parts = read_rule(&r, &calm, Some(&held.position_ctx()), ts(10));
    assert!(!parts.conditions[0].ok);
    assert!(!parts.lines[0].holds);
    assert_eq!(r.held_step(&calm, &held, ts(10)), HeldAction::None);

    let dumped = track_at(1.5, 20);
    let parts = read_rule(&r, &dumped, Some(&held.position_ctx()), ts(20));
    assert!(parts.conditions[0].ok);
    assert!((parts.conditions[0].value - 25.0).abs() < 1e-9);
    assert!(parts.lines[0].holds);
    assert_eq!(parts.lines[0].sells, Some(ExitReason::Line("trail")));
    assert!(matches!(r.held_step(&dumped, &held, ts(20)), HeldAction::Sell { .. }));
}

/// Entry conditions read with no position even when one is held, as the decision does.
#[test]
fn entry_reads_with_no_position() {
    let r = rule(json!({
        "enter": { "filters": [c("m_state.liquidity_sol", ">", 10.0)] },
        "always": [{ "if": [c("m_position.pnl_pct", ">=", 50.0)], "sell": "tp" }]
    }));
    let track = track_at(1.6, 5);
    let parts = read_rule(&r, &track, Some(&entered(1.6).position_ctx()), ts(5));
    let filter = parts.conditions.iter().find(|x| x.part == ReadPart::Filter).unwrap();
    assert!(filter.ok);
    let tp = parts.conditions.iter().find(|x| x.r.metric == Metric::PnlPct).unwrap();
    assert!(tp.ok, "the sell line reads the held position");
    let before = read_rule(&r, &track, None, ts(5));
    let tp = before.conditions.iter().find(|x| x.r.metric == Metric::PnlPct).unwrap();
    assert!(tp.value.is_nan(), "no position, no pnl");
}

/// Every condition is placed: entry parts, signal groups, always and stage lines.
#[test]
fn every_condition_is_placed_where_it_is_written() {
    let r = rule(json!({
        "enter": { "event": [c("m_state.age_sec", ">=", 1.0)], "filters": [c("m_state.liquidity_sol", ">=", 14.0)] },
        "signals": { "hot": [[c("m_state.liquidity_sol", ">=", 30.0)]] },
        "always": [{ "if": [c("m_state.liquidity_sol", ">=", 80.0)], "sell": "top" }],
        "stages": [
            { "name": "a", "ends": { "age_sec": 20.0 }, "on": [{ "if": [{ "signal": "hot" }], "sell": "spike" }],
              "at_end": [{ "if": [c("m_state.liquidity_sol", "<", 5.0)], "sell": "dead" }] },
            { "name": "b" }
        ]
    }));
    let parts = read_rule(&r, &track_at(1.0, 5), None, ts(5));
    let places: Vec<ReadPart> = parts.conditions.iter().map(|x| x.part).collect();
    assert_eq!(
        places,
        vec![
            ReadPart::Event,
            ReadPart::Filter,
            ReadPart::Signal { signal: 0, name: "hot", group: 0 },
            ReadPart::Always { line: 0 },
            ReadPart::Stage { stage: 0, name: "a", at_end: true, line: 0 },
        ]
    );
    assert_eq!(parts.signals, vec![SignalRead { name: "hot", holds: true }]);
    let spike = parts.lines.iter().find(|l| l.part == ReadPart::Stage { stage: 0, name: "a", at_end: false, line: 0 }).unwrap();
    assert!(spike.holds, "a signal line holds when its signal does");
}

/// The replay folds stored trades into the same reading a live track gives.
#[test]
fn a_replay_reads_what_the_live_track_reads() {
    let r = rule(json!({
        "enter": { "filters": [{ "metric": "m_flow.buy_sol", "span": "10s", "is": [{ "operator": ">=", "value": 2.0 }] }] },
        "always": [{ "if": [c("m_position.retrace_pct", ">=", 20.0)], "sell": "trail" }]
    }));
    let trades = vec![trade(1.0, 1), trade(2.0, 3), trade(1.5, 6)];
    let ctx = ReplayCtx { created_at: ts(0), entry: Some((ts(1), 1.0)), stage: None, tags: None };
    let replay = replay_readout(&r, trades.clone(), &ctx, ts(8));

    let mut live = TokenTrack::new(ts(0));
    r.buffers.ensure_on(&mut live);
    for t in &trades {
        live.on_trade(*t);
    }
    live.on_tick(ts(8), None);
    let mut pos = PositionCtx::at_fill(1.0, ts(1));
    for t in &trades {
        pos.fold_price(t.price);
    }
    let direct = read_rule(&r, &live, Some(&pos), ts(8));
    assert_eq!(replay.conditions, direct.conditions);
    assert!(replay.conditions.iter().all(|x| x.value.is_finite()));
}

/// The series row at an instant equals the replay at that instant.
#[test]
fn a_series_row_equals_the_replay_at_its_instant() {
    let r = rule(json!({
        "enter": { "filters": [{ "metric": "m_flow.buy_sol", "span": "10s", "is": [{ "operator": ">=", "value": 2.0 }] }] },
        "always": [{ "if": [c("m_position.retrace_pct", ">=", 20.0)], "sell": "trail" }]
    }));
    let trades = vec![trade(1.0, 1), trade(2.0, 3), trade(1.5, 6)];
    let ctx = ReplayCtx { created_at: ts(0), entry: Some((ts(1), 1.0)), stage: None, tags: None };
    let series = replay_series(&r, trades.clone(), &ctx, ts(30), None, None);
    assert!(!series.at.is_empty());
    for (i, &at) in series.at.iter().enumerate() {
        let point = replay_readout(&r, trades.clone(), &ctx, at);
        for (k, col) in series.conditions.iter().enumerate() {
            let row = col.row(i);
            let p = &point.conditions[k];
            assert_eq!(row.part, p.part);
            assert!(row.value.to_bits() == p.value.to_bits() || (row.value.is_nan() && p.value.is_nan()), "row {i} col {k}");
            assert_eq!(row.ok, p.ok);
        }
    }
}
