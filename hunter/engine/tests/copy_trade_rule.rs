//! The copy trade: one target wallet, buy only on the curve, sell on both.
//!
//! Pins parse + compile + the whole live path of the rule
//! `hunter/scripts/seed-copy-trade-rule.sql` seeds, against
//! `hunter/docs/plans/strategies/copy-trade-plan.md`. It does not tune the cell —
//! the SOL floor, the age and depth filters and the backstop are the operator's,
//! and this asserts the SHAPE those knobs hang on:
//!
//! * the trigger is `m_copy_window` on a `1p` window, so a split buy is two fires
//!   and the second print of the token is not one;
//! * the exit's copy clause is `m_copy_window` too, and reads the same on the AMM;
//! * entry is curve-only for free — `Event::Migrated` disarms, and it disarms even
//!   though the position stays open and keeps pricing off AMM prints.

use std::sync::Arc;

use chrono::{Duration, TimeZone, Utc};
use hunter_engine::arm::CompiledRule;
use hunter_engine::event::{Effect, Event, Fill, LoadedRule, Mint, RuleId, TradeMode};
use hunter_engine::fingerprint::{Criteria, Fingerprint, FingerprintId};
use hunter_engine::grouping::TokenFingerprint;
use hunter_engine::metrics::copy::{CopyPatterns, CONFIG_KEY, TARGETS_FIELD};
use hunter_engine::metrics::{MetricId, Side, TradeLite, Ts, WindowUnit};
use hunter_engine::rule_params::{EntryLock, ExitSide, ReEntry, RuleParams};
use hunter_engine::reduce::reduce;
use hunter_engine::EngineState;
use serde_json::{json, Value};
use uuid::Uuid;

/// The wallet the rule copies. One rule per target, so this is the whole list.
const TARGET: &str = "TARGETwa11etAddre55Base58";
/// Anyone else on the same tape.
const OTHER: &str = "ANOTHERwa11etEntire1y";

const RULE: u128 = 1;
const FP: u128 = 2;

fn ts(secs: f64) -> Ts {
    Utc.timestamp_opt(1_700_000_000, 0).unwrap() + Duration::milliseconds((secs * 1000.0) as i64)
}

/// The seeded rule, verbatim: his buy is the event, the operator's filters are the
/// AND-gate, and the exit is his sell OR a time backstop.
///
/// The age and depth gates sit under `disabled` rather than in `entry`, because the
/// seeded target snipes at a median token age of 5.4 s and a 30 s floor would drop
/// almost every buy he makes. Parked conditions parse and validate exactly like live
/// ones, and nothing compiles them.
fn copy_params() -> Value {
    json!({
        "exclusive": true,
        "priority": 10,
        "reentry": { "cooldown_sec": 0, "max_episodes_per_token": 1 },
        "entry_lock": "slot",
        "entry_event": {
            "m_copy_window": {
                "window_size_prints": 1,
                "buy_sol": [{"operator": ">=", "value": 0.04}]
            }
        },
        "entry": {
            "m_copy": { "sell_count": [{"operator": "=", "value": 0}] }
        },
        "exit": [
            { "m_copy_window": {
                "window_size_prints": 1,
                "sell_sol": [{"operator": ">", "value": 0}]
            } },
            { "m_position": { "held": [{"operator": ">=", "value": 300}] } }
        ],
        "disabled": {
            "entry": {
                "m_state": {
                    "time": [{"operator": ">=", "value": 30}],
                    "liquidity": [{"operator": ">=", "value": 10}]
                }
            }
        }
    })
}

fn loaded(params: Value) -> LoadedRule {
    LoadedRule {
        id: RuleId(Uuid::from_u128(RULE)),
        fingerprint_id: FingerprintId(Uuid::from_u128(FP)),
        trade_mode: TradeMode::Paper,
        buy_amount_lamports: 100_000_000,
        max_concurrent_tokens: 0,
        max_total_tokens: 0,
        entry_enabled: true,
        params: RuleParams::parse(&params).unwrap_or_else(|e| panic!("copy params: {e}")),
    }
}

