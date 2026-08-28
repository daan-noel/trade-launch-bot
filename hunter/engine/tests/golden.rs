//! Golden-log tests — the engine's executable spec (plan §3.5). Each test scripts
//! an event vector and asserts on the exact decisions the fold emits. They drive
//! the crate's *public* surface (`EngineState` + `reduce`) — the same surface the
//! live / replay / sweep adapters use — so a regression here is a parity break.

use std::sync::Arc;

use chrono::{Duration, TimeZone, Utc};
use hunter_engine::arm::CompiledRule;
use hunter_engine::event::{
    ArmedStateTag, DisarmReason, Effect, Event, ExitReason, Fill, FillFailReason, LoadedRule, Mint,
    Portion, PositionId, PositionStatus, RuleId, TradeMode,
};
use hunter_engine::fingerprint::{AxisId, AxisPredicate, Criteria, Fingerprint, FingerprintId};
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
        wildcard: false,
        criteria: Criteria::new()
            .with(AxisId::CuLimit, AxisPredicate::exact(200_000)),
        metric_config: serde_json::json!({}),
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
        entry_enabled: true,
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
    TradeLite {
        slot: 0,
        marker_bits: 0,
        side: Side::Buy,
        sol,
        price,
        reserve_sol: reserve,
        at: ts(at),
        ..Default::default()
    }
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

fn sell_portions(fx: &[Effect]) -> Vec<Portion> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::SubmitSell { portion, .. } => Some(*portion),
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

fn stages(fx: &[Effect]) -> Vec<Option<u8>> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::PositionUpdate(d) => Some(d.stage),
            _ => None,
        })
        .collect()
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
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) , creator_wallet_hash: None, identity: None});
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
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) , creator_wallet_hash: None, identity: None});
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.5) });
    // −40% (below the −30% floor) → stop-loss.
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 0.6, 40.0, 1.0) });
    assert_eq!(sells(&fx).iter().map(|(_, r)| *r).collect::<Vec<_>>(), vec![ExitReason::StopLoss]);
}

/// The restart regression (2026-08-06, `247PRAda…`): a warm start replays a
/// token's cached history, and a stop-loss that was true minutes ago must not sell
/// at today's price. `prime_trade` folds the observation without the decision.
#[test]
fn primed_history_never_fires_an_exit() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(&mut s, reload(vec![rule(1, 1, json!({ "stop_loss": 30 }))], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0), creator_wallet_hash: None, identity: None });
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.5) });

    // The same −40% print that `stop_loss_fires_below_threshold` sells on — but
    // primed as history, so it may only shape the track.
    hunter_engine::prime_trade(&mut s, &m, trade(1.0, 0.6, 40.0, 1.0));
    // ... and the tick that follows decides on the CURRENT price, which is 0.6.
    // (Live, that price is minutes stale; here it proves priming defers rather
    // than swallows the decision.)
    let fx = reduce(&mut s, Event::Tick { now: ts(1.5) });
    assert_eq!(
        sells(&fx).iter().map(|(_, r)| *r).collect::<Vec<_>>(),
        vec![ExitReason::StopLoss],
        "priming defers the decision to the tick — it must not lose it"
    );
}

/// Priming rebuilds what a restart drops: the since-entry peak an adopted position
/// re-seeds to its entry price. Without it a trailing stop silently re-anchors and
/// a bag can round-trip its whole run-up.
#[test]
fn primed_history_restores_the_trailing_peak() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    // 20% trailing stop, armed only once 5% in profit.
    let params = json!({
        "exit": { "m_position": {
            "retrace": [{ "operator": ">=", "value": 20 }],
            "arm_above_pct": 5.0,
        } }
    });
    reduce(&mut s, reload(vec![rule(1, 1, params)], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0), creator_wallet_hash: None, identity: None });
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.5) });

    // History: the price doubled (peak 2.0) and eased to 1.9 — a 5% retrace, no exit.
    hunter_engine::prime_trade(&mut s, &m, trade(1.0, 2.0, 40.0, 1.0));
    hunter_engine::prime_trade(&mut s, &m, trade(1.0, 1.9, 40.0, 2.0));
    assert!(sells(&reduce(&mut s, Event::Tick { now: ts(2.5) })).is_empty());

    // Live print 25% off the *primed* peak ⇒ the trail fires. Anchored on entry
    // instead (peak 1.0), 1.5 would read as +50% and never exit.
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.5, 40.0, 3.0) });
    assert_eq!(sells(&fx).len(), 1, "the trail must measure from the primed peak");
}

/// The mirror of [`primed_history_restores_the_trailing_peak`]: priming must
/// restore the peak the position *held*, never one from before it entered.
///
/// A restart replays the token-cache seed, which reaches back hours — far past the
/// fill of an adopted position. `peak`/`trough` are position-scoped, so a dip-entry
/// bag must not inherit the run-up it deliberately did not buy; otherwise it wakes
/// up already deep in `retrace` and stops out on a high it never held.
#[test]
fn primed_history_before_entry_never_inflates_the_peak() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    let params = json!({
        "exit": { "m_position": { "retrace": [{ "operator": ">=", "value": 20 }] } }
    });
    reduce(&mut s, reload(vec![rule(1, 1, params)], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0), creator_wallet_hash: None, identity: None });
    let entry = buy_intent(&fx);
    // The dip entry: filled at 1.0, at t=10.
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 10.0) });

    // Seed replay hands back the pre-entry run-up to 3.0 — the position never held
    // it. Folding it as the peak would read the flat 1.0 price as a 67% retrace.
    hunter_engine::prime_trade(&mut s, &m, trade(1.0, 3.0, 40.0, 1.0));
    hunter_engine::prime_trade(&mut s, &m, trade(1.0, 1.0, 40.0, 2.0));

    let fx = reduce(&mut s, Event::Tick { now: ts(11.0) });
    assert!(sells(&fx).is_empty(), "a pre-entry high must not arm the trail");

    // Post-entry the peak tracks normally: up to 2.0, then −25% ⇒ the trail fires.
    reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 2.0, 40.0, 12.0) });
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.5, 40.0, 13.0) });
    assert_eq!(sells(&fx).len(), 1, "post-entry extremes still ratchet");
}

#[test]
fn metrics_exit_on_time_condition() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    let params = json!({ "exit": { "m_state": { "time": [{ "operator": ">", "value": 5 }] } } });
    reduce(&mut s, reload(vec![rule(1, 1, params)], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) , creator_wallet_hash: None, identity: None});
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.1) });
    // No exit before the deadline.
    let fx = reduce(&mut s, Event::Tick { now: ts(4.0) });
    assert!(sells(&fx).is_empty());
    // Past +5 s → metrics exit (tick-driven).
    let fx = reduce(&mut s, Event::Tick { now: ts(6.0) });
    assert_eq!(
        sells(&fx).iter().map(|(_, r)| *r).collect::<Vec<_>>(),
        vec![ExitReason::Metrics {
            metric: hunter_engine::metrics::MetricId::Time,
            operator: hunter_engine::metrics::evaluator::Operator::Gt,
            value: 5.0,
            window: None,
        }]
    );
}

#[test]
fn price_window_dip_trigger_enters_on_rolling_drawdown() {
    // The dip-reversion entry: buy once price sits >=12% below the 30 s rolling high.
    // Proves the new m_price_window group drives a real entry through the whole fold
    // (registry walk → compile → ensure_price_window → value routing → decide_arm).
    let mut s = EngineState::new();
    let m = Mint::from("tokDip");
    let params = json!({
        "entry": { "m_price_window": {
            "window_size_sec": 30,
            "trail": [{ "operator": ">=", "value": 12 }]
        } },
        "take_profit": 100
    });
    reduce(&mut s, reload(vec![rule(1, 1, params)], vec![cu_fp(1)]));
    // At creation the window is empty → trail NaN → no entry.
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0), creator_wallet_hash: None, identity: None });
    assert!(buys(&fx).is_empty(), "empty window: NaN trail never enters");
    // Establish the rolling high at price 1.0 — trail 0, still no entry.
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.0, 40.0, 1.0) });
    assert!(buys(&fx).is_empty(), "at the high: trail 0 < 12");
    // A dip to 0.85 → 15% below the 1.0 high → entry fires.
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 0.85, 40.0, 2.0) });
    assert_eq!(buys(&fx), vec![(rid(1), BUY)], "15% dip below the rolling high enters");
}

#[test]
fn position_retrace_is_a_trailing_stop_off_the_since_entry_peak() {
    // The dip-reversion exit: a 3% trailing stop. Enter, let price run +30%, then a
    // pullback of >3% off the new peak fires `m_position.retrace >= 3`.
    let mut s = EngineState::new();
    let m = Mint::from("tokTrail");
    let params = json!({ "exit": { "m_position": { "retrace": [{ "operator": ">=", "value": 3 }] } } });
    reduce(&mut s, reload(vec![rule(1, 1, params)], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0), creator_wallet_hash: None, identity: None });
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.1) });
    // Run to +30% (peak 1.3): retrace 0 → hold.
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.3, 40.0, 1.0) });
    assert!(sells(&fx).is_empty(), "at the peak: retrace 0 < 3");
    // A small dip to 1.25 → (1.3 − 1.25)/1.3 = 3.85% off the peak → trailing stop.
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.25, 40.0, 2.0) });
    assert_eq!(
        sells(&fx).iter().map(|(_, r)| *r).collect::<Vec<_>>(),
        vec![ExitReason::Metrics {
            metric: hunter_engine::metrics::MetricId::Retrace,
            operator: hunter_engine::metrics::evaluator::Operator::Gte,
            value: 3.0,
            window: None,
        }]
    );
}

