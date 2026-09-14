//! `hydrate_token`: a rule switched on mid-run decides on a token born before it
//! exactly as it would have had it been on from the token's birth. Each case books
//! the rule "on from birth" and "switched on later + hydrated" over the same prints
//! and asserts the same decision; the control shows the switched-on rule is blind
//! without the rebuild.

use std::sync::Arc;

use chrono::{Duration, TimeZone, Utc};
use hunter_engine::event::{BuildBreadth, Effect, Event, LoadedRule, Mint, RuleId, TradeMode};
use hunter_engine::fingerprint::{AxisId, AxisPredicate, Criteria, Fingerprint, FingerprintId};
use hunter_engine::grouping::TokenFingerprint;
use hunter_engine::metrics::{Side, TradeLite, Ts};
use hunter_engine::rule_params::RuleParams;
use hunter_engine::{hydrate_token, reduce, EngineState, HydrateFacts};
use serde_json::json;
use uuid::Uuid;

const CREATOR: u64 = 999;

fn ts(secs: f64) -> Ts {
    Utc.timestamp_opt(1_700_000_000, 0).unwrap() + Duration::milliseconds((secs * 1000.0) as i64)
}

fn fp() -> Fingerprint {
    Fingerprint {
        id: FingerprintId(Uuid::from_u128(0xF)),
        wildcard: false,
        criteria: Criteria::new().with(AxisId::CuLimit, AxisPredicate::exact(200_000)),
        metric_config: json!({}),
    }
}

fn axes() -> TokenFingerprint {
    TokenFingerprint { cu_limit: Some(200_000), ..Default::default() }
}

/// Buys on the first print that is a sell of 1 SOL or more once 3 wallets other than
/// the creator have bought since birth: a lifetime term (the buyer set) and a
/// one-print window, the two shapes a mid-run switch-on gets wrong without a rebuild.
fn crowd_rule(id: u128) -> LoadedRule {
    rule(id, json!({
        "entry": {
            "m_crowd_after_age": {"after_age_sec": 0,
                                  "non_creator_buyers": [{"operator": ">=", "value": 3}]},
            "m_flow_window": {"window_size_prints": 1, "sell": [{"operator": ">=", "value": 1}]}
        },
        "take_profit": 50
    }))
}

/// Arms every token and never enters: keeps a token tracked by another rule.
fn idle_rule(id: u128) -> LoadedRule {
    rule(id, json!({
        "entry": {"m_state": {"time": [{"operator": ">=", "value": 100000}]}},
        "take_profit": 50
    }))
}

fn rule(id: u128, params: serde_json::Value) -> LoadedRule {
    LoadedRule {
        id: RuleId(Uuid::from_u128(id)),
        fingerprint_id: fp().id,
        trade_mode: TradeMode::Paper,
        buy_amount_lamports: 100_000_000,
        max_concurrent_tokens: 0,
        max_total_tokens: 0,
        params: RuleParams::parse(&params).expect("valid params"),
        entry_enabled: true,
    }
}

fn reload(state: &mut EngineState, rules: Vec<LoadedRule>) {
    let _ = reduce(state, Event::RulesReloaded { rules: Arc::from(rules), fps: Arc::from(vec![fp()]) });
}

fn create(state: &mut EngineState, mint: &Mint) {
    let _ = reduce(state, Event::TokenCreated {
        mint: mint.clone(),
        fp: Box::new(axes()),
        at: ts(0.0),
        creator_wallet_hash: Some(CREATOR),
        identity: None,
        creation_slot: Some(100),
    });
}

fn print(side: Side, sol: f64, wallet: u64, slot: u64, at: f64) -> TradeLite {
    TradeLite {
        side,
        sol,
        price: 1e-7,
        reserve_sol: 40.0,
        priced_reserve_sol: 70.0,
        at: ts(at),
        wallet_hash: wallet,
        slot,
        on_curve: true,
        ..Default::default()
    }
}

/// Three buyers, then an unrelated small print: history before the switch-on.
fn history() -> Vec<TradeLite> {
    vec![
        print(Side::Buy, 0.5, 1, 100, 0.1),
        print(Side::Buy, 0.5, 2, 101, 1.0),
        print(Side::Buy, 0.5, 3, 102, 2.0),
        print(Side::Buy, 0.1, 1, 103, 3.0),
    ]
}

/// The trigger: a 1.5 SOL sell after the switch-on.
fn trigger() -> TradeLite {
    print(Side::Sell, 1.5, 4, 110, 10.0)
}

