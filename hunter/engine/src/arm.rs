//! Arming state + the compiled form of a rule.
//!
//! [`ArmState`] is the per-(coin, rule) lifecycle the fold walks:
//! `PendingFirstSlot -> Armed -> EntryPending -> Entered -> ExitPending -> End/...` with
//! `Disarmed`/`Done` terminals. [`CompiledRule`] is a [`LoadedRule`] pre-chewed at
//! `RulesReloaded` into exactly what the hot path needs — flat condition lists, stage
//! indices instead of names, the buffers each condition reads, and the derived monotonic
//! kills that let a hopeless entry disarm itself — so nothing is parsed per event.
//!
//! **Held-side evaluation, one step per print or tick** ([`CompiledRule::held_step`]):
//! the `always` lines, then — at the stage's deadline — its `at_end` lines or the move to
//! `then`, else its `on` lines. The first line that holds acts. A move takes effect from
//! the next print or tick; a partial sell moves when its fill lands.
//!
//! **One condition walk, two sources.** Every decision reads the coin through
//! [`CoinReads`]: the live fold's [`TokenTrack`], or one row of the lab sweep's
//! precomputed series. The walk is generic, so a sweep scan decides exactly as the fold
//! does instead of re-implementing it.

use smallvec::SmallVec;

use crate::cap::Cap;
use crate::event::{DisarmReason, ExitReason, IntentId, LoadedRule, Portion, PositionId, RuleId, TradeMode};
use crate::fingerprint::FingerprintId;
use crate::metrics::buffers::Buffers;
use crate::metrics::evaluator::{eval, Condition, ConditionExpr, Operator};
use crate::metrics::position::{position_value, PositionCtx};
use crate::metrics::registry::Metric;
use crate::metrics::series::SeriesColumn;
use crate::metrics::state::StateMetrics;
use crate::metrics::track::TokenTrack;
use crate::metrics::{metric_spec, MetricRef, Ts, WindowUnit};
use crate::rule_params::{Cond, DeadlineBasis, EntryLock, Line, ReEntry, MAX_SELL_BPS};

/// One metric condition, ready to read: what to read, under which fingerprint, and the
/// DNF it is judged against.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricReq {
    pub r: MetricRef,
    /// The rule's fingerprint when the read needs one (a fingerprint tag); `None` else.
    pub fingerprint: Option<FingerprintId>,
    /// The metric's `=` band.
    pub tolerance: f64,
    /// DNF: OR of AND-arms.
    pub conds: ConditionExpr,
    /// Index of this read in [`CompiledRule::coin_reads`]; [`POSITION_READ`] for an
    /// `m_position` metric, which reads our position instead of the coin.
    pub read_id: u16,
}

/// [`MetricReq::read_id`] of a position metric.
pub const POSITION_READ: u16 = u16::MAX;

/// Where a rule reads the coin from: the live fold ([`TokenTrack`]) or one row of a
/// precomputed series (the lab sweep). Position metrics never come from here — they
/// read our [`PositionCtx`] against [`price`](Self::price).
pub trait CoinReads {
    /// The coin-side value `req` reads at `now`.
    fn coin_value(&self, req: &MetricReq, now: Ts) -> f64;
    /// The coin's current price.
    fn price(&self) -> f64;
    /// Seconds since the coin was created (`m_state.age_sec`, the `age_sec` deadline).
    fn age_sec(&self, now: Ts) -> f64;
    /// The slot of the coin's latest print (the `slot` entry lock).
    fn cur_slot(&self) -> u64;
}

impl CoinReads for TokenTrack {
    #[inline]
    fn coin_value(&self, req: &MetricReq, now: Ts) -> f64 {
        self.value(req.r, req.fingerprint, now)
    }

    #[inline]
    fn price(&self) -> f64 {
        self.current_price()
    }

    #[inline]
    fn age_sec(&self, now: Ts) -> f64 {
        StateMetrics::time(self.created_at(), now)
    }

    #[inline]
    fn cur_slot(&self) -> u64 {
        TokenTrack::cur_slot(self)
    }
}

impl MetricReq {
    /// The reading: our position for an `m_position` metric (`NaN` without one), the
    /// coin for everything else.
    #[inline]
    pub fn read<R: CoinReads + ?Sized>(&self, reads: &R, pos: Option<&PositionCtx>, now: Ts) -> f64 {
        if self.read_id == POSITION_READ {
            pos.map_or(f64::NAN, |p| position_value(self.r.metric, p, reads.price(), now))
        } else {
            reads.coin_value(self, now)
        }
    }

    #[inline]
    pub fn holds<R: CoinReads + ?Sized>(&self, reads: &R, pos: Option<&PositionCtx>, now: Ts) -> bool {
        eval(&self.conds, self.read(reads, pos, now), self.tolerance)
    }
}

/// One compiled condition.
#[derive(Debug, Clone, PartialEq)]
pub enum CondReq {
    Metric(MetricReq),
    /// Signal by index into [`CompiledRule::signals`].
    Signal { index: usize, negated: bool },
}

