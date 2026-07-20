//! `reduce` — the one pure fold that *is* the strategy engine. Every arming,
//! entry, exit, disarm, cap, and fill decision is made here and nowhere else
//! (plan §6, §8–11). Live, replay, simulate, and sweep all drive this function;
//! they differ only in who produces the [`Event`]s and who consumes the
//! [`Effect`]s, so identical event streams yield identical effect streams.
//!
//! Determinism: no clock (time rides on events), no randomness (intent/position
//! ids are monotonic counters), and iteration is over sorted keys — so the effect
//! vector for a given event vector is byte-reproducible. The golden-log tests are
//! the spec.
//!
//! Shape: two-phase per (token, rule) — **decide** (pure, immutable reads of the
//! compiled rule + metric track) then **apply** (mutate counters/arm/ids, emit
//! effects). Splitting them keeps the borrows disjoint and the decision logic
//! side-effect-free.

use std::collections::BTreeMap;

use smallvec::SmallVec;

use crate::arm::{ArmState, CompiledRule};
use crate::deadness::{is_dead_verdict, DEAD_MEANINGFUL_TRADE_SOL};
use crate::event::{
    ArmedDelta, ArmedStateTag, DisarmReason, Effect, Event, ExitReason, FillFailReason, Mint,
    PositionDelta, PositionStatus, RuleId,
};
use crate::fingerprint::{match_all, MatchPhase};
use crate::grouping::LAMPORTS_PER_SOL_F64;
use crate::metrics::Ts;
use crate::state::{EngineState, PositionRef, TokenState};

/// Bounded buy retries before an entry gives up (rolling its cap counters back).
const MAX_ENTRY_ATTEMPTS: u32 = 3;
/// Bounded sell retries before an exit is booked `ExitFailed`. Sells retry harder
/// than buys — a stranded bag is worse than a missed entry.
const MAX_EXIT_ATTEMPTS: u32 = 5;

/// The effect buffer one `reduce` call returns.
pub type Effects = SmallVec<[Effect; 8]>;