#[test]
fn position_held_is_a_time_stop() {
    // `m_position.held >= 5` — a time-stop, tick-driven (no price needed).
    let mut s = EngineState::new();
    let m = Mint::from("tokHold");
    let params = json!({ "exit": { "m_position": { "held": [{ "operator": ">=", "value": 5 }] } } });
    reduce(&mut s, reload(vec![rule(1, 1, params)], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0), creator_wallet_hash: None, identity: None });
    let entry = buy_intent(&fx);
    // Entry fills at t=0.5 → held counts from there.
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.5) });
    let fx = reduce(&mut s, Event::Tick { now: ts(4.0) });
    assert!(sells(&fx).is_empty(), "held 3.5 s < 5");
    let fx = reduce(&mut s, Event::Tick { now: ts(6.0) });
    assert_eq!(
        sells(&fx).iter().map(|(_, r)| *r).collect::<Vec<_>>(),
        vec![ExitReason::Metrics {
            metric: hunter_engine::metrics::MetricId::Held,
            operator: hunter_engine::metrics::evaluator::Operator::Gte,
            value: 5.0,
            window: None,
        }]
    );
}

#[test]
fn desugared_stop_loss_still_labels_as_stoploss_and_outranks_a_metric_exit() {
    // TP/SL desugar into m_position.pnl but keep their labels, and the prepend order
    // preserves the old SL-before-Metrics priority. Here a −40% move trips BOTH the
    // −30% stop (desugared pnl) and a retrace metric; the stop must win and read
    // `StopLoss`, not `pnl <= -30`.
    let mut s = EngineState::new();
    let m = Mint::from("tokSL");
    let params = json!({
        "stop_loss": 30,
        "exit": { "m_position": { "retrace": [{ "operator": ">=", "value": 1 }] } }
    });
    reduce(&mut s, reload(vec![rule(1, 1, params)], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0), creator_wallet_hash: None, identity: None });
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.1) });
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 0.6, 40.0, 1.0) });
    assert_eq!(
        sells(&fx).iter().map(|(_, r)| *r).collect::<Vec<_>>(),
        vec![ExitReason::StopLoss],
        "desugared SL keeps its label and outranks the retrace metric",
    );
}

#[test]
fn overlapping_entry_exit_metrics_do_not_enter() {
    // Entry liquidity > 50 and exit liquidity > 40 both hold at reserve 60 —
    // can_enter must refuse (would otherwise buy then metrics-exit next event).
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    let params = json!({
        "entry": { "m_state": { "liquidity": [{ "operator": ">", "value": 50 }] } },
        "exit":  { "m_state": { "liquidity": [{ "operator": ">", "value": 40 }] } },
        "take_profit": 100
    });
    reduce(&mut s, reload(vec![rule(1, 1, params)], vec![cu_fp(1)]));
    reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0), creator_wallet_hash: None, identity: None });
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.0, 60.0, 1.0) });
    assert!(buys(&fx).is_empty(), "must not enter while exit metrics already hold");
}

#[test]
fn enters_once_exit_metrics_clear_while_entry_still_holds() {
    // Entry liquidity > 10, exit liquidity > 40. At reserve 60 both hold → no
    // buy; at reserve 30 entry still holds and exit clears → buy.
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    let params = json!({
        "entry": { "m_state": { "liquidity": [{ "operator": ">", "value": 10 }] } },
        "exit":  { "m_state": { "liquidity": [{ "operator": ">", "value": 40 }] } },
        "take_profit": 100
    });
    reduce(&mut s, reload(vec![rule(1, 1, params)], vec![cu_fp(1)]));
    reduce(
        &mut s,
        Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0), creator_wallet_hash: None, identity: None },
    );
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.0, 60.0, 1.0) });
    assert!(buys(&fx).is_empty(), "overlap: entry and exit both true");
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.0, 30.0, 2.0) });
    assert_eq!(buys(&fx), vec![(rid(1), BUY)], "entry holds, exit cleared → buy");
}

#[test]
fn stall_exit_on_quiet_token_is_tick_driven() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    let params = json!({ "exit": { "m_price_lifetime": { "stall": [{ "operator": ">", "value": 3 }] } } });
    reduce(&mut s, reload(vec![rule(1, 1, params)], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) , creator_wallet_hash: None, identity: None});
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.0) });
    // A price move at t=1 resets the stall clock; no exit here.
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.2, 40.0, 1.0) });
    assert!(sells(&fx).is_empty());
    // Quiet until t=5 → stall = 4 s > 3 → exit on the tick alone.
    let fx = reduce(&mut s, Event::Tick { now: ts(5.0) });
    assert_eq!(
        sells(&fx).iter().map(|(_, r)| *r).collect::<Vec<_>>(),
        vec![ExitReason::Metrics {
            metric: hunter_engine::metrics::MetricId::Stall,
            operator: hunter_engine::metrics::evaluator::Operator::Gt,
            value: 3.0,
            window: None,
        }]
    );
}

#[test]
fn derived_unsatisfiable_disarms_before_entry() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    // Enter only if within 3 s AND liquidity > 100 (never true) → never enters, but the
    // `time < 3` upper bound lets the arm disarm itself once the clock passes 3 s.
    let params = json!({ "entry": { "m_state": {
        "time": [{ "operator": "<", "value": 3 }],
        "liquidity": [{ "operator": ">", "value": 100 }]
    } } });
    reduce(&mut s, reload(vec![rule(1, 1, params)], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) , creator_wallet_hash: None, identity: None});
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
    let params = json!({ "entry": { "m_state": { "liquidity": [{ "operator": ">", "value": 100 }] } } });
    reduce(&mut s, reload(vec![rule(1, 1, params)], vec![cu_fp(1)]));
    reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) , creator_wallet_hash: None, identity: None});
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
    let params = json!({ "entry": { "m_state": { "liquidity": [{ "operator": ">", "value": 100 }] } } });
    reduce(&mut s, reload(vec![rule(1, 1, params)], vec![cu_fp(1)]));
    reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) , creator_wallet_hash: None, identity: None});
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
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) , creator_wallet_hash: None, identity: None});
    let mut fired: Vec<RuleId> = buys(&fx).into_iter().map(|(r, _)| r).collect();
    fired.sort();
    assert_eq!(fired, vec![rid(1), rid(2)]);
    assert_eq!(s.positions.len(), 2, "two independent positions");
}

#[test]
fn exclusive_rules_contest_a_token_and_the_loser_stays_armed() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    // Both exclusive, both enter-on-arm, equal (default) priority → rule-id order
    // decides: rule 1 claims the token, rule 2 sees the claim and stands down.
    let rules = vec![
        rule(1, 1, json!({ "take_profit": 100, "exclusive": true })),
        rule(2, 1, json!({ "take_profit": 200, "exclusive": true })),
    ];
    reduce(&mut s, reload(rules, vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0), creator_wallet_hash: None, identity: None });
    assert_eq!(buys(&fx), vec![(rid(1), BUY)], "only the first exclusive rule enters");
    assert_eq!(s.positions.len(), 1);
    // Standing down is NOT a disarm — the loser must still be able to enter later.
    assert!(disarms(&fx).is_empty());
}

#[test]
fn higher_priority_wins_the_exclusive_claim_regardless_of_rule_id() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    // Rule 2 sorts last by id but carries the higher priority → it is visited first.
    let rules = vec![
        rule(1, 1, json!({ "take_profit": 100, "exclusive": true })),
        rule(2, 1, json!({ "take_profit": 200, "exclusive": true, "priority": 5 })),
    ];
    reduce(&mut s, reload(rules, vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0), creator_wallet_hash: None, identity: None });
    assert_eq!(buys(&fx), vec![(rid(2), BUY)], "priority beats rule-id order");
}

#[test]
fn exclusivity_is_asymmetric_and_clears_when_the_holder_exits() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    // Rule 1 is NON-exclusive (it never checks anyone); rule 2 is exclusive and is
    // blocked by rule 1's holding — the asymmetry is by design.
    let rules = vec![
        rule(1, 1, json!({ "take_profit": 100 })),
        rule(2, 1, json!({ "take_profit": 200, "exclusive": true })),
    ];
    reduce(&mut s, reload(rules, vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0), creator_wallet_hash: None, identity: None });
    assert_eq!(buys(&fx), vec![(rid(1), BUY)], "the non-exclusive rule holds; rule 2 waits");

    // Rule 1's buy fills, then TPs out at 2x and the sell confirms.
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.1) });
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 2.0, 40.0, 1.0) });
    assert!(buys(&fx).is_empty(), "an in-flight sell (ExitPending) still blocks");
    let exit = sell_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: exit, fill: fill(2.0, 1.1) });

    // Nobody holds the token now → the blocked exclusive rule enters on the next event.
    let fx = reduce(&mut s, Event::Tick { now: ts(2.0) });
    assert_eq!(buys(&fx), vec![(rid(2), BUY)], "enters once the holder lets go");
}

