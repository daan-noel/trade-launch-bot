//! Wave leftover: 2nd Axiom-program buy after a gap, mid tip, hole + tip_seen.
//!
//! Pins parse + compile of `hunter/scripts/seed-ax2-midtips-rule.sql`.
//! Does not retune the cell.

use chrono::{TimeZone, Utc};
use hunter_engine::arm::{is_leftover_metric, CompiledRule, EntryVerdict};
use hunter_engine::event::{LoadedRule, RuleId, TradeMode};
use hunter_engine::fingerprint::{AxisId, AxisPredicate, Criteria, FingerprintId};
use hunter_engine::hash::HashedSet;
use hunter_engine::metrics::burst_slot::BurstPatterns;
use hunter_engine::metrics::fee::FeeKeys;
use hunter_engine::metrics::template_grain::{grain_id_hash, program_id_hash};
use hunter_engine::metrics::track::TokenTrack;
use hunter_engine::metrics::{MetricId, Side, TradeLite, Ts};
use hunter_engine::rule_params::{EntryLock, ExitSide, ReEntry, RuleParams};
use serde_json::json;
use uuid::Uuid;

const WORKING_PROGRAMS: &[&str] = &["Axiom Trade"];

const AXIOM: &str = "Axiom Trade|CU|ATA|F";
const AXIOM_N: &str = "Axiom Trade|CU|ATA|N|F";
const AXIOM_ATA: &str = "Axiom Trade|ATA|F";
const PUMP: &str = "Pump.Fun";

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

fn ax2_params() -> serde_json::Value {
    json!({
        "exclusive": true,
        "priority": 10,
        "reentry": { "cooldown_sec": 0, "max_episodes_per_token": 1 },
        "entry_lock": "slot",
        "entry_event": {
            "m_burst_wave": {
                "this_member": [{"operator": "=", "value": 1}],
                "this_working": [{"operator": "=", "value": 1}],
                "working_buy_count": [{"operator": "=", "value": 2}],
                "gap_slots": [{"operator": ">=", "value": 2}],
                "this_tip": [
                    {"operator": ">=", "value": 100000},
                    {"operator": "<", "value": 1000000}
                ]
            }
        },
        "entry": {
            "m_burst_wave": {
                "hole": [{"operator": "=", "value": 1}],
                "tip_seen": [{"operator": "=", "value": 1}]
            }
        },
        "exit": harvest_exit()
    })
}