/// Fold one event into `state`, returning the effects it produced. Infallible:
/// malformed input is rejected at the adapter boundary, never here.
pub fn reduce(state: &mut EngineState, event: Event) -> Effects {
    let mut fx = Effects::new();
    match event {
        Event::RulesReloaded { rules, fps } => {
            state.reload(&rules, &fps);
        }

        Event::TokenCreated { mint, fp, at } => {
            if state.tokens.contains_key(&mint) {
                return fx; // duplicate creation — idempotent
            }
            let mut token = TokenState {
                created_at: at,
                tf: *fp,
                track: state.new_track(at),
                last_meaningful_at: None,
                first_slot_settled: false,
                arms: BTreeMap::new(),
            };
            // Arm every rule whose fingerprint matches the instant axes. A rule whose
            // fingerprint also carries a first-slot axis stays *pending* until the
            // creation slot settles; the rest arm immediately.
            let hits = match_all(&state.fps, &token.tf, MatchPhase::Instant);
            for (rule_id, compiled) in &state.rules {
                if hits.contains(&compiled.fingerprint_id) {
                    let arm = if state.fp_has_first_slot(compiled.fingerprint_id) {
                        ArmState::PendingFirstSlot
                    } else {
                        ArmState::Armed
                    };
                    token.arms.insert(*rule_id, arm);
                }
            }
            for (rule_id, arm) in &token.arms {
                if *arm == ArmState::Armed {
                    fx.push(armed(&mint, *rule_id, ArmedStateTag::Armed));
                }
            }
            // Evaluate now so enter-on-arm rules (and creation-time metrics like
            // `time`/`stall`) can fire at birth.
            evaluate_token(state, &mut token, &mint, at, &mut fx);
            if token.is_active() {
                state.tokens.insert(mint, token);
            }
        }

        Event::FirstSlotSettled { mint, buy_lamports, sell_lamports, at } => {
            let Some(mut token) = state.tokens.remove(&mint) else { return fx };
            if !token.first_slot_settled {
                token.first_slot_settled = true;
                token.tf.first_slot_buy_sol = Some(buy_lamports as f64 / LAMPORTS_PER_SOL_F64);
                token.tf.first_slot_sell_sol = Some(sell_lamports as f64 / LAMPORTS_PER_SOL_F64);
                let hits = match_all(&state.fps, &token.tf, MatchPhase::Full);
                let pending: SmallVec<[RuleId; 4]> = token
                    .arms
                    .iter()
                    .filter(|(_, a)| **a == ArmState::PendingFirstSlot)
                    .map(|(id, _)| *id)
                    .collect();
                for rule_id in pending {
                    let matched = state
                        .rules
                        .get(&rule_id)
                        .is_some_and(|c| hits.contains(&c.fingerprint_id));
                    if matched {
                        token.arms.insert(rule_id, ArmState::Armed);
                        fx.push(armed(&mint, rule_id, ArmedStateTag::Armed));
                    } else {
                        token.arms.remove(&rule_id); // never fully matched → drop
                    }
                }
                evaluate_token(state, &mut token, &mint, at, &mut fx);
            }
            if token.is_active() {
                state.tokens.insert(mint, token);
            }
        }

        Event::Trade { mint, trade } => {
            let Some(mut token) = state.tokens.remove(&mint) else { return fx };
            token.track.on_trade(trade);
            if trade.sol >= DEAD_MEANINGFUL_TRADE_SOL
                && token.last_meaningful_at.is_none_or(|t| trade.at >= t)
            {
                token.last_meaningful_at = Some(trade.at);
            }
            evaluate_token(state, &mut token, &mint, trade.at, &mut fx);
            if token.is_active() {
                state.tokens.insert(mint, token);
            }
        }

        Event::Tick { now } => {
            let mints: SmallVec<[Mint; 16]> = state.tokens.keys().cloned().collect();
            for mint in mints {
                let Some(mut token) = state.tokens.remove(&mint) else { continue };
                token.track.on_tick(now);
                evaluate_token(state, &mut token, &mint, now, &mut fx);
                if token.is_active() {
                    state.tokens.insert(mint, token);
                }
            }
        }

        Event::FillConfirmed { intent, fill } => {
            let mint = intent.mint.clone();
            let rule_id = intent.rule;
            let Some(mut token) = state.tokens.remove(&mint) else { return fx };
            match token.arms.get(&rule_id).cloned() {
                Some(ArmState::EntryPending { intent: pend, position, .. }) if pend == intent => {
                    token.arms.insert(
                        rule_id,
                        ArmState::Entered { position, entry_price: fill.price },
                    );
                    fx.push(Effect::PositionUpdate(PositionDelta {
                        position,
                        rule: rule_id,
                        mint: mint.clone(),
                        status: PositionStatus::Holding,
                        fill: Some(fill),
                        reason: None,
                        intent: Some(intent),
                    }));
                }
                Some(ArmState::ExitPending { intent: pend, position, reason, .. })
                    if pend == intent =>
                {
                    decrement_open(state, rule_id);
                    state.positions.remove(&position);
                    token.arms.insert(rule_id, ArmState::Done);
                    fx.push(Effect::PositionUpdate(PositionDelta {
                        position,
                        rule: rule_id,
                        mint: mint.clone(),
                        status: PositionStatus::End,
                        fill: Some(fill),
                        reason: Some(reason),
                        intent: Some(intent),
                    }));
                }
                _ => {} // stale / unknown intent — ignore
            }
            if token.is_active() {
                state.tokens.insert(mint, token);
            }
        }

        Event::FillFailed { intent, reason } => {
            let mint = intent.mint.clone();
            let rule_id = intent.rule;
            let Some(mut token) = state.tokens.remove(&mint) else { return fx };
            match token.arms.get(&rule_id).cloned() {
                Some(ArmState::EntryPending { intent: pend, position, attempts })
                    if pend == intent =>
                {
                    // Fatal = structural / StopFeeBurn — never burn fee retries.
                    let retry = reason != FillFailReason::Fatal && attempts < MAX_ENTRY_ATTEMPTS;
                    if retry {
                        let next = state.next_intent(rule_id, mint.clone());
                        let lamports =
                            state.rules.get(&rule_id).map(|c| c.buy_amount_lamports).unwrap_or(0);
                        token.arms.insert(
                            rule_id,
                            ArmState::EntryPending {
                                intent: next.clone(),
                                position,
                                attempts: attempts + 1,
                            },
                        );
                        fx.push(Effect::SubmitBuy {
                            intent: next,
                            rule: rule_id,
                            mint: mint.clone(),
                            lamports,
                        });
                    } else {
                        // Never entered — roll the cap counters back and drop it.
                        rollback_entry(state, rule_id);
                        state.positions.remove(&position);
                        token.arms.insert(rule_id, ArmState::Done);
                        fx.push(Effect::PositionUpdate(PositionDelta {
                            position,
                            rule: rule_id,
                            mint: mint.clone(),
                            status: PositionStatus::ExitFailed,
                            fill: None,
                            reason: None,
                            intent: Some(intent),
                        }));
                    }
                }
                Some(ArmState::ExitPending {
                    intent: pend,
                    position,
                    reason: exit_reason,
                    attempts,
                }) if pend == intent => {
                    if reason == FillFailReason::Unconfirmed {
                        // May have cleared — never re-sell; alarm for manual review.
                        decrement_open(state, rule_id);
                        state.positions.remove(&position);
                        token.arms.insert(rule_id, ArmState::Done);
                        fx.push(Effect::PositionUpdate(PositionDelta {
                            position,
                            rule: rule_id,
                            mint: mint.clone(),
                            status: PositionStatus::ExitUnconfirmed,
                            fill: None,
                            reason: Some(exit_reason),
                            intent: Some(intent),
                        }));
                    } else if reason == FillFailReason::Fatal || attempts >= MAX_EXIT_ATTEMPTS {
                        decrement_open(state, rule_id);
                        state.positions.remove(&position);
                        token.arms.insert(rule_id, ArmState::Done);
                        fx.push(Effect::PositionUpdate(PositionDelta {
                            position,
                            rule: rule_id,
                            mint: mint.clone(),
                            status: PositionStatus::ExitFailed,
                            fill: None,
                            reason: Some(exit_reason),
                            intent: Some(intent),
                        }));
                    } else {
                        let next = state.next_intent(rule_id, mint.clone());
                        token.arms.insert(
                            rule_id,
                            ArmState::ExitPending {
                                intent: next.clone(),
                                position,
                                reason: exit_reason,
                                attempts: attempts + 1,
                            },
                        );
                        fx.push(Effect::SubmitSell { intent: next, position, reason: exit_reason });
                    }
                }
                _ => {}
            }
            if token.is_active() {
                state.tokens.insert(mint, token);
            }
        }

        Event::ManualClose { position } => {
            let Some(pref) = state.positions.get(&position).cloned() else { return fx };
            let PositionRef { mint, rule } = pref;
            let Some(mut token) = state.tokens.remove(&mint) else { return fx };
            if let Some(ArmState::Entered { position: pos, .. }) = token.arms.get(&rule).cloned() {
                if pos == position {
                    let intent = state.next_intent(rule, mint.clone());
                    token.arms.insert(
                        rule,
                        ArmState::ExitPending {
                            intent: intent.clone(),
                            position,
                            reason: ExitReason::Manual,
                            attempts: 1,
                        },
                    );
                    fx.push(Effect::SubmitSell {
                        intent: intent.clone(),
                        position,
                        reason: ExitReason::Manual,
                    });
                    fx.push(Effect::PositionUpdate(PositionDelta {
                        position,
                        rule,
                        mint: mint.clone(),
                        status: PositionStatus::ExitPending,
                        fill: None,
                        reason: Some(ExitReason::Manual),
                        intent: Some(intent),
                    }));
                }
            }
            if token.is_active() {
                state.tokens.insert(mint, token);
            }
        }

        Event::ExternallyCleared { position, fill } => {
            let Some(pref) = state.positions.get(&position).cloned() else { return fx };
            let PositionRef { mint, rule } = pref;
            let Some(mut token) = state.tokens.remove(&mint) else { return fx };
            if let Some(ArmState::Entered { position: pos, .. }) = token.arms.get(&rule).cloned() {
                if pos == position {
                    // The bag is already gone → close terminally at the resolved fill.
                    // No `SubmitSell` (the twin of `ManualClose`, minus the sell).
                    decrement_open(state, rule);
                    state.positions.remove(&position);
                    token.arms.insert(rule, ArmState::Done);
                    fx.push(Effect::PositionUpdate(PositionDelta {
                        position,
                        rule,
                        mint: mint.clone(),
                        status: PositionStatus::End,
                        fill: Some(fill),
                        reason: Some(ExitReason::Manual),
                        intent: None,
                    }));
                }
            }
            if token.is_active() {
                state.tokens.insert(mint, token);
            }
        }

        Event::Migrated { mint, at: _ } => {
            let Some(mut token) = state.tokens.remove(&mint) else { return fx };
            let rule_ids: SmallVec<[RuleId; 4]> = token.arms.keys().copied().collect();
            for rule_id in rule_ids {
                match token.arms.get(&rule_id) {
                    Some(ArmState::Armed) | Some(ArmState::PendingFirstSlot) => {
                        token.arms.insert(rule_id, ArmState::Disarmed(DisarmReason::Migrated));
                        fx.push(armed(
                            &mint,
                            rule_id,
                            ArmedStateTag::Disarmed(DisarmReason::Migrated),
                        ));
                    }
                    // Open positions ride migration out — AMM trades keep pricing them.
                    _ => {}
                }
            }
            if token.is_active() {
                state.tokens.insert(mint, token);
            }
        }
    }
    fx
}

