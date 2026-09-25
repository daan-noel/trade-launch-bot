//! Rule readout — what the fold reads for one (coin, rule) **right now**, placed where
//! each condition sits in the rule.
//!
//! The decision path already computes this: [`TokenTrack`] folds every metric on each
//! trade and tick, and [`CompiledRule`] compiles a rule into entry conditions, signals,
//! `always` lines and stages. This module exposes that state read-only, so a UI can show
//! the live value beside each authored threshold, which lines hold, and which stage the
//! position is in — without re-folding anything.
//!
//! **A mirror, not a lookalike.** Every read goes through [`MetricReq::read`] and the
//! same [`evaluator`] the fold uses, and every line through the rule's own condition
//! walk, so the readout cannot report a condition the engine would decide differently.
//! The entry side reads with no position (position metrics read `NaN` there), exactly
//! as the pre-entry decision does.
//!
//! Read on demand (a modal, an API call), never per event.
//!
//! [`evaluator`]: crate::metrics::evaluator

use crate::arm::{CompiledLine, CompiledRule, CondReq, MetricReq};
use crate::event::{ExitReason, Mint, RuleId};
use crate::fingerprint::FingerprintId;
use crate::metrics::evaluator::{eval, first_satisfied_cond, Condition, ConditionExpr};
use crate::metrics::grid::{fold_sparse, SparseGrid};
use crate::metrics::position::{position_value, PositionCtx};
use crate::metrics::series::{MetricSeries, SeriesColumn};
use crate::metrics::tags::config::CompiledTag;
use crate::metrics::track::TokenTrack;
use crate::metrics::{MetricRef, TradeLite, Ts};
use crate::state::EngineState;

/// Where in the rule a condition sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadPart {
    /// `enter.event`.
    Event,
    /// `enter.filters`.
    Filter,
    /// `enter.final_filters`.
    FinalFilter,
    /// Group `group` of signal `signal` (index into [`CompiledRule::signals`]), named
    /// `name`.
    Signal { signal: u16, name: &'static str, group: u16 },
    /// `always` line `line` (the TP/SL shortcuts first).
    Always { line: u16 },
    /// Line `line` of stage `stage` (named `name`), in its `on` or its `at_end` list.
    Stage { stage: u8, name: &'static str, at_end: bool, line: u16 },
}

impl ReadPart {
    /// Entry conditions read with no position, as the pre-entry decision does.
    fn is_entry(self) -> bool {
        matches!(self, Self::Event | Self::Filter | Self::FinalFilter)
    }
}

/// One metric condition, at one instant: what it reads, the value the fold sees, and
/// whether it holds.
#[derive(Debug, Clone, PartialEq)]
pub struct ConditionRead {
    pub part: ReadPart,
    pub r: MetricRef,
    /// The value the fold reads. `NaN` when unreadable (an unregistered buffer, a tag the
    /// fingerprint does not define, a position metric with no position) — which
    /// satisfies nothing.
    pub value: f64,
    /// The authored DNF (`OR` of `AND` arms).
    pub conds: ConditionExpr,
    /// The metric's `=` band.
    pub tolerance: f64,
    /// The first condition on the first satisfied arm, when one holds.
    pub matched: Option<Condition>,
    pub ok: bool,
}

/// One line at one instant: whether all its conditions hold, and what it would do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineRead {
    pub part: ReadPart,
    pub holds: bool,
    /// The exit reason when the line sells.
    pub sells: Option<ExitReason>,
    /// Basis points of the first bag for a partial sell.
    pub sell_bps: Option<u16>,
    /// The stage index it moves to.
    pub goes_to: Option<u8>,
}

/// One signal at one instant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SignalRead {
    pub name: &'static str,
    pub holds: bool,
}

/// Where a readout's numbers come from. The two are not equally trustworthy and a UI
/// must say which it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadoutSource {
    /// Read out of the live fold's own track. Exact by construction.
    Engine,
    /// Reconstructed by folding stored trades through a fresh track
    /// ([`replay_readout`]): stored rows carry an approximated real reserve, and a trade
    /// the feed saw but never persisted is absent.
    Replay,
}

/// One (coin, rule) arm's readout.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleReadout {
    pub source: ReadoutSource,
    /// The arm's lifecycle state ([`ArmState::tag`](crate::arm::ArmState::tag)); `None`
    /// on a replay.
    pub arm: Option<&'static str>,
    /// The held position's stage index and name; `None` when nothing is held.
    pub stage: Option<(u8, &'static str)>,
    /// The instant every value is read at.
    pub at: Ts,
    pub conditions: Vec<ConditionRead>,
    pub signals: Vec<SignalRead>,
    pub lines: Vec<LineRead>,
}

