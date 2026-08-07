//! Guard on the settled-token tick skip (`state::Settled`).
//!
//! `reduce`'s `Tick` branch may skip a token whose readings provably cannot move
//! and whose cross-token inputs have not changed. That is an **optimization**, and
//! the contract it must not break is the module's headline promise: *identical
//! event streams yield identical effect streams*. So the guard is a differential
//! test — one recorded stream, replayed through an engine with the skip on and one
//! with `dense_ticks` forced, asserting the two produce the same effects, the same
//! tracked tokens, and the same arm states.
//!
//! Why this and not a unit test on the horizon arithmetic: the horizons are only
//! correct *in aggregate* (windows AND `time` AND `stall` AND `held` AND the dead
//! flip AND cooldowns AND the cross-token epoch). A test per anchor would pass
//! while the conjunction was wrong. The two scenarios below therefore drive whole
//! rule sets: a fuzzed stream for breadth, and the exact shape that motivated the
//! optimization — a token that can never die and so is never pruned.

use std::sync::Arc;

use chrono::{Duration, TimeZone, Utc};
use hunter_engine::arm::ArmState;
use hunter_engine::event::{
    Effect, Event, Fill, FillFailReason, IntentId, LoadedRule, Mint, RuleId, TradeMode,
};
use hunter_engine::fingerprint::{Fingerprint, FingerprintId};
use hunter_engine::grouping::TokenFingerprint;
use hunter_engine::metrics::{Side, TradeLite, Ts};
use hunter_engine::reduce::reduce;
use hunter_engine::rule_params::RuleParams;
use hunter_engine::EngineState;
use serde_json::json;
use uuid::Uuid;

/// Deterministic xorshift64* PRNG (same generator as `property.rs` — the engine's
/// purity guard bans a `rand` dependency).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn frac(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn fid(n: u128) -> FingerprintId {
    FingerprintId(Uuid::from_u128(n))
}
fn rid(n: u128) -> RuleId {
    RuleId(Uuid::from_u128(n))
}
fn ts(secs: f64) -> Ts {
    Utc.timestamp_opt(1_700_000_000, 0).unwrap() + Duration::milliseconds((secs * 1000.0) as i64)
}

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
        bucket_size_amount: Some(0.1),
        metric_config: json!({ "m_flow_split": { "volume_ix_patterns": [["Pump.Fun: Buy"]] } }),
    }
}

/// A rule set that reads **every** clock the horizons model, so a wrong horizon in
/// any one of them shows up as a diverging effect stream: trailing flow + price
/// windows, `time` (entry), `stall` (exit), `held` (exit), a scale-out stage, and
/// re-entry with a cooldown. Plus a bare enter-on-arm rule, whose horizon collapses
/// to the dead flip alone — the settle-fastest case.
fn rules() -> Vec<LoadedRule> {
    vec![
        LoadedRule {
            id: rid(1),
            fingerprint_id: fid(1),
            trade_mode: TradeMode::Paper,
            buy_amount_lamports: 500_000_000,
            max_concurrent_tokens: 2,
            max_total_tokens: 6,
            params: RuleParams::parse(&json!({
                "take_profit": 80,
                "stop_loss": 40,
                "entry": {
                    "m_snapshot": { "time": [{ "operator": "<", "value": 45 }] },
                    "m_flow_window": {
                        "window_size_sec": 20,
                        "gross_flow": [{ "operator": ">", "value": 0.4 }]
                    }
                },
                "exit": {
                    "m_price_lifetime": { "stall": [{ "operator": ">", "value": 25 }] },
                    "m_position": { "held": [{ "operator": ">=", "value": 90 }] }
                },
                "reentry": { "cooldown_sec": 30, "max_episodes_per_token": 3 }
            }))
            .unwrap(),
            entry_enabled: true,
        },
        LoadedRule {
            id: rid(2),
            fingerprint_id: fid(1),
            trade_mode: TradeMode::Paper,
            buy_amount_lamports: 250_000_000,
            max_concurrent_tokens: 1,
            max_total_tokens: 0,
            params: RuleParams::parse(&json!({
                "entry": {
                    "m_price_window": {
                        "window_size_sec": 35,
                        "trail": [{ "operator": ">=", "value": 10 }]
                    },
                    "m_flow_split_window": {
                        "window_size_sec": 15,
                        "vol_share": [{ "operator": "<", "value": 70 }]
                    }
                },
                "scale_out": [{ "sell_bps": 5000, "take_profit": 25 }],
                "exit": { "m_flow_split": { "nonvol_net": [{ "operator": "<", "value": -3 }] } }
            }))
            .unwrap(),
            entry_enabled: true,
        },
        LoadedRule {
            id: rid(3),
            fingerprint_id: fid(1),
            trade_mode: TradeMode::Paper,
            buy_amount_lamports: 100_000_000,
            max_concurrent_tokens: 1,
            max_total_tokens: 2,
            params: RuleParams::parse(&json!({ "take_profit": 500 })).unwrap(),
            entry_enabled: true,
        },
    ]
}