/// The pure per-(token, rule) verdict for a trade/tick sweep. No mutation.
enum ArmDecision {
    None,
    Disarm(DisarmReason),
    Enter,
    Exit(ExitReason),
}

/// Sweep every arm on one token at `now`, deciding then applying. Used by
/// `TokenCreated`, `FirstSlotSettled`, `Trade`, and `Tick`.
fn evaluate_token(
    state: &mut EngineState,
    token: &mut TokenState,
    mint: &Mint,
    now: Ts,
    fx: &mut Effects,
) {
    // The dead-token verdict is a token-wide fact — compute it once, reuse per arm.
    let reserves = {
        let r = token.track.current_reserves();
        r.is_finite().then_some(r)
    };
    let last_meaningful = token.last_meaningful_at.unwrap_or(token.created_at);
    let dead = is_dead_verdict(reserves, last_meaningful, now);

    let rule_ids: SmallVec<[RuleId; 4]> = token.arms.keys().copied().collect();
    for rule_id in rule_ids {
        let (decision, buy_lamports, cap, max_total) = {
            let Some(c) = state.rules.get(&rule_id) else { continue };
            let arm = &token.arms[&rule_id];
            (
                decide_arm(c, arm, token, dead, now),
                c.buy_amount_lamports,
                c.concurrent_cap,
                c.max_total,
            )
        };
        apply_decision(state, token, mint, rule_id, decision, buy_lamports, cap, max_total, fx);
    }
}

