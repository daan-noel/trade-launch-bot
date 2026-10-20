//! Tests for [`super::RuleParams`]: the grammar round-trips, and every authoring mistake
//! is refused with a message that names where it is.

use super::*;
use serde_json::json;

fn c(metric: &str, op: &str, v: f64) -> Value {
    json!({ "metric": metric, "is": [{ "operator": op, "value": v }] })
}

/// A staged rule: signals, a go line, tagged reads, and fixed exits.
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
            ],
            "burst": [
                [{ "metric": "m_flow.buy_sol", "tag": "!volume", "span": "10s", "is": [{ "operator": ">=", "value": 2.0 }] }]
            ]
        },
        "always": [
            { "if": [c("m_state.liquidity_sol", ">=", 80.0)], "sell": "top" },
            { "if": [{ "metric": "m_flow.sell_tx_count", "tag": "dump", "span": "1p", "is": [{ "operator": ">=", "value": 1.0 }] }], "sell": "dump" },
            { "if": [c("m_position.held_sec", ">=", 1000.0)], "sell": "clock" },
            { "if": [{ "signal": "cashout" }, c("m_state.age_sec", "<", 20.0)], "sell": "spike" },
            { "if": [{ "signal": "cashout" }, c("m_state.age_sec", ">=", 20.0)], "go": "ride" }
        ],
        "stages": [
            { "name": "open" },
            { "name": "ride",
              "on": [{ "if": [{ "signal": "burst" }, c("m_position.stage_sec", "<=", 30.0)], "sell": "burst" }] }
        ]
    })
}