/// The wildcard fingerprint that CARRIES the target list. Identity is "every token";
/// the copy rule's selectivity is the wallet, not the creation axes.
fn target_fp() -> Fingerprint {
    Fingerprint {
        id: FingerprintId(Uuid::from_u128(FP)),
        wildcard: true,
        criteria: Criteria::new(),
        metric_config: json!({ CONFIG_KEY: { TARGETS_FIELD: [TARGET] } }),
    }
}

fn print(side: Side, who: &str, sol: f64, secs: f64, slot: u64, on_curve: bool) -> TradeLite {
    TradeLite {
        side,
        sol,
        price: 1.0,
        reserve_sol: 40.0,
        priced_reserve_sol: 70.0,
        at: ts(secs),
        slot,
        leg_index: 0,
        tx_index: Some(slot as u32),
        wallet_hash: hunter_engine::metrics::flow_ix::wallet_hash(who),
        on_curve,
        ..Default::default()
    }
}

/// A live engine with the rule loaded and one token tracked from creation — the only
/// admission lane this strategy has, and the reason entry is curve-only for free.
fn armed_engine(mint: &Mint) -> EngineState {
    let mut state = EngineState::default();
    reduce(
        &mut state,
        Event::RulesReloaded {
            rules: Arc::from(vec![loaded(copy_params())]),
            fps: Arc::from(vec![target_fp()]),
        },
    );
    reduce(
        &mut state,
        Event::TokenCreated {
            mint: mint.clone(),
            fp: Box::new(TokenFingerprint::default()),
            at: ts(0.0),
            creator_wallet_hash: None,
            identity: None,
            creation_slot: Some(99),
        },
    );
    state
}

fn submitted(fx: &[Effect]) -> Option<hunter_engine::event::IntentId> {
    fx.iter().find_map(|e| match e {
        Effect::SubmitBuy { intent, .. } => Some(intent.clone()),
        _ => None,
    })
}

fn sold(fx: &[Effect]) -> bool {
    fx.iter().any(|e| matches!(e, Effect::SubmitSell { .. }))
}

#[test]
fn the_rule_parses_round_trips_and_compiles_to_the_shape_it_is_authored_in() {
    let params = copy_params();
    let p = RuleParams::parse(&params).unwrap_or_else(|e| panic!("{e}"));
    assert!(p.exclusive);
    assert_eq!(p.entry_lock, Some(EntryLock::Slot));
    assert_eq!(
        p.reentry,
        Some(ReEntry { cooldown_sec: 0.0, max_episodes_per_token: 1 })
    );
    assert!(matches!(p.exit, Some(ExitSide::Dnf(ref cs)) if cs.len() == 2));
    assert_eq!(RuleParams::parse(&p.to_value()).unwrap(), p);

    let c = CompiledRule::compile(&loaded(params));
    assert_eq!(c.exit_clauses.len(), 2, "his sell OR the backstop");
    assert_eq!(c.entry_lock, Some(EntryLock::Slot));

    // The trigger is the WINDOW, on this print alone.
    let ev = c
        .event_reqs
        .iter()
        .find(|r| r.metric == MetricId::WinCopyBuySol)
        .expect("his buy is the event");
    let w = ev.window.primary.expect("a windowed trigger");
    assert_eq!((w.size, w.lag, w.unit), (1.0, 0.0, WindowUnit::Print));
    assert!(ev.fingerprint.is_some(), "the target list is fingerprint-scoped");

    // ...and the lifetime group is only ever a filter here.
    assert!(c.entry_reqs.iter().any(|r| r.metric == MetricId::CopySellCount));
    assert!(!c.event_reqs.iter().any(|r| r.metric == MetricId::CopyBuyCount));

    // One buffer per backing deque: the copy window must not land on the flow one,
    // where nothing would ever read it.
    assert_eq!(c.copy_windows.len(), 1);
    assert!(c.flow_windows.is_empty() && c.crowd_windows.is_empty());
    assert!(!c.needs_slot, "a print window does not need the slot column");
}

