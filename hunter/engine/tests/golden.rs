//! Golden-log tests — the engine's executable spec (plan §3.5). Each test scripts
//! an event vector and asserts on the exact decisions the fold emits. They drive
//! the crate's *public* surface (`EngineState` + `reduce`) — the same surface the
//! live / replay / sweep adapters use — so a regression here is a parity break.

use std::sync::Arc;

use chrono::{Duration, TimeZone, Utc};
use hunter_engine::arm::CompiledRule;
use hunter_engine::event::{
    ArmedStateTag, DisarmReason, Effect, Event, ExitReason, Fill, FillFailReason, LoadedRule, Mint,
    PositionId, PositionStatus, RuleId, TradeMode,
};
use hunter_engine::fingerprint::{Fingerprint, FingerprintId};
use hunter_engine::grouping::TokenFingerprint;
use hunter_engine::metrics::{Side, TradeLite, Ts};
use hunter_engine::reduce::reduce;
use hunter_engine::rule_params::RuleParams;
use hunter_engine::EngineState;
use serde_json::{json, Value};
use uuid::Uuid;

// ── Builders ────────────────────────────────────────────────────────────────

fn ts(secs: f64) -> Ts {
    Utc.timestamp_opt(1_700_000_000, 0).unwrap()
        + Duration::milliseconds((secs * 1000.0) as i64)
}

fn rid(n: u128) -> RuleId {
    RuleId(Uuid::from_u128(n))
}

fn fid(n: u128) -> FingerprintId {
    FingerprintId(Uuid::from_u128(n))
}

/// A fingerprint that matches solely on `cu_limit == 200_000` (instant, no
/// first-slot axis) — the default "this token is ours" shape for these tests.
fn cu_fp(id: u128) -> Fingerprint {
    Fingerprint {
        id: fid(id),
        cu_limit: Some(200_000),
        cu_price: None,
        ix_labels: None,
        init_buy_lamports: None,
        max_cost_lamports: None,
        spendable_lamports_in: None,
        first_slot_buy_lamports: None,
        first_slot_sell_lamports: None,
        bucket_size_amount: 0.1,
    }
}

fn rule(id: u128, fp: u128, params: Value) -> LoadedRule {
    rule_capped(id, fp, params, 1, 0)
}

fn rule_capped(id: u128, fp: u128, params: Value, max_concurrent: u32, max_total: u32) -> LoadedRule {
    LoadedRule {
        id: rid(id),
        fingerprint_id: fid(fp),
        trade_mode: TradeMode::Paper,
        buy_amount_lamports: 1_000_000_000,
        max_concurrent_tokens: max_concurrent,
        max_total_tokens: max_total,
        params: RuleParams::parse(&params).expect("valid params"),
    }
}

/// A token whose creation axes match [`cu_fp`].
fn cu_token() -> Box<TokenFingerprint> {
    Box::new(TokenFingerprint { cu_limit: Some(200_000), ..Default::default() })
}

fn reload(rules: Vec<LoadedRule>, fps: Vec<Fingerprint>) -> Event {
    Event::RulesReloaded { rules: Arc::from(rules), fps: Arc::from(fps) }
}

fn trade(sol: f64, price: f64, reserve: f64, at: f64) -> TradeLite {
    TradeLite { side: Side::Buy, sol, price, reserve_sol: reserve, at: ts(at) }
}

fn fill(price: f64, at: f64) -> Fill {
    Fill { price, sol: 1.0, token_amount: 1_000_000, at: ts(at) }
}

// ── Effect extractors ─────────────────────────────────────────────────────────

fn buys(fx: &[Effect]) -> Vec<(RuleId, u64)> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::SubmitBuy { rule, lamports, .. } => Some((*rule, *lamports)),
            _ => None,
        })
        .collect()
}

