//! Rule readout — what the fold reads for one (token, rule) **right now**.
//!
//! The decision path already computes this: [`TokenTrack`] folds every metric on
//! each trade and tick, and [`CompiledRule`] flattens a rule's conditions into
//! [`MetricReq`]s. This module exposes that state read-only, so a UI can show the
//! live value beside each authored threshold without re-folding anything.
//!
//! **It is a mirror, not a lookalike.** Every read here goes through the same
//! `track.value(..)` / [`position_value`] and the same [`evaluator`] the fold uses,
//! so the readout cannot report a condition the engine would decide differently.
//! Three places where a naive copy silently diverges — each has a guard test at the
//! bottom of this file:
//!
//! * **The two sides use different combinators.** Entry mirrors `reqs_satisfied`
//!   ([`eval`], so an empty expr is vacuously true); exit and scale-out mirror
//!   `reqs_exit_fired` ([`first_satisfied_cond`], so an empty expr fires nothing).
//! * **A disarmed trailing req is skipped, not false.** `reqs_exit_fired` `continue`s
//!   past a trailing exit while the position is under `m_position.arm_above_pct`, so
//!   [`ConditionRead::disarmed`] is its own fact — a UI that renders it as a plain
//!   failing condition shows a stop that looks live when the fold is not evaluating it.
//! * **The fold evaluates only the active scale-out stage.** Every stage is returned
//!   so the ladder is visible, each tagged [`ReadSide::Stage::active`]; an inactive
//!   stage's `ok` is what the fold *would* read, never a decision it is making.
//!
//! Read on demand (a modal, an API call), never per event: unlike the hot path this
//! allocates — the returned vector, and a clone of each req's authored condition expr
//! so the caller can render `metric op threshold` without reaching back into the rule.
//!
//! [`evaluator`]: crate::metrics::evaluator

use crate::arm::{CompiledRule, MetricReq, ReqOrigin};
use crate::event::{Mint, RuleId};
use crate::fingerprint::FingerprintId;
use crate::metrics::evaluator::{eval, first_satisfied_cond, Condition, ConditionExpr};
use crate::metrics::flow_split::FlowPatterns;
use crate::metrics::position::{position_value, trailing_armed, PositionCtx};
use crate::metrics::track::TokenTrack;
use crate::metrics::{MetricId, TradeLite, Ts};
use crate::state::EngineState;

/// Which side of the rule a [`ConditionRead`] comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadSide {
    /// Entry conditions — **AND** across reqs (all must hold to enter).
    Entry,
    /// The global exit side — **OR** across reqs (any one fires the sell).
    Exit,
    /// One scale-out stage, `OR` within it like [`Exit`](Self::Exit).
    Stage {
        index: u8,
        /// Whether the position is currently at this stage. The fold evaluates
        /// **only** the active stage (`CompiledRule::stage_fired`).
        active: bool,
    },
}

impl ReadSide {
    /// Whether this side's combinator treats an empty condition expr as satisfied.
    /// Entry does (vacuous, per `reqs_satisfied`); the exit sides do not (nothing to
    /// fire, per `reqs_exit_fired`).
    fn vacuous_when_empty(self) -> bool {
        matches!(self, ReadSide::Entry)
    }
}

/// One metric read of a rule, at one instant: the authored conditions, the value the
/// fold sees, and whether it holds.
#[derive(Debug, Clone, PartialEq)]
pub struct ConditionRead {
    pub side: ReadSide,
    pub metric: MetricId,
    /// Trailing-window size for dynamic metrics; `None` for static ones.
    pub window: Option<f64>,
    /// The value the fold reads at `now`. `NaN` when unreadable (an unregistered
    /// window, a flow metric with no fingerprint state, or a position metric with no
    /// position) — which, per the engine convention, satisfies nothing.
    pub value: f64,
    /// The authored DNF (`OR` of `AND` arms) this req judges `value` against.
    pub conds: ConditionExpr,
    /// The metric's registry `=`-tolerance, as used on `value`.
    pub tolerance: f64,
    /// The first condition on the first satisfied arm, when one holds — the same
    /// detail `ExitReason::Metrics` is stamped from.
    pub matched: Option<Condition>,
    /// Whether this req is satisfied at `now`, under its side's combinator.
    pub ok: bool,
    /// `TakeProfit` / `StopLoss` for a desugared ladder req, else `Authored`. Present
    /// so a TP/SL chip is labelled as such instead of as a raw `pnl` condition.
    pub origin: ReqOrigin,
    /// `m_position.arm_above_pct` on a trailing req — the PnL the trail arms at.
    pub arm_above_pct: Option<f64>,
    /// The trailing gate is set and not cleared, so the fold **skips** this req.
    /// Always paired with `ok == false`, and distinct from it: a disarmed trail is
    /// not being evaluated at all.
    pub disarmed: bool,
}

