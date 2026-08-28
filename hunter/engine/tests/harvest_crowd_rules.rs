//! The concentrated harvest: two exclusive rules, one door fingerprint, DNF exit.
//!
//! Pins parse + compile of the mapping in
//! `hunter/docs/plans/strategies/ix-live-rule.md`. Does not retune the cell.

use hunter_engine::arm::CompiledRule;
use hunter_engine::event::{LoadedRule, RuleId, TradeMode};
use hunter_engine::fingerprint::{AxisId, AxisPredicate, Criteria, FingerprintId};
use hunter_engine::metrics::{MetricId, WindowUnit};
use hunter_engine::rule_params::{ExitSide, ReEntry, RuleParams};
use serde_json::json;
use uuid::Uuid;

const WORKING: &[&str] = &[
    "Axiom Trade|CU|ATA|F",
    "Axiom Trade|CU|ATA|N|F",
    "Photon|CU|ATA|F",
    "Terminal|CU|ATA|F",
    "GMGN Bot|CU|ATA|F",
    "GMGN|CU|ATA|F",
    "Bloom Router|CU|F",
    "Bloom|CU|F",
];

fn shared_entry() -> serde_json::Value {
    json!({
        "m_state": { "time": [{"operator": ">=", "value": 20}] },
        "m_flow_window": {
            "window_size_slots": 4,
            "window_lag": 1,
            "buy_count": [{"operator": "=", "value": 0}]
        },
        "m_burst_slot": {
            "working_template": [{"operator": "=", "value": 1}],
            "new_on_mint_wallets": [{"operator": ">=", "value": 1}],
            "pre_slot_liquidity": [{"operator": "<", "value": 16}],
            "pre_print_trail": [{"operator": ">=", "value": 15}]
        }
    })
}

fn harvest_exit() -> serde_json::Value {
    json!([
        { "m_position": {
            "armed": [{"operator": "=", "value": 1}],
            "retrace": [{"operator": ">=", "value": 18}],
            "arm_above_pct": 10
        } },
        {
            "m_position": { "armed": [{"operator": "=", "value": 0}] },
            "m_flow_window": {
                "window_size_sec": 8,
                "buy_count": [{"operator": "=", "value": 0}]
            }
        }
    ])
}

fn merge_entry(extra: serde_json::Value) -> serde_json::Value {
    let mut e = shared_entry();
    let extra = extra.as_object().expect("object");
    let dst = e.as_object_mut().expect("object");
    for (k, v) in extra {
        if k == "m_burst_slot" {
            let src = v.as_object().expect("burst extra");
            let burst = dst.get_mut("m_burst_slot").unwrap().as_object_mut().unwrap();
            for (bk, bv) in src {
                burst.insert(bk.clone(), bv.clone());
            }
        } else {
            dst.insert(k.clone(), v.clone());
        }
    }
    e
}

fn harvest_params(entry: serde_json::Value) -> serde_json::Value {
    json!({
        "exclusive": true,
        "priority": 10,
        "reentry": { "cooldown_sec": 0, "max_episodes_per_token": 100 },
        "entry": entry,
        "exit": harvest_exit()
    })
}

fn compile(params: serde_json::Value) -> CompiledRule {
    let parsed = RuleParams::parse(&params).unwrap_or_else(|e| panic!("harvest params: {e}"));
    CompiledRule::compile(&LoadedRule {
        id: RuleId(Uuid::nil()),
        fingerprint_id: FingerprintId(Uuid::nil()),
        trade_mode: TradeMode::Paper,
        buy_amount_lamports: 100_000_000,
        max_concurrent_tokens: 0,
        max_total_tokens: 0,
        entry_enabled: true,
        params: parsed,
    })
}

#[test]
fn working_list_is_the_eight_grain_ids() {
    assert_eq!(WORKING.len(), 8);
    assert!(WORKING.iter().any(|s| *s == "Bloom Router|CU|F"));
    assert!(WORKING.iter().all(|s| !s.contains("LAUNCH")));
}