fn compile(params: serde_json::Value) -> CompiledRule {
    let parsed = RuleParams::parse(&params).unwrap_or_else(|e| panic!("ax2 params: {e}"));
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

fn ts(secs: i64) -> Ts {
    Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap()
}

fn patterns(programs: &[&str]) -> BurstPatterns {
    let mut h = HashedSet::default();
    for id in programs {
        h.insert(program_id_hash(id));
    }
    BurstPatterns::new(HashedSet::default()).with_programs(h)
}

fn member_buy(
    slot: u64,
    tx: Option<u32>,
    wallet: u64,
    grain: &str,
    tip: Option<u64>,
) -> TradeLite {
    TradeLite {
        side: Side::Buy,
        sol: 0.4,
        price: 1.0,
        reserve_sol: 10.0,
        priced_reserve_sol: 40.0,
        at: ts(slot as i64),
        slot,
        tx_index: tx,
        template_hash: Some(grain_id_hash(grain)),
        program_hash: Some(program_id_hash(grain.split('|').next().unwrap_or(grain))),
        wallet_hash: wallet,
        on_curve: true,
        is_launch: false,
        fee: FeeKeys::new(None, None, tip),
        ..Default::default()
    }
}

fn primed(c: &CompiledRule) -> TokenTrack {
    let mut track = TokenTrack::new(ts(0));
    track.ensure_burst(FingerprintId(Uuid::nil()), &patterns(WORKING_PROGRAMS));
    for w in &c.flow_windows {
        track.ensure_window(*w);
    }
    track.seed_creation_slot(10);
    track.on_trade(member_buy(10, Some(1), 9, PUMP, Some(0)));
    track
}

#[test]
fn working_list_is_the_axiom_program() {
    assert_eq!(WORKING_PROGRAMS, ["Axiom Trade"]);
}

#[test]
fn no_cu_axiom_grain_still_completes() {
    let c = compile(ax2_params());
    let mut track = primed(&c);
    track.on_trade(member_buy(20, Some(1), 1, PUMP, Some(200_000)));
    track.on_trade(member_buy(20, Some(2), 2, AXIOM_ATA, Some(150_000)));
    track.on_trade(member_buy(20, Some(4), 3, AXIOM_ATA, Some(250_000)));
    assert_eq!(c.try_enter(&track, ts(20), None), EntryVerdict::Enter);
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
fn leftover_metrics_are_hole_and_tip_seen() {
    assert!(is_leftover_metric(MetricId::WaveHole));
    assert!(is_leftover_metric(MetricId::WaveTipSeen));
    assert!(is_leftover_metric(MetricId::WorkingTemplatesSeen));
    assert!(!is_leftover_metric(MetricId::WaveThisWorking));
    assert!(!is_leftover_metric(MetricId::WaveThisTip));
}

#[test]
fn rule_parses_and_compiles_leftover_out_of_entry() {
    let params = ax2_params();
    let p = RuleParams::parse(&params).unwrap_or_else(|e| panic!("{e}"));
    assert!(p.exclusive);
    assert_eq!(p.entry_lock, Some(EntryLock::Slot));
    assert_eq!(
        p.reentry,
        Some(ReEntry {
            cooldown_sec: 0.0,
            max_episodes_per_token: 1
        })
    );
    assert!(matches!(p.exit, Some(ExitSide::Dnf(ref cs)) if cs.len() == 2));
    assert_eq!(RuleParams::parse(&p.to_value()).unwrap(), p);

    let c = compile(params);
    assert!(c.exclusive);
    assert_eq!(c.entry_lock, Some(EntryLock::Slot));
    assert_eq!(c.exit_clauses.len(), 2);
    assert_eq!(c.trail_arm_pct, Some(10.0));
    assert!(c.event_reqs.iter().any(|r| r.metric == MetricId::WaveThisMember));
    assert!(c.event_reqs.iter().any(|r| r.metric == MetricId::WaveThisWorking));
    assert!(c.event_reqs.iter().any(|r| r.metric == MetricId::WaveWorkingBuyCount));
    assert!(c.event_reqs.iter().any(|r| r.metric == MetricId::WaveGapSlots));
    assert!(c.event_reqs.iter().any(|r| r.metric == MetricId::WaveThisTip));
    assert!(!c.event_reqs.iter().any(|r| r.metric == MetricId::ThisWorking));
    assert_eq!(c.leftover_reqs.len(), 2);
    assert!(c.leftover_reqs.iter().any(|r| r.metric == MetricId::WaveHole));
    assert!(c.leftover_reqs.iter().any(|r| r.metric == MetricId::WaveTipSeen));
    assert!(c.entry_reqs.is_empty());
}

#[test]
fn completing_print_enters_on_second_working_mid_tip_hole_and_seen() {
    let c = compile(ax2_params());
    let mut track = primed(&c);
    track.on_trade(member_buy(20, Some(1), 1, PUMP, Some(200_000)));
    track.on_trade(member_buy(20, Some(2), 2, AXIOM, Some(150_000)));
    assert_eq!(c.try_enter(&track, ts(20), None), EntryVerdict::No);
    track.on_trade(member_buy(20, Some(4), 3, AXIOM, Some(250_000)));
    assert_eq!(c.try_enter(&track, ts(20), None), EntryVerdict::Enter);
}

#[test]
fn consecutive_slot_wave_still_completes() {
    let c = compile(ax2_params());
    let mut track = primed(&c);
    track.on_trade(member_buy(20, Some(5), 1, PUMP, Some(200_000)));
    track.on_trade(member_buy(20, Some(6), 2, AXIOM, Some(150_000)));
    track.on_trade(member_buy(21, Some(8), 3, AXIOM_N, Some(250_000)));
    // 8 - 6 > 1 and mid-tip already seen on the pump print.
    assert_eq!(c.try_enter(&track, ts(21), None), EntryVerdict::Enter);
}

#[test]
fn leftover_fail_exhausts_when_no_hole() {
    let c = compile(ax2_params());
    let mut track = primed(&c);
    track.on_trade(member_buy(20, Some(1), 1, PUMP, Some(200_000)));
    track.on_trade(member_buy(20, Some(2), 2, AXIOM, Some(150_000)));
    track.on_trade(member_buy(20, Some(3), 3, AXIOM, Some(250_000)));
    assert_eq!(c.try_enter(&track, ts(20), None), EntryVerdict::Exhaust);
}

#[test]
fn leftover_fail_exhausts_when_tip_band_is_new() {
    let c = compile(ax2_params());
    let mut track = primed(&c);
    track.on_trade(member_buy(20, Some(1), 1, PUMP, Some(0)));
    track.on_trade(member_buy(20, Some(2), 2, AXIOM, Some(50_000)));
    track.on_trade(member_buy(20, Some(4), 3, AXIOM, Some(250_000)));
    assert_eq!(c.try_enter(&track, ts(20), None), EntryVerdict::Exhaust);
}

#[test]
fn absent_tip_is_not_mid_band() {
    let c = compile(ax2_params());
    let mut track = primed(&c);
    track.on_trade(member_buy(20, Some(1), 1, PUMP, Some(200_000)));
    track.on_trade(member_buy(20, Some(2), 2, AXIOM, Some(150_000)));
    track.on_trade(member_buy(20, Some(4), 3, AXIOM, None));
    assert_eq!(c.try_enter(&track, ts(20), None), EntryVerdict::No);
}

#[test]
fn pumpfun_is_not_working_so_does_not_complete() {
    let c = compile(ax2_params());
    let mut track = primed(&c);
    track.on_trade(member_buy(20, Some(1), 1, AXIOM, Some(200_000)));
    track.on_trade(member_buy(20, Some(3), 2, PUMP, Some(200_000)));
    assert_eq!(c.try_enter(&track, ts(20), None), EntryVerdict::No);
}