/// Read every condition of `rule` against the token's live fold at `now`.
///
/// `ctx` is the held position's [`PositionCtx`] (from `ArmState::Entered`) — `None`
/// for an armed-but-unentered token, where position-scoped reqs read `NaN` exactly as
/// they do in the pre-entry `can_enter` gate. `stage` is the position's current
/// scale-out stage, used only to tag which stage is active.
///
/// Order is stable and meaningful: entry reqs, then exit reqs in fold order
/// (desugared stop-loss, take-profit, then authored), then each ladder stage.
pub fn read_rule(
    rule: &CompiledRule,
    track: &TokenTrack,
    ctx: Option<&PositionCtx>,
    stage: Option<u8>,
    now: Ts,
) -> Vec<ConditionRead> {
    // The mark every position-scoped read is taken against — one read, as the fold
    // does it, so `retrace`/`pnl` here cannot disagree with the exit decision.
    let price = track.current_price();

    let stage_reqs: usize = rule.scale_out.iter().map(|s| s.reqs.len()).sum();
    let mut out = Vec::with_capacity(rule.entry_reqs.len() + rule.exit_reqs.len() + stage_reqs);

    // Entry reqs are token-scoped by construction (`m_position` is exit-only), so they
    // read through the track with no position context — matching `reqs_satisfied`.
    for r in &rule.entry_reqs {
        out.push(read_req(r, ReadSide::Entry, track, None, price, now));
    }
    for r in &rule.exit_reqs {
        out.push(read_req(r, ReadSide::Exit, track, ctx, price, now));
    }
    for (i, s) in rule.scale_out.iter().enumerate() {
        let index = i as u8;
        let side = ReadSide::Stage { index, active: stage == Some(index) };
        for r in &s.reqs {
            out.push(read_req(r, side, track, ctx, price, now));
        }
    }
    out
}

/// Where a readout's numbers come from. Load-bearing for honesty, not decoration:
/// the two are not equally trustworthy and a UI must be able to say which it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadoutSource {
    /// Read out of the live fold's own [`TokenTrack`] — the state the engine is
    /// deciding on at this instant. Exact by construction.
    Engine,
    /// Reconstructed by folding stored trades back through a fresh track
    /// ([`replay_readout`]). A close approximation, not the same thing: stored rows
    /// carry an *approximated* real-reserve value rather than the exact emitted one,
    /// and any trade the live feed saw but never persisted is simply absent.
    Replay,
}

/// One (token, rule) arm's readout: its conditions plus the context needed to render
/// them honestly.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleReadout {
    pub source: ReadoutSource,
    /// The arm's lifecycle state ([`ArmState::tag`](crate::arm::ArmState::tag)) —
    /// an `Armed` readout has no position, so its exit reads are hypothetical.
    /// `None` on a replay, which has no arm: the arm is long gone.
    pub arm: Option<&'static str>,
    /// The held position's scale-out stage; `None` when nothing is held.
    pub stage: Option<u8>,
    /// The instant every value is read at. Values move between reads; a caller that
    /// renders two readouts side by side needs to know they are different instants.
    pub at: Ts,
    pub reads: Vec<ConditionRead>,
}