fn facts() -> HydrateFacts {
    HydrateFacts {
        fp: axes(),
        created_at: ts(0.0),
        creator_wallet_hash: Some(CREATOR),
        identity: None,
        creation_slot: Some(100),
        first_slot: None,
    }
}

fn buys_for(fx: &[Effect], rule: RuleId) -> usize {
    fx.iter().filter(|e| matches!(e, Effect::SubmitBuy { rule: r, .. } if *r == rule)).count()
}

fn trade(state: &mut EngineState, mint: &Mint, t: TradeLite) -> Vec<Effect> {
    reduce(state, Event::Trade { mint: mint.clone(), trade: t }).into_vec()
}

#[test]
fn a_rule_on_from_birth_buys_the_trigger() {
    let mint = Mint::from("MINT-birth");
    let r = crowd_rule(1);
    let mut s = EngineState::new();
    reload(&mut s, vec![r.clone()]);
    create(&mut s, &mint);
    for t in history() {
        assert_eq!(buys_for(&trade(&mut s, &mint, t), r.id), 0);
    }
    assert_eq!(buys_for(&trade(&mut s, &mint, trigger()), r.id), 1);
}

#[test]
fn a_rule_switched_on_later_is_blind_without_the_rebuild() {
    let mint = Mint::from("MINT-blind");
    let r = crowd_rule(1);
    let mut s = EngineState::new();
    reload(&mut s, vec![]);
    create(&mut s, &mint);
    for t in history() {
        let _ = trade(&mut s, &mint, t);
    }
    reload(&mut s, vec![r.clone()]);
    assert_eq!(buys_for(&trade(&mut s, &mint, trigger()), r.id), 0);
}

#[test]
fn a_rule_switched_on_later_buys_the_trigger_once_hydrated() {
    let mint = Mint::from("MINT-hydrated");
    let r = crowd_rule(1);
    let mut s = EngineState::new();
    reload(&mut s, vec![]);
    create(&mut s, &mint);
    for t in history() {
        let _ = trade(&mut s, &mint, t);
    }
    reload(&mut s, vec![r.clone()]);
    let fx = hydrate_token(&mut s, &mint, &facts(), Some(&history()), ts(5.0), |id| id == r.id);
    assert_eq!(fx.len(), 1, "one ArmedChanged for the switched-on rule");
    assert_eq!(buys_for(&fx, r.id), 0, "hydration never decides");
    assert_eq!(buys_for(&trade(&mut s, &mint, trigger()), r.id), 1);
}

#[test]
fn a_tracked_token_gets_the_new_rule_and_its_history() {
    let mint = Mint::from("MINT-tracked");
    let (idle, r) = (idle_rule(2), crowd_rule(1));
    let mut s = EngineState::new();
    reload(&mut s, vec![idle.clone()]);
    create(&mut s, &mint);
    for t in history() {
        let _ = trade(&mut s, &mint, t);
    }
    reload(&mut s, vec![idle.clone(), r.clone()]);
    let fx = hydrate_token(&mut s, &mint, &facts(), Some(&history()), ts(5.0), |id| id == r.id);
    assert_eq!(fx.len(), 1);
    let fx = trade(&mut s, &mint, trigger());
    assert_eq!(buys_for(&fx, r.id), 1);
    assert_eq!(buys_for(&fx, idle.id), 0, "the other rule's arm is untouched");
}

#[test]
fn a_rule_switched_off_and_on_is_rearmed() {
    let mint = Mint::from("MINT-paused");
    let r = crowd_rule(1);
    let mut s = EngineState::new();
    reload(&mut s, vec![r.clone(), idle_rule(2)]);
    create(&mut s, &mint);
    reload(&mut s, vec![idle_rule(2)]);
    for t in history() {
        let _ = trade(&mut s, &mint, t);
    }
    reload(&mut s, vec![r.clone(), idle_rule(2)]);
    assert_eq!(buys_for(&trade(&mut s, &mint, print(Side::Buy, 0.1, 5, 104, 4.0)), r.id), 0);
    let mut all = history();
    all.push(print(Side::Buy, 0.1, 5, 104, 4.0));
    let _ = hydrate_token(&mut s, &mint, &facts(), Some(&all), ts(5.0), |id| id == r.id);
    assert_eq!(buys_for(&trade(&mut s, &mint, trigger()), r.id), 1);
}

/// The recipe every door-test buy goes through, public on the loaded table.
const PUBLIC_BUILD: u64 = 0xB1;

/// The loss door alone on a sell trigger: buys a 1 SOL sell while public-app first
/// buyers hold at least 70 % of live supply.
fn door_rule(id: u128) -> LoadedRule {
    rule(id, json!({
        "entry": {
            "m_holder_book": {"public_app_share": [{"operator": ">=", "value": 70}]},
            "m_flow_window": {"window_size_prints": 1, "sell": [{"operator": ">=", "value": 1}]}
        },
        "take_profit": 50
    }))
}