/// A compiled signal: OR of AND-groups of metric conditions.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledSignal {
    pub name: &'static str,
    pub groups: Vec<Vec<MetricReq>>,
}

impl CompiledSignal {
    pub fn holds<R: CoinReads + ?Sized>(&self, reads: &R, pos: Option<&PositionCtx>, now: Ts) -> bool {
        self.groups.iter().any(|g| g.iter().all(|r| r.holds(reads, pos, now)))
    }
}

/// What a sell line sells.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledSell {
    pub reason: ExitReason,
    /// Basis points of the FIRST buy's bag; `None` = everything left.
    pub bps: Option<u16>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledLine {
    /// This line's place among the rule's held lines: `always` (the TP/SL shortcuts
    /// first), then each stage's `on` and `at_end` lines. Stable per compiled rule, so
    /// an exit can be counted by the line that made it.
    pub index: u16,
    /// AND.
    pub conds: Vec<CondReq>,
    pub sell: Option<CompiledSell>,
    /// Stage index to move to.
    pub go: Option<u8>,
}

impl CompiledLine {
    /// The line goes to `stage` and does not sell the whole bag, so in `stage` it has
    /// nothing to do: moving to where the position already is would only restart the
    /// stage clock and hide every line below it, and a partial sell would sell again on
    /// every print. Such a line does not act while the position is in `stage`.
    pub fn idle_in(&self, stage: u8) -> bool {
        self.go == Some(stage) && !matches!(self.sell, Some(CompiledSell { bps: None, .. }))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledStage {
    pub name: &'static str,
    pub ends: Option<(DeadlineBasis, f64)>,
    pub on: Vec<CompiledLine>,
    pub at_end: Vec<CompiledLine>,
    /// Where the deadline leads when no `at_end` line acts.
    pub then: Option<u8>,
}

/// What one held-side step picked: nothing, a line, or the deadline's move.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HeldStep<'a> {
    None,
    Line(&'a CompiledLine),
    /// The stage deadline passed with no `at_end` line holding: move to `then`.
    Move(u8),
}

/// One held-side decision.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HeldAction {
    None,
    /// Sell (all, or `bps` of the first bag); a partial sell then moves to `then_stage`
    /// when its fill lands.
    Sell { reason: ExitReason, bps: Option<u16>, then_stage: Option<u8> },
    /// Move to another stage, from the next print or tick.
    Move { stage: u8 },
}

/// A derived monotonic upper bound from an **entry** condition arm: a monotonic metric
/// never decreases, so once its value crosses this the arm can never hold again.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonoBound {
    pub r: MetricRef,
    pub threshold: f64,
    /// `true` ⇒ crossed at `value >= threshold` (from `<`); `false` ⇒ at `value >
    /// threshold` (from `<=` / `=`'s upper edge).
    pub cross_at_ge: bool,
}

impl MonoBound {
    /// True once `value` has crossed this bound. Non-finite ⇒ not crossed. Public so the
    /// sweep scan can replicate the fold's derived disarm.
    pub fn crossed(self, value: f64) -> bool {
        if !value.is_finite() {
            return false;
        }
        if self.cross_at_ge {
            value >= self.threshold
        } else {
            value > self.threshold
        }
    }
}

/// Per entry condition derived kill (OR of arms): permanently false only when EVERY arm
/// is dead; an arm with no upper bound (`None`) never dies from a rising metric.
#[derive(Debug, Clone, PartialEq)]
pub struct MonoMetricKill {
    pub req: MetricReq,
    pub arms: SmallVec<[Option<MonoBound>; 2]>,
}

impl MonoMetricKill {
    pub fn permanently_false(&self, value: f64) -> bool {
        !self.arms.is_empty() && self.arms.iter().all(|arm| arm.is_some_and(|b| b.crossed(value)))
    }

    /// The bound that crossed LAST — the one whose crossing ended the episode. `None`
    /// while any arm can still hold.
    pub fn binding_bound(&self, value: f64) -> Option<MonoBound> {
        if !self.permanently_false(value) {
            return None;
        }
        self.arms.iter().flatten().copied().reduce(|a, b| {
            let later = b.threshold > a.threshold || (b.threshold == a.threshold && a.cross_at_ge && !b.cross_at_ge);
            if later {
                b
            } else {
                a
            }
        })
    }
}

/// One entry condition that was **not** met at the disarm instant, with its reading.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockedReq {
    pub r: MetricRef,
    /// `NaN` when unreadable — itself a blocker.
    pub value: f64,
    pub conds: ConditionExpr,
}

/// Why an entry became permanently unsatisfiable: the deadline that crossed
/// ([`killed_by`](Self::killed_by)) and the entry conditions still unmet as it did
/// ([`unmet`](Self::unmet); empty = everything else held, the coin qualified too late).
#[derive(Debug, Clone, PartialEq)]
pub struct EntryBlockers {
    pub killed_by: MonoBound,
    pub unmet: Vec<BlockedReq>,
}