/// Read one tracked (token, rule) arm straight off engine state at `now`.
///
/// The out-of-band entry point: a caller holding an [`EngineState`] (the live
/// decision loop, answering a command) gets the readout without reproducing the
/// resolution the fold does — notably the manual-episode rule lookup, which lives in
/// `manual_rules` keyed by position and is invisible to a plain `state.rules` get.
///
/// `None` ⇒ the token is not tracked, the rule has no arm on it, or the arm resolves
/// to no rule (a tracked-only manual position, which by construction has no
/// conditions to read). All three are "nothing to show", never an error.
pub fn read_state(
    state: &EngineState,
    mint: &Mint,
    rule_id: RuleId,
    now: Ts,
) -> Option<RuleReadout> {
    let token = state.tokens.get(mint)?;
    let arm = token.arms.get(&rule_id)?;
    let rule = state.rule_for(rule_id, arm.position())?;
    let held = arm.held();
    let ctx = held.map(|h| h.position_ctx());
    let stage = held.map(|h| h.stage);
    Some(RuleReadout {
        source: ReadoutSource::Engine,
        arm: Some(arm.tag()),
        stage,
        at: now,
        reads: read_rule(rule, &token.track, ctx.as_ref(), stage, now),
    })
}

/// The flow context a replay needs to classify volume vs organic exactly as the live
/// fold did — the rule's fingerprint patterns **and** the token's creator wallet.
///
/// The creator is not optional garnish: it is volume-side unconditionally and seeds
/// the contagion set, so a replay folded without it books the dev buy and dev dump —
/// usually a token's two largest single flows — as organic, and every `m_flow_split`
/// condition reads against a different classification than the one that decided.
pub struct ReplayFlow<'a> {
    pub fingerprint: FingerprintId,
    pub patterns: &'a FlowPatterns,
    pub creator_wallet_hash: Option<u64>,
}

/// Everything a replay needs besides the rule, the trades, and the instant.
pub struct ReplayCtx<'a> {
    /// Token creation instant — what `m_snapshot.time` and the lifetime extrema
    /// anchor on. Pass the first trade's `block_time`, which is what the replay
    /// driver and the lab's metric-series both use (the dev-buy slot).
    pub created_at: Ts,
    /// Entry fill `(time, price)`; `None` for a position that never entered, whose
    /// `m_position` reads then have no anchor and stay `NaN`.
    pub entry: Option<(Ts, f64)>,
    /// The position's scale-out stage at the read instant.
    pub stage: Option<u8>,
    /// Absent ⇒ `m_flow_split*` metrics read `NaN` (no pattern context).
    pub flow: Option<ReplayFlow<'a>>,
}

/// Reconstruct a rule's readout at `at` by folding stored trades through a fresh
/// track — the **closed-position** path, where the engine's own state is gone.
///
/// Trades must arrive in execution order; anything after `at` is ignored, so the
/// caller may pass a longer history than it needs (bounding the query is still worth
/// it — this walks every row it is given).
///
/// This is a *driver*, not a second fold: the track, the position context and the
/// condition walk are the same ones `read_state` uses, so the two readouts can only
/// differ through their inputs. That difference is real and is why the result carries
/// [`ReadoutSource::Replay`] — see its docs for what stored rows lose.
///
/// The closing `on_tick(at)` is load-bearing: trailing windows decay by eviction, so
/// without it a token that went quiet before `at` reads its windows as though the last
/// trade had just landed.
pub fn replay_readout(
    rule: &CompiledRule,
    trades: impl IntoIterator<Item = TradeLite>,
    ctx: &ReplayCtx<'_>,
    at: Ts,
) -> RuleReadout {
    let mut track = TokenTrack::new(ctx.created_at);
    for &w in &rule.flow_windows {
        track.ensure_window(w);
    }
    for &w in &rule.price_windows {
        track.ensure_price_window(w);
    }
    if let Some(f) = &ctx.flow {
        // Same order as the live `TokenCreated` arm (`new_track` → `seed_creator`):
        // the seed back-fills every flow state already registered.
        track.ensure_flow(f.fingerprint, f.patterns, &rule.flow_windows);
        if let Some(h) = f.creator_wallet_hash {
            track.seed_creator(h);
        }
    }

    let mut position = ctx.entry.map(|(at, price)| PositionCtx::at_fill(price, at));
    for t in trades {
        if t.at > at {
            continue;
        }
        track.on_trade(t);
        // Ratchet peak/trough only over prices the position actually lived through —
        // mirrors `reduce`'s `fold_entered_extremes`, which runs only on an Entered arm.
        if let (Some(p), Some((entered_at, _))) = (position.as_mut(), ctx.entry) {
            if t.at >= entered_at {
                p.fold_price(t.price);
            }
        }
    }
    track.on_tick(at);

    RuleReadout {
        source: ReadoutSource::Replay,
        arm: None,
        stage: ctx.stage,
        at,
        reads: read_rule(rule, &track, position.as_ref(), ctx.stage, at),
    }
}

