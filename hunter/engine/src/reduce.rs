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

use crate::arm::{ArmState, ClockHorizons, CompiledRule, EnteredCtx, EntryBlockers};
use crate::cap::Cap;
use crate::deadness::{is_dead_verdict, DEAD_MEANINGFUL_TRADE_SOL, DEAD_QUIET_SECS};
use crate::event::{
    ArmedDelta, ArmedStateTag, DisarmReason, Effect, Event, ExitReason, FillFailReason, Mint,
    Portion, PositionDelta, PositionStatus, RuleId, TradeMode,
};
use crate::fingerprint::{match_all, MatchPhase};
use crate::grouping::{TokenFingerprint, LAMPORTS_PER_SOL_F64};
use crate::metrics::Ts;
use crate::state::{EngineState, PositionRef, Settled, TokenState};

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
    // Any event other than a bare clock advance can change a token, so the
    // "everything is settled" memo cannot survive one. Cleared here, once, rather
    // than in each branch — a branch that forgot would skip real decisions.
    if !matches!(event, Event::Tick { .. }) {
        state.all_settled_at = None;
    }
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
                token.unsettle();
                for rid in stale {
                    token.arms.insert(rid, ArmState::Disarmed(DisarmReason::Paused));
                    fx.push(armed(mint, rid, ArmedStateTag::Disarmed(DisarmReason::Paused)));
                }
            }
            state.reload(&rules, &fps);
        }

        Event::TokenCreated { mint, fp, at, creator_wallet_hash, identity } => {
            if state.tokens.contains_key(&mint) {
                return fx; // duplicate creation — idempotent
            }
            let mut track = state.new_track(at);
            if let Some(h) = creator_wallet_hash {
                track.seed_creator(h);
                // Strictly-prior count, and the increment happens here so the tally
                // advances exactly once per creation — the duplicate-creation guard
                // above has already returned, so a replayed event cannot double-count.
                track.seed_prior_launches(state.take_prior_launches(h));
            }
            track.seed_ix_count(fp.ix_labels.len());
            let mut token = TokenState {
                created_at: at,
                tf: *fp,
                identity,
                track,
                last_meaningful_at: None,
                last_trade_at: None,
                settled: None,
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
                // The metric reads the SAME number the fingerprint buckets — seeded
                // here rather than at `TokenCreated` because this is where it exists.
                // A rule gated on `m_snapshot.first_slot_buy` therefore reads NaN (and
                // cannot fire) until this event, exactly like a `PendingFirstSlot` arm.
                token.track.seed_first_slot_buy(buy_lamports as f64 / LAMPORTS_PER_SOL_F64);
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
            // Lend the map out rather than `remove` + re-`insert`: the sweep never
            // reads `tokens`, so borrowing it away is enough to satisfy the borrow
            // checker, and it turns two keyed `BTreeMap` operations (base58 string
            // compares + rebalancing) per trade into one.
            let mut tokens = std::mem::take(&mut state.tokens);
            if let Some(token) = tokens.get_mut(&mint) {
                fold_trade(token, trade);
                evaluate_token(state, token, &mint, trade.at, &mut fx);
                if !token.is_active() {
                    tokens.remove(&mint);
                }
            }
            state.tokens = tokens;
        }

        Event::Tick { now } => {
            // Expire the copycat guard's memory on event time (self-rate-limited),
            // so a replay sweeps exactly where live did.
            state.dupe_guard.prune(now);
            let dense = state.dense_ticks;
            // Whole-map no-op: the previous sweep settled everyone and the set has
            // not changed since. O(1) — no walk at all.
            if !dense && state.all_settled_at == Some(state.tokens.len()) {
                return fx;
            }
            // Same lend-the-map trick as `Trade`, over the whole set: one ordered
            // walk with an in-place `retain` (which also does the pruning) replaces
            // a cloned key list plus a `remove`/`insert` pair per token per tick.
            let epoch = state.cross_epoch;
            let mut tokens = std::mem::take(&mut state.tokens);
            let mut all_settled = true;
            tokens.retain(|mint, token| {
                // Provably-static token: every reading re-derives identically and no
                // cross-token input has moved, so the whole sweep is a no-op. See
                // `Settled` — this is what stops an un-prunable token (healthy
                // reserves, or no reserve reading at all) from being swept five times
                // a second for the rest of the run.
                if !dense && token.tick_is_noop(epoch) {
                    return true;
                }
                token.track.on_tick(now);
                evaluate_token(state, token, mint, now, &mut fx);
                all_settled &= token.settled.is_some();
                token.is_active()
            });
            // Only memoize when the epoch held still for the whole walk — a decision
            // taken mid-walk (an entry, a close) invalidates the stamps written
            // before it, and re-deriving that is not worth the bookkeeping.
            state.all_settled_at =
                (all_settled && !dense && state.cross_epoch == epoch).then_some(tokens.len());
            state.tokens = tokens;
        }

        Event::FillConfirmed { intent, fill } => {
            let mint = intent.mint.clone();
            let rule_id = intent.rule;
            let Some(mut token) = state.tokens.remove(&mint) else { return fx };
            token.unsettle();
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

        Event::FillFailed { intent, reason, at } => {
            let mint = intent.mint.clone();
            let rule_id = intent.rule;
            let Some(mut token) = state.tokens.remove(&mint) else { return fx };
            token.unsettle();
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
                    // A retry is a NEW buy, so it must clear the SAME entry gate the
                    // first one did — the decision that authorized attempt 1 is only
                    // as fresh as attempt 1. Confirming a revert takes seconds (12.3 s
                    // on 2026-08-07, `XsPXZt…`), and the pool can be drained inside
                    // that window: there, attempt 1 was decided at 14.65 SOL liquidity
                    // under an `entry liquidity > 10` rule, reverted 6042 (slippage),
                    // and the blind retry filled at 0.276 SOL — 36x under the rule's
                    // own floor. `can_enter` is otherwise reachable only from
                    // `decide_arm`'s `Armed` branch, which a retry never passes
                    // through.
                    //
                    // `entry_enabled` is checked alongside, because `can_enter` does
                    // NOT cover it — `decide_arm` gates the two separately, and a
                    // rule the operator stopped mid-flight stays loaded (as a drain
                    // rule, so its open positions can still exit) with
                    // `entry_enabled = false`. Without this a "stop rule" click would
                    // still let an in-flight revert buy again.
                    //
                    // A manual episode has no row in `rules` (`is_none_or` ⇒ retry):
                    // manual buys bypass entry conditions by design. `at: None` is a
                    // pre-`at` log line with no clock to judge against — replay it as
                    // it originally ran.
                    let requalifies = match at {
                        Some(now) => state
                            .rules
                            .get(&rule_id)
                            .is_none_or(|c| c.entry_enabled && c.can_enter(&token.track, now)),
                        None => true,
                    };
                    // Fatal = structural / StopFeeBurn — never burn fee retries.
                    let attempts_left = reason != FillFailReason::Fatal
                        && attempts < MAX_ENTRY_ATTEMPTS
                        && retry_lamports > 0;
                    let retry = attempts_left && requalifies;
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
                        // "Not qualified right now" is NOT "finished with this token".
                        // Only an exhausted ladder / Fatal is terminal; a live rule
                        // that merely failed the re-check goes back to `Armed`, so the
                        // ONE gate (`decide_arm`) re-decides on the next trade/tick —
                        // which is also how the gates a retry cannot express get
                        // applied: `exclusive` means *wait* there, not *give up*, and
                        // `dead` / `entry_unsatisfiable` disarm properly.
                        // Re-arming cannot spin: re-entry has to clear the full gate
                        // again, exactly like a first entry.
                        let next = if attempts_left && state.rules.contains_key(&rule_id) {
                            fx.push(armed(&mint, rule_id, ArmedStateTag::Armed));
                            ArmState::Armed
                        } else {
                            ArmState::Done
                        };
                        token.arms.insert(rule_id, next);
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
                        // A manual buy can target a mint the engine never armed,
                        // so there is no metadata to key on here. The guard never
                        // blocks a manual buy anyway; `None` just means this
                        // episode records nothing.
                        identity: None,
                        track,
                        last_meaningful_at: None,
                        last_trade_at: None,
                        settled: None,
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
            token.unsettle();
            token.arms.insert(
                rule,
                ArmState::EntryPending { intent: intent.clone(), position, attempts: 1, lamports },
            );
            // A manual episode is never *blocked* by the copycat guard (the
            // operator's call always wins — manual already bypasses entry
            // conditions and caps), but it is still a trade on that identity, so
            // it is remembered against the bot's future entries. Manual buys are
            // real-money only by construction (`POST /api/positions/manual-buy`
            // spends the live wallet), so they land in the real memory.
            state.record_entry_identity(TradeMode::Real, &mint, at);
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
            token.unsettle();
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
            token.unsettle();
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
            token.unsettle();
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
    /// The detail is captured **in the verdict**, not at apply time: this is the
    /// one instant the failing readings still exist, and `decide_arm` is where the
    /// track is in hand. It is `Some` only for `Unsatisfiable`.
    Disarm(DisarmReason, Option<Box<EntryBlockers>>),
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
    if token.last_trade_at.is_none_or(|t| trade.at > t) {
        token.last_trade_at = Some(trade.at);
    }
    // A fold moves the track, so any settled verdict is stale. Doing it here (not
    // in the `Trade` branch) also covers `prime_trade`, which folds *without*
    // evaluating and would otherwise leave a stale verdict behind on boot replay.
    token.unsettle();
}

/// Seconds as a `Duration`, clamped like the sparse grid's horizons so a
/// fat-fingered threshold cannot overflow the instant arithmetic.
fn horizon_secs(s: f64) -> chrono::Duration {
    let ms = (crate::metrics::grid::SparseGrid::clamp_secs(s) * 1000.0).ceil();
    chrono::Duration::milliseconds(if ms.is_finite() { ms.max(0.0) as i64 } else { 0 })
}

/// The last instant at which anything about `token` can still change on its own —
/// the `until` half of [`Settled`]. `None` ⇒ never settle.
///
/// Anchors, all of which must be covered or a tick skip would drop a decision:
/// * trailing windows decay from the newest trade;
/// * `time` climbs from creation, `stall` from the last trade (an upper bound on
///   the last all-time high), `held` from each entry fill;
/// * the dead verdict flips once, at `last_meaningful + DEAD_QUIET_SECS`;
/// * a re-entry `Cooldown` re-arms at its own instant.
///
/// A **manual** arm bails out to `None`: its exit rule lives in `manual_rules`, so
/// its clocks are not in the reloaded [`ClockHorizons`] union and the horizon below
/// would not cover them.
fn settle_until(state: &EngineState, token: &TokenState, h: &ClockHorizons) -> Option<Ts> {
    let anchor = token.last_trade_at.unwrap_or(token.created_at);
    let mut until = anchor
        .max(anchor + horizon_secs(h.max_window_secs))
        .max(token.created_at + horizon_secs(h.time_secs))
        .max(anchor + horizon_secs(h.stall_secs))
        .max(
            token.last_meaningful_at.unwrap_or(token.created_at)
                + chrono::Duration::seconds(DEAD_QUIET_SECS),
        );
    for (rule_id, arm) in &token.arms {
        if !arm.is_active() {
            continue;
        }
        if !state.rules.contains_key(rule_id) {
            return None; // manual episode — horizons unknown here
        }
        match arm {
            ArmState::Cooldown { until: re } => until = until.max(*re),
            ArmState::Entered(ctx) => {
                until = until.max(ctx.entered_at + horizon_secs(h.held_secs))
            }
            _ => {}
        }
    }
    Some(until)
}

/// Ratchet every held position's **since-entry** peak/trough to the track's current
/// price, as observed at `at`. Two bare compares per `Entered` arm, no alloc.
///
/// `at < entered_at` is skipped, and that guard is the whole point: `peak`/`trough`
/// are *position-scoped* (they define `retrace` and `bounce`), so only prices the
/// position actually lived through may move them. On the live path every event is
/// newer than the fill by construction, so the guard is a no-op there — it exists
/// for the **restart** path, where `prime_trade` replays the token-cache seed
/// (`SEED_TRADES_PER_MINT` rows reaching back `SEED_TRADES_MAX_AGE_HOURS`), which
/// spans trades from *before* the adopted position entered. Without the guard a
/// dip-entry position wakes up owning the run-up it deliberately did not buy, and
/// its trailing stop fires on a peak it never held.
fn fold_entered_extremes(token: &mut TokenState, at: Ts) {
    let cur_price = token.track.current_price();
    if !cur_price.is_finite() {
        return;
    }
    for arm in token.arms.values_mut() {
        if let ArmState::Entered(ctx) = arm {
            if at < ctx.entered_at {
                continue;
            }
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
    fold_entered_extremes(token, trade.at);
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
    fold_entered_extremes(token, now);

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
    //
    // With every priority equal the sort key collapses to `*id`, and `arms` is a
    // `BTreeMap` — already in that order. So the sort is skipped unless some loaded
    // rule actually sets a priority, which is the only case where it can reorder
    // anything (`any_priority`, recomputed on reload).
    if state.any_priority {
        rule_ids.sort_unstable_by_key(|id| {
            (
                std::cmp::Reverse(state.rules.get(id).map_or(0, |c| c.priority)),
                *id,
            )
        });
    }
    for rule_id in rule_ids {
        let (decision, buy_lamports, cap, max_total, trade_mode) = {
            let arm = &token.arms[&rule_id];
            // Manual episodes resolve via their per-position exit rule; a
            // tracked-only manual arm has neither ⇒ no decision (no auto-exit).
            let Some(c) = state.rule_for(rule_id, arm_position(arm)) else { continue };
            // Only an armed rule can be copycat-blocked, so the guard lookup stays
            // off the held/in-flight paths entirely.
            let dupe_blocked = matches!(arm, ArmState::Armed)
                && state.dupe_guard.blocks(c.trade_mode, token.identity, mint, now);
            (
                decide_arm(c, rule_id, arm, token, dead, dupe_blocked, now),
                resolve_buy_lamports(c, token.track.current_reserves()),
                c.concurrent_cap,
                c.max_total,
                c.trade_mode,
            )
        };
        apply_decision(
            state, token, mint, rule_id, decision, buy_lamports, cap, max_total, trade_mode, now,
            fx,
        );
    }

    // Stamp the settled verdict LAST, off the post-decision arm states — a decision
    // taken above (an entry, a cooldown re-arm) changes what the token is still
    // waiting for. Any later mutation that bypasses this sweep must `unsettle`.
    //
    // `now >= until` is the load-bearing half: it says THIS sweep already stood past
    // every clock, so no later instant can reveal a crossing it missed. Stamping on
    // `until` alone would instead assume the next tick lands close enough to observe
    // one, which is a cadence assumption the engine does not get to make.
    token.settled = settle_until(state, token, &state.tick_horizons)
        .filter(|until| now >= *until)
        .map(|until| Settled { until, epoch: state.cross_epoch });
}

/// The position an arm owns, when it owns one (keys the manual exit-rule lookup).
/// Delegates to [`ArmState::position`] so the fold and every out-of-band reader
/// resolve a manual episode's rule through the one match.
fn arm_position(arm: &ArmState) -> Option<crate::event::PositionId> {
    arm.position()
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
    dupe_blocked: bool,
    now: Ts,
) -> ArmDecision {
    match arm {
        ArmState::Armed => {
            if !c.entry_enabled {
                return ArmDecision::None;
            }
            if dead {
                return ArmDecision::Disarm(DisarmReason::Dead, None);
            }
            // Capture what the entry was short of before the state is gone. Once
            // this returns, the arm is terminal and the track keeps moving — the
            // readings that explain the disarm exist only here. One extra walk of
            // the entry reqs, once per episode, on the branch that ends it.
            if let Some(killed_by) = c.entry_unsatisfiable(&token.track, now) {
                return ArmDecision::Disarm(
                    DisarmReason::Unsatisfiable,
                    Some(Box::new(c.entry_blockers(&token.track, now, killed_by))),
                );
            }
            // Copycat guard: a different mint with this token's `(name, symbol)`
            // was traded inside the window. A **Disarm**, not `exclusive`'s wait —
            // the block outlives any curve token, so waiting would re-ask a fixed
            // question on every tick until the token dies.
            if dupe_blocked {
                return ArmDecision::Disarm(DisarmReason::DuplicateIdentity, None);
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

/// The buy size for one entry: the rule's fixed amount, or its percent of the pool's
/// SOL reserve when it authors one.
///
/// Resolved **here**, in the kernel, so live-real, live-paper and simulate cannot size
/// differently — everything downstream receives absolute lamports on `SubmitBuy` and is
/// unchanged by this feature.
///
/// `reserve_sol` is the depth the fold already holds at the entry instant. It is `NaN`
/// before the first trade with a decoded reserve, and a percent of an unknown pool is
/// not a size — so an unusable depth **falls back to the fixed amount** rather than
/// guessing. That is the conservative direction: the fixed amount is a number a human
/// authored, where a guessed one is not.
fn resolve_buy_lamports(c: &CompiledRule, reserve_sol: f64) -> u64 {
    let Some(pct) = c.buy_pct_of_vsol else { return c.buy_amount_lamports };
    if !reserve_sol.is_finite() || reserve_sol <= 0.0 {
        return c.buy_amount_lamports;
    }
    let sol = reserve_sol * pct / 100.0;
    // `as u64` saturates at 0 for a negative and at u64::MAX for an overflow, but a
    // silent 0 would submit a buy for nothing, so floor at one lamport: the pool being
    // dust is a reason to buy small, not a reason to emit a malformed order.
    let lamports = (sol * LAMPORTS_PER_SOL_F64).round();
    if lamports < 1.0 {
        1
    } else {
        lamports as u64
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
    trade_mode: TradeMode,
    now: Ts,
    fx: &mut Effects,
) {
    match decision {
        ArmDecision::None => {}
        ArmDecision::Disarm(reason, detail) => {
            // `ArmState` keeps only the reason: it is per-(token, rule) RAM that
            // outlives the episode, and the detail's one consumer is the effect.
            token.arms.insert(rule_id, ArmState::Disarmed(reason));
            fx.push(disarmed(mint, rule_id, reason, detail));
        }
        ArmDecision::Enter => {
            // Caps enforced at entry (not arm): concurrency + lifetime. Over a cap ⇒
            // wait (stay armed) and re-check on the next event.
            {
                let counters = state.counters.entry(rule_id).or_default();
                if !cap.allows(counters.open) || !max_total.allows(counters.total) {
                    // Stay armed and re-check later. Nothing changed, so nothing is
                    // invalidated: the freeing close is what bumps the epoch and
                    // wakes this arm back up (see `decrement_open`).
                    return;
                }
            }
            state.with_counters(rule_id, |c| {
                c.open += 1;
                c.total += 1;
            });
            let position = state.next_position();
            let intent = state.next_intent(rule_id, mint.clone());
            state.positions.insert(position, PositionRef { mint: mint.clone(), rule: rule_id });
            // Remember the identity at the ATTEMPT, not at the fill: a copycat
            // that reverts our buy is exactly the trap worth not re-entering, and
            // an entry that never fills still proves this identity looked
            // tradeable. `rollback_entry` deliberately does not undo this.
            state.record_identity(trade_mode, token.identity, mint, now);
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

/// Decrement a rule's open counter on a position close (saturating). Goes through
/// [`EngineState::with_counters`], which bumps the cross-token epoch — a freed slot
/// is exactly what a cap-refused arm on some other token is waiting for.
fn decrement_open(state: &mut EngineState, rule: RuleId) {
    state.with_counters(rule, |c| c.open = c.open.saturating_sub(1));
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

/// Roll back both counters for an entry that never filled (saturating). See
/// [`decrement_open`] for why this goes through `with_counters`.
fn rollback_entry(state: &mut EngineState, rule: RuleId) {
    state.with_counters(rule, |c| {
        c.open = c.open.saturating_sub(1);
        c.total = c.total.saturating_sub(1);
    });
}

/// Build an `ArmedChanged` effect.
fn armed(mint: &Mint, rule: RuleId, state: ArmedStateTag) -> Effect {
    Effect::ArmedChanged(ArmedDelta { mint: mint.clone(), rule, state, detail: None })
}

/// Build a disarm `ArmedChanged`, carrying whatever the decision captured about
/// why the entry was hopeless (`Unsatisfiable` only — see [`ArmedDelta::detail`]).
fn disarmed(
    mint: &Mint,
    rule: RuleId,
    reason: DisarmReason,
    detail: Option<Box<EntryBlockers>>,
) -> Effect {
    Effect::ArmedChanged(ArmedDelta {
        mint: mint.clone(),
        rule,
        state: ArmedStateTag::Disarmed(reason),
        detail,
    })
}

#[cfg(test)]
mod buy_sizing {
    use super::*;
    use crate::event::{LoadedRule, TradeMode};
    use crate::fingerprint::FingerprintId;
    use crate::rule_params::RuleParams;

    fn rule(pct: Option<f64>) -> CompiledRule {
        CompiledRule::compile(&LoadedRule {
            id: RuleId(uuid::Uuid::nil()),
            fingerprint_id: FingerprintId(uuid::Uuid::nil()),
            trade_mode: TradeMode::Paper,
            buy_amount_lamports: 300_000_000, // 0.3 SOL
            max_concurrent_tokens: 1,
            max_total_tokens: 0,
            params: RuleParams { buy_pct_of_vsol: pct, ..Default::default() },
            entry_enabled: true,
        })
    }

    #[test]
    fn without_a_percent_the_fixed_amount_is_used() {
        assert_eq!(resolve_buy_lamports(&rule(None), 50.0), 300_000_000);
        // …including when the pool is unknown, which must not matter here.
        assert_eq!(resolve_buy_lamports(&rule(None), f64::NAN), 300_000_000);
    }

    /// The point of the feature: impact is `buy / vsol`, so a percent must hold that
    /// ratio fixed across the band a fixed size varies over.
    #[test]
    fn a_percent_scales_with_the_pool() {
        let r = rule(Some(1.0));
        assert_eq!(resolve_buy_lamports(&r, 40.0), 400_000_000);
        assert_eq!(resolve_buy_lamports(&r, 75.0), 750_000_000);
        // Same ratio at both ends — the invariant a fixed size breaks.
        for vsol in [40.0, 75.0] {
            let sol = resolve_buy_lamports(&r, vsol) as f64 / 1e9;
            assert!((sol / vsol - 0.01).abs() < 1e-12, "impact drifted at vsol {vsol}");
        }
    }

    /// An unusable depth falls back to the authored amount rather than guessing one.
    /// `NaN` is the pre-first-trade reserve, and a fold can reach an entry decision
    /// before any trade carries a decoded reserve.
    #[test]
    fn an_unknown_pool_falls_back_to_the_fixed_amount() {
        let r = rule(Some(1.0));
        for bad in [f64::NAN, 0.0, -5.0, f64::INFINITY] {
            assert_eq!(
                resolve_buy_lamports(&r, bad),
                300_000_000,
                "reserve {bad} must fall back, not size off it",
            );
        }
    }

    /// A dust pool must still produce a well-formed order. Rounding to zero would
    /// submit a buy for nothing.
    #[test]
    fn a_dust_pool_floors_at_one_lamport() {
        assert_eq!(resolve_buy_lamports(&rule(Some(0.0001)), 0.000_000_001), 1);
    }

    /// The ceiling is a real-money rail, so it is enforced at parse — a rule that
    /// would spend half the pool must not be storable in the first place.
    #[test]
    fn an_absurd_percent_is_rejected_at_parse() {
        let err = RuleParams::parse(&serde_json::json!({ "buy_pct_of_vsol": 50 }))
            .expect_err("50% of a pool is not a size");
        assert!(err.contains("buy_pct_of_vsol"), "{err}");
        for bad in [0, -1] {
            assert!(RuleParams::parse(&serde_json::json!({ "buy_pct_of_vsol": bad })).is_err());
        }
        assert!(RuleParams::parse(&serde_json::json!({ "buy_pct_of_vsol": 1.5 })).is_ok());
    }

    /// Params round-trip: a stored rule must come back byte-identical, and a rule
    /// without the knob must not grow the key.
    #[test]
    fn the_percent_round_trips_and_stays_absent_by_default() {
        let json = serde_json::json!({ "buy_pct_of_vsol": 1.25 });
        let p = RuleParams::parse(&json).unwrap();
        assert_eq!(p.buy_pct_of_vsol, Some(1.25));
        assert_eq!(p.to_value(), json);
        assert_eq!(RuleParams::default().to_value(), serde_json::json!({}));
    }
}