/// Every metric condition of `rule` with its part, in the rule's order: entry (event,
/// final filters, filters), signals, `always` lines, then each stage's `on` and `at_end`
/// lines. The ONE place that order is expressed: [`read_rule`] and [`replay_series`] both
/// walk it, so a series column and a point read at index `i` are the same condition.
fn listed_reqs(rule: &CompiledRule) -> Vec<(ReadPart, &MetricReq)> {
    let mut out = Vec::new();
    push_conds(&mut out, ReadPart::Event, &rule.event);
    push_conds(&mut out, ReadPart::FinalFilter, &rule.final_filters);
    push_conds(&mut out, ReadPart::Filter, &rule.filters);
    for (si, s) in rule.signals.iter().enumerate() {
        for (gi, g) in s.groups.iter().enumerate() {
            for m in g {
                out.push((ReadPart::Signal { signal: si as u16, name: s.name, group: gi as u16 }, m));
            }
        }
    }
    for (part, l) in lines(rule) {
        push_conds(&mut out, part, &l.conds);
    }
    out
}

fn push_conds<'a>(out: &mut Vec<(ReadPart, &'a MetricReq)>, part: ReadPart, conds: &'a [CondReq]) {
    for c in conds {
        if let CondReq::Metric(m) = c {
            out.push((part, m));
        }
    }
}

/// Every line with its part.
fn lines(rule: &CompiledRule) -> Vec<(ReadPart, &CompiledLine)> {
    let mut out: Vec<(ReadPart, &CompiledLine)> =
        rule.always.iter().enumerate().map(|(i, l)| (ReadPart::Always { line: i as u16 }, l)).collect();
    for (si, s) in rule.stages.iter().enumerate() {
        for (at_end, ls) in [(false, &s.on), (true, &s.at_end)] {
            for (i, l) in ls.iter().enumerate() {
                out.push((ReadPart::Stage { stage: si as u8, name: s.name, at_end, line: i as u16 }, l));
            }
        }
    }
    out
}

/// Judge one already-read `value` against its condition. The one body the point read
/// and the series read share.
fn judge(part: ReadPart, m: &MetricReq, value: f64) -> ConditionRead {
    ConditionRead {
        part,
        r: m.r,
        value,
        conds: m.conds.clone(),
        tolerance: m.tolerance,
        matched: first_satisfied_cond(&m.conds, value, m.tolerance),
        ok: eval(&m.conds, value, m.tolerance),
    }
}

/// Read every condition, signal and line of `rule` against the coin's fold at `now`.
/// `pos` is the held position (`None` before entry, where position metrics read `NaN`).
pub fn read_rule(rule: &CompiledRule, track: &TokenTrack, pos: Option<&PositionCtx>, now: Ts) -> RuleReadoutParts {
    let conditions = listed_reqs(rule)
        .into_iter()
        .map(|(part, m)| {
            let p = if part.is_entry() { None } else { pos };
            judge(part, m, m.read(track, p, now))
        })
        .collect();
    let signals = rule
        .signals
        .iter()
        .map(|s| SignalRead { name: s.name, holds: s.groups.iter().any(|g| g.iter().all(|m| m.holds(track, pos, now))) })
        .collect::<Vec<_>>();
    let lines = lines(rule)
        .into_iter()
        .map(|(part, l)| LineRead {
            part,
            holds: l.conds.iter().all(|c| match c {
                CondReq::Metric(m) => m.holds(track, pos, now),
                CondReq::Signal { index, negated } => {
                    let s = &rule.signals[*index];
                    s.groups.iter().any(|g| g.iter().all(|m| m.holds(track, pos, now))) != *negated
                }
            }),
            sells: l.sell.map(|s| s.reason),
            sell_bps: l.sell.and_then(|s| s.bps),
            goes_to: l.go,
        })
        .collect();
    RuleReadoutParts { conditions, signals, lines }
}

/// The three lists [`read_rule`] returns.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleReadoutParts {
    pub conditions: Vec<ConditionRead>,
    pub signals: Vec<SignalRead>,
    pub lines: Vec<LineRead>,
}

fn stage_of(rule: &CompiledRule, stage: Option<u8>) -> Option<(u8, &'static str)> {
    stage.and_then(|s| rule.stages.get(usize::from(s)).map(|st| (s, st.name)))
}