/// One req's reading. Mirrors the body of `reqs_exit_fired` / `reqs_satisfied`.
fn read_req(
    r: &MetricReq,
    side: ReadSide,
    track: &TokenTrack,
    ctx: Option<&PositionCtx>,
    price: f64,
    now: Ts,
) -> ConditionRead {
    // `disarmed` means one thing only: **the fold skips this req**. That is a held-side
    // fact — `reqs_exit_fired` consults `trailing_armed`, but the pre-entry walk
    // (`exit_metrics_fired` → `reqs_first_fired`, reached through `can_enter`) does
    // not, so with no position a gated trail is *evaluated*, reads `NaN` as a position
    // metric, and simply fires nothing. Reporting it as skipped there would describe
    // behaviour the engine does not have. The gate itself still travels on
    // `arm_above_pct`, so a caller can show "arms at +N%" whether or not it is skipped.
    let disarmed = ctx.is_some_and(|ctx| !trailing_armed(r.arm_above_pct, ctx, price));

    let value = if r.position_scoped {
        match ctx {
            Some(ctx) => position_value(r.metric, ctx, price, now),
            None => f64::NAN,
        }
    } else {
        track.value(r.metric, r.window, r.fingerprint, now)
    };

    let matched = if disarmed {
        None
    } else {
        first_satisfied_cond(&r.conds, value, r.tolerance)
    };
    // Entry's `eval` is `first_satisfied_cond(..).is_some() || conds.is_empty()`; the
    // exit walk has no vacuous case. Splitting here is what keeps an empty-expr req
    // reading the same as the side that owns it.
    let ok = !disarmed
        && if side.vacuous_when_empty() {
            eval(&r.conds, value, r.tolerance)
        } else {
            matched.is_some()
        };

    ConditionRead {
        side,
        metric: r.metric,
        window: r.window,
        value,
        conds: r.conds.clone(),
        tolerance: r.tolerance,
        matched,
        ok,
        origin: r.origin,
        arm_above_pct: r.arm_above_pct,
        disarmed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{ExitReason, LoadedRule, RuleId, TradeMode};
    use crate::fingerprint::FingerprintId;
    use crate::metrics::evaluator::Operator;
    use crate::metrics::{Side, TradeLite};
    use crate::rule_params::RuleParams;
    use chrono::{Duration, TimeZone, Utc};
    use uuid::Uuid;

    fn ts(secs: i64) -> Ts {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap() + Duration::seconds(secs)
    }

    /// A rule from a `params` JSON blob, compiled as the engine compiles it.
    /// Goes through `RuleParams::parse` — the same validated path rule load uses —
    /// so a fixture can never express params a stored rule could not.
    fn rule(params: serde_json::Value) -> CompiledRule {
        let params = RuleParams::parse(&params).expect("params parse");
        CompiledRule::compile(&LoadedRule {
            id: RuleId(Uuid::nil()),
            fingerprint_id: FingerprintId(Uuid::nil()),
            trade_mode: TradeMode::Paper,
            buy_amount_lamports: 100_000_000,
            max_concurrent_tokens: 1,
            max_total_tokens: 0,
            entry_enabled: true,
            params,
        })
    }

    /// One buy print at `price`, `secs` after creation.
    fn trade(price: f64, secs: i64) -> TradeLite {
        TradeLite {
            side: Side::Buy,
            sol: 1.0,
            price,
            reserve_sol: 30.0,
            at: ts(secs),
            ..Default::default()
        }
    }

    /// A track carrying one print at `price`, `secs` after creation.
    fn track_at(price: f64, secs: i64) -> TokenTrack {
        let mut t = TokenTrack::new(ts(0));
        t.on_trade(trade(price, secs));
        t
    }

    fn find(reads: &[ConditionRead], side: ReadSide, metric: MetricId) -> &ConditionRead {
        reads
            .iter()
            .find(|r| r.side == side && r.metric == metric)
            .expect("read present")
    }

    /// The point of the module: `ok` on an exit read agrees with the fold's own
    /// `exit_fired`, at a price where it fires and at one where it does not.
    #[test]
    fn exit_ok_agrees_with_exit_fired() {
        let c = rule(serde_json::json!({
            "exit": { "m_position": { "retrace": [{ "operator": ">=", "value": 20 }] } }
        }));
        let mut ctx = PositionCtx::at_fill(1.0, ts(0));
        ctx.fold_price(2.0); // peak 2.0 → retrace is measured off this

        // 10% off the peak: no exit.
        let held = track_at(1.8, 10);
        let reads = read_rule(&c, &held, Some(&ctx), None, ts(10));
        assert!(c.exit_fired(&held, &ctx, ts(10)).is_none());
        assert!(!find(&reads, ReadSide::Exit, MetricId::Retrace).ok);

        // 25% off the peak: the fold exits, and so does the readout.
        let dumped = track_at(1.5, 20);
        let reads = read_rule(&c, &dumped, Some(&ctx), None, ts(20));
        let read = find(&reads, ReadSide::Exit, MetricId::Retrace);
        assert!(matches!(
            c.exit_fired(&dumped, &ctx, ts(20)),
            Some(ExitReason::Metrics { metric: MetricId::Retrace, .. })
        ));
        assert!(read.ok);
        assert_eq!(read.matched.map(|m| m.operator), Some(Operator::Gte));
        assert!((read.value - 25.0).abs() < 1e-9);
    }

    /// `reqs_exit_fired` SKIPS a trailing req under `arm_above_pct` rather than
    /// evaluating it false. A readout that evaluated it anyway would show a live trail
    /// on a position the fold is not trailing.
    #[test]
    fn a_trailing_req_under_its_gate_is_disarmed_not_merely_false() {
        let c = rule(serde_json::json!({
            "exit": { "m_position": {
                "arm_above_pct": 50,
                "retrace": [{ "operator": ">=", "value": 10 }]
            } }
        }));
        // Entry 1.0, peak 2.0, now 1.5 → retrace 25 (well past the 10 threshold) but
        // pnl 50 vs a gate of 50.
        let mut ctx = PositionCtx::at_fill(1.0, ts(0));
        ctx.fold_price(2.0);

        // pnl 20 < gate 50 ⇒ the fold skips the req entirely.
        let below = track_at(1.2, 10);
        let read = read_rule(&c, &below, Some(&ctx), None, ts(10));
        let read = find(&read, ReadSide::Exit, MetricId::Retrace);
        assert!(read.disarmed, "under the gate the fold does not evaluate this req");
        assert!(!read.ok);
        assert_eq!(read.arm_above_pct, Some(50.0));
        assert!(c.exit_fired(&below, &ctx, ts(10)).is_none());

        // pnl 60 >= gate ⇒ armed, and retrace 20 >= 10 fires — both agree.
        let above = track_at(1.6, 20);
        let reads = read_rule(&c, &above, Some(&ctx), None, ts(20));
        let read = find(&reads, ReadSide::Exit, MetricId::Retrace);
        assert!(!read.disarmed);
        assert!(read.ok);
        assert!(c.exit_fired(&above, &ctx, ts(20)).is_some());
    }

    /// Entry is AND-of-reqs with a vacuous empty expr; exit is OR-of-reqs where an
    /// empty expr fires nothing. Reading both sides through one combinator silently
    /// flips whichever side it isn't.
    #[test]
    fn each_side_reads_through_its_own_combinator() {
        let c = rule(serde_json::json!({
            "entry": { "m_snapshot": { "liquidity": [{ "operator": ">", "value": 10 }] } },
            "exit":  { "m_price_lifetime": { "stall": [{ "operator": ">", "value": 60 }] } }
        }));
        let track = track_at(1.0, 5);

        let reads = read_rule(&c, &track, None, None, ts(5));
        // Entry: liquidity 30 > 10 holds, matching `entry_satisfied`.
        assert!(find(&reads, ReadSide::Entry, MetricId::Liquidity).ok);
        assert!(c.entry_satisfied(&track, ts(5)));
        // Exit: 5 s since the high, threshold 60 — no fire, matching the fold.
        assert!(!find(&reads, ReadSide::Exit, MetricId::Stall).ok);
        assert!(!c.exit_metrics_satisfied(&track, ts(5)));

        // 90 s of stall flips the exit, and the readout with it.
        let reads = read_rule(&c, &track, None, None, ts(95));
        assert!(find(&reads, ReadSide::Exit, MetricId::Stall).ok);
        assert!(c.exit_metrics_satisfied(&track, ts(95)));
    }

    /// Desugared TP/SL keep their provenance, so a chip reads "take profit", not
    /// "pnl >= 40" — and they lead the exit list, in the fold's own order.
    #[test]
    fn desugared_tp_sl_keep_their_origin_and_fold_order() {
        let c = rule(serde_json::json!({ "take_profit": 40, "stop_loss": 15 }));
        let ctx = PositionCtx::at_fill(1.0, ts(0));
        let track = track_at(1.5, 10);
        let reads = read_rule(&c, &track, Some(&ctx), None, ts(10));

        let exits: Vec<_> = reads.iter().filter(|r| r.side == ReadSide::Exit).collect();
        assert_eq!(exits.len(), 2);
        // Stop-loss is prepended first — the `Dead > StopLoss > TakeProfit` order.
        assert_eq!(exits[0].origin, ReqOrigin::StopLoss);
        assert_eq!(exits[1].origin, ReqOrigin::TakeProfit);
        assert!(exits.iter().all(|r| r.metric == MetricId::Pnl));
        // +50% ⇒ the take-profit holds and the stop does not.
        assert!(!exits[0].ok);
        assert!(exits[1].ok);
        assert!(matches!(
            c.exit_fired(&track, &ctx, ts(10)),
            Some(ExitReason::TakeProfit)
        ));
    }

    /// Only the position's current stage is evaluated by the fold, so every stage is
    /// returned but exactly one is tagged active.
    #[test]
    fn every_ladder_stage_is_returned_and_only_the_current_one_is_active() {
        let c = rule(serde_json::json!({
            "scale_out": [
                { "sell_bps": 5000, "take_profit": 30 },
                { "sell_bps": 4000, "take_profit": 80 }
            ]
        }));
        let ctx = PositionCtx::at_fill(1.0, ts(0));
        let track = track_at(1.5, 10);
        let reads = read_rule(&c, &track, Some(&ctx), Some(1), ts(10));

        let stages: Vec<_> = reads
            .iter()
            .filter(|r| matches!(r.side, ReadSide::Stage { .. }))
            .collect();
        assert_eq!(stages.len(), 2);
        assert_eq!(stages[0].side, ReadSide::Stage { index: 0, active: false });
        assert_eq!(stages[1].side, ReadSide::Stage { index: 1, active: true });
        // Stage 0's `pnl >= 30` reads satisfied at +50%, but the fold is at stage 1
        // and never evaluates it — the `active` flag is the only thing saying so.
        assert!(stages[0].ok);
        assert!(!stages[1].ok);
        assert!(c.stage_fired(1, &track, &ctx, ts(10)).is_none());
    }

    /// A tracked token carrying `track`, with one arm under `rule_id`.
    fn state_with(rule_id: RuleId, track: TokenTrack, arm: crate::arm::ArmState) -> (EngineState, Mint) {
        let mint = Mint::from("MintAAA");
        let mut state = EngineState::new();
        state.tokens.insert(
            mint.clone(),
            crate::state::TokenState {
                created_at: ts(0),
                tf: Default::default(),
                identity: None,
                track,
                last_meaningful_at: None,
                last_trade_at: None,
                settled: None,
                first_slot_settled: true,
                arms: [(rule_id, arm)].into_iter().collect(),
                episodes: Default::default(),
            },
        );
        (state, mint)
    }

    /// `read_state` must resolve the arm's rule the way the fold does — including a
    /// **manual** episode, whose one-off exit rule lives in `manual_rules` keyed by
    /// position, not in `rules`. A resolver that only reads `state.rules` returns
    /// nothing here and the modal shows an empty strip for every manual TP/SL bag.
    #[test]
    fn read_state_resolves_a_manual_episodes_rule_through_its_position() {
        let rule_id = RuleId(Uuid::from_u128(7));
        let position = crate::event::PositionId(42);
        let held = crate::arm::EnteredCtx::at_fill(position, 1.0, ts(0));
        let (mut state, mint) =
            state_with(rule_id, track_at(1.5, 10), crate::arm::ArmState::Entered(held));

        // Nothing in `rules` yet — a manual episode never appears there.
        assert!(read_state(&state, &mint, rule_id, ts(10)).is_none());

        // Install the episode's own exit rule where the fold keeps it.
        state
            .manual_rules
            .insert(position, rule(serde_json::json!({ "take_profit": 40 })));

        let out = read_state(&state, &mint, rule_id, ts(10)).expect("readout");
        assert_eq!(out.source, ReadoutSource::Engine);
        assert_eq!(out.arm, Some("Entered"));
        assert_eq!(out.stage, Some(0));
        // +50% on a 40% take-profit — read through the position context the arm
        // carries, which is the other half `read_state` exists to get right.
        let tp = out
            .reads
            .iter()
            .find(|r| r.origin == ReqOrigin::TakeProfit)
            .expect("tp read");
        assert!(tp.ok);
        assert!((tp.value - 50.0).abs() < 1e-9);
    }

    /// An armed (never-entered) token reads its entry side with no position context,
    /// and reports the arm state so a caller cannot mistake it for a held one.
    #[test]
    fn read_state_reads_an_armed_arm_with_no_position() {
        let rule_id = RuleId(Uuid::from_u128(9));
        let (mut state, mint) =
            state_with(rule_id, track_at(1.0, 5), crate::arm::ArmState::Armed);
        state.rules.insert(
            rule_id,
            rule(serde_json::json!({
                "entry": { "m_snapshot": { "liquidity": [{ "operator": ">", "value": 10 }] } }
            })),
        );

        let out = read_state(&state, &mint, rule_id, ts(5)).expect("readout");
        assert_eq!(out.arm, Some("Armed"));
        assert_eq!(out.stage, None);
        assert!(find(&out.reads, ReadSide::Entry, MetricId::Liquidity).ok);

        // An untracked mint and an unarmed rule are both "nothing to show", not errors.
        assert!(read_state(&state, &Mint::from("Other"), rule_id, ts(5)).is_none());
        assert!(read_state(&state, &mint, RuleId(Uuid::from_u128(99)), ts(5)).is_none());
    }

    /// `disarmed` is a HELD-side fact. The pre-entry walk reached through
    /// `can_enter` (`exit_metrics_fired` → `reqs_first_fired`) never consults
    /// `trailing_armed`, so with no position a gated trail is evaluated — it just
    /// reads `NaN` and fires nothing. Marking it skipped would describe behaviour
    /// the engine does not have.
    #[test]
    fn a_gated_trail_is_not_disarmed_when_there_is_no_position() {
        let c = rule(serde_json::json!({
            "exit": { "m_position": {
                "arm_above_pct": 50,
                "retrace": [{ "operator": ">=", "value": 10 }]
            } }
        }));
        let track = track_at(1.0, 5);
        let reads = read_rule(&c, &track, None, None, ts(5));
        let read = find(&reads, ReadSide::Exit, MetricId::Retrace);

        assert!(!read.disarmed, "no position ⇒ nothing is being skipped");
        assert!(read.value.is_nan(), "a position metric with no position reads NaN");
        assert!(!read.ok);
        // The gate still travels, so a UI can say "arms at +50%" regardless.
        assert_eq!(read.arm_above_pct, Some(50.0));
        // And the fold agrees: the armed side evaluates it and nothing fires.
        assert!(c.exit_metrics_fired(&track, ts(5)).is_none());
    }

    /// The replay driver must land on the same readout as reading a track that was
    /// folded live over the same trades. Same track type, same condition walk — this
    /// pins the *driver* (window setup, fold order, the closing tick, the position
    /// ratchet), which is the only part that can drift.
    #[test]
    fn replay_matches_a_live_folded_track_over_the_same_trades() {
        let c = rule(serde_json::json!({
            "exit": { "m_position": { "retrace": [{ "operator": ">=", "value": 20 }] } }
        }));
        let prints = [(1.0, 0_i64), (2.0, 10), (1.5, 20)];

        // Live shape: fold each print, ratcheting the position ctx as `reduce` does.
        let mut live = TokenTrack::new(ts(0));
        let mut ctx = PositionCtx::at_fill(1.0, ts(0));
        for (p, secs) in prints {
            live.on_trade(trade(p, secs));
            ctx.fold_price(p);
        }
        live.on_tick(ts(20));
        let expected = read_rule(&c, &live, Some(&ctx), None, ts(20));

        let replayed = replay_readout(
            &c,
            prints.map(|(p, secs)| trade(p, secs)),
            &ReplayCtx {
                created_at: ts(0),
                entry: Some((ts(0), 1.0)),
                stage: None,
                flow: None,
            },
            ts(20),
        );

        assert_eq!(replayed.source, ReadoutSource::Replay);
        assert_eq!(replayed.arm, None, "a replay has no arm");
        assert_eq!(replayed.reads, expected);
        // 1.5 off a 2.0 peak = 25% retrace, past the 20 threshold.
        let read = find(&replayed.reads, ReadSide::Exit, MetricId::Retrace);
        assert!(read.ok);
        assert!((read.value - 25.0).abs() < 1e-9);
    }

    /// Reading at the exit instant must not see the future. A token that kept
    /// trading after the position closed would otherwise report a retrace off a peak
    /// the position never lived through — the exact way a post-mortem lies.
    #[test]
    fn replay_ignores_trades_after_the_read_instant() {
        let c = rule(serde_json::json!({
            "exit": { "m_position": { "retrace": [{ "operator": ">=", "value": 20 }] } }
        }));
        // Exit at t=10 with price flat at 1.0; the token then pumps to 5.0 and dumps.
        let trades = [
            trade(1.0, 0),
            trade(1.0, 10),
            trade(5.0, 30),
            trade(1.0, 40),
        ];
        let ctx = ReplayCtx {
            created_at: ts(0),
            entry: Some((ts(0), 1.0)),
            stage: None,
            flow: None,
        };

        let at_exit = replay_readout(&c, trades, &ctx, ts(10));
        let read = find(&at_exit.reads, ReadSide::Exit, MetricId::Retrace);
        assert_eq!(read.value, 0.0, "peak is still the entry price at t=10");
        assert!(!read.ok, "the 80% dump is in the future and must not fire");

        // Read later and the same call does see it — the bound is `at`, not a filter
        // baked into the data.
        let after = replay_readout(&c, trades, &ctx, ts(40));
        assert!(find(&after.reads, ReadSide::Exit, MetricId::Retrace).ok);
    }

    /// Prices before the entry fill must not move the since-entry peak/trough — the
    /// position did not live through them. Mirrors `fold_entered_extremes`, which
    /// only ever runs on an `Entered` arm.
    #[test]
    fn replay_ratchets_the_position_only_from_the_entry_fill() {
        let c = rule(serde_json::json!({
            "exit": { "m_position": { "retrace": [{ "operator": ">=", "value": 90 }] } }
        }));
        // A 10.0 spike BEFORE entry, then entry at 1.0 and a hold at 1.0.
        let trades = [trade(10.0, 0), trade(1.0, 10), trade(1.0, 20)];
        let out = replay_readout(
            &c,
            trades,
            &ReplayCtx {
                created_at: ts(0),
                entry: Some((ts(10), 1.0)),
                stage: None,
                flow: None,
            },
            ts(20),
        );
        let read = find(&out.reads, ReadSide::Exit, MetricId::Retrace);
        // Counting the pre-entry 10.0 as the peak would read 90% retrace and fire.
        assert_eq!(read.value, 0.0);
        assert!(!read.ok);
    }

    /// Without a position, position-scoped reqs read `NaN` and satisfy nothing —
    /// the same behaviour the pre-entry `can_enter` gate relies on.
    #[test]
    fn position_reqs_read_nan_with_no_position() {
        let c = rule(serde_json::json!({
            "exit": { "m_position": { "pnl": [{ "operator": "<=", "value": -15 }] } }
        }));
        let track = track_at(1.0, 5);
        let reads = read_rule(&c, &track, None, None, ts(5));
        let read = find(&reads, ReadSide::Exit, MetricId::Pnl);
        assert!(read.value.is_nan());
        assert!(!read.ok);
        assert!(c.can_enter(&track, ts(5)));
    }
}