fn engine(dense: bool) -> EngineState {
    let mut s = EngineState::new();
    s.dense_ticks = dense;
    reduce(
        &mut s,
        Event::RulesReloaded { rules: Arc::from(rules()), fps: Arc::from(vec![cu_fp(1)]) },
    );
    s
}

/// Replay and also return the observable end state (tracked mints + arm states),
/// so a skip that merely *delays* a disarm is caught as well as one that drops it.
fn run_with_state(events: &[Event], dense: bool) -> (Vec<Effect>, Vec<(Mint, Vec<ArmState>)>) {
    let mut s = engine(dense);
    let mut out = Vec::new();
    for e in events {
        out.extend(reduce(&mut s, e.clone()));
    }
    let end = s
        .tokens
        .iter()
        .map(|(m, t)| (m.clone(), t.arms.values().cloned().collect()))
        .collect();
    (out, end)
}

/// Build a pseudo-random event stream. Runs a throwaway dense engine alongside so
/// fill events can reference intents the engine really minted (a stream of bogus
/// intents would exercise nothing).
fn stream(seed: u64, n_events: usize) -> Vec<Event> {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut s = engine(true);
    let mints: Vec<Mint> = (0..5).map(|i| Mint::from(format!("tok{i}"))).collect();
    let mut pending: Vec<IntentId> = Vec::new();
    let mut out: Vec<Event> = Vec::new();
    let mut now = 0.0f64;

    for _ in 0..n_events {
        // Long quiet stretches are the whole point — most steps are a small tick
        // step, but one in eight jumps far enough to settle every token (past the
        // widest window and the 300 s dead window).
        now += if rng.below(8) == 0 { 60.0 + rng.frac() * 400.0 } else { rng.frac() * 2.0 };
        let mint = mints[rng.below(mints.len() as u64) as usize].clone();
        let event = match rng.below(10) {
            0 => Event::TokenCreated {
                mint,
                fp: Box::new(TokenFingerprint {
                    cu_limit: Some(200_000),
                    ..Default::default()
                }),
                at: ts(now),
                creator_wallet_hash: Some(7),
                identity: None,
            },
            1..=3 => Event::Trade {
                mint,
                trade: TradeLite {
                    side: if rng.frac() < 0.5 { Side::Buy } else { Side::Sell },
                    sol: rng.frac() * 2.0,
                    price: 0.5 + rng.frac() * 3.0,
                    // Straddles DEAD_MAX_LIQUIDITY_SOL (30) so both the prunable and
                    // the never-dies (and therefore never-pruned) token exist here.
                    reserve_sol: rng.frac() * 60.0,
                    at: ts(now),
                    ix_hash: (rng.frac() < 0.4)
                        .then(|| hunter_engine::metrics::flow_split::ix_hash(&["Pump.Fun: Buy"])),
                    wallet_hash: rng.below(6),
                },
            },
            8 => {
                if pending.is_empty() {
                    Event::Tick { now: ts(now) }
                } else {
                    let intent = pending.swap_remove(rng.below(pending.len() as u64) as usize);
                    if rng.frac() < 0.7 {
                        Event::FillConfirmed {
                            intent,
                            fill: Fill {
                                price: 0.5 + rng.frac() * 3.0,
                                sol: rng.frac() * 2.0,
                                token_amount: 1_000_000,
                                at: ts(now),
                            },
                        }
                    } else {
                        Event::FillFailed {
                            intent,
                            reason: FillFailReason::Reverted,
                            at: Some(ts(now)),
                        }
                    }
                }
            }
            _ => Event::Tick { now: ts(now) },
        };
        for e in reduce(&mut s, event.clone()) {
            if let Effect::SubmitBuy { intent, .. } | Effect::SubmitSell { intent, .. } = e {
                pending.push(intent);
            }
        }
        out.push(event);
    }
    out
}