/// Read one tracked (coin, rule) arm straight off engine state at `now`, resolving the
/// manual-episode rule the way the fold does. `None` ⇒ nothing to show.
pub fn read_state(state: &EngineState, mint: &Mint, rule_id: RuleId, now: Ts) -> Option<RuleReadout> {
    let token = state.tokens.get(mint)?;
    let arm = token.arms.get(&rule_id)?;
    let rule = state.rule_for(rule_id, arm.position())?;
    let held = arm.held();
    let pos = held.map(|h| h.position_ctx());
    let parts = read_rule(rule, &token.track, pos.as_ref(), now);
    Some(RuleReadout {
        source: ReadoutSource::Engine,
        arm: Some(arm.tag()),
        stage: stage_of(rule, held.map(|h| h.stage)),
        at: now,
        conditions: parts.conditions,
        signals: parts.signals,
        lines: parts.lines,
    })
}

/// The tag context a replay needs to classify trades exactly as the live fold did: the
/// fingerprint, its compiled tags, and the coin's creator (the `creator` matcher and a
/// sticky tag's first member).
pub struct ReplayTags<'a> {
    pub fingerprint: FingerprintId,
    pub tags: &'a [CompiledTag],
    pub creator_wallet_hash: Option<u64>,
}

/// Everything a replay needs besides the rule, the trades and the instant.
pub struct ReplayCtx<'a> {
    /// Coin creation instant (pass the first trade's block time, as the replay driver
    /// and the lab's metric series do).
    pub created_at: Ts,
    /// Entry fill `(time, price)`; `None` for a position that never entered.
    pub entry: Option<(Ts, f64)>,
    /// The position's stage at the read instant and when it began. A stored position
    /// keeps its stage index but not the move time, so a caller without it passes the
    /// entry time (then `m_position.stage_sec` reads time since the fill).
    pub stage: Option<(u8, Ts)>,
    /// Absent ⇒ tagged metrics read `NaN`.
    pub tags: Option<ReplayTags<'a>>,
}

/// Register what `rule` reads on a fresh track, the way `EngineState::new_track` does.
fn register(track: &mut TokenTrack, rule: &CompiledRule, ctx: &ReplayCtx<'_>) {
    rule.buffers.ensure_on(track);
    if let Some(t) = &ctx.tags {
        for read in &rule.buffers.tags {
            let Some(tag) = t.tags.iter().find(|x| x.key == read.key) else { continue };
            if read.trade {
                track.ensure_tag(t.fingerprint, read.key, &tag.patterns, &read.windows);
            }
            if read.template {
                if let Some(tp) = tag.patterns.templates() {
                    track.ensure_template_tag(t.fingerprint, read.key, &tp);
                }
            }
        }
        if let Some(h) = t.creator_wallet_hash {
            track.seed_creator(h);
        }
    }
}

fn position_at_fill(ctx: &ReplayCtx<'_>) -> Option<PositionCtx> {
    ctx.entry.map(|(at, price)| {
        let mut p = PositionCtx::at_fill(price, at);
        if let Some((_, since)) = ctx.stage {
            p.stage_since = since;
        }
        p
    })
}

/// Reconstruct a rule's readout at `at` by folding stored trades through a fresh track
/// — the **closed-position** path, where the engine's own state is gone. Trades must
/// arrive in execution order; anything after `at` is ignored. The closing tick is
/// load-bearing: windows decay by eviction.
pub fn replay_readout(rule: &CompiledRule, trades: impl IntoIterator<Item = TradeLite>, ctx: &ReplayCtx<'_>, at: Ts) -> RuleReadout {
    let mut track = TokenTrack::new(ctx.created_at);
    register(&mut track, rule, ctx);
    let mut position = position_at_fill(ctx);
    for t in trades {
        if t.at > at {
            continue;
        }
        track.on_trade(t);
        // Peak/trough only over prices the position lived through (mirrors `reduce`'s
        // `fold_entered_extremes`); `room_taken_pct` reads the depth at the fill.
        if let (Some(p), Some((entered_at, _))) = (position.as_mut(), ctx.entry) {
            if t.at >= entered_at {
                p.fold_price(t.price);
            }
            if t.at <= entered_at {
                p.entry_priced_reserve = track.current_priced_reserves();
            }
        }
    }
    track.on_tick(at, None);
    let parts = read_rule(rule, &track, position.as_ref(), at);
    RuleReadout {
        source: ReadoutSource::Replay,
        arm: None,
        stage: stage_of(rule, ctx.stage.map(|(s, _)| s)),
        at,
        conditions: parts.conditions,
        signals: parts.signals,
        lines: parts.lines,
    }
}

