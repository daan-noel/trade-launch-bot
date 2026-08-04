//! Property tests (plan §3.6) — fuzz the fold with pseudo-random event streams and
//! assert the invariants that must hold *whatever* the input: it never panics,
//! every effect references a live position, and the cap counters stay consistent
//! with the actual arm states (and within their caps).
//!
//! Randomness is a seeded xorshift written inline — the engine's purity guard bans
//! a `rand` dependency in the manifest, and a deterministic generator keeps a
//! failing seed reproducible anyway.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{Duration, TimeZone, Utc};
use hunter_engine::arm::ArmState;
use hunter_engine::cap::Cap;
use hunter_engine::event::{
    Effect, Event, Fill, FillFailReason, IntentId, LoadedRule, Mint, PositionId, RuleId, TradeMode,
};
use hunter_engine::fingerprint::{Fingerprint, FingerprintId};
use hunter_engine::grouping::TokenFingerprint;
use hunter_engine::metrics::{Side, TradeLite, Ts};
use hunter_engine::reduce::reduce;
use hunter_engine::rule_params::RuleParams;
use hunter_engine::EngineState;
use serde_json::json;
use uuid::Uuid;

/// Deterministic xorshift64* PRNG.
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

fn cu_fp(id: u128) -> Fingerprint {
    Fingerprint {
        id: fid(id),
        cu_limit: Some(200_000),
        cu_price: None,
        ix_labels: None,
        init_buy_lamports: None,
        max_cost_lamports: None,
        spendable_lamports_in: None,
        first_slot_buy_lamports: Some(1_000_000_000),
        first_slot_sell_lamports: None,
        bucket_size_amount: Some(0.1),
        metric_config: serde_json::json!({}),
    }
}

/// A varied rule set: an enter-on-arm TP rule (matched via an instant fingerprint),
/// plus a conditional rule behind a first-slot fingerprint with SL + stall exit.
fn rules() -> Vec<LoadedRule> {
    vec![
        LoadedRule {
            id: rid(1),
            fingerprint_id: fid(2), // instant cu-only fingerprint
            trade_mode: TradeMode::Paper,
            buy_amount_lamports: 500_000_000,
            max_concurrent_tokens: 2,
            max_total_tokens: 5,
            params: RuleParams::parse(&json!({ "take_profit": 80 })).unwrap(),
            entry_enabled: true,
        },
        LoadedRule {
            id: rid(2),
            fingerprint_id: fid(1), // first-slot fingerprint
            trade_mode: TradeMode::Real,
            buy_amount_lamports: 1_000_000_000,
            max_concurrent_tokens: 1,
            max_total_tokens: 0,
            params: RuleParams::parse(&json!({
                "stop_loss": 40,
                "entry": { "m_snapshot": { "time": [{ "operator": "<", "value": 60 }] } },
                "exit":  { "m_price_lifetime": { "stall": [{ "operator": ">", "value": 8 }] } }
            }))
            .unwrap(),
            entry_enabled: true,
        },
    ]
}

/// A fingerprint set with both an instant-only and a first-slot fingerprint.
fn fps() -> Vec<Fingerprint> {
    let instant = Fingerprint { first_slot_buy_lamports: None, ..cu_fp(2) };
    vec![cu_fp(1), instant]
}

fn caps() -> BTreeMap<RuleId, (Cap, Cap)> {
    rules()
        .iter()
        .map(|r| (r.id, (r.concurrent_cap(), r.total_cap())))
        .collect()
}

/// Count non-terminal in-flight/held arms per rule — must equal `counters.open`.
fn live_open(s: &EngineState) -> BTreeMap<RuleId, u32> {
    let mut live: BTreeMap<RuleId, u32> = BTreeMap::new();
    for token in s.tokens.values() {
        for (rule, arm) in &token.arms {
            if matches!(
                arm,
                ArmState::EntryPending { .. } | ArmState::Entered(_) | ArmState::ExitPending { .. }
            ) {
                *live.entry(*rule).or_default() += 1;
            }
        }
    }
    live
}