/// **The guard.** Skipping settled tokens on a tick must change nothing an observer
/// can see: same effects, in the same order, and the same end state.
#[test]
fn settled_tick_skip_is_decision_neutral() {
    for seed in 1..25u64 {
        let events = stream(seed, 600);
        let (sparse_fx, sparse_end) = run_with_state(&events, false);
        let (dense_fx, dense_end) = run_with_state(&events, true);
        // Report the FIRST divergence, not just the counts — a length mismatch alone
        // says nothing about which horizon is short.
        for (i, (a, b)) in sparse_fx.iter().zip(&dense_fx).enumerate() {
            assert_eq!(a, b, "seed {seed}: effect {i} diverged");
        }
        assert_eq!(
            sparse_fx.len(),
            dense_fx.len(),
            "seed {seed}: effect count diverged; first extra dense effect: {:?}",
            dense_fx.get(sparse_fx.len().min(dense_fx.len()))
        );
        assert_eq!(sparse_end, dense_end, "seed {seed}: end state diverged");
    }
}

/// The shape the optimization exists for: healthy reserves (≥ `DEAD_MAX_LIQUIDITY_SOL`)
/// mean the dead verdict can never fire, so the arm never disarms and the token is
/// never pruned. It must (a) stop being swept once every clock it reads has run out,
/// and (b) still decide correctly when a trade finally arrives hours later.
#[test]
fn an_undying_quiet_token_settles_then_still_decides_on_a_late_trade() {
    let mut s = engine(false);
    let mint = Mint::from("undying");
    let trade = |secs: f64, price: f64| Event::Trade {
        mint: Mint::from("undying"),
        trade: TradeLite {
            side: Side::Buy,
            sol: 1.0,
            price,
            // Well above the dead floor ⇒ `is_dead_verdict` can never fire ⇒ the arm
            // never disarms ⇒ the token is never pruned.
            reserve_sol: 90.0,
            at: ts(secs),
            ..Default::default()
        },
    };

    // Rule 3 is enter-on-arm, so creation submits a buy; confirm it so the token
    // ends up genuinely *holding* across the quiet stretch.
    let fx = reduce(
        &mut s,
        Event::TokenCreated {
            mint: mint.clone(),
            fp: Box::new(TokenFingerprint { cu_limit: Some(200_000), ..Default::default() }),
            at: ts(0.0),
            creator_wallet_hash: None,
            identity: None,
        },
    );
    let intent = fx
        .iter()
        .find_map(|e| match e {
            Effect::SubmitBuy { intent, .. } => Some(intent.clone()),
            _ => None,
        })
        .expect("the enter-on-arm rule buys at birth");
    reduce(&mut s, trade(1.0, 1.0));
    reduce(
        &mut s,
        Event::FillConfirmed {
            intent,
            fill: Fill { price: 1.0, sol: 0.1, token_amount: 1_000_000, at: ts(1.0) },
        },
    );

    // Tick past every clock this rule set reads (widest window 35 s, `time` 45 s,
    // `stall` 25 s, `held` 90 s, dead 300 s), all anchored at t≈1.
    for k in 1..=4000 {
        reduce(&mut s, Event::Tick { now: ts(k as f64 * 0.2) });
    }
    let epoch = s.cross_epoch;
    let token = s.tokens.get(&mint).expect("healthy reserves ⇒ never dead ⇒ never pruned");
    assert!(
        token.tick_is_noop(epoch),
        "a quiet token past every clock must stop being swept — this is the whole optimization",
    );

    // Hours later a print lands. The skipped ticks must have left no stale state
    // behind: a 10x move has to fire rule 3's take_profit (500%) right there.
    let fx = reduce(&mut s, trade(9_000.0, 10.0));
    assert!(
        fx.iter().any(|e| matches!(e, Effect::SubmitSell { .. })),
        "a late trade on a settled token must still be decided on: {fx:?}",
    );
}