/// Tightest mono upper bound in one AND arm, if any.
fn arm_mono_upper(arm: &[Condition], half_tol: f64, r: MetricRef) -> Option<MonoBound> {
    let mut best: Option<(f64, bool)> = None;
    for c in arm {
        let bound = match c.operator {
            Operator::Lt => Some((c.value, true)),
            Operator::Lte => Some((c.value, false)),
            Operator::Eq => Some((c.value + half_tol, false)),
            Operator::Gt | Operator::Gte | Operator::Ne => None,
        };
        if let Some((th, ge)) = bound {
            best = Some(match best {
                None => (th, ge),
                Some((bth, bge)) if th < bth || (th == bth && ge && !bge) => (th, ge),
                Some(b) => b,
            });
        }
    }
    best.map(|(threshold, cross_at_ge)| MonoBound { r, threshold, cross_at_ge })
}

/// How long a rule's reads can keep changing **without a trade** — the per-rule half of
/// `reduce`'s settled-token tick skip. Every clock a condition or a deadline reads has a
/// last instant past which it can no longer change a decision; `0.0` = "this rule never
/// reads that clock". Over-wide is safe; too narrow silently drops a decision, so every
/// combinator is a `max`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ClockHorizons {
    /// Widest trailing window read (slot windows at the nominal slot time).
    pub max_window_secs: f64,
    /// Largest `m_state.age_sec` threshold or age deadline, from creation.
    pub time_secs: f64,
    /// Largest `m_price.stall_sec` threshold, from the last all-time high.
    pub stall_secs: f64,
    /// Largest `m_position.held_sec` threshold or held deadline, from the entry fill.
    pub held_secs: f64,
    /// Largest `m_position.stage_sec` threshold or stage deadline, from the stage start.
    pub stage_secs: f64,
}

impl ClockHorizons {
    pub fn widen(self, other: Self) -> Self {
        Self {
            max_window_secs: self.max_window_secs.max(other.max_window_secs),
            time_secs: self.time_secs.max(other.time_secs),
            stall_secs: self.stall_secs.max(other.stall_secs),
            held_secs: self.held_secs.max(other.held_secs),
            stage_secs: self.stage_secs.max(other.stage_secs),
        }
    }

    fn absorb_req(&mut self, r: &MetricReq) {
        use crate::metrics::grid::SparseGrid;
        for w in [r.r.span.window, r.r.span.slice].into_iter().flatten() {
            // The tick grid is a wall clock, so a slot span converts at the nominal slot
            // time — horizon sizing only, never a reading. A PRINT span moves only on a
            // trade, which evaluates anyway, so it adds nothing.
            let secs = match w.unit {
                WindowUnit::Sec => w.size + w.lag,
                WindowUnit::Slot => (w.size + w.lag) * crate::metrics::NOMINAL_SLOT_SECS,
                WindowUnit::Print => 0.0,
            };
            self.max_window_secs = self.max_window_secs.max(SparseGrid::clamp_secs(secs));
        }
        let slot = match r.r.metric {
            Metric::AgeSec => &mut self.time_secs,
            Metric::StallSec => &mut self.stall_secs,
            Metric::HeldSec => &mut self.held_secs,
            Metric::StageSec => &mut self.stage_secs,
            _ => return,
        };
        for c in r.conds.iter().flatten() {
            *slot = slot.max(SparseGrid::clamp_secs(c.value.abs() + r.tolerance));
        }
    }

    fn absorb_deadline(&mut self, basis: DeadlineBasis, secs: f64) {
        use crate::metrics::grid::SparseGrid;
        let slot = match basis {
            DeadlineBasis::Age => &mut self.time_secs,
            DeadlineBasis::Held => &mut self.held_secs,
            DeadlineBasis::Stage => &mut self.stage_secs,
        };
        // A deadline is reached at `>= secs`; one tick past it is enough to observe.
        *slot = slot.max(SparseGrid::clamp_secs(secs + 0.5));
    }
}

/// A [`LoadedRule`] pre-chewed for the hot path.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledRule {
    pub id: RuleId,
    pub fingerprint_id: FingerprintId,
    pub trade_mode: TradeMode,
    pub buy_amount_lamports: u64,
    /// Percent-of-pool sizing, resolved per entry (the pool is only known then).
    pub size_pct_of_pool: Option<f64>,
    /// Both caps arrive already decoded ([`Cap`]).
    pub concurrent_cap: Cap,
    pub max_total: Cap,
    /// The TP/SL shortcuts as authored — the sweep's parallel input; the fold reads the
    /// `always` lines they compile to.
    pub take_profit: Option<f64>,
    pub stop_loss: Option<f64>,
    /// `enter.event` (AND). Empty ⇒ no separate event.
    pub event: Vec<CondReq>,
    /// `enter.filters` (AND): a failure keeps watching.
    pub filters: Vec<CondReq>,
    /// `enter.final_filters` (AND): a failure on a print that would buy ends the coin.
    pub final_filters: Vec<CondReq>,
    pub entry_lock: Option<EntryLock>,
    pub signals: Vec<CompiledSignal>,
    /// Stop loss, take profit, then the authored `always` lines.
    pub always: Vec<CompiledLine>,
    /// At least one stage (an implicit empty one when none is authored).
    pub stages: Vec<CompiledStage>,
    /// Every buffer the conditions read.
    pub buffers: Buffers,
    /// Every distinct coin-side read, indexed by [`MetricReq::read_id`] — the columns a
    /// precomputed series must carry for this rule.
    pub coin_reads: Vec<SeriesColumn>,
    pub mono_kills: SmallVec<[MonoMetricKill; 2]>,
    pub clock_horizons: ClockHorizons,
    pub reentry: Option<ReEntry>,
    pub exclusive: bool,
    pub priority: i32,
    /// `false` ⇒ skip arming and new entries; exits on held positions still run.
    pub entry_enabled: bool,
}