fn buy_intent(fx: &[Effect]) -> hunter_engine::event::IntentId {
    fx.iter()
        .find_map(|e| match e {
            Effect::SubmitBuy { intent, .. } => Some(intent.clone()),
            _ => None,
        })
        .expect("a SubmitBuy effect")
}

fn sells(fx: &[Effect]) -> Vec<(PositionId, ExitReason)> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::SubmitSell { position, reason, .. } => Some((*position, *reason)),
            _ => None,
        })
        .collect()
}

fn sell_intent(fx: &[Effect]) -> hunter_engine::event::IntentId {
    fx.iter()
        .find_map(|e| match e {
            Effect::SubmitSell { intent, .. } => Some(intent.clone()),
            _ => None,
        })
        .expect("a SubmitSell effect")
}

fn statuses(fx: &[Effect]) -> Vec<PositionStatus> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::PositionUpdate(d) => Some(d.status),
            _ => None,
        })
        .collect()
}

fn disarms(fx: &[Effect]) -> Vec<DisarmReason> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::ArmedChanged(d) => match d.state {
                ArmedStateTag::Disarmed(r) => Some(r),
                ArmedStateTag::Armed => None,
            },
            _ => None,
        })
        .collect()
}

/// One buy-lamports value the buy effect must carry (the rule's configured size).
const BUY: u64 = 1_000_000_000;

// ── Scenarios ─────────────────────────────────────────────────────────────────

#[test]
fn arm_enter_then_take_profit() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(&mut s, reload(vec![rule(1, 1, json!({ "take_profit": 100 }))], vec![cu_fp(1)]));

    // Creation of a matching token → armed + (enter-on-arm) buy.
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) });
    assert_eq!(buys(&fx), vec![(rid(1), BUY)]);
    assert_eq!(statuses(&fx), vec![PositionStatus::BuySubmitted]);
    let entry = buy_intent(&fx);

    // Entry fill → holding.
    let fx = reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.5) });
    assert_eq!(statuses(&fx), vec![PositionStatus::Holding]);

    // A trade at +100% → take-profit sell.
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 2.0, 40.0, 1.0) });
    assert_eq!(sells(&fx).iter().map(|(_, r)| *r).collect::<Vec<_>>(), vec![ExitReason::TakeProfit]);
    let exit = sell_intent(&fx);

    // Exit fill → End(TakeProfit).
    let fx = reduce(&mut s, Event::FillConfirmed { intent: exit, fill: fill(2.0, 1.1) });
    assert_eq!(statuses(&fx), vec![PositionStatus::End]);
    // Token is done forever for this rule — the token was pruned from tracking.
    assert!(!s.tokens.contains_key(&m));
}

#[test]
fn stop_loss_fires_below_threshold() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(&mut s, reload(vec![rule(1, 1, json!({ "stop_loss": 30 }))], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) });
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.5) });
    // −40% (below the −30% floor) → stop-loss.
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 0.6, 40.0, 1.0) });
    assert_eq!(sells(&fx).iter().map(|(_, r)| *r).collect::<Vec<_>>(), vec![ExitReason::StopLoss]);
}

#[test]
fn metrics_exit_on_time_condition() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    let params = json!({ "exit": { "m_snapshot": { "time": [{ "operator": ">", "value": 5 }] } } });
    reduce(&mut s, reload(vec![rule(1, 1, params)], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) });
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.1) });
    // No exit before the deadline.
    let fx = reduce(&mut s, Event::Tick { now: ts(4.0) });
    assert!(sells(&fx).is_empty());
    // Past +5 s → metrics exit (tick-driven).
    let fx = reduce(&mut s, Event::Tick { now: ts(6.0) });
    assert_eq!(sells(&fx).iter().map(|(_, r)| *r).collect::<Vec<_>>(), vec![ExitReason::Metrics]);
}