#[test]
fn a_manual_position_blocks_an_exclusive_rule() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    // Manual arms live in the same `token.arms` map under their own rule id, so
    // "ANY rule holds this token" covers them with no special-casing.
    reduce(
        &mut s,
        reload(vec![rule(1, 1, json!({ "take_profit": 100, "exclusive": true }))], vec![cu_fp(1)]),
    );
    let fx = reduce(
        &mut s,
        Event::ManualBuy { mint: m.clone(), rule: rid(9), lamports: BUY, at: ts(0.0), exit: None },
    );
    assert_eq!(buys(&fx), vec![(rid(9), BUY)]);
    let manual_entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: manual_entry, fill: fill(1.0, 0.1) });

    // The token now matches the fingerprint and arms rule 1 — but the manual hold blocks it.
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(1.0), creator_wallet_hash: None, identity: None });
    assert!(buys(&fx).is_empty(), "a manual holding blocks the exclusive rule");
    assert!(disarms(&fx).is_empty(), "blocked, not disarmed");
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
    let fx = reduce(&mut s, Event::TokenCreated { mint: a.clone(), fp: cu_token(), at: ts(0.0) , creator_wallet_hash: None, identity: None});
    let a_entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: a_entry, fill: fill(1.0, 0.1) });

    // Token B arrives while A holds → the cap blocks its entry.
    let fx = reduce(&mut s, Event::TokenCreated { mint: b.clone(), fp: cu_token(), at: ts(1.0) , creator_wallet_hash: None, identity: None});
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
    let fx = reduce(&mut s, Event::TokenCreated { mint: a.clone(), fp: cu_token(), at: ts(0.0) , creator_wallet_hash: None, identity: None});
    let a_entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: a_entry, fill: fill(1.0, 0.1) });
    let fx = reduce(&mut s, Event::Trade { mint: a.clone(), trade: trade(1.0, 2.0, 40.0, 1.0) });
    let a_exit = sell_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: a_exit, fill: fill(2.0, 1.1) });

    // Even with the slot free, the lifetime cap blocks B.
    let fx = reduce(&mut s, Event::TokenCreated { mint: b.clone(), fp: cu_token(), at: ts(2.0) , creator_wallet_hash: None, identity: None});
    assert!(buys(&fx).is_empty(), "lifetime cap of 1 reached — B never enters");
}

#[test]
fn entry_fill_failure_retries_then_gives_up() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(&mut s, reload(vec![rule(1, 1, json!({ "take_profit": 100 }))], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) , creator_wallet_hash: None, identity: None});
    let mut intent = buy_intent(&fx);

    // Two retries, each re-submitting a buy with a fresh intent.
    for _ in 0..2 {
        let fx =
            reduce(&mut s, Event::FillFailed { intent: intent.clone(), reason: FillFailReason::Reverted, at: None });
        assert_eq!(buys(&fx), vec![(rid(1), BUY)], "retry re-submits the buy");
        let next = buy_intent(&fx);
        assert_ne!(next, intent, "retry uses a fresh intent id");
        intent = next;
    }
    // Third failure → give up (no more buys), book a terminal failure.
    let fx = reduce(&mut s, Event::FillFailed { intent, reason: FillFailReason::Reverted, at: None });
    assert!(buys(&fx).is_empty(), "gives up after MAX_ENTRY_ATTEMPTS");
    assert_eq!(statuses(&fx), vec![PositionStatus::EntryFailed]);
    // Counters rolled back → the token is done, pruned.
    assert!(!s.tokens.contains_key(&m));
}

/// A retry is a NEW buy and must clear the SAME entry gate the first one did.
///
/// Replays the 2026-08-07 `XsPXZt…` incident in miniature: an
/// `entry liquidity > 10` rule enters at 14.65 SOL, the buy reverts (6042
/// slippage), and by the time the revert is confirmed the pool has been drained
/// to 0.276 SOL. Before the fix the retry re-submitted blind and filled 36x under
/// the rule's own floor.
#[test]
fn entry_retry_requalifies_against_entry_conditions() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(
        &mut s,
        reload(
            vec![rule(
                1,
                1,
                json!({
                    "entry": { "m_state": { "liquidity": [{"operator": ">", "value": 10}] } },
                    "take_profit": 100
                })),
            ],
            vec![cu_fp(1)],
        ),
    );
    reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0), creator_wallet_hash: None, identity: None });
    // Liquidity crosses the floor → entry fires.
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.0, 14.65, 1.0) });
    let intent = buy_intent(&fx);

    // While the buy is in flight the pool is drained well under the floor.
    reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 0.5, 0.276, 12.0) });

    // The revert lands 12s after the decision — the gate no longer holds.
    let fx = reduce(
        &mut s,
        Event::FillFailed {
            intent,
            reason: FillFailReason::Reverted,
            at: Some(ts(13.0)),
        },
    );
    assert!(buys(&fx).is_empty(), "retry must not buy into a drained pool");
    assert_eq!(statuses(&fx), vec![PositionStatus::EntryFailed]);

    // And the slot is released, not leaked: the counters rolled back with it.
    assert_eq!(s.counters.get(&rid(1)).map_or(0, |c| c.open), 0);

    // "Not qualified right now" is not "done with this token": the arm re-arms, so
    // the ONE gate re-decides. Liquidity recovers → it enters again, cleanly.
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.0, 20.0, 20.0) });
    assert_eq!(buys(&fx), vec![(rid(1), BUY)], "re-armed and re-entered once qualified");
}

/// An exhausted ladder is still terminal — re-arming must not turn the bounded
/// attempt count into an unbounded loop.
#[test]
fn entry_retry_exhausted_ladder_stays_terminal() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(&mut s, reload(vec![rule(1, 1, json!({ "take_profit": 100 }))], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0), creator_wallet_hash: None, identity: None });
    let mut intent = buy_intent(&fx);
    for _ in 0..2 {
        let fx = reduce(
            &mut s,
            Event::FillFailed { intent, reason: FillFailReason::Reverted, at: Some(ts(1.0)) },
        );
        intent = buy_intent(&fx);
    }
    let fx = reduce(
        &mut s,
        Event::FillFailed { intent, reason: FillFailReason::Reverted, at: Some(ts(1.0)) },
    );
    assert!(buys(&fx).is_empty(), "ladder exhausted");
    assert_eq!(statuses(&fx), vec![PositionStatus::EntryFailed]);
    // Terminal ⇒ the arm is Done and the token is pruned, not left re-armed.
    assert!(!s.tokens.contains_key(&m), "exhausted ladder must not re-arm");
}

/// The mirror case: a retryable failure while the entry conditions still hold
/// re-submits exactly as before. The gate must not turn every revert terminal.
#[test]
fn entry_retry_resubmits_while_conditions_still_hold() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(
        &mut s,
        reload(
            vec![rule(
                1,
                1,
                json!({
                    "entry": { "m_state": { "liquidity": [{"operator": ">", "value": 10}] } },
                    "take_profit": 100
                })),
            ],
            vec![cu_fp(1)],
        ),
    );
    reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0), creator_wallet_hash: None, identity: None });
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.0, 40.0, 1.0) });
    let intent = buy_intent(&fx);

    // Pool still deep at the retry instant.
    let fx = reduce(
        &mut s,
        Event::FillFailed { intent, reason: FillFailReason::Reverted, at: Some(ts(2.0)) },
    );
    assert_eq!(buys(&fx), vec![(rid(1), BUY)], "still qualified — retry re-submits");
}

/// Stopping a rule must also stop its in-flight retries. A rule with an open or
/// in-flight position stays loaded as a *drain* rule (so its positions can still
/// exit) with `entry_enabled = false` — and `can_enter` does not cover that flag,
/// so the retry gate has to check it explicitly.
#[test]
fn entry_retry_refuses_once_the_rule_is_stopped() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    let params = json!({
        "entry": { "m_state": { "liquidity": [{"operator": ">", "value": 10}] } },
        "take_profit": 100
    });
    reduce(&mut s, reload(vec![rule(1, 1, params.clone())], vec![cu_fp(1)]));
    reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0), creator_wallet_hash: None, identity: None });
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.0, 40.0, 1.0) });
    let intent = buy_intent(&fx);

    // Operator stops the rule while the buy is in flight: it stays loaded to drain,
    // entry disabled. The arm holds a position, so the reload preserves it.
    let mut stopped = rule(1, 1, params);
    stopped.entry_enabled = false;
    reduce(&mut s, reload(vec![stopped], vec![cu_fp(1)]));

    // Entry conditions still hold — only the stop switch differs.
    let fx = reduce(
        &mut s,
        Event::FillFailed { intent, reason: FillFailReason::Reverted, at: Some(ts(2.0)) },
    );
    assert!(buys(&fx).is_empty(), "a stopped rule must not retry its buy");
    assert_eq!(statuses(&fx), vec![PositionStatus::EntryFailed]);
}