/// **The only one-way door is the one that is meant to be one.** An upper bound on a
/// monotonic entry metric disarms the token permanently, so which metrics carry one
/// is a decision, not an accident:
///
/// * `m_copy.sell_count = 0` SHOULD latch — once he has taken something off, this
///   token is over for us and re-checking it every print is waste;
/// * `m_state.time >= 30` is a floor and must stay one. `time <= N` would disarm at
///   N seconds, and the target's own entry ages run out past 15 minutes.
#[test]
fn the_only_permanent_disarm_is_his_first_sell() {
    let c = CompiledRule::compile(&loaded(copy_params()));
    let killed: Vec<MetricId> = c.mono_kills.iter().map(|k| k.metric).collect();
    assert_eq!(killed, vec![MetricId::CopySellCount]);
    assert!(
        !killed.contains(&MetricId::Time),
        "an age CEILING would disarm the token long before the target acts"
    );
}

/// The whole live path, in order: his buy on the curve enters, the token migrates
/// while the position is open, and his sell on the AMM exits it.
#[test]
fn his_curve_buy_enters_and_his_amm_sell_exits() {
    let mint = Mint("copy-mint".into());
    let mut state = armed_engine(&mint);

    // Someone else buys 5 SOL. Not the target: no entry, however big.
    let fx = reduce(
        &mut state,
        Event::Trade { mint: mint.clone(), trade: print(Side::Buy, OTHER, 5.0, 31.0, 100, true) },
    );
    assert!(submitted(&fx).is_none(), "a stranger's buy is not the signal");

    // The target buys 0.6 SOL on the curve, past the age door.
    let fx = reduce(
        &mut state,
        Event::Trade { mint: mint.clone(), trade: print(Side::Buy, TARGET, 0.6, 32.0, 101, true) },
    );
    let intent = submitted(&fx).expect("his buy is the signal");
    reduce(
        &mut state,
        Event::FillConfirmed {
            intent,
            fill: Fill { price: 1.0, sol: 0.1, token_amount: 1_000_000, at: ts(32.1) },
        },
    );

    // The token migrates. The position rides it out — AMM prints keep pricing it.
    let fx = reduce(&mut state, Event::Migrated { mint: mint.clone(), at: ts(60.0) });
    assert!(!sold(&fx), "migration is not an exit");

    // A stranger sells on the AMM: still not our signal.
    let fx = reduce(
        &mut state,
        Event::Trade { mint: mint.clone(), trade: print(Side::Sell, OTHER, 4.0, 70.0, 200, false) },
    );
    assert!(!sold(&fx), "a stranger's sell is not the exit");

    // The target sells on the AMM. Copy sell on both venues — this is the arm the
    // rule exists for, and it must read the same off the curve.
    let fx = reduce(
        &mut state,
        Event::Trade { mint: mint.clone(), trade: print(Side::Sell, TARGET, 0.5, 71.0, 201, false) },
    );
    assert!(sold(&fx), "his sell is the exit, on the AMM as on the curve");
}

/// **Entry is curve-only, and it costs nothing to make it so.** A token that has
/// already migrated is disarmed, so the target buying it again never enters.
#[test]
fn a_migrated_token_never_enters_however_the_target_buys_it() {
    let mint = Mint("migrated-mint".into());
    let mut state = armed_engine(&mint);
    reduce(&mut state, Event::Migrated { mint: mint.clone(), at: ts(31.0) });
    let fx = reduce(
        &mut state,
        Event::Trade { mint: mint.clone(), trade: print(Side::Buy, TARGET, 5.0, 40.0, 120, false) },
    );
    assert!(submitted(&fx).is_none(), "the curve is over; no copy buy may fire");
}

/// The SOL floor is a real gate and a failed one does not consume the token — and,
/// the half that matters for THIS target, the floor sits under his smallest preset
/// so a 5-second-old snipe still fires. A floor authored at a round 0.5 would have
/// made the whole rule inert against a wallet whose median buy is 0.0988.
#[test]
fn the_floor_admits_his_presets_and_a_failed_gate_keeps_the_token() {
    let mint = Mint("gated-mint".into());
    let mut state = armed_engine(&mint);

    let fx = reduce(
        &mut state,
        Event::Trade { mint: mint.clone(), trade: print(Side::Buy, TARGET, 0.01, 3.0, 100, true) },
    );
    assert!(submitted(&fx).is_none(), "dust is under the floor");

    // Neither refusal spent the token, and his smallest real preset at 5 s old -
    // squarely inside his p50 entry age - is a fire.
    let fx = reduce(
        &mut state,
        Event::Trade { mint: mint.clone(), trade: print(Side::Buy, TARGET, 0.0494, 5.0, 101, true) },
    );
    assert!(submitted(&fx).is_some(), "his smallest preset, at his median entry age");
}