/// Decide one arm's fate. Priorities: armed side disarms (dead, then derived-unsat)
/// before it enters; the open side follows `Dead > StopLoss > TakeProfit > Metrics`.
fn decide_arm(
    c: &CompiledRule,
    arm: &ArmState,
    token: &TokenState,
    dead: bool,
    now: Ts,
) -> ArmDecision {
    match arm {
        ArmState::Armed => {
            if dead {
                return ArmDecision::Disarm(DisarmReason::Dead);
            }
            if c.entry_unsatisfiable(&token.track, now) {
                return ArmDecision::Disarm(DisarmReason::Unsatisfiable);
            }
            if c.enter_on_arm() || c.entry_satisfied(&token.track, now) {
                return ArmDecision::Enter;
            }
            ArmDecision::None
        }
        ArmState::Entered { entry_price, .. } => {
            if dead {
                return ArmDecision::Exit(ExitReason::Dead);
            }
            let price = token.track.current_price();
            if price.is_finite() {
                if let Some(sl) = c.stop_loss {
                    if price <= *entry_price * (1.0 - sl / 100.0) {
                        return ArmDecision::Exit(ExitReason::StopLoss);
                    }
                }
                if let Some(tp) = c.take_profit {
                    if price >= *entry_price * (1.0 + tp / 100.0) {
                        return ArmDecision::Exit(ExitReason::TakeProfit);
                    }
                }
            }
            if c.exit_metrics_satisfied(&token.track, now) {
                return ArmDecision::Exit(ExitReason::Metrics);
            }
            ArmDecision::None
        }
        // Pending / in-flight / terminal arms make no sweep decision.
        _ => ArmDecision::None,
    }
}

