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

use crate::arm::{ArmState, CompiledRule, EnteredCtx};
use crate::cap::Cap;
use crate::deadness::{is_dead_verdict, DEAD_MEANINGFUL_TRADE_SOL};
use crate::event::{
    ArmedDelta, ArmedStateTag, DisarmReason, Effect, Event, ExitReason, FillFailReason, Mint,
    Portion, PositionDelta, PositionStatus, RuleId,
};
use crate::fingerprint::{match_all, MatchPhase};
use crate::grouping::{TokenFingerprint, LAMPORTS_PER_SOL_F64};
use crate::metrics::Ts;
use crate::state::{EngineState, PositionRef, TokenState};

/// Bounded buy retries before an entry gives up (rolling its cap counters back).
const MAX_ENTRY_ATTEMPTS: u32 = 3;
/// Bounded sell retries before an exit is booked `ExitStuck` (bag still held,
/// reaper takes over). Sells retry harder than buys — a stranded bag is worse
/// than a missed entry.
const MAX_EXIT_ATTEMPTS: u32 = 5;

/// The effect buffer one `reduce` call returns.
pub type Effects = SmallVec<[Effect; 8]>;

/// Fold one event into `state`, returning the effects it produced. Infallible:
/// malformed input is rejected at the adapter boundary, never here.
pub fn reduce(state: &mut EngineState, event: Event) -> Effects {
    let mut fx = Effects::new();
    match event {
        Event::RulesReloaded { rules, fps } => {
            let incoming: std::collections::HashMap<_, _> =
                rules.iter().map(|r| (r.id, r.entry_enabled)).collect();
            for (mint, token) in state.tokens.iter_mut() {
                let stale: smallvec::SmallVec<[RuleId; 4]> = token
                    .arms
                    .iter()
                    .filter(|(rid, arm)| {
                        if arm_holds_position(arm) {
                            return false;
                        }
                        match incoming.get(rid) {
                            None => true,
                            Some(entry_enabled) => !entry_enabled,
                        }
                    })
                    .map(|(id, _)| *id)
                    .collect();
                for rid in stale {
                    token.arms.insert(rid, ArmState::Disarmed(DisarmReason::Paused));
                    fx.push(armed(mint, rid, ArmedStateTag::Disarmed(DisarmReason::Paused)));
                }
            }
            state.reload(&rules, &fps);
        }

        Event::TokenCreated { mint, fp, at, creator_wallet_hash } => {
            if state.tokens.contains_key(&mint) {
                return fx; // duplicate creation — idempotent
            }
            let mut track = state.new_track(at);
            if let Some(h) = creator_wallet_hash {
                track.seed_creator(h);
            }
            let mut token = TokenState {
                created_at: at,
                tf: *fp,
                track,
                last_meaningful_at: None,
                first_slot_settled: false,
                arms: BTreeMap::new(),
                episodes: BTreeMap::new(),
            };
            // Arm every rule whose fingerprint matches the instant axes. A rule whose
            // fingerprint also carries a first-slot axis stays *pending* until the
            // creation slot settles; the rest arm immediately.
            let hits = match_all(&state.fps, &token.tf, MatchPhase::Instant);
            for (rule_id, compiled) in &state.rules {
                if !compiled.entry_enabled {
                    continue;
                }
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
                    let Some(compiled) = state.rules.get(&rule_id) else { continue };
                    if !compiled.entry_enabled {
                        continue;
                    }
                    let matched = hits.contains(&compiled.fingerprint_id);
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
            fold_trade(&mut token, trade);
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
                    // Peak/trough start at the fill: before any run-up
                    // `retrace` measures the drop from entry (a soft stop);
                    // before any dip `bounce` equals `pnl`.
                    token.arms.insert(
                        rule_id,
                        ArmState::Entered(EnteredCtx::at_fill(position, fill.price, fill.at)),
                    );
                    fx.push(Effect::PositionUpdate(PositionDelta {
                        position,
                        rule: rule_id,
                        mint: mint.clone(),
                        status: PositionStatus::Holding,
                        fill: Some(fill),
                        reason: None,
                        intent: Some(intent),
                        stage: Some(0),
                    }));
                }
                Some(ArmState::ExitPending {
                    intent: pend,
                    reason,
                    portion,
                    held,
                    ..
                }) if pend == intent =>
                {
                    if portion.is_partial() {
                        // Partial fill: advance stage, keep bag open, resume peak/trough.
                        let bps = match portion {
                            Portion::BpsOfInitial(b) => b,
                            Portion::All => 0, // unreachable via is_partial
                        };
                        let mut next = held;
                        next.stage = next.stage.saturating_add(1);
                        next.sold_bps = next.sold_bps.saturating_add(bps);
                        let stage = next.stage;
                        let position = next.position;
                        token.arms.insert(rule_id, ArmState::Entered(next));
                        fx.push(Effect::PositionUpdate(PositionDelta {
                            position,
                            rule: rule_id,
                            mint: mint.clone(),
                            status: PositionStatus::Holding,
                            fill: Some(fill),
                            reason: Some(reason),
                            intent: Some(intent),
                            stage: Some(stage),
                        }));
                    } else {
                        // Full / remainder close.
                        let position = held.position;
                        decrement_open(state, rule_id);
                        state.remove_position(position);
                        // Re-arm into a cooldown if the rule allows it (else terminal Done).
                        let next =
                            rearm_after_close(state, &mut token.episodes, rule_id, reason, fill.at);
                        token.arms.insert(rule_id, next);
                        fx.push(Effect::PositionUpdate(PositionDelta {
                            position,
                            rule: rule_id,
                            mint: mint.clone(),
                            status: PositionStatus::End,
                            fill: Some(fill),
                            reason: Some(reason),
                            intent: Some(intent),
                            stage: None,
                        }));
                    }
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
                Some(ArmState::EntryPending { intent: pend, position, attempts, lamports })
                    if pend == intent =>
                {
                    // Retry size: frozen at submit on the arm; `0` (a boot-adopted
                    // arm) falls back to the rule's configured amount. No amount
                    // resolvable ⇒ no blind resend.
                    let retry_lamports = if lamports > 0 {
                        lamports
                    } else {
                        state.rules.get(&rule_id).map(|c| c.buy_amount_lamports).unwrap_or(0)
                    };
                    // Fatal = structural / StopFeeBurn — never burn fee retries.
                    let retry = reason != FillFailReason::Fatal
                        && attempts < MAX_ENTRY_ATTEMPTS
                        && retry_lamports > 0;
                    if retry {
                        let next = state.next_intent(rule_id, mint.clone());
                        token.arms.insert(
                            rule_id,
                            ArmState::EntryPending {
                                intent: next.clone(),
                                position,
                                attempts: attempts + 1,
                                lamports: retry_lamports,
                            },
                        );
                        fx.push(Effect::SubmitBuy {
                            intent: next,
                            rule: rule_id,
                            mint: mint.clone(),
                            lamports: retry_lamports,
                        });
                    } else {
                        // Never entered — roll the cap counters back and drop it.
                        rollback_entry(state, rule_id);
                        state.remove_position(position);
                        token.arms.insert(rule_id, ArmState::Done);
                        fx.push(Effect::PositionUpdate(PositionDelta {
                            position,
                            rule: rule_id,
                            mint: mint.clone(),
                            status: PositionStatus::EntryFailed,
                            fill: None,
                            reason: None,
                            intent: Some(intent),
                            stage: None,
                        }));
                    }
                }
                Some(ArmState::ExitPending {
                    intent: pend,
                    reason: exit_reason,
                    attempts,
                    portion,
                    held,
                }) if pend == intent => {
                    let position = held.position;
                    if reason == FillFailReason::Unconfirmed {
                        // May have cleared — never re-sell; alarm for manual review.
                        decrement_open(state, rule_id);
                        state.remove_position(position);
                        token.arms.insert(rule_id, ArmState::Done);
                        fx.push(Effect::PositionUpdate(PositionDelta {
                            position,
                            rule: rule_id,
                            mint: mint.clone(),
                            status: PositionStatus::ExitUnconfirmed,
                            fill: None,
                            reason: Some(exit_reason),
                            intent: Some(intent),
                            stage: None,
                        }));
                    } else if reason == FillFailReason::Fatal || attempts >= MAX_EXIT_ATTEMPTS {
                        // Sell gave up but the bag is still held — the position
                        // stays open in PG (`ExitStuck`); the reaper owns it now.
                        // Same for a partial-leg exhaust (bag = the remainder).
                        decrement_open(state, rule_id);
                        state.remove_position(position);
                        token.arms.insert(rule_id, ArmState::Done);
                        fx.push(Effect::PositionUpdate(PositionDelta {
                            position,
                            rule: rule_id,
                            mint: mint.clone(),
                            status: PositionStatus::ExitStuck,
                            fill: None,
                            reason: Some(exit_reason),
                            intent: Some(intent),
                            stage: None,
                        }));
                    } else {
                        let next = state.next_intent(rule_id, mint.clone());
                        token.arms.insert(
                            rule_id,
                            ArmState::ExitPending {
                                intent: next.clone(),
                                reason: exit_reason,
                                attempts: attempts + 1,
                                portion,
                                held,
                            },
                        );
                        fx.push(Effect::SubmitSell {
                            intent: next,
                            position,
                            reason: exit_reason,
                            portion,
                        });
                    }
                }
                _ => {}
            }
            if token.is_active() {
                state.tokens.insert(mint, token);
            }
        }

        Event::ManualBuy { mint, rule, lamports, at, exit } => {
            if lamports == 0 {
                return fx;
            }
            // Ensure a token state exists — a manual buy may target a mint the
            // engine never armed (or one it already pruned). A bare track folds
            // prices forward from subsequent trades; first-slot is irrelevant here.
            if !state.tokens.contains_key(&mint) {
                let track = state.new_track(at);
                state.tokens.insert(
                    mint.clone(),
                    TokenState {
                        created_at: at,
                        tf: TokenFingerprint::default(),
                        track,
                        last_meaningful_at: None,
                        first_slot_settled: true,
                        arms: BTreeMap::new(),
                        episodes: BTreeMap::new(),
                    },
                );
            }
            // The adapter mints a fresh per-episode rule id, so a collision here
            // means a duplicate command — idempotent no-op.
            if state.tokens.get(&mint).is_some_and(|t| t.arms.contains_key(&rule)) {
                return fx;
            }
            let position = state.next_position();
            let intent = state.next_intent(rule, mint.clone());
            state.positions.insert(position, PositionRef { mint: mint.clone(), rule });
            // TP/SL config compiles into a one-off exit rule (full engine exit
            // stack incl. Dead); absent ⇒ tracked-only, no auto-exit of any kind.
            state.set_manual_exit(position, rule, exit);
            let Some(token) = state.tokens.get_mut(&mint) else { return fx };
            token.arms.insert(
                rule,
                ArmState::EntryPending { intent: intent.clone(), position, attempts: 1, lamports },
            );
            fx.push(Effect::SubmitBuy {
                intent: intent.clone(),
                rule,
                mint: mint.clone(),
                lamports,
            });
            fx.push(Effect::PositionUpdate(PositionDelta {
                position,
                rule,
                mint: mint.clone(),
                status: PositionStatus::BuySubmitted,
                fill: None,
                reason: None,
                intent: Some(intent),
                stage: None,
            }));
        }

        Event::SetManualExit { position, exit } => {
            let Some(pref) = state.positions.get(&position).cloned() else { return fx };
            state.set_manual_exit(position, pref.rule, exit);
        }

        Event::ManualClose { position, portion } => {
            let Some(pref) = state.positions.get(&position).cloned() else { return fx };
            let PositionRef { mint, rule } = pref;
            let Some(mut token) = state.tokens.remove(&mint) else { return fx };
            if let Some(ArmState::EntryPending { intent: _, position: pos, .. }) =
                token.arms.get(&rule).cloned()
            {
                if pos == position {
                    rollback_entry(state, rule);
                    state.remove_position(position);
                    token.arms.insert(rule, ArmState::Done);
                    fx.push(Effect::PositionUpdate(PositionDelta {
                        position,
                        rule,
                        mint: mint.clone(),
                        status: PositionStatus::EntryFailed,
                        fill: None,
                        reason: Some(ExitReason::Manual),
                        intent: None,
                        stage: None,
                    }));
                }
            } else if let Some(ArmState::Entered(held)) = token.arms.get(&rule).cloned() {
                if held.position == position {
                    let intent = state.next_intent(rule, mint.clone());
                    // Same ExitPending + SubmitSell shape as PartialExit / full
                    // Exit — exec/sinks already size from `portion`. A partial
                    // Manual fill advances stage like a ladder leg (pure reuse).
                    token.arms.insert(
                        rule,
                        ArmState::ExitPending {
                            intent: intent.clone(),
                            reason: ExitReason::Manual,
                            attempts: 1,
                            portion,
                            held,
                        },
                    );
                    fx.push(Effect::SubmitSell {
                        intent: intent.clone(),
                        position,
                        reason: ExitReason::Manual,
                        portion,
                    });
                    fx.push(Effect::PositionUpdate(PositionDelta {
                        position,
                        rule,
                        mint: mint.clone(),
                        status: PositionStatus::ExitPending,
                        fill: None,
                        reason: Some(ExitReason::Manual),
                        intent: Some(intent),
                        stage: None,
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
            if let Some(ArmState::Entered(held)) = token.arms.get(&rule).cloned() {
                if held.position == position {
                    // The bag is already gone → close terminally at the resolved fill.
                    // No `SubmitSell` (the twin of `ManualClose`, minus the sell).
                    decrement_open(state, rule);
                    state.remove_position(position);
                    token.arms.insert(rule, ArmState::Done);
                    fx.push(Effect::PositionUpdate(PositionDelta {
                        position,
                        rule,
                        mint: mint.clone(),
                        status: PositionStatus::End,
                        fill: Some(fill),
                        reason: Some(ExitReason::Manual),
                        intent: None,
                        stage: None,
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
                    // Armed / pending / cooling-down (a re-entry arm awaiting re-arm)
                    // all disarm at migration — there is no more curve to trade.
                    Some(ArmState::Armed)
                    | Some(ArmState::PendingFirstSlot)
                    | Some(ArmState::Cooldown { .. }) => {
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
    /// Scale-out leg: sell `sell_bps` of the initial bag; fill restores Entered.
    PartialExit { reason: ExitReason, sell_bps: u16 },
}

/// Sweep every arm on one token at `now`, deciding then applying. Used by
/// `TokenCreated`, `FirstSlotSettled`, `Trade`, and `Tick`.
/// Fold one trade into a token's metric state — the **observation** half of
/// [`Event::Trade`], with no decision. Shared by the live fold and
/// [`prime_trade`] so a primed trade and a decided trade can never leave the
/// track in different shapes.
fn fold_trade(token: &mut TokenState, trade: crate::metrics::TradeLite) {
    token.track.on_trade(trade);
    if trade.sol >= DEAD_MEANINGFUL_TRADE_SOL
        && token.last_meaningful_at.is_none_or(|t| trade.at >= t)
    {
        token.last_meaningful_at = Some(trade.at);
    }
}

/// Ratchet every held position's since-entry peak/trough to the track's current
/// price. Two bare compares per `Entered` arm, no alloc.
fn fold_entered_extremes(token: &mut TokenState) {
    let cur_price = token.track.current_price();
    if !cur_price.is_finite() {
        return;
    }
    for arm in token.arms.values_mut() {
        if let ArmState::Entered(ctx) = arm {
            if cur_price > ctx.peak_price {
                ctx.peak_price = cur_price;
            }
            if cur_price < ctx.trough_price {
                ctx.trough_price = cur_price;
            }
        }
    }
}

/// **Observe** a trade without deciding on it — the boot/backfill path.
///
/// A trade that happened before this process started is history: it must shape
/// the metric track (price, windows, peak/trough, the deadness clock) but must
/// never be handed to [`evaluate_token`], because an entry or exit decided at a
/// stale price is executed at the live one. That mismatch sold a real position's
/// last leg on a `stop_loss` that was true five minutes earlier
/// (2026-08-06, `247PRAda…`) — the fill printed **+79%** against its own stop.
///
/// Decisions are not lost, only deferred: the engine's 200 ms `Tick` re-evaluates
/// every token against the freshly primed track and the wall clock.
///
/// Emits no effects **by construction** (no return value to discard) — priming a
/// closed/untracked mint is a no-op.
pub fn prime_trade(state: &mut EngineState, mint: &Mint, trade: crate::metrics::TradeLite) {
    let Some(token) = state.tokens.get_mut(mint) else { return };
    fold_trade(token, trade);
    fold_entered_extremes(token);
}

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

    // Fold the current price into every held position's since-entry peak/trough
    // BEFORE deciding — two bare compares per Entered arm, no alloc — so
    // `retrace`/`bounce` already include this event's price.
    fold_entered_extremes(token);

    // Re-entry: promote any cooldown arm whose window has elapsed back to Armed, so
    // the same event's decide loop below can re-enter on a fresh signal. Sorted
    // iteration (BTreeMap) keeps the emitted `ArmedChanged` order deterministic.
    for (rule_id, arm) in token.arms.iter_mut() {
        if let ArmState::Cooldown { until } = arm {
            if now >= *until
                && state.rules.get(rule_id).is_some_and(|c| c.entry_enabled)
            {
                *arm = ArmState::Armed;
                fx.push(armed(mint, *rule_id, ArmedStateTag::Armed));
            }
        }
    }

    let mut rule_ids: SmallVec<[RuleId; 4]> = token.arms.keys().copied().collect();
    // `priority` is NOT a general scheduling concept — it only ever changes behavior
    // when two `exclusive` rules want the same token on the same event. Arms are
    // decided-and-applied one at a time below, so whoever is visited first is already
    // `EntryPending` when the next arm is decided and therefore wins the claim. A
    // non-exclusive rule never reads another arm, so reordering it is a no-op.
    // Manual arms (no entry in `rules`) sort at the default 0.
    rule_ids.sort_unstable_by_key(|id| {
        (
            std::cmp::Reverse(state.rules.get(id).map_or(0, |c| c.priority)),
            *id,
        )
    });
    for rule_id in rule_ids {
        let (decision, buy_lamports, cap, max_total) = {
            let arm = &token.arms[&rule_id];
            // Manual episodes resolve via their per-position exit rule; a
            // tracked-only manual arm has neither ⇒ no decision (no auto-exit).
            let Some(c) = state.rule_for(rule_id, arm_position(arm)) else { continue };
            (
                decide_arm(c, rule_id, arm, token, dead, now),
                c.buy_amount_lamports,
                c.concurrent_cap,
                c.max_total,
            )
        };
        apply_decision(state, token, mint, rule_id, decision, buy_lamports, cap, max_total, fx);
    }
}

/// The position an arm owns, when it owns one (keys the manual exit-rule lookup).
fn arm_position(arm: &ArmState) -> Option<crate::event::PositionId> {
    match arm {
        ArmState::EntryPending { position, .. } => Some(*position),
        ArmState::Entered(ctx) | ArmState::ExitPending { held: ctx, .. } => Some(ctx.position),
        _ => None,
    }
}

/// Decide one arm's fate. Priorities: armed side disarms (dead, then derived-unsat)
/// before it enters; an `exclusive` rule then stands down while another arm holds the
/// token; entry is gated by [`CompiledRule::can_enter`] (no buy while exit metrics
/// already hold); the open side follows `Dead > exit_fired > stage_fired`, where
/// `exit_fired` walks the desugared-TP/SL-then-authored exit reqs in order (so the
/// legacy `SL > TP > Metrics` tiebreak is preserved inside the one loop) and stages
/// only fire when no global exit did.
fn decide_arm(
    c: &CompiledRule,
    rule_id: RuleId,
    arm: &ArmState,
    token: &TokenState,
    dead: bool,
    now: Ts,
) -> ArmDecision {
    match arm {
        ArmState::Armed => {
            if !c.entry_enabled {
                return ArmDecision::None;
            }
            if dead {
                return ArmDecision::Disarm(DisarmReason::Dead);
            }
            if c.entry_unsatisfiable(&token.track, now) {
                return ArmDecision::Disarm(DisarmReason::Unsatisfiable);
            }
            // Exclusivity: any other arm holding the token (in-flight buy/sell
            // included, manual arms included) blocks this entry. Stays `Armed` so it
            // retries once the holder lets go — deliberately not a Disarm.
            if c.exclusive
                && token
                    .arms
                    .iter()
                    .any(|(rid, a)| *rid != rule_id && arm_position(a).is_some())
            {
                return ArmDecision::None;
            }
            if c.can_enter(&token.track, now) {
                return ArmDecision::Enter;
            }
            ArmDecision::None
        }
        ArmState::Entered(held) => {
            // Dead stays first and special (liquidity-based, not price). Every
            // price-based exit — the desugared TP/SL and every authored metric —
            // flows through the one `exit_fired` loop, so priority collapses from
            // `Dead > SL > TP > Metrics` to `Dead > exit_fired` (SL/TP are prepended
            // pnl reqs, so their old relative order is preserved inside the loop).
            // Scale-out stages rank after the global side (catastrophe path first).
            if dead {
                return ArmDecision::Exit(ExitReason::Dead);
            }
            let ctx = held.position_ctx();
            if let Some(reason) = c.exit_fired(&token.track, &ctx, now) {
                return ArmDecision::Exit(reason);
            }
            if let Some(reason) = c.stage_fired(held.stage, &token.track, &ctx, now) {
                match c.stage_at(held.stage).and_then(|s| s.sell_bps) {
                    Some(sell_bps) => {
                        return ArmDecision::PartialExit { reason, sell_bps };
                    }
                    // Remainder stage (`sell_bps` omitted) ⇒ full close under its
                    // own conditions.
                    None => return ArmDecision::Exit(reason),
                }
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
    cap: Cap,
    max_total: Cap,
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
                if !cap.allows(counters.open) || !max_total.allows(counters.total) {
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
                ArmState::EntryPending {
                    intent: intent.clone(),
                    position,
                    attempts: 1,
                    lamports: buy_lamports,
                },
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
                stage: None,
            }));
        }
        ArmDecision::Exit(reason) => {
            let Some(ArmState::Entered(held)) = token.arms.get(&rule_id).cloned() else {
                return;
            };
            let position = held.position;
            let intent = state.next_intent(rule_id, mint.clone());
            token.arms.insert(
                rule_id,
                ArmState::ExitPending {
                    intent: intent.clone(),
                    reason,
                    attempts: 1,
                    portion: Portion::All,
                    held,
                },
            );
            fx.push(Effect::SubmitSell {
                intent: intent.clone(),
                position,
                reason,
                portion: Portion::All,
            });
            fx.push(Effect::PositionUpdate(PositionDelta {
                position,
                rule: rule_id,
                mint: mint.clone(),
                status: PositionStatus::ExitPending,
                fill: None,
                reason: Some(reason),
                intent: Some(intent),
                stage: None,
            }));
        }
        ArmDecision::PartialExit { reason, sell_bps } => {
            let Some(ArmState::Entered(held)) = token.arms.get(&rule_id).cloned() else {
                return;
            };
            let position = held.position;
            let stage = held.stage;
            let portion = Portion::BpsOfInitial(sell_bps);
            let intent = state.next_intent(rule_id, mint.clone());
            token.arms.insert(
                rule_id,
                ArmState::ExitPending {
                    intent: intent.clone(),
                    reason,
                    attempts: 1,
                    portion,
                    held,
                },
            );
            fx.push(Effect::SubmitSell {
                intent: intent.clone(),
                position,
                reason,
                portion,
            });
            fx.push(Effect::PositionUpdate(PositionDelta {
                position,
                rule: rule_id,
                mint: mint.clone(),
                status: PositionStatus::ExitPending,
                fill: None,
                reason: Some(reason),
                intent: Some(intent),
                stage: Some(stage),
            }));
        }
    }
}

/// The post-close arm state (plan Ph4). Re-arms into [`ArmState::Cooldown`] when the
/// rule has re-entry configured, the close `reason` is a normal strategy exit (not a
/// dead/manual/migrated close — see [`reason_allows_reentry`]), and the per-token
/// episode cap is not yet reached; otherwise terminal [`ArmState::Done`]. Only rules
/// with re-entry configured ever touch `episodes`, so one-shot rules stay untouched.
fn rearm_after_close(
    state: &EngineState,
    episodes: &mut BTreeMap<RuleId, u32>,
    rule: RuleId,
    reason: ExitReason,
    closed_at: Ts,
) -> ArmState {
    let Some(re) = state.rules.get(&rule).and_then(|c| c.reentry) else {
        return ArmState::Done; // one-shot — no episode tracking, terminal.
    };
    if !reason_allows_reentry(reason) {
        return ArmState::Done; // dead / manual / migrated — never re-arm.
    }
    // Count this closed episode; re-arm only while under the cap.
    let n = {
        let e = episodes.entry(rule).or_insert(0);
        *e += 1;
        *e
    };
    if n >= re.max_episodes_per_token {
        return ArmState::Done;
    }
    let until = closed_at + chrono::Duration::milliseconds((re.cooldown_sec * 1000.0) as i64);
    ArmState::Cooldown { until }
}

/// Whether a close `reason` is a normal strategy exit that should re-arm under
/// re-entry. TP / SL / a metric exit ⇒ yes (the trailing stop or a rule condition
/// fired — trade the next signal). Dead / Manual / Migrated ⇒ no: the token is gone,
/// a human intervened, or it left the curve — re-arming would fight that.
fn reason_allows_reentry(reason: ExitReason) -> bool {
    matches!(
        reason,
        ExitReason::TakeProfit | ExitReason::StopLoss | ExitReason::Metrics { .. }
    )
}

/// Decrement a rule's open counter on a position close (saturating).
fn decrement_open(state: &mut EngineState, rule: RuleId) {
    if let Some(c) = state.counters.get_mut(&rule) {
        c.open = c.open.saturating_sub(1);
    }
}

/// Whether an arm still owns an open or in-flight position (must survive reload).
fn arm_holds_position(arm: &ArmState) -> bool {
    matches!(
        arm,
        ArmState::Entered(_)
            | ArmState::EntryPending { .. }
            | ArmState::ExitPending { .. }
    )
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