#[test]
fn stall_exit_on_quiet_token_is_tick_driven() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    let params = json!({ "exit": { "m_price_path": { "stall": [{ "operator": ">", "value": 3 }] } } });
    reduce(&mut s, reload(vec![rule(1, 1, params)], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) });
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.0) });
    // A price move at t=1 resets the stall clock; no exit here.
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.2, 40.0, 1.0) });
    assert!(sells(&fx).is_empty());
    // Quiet until t=5 → stall = 4 s > 3 → exit on the tick alone.
    let fx = reduce(&mut s, Event::Tick { now: ts(5.0) });
    assert_eq!(sells(&fx).iter().map(|(_, r)| *r).collect::<Vec<_>>(), vec![ExitReason::Metrics]);
}

#[test]
fn derived_unsatisfiable_disarms_before_entry() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    // Enter only if within 3 s AND liquidity > 100 (never true) → never enters, but the
    // `time < 3` upper bound lets the arm disarm itself once the clock passes 3 s.
    let params = json!({ "entry": { "m_snapshot": {
        "time": [{ "operator": "<", "value": 3 }],
        "liquidity": [{ "operator": ">", "value": 100 }]
    } } });
    reduce(&mut s, reload(vec![rule(1, 1, params)], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) });
    assert!(buys(&fx).is_empty(), "must not enter with liquidity unmet");
    // Tick past 3 s → derived unsatisfiability disarm.
    let fx = reduce(&mut s, Event::Tick { now: ts(4.0) });
    assert_eq!(disarms(&fx), vec![DisarmReason::Unsatisfiable]);
    assert!(!s.tokens.contains_key(&m), "disarmed + no position → pruned");
}

#[test]
fn dead_token_disarms_armed_rule() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    // Entry never satisfied (liquidity > 100) so the token stays armed until death.
    let params = json!({ "entry": { "m_snapshot": { "liquidity": [{ "operator": ">", "value": 100 }] } } });
    reduce(&mut s, reload(vec![rule(1, 1, params)], vec![cu_fp(1)]));
    reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) });
    // One meaningful trade with depleted reserves at t=10.
    reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.0, 5.0, 10.0) });
    // 300 s of silence past that trade → dead → disarm.
    let fx = reduce(&mut s, Event::Tick { now: ts(310.0) });
    assert_eq!(disarms(&fx), vec![DisarmReason::Dead]);
}

#[test]
fn migration_disarms_armed_rule() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    let params = json!({ "entry": { "m_snapshot": { "liquidity": [{ "operator": ">", "value": 100 }] } } });
    reduce(&mut s, reload(vec![rule(1, 1, params)], vec![cu_fp(1)]));
    reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) });
    let fx = reduce(&mut s, Event::Migrated { mint: m.clone(), at: ts(5.0) });
    assert_eq!(disarms(&fx), vec![DisarmReason::Migrated]);
}

#[test]
fn multi_rule_concurrent_entry_on_one_token() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    // Two rules on the same fingerprint, both enter-on-arm → two concurrent positions.
    let rules = vec![
        rule(1, 1, json!({ "take_profit": 100 })),
        rule(2, 1, json!({ "take_profit": 200 })),
    ];
    reduce(&mut s, reload(rules, vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) });
    let mut fired: Vec<RuleId> = buys(&fx).into_iter().map(|(r, _)| r).collect();
    fired.sort();
    assert_eq!(fired, vec![rid(1), rid(2)]);
    assert_eq!(s.positions.len(), 2, "two independent positions");
}