#[test]
fn entry_fatal_gives_up_without_retry() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(&mut s, reload(vec![rule(1, 1, json!({ "take_profit": 100 }))], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) , creator_wallet_hash: None, identity: None});
    let intent = buy_intent(&fx);
    let fx = reduce(&mut s, Event::FillFailed { intent, reason: FillFailReason::Fatal, at: None });
    assert!(buys(&fx).is_empty(), "Fatal must not retry");
    assert_eq!(statuses(&fx), vec![PositionStatus::EntryFailed]);
    assert!(!s.tokens.contains_key(&m));
}

#[test]
fn manual_close_sells_held_position() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(&mut s, reload(vec![rule(1, 1, json!({ "take_profit": 100 }))], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) , creator_wallet_hash: None, identity: None});
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.1) });

    let position = *s.positions.keys().next().expect("one open position");
    let fx = reduce(&mut s, Event::ManualClose { position, portion: Portion::All });
    assert_eq!(sells(&fx).iter().map(|(_, r)| *r).collect::<Vec<_>>(), vec![ExitReason::Manual]);
    assert_eq!(sell_portions(&fx), vec![Portion::All]);
    let exit = sell_intent(&fx);
    let fx = reduce(&mut s, Event::FillConfirmed { intent: exit, fill: fill(1.5, 1.0) });
    assert_eq!(statuses(&fx), vec![PositionStatus::End]);
}

#[test]
fn externally_cleared_closes_without_sell() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(&mut s, reload(vec![rule(1, 1, json!({ "take_profit": 100 }))], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) , creator_wallet_hash: None, identity: None});
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
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) , creator_wallet_hash: None, identity: None});
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.1) });
    let position = *s.positions.keys().next().unwrap();
    let fx = reduce(&mut s, Event::ManualClose { position, portion: Portion::All });
    let exit = sell_intent(&fx);

    // The sell may have cleared and the feed never confirmed → alarm, never re-sell.
    let fx = reduce(&mut s, Event::FillFailed { intent: exit, reason: FillFailReason::Unconfirmed, at: None });
    assert!(sells(&fx).is_empty(), "unconfirmed sell must NOT re-submit");
    assert_eq!(statuses(&fx), vec![PositionStatus::ExitUnconfirmed]);
}

#[test]
fn dead_outranks_stop_loss_on_open_position() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(&mut s, reload(vec![rule(1, 1, json!({ "stop_loss": 30 }))], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) , creator_wallet_hash: None, identity: None});
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
    // A fingerprint whose identity is a first-slot buy in [2.0, 2.1) SOL (deferred
    // axis). The window is stated OUTRIGHT — the retired form pinned 2.0 and leaned on
    // a row-wide 0.1 bucket width to widen it, so what the rule matched was a fact
    // about the row's width rather than about this axis.
    let mut fp = cu_fp(1);
    fp.criteria.remove(AxisId::CuLimit);
    fp.criteria.insert(
        AxisId::FirstSlotBuyLamports,
        AxisPredicate::half_open(2_000_000_000, Some(2_100_000_000)),
    );
    reduce(&mut s, reload(vec![rule(1, 1, json!({ "take_profit": 100 }))], vec![fp]));

    // At creation the first-slot axis is unknown → pending, no buy yet.
    let fx = reduce(
        &mut s,
        Event::TokenCreated { mint: m.clone(), fp: Box::new(TokenFingerprint::default()), at: ts(0.0) , creator_wallet_hash: None, identity: None},
    );
    assert!(buys(&fx).is_empty(), "first-slot fingerprint stays pending at creation");

    // Slot settles inside the window (2.05 SOL) → armed → enter-on-arm buys.
    let fx = reduce(
        &mut s,
        Event::FirstSlotSettled {
            mint: m.clone(),
            buy_lamports: 2_050_000_000,
            sell_lamports: 0,
            at: ts(1.0),
        },
    );
    assert_eq!(buys(&fx), vec![(rid(1), BUY)], "arms + enters once settled in-window");
}

#[test]
fn first_slot_mismatch_drops_the_arm() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    let mut fp = cu_fp(1);
    fp.criteria.remove(AxisId::CuLimit);
    fp.criteria.insert(
        AxisId::FirstSlotBuyLamports,
        AxisPredicate::half_open(2_000_000_000, Some(2_100_000_000)),
    );
    reduce(&mut s, reload(vec![rule(1, 1, json!({ "take_profit": 100 }))], vec![fp]));
    reduce(
        &mut s,
        Event::TokenCreated { mint: m.clone(), fp: Box::new(TokenFingerprint::default()), at: ts(0.0) , creator_wallet_hash: None, identity: None},
    );
    // Settles far outside the window (5 SOL) → never fully matched → dropped, pruned.
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
    let fx = reduce(&mut s, Event::ManualClose { position: PositionId(999), portion: Portion::All });
    assert!(fx.is_empty());
}

#[test]
fn non_matching_token_is_never_tracked() {
    let mut s = EngineState::new();
    reduce(&mut s, reload(vec![rule(1, 1, json!({ "take_profit": 100 }))], vec![cu_fp(1)]));
    // cu_limit differs → no fingerprint match → not armed, not tracked.
    let tf = Box::new(TokenFingerprint { cu_limit: Some(999), ..Default::default() });
    let fx = reduce(&mut s, Event::TokenCreated { mint: Mint::from("nope"), fp: tf, at: ts(0.0) , creator_wallet_hash: None, identity: None});
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
            Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) , creator_wallet_hash: None, identity: None},
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
                reduce(&mut s2, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0) , creator_wallet_hash: None, identity: None});
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

#[test]
fn flow_entry_on_tagged_net_and_exit_when_organic_goes_quiet() {
    use hunter_engine::metrics::flow_ix::ix_hash;

    let mut fp = cu_fp(1);
    fp.metric_config = json!({
        "m_flow_ix": { "ix_patterns": [["vol"]] }
    });
    let params = json!({
        "entry": { "m_flow_ix": { "tagged_net": [{"operator": ">", "value": 2}] } },
        "exit": {
            "m_flow_ix_window": {
                "window_size_sec": 5,
                "untagged_gross": [{"operator": "=", "value": 0}]
            }
        }
    });
    let mut s = EngineState::new();
    let m = Mint::from("tokFlow");
    reduce(&mut s, reload(vec![rule(1, 1, params)], vec![fp]));
    reduce(
        &mut s,
        Event::TokenCreated {
            mint: m.clone(),
            fp: cu_token(),
            at: ts(0.0),
            creator_wallet_hash: Some(99), identity: None,
        },
    );
    // Organic buy first — not enough tagged_net to enter.
    let fx = reduce(
        &mut s,
        Event::Trade {
            mint: m.clone(),
            trade: TradeLite {
               slot: 0,
               marker_bits: 0,
                side: Side::Buy,
                sol: 1.0,
                price: 1.0,
                reserve_sol: 20.0,
                priced_reserve_sol: 20.0,
                at: ts(1.0),
                ix_hash: None,
                wallet_hash: 7,
            },
        },
    );
    assert!(buys(&fx).is_empty());

    // Volume-side buy → tagged_net=3 → entry.
    let fx = reduce(
        &mut s,
        Event::Trade {
            mint: m.clone(),
            trade: TradeLite {
               slot: 0,
               marker_bits: 0,
                side: Side::Buy,
                sol: 3.0,
                price: 1.1,
                reserve_sol: 23.0,
                priced_reserve_sol: 23.0,
                at: ts(2.0),
                ix_hash: Some(ix_hash(&["vol"])),
                wallet_hash: 8,
            },
        },
    );
    assert_eq!(buys(&fx), vec![(rid(1), BUY)]);
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.1, 2.0) });

    // Tick past the organic trade's window → untagged_gross(5s)=0 → exit.
    let fx = reduce(&mut s, Event::Tick { now: ts(7.0) });
    assert_eq!(
        sells(&fx).iter().map(|(_, r)| *r).collect::<Vec<_>>(),
        vec![ExitReason::Metrics {
            metric: hunter_engine::metrics::MetricId::WinUntaggedGross,
            operator: hunter_engine::metrics::evaluator::Operator::Eq,
            value: 0.0,
            window: Some(hunter_engine::metrics::WindowSpec::secs(5.0)),
        }]
    );
}