/// How a line that sells with no label names itself: its first live metric condition,
/// `m_flow.buy_sol @!volume [10s] >= 2`.
fn auto_label(line: &Line) -> &'static str {
    let text = line
        .when
        .iter()
        .find_map(|c| match c {
            Cond::Metric { r, is, off: false } => {
                let first = is.first().and_then(|a| a.first());
                Some(match first {
                    Some(c) => format!("{} {} {}", r.label(), c.operator.symbol(), crate::event::format_metric_threshold(c.value)),
                    None => r.label(),
                })
            }
            Cond::Signal { name, negated, off: false } => Some(if *negated { format!("not {name}") } else { (*name).to_string() }),
            _ => None,
        })
        .unwrap_or_else(|| "rule".to_string());
    crate::intern::intern(&text)
}

struct Compiler {
    fp: FingerprintId,
    signal_index: Vec<&'static str>,
    stage_index: Vec<&'static str>,
    coin_reads: Vec<SeriesColumn>,
    next_line: u16,
}

impl Compiler {
    fn req(&mut self, r: MetricRef, conds: ConditionExpr) -> MetricReq {
        let fingerprint = r.is_fingerprint_scoped().then_some(self.fp);
        let read_id = if r.is_position() {
            POSITION_READ
        } else {
            let col = SeriesColumn { r, fp: fingerprint };
            let at = self.coin_reads.iter().position(|c| *c == col).unwrap_or_else(|| {
                self.coin_reads.push(col);
                self.coin_reads.len() - 1
            });
            u16::try_from(at).expect("a rule reads fewer than 65535 distinct metrics")
        };
        MetricReq { r, fingerprint, tolerance: metric_spec(r.metric).eq_tolerance, conds, read_id }
    }

    fn conds(&mut self, conds: &[Cond]) -> Vec<CondReq> {
        conds
            .iter()
            .filter(|c| !c.is_off())
            .map(|c| match c {
                Cond::Metric { r, is, .. } => CondReq::Metric(self.req(*r, is.clone())),
                Cond::Signal { name, negated, .. } => CondReq::Signal {
                    index: self.signal_index.iter().position(|s| s == name).expect("validated signal"),
                    negated: *negated,
                },
            })
            .collect()
    }

    fn stage(&self, name: &str) -> u8 {
        self.stage_index.iter().position(|s| *s == name).expect("validated stage") as u8
    }

    fn line_index(&mut self) -> u16 {
        let i = self.next_line;
        self.next_line += 1;
        i
    }

    fn lines(&mut self, lines: &[Line]) -> Vec<CompiledLine> {
        lines
            .iter()
            .filter(|l| !l.off)
            .map(|l| CompiledLine {
                index: self.line_index(),
                conds: self.conds(&l.when),
                sell: l.sell.map(|s| CompiledSell {
                    reason: ExitReason::Line(s.label.unwrap_or_else(|| auto_label(l))),
                    bps: s.pct.map(|p| (p * 100.0).round().clamp(1.0, f64::from(MAX_SELL_BPS)) as u16),
                }),
                go: l.go.map(|g| self.stage(g)),
            })
            .collect()
    }

    /// A position-scoped `pnl_pct <op> value` line — the TP/SL shortcuts.
    fn pnl_line(&mut self, op: Operator, value: f64, reason: ExitReason) -> CompiledLine {
        CompiledLine {
            index: self.line_index(),
            conds: vec![CondReq::Metric(
                self.req(MetricRef::life(Metric::PnlPct), vec![vec![Condition { operator: op, value }]]),
            )],
            sell: Some(CompiledSell { reason, bps: None }),
            go: None,
        }
    }
}

