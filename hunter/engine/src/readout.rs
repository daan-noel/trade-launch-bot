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
use crate::metrics::dump_ix::DumpPatterns;
use crate::metrics::flow_ix::FlowPatterns;
use crate::metrics::grid::{fold_sparse, SparseGrid};
use crate::metrics::position::{position_value, trailing_armed, PositionCtx};
use crate::metrics::series::{MetricSeries, SeriesColumn};
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
    pub window: Option<crate::metrics::WindowSpec>,
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
    rule_reqs(rule, stage)
        .into_iter()
        .map(|(side, r)| read_req(r, side, track, side_ctx(side, ctx), price, now))
        .collect()
}

/// Every req of `rule` paired with the side that owns it, in the fold's own order:
/// entry reqs, then exit reqs (desugared stop-loss, take-profit, then authored),
/// then each ladder stage.
///
/// The ONE place that order is expressed. [`read_rule`] and [`replay_series`] both
/// walk it, so a series column and a point read at index `i` are the same condition
/// by construction rather than by two lists agreeing.
fn rule_reqs(rule: &CompiledRule, stage: Option<u8>) -> Vec<(ReadSide, &MetricReq)> {
    let stage_reqs: usize = rule.scale_out.iter().map(|s| s.reqs.len()).sum();
    let mut out = Vec::with_capacity(
        rule.event_reqs.len() + rule.entry_reqs.len() + rule.exit_reqs.len() + stage_reqs,
    );
    out.extend(rule.event_reqs.iter().map(|r| (ReadSide::Entry, r)));
    out.extend(rule.entry_reqs.iter().map(|r| (ReadSide::Entry, r)));
    out.extend(rule.exit_reqs.iter().map(|r| (ReadSide::Exit, r)));
    for (i, s) in rule.scale_out.iter().enumerate() {
        let index = i as u8;
        let side = ReadSide::Stage { index, active: stage == Some(index) };
        out.extend(s.reqs.iter().map(|r| (side, r)));
    }
    out
}

/// The position context a side reads through. Entry reqs are token-scoped by
/// construction (`m_position` is exit-only), so they read with **no** position —
/// matching `reqs_satisfied`, which never consults one.
fn side_ctx(side: ReadSide, ctx: Option<&PositionCtx>) -> Option<&PositionCtx> {
    match side {
        ReadSide::Entry => None,
        _ => ctx,
    }
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
/// usually a token's two largest single flows — as organic, and every `m_flow_ix`
/// condition reads against a different classification than the one that decided.
pub struct ReplayFlow<'a> {
    pub fingerprint: FingerprintId,
    /// `m_flow_ix.ix_patterns`; `None` ⇒ that group alone reads `NaN`.
    pub patterns: Option<&'a FlowPatterns>,
    /// `m_dump_ix.ix_patterns`. Independently optional, because the two lists are
    /// separate groups on one row: a fingerprint may carry either, and gating the
    /// dump registration on the flow list would leave a dump-only rule's conditions
    /// blank in the readout while the live engine evaluates them.
    pub dump: Option<&'a DumpPatterns>,
    /// `m_burst_slot.working_templates`. Independently optional.
    pub burst: Option<&'a crate::metrics::burst_slot::BurstPatterns>,
    /// Seeds the flow contagion set only — `m_dump_ix` has no wallet rule.
    pub creator_wallet_hash: Option<u64>,
}