#[test]
fn concurrent_cap_blocks_second_until_slot_frees() {
    let mut s = EngineState::new();
    let (a, b) = (Mint::from("tokA"), Mint::from("tokB"));
    // max_concurrent = 1.
    reduce(
        &mut s,
        reload(vec![rule_capped(1, 1, json!({ "take_profit": 100 }), 1, 0)], vec![cu_fp(1)]),
    );

    // Token A enters and fills.
    let fx = reduce(&mut s, Event::TokenCreated { mint: a.clone(), fp: cu_token(), at: ts(0.0) });
    let a_entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: a_entry, fill: fill(1.0, 0.1) });

    // Token B arrives while A holds → the cap blocks its entry.
    let fx = reduce(&mut s, Event::TokenCreated { mint: b.clone(), fp: cu_token(), at: ts(1.0) });
    assert!(buys(&fx).is_empty(), "concurrent cap reached — B must wait");

    // A exits and confirms, freeing the slot.
    let fx = reduce(&mut s, Event::Trade { mint: a.clone(), trade: trade(1.0, 2.0, 40.0, 2.0) });
    let a_exit = sell_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: a_exit, fill: fill(2.0, 2.1) });

    // Now a tick lets B enter.
    let fx = reduce(&mut s, Event::Tick { now: ts(3.0) });
    assert_eq!(buys(&fx), vec![(rid(1), BUY)], "B enters once the slot frees");
}

#[test]
fn total_cap_permanently_stops_new_entries() {
    let mut s = EngineState::new();
    let (a, b) = (Mint::from("tokA"), Mint::from("tokB"));
    // max_total = 1, generous concurrency.
    reduce(
        &mut s,
        reload(vec![rule_capped(1, 1, json!({ "take_profit": 100 }), 10, 1)], vec![cu_fp(1)]),
    );
    let fx = reduce(&mut s, Event::TokenCreated { mint: a.clone(), fp: cu_token(), at: ts(0.0) });
    let a_entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: a_entry, fill: fill(1.0, 0.1) });
    let fx = reduce(&mut s, Event::Trade { mint: a.clone(), trade: trade(1.0, 2.0, 40.0, 1.0) });
    let a_exit = sell_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: a_exit, fill: fill(2.0, 1.1) });

    // Even with the slot free, the lifetime cap blocks B.
    let fx = reduce(&mut s, Event::TokenCreated { mint: b.clone(), fp: cu_token(), at: ts(2.0) });
    assert!(buys(&fx).is_empty(), "lifetime cap of 1 reached — B never enters");
}

#[test]
fn entry_fill_failure_retries_then_gives_up() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(&mut s, reload(vec![rule(1, 1, json!({ "take_profit": 100 }))], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) });
    let mut intent = buy_intent(&fx);

    // Two retries, each re-submitting a buy with a fresh intent.
    for _ in 0..2 {
        let fx =
            reduce(&mut s, Event::FillFailed { intent: intent.clone(), reason: FillFailReason::Reverted });
        assert_eq!(buys(&fx), vec![(rid(1), BUY)], "retry re-submits the buy");
        let next = buy_intent(&fx);
        assert_ne!(next, intent, "retry uses a fresh intent id");
        intent = next;
    }
    // Third failure → give up (no more buys), book a terminal failure.
    let fx = reduce(&mut s, Event::FillFailed { intent, reason: FillFailReason::Reverted });
    assert!(buys(&fx).is_empty(), "gives up after MAX_ENTRY_ATTEMPTS");
    assert_eq!(statuses(&fx), vec![PositionStatus::ExitFailed]);
    // Counters rolled back → the token is done, pruned.
    assert!(!s.tokens.contains_key(&m));
}

#[test]
fn entry_fatal_gives_up_without_retry() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(&mut s, reload(vec![rule(1, 1, json!({ "take_profit": 100 }))], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) });
    let intent = buy_intent(&fx);
    let fx = reduce(&mut s, Event::FillFailed { intent, reason: FillFailReason::Fatal });
    assert!(buys(&fx).is_empty(), "Fatal must not retry");
    assert_eq!(statuses(&fx), vec![PositionStatus::ExitFailed]);
    assert!(!s.tokens.contains_key(&m));
}

