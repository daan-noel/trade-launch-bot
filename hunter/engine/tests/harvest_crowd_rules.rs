//! The concentrated harvest: two exclusive rules, one door fingerprint, DNF exit.
//!
//! Pins parse + compile of the mapping in
//! `hunter/docs/plans/strategies/ix-live-rule.md`. Does not retune the cell.

use chrono::{TimeZone, Utc};
use hunter_engine::arm::{CompiledRule, EntryVerdict};
use hunter_engine::event::{LoadedRule, RuleId, TradeMode};
use hunter_engine::fingerprint::{AxisId, AxisPredicate, Criteria, FingerprintId};
use hunter_engine::hash::HashedSet;
use hunter_engine::metrics::burst_slot::BurstPatterns;
use hunter_engine::metrics::template_grain::grain_id_hash;
use hunter_engine::metrics::track::TokenTrack;
use hunter_engine::metrics::{MetricId, Side, TradeLite, Ts, WindowUnit};
use hunter_engine::rule_params::{EntryLock, ExitSide, ReEntry, RuleParams};
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

fn shared_event() -> serde_json::Value {
    json!({
        "m_burst_slot": {
            "this_member": [{"operator": "=", "value": 1}],
            "this_working": [{"operator": "=", "value": 1}],
            "has_new": [{"operator": "=", "value": 1}],
            "has_unknown": [{"operator": "=", "value": 0}]
        }
    })
}