/// Apply a decision: mutate cap counters + arm state + ids, emit effects.
#[allow(clippy::too_many_arguments)]
fn apply_decision(
    state: &mut EngineState,
    token: &mut TokenState,
    mint: &Mint,
    rule_id: RuleId,
    decision: ArmDecision,
    buy_lamports: u64,
    cap: u32,
    max_total: u32,
    fx: &mut Effects,
) {
    match decision {
        ArmDecision::None => {}
        ArmDecision::Disarm(reason) => {
            token.arms.insert(rule_id, ArmState::Disarmed(reason));
            fx.push(armed(mint, rule_id, ArmedStateTag::Disarmed(reason)));
        }
        ArmDecision::Enter => {
            // Caps enforced at entry (not arm): concurrency + lifetime. Over a cap ⇒
            // wait (stay armed) and re-check on the next event.
            {
                let counters = state.counters.entry(rule_id).or_default();
                if counters.open >= cap {
                    return;
                }
                if max_total != 0 && counters.total >= max_total {
                    return;
                }
                counters.open += 1;
                counters.total += 1;
            }
            let position = state.next_position();
            let intent = state.next_intent(rule_id, mint.clone());
            state.positions.insert(position, PositionRef { mint: mint.clone(), rule: rule_id });
            token.arms.insert(
                rule_id,
                ArmState::EntryPending { intent: intent.clone(), position, attempts: 1 },
            );
            fx.push(Effect::SubmitBuy {
                intent: intent.clone(),
                rule: rule_id,
                mint: mint.clone(),
                lamports: buy_lamports,
            });
            fx.push(Effect::PositionUpdate(PositionDelta {
                position,
                rule: rule_id,
                mint: mint.clone(),
                status: PositionStatus::BuySubmitted,
                fill: None,
                reason: None,
                intent: Some(intent),
            }));
        }
        ArmDecision::Exit(reason) => {
            let Some(ArmState::Entered { position, .. }) = token.arms.get(&rule_id).cloned() else {
                return;
            };
            let intent = state.next_intent(rule_id, mint.clone());
            token.arms.insert(
                rule_id,
                ArmState::ExitPending { intent: intent.clone(), position, reason, attempts: 1 },
            );
            fx.push(Effect::SubmitSell { intent: intent.clone(), position, reason });
            fx.push(Effect::PositionUpdate(PositionDelta {
                position,
                rule: rule_id,
                mint: mint.clone(),
                status: PositionStatus::ExitPending,
                fill: None,
                reason: Some(reason),
                intent: Some(intent),
            }));
        }
    }
}

/// Decrement a rule's open counter on a position close (saturating).
fn decrement_open(state: &mut EngineState, rule: RuleId) {
    if let Some(c) = state.counters.get_mut(&rule) {
        c.open = c.open.saturating_sub(1);
    }
}

/// Roll back both counters for an entry that never filled (saturating).
fn rollback_entry(state: &mut EngineState, rule: RuleId) {
    if let Some(c) = state.counters.get_mut(&rule) {
        c.open = c.open.saturating_sub(1);
        c.total = c.total.saturating_sub(1);
    }
}

/// Build an `ArmedChanged` effect.
fn armed(mint: &Mint, rule: RuleId, state: ArmedStateTag) -> Effect {
    Effect::ArmedChanged(ArmedDelta { mint: mint.clone(), rule, state })
}