#[test]
fn a_full_rule_round_trips() {
    let v = crew_rule();
    let p = RuleParams::parse(&v).unwrap();
    assert_eq!(p.stages.len(), 2);
    assert_eq!(p.signals["cashout"].len(), 2);
    assert_eq!(p.signals["burst"].len(), 1);
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

/// The per-rule buy slippage survives a save/load and does not make a rule gate its buy:
/// it is an execution setting, never a condition.
#[test]
fn entry_slippage_round_trips_without_gating_the_buy() {
    let v = json!({ "enter": { "slippage_pct": 65.0 } });
    let p = RuleParams::parse(&v).unwrap();
    assert_eq!(p.enter.slippage_pct, Some(65.0));
    assert_eq!(p.to_value(), v);
    assert!(p.enter_on_arm());
    let blank = RuleParams::parse(&json!({ "enter": { "slippage_pct": null } })).unwrap();
    assert_eq!(blank.enter.slippage_pct, None);
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
        (json!({ "enter": { "slippage_pct": 0.0 } }), "enter.slippage_pct"),
        (json!({ "enter": { "slippage_pct": 95.0 } }), "enter.slippage_pct"),
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

/// `fixtures/rule_v2_tree.json` is the 6ix stage-tree rule; the frontend round-trips the
/// same file (`validate.test.ts`).
#[test]
fn the_shared_tree_rule_fixture_parses_and_round_trips() {
    let doc: Value = serde_json::from_str(include_str!("../fixtures/rule_v2_tree.json")).expect("fixture parses");
    let p = RuleParams::parse(&doc).expect("the shared tree fixture is a valid rule");
    assert_eq!(norm(&p.to_value()), norm(&doc));
    let t = p.tree.as_ref().expect("a stage tree");
    assert_eq!((t.phases.len(), t.situations.len(), t.cells.len()), (6, 6, 2));
    assert_eq!(p.enter.cells.len(), 6);
}

/// A stage's own line that goes to that stage and keeps part of the bag would never act.
#[test]
fn a_stage_line_to_its_own_stage_is_refused() {
    let go_self = json!({ "stages": [{ "name": "a", "on": [{ "if": [c("m_position.pnl_pct", ">=", 10.0)], "go": "a" }] }] });
    assert!(RuleParams::parse(&go_self).unwrap_err().contains("goes to its own stage `a`"));
    let part_self = json!({ "stages": [{ "name": "a", "on": [{ "if": [c("m_position.pnl_pct", ">=", 10.0)], "sell": "p", "sell_pct": 50.0, "go": "a" }] }] });
    assert!(RuleParams::parse(&part_self).unwrap_err().contains("own stage"));
    let all_self = json!({ "stages": [{ "name": "a", "on": [{ "if": [c("m_position.pnl_pct", ">=", 10.0)], "sell": "x", "go": "a" }] }] });
    assert!(RuleParams::parse(&all_self).is_ok(), "selling everything acts wherever it goes");
}

/// A loop of deadline moves needs a stage_sec wait above 0 somewhere in it.
#[test]
fn a_deadline_loop_without_a_wait_is_refused() {
    let loop_on_age = json!({ "stages": [
        { "name": "a", "ends": { "age_sec": 20.0 }, "then": "b" },
        { "name": "b", "ends": { "held_sec": 30.0 }, "then": "a" }
    ] });
    let e = RuleParams::parse(&loop_on_age).unwrap_err();
    assert!(e.contains("a -> b -> a") && e.contains("stage_sec"), "{e}");
    let zero = json!({ "stages": [{ "name": "a", "ends": { "stage_sec": 0.0 }, "then": "a" }] });
    assert!(RuleParams::parse(&zero).is_err());
    let recheck = json!({ "stages": [{ "name": "a", "ends": { "stage_sec": 30.0 }, "then": "a",
        "at_end": [{ "if": [c("m_position.pnl_pct", "<", 0.0)], "sell": "weak" }] }] });
    assert!(RuleParams::parse(&recheck).is_ok(), "a check every 30 s");
    let implicit_next = json!({ "stages": [
        { "name": "a", "ends": { "age_sec": 20.0 } },
        { "name": "b", "ends": { "stage_sec": 5.0 }, "then": "a" }
    ] });
    assert!(RuleParams::parse(&implicit_next).is_ok(), "b waits 5 s each time round");
}

#[test]
fn a_copy_shortcut_canonicalizes_to_the_episode_and_refuses_a_mixed_document() {
    let copy = json!({ "copy": { "min_buy_sol": 0.04 } });
    let p = RuleParams::parse(&copy).unwrap();
    assert!(matches!(p.copy, Some(CopyForm::Spec(s)) if s.min_buy_sol == 0.04));
    let stored = p.to_value();
    assert_eq!(stored.get("copy").and_then(|v| v.as_bool()), Some(true));
    assert!(stored.get("enter").is_some());
    assert!(stored.get("stages").is_some());
    let again = RuleParams::parse(&stored).unwrap();
    assert!(matches!(again.copy, Some(CopyForm::Document)));
    assert_eq!(again.to_value(), stored);
    assert!(!p.enter_on_arm());
    let expanded = p.expand_copy();
    assert!(expanded.copy.is_none());
    assert_eq!(expanded.enter.event.len(), 2);
    assert_eq!(expanded.stages.len(), 2);
    assert!(expanded.stages[0].on[0].sell.unwrap().of_bag);
    let mixed = json!({ "copy": { "min_buy_sol": 0.04 }, "stages": [] });
    assert!(RuleParams::parse(&mixed).unwrap_err().contains("only"));
    let bag = json!({ "stages": [
        { "name": "a", "on": [{ "if": [c("m_position.pnl_pct", ">=", 10.0)], "sell": true, "sell_of": "bag", "go": "b" }] },
        { "name": "b" }
    ] });
    assert!(RuleParams::parse(&bag).unwrap_err().contains("sell_of"));
}

#[test]
fn a_copy_document_keeps_the_episode_the_editor_wrote() {
    let spec = RuleParams::parse(&json!({ "copy": { "min_buy_sol": 0.04 } })).unwrap();
    let mut episode = spec.expand_copy();
    episode.copy = Some(CopyForm::Document);
    let stored = episode.to_value();
    assert_eq!(stored.get("copy").and_then(|v| v.as_bool()), Some(true));
    let parsed = RuleParams::parse(&stored).unwrap();
    assert!(matches!(parsed.copy, Some(CopyForm::Document)));
    let again = parsed.expand_copy();
    assert!(again.copy.is_none());
    assert_eq!(again.enter.event.len(), 2);
    assert!(again.stages[0].on[0].sell.unwrap().of_bag);
    assert_eq!(parsed.to_value(), stored);
    assert_eq!(parsed.follow, CopyFollow::All);
}

#[test]
fn follow_bought_roundtrips_and_each_is_refused() {
    let mut episode = RuleParams::parse(&json!({ "copy": { "min_buy_sol": 0.04 } })).unwrap().expand_copy();
    episode.copy = Some(CopyForm::Document);
    episode.follow = CopyFollow::Bought;
    let stored = episode.to_value();
    assert_eq!(stored.get("follow").and_then(|v| v.as_str()), Some("bought"));
    let parsed = RuleParams::parse(&stored).unwrap();
    assert_eq!(parsed.follow, CopyFollow::Bought);
    assert_eq!(parsed.to_value(), stored);
    let each = json!({ "copy": true, "follow": "each", "enter": { "event": [] } });
    assert!(RuleParams::parse(&each).unwrap_err().contains("each"));
    let bare = json!({ "follow": "bought", "enter": { "event": [] } });
    assert!(RuleParams::parse(&bare).unwrap_err().contains("unknown"));
}

/// A stage tree: two phases, two situations, a row entry, one exact cell and entry cells.
fn tree_doc() -> Value {
    json!({
        "enter": { "event": [c("m_state.age_sec", ">=", 1.0)], "cells": [["late", "*"], ["early", "calm"]] },
        "phases": [
            { "name": "early", "until": [c("m_state.peak_liquidity_sol", ">=", 45.0)] },
            { "name": "late" }
        ],
        "situations": [
            { "name": "hot", "when": [c("m_state.liquidity_sol", ">=", 30.0)] },
            { "name": "calm" }
        ],
        "cells": [
            { "phase": "*", "situation": "hot", "on": [{ "if": [c("m_state.liquidity_sol", ">=", 30.0)], "sell": "burst" }] },
            { "phase": "late", "situation": "hot", "on": [{ "if": [c("m_state.liquidity_sol", ">=", 60.0)], "sell": "big" }] }
        ]
    })
}

#[test]
fn a_stage_tree_round_trips() {
    let v = tree_doc();
    let p = RuleParams::parse(&v).unwrap();
    let t = p.tree.as_ref().unwrap();
    assert_eq!((t.phases.len(), t.situations.len(), t.cells.len()), (2, 2, 2));
    assert_eq!(p.to_value(), v);
    assert_eq!(RuleParams::parse(&p.to_value()).unwrap(), p);
}

/// A part sell in a cell stops itself when its `sold_pct` range is no wider than the sell.
#[test]
fn a_cell_part_sell_needs_a_sold_pct_range_no_wider_than_the_sell() {
    let with = |conds: Vec<Value>, pct: f64| {
        let mut v = tree_doc();
        v["cells"] = json!([{ "phase": "*", "situation": "hot", "on": [{ "if": conds, "sell": "half", "sell_pct": pct }] }]);
        RuleParams::parse(&v)
    };
    let hot = || c("m_state.liquidity_sol", ">=", 30.0);
    assert!(with(vec![hot(), c("m_position.sold_pct", "<", 50.0)], 50.0).is_ok());
    assert!(with(vec![hot(), c("m_position.sold_pct", ">=", 50.0), c("m_position.sold_pct", "<", 75.0)], 25.0).is_ok());
    assert!(with(vec![hot(), c("m_position.sold_pct", "<=", 50.0)], 50.0).is_err(), "at 50 % it would sell again");
    assert!(with(vec![hot(), c("m_position.sold_pct", "<", 50.0)], 30.0).is_err(), "a second 30 % fits under 50");
    assert!(with(vec![hot()], 50.0).is_err());
}

#[test]
fn a_stage_tree_refuses_what_could_not_run_as_written() {
    let refused = |patch: Value, needle: &str| {
        let mut v = tree_doc();
        for (k, x) in patch.as_object().unwrap() {
            v[k] = x.clone();
        }
        let e = RuleParams::parse(&v).unwrap_err();
        assert!(e.contains(needle), "`{e}` should say `{needle}`");
    };
    refused(json!({ "phases": [{ "name": "early", "until": [c("m_state.liquidity_sol", ">=", 45.0)] }, { "name": "late" }] }), "can fall");
    refused(json!({ "phases": [{ "name": "early", "until": [c("m_state.age_sec", "<", 5.0)] }, { "name": "late" }] }), "use > or >=");
    refused(json!({ "phases": [{ "name": "early" }, { "name": "late" }] }), "needs an `until`");
    refused(json!({ "situations": [{ "name": "hot", "when": [c("m_state.liquidity_sol", ">=", 30.0)] }, { "name": "calm", "when": [c("m_state.age_sec", ">=", 1.0)] }] }), "catches the rest");
    refused(json!({ "cells": [
        { "phase": "late", "situation": "*", "on": [{ "if": [c("m_state.age_sec", ">=", 9.0)], "sell": "a" }] },
        { "phase": "*", "situation": "hot", "on": [{ "if": [c("m_state.age_sec", ">=", 9.0)], "sell": "b" }] }
    ] }), "covered by both");
    refused(json!({ "cells": [{ "phase": "*", "situation": "hot", "on": [{ "if": [c("m_state.age_sec", ">=", 9.0)], "sell": "a", "sell_pct": 50.0 }] }] }), "part sell");
    refused(json!({ "cells": [{ "phase": "*", "situation": "hot", "on": [{ "if": [c("m_state.age_sec", ">=", 9.0), c("m_position.sold_pct", "<", 60.0)], "sell": "a", "sell_pct": 50.0 }] }] }), "part sell");
    refused(json!({ "cells": [{ "phase": "nope", "situation": "hot", "on": [] }] }), "no phase `nope`");
    refused(json!({ "stages": [{ "name": "a" }] }), "not both");
    let mut no_tree = tree_doc();
    for k in ["phases", "situations", "cells"] {
        no_tree.as_object_mut().unwrap().remove(k);
    }
    assert!(RuleParams::parse(&no_tree).unwrap_err().contains("enter.cells needs a stage tree"));
}