#[test]
fn manual_close_sells_held_position() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(&mut s, reload(vec![rule(1, 1, json!({ "take_profit": 100 }))], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) });
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.1) });

    let position = *s.positions.keys().next().expect("one open position");
    let fx = reduce(&mut s, Event::ManualClose { position });
    assert_eq!(sells(&fx).iter().map(|(_, r)| *r).collect::<Vec<_>>(), vec![ExitReason::Manual]);
    let exit = sell_intent(&fx);
    let fx = reduce(&mut s, Event::FillConfirmed { intent: exit, fill: fill(1.5, 1.0) });
    assert_eq!(statuses(&fx), vec![PositionStatus::End]);
}

#[test]
fn externally_cleared_closes_without_sell() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(&mut s, reload(vec![rule(1, 1, json!({ "take_profit": 100 }))], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) });
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.1) });

    let position = *s.positions.keys().next().expect("one open position");
    // The bag was already cleared off-chain (manual wallet sell) → book the position
    // closed at the resolved fill in ONE step, with NO sell (the twin of ManualClose,
    // minus the on-chain sell that would only revert into an empty wallet).
    let fx = reduce(&mut s, Event::ExternallyCleared { position, fill: fill(1.5, 1.0) });
    assert!(sells(&fx).is_empty(), "externally-cleared close must NOT submit a sell");
    assert_eq!(statuses(&fx), vec![PositionStatus::End]);
    // Position pruned + open counter decremented → the token is done.
    assert!(!s.positions.contains_key(&position), "closed position is removed from state");
}

#[test]
fn unconfirmed_sell_is_terminal_and_never_resold() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(&mut s, reload(vec![rule(1, 1, json!({ "take_profit": 100 }))], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) });
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.1) });
    let position = *s.positions.keys().next().unwrap();
    let fx = reduce(&mut s, Event::ManualClose { position });
    let exit = sell_intent(&fx);

    // The sell may have cleared and the feed never confirmed → alarm, never re-sell.
    let fx = reduce(&mut s, Event::FillFailed { intent: exit, reason: FillFailReason::Unconfirmed });
    assert!(sells(&fx).is_empty(), "unconfirmed sell must NOT re-submit");
    assert_eq!(statuses(&fx), vec![PositionStatus::ExitUnconfirmed]);
}

#[test]
fn dead_outranks_stop_loss_on_open_position() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(&mut s, reload(vec![rule(1, 1, json!({ "stop_loss": 30 }))], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) });
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.1) });
    // A meaningful trade at t=1 depletes reserves but holds price above the SL floor,
    // so it does not exit and it seeds the quiet clock.
    reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.0, 5.0, 1.0) });
    // At t=305 a *dust* trade crashes the price below the SL floor. Dust does NOT reset
    // the quiet clock, so the token is simultaneously dead AND below stop-loss in one
    // evaluation → the `Dead > StopLoss` priority makes Dead win.
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(0.001, 0.5, 5.0, 305.0) });
    assert_eq!(sells(&fx).iter().map(|(_, r)| *r).collect::<Vec<_>>(), vec![ExitReason::Dead]);
}

#[test]
fn first_slot_fingerprint_arms_only_after_settlement() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    // A fingerprint whose identity is a first-slot buy of ~2 SOL (deferred axis).
    let mut fp = cu_fp(1);
    fp.cu_limit = None;
    fp.first_slot_buy_lamports = Some(2_000_000_000);
    reduce(&mut s, reload(vec![rule(1, 1, json!({ "take_profit": 100 }))], vec![fp]));

    // At creation the first-slot axis is unknown → pending, no buy yet.
    let fx = reduce(
        &mut s,
        Event::TokenCreated { mint: m.clone(), fp: Box::new(TokenFingerprint::default()), at: ts(0.0) },
    );
    assert!(buys(&fx).is_empty(), "first-slot fingerprint stays pending at creation");

    // Slot settles in-bucket (2.05 SOL ∈ [2.0, 2.1)) → armed → enter-on-arm buys.
    let fx = reduce(
        &mut s,
        Event::FirstSlotSettled {
            mint: m.clone(),
            buy_lamports: 2_050_000_000,
            sell_lamports: 0,
            at: ts(1.0),
        },
    );
    assert_eq!(buys(&fx), vec![(rid(1), BUY)], "arms + enters once settled in-bucket");
}