/// A settled token that stayed `Armed` only because a cap refused it must wake up
/// when another token's position closes and frees the slot — the cross-token epoch,
/// which no per-token time horizon could express.
#[test]
fn a_freed_cap_slot_wakes_a_settled_token() {
    // One enter-on-arm rule, one slot in total.
    let rule = LoadedRule {
        id: rid(9),
        fingerprint_id: fid(1),
        trade_mode: TradeMode::Paper,
        buy_amount_lamports: 100_000_000,
        max_concurrent_tokens: 1,
        max_total_tokens: 0,
        params: RuleParams::parse(&json!({ "take_profit": 50 })).unwrap(),
        entry_enabled: true,
    };
    let mut s = EngineState::new();
    reduce(
        &mut s,
        Event::RulesReloaded { rules: Arc::from(vec![rule]), fps: Arc::from(vec![cu_fp(1)]) },
    );
    let create = |m: &str, at: f64| Event::TokenCreated {
        mint: Mint::from(m),
        fp: Box::new(TokenFingerprint { cu_limit: Some(200_000), ..Default::default() }),
        at: ts(at),
        creator_wallet_hash: None,
        identity: None,
    };
    // `a` takes the only slot at birth; `b` is refused and stays armed.
    let fx = reduce(&mut s, create("a", 0.0));
    let intent = fx
        .iter()
        .find_map(|e| match e {
            Effect::SubmitBuy { intent, .. } => Some(intent.clone()),
            _ => None,
        })
        .expect("first token takes the slot");
    let fx = reduce(&mut s, create("b", 1.0));
    assert!(
        !fx.iter().any(|e| matches!(e, Effect::SubmitBuy { .. })),
        "the cap must refuse the second token"
    );

    // Let `b` settle: healthy reserves (never dead) and no clock-reading conditions.
    reduce(
        &mut s,
        Event::Trade {
            mint: Mint::from("b"),
            trade: TradeLite {
                side: Side::Buy,
                sol: 1.0,
                price: 1.0,
                reserve_sol: 90.0,
                at: ts(2.0),
                ..Default::default()
            },
        },
    );
    for k in 11..=4000 {
        reduce(&mut s, Event::Tick { now: ts(k as f64 * 0.2) });
    }
    assert!(
        s.tokens[&Mint::from("b")].tick_is_noop(s.cross_epoch),
        "the refused token should have settled",
    );

    // `a`'s entry reverts terminally, rolling its counters back and freeing the slot.
    reduce(
        &mut s,
        Event::FillFailed { intent, reason: FillFailReason::Fatal, at: Some(ts(801.0)) },
    );
    let fx = reduce(&mut s, Event::Tick { now: ts(801.2) });
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::SubmitBuy { mint, .. } if mint.0.as_ref() == "b")),
        "freeing the slot must wake the settled token that was waiting for it: {fx:?}",
    );
}

/// The parity test only means something if the fixture actually settles tokens —
/// otherwise it compares two identical dense runs and passes vacuously. Count the
/// skips the sparse engine takes across the same stream.
#[test]
fn the_parity_fixture_really_exercises_skipping() {
    let events = stream(3, 600);
    let mut s = engine(false);
    let (mut skips, mut swept, mut whole_tick_skips) = (0usize, 0usize, 0usize);
    for e in &events {
        if matches!(e, Event::Tick { .. }) {
            if s.all_tokens_settled() {
                whole_tick_skips += 1;
            }
            let epoch = s.cross_epoch;
            for t in s.tokens.values() {
                if t.tick_is_noop(epoch) {
                    skips += 1;
                } else {
                    swept += 1;
                }
            }
        }
        reduce(&mut s, e.clone());
    }
    assert!(
        skips > 0,
        "the fixture never settles a token, so the parity test proves nothing (skips={skips}, swept={swept})",
    );
    // ...and the O(1) whole-map short circuit has to engage too, or a long quiet
    // stretch still pays one compare per tracked token per tick.
    assert!(
        whole_tick_skips > 0,
        "no tick was skipped wholesale (skips={skips}, swept={swept})",
    );
}