impl CompiledRule {
    /// Pre-chew a loaded rule. Its params are already parsed and validated, so this is a
    /// pure structural walk.
    pub fn compile(rule: &LoadedRule) -> Self {
        let p = &rule.params;
        let mut cx = Compiler {
            fp: rule.fingerprint_id,
            signal_index: p.signals.keys().copied().collect(),
            stage_index: p.stages.iter().map(|s| s.name).collect(),
            coin_reads: Vec::new(),
            next_line: 0,
        };
        // Entry first, so an entry condition's read id is the same whatever the held side
        // says (the sweep's shared entry walk keys on it).
        let event = cx.conds(&p.enter.event);
        let filters = cx.conds(&p.enter.filters);
        let final_filters = cx.conds(&p.enter.final_filters);
        let signals: Vec<CompiledSignal> = p
            .signals
            .iter()
            .map(|(name, groups)| CompiledSignal {
                name,
                groups: groups
                    .iter()
                    .map(|g| {
                        cx.conds(g)
                            .into_iter()
                            .filter_map(|c| match c {
                                CondReq::Metric(m) => Some(m),
                                CondReq::Signal { .. } => None, // validated away
                            })
                            .collect()
                    })
                    .filter(|g: &Vec<MetricReq>| !g.is_empty())
                    .collect(),
            })
            .collect();
        let mut always = Vec::new();
        if let Some(sl) = p.stop_loss {
            always.push(cx.pnl_line(Operator::Lte, -sl, ExitReason::StopLoss));
        }
        if let Some(tp) = p.take_profit {
            always.push(cx.pnl_line(Operator::Gte, tp, ExitReason::TakeProfit));
        }
        always.extend(cx.lines(&p.always));
        let mut stages: Vec<CompiledStage> = Vec::with_capacity(p.stages.len());
        for (i, s) in p.stages.iter().enumerate() {
            let on = cx.lines(&s.on);
            let at_end = cx.lines(&s.at_end);
            stages.push(CompiledStage {
                name: s.name,
                ends: s.ends.map(|d| (d.basis, d.secs)),
                on,
                at_end,
                then: match (s.ends, s.then) {
                    (None, _) => None,
                    (Some(_), Some(t)) => Some(cx.stage(t)),
                    (Some(_), None) => Some((i + 1) as u8),
                },
            });
        }
        if stages.is_empty() {
            stages.push(CompiledStage { name: "main", ends: None, on: Vec::new(), at_end: Vec::new(), then: None });
        }
        // Every metric condition anywhere: buffers + clock horizons.
        let mut buffers = Buffers::default();
        let mut clock_horizons = ClockHorizons::default();
        let line_conds = always
            .iter()
            .chain(stages.iter().flat_map(|s| s.on.iter().chain(s.at_end.iter())))
            .flat_map(|l| l.conds.iter());
        let all_metric = event
            .iter()
            .chain(filters.iter())
            .chain(final_filters.iter())
            .chain(line_conds)
            .filter_map(|c| match c {
                CondReq::Metric(m) => Some(m),
                CondReq::Signal { .. } => None,
            })
            .chain(signals.iter().flat_map(|s| s.groups.iter().flatten()));
        for m in all_metric {
            buffers.absorb(m.r, anchor_cap(m));
            clock_horizons.absorb_req(m);
        }
        for s in &stages {
            if let Some((basis, secs)) = s.ends {
                clock_horizons.absorb_deadline(basis, secs);
            }
        }

        // Monotonic entry kills: per metric condition, OR of arm upper bounds.
        let mut mono_kills: SmallVec<[MonoMetricKill; 2]> = SmallVec::new();
        for c in event.iter().chain(filters.iter()).chain(final_filters.iter()) {
            let CondReq::Metric(m) = c else { continue };
            if !m.r.is_monotonic() {
                continue;
            }
            let half = m.tolerance / 2.0;
            let arms: SmallVec<[Option<MonoBound>; 2]> = m.conds.iter().map(|arm| arm_mono_upper(arm, half, m.r)).collect();
            if arms.iter().any(Option::is_some) {
                mono_kills.push(MonoMetricKill { req: m.clone(), arms });
            }
        }

        Self {
            id: rule.id,
            fingerprint_id: rule.fingerprint_id,
            trade_mode: rule.trade_mode,
            buy_amount_lamports: rule.buy_amount_lamports,
            size_pct_of_pool: p.enter.size_pct_of_pool,
            concurrent_cap: rule.concurrent_cap(),
            max_total: rule.total_cap(),
            take_profit: p.take_profit,
            stop_loss: p.stop_loss,
            event,
            filters,
            final_filters,
            entry_lock: p.enter.lock,
            signals,
            always,
            stages,
            buffers,
            coin_reads: cx.coin_reads,
            mono_kills,
            clock_horizons,
            reentry: p.reentry,
            exclusive: p.exclusive,
            priority: p.priority,
            entry_enabled: rule.entry_enabled,
        }
    }

    // ── Condition reads ──────────────────────────────────────────────────────

    fn cond_holds<R: CoinReads + ?Sized>(&self, c: &CondReq, reads: &R, pos: Option<&PositionCtx>, now: Ts) -> bool {
        match c {
            CondReq::Metric(m) => m.holds(reads, pos, now),
            CondReq::Signal { index, negated } => self.signals[*index].holds(reads, pos, now) != *negated,
        }
    }