fn check_invariants(s: &EngineState, fx: &[Effect]) {
    // 1. Every trade decision references a currently-live position.
    for e in fx {
        match e {
            Effect::SubmitSell { position, .. } => {
                assert!(s.positions.contains_key(position), "sell references dead position");
            }
            Effect::SubmitBuy { rule, .. } => {
                assert!(s.rules.contains_key(rule), "buy references unknown rule");
            }
            _ => {}
        }
    }
    // 2. Cap counters within their caps.
    let caps = caps();
    for (rule, c) in &s.counters {
        if let Some((cap, max_total)) = caps.get(rule) {
            // `allows(n - 1)` ⇔ `n <= bound`; unlimited always passes.
            assert!(
                c.open == 0 || cap.allows(c.open - 1),
                "open {} > cap {:?} for {rule:?}",
                c.open,
                cap.bounded()
            );
            assert!(
                c.total == 0 || max_total.allows(c.total - 1),
                "total exceeds max_total"
            );
        }
        assert!(c.open <= c.total, "open must never exceed committed total");
    }
    // 3. `open` counter agrees with the actual live arms, and the positions map holds
    //    exactly one entry per live arm.
    let live = live_open(s);
    for (rule, c) in &s.counters {
        assert_eq!(c.open, live.get(rule).copied().unwrap_or(0), "open counter drift for {rule:?}");
    }
    let total_live: u32 = live.values().sum();
    assert_eq!(s.positions.len() as u32, total_live, "positions map size drift");
}

fn ts(secs: f64) -> Ts {
    Utc.timestamp_opt(1_700_000_000, 0).unwrap() + Duration::milliseconds((secs * 1000.0) as i64)
}

#[test]
fn random_streams_preserve_invariants_and_never_panic() {
    for seed in 1..40u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
        let mut s = EngineState::new();
        reduce(&mut s, Event::RulesReloaded { rules: Arc::from(rules()), fps: Arc::from(fps()) });

        let mints: Vec<Mint> = (0..4).map(|i| Mint::from(format!("tok{i}"))).collect();
        // Intents the engine has handed us but we have not yet resolved.
        let mut pending: Vec<IntentId> = Vec::new();
        let mut now = 0.0f64;

        for _ in 0..400 {
            now += rng.frac() * 3.0; // monotonically increasing clock
            let mint = mints[rng.below(mints.len() as u64) as usize].clone();

            let event = match rng.below(9) {
                0 => Event::TokenCreated {
                    mint,
                    fp: Box::new(TokenFingerprint {
                        cu_limit: Some(200_000),
                        ..Default::default()
                    }),
                    at: ts(now),
                    creator_wallet_hash: None,
                },
                1 => Event::FirstSlotSettled {
                    mint,
                    // Sometimes in-bucket (1.0 SOL), sometimes not.
                    buy_lamports: if rng.frac() < 0.5 { 1_050_000_000 } else { 9_000_000_000 },
                    sell_lamports: 0,
                    at: ts(now),
                },
                2 | 3 => Event::Trade {
                    mint,
                    trade: TradeLite {
                        side: if rng.frac() < 0.5 { Side::Buy } else { Side::Sell },
                        sol: rng.frac() * 2.0,
                        price: 0.5 + rng.frac() * 3.0,
                        reserve_sol: rng.frac() * 60.0,
                        at: ts(now),
                        ..Default::default()
                    },
                },
                4 => Event::Tick { now: ts(now) },
                5 => Event::Migrated { mint, at: ts(now) },
                6 => {
                    // Resolve a real pending intent (confirm or fail), or feed garbage.
                    if !pending.is_empty() && rng.frac() < 0.8 {
                        let i = rng.below(pending.len() as u64) as usize;
                        let intent = pending.swap_remove(i);
                        if rng.frac() < 0.6 {
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
                            let reason = match rng.below(3) {
                                0 => FillFailReason::Reverted,
                                1 => FillFailReason::Timeout,
                                2 => FillFailReason::Unconfirmed,
                                _ => FillFailReason::Fatal,
                            };
                            Event::FillFailed { intent, reason }
                        }
                    } else {
                        // Garbage intent — the engine must ignore it.
                        Event::FillFailed {
                            intent: IntentId { rule: rid(1), mint, seq: 99_999 },
                            reason: FillFailReason::Reverted,
                        }
                    }
                }
                7 => {
                    // Manual-close a random known position (or a bogus one).
                    let position = s
                        .positions
                        .keys()
                        .next()
                        .copied()
                        .unwrap_or(PositionId(rng.next()));
                    Event::ManualClose {
                        position,
                        portion: hunter_engine::event::Portion::All,
                    }
                }
                _ => Event::Tick { now: ts(now) },
            };

            let fx = reduce(&mut s, event);
            // Record every fresh intent the engine minted so we can resolve it later.
            for e in &fx {
                match e {
                    Effect::SubmitBuy { intent, .. } | Effect::SubmitSell { intent, .. } => {
                        pending.push(intent.clone())
                    }
                    _ => {}
                }
            }
            check_invariants(&s, &fx);
        }
    }
}