#[test]
fn first_slot_mismatch_drops_the_arm() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    let mut fp = cu_fp(1);
    fp.cu_limit = None;
    fp.first_slot_buy_lamports = Some(2_000_000_000);
    reduce(&mut s, reload(vec![rule(1, 1, json!({ "take_profit": 100 }))], vec![fp]));
    reduce(
        &mut s,
        Event::TokenCreated { mint: m.clone(), fp: Box::new(TokenFingerprint::default()), at: ts(0.0) },
    );
    // Settles far out of bucket (5 SOL) → never fully matched → dropped, no buy, pruned.
    let fx = reduce(
        &mut s,
        Event::FirstSlotSettled {
            mint: m.clone(),
            buy_lamports: 5_000_000_000,
            sell_lamports: 0,
            at: ts(1.0),
        },
    );
    assert!(buys(&fx).is_empty());
    assert!(!s.tokens.contains_key(&m), "no active arm → token pruned");
}

#[test]
fn untracked_token_events_are_ignored() {
    // A trade/tick/manual-close for a token that never matched must be a no-op.
    let mut s = EngineState::new();
    reduce(&mut s, reload(vec![rule(1, 1, json!({ "take_profit": 100 }))], vec![cu_fp(1)]));
    let fx = reduce(
        &mut s,
        Event::Trade { mint: Mint::from("ghost"), trade: trade(1.0, 1.0, 40.0, 1.0) },
    );
    assert!(fx.is_empty());
    let fx = reduce(&mut s, Event::ManualClose { position: PositionId(999) });
    assert!(fx.is_empty());
}

#[test]
fn non_matching_token_is_never_tracked() {
    let mut s = EngineState::new();
    reduce(&mut s, reload(vec![rule(1, 1, json!({ "take_profit": 100 }))], vec![cu_fp(1)]));
    // cu_limit differs → no fingerprint match → not armed, not tracked.
    let tf = Box::new(TokenFingerprint { cu_limit: Some(999), ..Default::default() });
    let fx = reduce(&mut s, Event::TokenCreated { mint: Mint::from("nope"), fp: tf, at: ts(0.0) });
    assert!(fx.is_empty());
    assert!(s.tokens.is_empty());
}

#[test]
fn identical_event_vectors_yield_identical_effects() {
    // Determinism: the same script twice produces byte-identical effect vectors.
    fn run() -> Vec<String> {
        let mut s = EngineState::new();
        let m = Mint::from("tokA");
        let mut out = Vec::new();
        let script = vec![
            reload(vec![rule(1, 1, json!({ "take_profit": 100 }))], vec![cu_fp(1)]),
            Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) },
        ];
        for e in script {
            let fx = reduce(&mut s, e);
            out.push(format!("{fx:?}"));
        }
        // Feed the fill + a TP trade using the minted intent.
        let entry = {
            let mut s2 = EngineState::new();
            reduce(&mut s2, reload(vec![rule(1, 1, json!({ "take_profit": 100 }))], vec![cu_fp(1)]));
            let fx =
                reduce(&mut s2, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) });
            buy_intent(&fx)
        };
        out.push(format!("{:?}", reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.5) })));
        out.push(format!(
            "{:?}",
            reduce(&mut s, Event::Trade { mint: m, trade: trade(1.0, 2.0, 40.0, 1.0) })
        ));
        out
    }
    assert_eq!(run(), run());
}

/// A `CompiledRule` sanity check that the public compile entry point is reachable
/// (the adapters compile rules through `RulesReloaded`, but the type is public for
/// the sweep scan too).
#[test]
fn compiled_rule_is_public() {
    let c = CompiledRule::compile(&rule(1, 1, json!({ "take_profit": 100 })));
    assert!(c.enter_on_arm());
}
