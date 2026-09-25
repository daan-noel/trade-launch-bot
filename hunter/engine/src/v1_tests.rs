//! Tests for the v1 converter: each v1 shape converts to a v2 document that parses, and
//! every stored rule and fingerprint converts (the snapshot test, pointed at an export).

use super::*;
use crate::rule_params::RuleParams;
use serde_json::json;

fn v2(v1: Value) -> Value {
    let out = convert_params(&v1).unwrap_or_else(|e| panic!("{v1} -> {e}"));
    RuleParams::parse(&out).unwrap_or_else(|e| panic!("converted rule does not parse: {e}\n{out:#}"));
    out
}

fn ge(v: f64) -> Value {
    json!([{ "operator": ">=", "value": v }])
}

#[test]
fn entry_event_lock_size_and_shortcuts_carry_over() {
    let out = v2(json!({
        "entry": { "m_state": { "liquidity": ge(14.0) }, "m_flow_window": { "window_size_sec": 10, "buy": ge(2.0) } },
        "entry_event": { "m_state": { "time": ge(1.0) } },
        "entry_lock": "token",
        "buy_pct_of_vsol": 1.5,
        "take_profit": 100, "stop_loss": 30,
        "reentry": { "cooldown_sec": 5.0, "max_episodes_per_token": 3 }
    }));
    assert_eq!(out["enter"]["lock"], "token");
    assert_eq!(out["enter"]["size_pct_of_pool"], 1.5);
    assert_eq!(out["enter"]["event"][0]["metric"], "m_state.age_sec");
    let filters = out["enter"]["filters"].as_array().unwrap();
    assert!(filters.iter().any(|c| c["metric"] == "m_flow.buy_sol" && c["span"] == "10s"));
    assert_eq!(out["reentry"]["max_per_coin"], 3);
    assert_eq!(out["take_profit"], 100);
}

#[test]
fn fingerprint_groups_become_tags_and_metrics_read_them() {
    let out = v2(json!({ "entry": {
        "m_flow_ix_window": { "window_size_sec": 3, "untagged_buy": ge(1.0) },
        "m_flow_ix": { "tagged_pnl": ge(0.8) },
        "m_dump_ix_window": { "window_size_prints": 1, "dump_sell_count": ge(1.0) },
        "m_copy_window": { "window_size_slots": 1, "buy_count": ge(1.0) },
        "m_holder_book": { "bundled_share": ge(30.0) }
    } }));
    let f = out["enter"]["filters"].as_array().unwrap();
    let find = |p: &str| f.iter().find(|c| c["metric"] == p).unwrap_or_else(|| panic!("{p} missing in {out}"));
    assert_eq!(find("m_flow.buy_sol")["tag"], "!volume");
    assert_eq!(find("m_flow.buy_sol")["span"], "3s");
    assert_eq!(find("m_holdings.profit_sol")["tag"], "volume");
    assert_eq!(find("m_flow.sell_tx_count")["tag"], "dump");
    assert_eq!(find("m_flow.sell_tx_count")["span"], "1p");
    assert_eq!(find("m_flow.buy_tx_count")["tag"], "targets");
    assert_eq!(find("m_holdings.bag_share_pct")["tag"], "bundled");
}

#[test]
fn leftover_metrics_become_final_filters() {
    let out = v2(json!({ "entry": { "m_burst_slot": { "working_templates_seen": ge(4.0), "this_member": [{ "operator": "=", "value": 1.0 }] } } }));
    assert_eq!(out["enter"]["final_filters"][0]["metric"], "m_crowd.unique_ix_templates");
    assert_eq!(out["enter"]["filters"][0]["metric"], "m_slot.this_joined");
}

#[test]
fn a_lone_trailing_clause_carries_its_gate() {
    let out = v2(json!({ "exit": { "m_position": { "retrace": ge(20.0), "arm_above_pct": 5.0 } } }));
    let when = out["always"][0]["if"].as_array().unwrap();
    assert_eq!(when.len(), 2);
    assert_eq!(when[1]["metric"], "m_position.pnl_pct");
}

