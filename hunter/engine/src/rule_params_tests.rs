//! Tests for [`super::RuleParams`]: the grammar round-trips, and every authoring mistake
//! is refused with a message that names where it is.

use super::*;
use serde_json::json;

fn c(metric: &str, op: &str, v: f64) -> Value {
    json!({ "metric": metric, "is": [{ "operator": op, "value": v }] })
}

/// The 7ix BROAD 0.65 rule, written the way a person reads it.
fn crew_rule() -> Value {
    json!({
        "enter": {
            "event": [c("m_state.age_sec", ">=", 1.0)],
            "filters": [c("m_state.liquidity_sol", ">=", 14.0)],
            "lock": "token"
        },
        "signals": {
            "cashout": [
                [{ "metric": "m_holdings.profit_sol", "tag": "volume", "is": [{ "operator": ">=", "value": 1.18 }] }],
                [
                    { "metric": "m_holdings.profit_sol", "tag": "volume", "is": [{ "operator": ">=", "value": 0.71 }] },
                    { "metric": "m_flow.buy_sol", "tag": "!volume", "span": "3s", "is": [{ "operator": ">=", "value": 1.0 }] }
                ]
            ]
        },
        "always": [
            { "if": [c("m_state.liquidity_sol", ">=", 80.0)], "sell": "top" },
            { "if": [{ "metric": "m_flow.sell_tx_count", "tag": "dump", "span": "1p", "is": [{ "operator": ">=", "value": 1.0 }] }], "sell": "dump" },
            { "if": [c("m_position.held_sec", ">=", 1000.0)], "sell": "clock" }
        ],
        "stages": [
            { "name": "early", "ends": { "age_sec": 20.0 },
              "on": [{ "if": [{ "signal": "cashout" }], "sell": "spike" }] },
            { "name": "late",
              "on": [{ "if": [{ "signal": "cashout" }], "go": "ride" }] },
            { "name": "ride", "ends": { "stage_sec": 30.0 }, "then": "hold",
              "on": [{ "if": [{ "metric": "m_flow.buy_sol", "tag": "!volume", "span": "10s", "is": [{ "operator": ">=", "value": 2.0 }] }], "sell": "burst" }] },
            { "name": "hold" }
        ]
    })
}

#[test]
fn a_full_rule_round_trips() {
    let v = crew_rule();
    let p = RuleParams::parse(&v).unwrap();
    assert_eq!(p.stages.len(), 4);
    assert_eq!(p.signals["cashout"].len(), 2);
    assert_eq!(p.to_value(), v);
    assert_eq!(RuleParams::parse(&p.to_value()).unwrap(), p);
}

#[test]
fn off_items_round_trip_and_are_not_read() {
    let v = json!({
        "enter": { "filters": [
            c("m_state.liquidity_sol", ">=", 14.0),
            { "metric": "m_state.age_sec", "is": [{ "operator": "<", "value": 30.0 }], "off": true }
        ] },
        "always": [
            { "if": [c("m_position.pnl_pct", ">=", 50.0)], "sell": "tp" },
            { "if": [c("m_position.pnl_pct", "<=", -20.0)], "sell": "sl", "off": true }
        ]
    });
    let p = RuleParams::parse(&v).unwrap();
    assert_eq!(p.to_value(), v);
    assert_eq!(p.metric_refs().len(), 2, "the parked condition and line are not read");
}

#[test]
fn an_empty_rule_buys_on_arming() {
    let p = RuleParams::parse(&json!({})).unwrap();
    assert!(p.enter_on_arm());
    assert_eq!(p.to_value(), json!({}));
}

#[test]
fn every_mistake_names_where_it_is() {
    let mut cases: Vec<(Value, &str)> = vec![
        (json!({ "entry": {} }), "unknown key `entry`"),
        (json!({ "enter": { "filters": [c("m_state.agee", ">", 1.0)] } }), "enter.filters[0]"),
        (json!({ "enter": { "filters": [c("m_position.pnl_pct", ">", 1.0)] } }), "before the buy"),
        (json!({ "enter": { "lock": "token" } }), "needs an enter.event"),
        (json!({ "enter": { "size_pct_of_pool": 20.0 } }), "size_pct_of_pool"),
        (json!({ "always": [{ "sell": "x" }] }), "no condition"),
        (json!({ "always": [{ "if": [c("m_state.age_sec", ">", 1.0)] }] }), "does nothing"),
        (json!({ "always": [{ "if": [c("m_state.age_sec", ">", 1.0)], "go": "nowhere" }] }), "no stage named `nowhere`"),
        (json!({ "always": [{ "if": [{ "signal": "nope" }], "sell": true }] }), "no signal `nope`"),
        (json!({ "always": [{ "if": [{ "metric": "m_state.age_sec", "is": [{ "operator": ">", "value": 5.0 }, { "operator": "<", "value": 2.0 }] }], "sell": true }] }), "age_sec"),
        (json!({ "signals": { "a": [[{ "signal": "b" }]], "b": [[c("m_state.age_sec", ">", 1.0)]] } }), "not other signals"),
        (json!({ "stages": [{ "name": "a", "on": [{ "if": [c("m_state.age_sec", ">", 1.0)], "sell": true, "sell_pct": 50.0 }] }] }), "must also `go`"),
        (json!({ "stages": [{ "name": "a" }, { "name": "a" }] }), "two stages"),
        (json!({ "stages": [{ "name": "a", "ends": { "age_sec": 5.0 } }] }), "no stage follows"),
        (json!({ "stages": [{ "name": "a", "at_end": [{ "sell": true }] }] }), "no deadline"),
        (json!({ "stages": [{ "name": "a", "ends": { "minutes": 5.0 } }, { "name": "b" }] }), "unknown clock"),
        (json!({ "stages": [{ "name": "Early" }] }), "a-z"),
        (json!({ "reentry": { "cooldown_sec": 5.0 } }), "max_per_coin"),
    ];
    cases.push((json!({ "take_profit": -5.0 }), "take_profit"));
    for (v, needle) in cases {
        let e = RuleParams::parse(&v).unwrap_err();
        assert!(e.contains(needle), "{v}\n -> {e}\n (wanted `{needle}`)");
    }
}

#[test]
fn an_at_end_line_may_be_unconditional() {
    let v = json!({ "stages": [
        { "name": "check", "ends": { "age_sec": 60.0 }, "at_end": [{ "sell": "dead at 60 s" }] },
        { "name": "after" }
    ] });
    assert!(RuleParams::parse(&v).is_ok());
}

/// Numbers as `f64`, so `1` and `1.0` compare equal (the frontend writes whatever a
/// person typed; the engine writes `f64`).
fn norm(v: &Value) -> Value {
    match v {
        Value::Number(n) => json!(n.as_f64().expect("finite")),
        Value::Array(a) => Value::Array(a.iter().map(norm).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), norm(v))).collect()),
        other => other.clone(),
    }
}

/// `fixtures/rule_v2_full.json` uses every part of the grammar. The frontend's rule
/// model round-trips the same file key for key (`validate.test.ts`), so the editor and
/// the engine read and write one document.
#[test]
fn the_shared_full_rule_fixture_parses_and_round_trips() {
    let doc: Value = serde_json::from_str(include_str!("../fixtures/rule_v2_full.json")).expect("fixture parses");
    let p = RuleParams::parse(&doc).expect("the shared fixture is a valid rule");
    assert_eq!(norm(&p.to_value()), norm(&doc));
    assert_eq!(p.stages.len(), 3);
    assert_eq!(p.stages[2].at_end.len(), 1);
}