/// One condition across a whole series: the condition itself plus one entry per row.
#[derive(Debug, Clone, PartialEq)]
pub struct ConditionSeries {
    pub part: ReadPart,
    pub req: MetricReq,
    /// The value at each row; `NaN` where unreadable.
    pub values: Vec<f64>,
    /// Whether the condition holds at each row.
    pub ok: Vec<bool>,
}

impl ConditionSeries {
    /// Row `i` as the point route's [`ConditionRead`].
    pub fn row(&self, i: usize) -> ConditionRead {
        judge(self.part, &self.req, self.values[i])
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// A rule's readout at every row of the coin's event grid.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadoutSeries {
    /// Row instants, ascending: trades plus the sparse grid's ticks.
    pub at: Vec<Ts>,
    /// One per metric condition, in [`read_rule`]'s order.
    pub conditions: Vec<ConditionSeries>,
    /// A row budget stopped the fold before the tail: coverage ends at `covered_until`.
    pub truncated: bool,
    pub covered_until: Ts,
    pub covered_from: Ts,
}

/// The series column a condition reads; `None` for a position metric, which folds from
/// the position per row.
fn req_column(m: &MetricReq) -> Option<SeriesColumn> {
    (!m.r.is_position()).then_some(SeriesColumn { r: m.r, fp: m.fingerprint })
}

/// [`replay_readout`] at every row of the coin's event stream, over the shared sparse
/// tick grid. **The row at an instant equals `replay_readout` at that instant**,
/// condition for condition. Only the columns this rule reads are folded.
///
/// `as_of` bounds the tail; `max_rows` bounds the recorded rows (and sets `truncated`);
/// `record_from` moves where recording starts without moving where the fold starts.
pub fn replay_series(
    rule: &CompiledRule,
    trades: impl IntoIterator<Item = TradeLite>,
    ctx: &ReplayCtx<'_>,
    as_of: Ts,
    max_rows: Option<usize>,
    record_from: Option<Ts>,
) -> ReadoutSeries {
    let reqs = listed_reqs(rule);
    let mut columns: Vec<SeriesColumn> = Vec::new();
    let col_of: Vec<Option<usize>> = reqs
        .iter()
        .map(|(_, m)| {
            req_column(m).map(|c| {
                columns.iter().position(|x| *x == c).unwrap_or_else(|| {
                    columns.push(c);
                    columns.len() - 1
                })
            })
        })
        .collect();
    let mut series = MetricSeries::new(ctx.created_at, columns);
    series.ensure_buffers(&rule.buffers);
    if let Some(t) = &ctx.tags {
        for read in &rule.buffers.tags {
            let Some(tag) = t.tags.iter().find(|x| x.key == read.key) else { continue };
            if read.trade {
                series.ensure_tag(t.fingerprint, read.key, &tag.patterns, &read.windows);
            }
            if read.template {
                if let Some(tp) = tag.patterns.templates() {
                    series.ensure_template_tag(t.fingerprint, read.key, &tp);
                }
            }
        }
        if let Some(h) = t.creator_wallet_hash {
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
        // `held` and `stage` climb from instants at or before the last trade, so they
        // ride on the `stall` horizon (measured from the last trade). Over-wide costs
        // ticks; too narrow drops the row a crossing lands on.
        stall_horizon_secs: h.stall_secs.max(h.held_secs).max(h.stage_secs),
    };
    let fold = fold_sparse(&mut series, ctx.created_at, trades.into_iter().map(|t| (t, None)), &grid, as_of, max_rows);

    let n = series.n_rows();
    let mut out: Vec<ConditionSeries> = reqs
        .iter()
        .map(|(part, m)| ConditionSeries { part: *part, req: (*m).clone(), values: Vec::with_capacity(n), ok: Vec::with_capacity(n) })
        .collect();
    let mut position = position_at_fill(ctx);
    for i in 0..n {
        let now = series.at[i];
        let price = series.price[i];
        let mut entered = false;
        if let (Some(p), Some((entered_at, _))) = (position.as_mut(), ctx.entry) {
            if now <= entered_at {
                p.entry_priced_reserve = series.priced_reserve_sol[i];
            }
            if now >= entered_at {
                p.fold_price(price);
                entered = true;
            }
        }
        let held = if entered { position.as_ref() } else { None };
        for (k, col) in out.iter_mut().enumerate() {
            let pos = if col.part.is_entry() { None } else { held };
            let value = match col_of[k] {
                Some(idx) => series.value_at(i, idx),
                None => pos.map_or(f64::NAN, |c| position_value(col.req.r.metric, c, price, now)),
            };
            let read = judge(col.part, &col.req, value);
            col.values.push(read.value);
            col.ok.push(read.ok);
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

#[cfg(test)]
#[path = "readout_tests.rs"]
mod tests;