fn shared_filters() -> serde_json::Value {
    json!({
        "m_state": { "time": [{"operator": ">=", "value": 20}] },
        "m_flow_window": {
            "window_size_slots": 4,
            "window_lag": 1,
            "buy_count": [{"operator": "=", "value": 0}]
        },
        "m_burst_slot": {
            "pre_slot_liquidity": [{"operator": "<", "value": 16}]
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

fn merge_burst(base: serde_json::Value, extra: serde_json::Value) -> serde_json::Value {
    let mut e = base;
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

fn harvest_params(event: serde_json::Value) -> serde_json::Value {
    harvest_params_with_entry(event, shared_filters())
}

fn harvest_params_with_entry(event: serde_json::Value, entry: serde_json::Value) -> serde_json::Value {
    json!({
        "exclusive": true,
        "priority": 10,
        "reentry": { "cooldown_sec": 0, "max_episodes_per_token": 1 },
        "entry_lock": "slot",
        "entry_event": event,
        "entry": entry,
        "exit": harvest_exit()
    })
}

fn tools4_entry() -> serde_json::Value {
    merge_burst(
        shared_filters(),
        json!({
            "m_burst_slot": {
                "working_templates_seen": [{"operator": ">=", "value": 4}]
            }
        }),
    )
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
    assert!(WORKING.contains(&"Bloom Router|CU|F"));
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
    let params = harvest_params(merge_burst(
        shared_event(),
        json!({
            "m_burst_slot": {
                "member_template_count": [{"operator": "=", "value": 1}],
                "same_buy_count": [{"operator": ">=", "value": 2}],
                "same_buy_sol": [
                    {"operator": ">=", "value": 0.9},
                    {"operator": "<", "value": 4}
                ],
                "same_wallet_count": [{"operator": ">=", "value": 2}]
            }
        }),
    ));
    let p = RuleParams::parse(&params).unwrap_or_else(|e| panic!("{e}"));
    assert!(p.exclusive);
    assert_eq!(p.entry_lock, Some(EntryLock::Slot));
    assert_eq!(
        p.reentry,
        Some(ReEntry { cooldown_sec: 0.0, max_episodes_per_token: 1 })
    );
    assert!(matches!(p.exit, Some(ExitSide::Dnf(ref cs)) if cs.len() == 2));
    assert_eq!(RuleParams::parse(&p.to_value()).unwrap(), p);

    let c = compile(params);
    assert!(c.exclusive);
    assert_eq!(c.entry_lock, Some(EntryLock::Slot));
    assert_eq!(c.exit_clauses.len(), 2, "DNF: trail OR death (no TP/SL)");
    assert_eq!(c.trail_arm_pct, Some(10.0));
    assert!(c.needs_slot, "4sl@1 + m_burst_slot both need the slot column");
    assert!(c.entry_reqs.iter().any(|r| r.metric == MetricId::Time));
    assert!(c.entry_reqs.iter().any(|r| r.metric == MetricId::BuyCount
        && r.window.primary.is_some_and(|w| w.size == 4.0 && w.lag == 1.0 && w.unit == WindowUnit::Slot)));
    assert!(c.event_reqs.iter().any(|r| r.metric == MetricId::MemberTemplateCount));
    assert!(!c.event_reqs.iter().any(|r| r.metric == MetricId::WorkingTemplateCount));
    assert!(!c.event_reqs.iter().any(|r| r.metric == MetricId::WorkingBuyShare));
    assert!(c.event_reqs.iter().any(|r| r.metric == MetricId::SameBuySol));
    assert!(c.event_reqs.iter().any(|r| r.metric == MetricId::ThisMember));
    assert!(c.event_reqs.iter().any(|r| r.metric == MetricId::HasNew));
    assert!(!c.event_reqs.iter().any(|r| r.metric == MetricId::Packed));
    assert!(c.leftover_reqs.is_empty());
}

#[test]
fn rule_b_mixed_parses_and_compiles() {
    let params = harvest_params(merge_burst(
        shared_event(),
        json!({
            "m_burst_slot": {
                "working_template_count": [{"operator": ">=", "value": 2}],
                "working_buy_sol": [
                    {"operator": ">=", "value": 0.9},
                    {"operator": "<", "value": 4}
                ],
                "working_wallet_count": [{"operator": ">=", "value": 2}]
            }
        }),
    ));
    let p = RuleParams::parse(&params).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(RuleParams::parse(&p.to_value()).unwrap(), p);
    let c = compile(params);
    assert!(c.exclusive);
    assert_eq!(c.exit_clauses.len(), 2);
    assert!(c.event_reqs.iter().any(|r| r.metric == MetricId::WorkingBuySol));
    assert!(!c.event_reqs.iter().any(|r| r.metric == MetricId::WorkingBuyShare));
    assert!(!c.event_reqs.iter().any(|r| r.metric == MetricId::SameBuySol));
    assert!(c.leftover_reqs.is_empty());
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

#[test]
fn entry_lock_without_event_is_rejected() {
    let e = RuleParams::parse(&json!({ "entry_lock": "slot" })).unwrap_err();
    assert!(e.contains("entry_lock"), "{e}");
}

// ── Completing-print lock ──────────────────────────────────────────────────

fn ts(secs: i64) -> Ts {
    Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap()
}

fn patterns(ids: &[&str]) -> BurstPatterns {
    let mut h = HashedSet::default();
    for id in ids {
        h.insert(grain_id_hash(id));
    }
    BurstPatterns::new(h)
}

fn member_buy(
    slot: u64,
    tx: u32,
    wallet: u64,
    sol: f64,
    secs: i64,
    price: f64,
    grain: &str,
) -> TradeLite {
    TradeLite {
        side: Side::Buy,
        sol,
        price,
        reserve_sol: 10.0,
        priced_reserve_sol: 40.0,
        at: ts(secs),
        slot,
        tx_index: Some(tx),
        template_hash: Some(grain_id_hash(grain)),
        wallet_hash: wallet,
        on_curve: true,
        is_launch: false,
        ..Default::default()
    }
}

const AXIOM: &str = "Axiom Trade|CU|ATA|F";
const PHOTON: &str = "Photon|CU|ATA|F";
const TERMINAL: &str = "Terminal|CU|ATA|F";
const GMGN: &str = "GMGN|CU|ATA|F";
const PUMP: &str = "Pump.Fun";

fn harvest_a_compiled() -> CompiledRule {
    compile(harvest_params(merge_burst(
        shared_event(),
        json!({
            "m_burst_slot": {
                "member_template_count": [{"operator": "=", "value": 1}],
                "same_buy_count": [{"operator": ">=", "value": 2}],
                "same_buy_sol": [
                    {"operator": ">=", "value": 0.9},
                    {"operator": "<", "value": 4}
                ],
                "same_wallet_count": [{"operator": ">=", "value": 2}]
            }
        }),
    )))
}

fn harvest_b_compiled() -> CompiledRule {
    compile(harvest_params(merge_burst(
        shared_event(),
        json!({
            "m_burst_slot": {
                "working_template_count": [{"operator": ">=", "value": 2}],
                "working_buy_sol": [
                    {"operator": ">=", "value": 0.9},
                    {"operator": "<", "value": 4}
                ],
                "working_wallet_count": [{"operator": ">=", "value": 2}]
            }
        }),
    )))
}

fn harvest_a_tools4_compiled() -> CompiledRule {
    compile(harvest_params_with_entry(
        merge_burst(
            shared_event(),
            json!({
                "m_burst_slot": {
                    "member_template_count": [{"operator": "=", "value": 1}],
                    "same_buy_count": [{"operator": ">=", "value": 2}],
                    "same_buy_sol": [
                        {"operator": ">=", "value": 0.9},
                        {"operator": "<", "value": 4}
                    ],
                    "same_wallet_count": [{"operator": ">=", "value": 2}]
                }
            }),
        ),
        tools4_entry(),
    ))
}

fn harvest_b_tools4_compiled() -> CompiledRule {
    compile(harvest_params_with_entry(
        merge_burst(
            shared_event(),
            json!({
                "m_burst_slot": {
                    "working_template_count": [{"operator": ">=", "value": 2}],
                    "working_buy_sol": [
                        {"operator": ">=", "value": 0.9},
                        {"operator": "<", "value": 4}
                    ],
                    "working_wallet_count": [{"operator": ">=", "value": 2}]
                }
            }),
        ),
        tools4_entry(),
    ))
}

fn primed(c: &CompiledRule, ids: &[&str]) -> TokenTrack {
    let mut track = TokenTrack::new(ts(0));
    track.ensure_burst(FingerprintId(Uuid::nil()), &patterns(ids));
    for w in &c.flow_windows {
        track.ensure_window(*w);
    }
    track
}

#[test]
fn completing_print_is_the_buy_that_crosses_size() {
    let c = harvest_a_compiled();
    let mut track = primed(&c, &[AXIOM]);
    // Quiet 4sl@1: a buy in slot 1, then resume at slot 6 (dslot = 5).
    track.on_trade(member_buy(1, 1, 9, 0.1, 1, 1.0, AXIOM));
    // Age 25 s, trail 31 on the first resume buy (0.4 SOL — not yet 0.9).
    track.on_trade(member_buy(6, 1, 1, 0.4, 25, 0.69, AXIOM));
    assert_eq!(c.try_enter(&track, ts(25), None), EntryVerdict::No);
    // Second wallet, tot 1.0 — completing print.
    track.on_trade(member_buy(6, 2, 2, 0.6, 25, 0.69, AXIOM));
    assert_eq!(c.try_enter(&track, ts(25), None), EntryVerdict::Enter);
    // A third buy in the same slot is not a new candidate once locked.
    track.on_trade(member_buy(6, 3, 3, 0.1, 25, 0.69, AXIOM));
    assert_eq!(c.try_enter(&track, ts(25), Some(6)), EntryVerdict::No);
}

#[test]
fn failed_depth_on_completing_print_spends_the_slot() {
    let c = harvest_a_compiled();
    let mut track = primed(&c, &[AXIOM]);
    let mut quiet = member_buy(1, 1, 9, 0.1, 1, 1.0, AXIOM);
    quiet.reserve_sol = 50.0;
    track.on_trade(quiet);
    // Event crosses; pre_slot_liquidity is the previous slot's reserve (50).
    track.on_trade(member_buy(6, 1, 1, 0.5, 25, 1.0, AXIOM));
    track.on_trade(member_buy(6, 2, 2, 0.5, 25, 1.0, AXIOM));
    assert_eq!(c.try_enter(&track, ts(25), None), EntryVerdict::SpendSlot);
    // A later sell cannot reopen this slot.
    track.on_trade(TradeLite {
        side: Side::Sell,
        sol: 2.0,
        price: 0.5,
        reserve_sol: 8.0,
        slot: 6,
        at: ts(25),
        on_curve: true,
        ..Default::default()
    });
    assert_eq!(c.try_enter(&track, ts(25), Some(6)), EntryVerdict::No);
}

#[test]
fn pumpfun_padding_does_not_fill_rule_b_band() {
    let c = harvest_b_compiled();
    let mut track = primed(&c, &[AXIOM, PHOTON]);
    track.on_trade(member_buy(1, 1, 9, 0.1, 1, 1.0, AXIOM));
    track.on_trade(member_buy(6, 1, 1, 0.3, 25, 0.69, AXIOM));
    track.on_trade(member_buy(6, 2, 2, 0.3, 25, 0.69, PHOTON));
    track.on_trade(member_buy(6, 3, 3, 5.0, 25, 0.69, PUMP));
    // Working-list size is 0.6; Pump.Fun is a member but not on the list.
    assert_eq!(c.try_enter(&track, ts(25), None), EntryVerdict::No);
    track.on_trade(member_buy(6, 4, 2, 0.4, 25, 0.69, PHOTON));
    // Working size now 1.0. Pump.Fun is not on the list, so it does not
    // fill the band and does not block once the working pack itself is in.
    assert_eq!(c.try_enter(&track, ts(25), None), EntryVerdict::Enter);
}

#[test]
fn mixed_working_pack_fires_rule_b() {
    let c = harvest_b_compiled();
    let mut track = primed(&c, &[AXIOM, PHOTON]);
    track.on_trade(member_buy(1, 1, 9, 0.1, 1, 1.0, AXIOM));
    track.on_trade(member_buy(6, 1, 1, 0.5, 25, 0.69, AXIOM));
    track.on_trade(member_buy(6, 2, 2, 0.5, 25, 0.69, PHOTON));
    assert_eq!(c.try_enter(&track, ts(25), None), EntryVerdict::Enter);
}

#[test]
fn all_repeat_working_list_does_not_fire() {
    let c = harvest_a_compiled();
    let mut track = primed(&c, &[AXIOM]);
    track.on_trade(member_buy(1, 1, 1, 0.1, 1, 1.0, AXIOM));
    track.on_trade(member_buy(1, 2, 2, 0.1, 1, 1.0, AXIOM));
    track.on_trade(member_buy(6, 1, 1, 0.5, 25, 0.69, AXIOM));
    track.on_trade(member_buy(6, 2, 2, 0.5, 25, 0.69, AXIOM));
    assert_eq!(c.try_enter(&track, ts(25), None), EntryVerdict::No);
}

#[test]
fn pumpfun_padding_does_not_fire_rule_a() {
    let c = harvest_a_compiled();
    let mut track = primed(&c, &[AXIOM]);
    track.on_trade(member_buy(1, 1, 9, 0.1, 1, 1.0, AXIOM));
    track.on_trade(member_buy(6, 1, 1, 0.5, 25, 0.69, AXIOM));
    track.on_trade(member_buy(6, 2, 3, 3.0, 25, 0.69, PUMP));
    track.on_trade(member_buy(6, 3, 2, 0.5, 25, 0.69, AXIOM));
    // same_buy_sol is 1.0 in band; Pump.Fun makes member_template_count 2.
    assert_eq!(c.try_enter(&track, ts(25), None), EntryVerdict::No);
}

#[test]
fn tools4_compiles_leftover_out_of_entry() {
    let a = harvest_a_tools4_compiled();
    assert_eq!(a.leftover_reqs.len(), 1);
    assert_eq!(a.leftover_reqs[0].metric, MetricId::WorkingTemplatesSeen);
    assert!(!a.entry_reqs.iter().any(|r| r.metric == MetricId::WorkingTemplatesSeen));
    assert!(a.entry_reqs.iter().any(|r| r.metric == MetricId::Time));
    let b = harvest_b_tools4_compiled();
    assert_eq!(b.leftover_reqs.len(), 1);
    assert_eq!(b.leftover_reqs[0].metric, MetricId::WorkingTemplatesSeen);
}

#[test]
fn parent_a_still_enters_on_one_working_grain() {
    let c = harvest_a_compiled();
    let mut track = primed(&c, WORKING);
    track.on_trade(member_buy(1, 1, 9, 0.1, 1, 1.0, AXIOM));
    track.on_trade(member_buy(6, 1, 1, 0.5, 25, 0.69, AXIOM));
    track.on_trade(member_buy(6, 2, 2, 0.5, 25, 0.69, AXIOM));
    assert_eq!(c.try_enter(&track, ts(25), None), EntryVerdict::Enter);
}

#[test]
fn leftover_fail_on_completing_print_exhausts_the_episode() {
    let c = harvest_a_tools4_compiled();
    let mut track = primed(&c, WORKING);
    track.on_trade(member_buy(1, 1, 9, 0.1, 1, 1.0, AXIOM));
    track.on_trade(member_buy(6, 1, 1, 0.5, 25, 0.69, AXIOM));
    track.on_trade(member_buy(6, 2, 2, 0.5, 25, 0.69, AXIOM));
    assert_eq!(c.try_enter(&track, ts(25), None), EntryVerdict::Exhaust);
}

#[test]
fn leftover_fail_with_depth_fail_still_spends_the_slot() {
    let c = harvest_a_tools4_compiled();
    let mut track = primed(&c, WORKING);
    let mut quiet = member_buy(1, 1, 9, 0.1, 1, 1.0, AXIOM);
    quiet.reserve_sol = 50.0;
    track.on_trade(quiet);
    track.on_trade(member_buy(6, 1, 1, 0.5, 25, 1.0, AXIOM));
    track.on_trade(member_buy(6, 2, 2, 0.5, 25, 1.0, AXIOM));
    assert_eq!(c.try_enter(&track, ts(25), None), EntryVerdict::SpendSlot);
}

#[test]
fn leftover_pass_enters_when_four_working_grains_already_seen() {
    let c = harvest_a_tools4_compiled();
    let mut track = primed(&c, WORKING);
    track.on_trade(member_buy(1, 1, 9, 0.1, 1, 1.0, AXIOM));
    track.on_trade(member_buy(2, 1, 10, 0.1, 5, 1.0, PHOTON));
    track.on_trade(member_buy(3, 1, 11, 0.1, 10, 1.0, TERMINAL));
    track.on_trade(member_buy(4, 1, 12, 0.1, 15, 1.0, GMGN));
    track.on_trade(member_buy(9, 1, 1, 0.5, 25, 0.69, AXIOM));
    assert_eq!(c.try_enter(&track, ts(25), None), EntryVerdict::No);
    track.on_trade(member_buy(9, 2, 2, 0.5, 25, 0.69, AXIOM));
    assert_eq!(c.try_enter(&track, ts(25), None), EntryVerdict::Enter);
}

#[test]
fn leftover_pass_enters_rule_b_mixed() {
    let c = harvest_b_tools4_compiled();
    let mut track = primed(&c, WORKING);
    track.on_trade(member_buy(1, 1, 9, 0.1, 1, 1.0, AXIOM));
    track.on_trade(member_buy(2, 1, 10, 0.1, 5, 1.0, TERMINAL));
    track.on_trade(member_buy(3, 1, 11, 0.1, 10, 1.0, GMGN));
    track.on_trade(member_buy(9, 1, 1, 0.5, 25, 0.69, AXIOM));
    track.on_trade(member_buy(9, 2, 2, 0.5, 25, 0.69, PHOTON));
    assert_eq!(c.try_enter(&track, ts(25), None), EntryVerdict::Enter);
}