/// A pnl latch: `start` sells on the crossing event, the armed=0 clause is kept to pnl
/// under the gate, and the move follows.
#[test]
fn a_pnl_latch_becomes_two_stages() {
    let out = v2(json!({ "exit": [
        { "m_position": { "arm_above_pct": 10.0, "armed": [{ "operator": "=", "value": 1 }], "retrace": ge(18.0) } },
        { "m_position": { "armed": [{ "operator": "=", "value": 0 }], "held": ge(180.0) } },
        { "m_position": { "held": ge(1200.0) } }
    ] }));
    assert_eq!(out["always"].as_array().unwrap().len(), 1, "the unlatched clause is an always line");
    let start = out["stages"][0]["on"].as_array().unwrap();
    assert_eq!(start.len(), 3);
    assert_eq!(start[2]["go"], "armed");
    let armed = out["stages"][1]["on"].as_array().unwrap();
    assert_eq!(armed.len(), 1);
    assert_eq!(armed[0]["if"][0]["metric"], "m_position.retrace_pct");
}

/// `arm` clauses (the 7ix ride): start holds the exits and one `go` line per clause;
/// `since_armed` becomes `stage_sec` in the armed stage.
#[test]
fn arm_clauses_become_go_lines() {
    let out = v2(json!({
        "exit": [
            { "m_state": { "liquidity": ge(80.0) } },
            { "m_position": { "armed": [{ "operator": "=", "value": 1 }], "since_armed": [{ "operator": "<=", "value": 30.0 }] },
              "m_flow_ix_window": { "window_size_sec": 10, "untagged_buy": ge(2.0) } }
        ],
        "arm": [{ "m_state": { "time": ge(20.0) }, "m_flow_ix": { "tagged_pnl": ge(1.18) } }]
    }));
    assert_eq!(out["always"].as_array().unwrap().len(), 1);
    assert_eq!(out["stages"][0]["on"][0]["go"], "armed");
    let guard = out["stages"][1]["on"][0]["if"].as_array().unwrap();
    assert!(guard.iter().any(|c| c["metric"] == "m_position.stage_sec"));
}

#[test]
fn a_ladder_becomes_one_stage_per_rung() {
    let out = v2(json!({
        "scale_out": [
            { "sell_bps": 5000, "conditions": { "m_position": { "bounce": ge(50.0) } } },
            { "sell_bps": 3000, "take_profit": 50.0, "conditions": { "m_position": { "held": ge(90.0) } } },
            { "conditions": { "m_price_window": { "window_size_sec": 30, "trail": ge(30.0) } } }
        ],
        "stop_loss": 28.0
    }));
    let stages = out["stages"].as_array().unwrap();
    assert_eq!(stages.len(), 3, "the remainder rung is the last stage");
    assert_eq!(stages[0]["on"][0]["sell_pct"], 50.0);
    assert_eq!(stages[0]["on"][0]["go"], "step_2");
    assert_eq!(stages[1]["on"].as_array().unwrap().len(), 2, "take profit + held");
    assert!(stages[2]["on"][0].get("sell_pct").is_none(), "the remainder sells everything");
}

#[test]
fn parked_items_stay_parked() {
    let out = v2(json!({
        "entry": { "m_state": { "liquidity": ge(14.0) } },
        "exit": { "m_position": { "held": ge(60.0) } },
        "disabled": { "entry": { "m_state": { "time": ge(5.0) } }, "exit": { "m_position": { "pnl": ge(50.0) } } }
    }));
    let f = out["enter"]["filters"].as_array().unwrap();
    assert!(f.iter().any(|c| c["off"] == true));
    assert!(out["always"].as_array().unwrap().iter().any(|l| l["off"] == true));
}

#[test]
fn a_flow_classifier_becomes_the_volume_tag() {
    let tags = convert_metric_config(&json!({
        "m_flow_ix": { "tagged_programs": ["Unknown (x)"], "volume_cluster": { "min_prints": 3, "sol_tol_pct": 10 },
                       "creation_slot_buyers": "excluded", "creator_is_tagged": true, "wallet_contagion": true },
        "m_dump_ix": { "ix_patterns": [["S"]], "creator_is_listed": true },
        "m_copy": { "target_wallets": ["W"] },
        "m_burst_slot": { "working_templates": ["Axiom Trade|CU|ATA|F", "Photon"] }
    }))
    .unwrap();
    assert_eq!(tags["volume"]["sticky"], true);
    assert_eq!(tags["volume"]["exclude_creation_slot"], true);
    assert_eq!(tags["volume"]["match"]["creator"], true);
    assert_eq!(tags["dump"]["side"], "sell");
    assert_eq!(tags["dump"]["match"]["creator"], true);
    assert_eq!(tags["targets"]["match"]["wallet"][0], "W");
    assert_eq!(tags["working"]["match"]["ix_template"][0], "Axiom Trade|CU|ATA|F");
    assert_eq!(tags["working"]["match"]["program"][0], "Photon");
    // A classifier that tags nothing stays a valid tag that matches nothing.
    let none = convert_metric_config(&json!({ "m_flow_ix": { "creator_is_tagged": false, "wallet_contagion": false } })).unwrap();
    assert_eq!(none["volume"]["match"]["ix_shape"], json!([]));
}