#[test]
fn two_fingerprints_flow_states_diverge() {
    use hunter_engine::metrics::flow_ix::ix_hash;

    let mut fp_a = cu_fp(1);
    fp_a.metric_config = json!({
        "m_flow_ix": { "ix_patterns": [["a"]] }
    });
    let mut fp_b = cu_fp(2);
    fp_b.criteria.insert(AxisId::CuLimit, AxisPredicate::exact(200_000)); // same match
    fp_b.metric_config = json!({
        "m_flow_ix": { "ix_patterns": [["b"]] }
    });
    // Rule A enters on tagged_buy>0 for pattern A; rule B would need pattern B.
    let params_a = json!({
        "entry": { "m_flow_ix": { "tagged_buy": [{"operator": ">", "value": 0}] } }
    });
    let params_b = json!({
        "entry": { "m_flow_ix": { "tagged_buy": [{"operator": ">", "value": 0}] } }
    });
    let mut s = EngineState::new();
    let m = Mint::from("tokMulti");
    reduce(
        &mut s,
        reload(
            vec![rule(1, 1, params_a), rule(2, 2, params_b)],
            vec![fp_a, fp_b],
        ),
    );
    reduce(
        &mut s,
        Event::TokenCreated {
            mint: m.clone(),
            fp: cu_token(),
            at: ts(0.0),
            creator_wallet_hash: None, identity: None,
        },
    );
    let fx = reduce(
        &mut s,
        Event::Trade {
            mint: m,
            trade: TradeLite {
               slot: 0,
               marker_bits: 0,
                side: Side::Buy,
                sol: 2.0,
                price: 1.0,
                reserve_sol: 20.0,
                priced_reserve_sol: 20.0,
                at: ts(1.0),
                ix_hash: Some(ix_hash(&["a"])),
                wallet_hash: 1,
            },
        },
    );
    // Only rule 1 (pattern A) enters; rule 2 still sees organic.
    assert_eq!(buys(&fx), vec![(rid(1), BUY)]);
}

// ── Re-entry lifecycle (plan Ph4) ─────────────────────────────────────────────
//
// One-shot behavior is the golden non-regression: every scenario above runs rules
// WITHOUT `reentry` and already asserts `Done` is terminal (e.g.
// `arm_enter_then_take_profit` asserts the token is pruned after `End`).

/// An enter-on-arm TP rule with re-entry configured — the fastest full-cycle rig.
fn reentry_rule(cooldown_sec: f64, max_episodes: u32) -> LoadedRule {
    rule_capped(
        1,
        1,
        json!({
            "take_profit": 100,
            "reentry": { "cooldown_sec": cooldown_sec, "max_episodes_per_token": max_episodes }
        }),
        1,
        0,
    )
}

/// Run one full episode: enter (already armed / enter-on-arm), fill, TP, exit fill.
/// Returns the close time used for the exit fill.
fn run_episode(s: &mut EngineState, m: &Mint, t0: f64) -> f64 {
    let fx = reduce(s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.0, 40.0, t0) });
    let entry = buy_intent(&fx);
    reduce(s, Event::FillConfirmed { intent: entry, fill: fill(1.0, t0 + 0.1) });
    let fx = reduce(s, Event::Trade { mint: m.clone(), trade: trade(1.0, 2.0, 40.0, t0 + 0.2) });
    let exit = sell_intent(&fx);
    let close_at = t0 + 0.3;
    let fx = reduce(s, Event::FillConfirmed { intent: exit, fill: fill(2.0, close_at) });
    assert_eq!(statuses(&fx), vec![PositionStatus::End]);
    close_at
}

#[test]
fn reentry_rearm_after_cooldown_and_reenter() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(&mut s, reload(vec![reentry_rule(5.0, 3)], vec![cu_fp(1)]));

    // Episode 1: creation enters immediately (enter-on-arm), runs to a TP close.
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0), creator_wallet_hash: None, identity: None });
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.1) });
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 2.0, 40.0, 1.0) });
    let exit = sell_intent(&fx);
    let fx = reduce(&mut s, Event::FillConfirmed { intent: exit, fill: fill(2.0, 1.1) });
    assert_eq!(statuses(&fx), vec![PositionStatus::End]);

    // NOT pruned (one-shot would drop the token here): the arm is cooling down.
    let token = s.tokens.get(&m).expect("token stays tracked through cooldown");
    assert!(
        matches!(token.arms.get(&rid(1)), Some(hunter_engine::arm::ArmState::Cooldown { .. })),
        "closed episode re-arms into Cooldown"
    );
    assert_eq!(token.episodes.get(&rid(1)), Some(&1), "episode counted at close");

    // Inside the cooldown window (until = 1.1 + 5 = 6.1): no promotion, no buy.
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.0, 40.0, 5.0) });
    assert!(buys(&fx).is_empty(), "no re-entry inside the cooldown window");

    // Past the window: the same event promotes Cooldown → Armed AND re-enters
    // (enter-on-arm), so the effects carry the re-arm notice then the buy.
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.0, 40.0, 7.0) });
    assert!(
        fx.iter().any(|e| matches!(
            e,
            Effect::ArmedChanged(d) if d.state == ArmedStateTag::Armed && d.rule == rid(1)
        )),
        "promotion emits ArmedChanged(Armed)"
    );
    assert_eq!(buys(&fx), vec![(rid(1), BUY)], "episode 2 entry fires after cooldown");
    // Lifetime counter counts EPISODES: two committed entries so far.
    assert_eq!(s.counters.get(&rid(1)).map(|c| c.total), Some(2));
}

#[test]
fn reentry_tick_promotes_cooldown() {
    // Promotion is trade/tick-driven — a quiet token re-arms on the clock tick.
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(&mut s, reload(vec![reentry_rule(5.0, 3)], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0), creator_wallet_hash: None, identity: None });
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.1) });
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 2.0, 40.0, 1.0) });
    let exit = sell_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: exit, fill: fill(2.0, 1.1) });

    // A tick before the deadline leaves it cooling; one after promotes (and, for an
    // enter-on-arm rule, immediately re-enters).
    let fx = reduce(&mut s, Event::Tick { now: ts(6.0) });
    assert!(buys(&fx).is_empty());
    let fx = reduce(&mut s, Event::Tick { now: ts(6.2) });
    assert_eq!(buys(&fx), vec![(rid(1), BUY)], "tick past the window re-arms + re-enters");
}

#[test]
fn reentry_episode_cap_stops_rearm() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(&mut s, reload(vec![reentry_rule(0.0, 2)], vec![cu_fp(1)]));

    // Episode 1 (from creation).
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0), creator_wallet_hash: None, identity: None });
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.1) });
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 2.0, 40.0, 1.0) });
    let exit = sell_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: exit, fill: fill(2.0, 1.1) });
    assert!(s.tokens.contains_key(&m), "episode 1 of 2 re-arms");

    // Episode 2 — the cap: its close must land Done and prune the token.
    run_episode(&mut s, &m, 2.0);
    assert!(
        !s.tokens.contains_key(&m),
        "episode cap reached — the close is terminal and the token prunes"
    );
}

#[test]
fn reentry_manual_close_never_rearms() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(&mut s, reload(vec![reentry_rule(0.0, 10)], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0), creator_wallet_hash: None, identity: None });
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.1) });

    // A human closed it — the bot must not re-buy the token they just exited.
    let position = *s.positions.keys().next().expect("one open position");
    let fx = reduce(&mut s, Event::ManualClose { position, portion: Portion::All });
    let exit = sell_intent(&fx);
    let fx = reduce(&mut s, Event::FillConfirmed { intent: exit, fill: fill(1.5, 1.0) });
    assert_eq!(statuses(&fx), vec![PositionStatus::End]);
    assert!(!s.tokens.contains_key(&m), "Manual close is terminal even with reentry");
}

#[test]
fn reentry_dead_exit_never_rearms() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    // Needs a non-enter-on-arm shape? No — enter-on-arm is fine; kill it via deadness.
    reduce(&mut s, reload(vec![reentry_rule(0.0, 10)], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0), creator_wallet_hash: None, identity: None });
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.1) });
    // Deplete reserves at t=1, then a long-quiet dust print → Dead exit (the
    // `dead_outranks_stop_loss_on_open_position` trigger).
    reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.0, 5.0, 1.0) });
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(0.001, 0.5, 5.0, 305.0) });
    assert_eq!(sells(&fx).iter().map(|(_, r)| *r).collect::<Vec<_>>(), vec![ExitReason::Dead]);
    let exit = sell_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: exit, fill: fill(0.5, 305.1) });
    assert!(!s.tokens.contains_key(&m), "a dead token never re-arms");
}

#[test]
fn reentry_cooldown_disarms_on_migration() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(&mut s, reload(vec![reentry_rule(60.0, 10)], vec![cu_fp(1)]));
    let fx = reduce(&mut s, Event::TokenCreated { mint: m.clone(), fp: cu_token(), at: ts(0.0), creator_wallet_hash: None, identity: None });
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.1) });
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 2.0, 40.0, 1.0) });
    let exit = sell_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: exit, fill: fill(2.0, 1.1) });
    assert!(s.tokens.contains_key(&m), "cooling down");

    // Migration while cooling: there is no curve left to re-enter — disarm + prune.
    let fx = reduce(&mut s, Event::Migrated { mint: m.clone(), at: ts(2.0) });
    assert_eq!(disarms(&fx), vec![DisarmReason::Migrated]);
    assert!(!s.tokens.contains_key(&m));
}

// ── Manual episodes (Console manual buys, plan P2) ───────────────────────────