#[test]
fn door_fingerprint_is_ata_plus_init_and_first_slot() {
    let mut c = Criteria::new();
    c.insert(AxisId::CreateAta, AxisPredicate::exact(1));
    c.insert(
        AxisId::InitBuyLamports,
        AxisPredicate::range(Some(200_000_000), None),
    );
    c.insert(
        AxisId::FirstSlotBuyLamports,
        AxisPredicate::range(Some(500_000_000), None),
    );
    assert!(c.problems().is_empty(), "{:?}", c.problems());
}

#[test]
fn rule_a_same_template_parses_and_compiles() {
    let params = harvest_params(merge_entry(json!({
        "m_burst_slot": {
            "slot_template_count": [{"operator": "=", "value": 1}],
            "template_buy_count": [{"operator": ">=", "value": 2}],
            "template_buy_sol": [
                {"operator": ">=", "value": 0.9},
                {"operator": "<", "value": 4}
            ],
            "template_wallet_count": [{"operator": ">=", "value": 2}]
        }
    })));
    let p = RuleParams::parse(&params).unwrap_or_else(|e| panic!("{e}"));
    assert!(p.exclusive);
    assert_eq!(
        p.reentry,
        Some(ReEntry { cooldown_sec: 0.0, max_episodes_per_token: 100 })
    );
    assert!(matches!(p.exit, Some(ExitSide::Dnf(ref cs)) if cs.len() == 2));
    assert_eq!(RuleParams::parse(&p.to_value()).unwrap(), p);

    let c = compile(params);
    assert!(c.exclusive);
    assert_eq!(c.exit_clauses.len(), 2, "DNF: trail OR death (no TP/SL)");
    assert_eq!(c.trail_arm_pct, Some(10.0));
    assert!(c.needs_slot, "4sl@1 + m_burst_slot both need the slot column");
    assert!(c.entry_reqs.iter().any(|r| r.metric == MetricId::Time));
    assert!(c.entry_reqs.iter().any(|r| r.metric == MetricId::BuyCount
        && r.window.primary.is_some_and(|w| w.size == 4.0 && w.lag == 1.0 && w.unit == WindowUnit::Slot)));
    assert!(c.entry_reqs.iter().any(|r| r.metric == MetricId::SlotTemplateCount));
    assert!(c.entry_reqs.iter().any(|r| r.metric == MetricId::TemplateBuySol));
    assert!(!c.entry_reqs.iter().any(|r| r.metric == MetricId::Packed));
}

#[test]
fn rule_b_mixed_parses_and_compiles() {
    let params = harvest_params(merge_entry(json!({
        "m_burst_slot": {
            "slot_template_count": [{"operator": ">=", "value": 2}],
            "slot_buy_sol": [
                {"operator": ">=", "value": 0.9},
                {"operator": "<", "value": 4}
            ],
            "slot_wallet_count": [{"operator": ">=", "value": 2}]
        }
    })));
    let p = RuleParams::parse(&params).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(RuleParams::parse(&p.to_value()).unwrap(), p);
    let c = compile(params);
    assert!(c.exclusive);
    assert_eq!(c.exit_clauses.len(), 2);
    assert!(c.entry_reqs.iter().any(|r| r.metric == MetricId::SlotBuySol));
    assert!(!c.entry_reqs.iter().any(|r| r.metric == MetricId::TemplateBuySol));
}

#[test]
fn object_form_exit_still_compiles_to_singleton_clauses() {
    let p = RuleParams::parse(&json!({
        "exit": { "m_position": { "retrace": [{"operator": ">=", "value": 18}] } }
    }))
    .unwrap();
    assert!(matches!(p.exit, Some(ExitSide::Any(_))));
    let c = compile(json!({
        "exit": { "m_position": { "retrace": [{"operator": ">=", "value": 18}] } }
    }));
    assert_eq!(c.exit_clauses.len(), 1);
    assert_eq!(c.exit_clauses[0].len(), 1);
    assert_eq!(c.exit_clauses[0][0].metric, MetricId::Retrace);
}

#[test]
fn dnf_empty_clause_is_rejected() {
    let e = RuleParams::parse(&json!({ "exit": [ {} ] })).unwrap_err();
    assert!(e.contains("empty"), "{e}");
}