    fn all_hold<R: CoinReads + ?Sized>(&self, conds: &[CondReq], reads: &R, pos: Option<&PositionCtx>, now: Ts) -> bool {
        conds.iter().all(|c| self.cond_holds(c, reads, pos, now))
    }

    fn line_holds<R: CoinReads + ?Sized>(&self, l: &CompiledLine, reads: &R, pos: Option<&PositionCtx>, now: Ts) -> bool {
        self.all_hold(&l.conds, reads, pos, now)
    }

    // ── Entry side ───────────────────────────────────────────────────────────

    /// Arming alone is the entry signal (no live entry condition).
    pub fn enter_on_arm(&self) -> bool {
        self.event.is_empty() && self.filters.is_empty() && self.final_filters.is_empty()
    }

    pub fn event_satisfied<R: CoinReads + ?Sized>(&self, reads: &R, now: Ts) -> bool {
        self.all_hold(&self.event, reads, None, now)
    }

    pub fn filters_satisfied<R: CoinReads + ?Sized>(&self, reads: &R, now: Ts) -> bool {
        self.all_hold(&self.filters, reads, None, now)
    }

    pub fn final_filters_satisfied<R: CoinReads + ?Sized>(&self, reads: &R, now: Ts) -> bool {
        self.all_hold(&self.final_filters, reads, None, now)
    }

    /// **The pre-entry veto**: a sell line that could act right after a buy — an
    /// `always` sell line or a sell line of the first stage — already holds on the coin
    /// (position metrics read `NaN` here, so a line on our position never vetoes).
    /// Buying then would be a round trip for nothing. Returns the line's reason.
    pub fn sell_line_holding_before_entry<R: CoinReads + ?Sized>(&self, reads: &R, now: Ts) -> Option<ExitReason> {
        self.always
            .iter()
            .chain(self.stages[0].on.iter())
            .filter(|l| !l.idle_in(0))
            .filter_map(|l| l.sell.map(|s| (l, s)))
            .find(|(l, _)| self.line_holds(l, reads, None, now))
            .map(|(_, s)| s.reason)
    }

    /// Entry holds (event, final filters, filters) and no sell line already holds. The
    /// buy-retry gate: a lock is already spent on the print that submitted.
    pub fn can_enter<R: CoinReads + ?Sized>(&self, reads: &R, now: Ts) -> bool {
        self.event_satisfied(reads, now)
            && self.final_filters_satisfied(reads, now)
            && self.filters_satisfied(reads, now)
            && self.sell_line_holding_before_entry(reads, now).is_none()
    }

    /// The Armed-side entry decision, with the lock.
    ///
    /// * No lock: the event must hold; then Enter, or Exhaust when only a final filter
    ///   failed.
    /// * `token`: only a print decides (a tick never does); the first print that makes
    ///   the event true enters, or ends the coin.
    /// * `slot`: the first print of a slot that makes the event true is its only chance;
    ///   a filter failure spends the slot, a final-filter failure ends the coin.
    pub fn try_enter<R: CoinReads + ?Sized>(&self, reads: &R, now: Ts, locked_slot: Option<u64>, on_print: bool) -> EntryVerdict {
        let final_ok = self.final_filters_satisfied(reads, now);
        let rest_ok = self.filters_satisfied(reads, now);
        let veto = self.sell_line_holding_before_entry(reads, now).is_some();
        let would_enter = final_ok && rest_ok && !veto;
        // A final-filter failure on a print that would otherwise buy ends the coin; a
        // filter or veto failure only spends the chance.
        let final_blocks = !final_ok && rest_ok && !veto;
        match self.entry_lock {
            None => {
                if !self.event_satisfied(reads, now) {
                    EntryVerdict::No
                } else if would_enter {
                    EntryVerdict::Enter
                } else if final_blocks {
                    EntryVerdict::Exhaust
                } else {
                    EntryVerdict::No
                }
            }
            Some(EntryLock::Token) => {
                if !on_print || !self.event_satisfied(reads, now) {
                    EntryVerdict::No
                } else if would_enter {
                    EntryVerdict::Enter
                } else {
                    EntryVerdict::Exhaust
                }
            }
            Some(EntryLock::Slot) => {
                let slot = reads.cur_slot();
                if locked_slot == Some(slot) && slot != 0 {
                    return EntryVerdict::No;
                }
                if !self.event_satisfied(reads, now) {
                    EntryVerdict::No
                } else if would_enter {
                    EntryVerdict::Enter
                } else if final_blocks {
                    EntryVerdict::Exhaust
                } else {
                    EntryVerdict::SpendSlot
                }
            }
        }
    }

    /// The monotonic entry bound permanently crossed at `now`, if any: the entry can
    /// never hold again, so the arm disarms.
    pub fn entry_unsatisfiable<R: CoinReads + ?Sized>(&self, reads: &R, now: Ts) -> Option<MonoBound> {
        self.mono_kills.iter().find_map(|k| k.binding_bound(k.req.read(reads, None, now)))
    }