fn load_public_table(state: &mut EngineState) {
    let breadth = BuildBreadth { build_hash: PUBLIC_BUILD, app_buyers: 101, app_buys: 202 };
    let _ = reduce(state, Event::BuildBreadthReloaded { breadth: Arc::from(vec![breadth]) });
}

/// Three wallets buy through the public recipe, starting `at` seconds after `ts(0)`.
fn public_buys(at: f64) -> Vec<TradeLite> {
    (1..=3)
        .map(|w| TradeLite {
            build_hash: Some(PUBLIC_BUILD),
            token_amount: 1_000.0,
            ..print(Side::Buy, 0.5, w, 100 + w, at + w as f64)
        })
        .collect()
}

/// [`trigger`] with the token amount every live print carries (a `NaN` amount breaks
/// the book).
fn door_trigger() -> TradeLite {
    TradeLite { token_amount: 1_000.0, ..trigger() }
}

#[test]
fn a_door_rule_on_from_birth_buys_the_trigger() {
    let mint = Mint::from("MINT-door-birth");
    let door = door_rule(1);
    let mut s = EngineState::new();
    load_public_table(&mut s);
    reload(&mut s, vec![door.clone()]);
    create(&mut s, &mint);
    for t in public_buys(0.0) {
        let _ = trade(&mut s, &mint, t);
    }
    assert_eq!(buys_for(&trade(&mut s, &mint, door_trigger()), door.id), 1);
}

/// A tracked token rebuilt for a door rule switched on after its buys: each history
/// buy is classed against the loaded table as the live trade path classes it. An
/// unclassed buy reads unknown, `public_app_share` stays `NaN`, and the door refuses
/// the token for life (2026-09-14, `EJF7c3...`: rebuilt 12 s after birth, refused).
#[test]
fn a_door_rule_switched_on_later_sees_the_public_holders() {
    let mint = Mint::from("MINT-door");
    let (idle, door) = (idle_rule(2), door_rule(1));
    let mut s = EngineState::new();
    load_public_table(&mut s);
    reload(&mut s, vec![idle.clone()]);
    create(&mut s, &mint);
    for t in public_buys(0.0) {
        let _ = trade(&mut s, &mint, t);
    }
    reload(&mut s, vec![idle.clone(), door.clone()]);
    let _ = hydrate_token(&mut s, &mint, &facts(), Some(&public_buys(0.0)), ts(5.0), |id| id == door.id);
    assert_eq!(buys_for(&trade(&mut s, &mint, door_trigger()), door.id), 1);
}

/// The loaded table is `now`'s UTC day's: a history buy from an earlier day was
/// classed against a table the engine no longer holds, so it stays unknown and the
/// door fails closed rather than class it on the wrong day's breadth.
#[test]
fn a_history_buy_from_before_the_tables_day_stays_unknown() {
    let mint = Mint::from("MINT-door-midnight");
    let (idle, door) = (idle_rule(2), door_rule(1));
    let mut s = EngineState::new();
    load_public_table(&mut s);
    reload(&mut s, vec![idle.clone()]);
    create(&mut s, &mint);
    for t in public_buys(0.0) {
        let _ = trade(&mut s, &mint, t);
    }
    reload(&mut s, vec![idle.clone(), door.clone()]);
    // `ts(0)` is 22:13:20 UTC, so two hours on is the next UTC day.
    let now = ts(2.0 * 3600.0);
    assert_ne!(now.date_naive(), ts(0.0).date_naive());
    let _ = hydrate_token(&mut s, &mint, &facts(), Some(&public_buys(0.0)), now, |id| id == door.id);
    let late = TradeLite { at: now, ..door_trigger() };
    assert_eq!(buys_for(&trade(&mut s, &mint, late), door.id), 0);
}

#[test]
fn a_rule_that_reads_new_state_asks_for_a_rebuild_and_a_removal_does_not() {
    let mut s = EngineState::new();
    reload(&mut s, vec![idle_rule(2)]);
    let idle_only = s.track_requirements();
    reload(&mut s, vec![idle_rule(2), crowd_rule(1)]);
    let with_crowd = s.track_requirements();
    assert!(with_crowd.adds_to(&idle_only), "a crowd anchor and a print window are new");
    assert!(!idle_only.adds_to(&with_crowd), "dropping a rule leaves nothing unfolded");
    assert!(!with_crowd.adds_to(&with_crowd));
}