/// Everything a replay needs besides the rule, the trades, and the instant.
pub struct ReplayCtx<'a> {
    /// Token creation instant — what `m_state.time` and the lifetime extrema
    /// anchor on. Pass the first trade's `block_time`, which is what the replay
    /// driver and the lab's metric-series both use (the dev-buy slot).
    pub created_at: Ts,
    /// Entry fill `(time, price)`; `None` for a position that never entered, whose
    /// `m_position` reads then have no anchor and stay `NaN`.
    pub entry: Option<(Ts, f64)>,
    /// The position's scale-out stage at the read instant.
    pub stage: Option<u8>,
    /// Absent ⇒ `m_flow_ix*` and `m_dump_ix*` metrics read `NaN` (no pattern context).
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
    // One bucket per backing buffer, exactly as `EngineState::new_track` registers
    // them — a span put on the wrong buffer reads NaN and the readout would show a
    // condition the live engine evaluates as a blank row.
    for &w in &rule.flow_windows {
        track.ensure_window(w);
    }
    for &w in &rule.crowd_windows {
        track.ensure_crowd_window(w);
    }
    for &w in &rule.price_windows {
        track.ensure_price_window(w);
    }
    if let Some(f) = &ctx.flow {
        // Same order as the live `TokenCreated` arm (`new_track` → `seed_creator`):
        // the seed back-fills every flow state already registered.
        if let Some(p) = f.patterns {
            track.ensure_flow(f.fingerprint, p, &rule.ix_windows);
        }
        if let Some(d) = f.dump {
            track.ensure_dump(f.fingerprint, d, &rule.dump_windows);
        }
        if let Some(b) = f.burst {
            track.ensure_burst(f.fingerprint, b);
        }
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
    // See `reduce.rs` - a tick carries no slot; slot windows hold their cursor.
    track.on_tick(at, None);

    RuleReadout {
        source: ReadoutSource::Replay,
        arm: None,
        stage: ctx.stage,
        at,
        reads: read_rule(rule, &track, position.as_ref(), ctx.stage, at),
    }
}

/// One condition read across a whole series: the req itself (so a caller renders
/// `metric op threshold` once, not per row) plus one entry per row.
///
/// `values` / `ok` / `disarmed` are parallel to [`ReadoutSeries::at`] and to each
/// other. Reading a row back as the point routes' [`ConditionRead`] is
/// [`row`](Self::row) — the same judgement body, so the two shapes cannot disagree.
#[derive(Debug, Clone, PartialEq)]
pub struct ConditionSeries {
    /// The compiled requirement this column reads — the one the fold evaluates.
    pub req: MetricReq,
    pub side: ReadSide,
    /// The value the fold reads at each row; `NaN` where unreadable.
    pub values: Vec<f64>,
    /// Whether the req is satisfied at each row, under its side's combinator.
    pub ok: Vec<bool>,
    /// Whether the fold **skips** the req at each row. Per row, not per series: a
    /// gated trail arms and disarms as the position's PnL crosses `arm_above_pct`,
    /// so collapsing this to one flag would erase exactly the distinction it exists
    /// to carry. All-`false` unless `req.arm_above_pct` is set.
    pub disarmed: Vec<bool>,
}

impl ConditionSeries {
    /// Row `i` as the point routes' [`ConditionRead`].
    pub fn row(&self, i: usize) -> ConditionRead {
        judge_req(&self.req, self.side, self.values[i], self.disarmed[i])
    }

    /// Rows recorded.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// A rule's whole readout over a token's event stream — [`replay_readout`]'s answer
/// at every row of the engine's decision grid instead of at one instant.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadoutSeries {
    /// Row instants, ascending. Trades plus the sparse grid's ticks.
    pub at: Vec<Ts>,
    /// One per condition, in [`read_rule`]'s order.
    pub conditions: Vec<ConditionSeries>,
    /// A row budget stopped the fold before the tail — coverage ends at
    /// `covered_until` and the caller must say so. A silently short series reads
    /// exactly like a token that stopped trading.
    pub truncated: bool,
    /// The last instant the series covers.
    pub covered_until: Ts,
    /// The first instant the series covers — the first event unless the caller passed
    /// a `record_from`. Coverage is a span with two ends, and a client that only knows
    /// the far one cannot tell "before the window" from "at the window's first row".
    pub covered_from: Ts,
}

/// The [`MetricSeries`] column a req reads; `None` for a position-scoped req, which
/// is not a track column at all and folds from the [`PositionCtx`] per row.
///
/// Mirrors `read_req`'s `track.value(metric, window, fingerprint, now)` argument for
/// argument — a [`SeriesColumn`] evaluates to that same call, so a mismatch here
/// would read a *different* metric rather than fail.
fn req_column(r: &MetricReq) -> Option<SeriesColumn> {
    if r.position_scoped {
        return None;
    }
    // The WHOLE carrier goes through on the windowed arm: a two-window req that lost
    // its second axis here would read NaN at every row and draw as a condition that
    // simply never holds — a blank timeline, not an error. `m_flow_ix*` is
    // single-window by construction, so the flow arm still takes `primary`.
    Some(match (r.fingerprint, r.window.is_windowed()) {
        (Some(fp), _) => SeriesColumn::Fingerprint(r.metric, r.window.primary, fp),
        (None, true) => SeriesColumn::Window(r.metric, r.window),
        (None, false) => SeriesColumn::Static(r.metric),
    })
}