/// **A parked condition is authored but not compiled.** The age and depth gates are
/// in `disabled`, so they must reach neither the entry reqs nor the tick horizons —
/// re-enabling one is an edit, never something the engine does on its own.
#[test]
fn the_parked_age_and_depth_gates_compile_into_nothing() {
    let p = RuleParams::parse(&copy_params()).unwrap_or_else(|e| panic!("{e}"));
    assert!(p.disabled.is_some(), "the parked bag round-trips");
    assert_eq!(RuleParams::parse(&p.to_value()).unwrap(), p);

    let c = CompiledRule::compile(&loaded(copy_params()));
    for m in [MetricId::Time, MetricId::Liquidity] {
        assert!(
            !c.entry_reqs.iter().chain(c.event_reqs.iter()).any(|r| r.metric == m),
            "{m} is parked, so nothing may compile it"
        );
    }
}

/// The trigger releases. A `1p` window is this print, so the print AFTER his buy
/// reads zero — which is what stops a filled rule from re-firing on the tape and,
/// more importantly, what a second target buy needs in order to be its own fire.
#[test]
fn the_print_after_his_buy_is_not_a_second_fire() {
    use hunter_engine::metrics::track::TokenTrack;
    use hunter_engine::metrics::{WindowSpec, Windows};

    let fp = FingerprintId(Uuid::from_u128(FP));
    let w = WindowSpec::prints(1.0, 0.0);
    let mut track = TokenTrack::new(ts(0.0));
    track.ensure_copy(fp, &CopyPatterns::from_addresses([TARGET]), &[w]);

    let read = |t: &TokenTrack, at: Ts| {
        t.value(MetricId::WinCopyBuySol, Windows::one(w), Some(fp), at)
    };

    track.on_trade(print(Side::Buy, TARGET, 0.6, 31.0, 100, true));
    assert!((read(&track, ts(31.0)) - 0.6).abs() < 1e-9, "his print IS the window");

    track.on_trade(print(Side::Buy, OTHER, 3.0, 31.5, 100, true));
    assert!(read(&track, ts(31.5)).abs() < 1e-9, "the next print releases it");

    track.on_trade(print(Side::Buy, TARGET, 0.7, 32.0, 101, true));
    assert!(
        (read(&track, ts(32.0)) - 0.7).abs() < 1e-9,
        "his second buy is its own fire, not a running total"
    );

    // The lifetime side latched through all of it — which is why it is never a trigger.
    assert_eq!(
        track.value(MetricId::CopyBuyCount, Windows::NONE, Some(fp), ts(32.0)),
        2.0
    );
}

/// **An unconfigured fingerprint reads NaN, not 0.** The exit's `sell_sol > 0` then
/// never fires, and — the dangerous half — the entry's `sell_count = 0` never fires
/// either. A rule pointed at a fingerprint that names no target does nothing at all,
/// rather than buying everything.
#[test]
fn a_fingerprint_with_no_target_list_makes_the_rule_inert() {
    let mint = Mint("no-list-mint".into());
    let mut state = EngineState::default();
    reduce(
        &mut state,
        Event::RulesReloaded {
            rules: Arc::from(vec![loaded(copy_params())]),
            fps: Arc::from(vec![Fingerprint {
                metric_config: json!({}),
                ..target_fp()
            }]),
        },
    );
    reduce(
        &mut state,
        Event::TokenCreated {
            mint: mint.clone(),
            fp: Box::new(TokenFingerprint::default()),
            at: ts(0.0),
            creator_wallet_hash: None,
            identity: None,
            creation_slot: Some(99),
        },
    );
    let fx = reduce(
        &mut state,
        Event::Trade { mint: mint.clone(), trade: print(Side::Buy, TARGET, 5.0, 40.0, 100, true) },
    );
    assert!(submitted(&fx).is_none(), "no list means no signal, never every signal");
}