#[test]
fn manual_buy_tracked_only_never_auto_exits() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    // No rules loaded at all — a manual buy needs none.
    let fx = reduce(&mut s, Event::ManualBuy {
        mint: m.clone(), rule: rid(9), lamports: 500, at: ts(0.0), exit: None,
    });
    assert_eq!(buys(&fx), vec![(rid(9), 500)], "manual buy submits immediately");
    assert_eq!(statuses(&fx), vec![PositionStatus::BuySubmitted]);
    let entry = buy_intent(&fx);
    let fx = reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.1) });
    assert_eq!(statuses(&fx), vec![PositionStatus::Holding]);

    // Tracked-only: neither a crash in price nor a dead pool may auto-exit it.
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 0.01, 0.5, 200.0) });
    assert!(sells(&fx).is_empty(), "tracked-only manual position has NO auto-exit");
    let fx = reduce(&mut s, Event::Tick { now: ts(2000.0) });
    assert!(sells(&fx).is_empty(), "not even the dead-token verdict fires");

    // Manual close still routes through the engine.
    let position = *s.positions.keys().next().expect("held");
    let fx = reduce(&mut s, Event::ManualClose { position, portion: Portion::All });
    assert_eq!(sells(&fx).iter().map(|(_, r)| *r).collect::<Vec<_>>(), vec![ExitReason::Manual]);
}

#[test]
fn manual_buy_with_tp_sl_gets_full_exit_stack() {
    use hunter_engine::event::ManualExit;
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    let fx = reduce(&mut s, Event::ManualBuy {
        mint: m.clone(), rule: rid(9), lamports: 500, at: ts(0.0),
        exit: Some(ManualExit { tp_pct: Some(100.0), sl_pct: Some(30.0) }),
    });
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.1) });

    // +100% → TakeProfit fires through the one desugared pnl path.
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 2.0, 40.0, 1.0) });
    assert_eq!(
        sells(&fx).iter().map(|(_, r)| *r).collect::<Vec<_>>(),
        vec![ExitReason::TakeProfit],
    );
    let exit = sell_intent(&fx);
    let fx = reduce(&mut s, Event::FillConfirmed { intent: exit, fill: fill(2.0, 1.1) });
    assert_eq!(statuses(&fx), vec![PositionStatus::End]);
    assert!(s.manual_rules.is_empty(), "one-off exit rule dies with the position");
    assert!(!s.tokens.contains_key(&m), "manual episode is one-shot — token pruned");
}

#[test]
fn set_manual_exit_upgrades_tracked_only_position() {
    use hunter_engine::event::ManualExit;
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    let fx = reduce(&mut s, Event::ManualBuy {
        mint: m.clone(), rule: rid(9), lamports: 500, at: ts(0.0), exit: None,
    });
    let entry = buy_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.1) });
    let position = *s.positions.keys().next().expect("held");

    // Still tracked-only: -70% does nothing.
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 0.3, 40.0, 1.0) });
    assert!(sells(&fx).is_empty());

    // [+TP/SL] → the same drop now fires the StopLoss.
    reduce(&mut s, Event::SetManualExit {
        position,
        exit: Some(ManualExit { tp_pct: None, sl_pct: Some(30.0) }),
    });
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 0.3, 40.0, 2.0) });
    assert_eq!(
        sells(&fx).iter().map(|(_, r)| *r).collect::<Vec<_>>(),
        vec![ExitReason::StopLoss],
    );
}

#[test]
fn manual_buy_retries_with_frozen_lamports_then_entry_failed() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    let fx = reduce(&mut s, Event::ManualBuy {
        mint: m.clone(), rule: rid(9), lamports: 777, at: ts(0.0), exit: None,
    });
    let mut intent = buy_intent(&fx);
    for _ in 0..2 {
        let fx = reduce(&mut s, Event::FillFailed { intent: intent.clone(), reason: FillFailReason::Reverted, at: None });
        assert_eq!(buys(&fx), vec![(rid(9), 777)], "retry resubmits the FROZEN manual size");
        intent = buy_intent(&fx);
    }
    let fx = reduce(&mut s, Event::FillFailed { intent, reason: FillFailReason::Reverted, at: None });
    assert!(buys(&fx).is_empty());
    assert_eq!(statuses(&fx), vec![PositionStatus::EntryFailed]);
    assert!(s.manual_rules.is_empty() && s.positions.is_empty());
}

// ── Scale-out / partial exits (docs/plans/strategies/partial-exits.md) ────────

/// Enter-on-arm + fill helper for scale-out scenarios.
fn enter_holding(s: &mut EngineState, m: &Mint) {
    let fx = reduce(
        s,
        Event::TokenCreated {
            mint: m.clone(),
            fp: cu_token(),
            at: ts(0.0),
            creator_wallet_hash: None, identity: None,
        },
    );
    let entry = buy_intent(&fx);
    reduce(s, Event::FillConfirmed { intent: entry, fill: fill(1.0, 0.1) });
}

#[test]
fn scale_out_stage_fires_partial_and_advances() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(
        &mut s,
        reload(
            vec![rule(
                1,
                1,
                json!({
                    "scale_out": [
                        { "sell_bps": 7000, "take_profit": 50 },
                        { "conditions": { "m_position": { "held": [{ "operator": ">=", "value": 30 }] } } }
                    ]
                }),
            )],
            vec![cu_fp(1)],
        ),
    );
    enter_holding(&mut s, &m);

    // +50% → stage-0 partial (7000 bps), NOT a full End.
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.5, 40.0, 1.0) });
    assert_eq!(sell_portions(&fx), vec![Portion::BpsOfInitial(7000)]);
    assert_eq!(
        sells(&fx).iter().map(|(_, r)| *r).collect::<Vec<_>>(),
        vec![ExitReason::TakeProfit]
    );
    assert_eq!(statuses(&fx), vec![PositionStatus::ExitPending]);
    assert_eq!(stages(&fx), vec![Some(0)]);
    let leg = sell_intent(&fx);

    // Partial fill → Holding again, stage advanced to 1; position still open.
    let fx = reduce(&mut s, Event::FillConfirmed { intent: leg, fill: fill(1.5, 1.1) });
    assert_eq!(statuses(&fx), vec![PositionStatus::Holding]);
    assert_eq!(stages(&fx), vec![Some(1)]);
    assert!(s.positions.len() == 1, "mid-ladder keeps the concurrency slot");
    let token = s.tokens.get(&m).expect("still tracked");
    match token.arms.get(&rid(1)) {
        Some(hunter_engine::arm::ArmState::Entered(ctx)) => {
            assert_eq!(ctx.stage, 1);
            assert_eq!(ctx.sold_bps, 7000);
            assert_eq!(ctx.peak_price, 1.5, "peak resumed, not reseeded");
        }
        other => panic!("expected Entered after partial fill, got {other:?}"),
    }

    // Remainder stage (held >= 30) closes All.
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.6, 40.0, 40.0) });
    assert_eq!(sell_portions(&fx), vec![Portion::All]);
    let exit = sell_intent(&fx);
    let fx = reduce(&mut s, Event::FillConfirmed { intent: exit, fill: fill(1.6, 40.1) });
    assert_eq!(statuses(&fx), vec![PositionStatus::End]);
    assert!(!s.tokens.contains_key(&m));
}

#[test]
fn scale_out_global_sl_mid_ladder_closes_remainder() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(
        &mut s,
        reload(
            vec![rule(
                1,
                1,
                json!({
                    "stop_loss": 30,
                    "scale_out": [{ "sell_bps": 7000, "take_profit": 50 }]
                }),
            )],
            vec![cu_fp(1)],
        ),
    );
    enter_holding(&mut s, &m);

    // Bank 70% at +50%.
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.5, 40.0, 1.0) });
    let leg = sell_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: leg, fill: fill(1.5, 1.1) });

    // Price crashes to −40% from entry → global SL closes the stub (All), not
    // another stage (there is none left).
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 0.6, 40.0, 2.0) });
    assert_eq!(sell_portions(&fx), vec![Portion::All]);
    assert_eq!(
        sells(&fx).iter().map(|(_, r)| *r).collect::<Vec<_>>(),
        vec![ExitReason::StopLoss]
    );
}

#[test]
fn scale_out_after_last_partial_global_trail_closes_stub() {
    // One partial into strength; stub trails under the global exit side.
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(
        &mut s,
        reload(
            vec![rule(
                1,
                1,
                json!({
                    "exit": { "m_position": {
                        "retrace": [{ "operator": ">=", "value": 8 }],
                        "arm_above_pct": 2
                    } },
                    "scale_out": [{ "sell_bps": 7000, "take_profit": 50 }]
                }),
            )],
            vec![cu_fp(1)],
        ),
    );
    enter_holding(&mut s, &m);

    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.5, 40.0, 1.0) });
    assert_eq!(sell_portions(&fx), vec![Portion::BpsOfInitial(7000)]);
    let leg = sell_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: leg, fill: fill(1.5, 1.1) });

    // Peak stays 1.5; drop to 1.35 = 10% retrace off peak, pnl +35% clears the gate.
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.35, 40.0, 2.0) });
    assert_eq!(sell_portions(&fx), vec![Portion::All]);
    assert!(matches!(
        sells(&fx)[0].1,
        ExitReason::Metrics { .. }
    ));
}