/// Reconstruct a rule's readout at **every** row of the token's event stream, by
/// folding stored trades through the shared sparse tick grid.
///
/// The series twin of [`replay_readout`], and deliberately built from the same
/// parts: the same `TokenTrack` (inside a [`MetricSeries`]), the same window
/// registration, the same position ratchet, and the same [`judge_req`] body. The
/// contract between them is that **the row at an instant equals `replay_readout` at
/// that instant**, condition for condition — pinned by a test at the bottom of this
/// file.
///
/// Only the columns *this rule* reads are folded, so a 6-condition rule folds 6
/// columns where the lab's `metric-series` folds the whole registry. The grid's
/// density comes off [`CompiledRule::clock_horizons`], so the caller declares
/// nothing: unlike the lab, the evaluation happens here.
///
/// The tick grid is load-bearing, not a nicety. Every decaying metric advances only
/// inside a tick, so a trade-only fold never samples a between-trades crossing and a
/// hovered instant in a quiet gap would read as though the last trade had just
/// landed.
///
/// `as_of` bounds the tail (the driver caps it at `last_trade + DEAD_QUIET +
/// TAIL_MARGIN` regardless); `max_rows` bounds the recorded rows and reports
/// [`ReadoutSeries::truncated`] when it bites.
///
/// `record_from` moves where the recorded span *starts* without moving where the fold
/// starts: rows before it are folded and then discarded, so `max_rows` buys its span
/// around the caller's window instead of around the token's first trade. `None` records
/// everything. It cannot change a value — lifetime metrics still fold from creation.
/// See [`MetricSeries::set_record_from`].
pub fn replay_series(
    rule: &CompiledRule,
    trades: impl IntoIterator<Item = TradeLite>,
    ctx: &ReplayCtx<'_>,
    as_of: Ts,
    max_rows: Option<usize>,
    record_from: Option<Ts>,
) -> ReadoutSeries {
    let reqs = rule_reqs(rule, ctx.stage);

    // Deduped columns: two reqs on the same (metric, window, fingerprint) fold once
    // and read the same column.
    let mut columns: Vec<SeriesColumn> = Vec::new();
    let col_of: Vec<Option<usize>> = reqs
        .iter()
        .map(|(_, r)| {
            req_column(r).map(|c| {
                columns.iter().position(|x| *x == c).unwrap_or_else(|| {
                    columns.push(c);
                    columns.len() - 1
                })
            })
        })
        .collect();

    let mut series = MetricSeries::new(ctx.created_at, columns);
    // Window + flow setup, in `replay_readout`'s exact order (`new_track` →
    // `seed_creator`, as the live `TokenCreated` arm does it): the seed back-fills
    // every flow state already registered.
    for &w in &rule.flow_windows {
        series.ensure_window(w);
    }
    for &w in &rule.crowd_windows {
        series.ensure_crowd_window(w);
    }
    for &w in &rule.price_windows {
        series.ensure_price_window(w);
    }
    if let Some(f) = &ctx.flow {
        if let Some(p) = f.patterns {
            series.ensure_flow(f.fingerprint, p, &rule.ix_windows);
        }
        if let Some(d) = f.dump {
            series.ensure_dump(f.fingerprint, d, &rule.dump_windows);
        }
        if let Some(b) = f.burst {
            series.ensure_burst(f.fingerprint, b);
        }
        if let Some(h) = f.creator_wallet_hash {
            series.seed_creator(h);
        }
    }
    if let Some(from) = record_from {
        series.set_record_from(from);
    }

    let h = rule.clock_horizons;
    let grid = SparseGrid {
        max_window_secs: h.max_window_secs,
        time_horizon_secs: h.time_secs,
        // `held` climbs from the entry fill and the grid has no slot for it, so it
        // rides on the `stall` horizon, which is measured from the last trade. Every
        // instant `held` can still cross is at or after the entry fill, and the entry
        // fill is at or before the last trade, so `last_trade + held_secs` covers it.
        // Over-wide costs ticks; too narrow drops the row a crossing lands on.
        stall_horizon_secs: h.stall_secs.max(h.held_secs),
    };
    let fold = fold_sparse(
        &mut series,
        ctx.created_at,
        trades.into_iter().map(|t| (t, None)),
        &grid,
        as_of,
        max_rows,
    );

    let n = series.n_rows();
    let mut out: Vec<ConditionSeries> = reqs
        .iter()
        .map(|(side, r)| ConditionSeries {
            req: (*r).clone(),
            side: *side,
            values: Vec::with_capacity(n),
            ok: Vec::with_capacity(n),
            disarmed: Vec::with_capacity(n),
        })
        .collect();

    let mut position = ctx.entry.map(|(at, price)| PositionCtx::at_fill(price, at));
    for i in 0..n {
        let now = series.at[i];
        let price = series.price[i];
        // Ratchet peak/trough only over prices the position lived through — mirrors
        // `reduce`'s `fold_entered_extremes`, which runs only on an Entered arm. A
        // tick row carries the last print, so re-folding it is a no-op.
        let mut entered = false;
        if let (Some(p), Some((entered_at, _))) = (position.as_mut(), ctx.entry) {
            if now >= entered_at {
                p.fold_price(price);
                entered = true;
            }
        }
        // Before the entry fill there is no position, so position-scoped reads are
        // `NaN` — the same thing the live fold sees on an un-entered arm. (The point
        // replay never lands there: it reads at the entry or exit fill.)
        let held = if entered { position.as_ref() } else { None };

        for (k, col) in out.iter_mut().enumerate() {
            // `read_req`'s body, with the value coming off a folded column instead of
            // a live track — including the entry side reading with no position.
            let pos = side_ctx(col.side, held);
            let disarmed = pos.is_some_and(|c| !trailing_armed(col.req.arm_above_pct, c, price));
            let value = match col_of[k] {
                Some(idx) => series.value_at(i, idx),
                None => match pos {
                    Some(c) => position_value(col.req.metric, c, price, now),
                    None => f64::NAN,
                },
            };
            let read = judge_req(&col.req, col.side, value, disarmed);
            col.values.push(read.value);
            col.ok.push(read.ok);
            col.disarmed.push(read.disarmed);
        }
    }

    ReadoutSeries {
        at: series.at,
        conditions: out,
        truncated: fold.truncated,
        covered_until: fold.covered_until,
        covered_from: fold.covered_from,
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

    judge_req(r, side, value, disarmed)
}

/// Judge one already-read `value` against its req — the half of [`read_req`] that
/// turns a number into a decision.
///
/// Split out so a **series** can reuse it: the per-row read resolves `value` and
/// `disarmed` from a [`MetricSeries`] column instead of from a live track, then
/// judges here. Everything a second evaluation gets wrong (the per-side combinator,
/// the disarmed skip, which condition matched) therefore has exactly one body, and
/// the point route and the series route cannot disagree about it.
fn judge_req(r: &MetricReq, side: ReadSide, value: f64, disarmed: bool) -> ConditionRead {
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
        window: r.window.primary,
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
    use crate::metrics::WindowSpec;
    use super::*;
    use crate::event::{ExitReason, LoadedRule, RuleId, TradeMode};
    use crate::fingerprint::FingerprintId;
    use crate::metrics::evaluator::Operator;
    use crate::metrics::dump_ix::DumpPatterns;
    use crate::metrics::flow_ix::ix_hash;
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
            "entry": { "m_state": { "liquidity": [{ "operator": ">", "value": 10 }] } },
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
                entry_locks: Default::default(),
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
        let held = crate::arm::EnteredCtx::at_fill(position, 1.0, ts(0), None);
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
                "entry": { "m_state": { "liquidity": [{ "operator": ">", "value": 10 }] } }
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
        live.on_tick(ts(20), None);
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

    /// **A readout must register every fingerprint-scoped group the rule reads, not
    /// just the flow one.** `m_dump_ix` lives on the same row as `m_flow_ix` but in
    /// its own state, so a driver that registers only flow returns `NaN` for a
    /// condition the live engine evaluates — a post-mortem that says "never held"
    /// about the term that fired.
    #[test]
    fn replay_registers_the_dump_group_the_rule_reads() {
        let c = rule(serde_json::json!({
            "exit": {
                "m_dump_ix": { "dump_sell_count": [{ "operator": ">=", "value": 2 }] }
            }
        }));
        let build = ix_hash(&["Pump.Fun: Sell"]);
        let dump = DumpPatterns::new(std::collections::BTreeSet::from([build]));
        let sell = |secs: i64| TradeLite {
            side: Side::Sell,
            sol: 1.0,
            price: 1.0,
            reserve_sol: 30.0,
            at: ts(secs),
            ix_hash: Some(build),
            ..Default::default()
        };

        let ctx = ReplayCtx {
            created_at: ts(0),
            entry: Some((ts(0), 1.0)),
            stage: None,
            flow: Some(ReplayFlow {
                fingerprint: FingerprintId(Uuid::nil()),
                // The dump-ONLY case: no tagged list on the row at all, which is the
                // shape that gated the whole context off and blanked the condition.
                patterns: None,
                dump: Some(&dump),
                burst: None,
                creator_wallet_hash: None,
            }),
        };

        let one = replay_readout(&c, [sell(1)], &ctx, ts(5));
        let read = find(&one.reads, ReadSide::Exit, MetricId::DumpSellCount);
        assert_eq!(read.value, 1.0, "one listed sell counts one");
        assert!(!read.ok);

        let two = replay_readout(&c, [sell(1), sell(2)], &ctx, ts(5));
        assert!(find(&two.reads, ReadSide::Exit, MetricId::DumpSellCount).ok);

        // Without the context the metric has no state to read — NaN, never 0, so the
        // difference between "no dump" and "not configured" stays visible.
        let blind = replay_readout(
            &c,
            [sell(1), sell(2)],
            &ReplayCtx { created_at: ts(0), entry: Some((ts(0), 1.0)), stage: None, flow: None },
            ts(5),
        );
        assert!(find(&blind.reads, ReadSide::Exit, MetricId::DumpSellCount).value.is_nan());
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

    /// **The contract between the point route and the series route.** The series
    /// row at an instant must equal `replay_readout` at that instant, condition for
    /// condition — otherwise hovering a chart tells a different story from the
    /// strip's own exit readout, with nothing on screen saying which one lies.
    ///
    /// Checked at both kinds of row: a **tick** (where the series' `push_tick` and
    /// the point replay's closing `on_tick` are literally the same call) and a
    /// **trade** (where the point replay additionally ticks at the trade's own
    /// instant — a no-op only because window eviction is idempotent at a fixed
    /// `now`, which is the part worth pinning).
    #[test]
    fn a_series_row_equals_the_point_replay_at_that_instant() {
        let c = rule(serde_json::json!({
            "entry": { "m_state": { "liquidity": [{ "operator": ">", "value": 10 }] } },
            "exit": {
                "m_position": { "retrace": [{ "operator": ">=", "value": 20 }] },
                "m_price_lifetime": { "stall": [{ "operator": ">", "value": 15 }] },
                "m_flow_window": [{
                    "window_size_sec": 30,
                    "buy": [{ "operator": "<", "value": 2 }]
                }]
            },
            "take_profit": 60
        }));
        let prints = [(1.0, 0_i64), (2.0, 10), (1.5, 20)];
        let entry = Some((ts(0), 1.0));
        let replay_ctx = |stage| ReplayCtx {
            created_at: ts(0),
            entry,
            stage,
            flow: None,
        };

        let series = replay_series(
            &c,
            prints.map(|(p, secs)| trade(p, secs)),
            &replay_ctx(None),
            ts(40),
            None,
            None,
        );
        assert!(!series.truncated);
        assert!(series.at.len() > prints.len(), "the grid must emit ticks, not only trades");

        // A trade instant and a tick instant well inside the covered span.
        for at in [ts(20), ts(15)] {
            let i = series
                .at
                .iter()
                .position(|t| *t == at)
                .unwrap_or_else(|| panic!("a row at {at}"));
            let point = replay_readout(
                &c,
                prints.map(|(p, secs)| trade(p, secs)),
                &replay_ctx(None),
                at,
            );
            assert_eq!(point.reads.len(), series.conditions.len());
            for (k, col) in series.conditions.iter().enumerate() {
                assert_eq!(
                    col.row(i),
                    point.reads[k],
                    "condition {k} ({:?}) disagrees at {at}",
                    col.req.metric,
                );
            }
        }
    }

    /// The series folds only the columns THIS rule reads, deduped — the reason it is
    /// affordable on the deploy box where the lab's whole-registry series is not.
    /// Position-scoped reqs are not track columns at all.
    #[test]
    fn only_the_rules_own_columns_are_folded() {
        let c = rule(serde_json::json!({
            "exit": {
                // Two reqs on the SAME (metric, window) pair → one column.
                "m_flow_window": [{
                    "window_size_sec": 30,
                    "buy": [{ "operator": "<", "value": 2 }],
                    "sell": [{ "operator": ">", "value": 9 }]
                }],
                "m_position": { "pnl": [{ "operator": ">=", "value": 40 }] }
            }
        }));
        let reqs = rule_reqs(&c, None);
        assert_eq!(reqs.len(), 3);
        let cols: Vec<_> = reqs.iter().filter_map(|(_, r)| req_column(r)).collect();
        assert_eq!(
            cols,
            vec![
                SeriesColumn::window(MetricId::Buy, WindowSpec::secs(30.0)),
                SeriesColumn::window(MetricId::Sell, WindowSpec::secs(30.0)),
            ],
            "m_position is not a track column",
        );
    }

    /// Position-scoped columns are blank before the entry fill — there is no
    /// position, and the live fold reads `NaN` on an un-entered arm. Hovering the
    /// pre-entry span must not show a `pnl` measured against a fill that had not
    /// happened yet.
    #[test]
    fn position_columns_are_blank_before_the_entry_fill() {
        let c = rule(serde_json::json!({
            "exit": { "m_position": { "pnl": [{ "operator": ">=", "value": 40 }] } }
        }));
        let prints = [(1.0, 0_i64), (2.0, 10), (3.0, 20)];
        let series = replay_series(
            &c,
            prints.map(|(p, secs)| trade(p, secs)),
            &ReplayCtx { created_at: ts(0), entry: Some((ts(10), 2.0)), stage: None, flow: None },
            ts(20),
            None,
            None,
        );
        let pnl = &series.conditions[0];
        for (i, at) in series.at.iter().enumerate() {
            if *at < ts(10) {
                assert!(pnl.values[i].is_nan(), "pre-entry row at {at} has a pnl");
                assert!(!pnl.ok[i]);
            }
        }
        // At the entry fill: flat. At the +50% print: fired.
        let at_entry = series.at.iter().position(|t| *t == ts(10)).expect("entry row");
        assert_eq!(pnl.values[at_entry], 0.0);
        let at_exit = series.at.iter().position(|t| *t == ts(20)).expect("exit row");
        assert!((pnl.values[at_exit] - 50.0).abs() < 1e-9);
        assert!(pnl.ok[at_exit]);
    }

    /// A gated trail arms and disarms as PnL crosses `arm_above_pct`, so `disarmed`
    /// is a per-ROW fact. One flag for the whole series would erase the distinction
    /// the point readout goes out of its way to carry.
    #[test]
    fn the_disarmed_flag_moves_row_by_row() {
        let c = rule(serde_json::json!({
            "exit": { "m_position": {
                "arm_above_pct": 50,
                "retrace": [{ "operator": ">=", "value": 10 }]
            } }
        }));
        // Entry 1.0, +20% (under the gate), +80% (armed), then 20% off that peak.
        let prints = [(1.0, 0_i64), (1.2, 10), (1.8, 20), (1.44, 30)];
        let series = replay_series(
            &c,
            prints.map(|(p, secs)| trade(p, secs)),
            &ReplayCtx { created_at: ts(0), entry: Some((ts(0), 1.0)), stage: None, flow: None },
            ts(30),
            None,
            None,
        );
        let trail = &series.conditions[0];
        let row = |at: Ts| series.at.iter().position(|t| *t == at).expect("row");
        assert!(trail.disarmed[row(ts(10))], "pnl 20 < gate 50 ⇒ the fold skips it");
        assert!(!trail.ok[row(ts(10))]);
        assert!(!trail.disarmed[row(ts(20))], "pnl 80 ⇒ armed");
        // 1.44 off the 1.8 peak = 20% retrace, past the 10 threshold, and pnl 44 is
        // back under the gate — the fold stops evaluating it again.
        assert!(trail.disarmed[row(ts(30))]);
        assert!(!trail.ok[row(ts(30))]);
    }

    /// The **armed** series shape: no entry fill, no stage, so a Waiting row's
    /// entry conditions read real values while its position-scoped ones stay blank.
    ///
    /// That asymmetry IS the answer the Waiting modal exists to give — "which entry
    /// condition is holding this out" — and it is exactly what `can_enter` sees
    /// pre-entry. The trap: an exit condition that reads `NaN` must never come back
    /// `ok`, or the strip would show a row leaving before it ever entered.
    #[test]
    fn an_armed_series_reads_entry_conditions_and_blanks_position_ones() {
        let c = rule(serde_json::json!({
            "entry": { "m_state": { "liquidity": [{ "operator": ">=", "value": 20 }] } },
            "exit": { "m_position": { "pnl": [{ "operator": ">=", "value": 40 }] } }
        }));
        let prints = [(1.0, 0_i64), (2.0, 10), (3.0, 20)];
        let series = replay_series(
            &c,
            prints.map(|(p, secs)| trade(p, secs)),
            // The armed anchor: nothing has filled and no ladder is running.
            &ReplayCtx { created_at: ts(0), entry: None, stage: None, flow: None },
            ts(20),
            None,
            None,
        );
        let entry = series
            .conditions
            .iter()
            .find(|c| c.req.metric == MetricId::Liquidity)
            .expect("the entry condition");
        let pnl = series
            .conditions
            .iter()
            .find(|c| c.req.metric == MetricId::Pnl)
            .expect("the position condition");

        assert!(
            entry.values.iter().any(|v| v.is_finite()),
            "the entry side must read the token, not the absent position",
        );
        // `reserve_sol` 30 − the 30 virtual floor = 0 real, under the 20 threshold:
        // a readable value that simply does not hold, which is the useful answer.
        assert!(entry.values.iter().all(|v| v.is_finite()));
        for (i, v) in pnl.values.iter().enumerate() {
            assert!(v.is_nan(), "row {i} has a pnl with no entry fill");
            assert!(!pnl.ok[i], "row {i} satisfies an exit on an unreadable metric");
        }
    }

    /// A row budget stops the fold and says so, exactly as the grid reports it — a
    /// silently short series reads like a token that stopped trading.
    #[test]
    fn a_row_budget_truncates_the_series_and_reports_it() {
        let c = rule(serde_json::json!({
            "exit": { "m_price_lifetime": { "stall": [{ "operator": ">", "value": 600 }] } }
        }));
        let prints = [(1.0, 0_i64), (1.0, 600)];
        let series = replay_series(
            &c,
            prints.map(|(p, secs)| trade(p, secs)),
            &ReplayCtx { created_at: ts(0), entry: None, stage: None, flow: None },
            ts(600),
            Some(50),
            None,
        );
        assert!(series.truncated);
        assert_eq!(series.at.len(), 50);
        assert_eq!(series.covered_until, *series.at.last().unwrap());
        assert!(series.conditions.iter().all(|c| c.len() == 50));
    }

    /// `record_from` moves the recorded window; it must NEVER move the fold.
    ///
    /// The trap it guards: "start folding later" looks like the same optimisation and
    /// is not. `m_price_lifetime`/`time` are defined from token creation, so a later
    /// fold start silently reports different numbers — a wrong answer rather than a
    /// missing one. Here the same rows are compared with and without the window, and
    /// the values on the overlap must be bit-identical.
    #[test]
    fn record_from_moves_coverage_but_never_a_value() {
        let c = rule(serde_json::json!({
            "exit": { "m_price_lifetime": { "stall": [{ "operator": ">=", "value": 900 }] } }
        }));
        let trades: Vec<_> = (0..600).map(|i| trade(1.0 + i as f64 * 0.001, i)).collect();
        let ctx = ReplayCtx { created_at: ts(0), entry: None, stage: None, flow: None };

        let full = replay_series(&c, trades.clone(), &ctx, ts(600), None, None);
        let windowed = replay_series(&c, trades, &ctx, ts(600), None, Some(ts(300)));

        assert_eq!(full.covered_from, full.at[0], "no window ⇒ coverage starts at row 0");
        assert!(windowed.covered_from >= ts(300), "the window must clip the head");
        assert!(windowed.at.len() < full.at.len(), "withheld rows are not recorded");
        assert_eq!(
            windowed.covered_until, full.covered_until,
            "the tail is untouched — only the start moved",
        );

        // Every windowed row is the full fold's row at the same instant, value for
        // value. If the fold had restarted at `record_from`, `stall` (measured from
        // the all-time high, which is before it) would differ here.
        let offset = full.at.iter().position(|t| *t == windowed.at[0]).expect("row");
        for (i, at) in windowed.at.iter().enumerate() {
            assert_eq!(*at, full.at[offset + i]);
            for (w, f) in windowed.conditions.iter().zip(&full.conditions) {
                assert_eq!(
                    w.values[i], f.values[offset + i],
                    "value drift at {at} — the fold moved, not just the window",
                );
                assert_eq!(w.ok[i], f.ok[offset + i]);
            }
        }
    }

    /// A withheld row costs no budget, so the cap buys its span around the window.
    ///
    /// This is the whole point of `record_from`: an 8k budget is ~22 minutes of grid,
    /// and a position entered later than that is otherwise *entirely* uncovered.
    #[test]
    fn record_from_spends_the_row_budget_on_the_requested_window() {
        let c = rule(serde_json::json!({
            "exit": { "m_position": { "held": [{ "operator": ">=", "value": 600 }] } }
        }));
        let trades: Vec<_> = (0..3600).map(|i| trade(1.0, i)).collect();
        let ctx = ReplayCtx { created_at: ts(0), entry: None, stage: None, flow: None };
        const CAP: usize = 1_000;

        let head = replay_series(&c, trades.clone(), &ctx, ts(3600), Some(CAP), None);
        let windowed = replay_series(&c, trades, &ctx, ts(3600), Some(CAP), Some(ts(3000)));

        assert!(head.truncated && windowed.truncated);
        assert_eq!(head.at.len(), CAP);
        assert_eq!(windowed.at.len(), CAP, "the budget is spent, just later");
        assert!(
            head.covered_until < ts(3000) && windowed.covered_from >= ts(3000),
            "the same budget covers a different span: {} vs {}",
            head.covered_until,
            windowed.covered_from,
        );
    }

    /// The row cap is a **coverage duration**, not a payload size.
    ///
    /// The grid emits a tick every `TICK_MS` for as long as any horizon is still
    /// open, and a rule with a time stop holds one open for longer than any gap an
    /// actively-traded token leaves. So the series records `1000/TICK_MS` rows per
    /// second of coverage almost regardless of how often the token prints, and a cap
    /// of N rows buys `N·TICK_MS/1000` seconds of chart — after which the readout
    /// stops following the crosshair.
    ///
    /// Pinned here because that conversion is the whole basis for choosing a cap,
    /// and nothing about `MAX_READOUT_SERIES_ROWS` reveals it at the call site.
    #[test]
    fn the_row_cap_buys_a_fixed_span_of_chart_not_a_fixed_payload() {
        let c = rule(serde_json::json!({
            "exit": { "m_position": { "held": [{ "operator": ">=", "value": 600 }] } }
        }));
        let rows_per_sec = 1000 / crate::TICK_MS;
        const CAP: usize = 8_000;

        // An hour of trading. The print cadence is deliberately varied across the
        // two runs: if coverage tracked trade COUNT the two would differ, and the
        // point is that they do not.
        for cadence_secs in [1_i64, 5] {
            let n = 3600 / cadence_secs;
            let trades: Vec<_> = (0..n).map(|i| trade(1.0, i * cadence_secs)).collect();
            let series = replay_series(
                &c,
                trades,
                &ReplayCtx {
                    created_at: ts(0),
                    entry: Some((ts(0), 1.0)),
                    stage: None,
                    flow: None,
                },
                ts(3600),
                Some(CAP),
                None,
            );
            assert!(series.truncated, "an hour must not fit under the cap");
            let covered = (series.covered_until - ts(0)).num_seconds();
            let budget = CAP as i64 / rows_per_sec;
            // Trades occupy rows too, so coverage lands at or just under the budget.
            assert!(
                covered <= budget && covered * 4 >= budget * 3,
                "cadence {cadence_secs}s covered {covered}s, expected ~{budget}s",
            );
        }
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