    /// Every entry metric condition still unmet at `now`, beside the deadline that
    /// killed the arm. Cold path: once per episode, on the disarm.
    pub fn entry_blockers<R: CoinReads + ?Sized>(&self, reads: &R, now: Ts, killed_by: MonoBound) -> EntryBlockers {
        let unmet = self
            .event
            .iter()
            .chain(self.final_filters.iter())
            .chain(self.filters.iter())
            .filter_map(|c| match c {
                CondReq::Metric(m) => Some(m),
                CondReq::Signal { .. } => None,
            })
            .filter(|m| m.r != killed_by.r)
            .filter_map(|m| {
                let value = m.read(reads, None, now);
                (!eval(&m.conds, value, m.tolerance)).then(|| BlockedReq { r: m.r, value, conds: m.conds.clone() })
            })
            .collect();
        EntryBlockers { killed_by, unmet }
    }

    // ── Held side ────────────────────────────────────────────────────────────

    /// Whether the held position's stage deadline is reached at `now`.
    fn deadline_reached<R: CoinReads + ?Sized>(&self, stage: &CompiledStage, reads: &R, pos: &PositionCtx, now: Ts) -> bool {
        let Some((basis, secs)) = stage.ends else { return false };
        let elapsed = match basis {
            DeadlineBasis::Age => reads.age_sec(now),
            DeadlineBasis::Held => pos.held(now),
            DeadlineBasis::Stage => pos.stage_sec(now),
        };
        elapsed >= secs
    }

    fn act(l: &CompiledLine) -> HeldAction {
        match (l.sell, l.go) {
            (Some(s), go) => HeldAction::Sell { reason: s.reason, bps: s.bps, then_stage: go },
            (None, Some(stage)) => HeldAction::Move { stage },
            (None, None) => HeldAction::None,
        }
    }

    /// **One held-side step**: the `always` lines; else at the deadline the `at_end`
    /// lines or the move to `then`; else the stage's `on` lines. The first line that
    /// holds acts; a line idle in the current stage ([`CompiledLine::idle_in`]) never
    /// does.
    pub fn held_step<R: CoinReads + ?Sized>(&self, reads: &R, held: &EnteredCtx, now: Ts) -> HeldAction {
        match self.held_line(reads, &held.position_ctx(), held.stage, now) {
            HeldStep::None => HeldAction::None,
            HeldStep::Line(l) => Self::act(l),
            HeldStep::Move(stage) => HeldAction::Move { stage },
        }
    }

    /// [`held_step`](Self::held_step) naming the line that acted — for a caller that
    /// counts exits by line (the lab sweep).
    pub fn held_line<R: CoinReads + ?Sized>(&self, reads: &R, pos: &PositionCtx, stage: u8, now: Ts) -> HeldStep<'_> {
        let acts = |l: &&CompiledLine| !l.idle_in(stage) && self.line_holds(l, reads, Some(pos), now);
        if let Some(l) = self.always.iter().find(acts) {
            return HeldStep::Line(l);
        }
        let Some(current) = self.stages.get(usize::from(stage)) else { return HeldStep::None };
        if self.deadline_reached(current, reads, pos, now) {
            if let Some(l) = current.at_end.iter().find(acts) {
                return HeldStep::Line(l);
            }
            return current.then.map_or(HeldStep::None, HeldStep::Move);
        }
        current.on.iter().find(acts).map_or(HeldStep::None, HeldStep::Line)
    }

    /// Every held line in [`CompiledLine::index`] order.
    pub fn held_lines(&self) -> impl Iterator<Item = &CompiledLine> {
        self.always.iter().chain(self.stages.iter().flat_map(|s| s.on.iter().chain(s.at_end.iter())))
    }

    /// `act` for a caller holding a [`HeldStep::Line`].
    pub fn line_action(l: &CompiledLine) -> HeldAction {
        Self::act(l)
    }

    /// Every metric condition, in authoring order: entry (event, final filters,
    /// filters), signals, then lines. What a readout lists.
    pub fn metric_reqs(&self) -> impl Iterator<Item = &MetricReq> {
        let lines = self
            .always
            .iter()
            .chain(self.stages.iter().flat_map(|s| s.on.iter().chain(s.at_end.iter())))
            .flat_map(|l| l.conds.iter());
        self.event
            .iter()
            .chain(self.final_filters.iter())
            .chain(self.filters.iter())
            .chain(lines)
            .filter_map(|c| match c {
                CondReq::Metric(m) => Some(m),
                CondReq::Signal { .. } => None,
            })
            .chain(self.signals.iter().flat_map(|s| s.groups.iter().flatten()))
    }
}

/// How many wallets a since-age buyer set must hold for one condition to stay exact: one
/// above the largest value it names on `buyer_count`, and at least one.
fn anchor_cap(r: &MetricReq) -> u32 {
    let mut cap = 1.0_f64;
    if r.r.metric == Metric::BuyerCount {
        for c in r.conds.iter().flatten() {
            if c.value.is_finite() {
                cap = cap.max(c.value.ceil() + 1.0);
            }
        }
    }
    cap.clamp(1.0, f64::from(u32::MAX)) as u32
}