#[test]
fn scale_out_partial_fill_fail_exhaust_goes_exit_stuck() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(
        &mut s,
        reload(
            vec![rule(1, 1, json!({ "scale_out": [{ "sell_bps": 5000, "take_profit": 20 }] }))],
            vec![cu_fp(1)],
        ),
    );
    enter_holding(&mut s, &m);

    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.25, 40.0, 1.0) });
    assert_eq!(sell_portions(&fx), vec![Portion::BpsOfInitial(5000)]);
    let mut intent = sell_intent(&fx);
    // MAX_EXIT_ATTEMPTS = 5 → four retries then ExitStuck on the 5th failure.
    for _ in 0..4 {
        let fx = reduce(
            &mut s,
            Event::FillFailed { intent: intent.clone(), reason: FillFailReason::Reverted, at: None },
        );
        assert_eq!(sell_portions(&fx), vec![Portion::BpsOfInitial(5000)]);
        intent = sell_intent(&fx);
    }
    let fx = reduce(&mut s, Event::FillFailed { intent, reason: FillFailReason::Reverted, at: None });
    assert!(sells(&fx).is_empty());
    assert_eq!(statuses(&fx), vec![PositionStatus::ExitStuck]);
}

#[test]
fn scale_out_reentry_only_after_final_close() {
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(
        &mut s,
        reload(
            vec![rule(
                1,
                1,
                json!({
                    "reentry": { "cooldown_sec": 0, "max_episodes_per_token": 3 },
                    "scale_out": [
                        { "sell_bps": 7000, "take_profit": 50 },
                        { "take_profit": 100 }
                    ]
                }),
            )],
            vec![cu_fp(1)],
        ),
    );
    enter_holding(&mut s, &m);

    // Partial bank — must NOT count an episode / re-arm.
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 1.5, 40.0, 1.0) });
    let leg = sell_intent(&fx);
    reduce(&mut s, Event::FillConfirmed { intent: leg, fill: fill(1.5, 1.1) });
    let token = s.tokens.get(&m).unwrap();
    assert!(token.episodes.get(&rid(1)).is_none(), "partial does not bump episodes");
    assert!(matches!(
        token.arms.get(&rid(1)),
        Some(hunter_engine::arm::ArmState::Entered(_))
    ));

    // Remainder TP at +100% → final End → then Cooldown (re-entry).
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 2.0, 40.0, 2.0) });
    assert_eq!(sell_portions(&fx), vec![Portion::All]);
    let exit = sell_intent(&fx);
    let fx = reduce(&mut s, Event::FillConfirmed { intent: exit, fill: fill(2.0, 2.1) });
    assert_eq!(statuses(&fx), vec![PositionStatus::End]);
    let token = s.tokens.get(&m).expect("re-entry keeps token tracked");
    assert_eq!(token.episodes.get(&rid(1)), Some(&1));
    assert!(matches!(
        token.arms.get(&rid(1)),
        Some(hunter_engine::arm::ArmState::Cooldown { .. })
            | Some(hunter_engine::arm::ArmState::Armed)
    ));
}

#[test]
fn scale_out_absent_legacy_sell_is_portion_all() {
    // Existing TP path must still emit Portion::All (byte-identical decisions aside
    // from the new field defaulting to All).
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(&mut s, reload(vec![rule(1, 1, json!({ "take_profit": 100 }))], vec![cu_fp(1)]));
    enter_holding(&mut s, &m);
    let fx = reduce(&mut s, Event::Trade { mint: m.clone(), trade: trade(1.0, 2.0, 40.0, 1.0) });
    assert_eq!(sell_portions(&fx), vec![Portion::All]);
    assert_eq!(
        sells(&fx).iter().map(|(_, r)| *r).collect::<Vec<_>>(),
        vec![ExitReason::TakeProfit]
    );
}

#[test]
fn manual_close_partial_preserves_holding_and_advances_sold_bps() {
    // Console "Sell N%" — same Portion plumbing as scale-out; fill restores Holding.
    let mut s = EngineState::new();
    let m = Mint::from("tokA");
    reduce(&mut s, reload(vec![rule(1, 1, json!({ "take_profit": 100 }))], vec![cu_fp(1)]));
    enter_holding(&mut s, &m);

    let position = *s.positions.keys().next().expect("one open position");
    let fx = reduce(
        &mut s,
        Event::ManualClose {
            position,
            portion: Portion::BpsOfInitial(5000),
        },
    );
    assert_eq!(sell_portions(&fx), vec![Portion::BpsOfInitial(5000)]);
    assert_eq!(
        sells(&fx).iter().map(|(_, r)| *r).collect::<Vec<_>>(),
        vec![ExitReason::Manual]
    );
    assert_eq!(statuses(&fx), vec![PositionStatus::ExitPending]);
    let leg = sell_intent(&fx);

    let fx = reduce(&mut s, Event::FillConfirmed { intent: leg, fill: fill(1.2, 1.0) });
    assert_eq!(statuses(&fx), vec![PositionStatus::Holding]);
    assert_eq!(stages(&fx), vec![Some(1)]);
    assert_eq!(s.positions.len(), 1, "partial manual keeps the concurrency slot");
    let token = s.tokens.get(&m).expect("still tracked");
    match token.arms.get(&rid(1)) {
        Some(hunter_engine::arm::ArmState::Entered(ctx)) => {
            assert_eq!(ctx.stage, 1);
            assert_eq!(ctx.sold_bps, 5000);
        }
        other => panic!("expected Entered after partial manual fill, got {other:?}"),
    }

    // Final ManualClose All closes the remainder → End (re-entry only then).
    let fx = reduce(&mut s, Event::ManualClose { position, portion: Portion::All });
    assert_eq!(sell_portions(&fx), vec![Portion::All]);
    let exit = sell_intent(&fx);
    let fx = reduce(&mut s, Event::FillConfirmed { intent: exit, fill: fill(1.3, 2.0) });
    assert_eq!(statuses(&fx), vec![PositionStatus::End]);
    assert!(!s.tokens.contains_key(&m));
}

// ── Duplicate-identity guard (`strategy.skip_duplicate_identity`) ─────────────

/// A create event for a token that matches [`cu_fp`], carrying an identity.
fn created(mint: &Mint, at: f64, name: &str, symbol: &str) -> Event {
    Event::TokenCreated {
        mint: mint.clone(),
        fp: cu_token(),
        at: ts(at),
        creator_wallet_hash: None,
        identity: hunter_engine::token_identity_hash(name, symbol),
    }
}

/// The whole point: a copycat re-launch keeps the name and icon and changes only
/// the mint, so every per-token gate lets it through. With the guard on, the
/// second mint disarms instead of buying.
#[test]
fn dupe_guard_blocks_a_second_mint_with_the_same_name_and_symbol() {
    let mut s = EngineState::new();
    s.set_dupe_guard_policy(true, hunter_engine::dupe_guard::DEFAULT_WINDOW_HOURS);
    // Two concurrent slots, so the block is the ONLY thing that can stop the
    // second buy (an exhausted cap would prove nothing).
    reduce(&mut s, reload(vec![rule_capped(1, 1, json!({ "take_profit": 100 }), 5, 0)], vec![cu_fp(1)]));

    let first = Mint::from("tokA");
    let fx = reduce(&mut s, created(&first, 0.0, "Moon Dog", "MDOG"));
    assert_eq!(buys(&fx), vec![(rid(1), BUY)], "the original still buys");

    let copycat = Mint::from("tokB");
    let fx = reduce(&mut s, created(&copycat, 30.0, "moon  dog", "mdog"));
    assert!(buys(&fx).is_empty(), "copycat must not be bought");
    assert_eq!(disarms(&fx), vec![DisarmReason::DuplicateIdentity]);

    // A genuinely different token is untouched — the guard is a key match, not a
    // blanket pause.
    let other = Mint::from("tokC");
    let fx = reduce(&mut s, created(&other, 31.0, "Sun Cat", "SCAT"));
    assert_eq!(buys(&fx), vec![(rid(1), BUY)]);
}

/// The guard must not block the token that created its own record — otherwise the
/// first entry poisons its own retry ladder and no rule can ever fill.
#[test]
fn dupe_guard_never_blocks_the_mint_that_recorded_the_identity() {
    let mut s = EngineState::new();
    s.set_dupe_guard_policy(true, hunter_engine::dupe_guard::DEFAULT_WINDOW_HOURS);
    reduce(&mut s, reload(vec![rule(1, 1, json!({ "take_profit": 100 }))], vec![cu_fp(1)]));

    let m = Mint::from("tokA");
    let fx = reduce(&mut s, created(&m, 0.0, "Moon Dog", "MDOG"));
    let entry = buy_intent(&fx);

    // The buy reverts; the engine re-submits on the SAME mint. The record it just
    // wrote must not be the thing that kills the retry.
    let fx = reduce(
        &mut s,
        Event::FillFailed {
            intent: entry,
            reason: FillFailReason::Reverted,
            at: Some(ts(1.0)),
        },
    );
    assert_eq!(buys(&fx).len(), 1, "the retry still goes out");
    assert!(disarms(&fx).is_empty());
}