/// Every stored rule and fingerprint converts to a v2 document that parses. Point
/// `V1_SNAPSHOT_DIR` at a folder holding `rules_snapshot.json` and
/// `fingerprints_snapshot.json` (the lab API's rule and fingerprint lists).
#[test]
#[ignore]
fn every_stored_rule_and_fingerprint_converts() {
    let dir = std::env::var("V1_SNAPSHOT_DIR").expect("V1_SNAPSHOT_DIR");
    let read = |f: &str| -> Value { serde_json::from_str(&std::fs::read_to_string(format!("{dir}/{f}")).unwrap()).unwrap() };
    let mut failed = Vec::new();
    for r in read("rules_snapshot.json").as_array().unwrap() {
        let name = r["rule_name"].as_str().unwrap_or("?");
        match convert_params(&r["params"]) {
            Ok(v) => {
                if let Err(e) = RuleParams::parse(&v) {
                    failed.push(format!("rule {name}: parse: {e}"));
                }
            }
            Err(e) => failed.push(format!("rule {name}: {e}")),
        }
    }
    let mut identities: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for f in read("fingerprints_snapshot.json").as_array().unwrap() {
        let name = f["name"].as_str().unwrap_or("?");
        match convert_metric_config(&f["metric_config"]) {
            Err(e) => failed.push(format!("fingerprint {name}: {e}")),
            // Row identity is (criteria, wildcard, tags): two v1 rows must not collapse
            // onto one v2 identity, or the migration breaks the unique index.
            Ok(tags) => {
                let id = format!("{}|{}|{}", convert_criteria(&f["criteria"]), f["wildcard"], tags);
                if let Some(other) = identities.insert(id, name.to_string()) {
                    failed.push(format!("fingerprints {other} and {name} convert to one identity"));
                }
            }
        }
        let c = convert_criteria(&f["criteria"]);
        if let Err(e) = serde_json::from_value::<crate::fingerprint::Criteria>(c) {
            failed.push(format!("fingerprint {name}: criteria: {e}"));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// A v1 sweep axis names the same read in v2, with its span spelled the v2 way.
#[test]
fn a_v1_axis_converts_to_its_v2_read() {
    let v1 = json!({ "side": "entry", "group": "m_flow_window", "metric": "buy", "operator": ">=", "window": 30, "values": [1, 2] });
    assert_eq!(
        convert_axis(&v1).unwrap(),
        json!({ "side": "entry", "metric": "m_flow.buy_sol", "span": "30s", "operator": ">=", "values": [1, 2] })
    );
    let tagged = json!({ "side": "exit", "group": "m_flow_ix_window", "metric": "untagged_buy", "operator": "<", "window": "20sl", "values": [3] });
    let out = convert_axis(&tagged).unwrap();
    assert_eq!((out["metric"].as_str(), out["tag"].as_str(), out["span"].as_str()), (Some("m_flow.buy_sol"), Some("!volume"), Some("20sl")));
    let tp = json!({ "kind": "take_profit", "values": [50] });
    assert_eq!(convert_axis(&tp).unwrap(), tp, "a TP axis passes through");
}

/// A sweep's ix patterns become the `volume` tag a fingerprint with them would carry,
/// and a ladder becomes the stages a rule with it would carry.
#[test]
fn stored_sweep_configs_convert_like_the_rules_they_came_from() {
    let p = json!([["Pump.Fun: Buy"]]);
    assert_eq!(ix_patterns_to_tags(&p).unwrap(), convert_metric_config(&json!({ "m_flow_ix": { "ix_patterns": p } })).unwrap());
    let ladder = json!([{ "sell_bps": 5000, "take_profit": 30 }, { "conditions": { "m_position": { "held": [{ "operator": ">=", "value": 20 }] } } }]);
    let stages = convert_ladder(&ladder).unwrap();
    assert_eq!(stages.as_array().map(Vec::len), Some(2));
    crate::rule_params::RuleParams::parse(&json!({ "stages": stages })).expect("the converted stages parse");
}