/// Armed-side entry result. [`SpendSlot`](Self::SpendSlot): an event print whose filters
/// failed — that slot does not retry. [`Exhaust`](Self::Exhaust): the coin is done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryVerdict {
    No,
    Enter,
    SpendSlot,
    Exhaust,
}

/// The held-position snapshot shared by [`ArmState::Entered`] and a partial
/// [`ArmState::ExitPending`] (restored on fill, so peak / trough / entry are not
/// reseeded).
#[derive(Debug, Clone, PartialEq)]
pub struct EnteredCtx {
    pub position: PositionId,
    pub entry_price: f64,
    pub entered_at: Ts,
    pub peak_price: f64,
    pub trough_price: f64,
    /// The current stage's index.
    pub stage: u8,
    /// When the current stage began (`m_position.stage_sec`).
    pub stage_since: Ts,
    /// Basis points of the first bag already sold by partial sells.
    pub sold_bps: u16,
    /// See [`PositionCtx::entry_priced_reserve`].
    pub entry_priced_reserve: f64,
}

impl EnteredCtx {
    /// A fresh fill: peak and trough at the fill price, the first stage starting now.
    pub fn at_fill(position: PositionId, fill_price: f64, at: Ts, entry_priced_reserve: f64) -> Self {
        Self {
            position,
            entry_price: fill_price,
            entered_at: at,
            peak_price: fill_price,
            trough_price: fill_price,
            stage: 0,
            stage_since: at,
            sold_bps: 0,
            entry_priced_reserve,
        }
    }

    /// Move to `stage` from `at`.
    pub fn move_to(&mut self, stage: u8, at: Ts) {
        self.stage = stage;
        self.stage_since = at;
    }

    pub fn position_ctx(&self) -> PositionCtx {
        PositionCtx {
            entry_price: self.entry_price,
            peak_price: self.peak_price,
            trough_price: self.trough_price,
            entered_at: self.entered_at,
            stage_since: self.stage_since,
            entry_priced_reserve: self.entry_priced_reserve,
        }
    }
}

/// Per-(coin, rule) arming lifecycle. `attempts` counts submit tries so a bounded retry
/// policy can give up.
#[derive(Debug, Clone, PartialEq)]
pub enum ArmState {
    /// Matched a fingerprint's instant axes; a first-slot axis is not settled yet.
    PendingFirstSlot,
    /// Armed and evaluating entry on every trade / tick.
    Armed,
    /// A buy is in flight; the position row exists. `lamports` is the submitted size,
    /// frozen so retries resize identically (`0` ⇒ the rule's configured amount).
    EntryPending { intent: IntentId, position: PositionId, attempts: u32, lamports: u64 },
    /// Entry filled; the position is held and walking its stages.
    Entered(EnteredCtx),
    /// A sell is in flight, closing (a portion of) the bag for `reason`. `held` is the
    /// Entered snapshot — restored on a partial fill (moving to `then_stage`), discarded
    /// on a full close.
    ExitPending { intent: IntentId, reason: ExitReason, attempts: u32, portion: Portion, held: EnteredCtx, then_stage: Option<u8> },
    /// A re-entry rule closed a position and waits out its cooldown. Non-terminal.
    Cooldown { until: Ts },
    /// Terminal: the position closed, or the coin is done for this rule.
    Done,
    /// Terminal: disarmed before entry.
    Disarmed(DisarmReason),
}

impl ArmState {
    /// Whether this arm still needs the coin tracked. [`Cooldown`](Self::Cooldown)
    /// counts as active.
    pub fn is_active(&self) -> bool {
        !matches!(self, ArmState::Done | ArmState::Disarmed(_))
    }

    /// The position this arm owns, if any.
    pub fn position(&self) -> Option<PositionId> {
        match self {
            ArmState::EntryPending { position, .. } => Some(*position),
            ArmState::Entered(ctx) | ArmState::ExitPending { held: ctx, .. } => Some(ctx.position),
            _ => None,
        }
    }

    /// The held-position snapshot when a bag is open.
    pub fn held(&self) -> Option<&EnteredCtx> {
        match self {
            ArmState::Entered(ctx) | ArmState::ExitPending { held: ctx, .. } => Some(ctx),
            _ => None,
        }
    }

    /// Short stable tag for a UI / log line.
    pub fn tag(&self) -> &'static str {
        match self {
            ArmState::PendingFirstSlot => "PendingFirstSlot",
            ArmState::Armed => "Armed",
            ArmState::EntryPending { .. } => "EntryPending",
            ArmState::Entered(_) => "Entered",
            ArmState::ExitPending { .. } => "ExitPending",
            ArmState::Cooldown { .. } => "Cooldown",
            ArmState::Disarmed(_) => "Disarmed",
            ArmState::Done => "Done",
        }
    }
}

#[cfg(test)]
#[path = "arm_tests.rs"]
mod tests;