/// A failed entry is still evidence the identity was traded (the user's call:
/// "if any ever-trade even it's failed, avoid the tokens"), so a copycat is
/// blocked even though nothing ever filled.
#[test]
fn dupe_guard_records_an_entry_that_never_filled() {
    let mut s = EngineState::new();
    s.set_dupe_guard_policy(true, hunter_engine::dupe_guard::DEFAULT_WINDOW_HOURS);
    reduce(&mut s, reload(vec![rule_capped(1, 1, json!({ "take_profit": 100 }), 5, 0)], vec![cu_fp(1)]));

    let first = Mint::from("tokA");
    let fx = reduce(&mut s, created(&first, 0.0, "Moon Dog", "MDOG"));
    let mut intent = buy_intent(&fx);
    // Burn the whole attempt ladder — the entry is abandoned, never filled.
    for i in 0..8 {
        let fx = reduce(
            &mut s,
            Event::FillFailed {
                intent: intent.clone(),
                reason: FillFailReason::Reverted,
                at: Some(ts(1.0 + i as f64)),
            },
        );
        match fx.iter().find_map(|e| match e {
            Effect::SubmitBuy { intent, .. } => Some(intent.clone()),
            _ => None,
        }) {
            Some(next) => intent = next,
            None => break,
        }
    }

    let copycat = Mint::from("tokB");
    let fx = reduce(&mut s, created(&copycat, 60.0, "Moon Dog", "MDOG"));
    assert!(buys(&fx).is_empty(), "a failed entry still burns the identity");
    assert_eq!(disarms(&fx), vec![DisarmReason::DuplicateIdentity]);
}

/// Off by default: an engine that never sets the policy behaves exactly as before,
/// so the switch cannot change anyone's live behavior until it is turned on.
#[test]
fn dupe_guard_is_off_until_enabled() {
    let mut s = EngineState::new();
    reduce(&mut s, reload(vec![rule_capped(1, 1, json!({ "take_profit": 100 }), 5, 0)], vec![cu_fp(1)]));

    let first = Mint::from("tokA");
    reduce(&mut s, created(&first, 0.0, "Moon Dog", "MDOG"));
    let copycat = Mint::from("tokB");
    let fx = reduce(&mut s, created(&copycat, 30.0, "Moon Dog", "MDOG"));
    assert_eq!(buys(&fx), vec![(rid(1), BUY)], "guard off ⇒ legacy behavior");
}

/// Paper and real keep separate memories, so a paper rule's entry can never
/// silence a real one (the mode-blind mistake the run cache already made).
#[test]
fn dupe_guard_keeps_paper_and_real_memories_apart() {
    let mut s = EngineState::new();
    s.set_dupe_guard_policy(true, hunter_engine::dupe_guard::DEFAULT_WINDOW_HOURS);
    let mut real = rule_capped(2, 1, json!({ "take_profit": 100 }), 5, 0);
    real.trade_mode = TradeMode::Real;
    // Paper rule (id 1) and real rule (id 2) both arm on the same fingerprint.
    reduce(
        &mut s,
        reload(vec![rule_capped(1, 1, json!({ "take_profit": 100 }), 5, 0), real], vec![cu_fp(1)]),
    );

    let first = Mint::from("tokA");
    let fx = reduce(&mut s, created(&first, 0.0, "Moon Dog", "MDOG"));
    assert_eq!(buys(&fx).len(), 2, "both modes enter the original");

    // The copycat is blocked in BOTH modes — each recorded its own entry.
    let copycat = Mint::from("tokB");
    let fx = reduce(&mut s, created(&copycat, 30.0, "Moon Dog", "MDOG"));
    assert!(buys(&fx).is_empty());
    assert_eq!(disarms(&fx).len(), 2);
}

/// A token with no name or no symbol has no identity, and two unknowns are not
/// the same token — it must never block, or every metadata-less mint would
/// blacklist every other one.
#[test]
fn dupe_guard_ignores_tokens_with_no_identity() {
    let mut s = EngineState::new();
    s.set_dupe_guard_policy(true, hunter_engine::dupe_guard::DEFAULT_WINDOW_HOURS);
    reduce(&mut s, reload(vec![rule_capped(1, 1, json!({ "take_profit": 100 }), 5, 0)], vec![cu_fp(1)]));

    let first = Mint::from("tokA");
    let fx = reduce(&mut s, created(&first, 0.0, "", ""));
    assert_eq!(buys(&fx), vec![(rid(1), BUY)]);

    let second = Mint::from("tokB");
    let fx = reduce(&mut s, created(&second, 30.0, "", ""));
    assert_eq!(buys(&fx), vec![(rid(1), BUY)], "blank ≠ blank");
}

/// The memory expires: past the window the same identity is tradeable again.
#[test]
fn dupe_guard_forgets_past_the_window() {
    let mut s = EngineState::new();
    s.set_dupe_guard_policy(true, 1); // 1-hour window
    reduce(&mut s, reload(vec![rule_capped(1, 1, json!({ "take_profit": 100 }), 5, 0)], vec![cu_fp(1)]));

    let first = Mint::from("tokA");
    reduce(&mut s, created(&first, 0.0, "Moon Dog", "MDOG"));

    let within = Mint::from("tokB");
    let fx = reduce(&mut s, created(&within, 1800.0, "Moon Dog", "MDOG"));
    assert!(buys(&fx).is_empty(), "still inside the window");

    let after = Mint::from("tokC");
    let fx = reduce(&mut s, created(&after, 3601.0, "Moon Dog", "MDOG"));
    assert_eq!(buys(&fx), vec![(rid(1), BUY)], "past the window it is tradeable again");
}

/// the `prior_launches` fingerprint axis counts a creator's launches STRICTLY BEFORE each
/// token, so their first reads `0` — the value a first-launch rule selects on.
///
/// The unknown-creator case is the one that matters: a `TokenCreated` with no
/// `creator_wallet_hash` must leave the metric `NaN`, never `0`. Seeding `0` there
/// would quietly widen a `prior_launches == 0` gate to every token whose creator the
/// feed failed to resolve.
#[test]
fn prior_launches_counts_strictly_prior_and_never_guesses_zero() {
    let mut s = EngineState::new();
    // A rule that arms but can never enter, so every created token stays tracked and
    // its metric is readable (`reduce` drops a token with no live arm).
    reduce(
        &mut s,
        reload(
            vec![rule(
                1,
                1,
                json!({ "entry": { "m_state": {
                    "time": [{ "operator": ">=", "value": 1e9 }] } } }),
            )],
            vec![cu_fp(1)],
        ),
    );
    // History that predates the stream — without this a live restart or a
    // corpus-scoped backtest reads every creator as a first-time launcher.
    s.prime_creator_launches([(77u64, 4u32)]);

    // Read the tally off the token's OBSERVED AXES — where `reduce` stamps it, and
    // what the matcher grades a `prior_launches` fingerprint against. `None` is the
    // honest "unknown creator", which fails a configured axis rather than reading as
    // a first launch.
    let read = |s: &EngineState, mint: &str| -> Option<u32> {
        s.tokens[&Mint::from(mint)].tf.prior_launches
    };

    for (i, mint) in ["a", "b", "c"].iter().enumerate() {
        reduce(
            &mut s,
            Event::TokenCreated {
                mint: Mint::from(*mint),
                fp: cu_token(),
                at: ts(i as f64),
                creator_wallet_hash: Some(9),
                identity: None,
            },
        );
    }
    assert_eq!(read(&s, "a"), Some(0), "a creator's first launch is 0, not 1");
    assert_eq!(read(&s, "b"), Some(1));
    assert_eq!(read(&s, "c"), Some(2));

    // Primed history is the floor the tally continues from.
    reduce(
        &mut s,
        Event::TokenCreated {
            mint: Mint::from("d"),
            fp: cu_token(),
            at: ts(9.0),
            creator_wallet_hash: Some(77),
            identity: None,
        },
    );
    assert_eq!(read(&s, "d"), Some(4), "primed count must not restart at 0");

    // Unknown creator ⇒ unknown count.
    reduce(
        &mut s,
        Event::TokenCreated {
            mint: Mint::from("e"),
            fp: cu_token(),
            at: ts(10.0),
            creator_wallet_hash: None,
            identity: None,
        },
    );
    assert_eq!(read(&s, "e"), None, "unknown creator must not read as a first launch");

    // A duplicate creation is idempotent in the reducer, so it must not advance the
    // tally either — a replayed event would otherwise inflate every later token.
    reduce(
        &mut s,
        Event::TokenCreated {
            mint: Mint::from("c"),
            fp: cu_token(),
            at: ts(2.0),
            creator_wallet_hash: Some(9),
            identity: None,
        },
    );
    reduce(
        &mut s,
        Event::TokenCreated {
            mint: Mint::from("f"),
            fp: cu_token(),
            at: ts(11.0),
            creator_wallet_hash: Some(9),
            identity: None,
        },
    );
    assert_eq!(read(&s, "f"), Some(3), "a duplicate creation must not advance the tally");
}
