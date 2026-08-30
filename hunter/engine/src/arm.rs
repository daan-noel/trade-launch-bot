//! Arming state + the compiled form of a rule.
//!
//! [`ArmState`] is the per-(token, rule) lifecycle the fold walks:
//! `PendingFirstSlot → Armed → EntryPending → Entered → ExitPending → End/…` with
//! `Disarmed`/`Done` terminals. [`CompiledRule`] is a [`LoadedRule`] pre-chewed at
//! `RulesReloaded` into exactly what the hot path needs — a flat list of metric
//! reads per side and the derived monotonic kills that let a hopeless entry
//! disarm itself (plan §2.2, §3.2) — so no JSON/param walking ever happens per
//! event.

use smallvec::SmallVec;

use crate::cap::Cap;
use crate::event::{
    DisarmReason, ExitReason, IntentId, LoadedRule, Portion, PositionId, RuleId, TradeMode,
};
use crate::fingerprint::FingerprintId;
use crate::metrics::evaluator::{eval, first_satisfied_cond, Condition, ConditionExpr, Operator};
use crate::metrics::position::{is_trailing, position_value, trailing_armed, PositionCtx};
use crate::metrics::track::TokenTrack;
use crate::metrics::{
    group_of, group_spec, is_fingerprint_scoped, is_two_window, metric_spec, MetricGroupId,
    MetricId, MetricKind, MetricScope, Ts, Windows,
};
use crate::rule_params::ReEntry;

/// Where an exit [`MetricReq`] came from — so a desugared TP/SL req still stamps
/// the `TakeProfit` / `StopLoss` exit-reason label (analytics group by it) instead
/// of the raw `pnl <op> value`. Entry reqs are always [`Authored`](Self::Authored).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReqOrigin {
    Authored,
    TakeProfit,
    StopLoss,
}

/// One metric read a rule side needs: the metric, the window it lives in (dynamic
/// metrics only), its `=`-tolerance, and the DNF condition arms. Precomputed so
/// evaluation is a flat loop of `track.value(..)` / `position_value(..)` + `eval(..)`.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricReq {
    pub metric: MetricId,
    pub window: Windows,
    /// Fingerprint scope for flow metrics; `None` for token-scoped groups.
    pub fingerprint: Option<FingerprintId>,
    pub tolerance: f64,
    /// DNF: OR of AND-arms.
    pub conds: ConditionExpr,
    /// `true` ⇒ read from the [`PositionCtx`] (`m_position`), not the token track —
    /// precomputed from the group's registry scope so the hot path never re-derives it.
    pub position_scoped: bool,
    /// Provenance (drives the exit-reason label); `Authored` for everything but the
    /// desugared TP/SL reqs.
    pub origin: ReqOrigin,
    /// `m_position.arm_above_pct` — set only on **trailing** exit reqs
    /// ([`is_trailing`](crate::metrics::position::is_trailing)). The req is skipped
    /// while position PnL is below this, so a trailing stop can be held off until
    /// the trade is in profit. `None` on every other req and on every stored rule.
    pub arm_above_pct: Option<f64>,
}

/// A derived monotonic upper bound from an **entry** condition arm: because a
/// monotonic metric (only `time` today) never decreases, once its value crosses
/// this threshold the arm can never re-satisfy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonoBound {
    pub metric: MetricId,
    pub window: Windows,
    pub threshold: f64,
    /// `true` ⇒ crossed at `value >= threshold` (from `<`); `false` ⇒ at
    /// `value > threshold` (from `<=` / `=`'s upper edge).
    pub cross_at_ge: bool,
}

impl MonoBound {
    /// True once a monotonic metric's `value` has crossed this derived entry
    /// upper bound. Non-finite ⇒ not crossed. Public so the sweep scan can
    /// replicate the fold's derived-disarm.
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

/// Per entry-metric derived mono-kill (OR of arms). The metric req is permanently
/// false only when **every** OR arm is dead; an arm with no upper mono bound
/// (`None`) never dies from a rising clock.
#[derive(Debug, Clone, PartialEq)]
pub struct MonoMetricKill {
    pub metric: MetricId,
    pub window: Windows,
    pub fingerprint: Option<FingerprintId>,
    /// Per OR arm: killing upper bound, or `None` if the arm never dies.
    pub arms: SmallVec<[Option<MonoBound>; 2]>,
}

impl MonoMetricKill {
    /// True when every OR arm is permanently unsatisfiable at `value`.
    pub fn permanently_false(&self, value: f64) -> bool {
        !self.arms.is_empty()
            && self.arms.iter().all(|arm| match arm {
                None => false,
                Some(b) => b.crossed(value),
            })
    }

    /// The bound that crossed **last** — the one whose crossing is what ended the
    /// episode, and so the only one worth naming as the deadline. `None` while any
    /// OR arm is still satisfiable, which makes this the whole kill test as well.
    ///
    /// Every arm is `Some` whenever [`permanently_false`](Self::permanently_false)
    /// holds (a `None` arm never dies), so the reduce always yields a bound.
    pub fn binding_bound(&self, value: f64) -> Option<MonoBound> {
        if !self.permanently_false(value) {
            return None;
        }
        self.arms.iter().flatten().copied().reduce(|a, b| {
            // Later on a rising metric = the higher threshold; at a tie the strict
            // `>` form (`cross_at_ge == false`), which needs one more step to cross.
            let later = b.threshold > a.threshold
                || (b.threshold == a.threshold && a.cross_at_ge && !b.cross_at_ge);
            if later {
                b
            } else {
                a
            }
        })
    }
}

/// One entry requirement that was **not** satisfied at the disarm instant, with
/// the reading that failed it.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockedReq {
    pub metric: MetricId,
    pub window: Windows,
    /// The value the fold read. `NaN` when unreadable — which, per the engine
    /// convention, satisfies nothing and is therefore itself a blocker.
    pub value: f64,
    /// The authored DNF this req judged `value` against.
    pub conds: ConditionExpr,
}

/// Why an entry became permanently unsatisfiable, captured at the instant the
/// fold gave up on it — the durable answer to "the rule watched this token for a
/// minute and passed; what was it short of".
///
/// [`killed_by`](Self::killed_by) is only ever the deadline (`time` is the sole
/// monotonic metric), so it says *when* the episode ended and never *why*. The
/// why is [`unmet`](Self::unmet): the entry conditions still failing as the clock
/// ran out. An **empty** `unmet` is its own answer — everything else held and the
/// token simply qualified too late.
#[derive(Debug, Clone, PartialEq)]
pub struct EntryBlockers {
    pub killed_by: MonoBound,
    pub unmet: Vec<BlockedReq>,
}

/// Tightest mono upper bound in one AND arm, if any.
fn arm_mono_upper(
    arm: &[Condition],
    half_tol: f64,
    metric: MetricId,
    window: Windows,
) -> Option<MonoBound> {
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
                Some((bth, bge)) => {
                    // Prefer the bound that crosses first on a rising metric.
                    if th < bth || (th == bth && ge && !bge) {
                        (th, ge)
                    } else {
                        (bth, bge)
                    }
                }
            });
        }
    }
    best.map(|(threshold, cross_at_ge)| MonoBound {
        metric,
        window,
        threshold,
        cross_at_ge,
    })
}

/// One compiled scale-out stage: optional `sell_bps` (`None` = remainder/`All`)
/// and the flattened exit reqs (per-stage TP desugared + authored conditions).
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledStage {
    pub sell_bps: Option<u16>,
    pub reqs: Vec<MetricReq>,
}

/// How long a rule's reads can keep changing **without a trade** — the per-rule
/// half of [`crate::reduce`]'s settled-token tick skip.
///
/// Almost every metric is frozen between two trades: price, reserves, lifetime
/// flows and extrema, and every position-scoped metric except `held` are functions
/// of trade data alone. Only four things move on a bare `Tick`, and each has a last
/// instant past which it can no longer flip a condition **this rule** authored:
///
/// * trailing windows decay until the newest trade ages out of the widest one;
/// * `time` climbs from creation until it passes the largest `time` threshold;
/// * `stall` climbs from the last all-time high (bounded above by the last trade);
/// * `held` climbs from the entry fill.
///
/// Past all four (and past the one-shot dead verdict, which is token- not
/// rule-scoped and so lives in [`crate::reduce`]) every further tick re-derives
/// identical readings and identical decisions. `0.0` means "this rule never reads
/// that clock", which is the strongest possible horizon, not a missing value.
///
/// Same idea — and the same horizon arithmetic — as the sweep's
/// [`SparseGrid`](crate::metrics::grid::SparseGrid), which skips provably-static
/// ticks when precomputing a series. This is that reasoning applied to the live
/// fold instead of to a precompute.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ClockHorizons {
    /// Widest trailing window read, across `m_flow_window`, `m_flow_ix_window`
    /// and `m_price_window`.
    pub max_window_secs: f64,
    /// Largest `m_state.time` threshold (+ its `=`-tolerance), from creation.
    pub time_secs: f64,
    /// Largest `m_price_lifetime.stall` threshold (+ tolerance), from the last trade.
    pub stall_secs: f64,
    /// Largest `m_position.held` threshold (+ tolerance), from the entry fill.
    pub held_secs: f64,
}

impl ClockHorizons {
    /// Widen every field to at least `other`'s — the union a whole rule set needs.
    /// Over-wide is always safe (it only re-evaluates a settled token); too narrow
    /// silently drops a decision, so every combinator here is a `max`.
    pub fn widen(self, other: Self) -> Self {
        Self {
            max_window_secs: self.max_window_secs.max(other.max_window_secs),
            time_secs: self.time_secs.max(other.time_secs),
            stall_secs: self.stall_secs.max(other.stall_secs),
            held_secs: self.held_secs.max(other.held_secs),
        }
    }

    /// Fold one metric requirement in: a clock metric's horizon is its largest
    /// authored threshold plus the metric's `=`-tolerance (an `=` condition is a
    /// band, so its upper edge is `value + tol/2`; the full tolerance is the cheap
    /// safe over-estimate). Clamped by [`SparseGrid::clamp_secs`] so a fat-fingered
    /// axis can't push the horizon to the end of time.
    ///
    /// [`SparseGrid::clamp_secs`]: crate::metrics::grid::SparseGrid::clamp_secs
    fn absorb_req(&mut self, r: &MetricReq) {
        use crate::metrics::grid::SparseGrid;
        // Both axes: a two-window group's horizon is its LONGEST window, and the
        // shorter one still needs a buffer.
        for w in [r.window.primary, r.window.secondary].into_iter().flatten() {
            // The tick grid is a wall clock, so a slot span converts at the nominal
            // slot time. It only sizes the horizon - never a metric reading - so an
            // approximation here costs coverage, never correctness.
            //
            // A PRINT span contributes nothing: its cursor moves only on a trade, and
            // a trade emits its own row. No tick between two prints can change what a
            // print window reads, so `0.0` here is the exact horizon, not an
            // under-estimate.
            let secs = match w.unit {
                crate::metrics::WindowUnit::Sec => w.size + w.lag,
                crate::metrics::WindowUnit::Slot => {
                    (w.size + w.lag) * crate::metrics::NOMINAL_SLOT_SECS
                }
                crate::metrics::WindowUnit::Print => 0.0,
            };
            self.max_window_secs = self.max_window_secs.max(SparseGrid::clamp_secs(secs));
        }
        let slot = match r.metric {
            MetricId::Time => &mut self.time_secs,
            MetricId::Stall => &mut self.stall_secs,
            MetricId::Held => &mut self.held_secs,
            _ => return,
        };
        for arm in &r.conds {
            for c in arm {
                *slot = slot.max(SparseGrid::clamp_secs(c.value.abs() + r.tolerance));
            }
        }
    }
}

/// A [`LoadedRule`] pre-chewed for the hot path. Metric reads are flattened, the
/// distinct windows are listed **one bucket per backing buffer** — see
/// [`flow_windows`](CompiledRule::flow_windows) — and the entry monotonic kills are
/// precomputed for derived-unsatisfiability disarm.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledRule {
    pub id: RuleId,
    pub fingerprint_id: FingerprintId,
    pub trade_mode: TradeMode,
    pub buy_amount_lamports: u64,
    /// Percent-of-pool sizing, when the rule authors it. `None` ⇒ the fixed
    /// [`buy_amount_lamports`](Self::buy_amount_lamports) above. Resolved per entry by
    /// [`resolve_buy_lamports`], never here — the size depends on the pool at the
    /// entry instant, which a compiled rule cannot know.
    pub buy_pct_of_vsol: Option<f64>,
    /// Both caps arrive **already decoded** out of their `0 = …` storage encoding
    /// ([`Cap`]) — the fold never re-derives the sentinel, it just asks `allows`.
    pub concurrent_cap: Cap,
    pub max_total: Cap,
    /// Authoring sugar, kept for the sweep / FE / stored rules. The **fold** does
    /// not read these — `compile` desugars them into position `pnl` reqs prepended
    /// to [`exit_reqs`](Self::exit_reqs); this field is the sweep's parallel-impl
    /// input only.
    pub take_profit: Option<f64>,
    /// See [`take_profit`](Self::take_profit).
    pub stop_loss: Option<f64>,
    /// Empty ⇒ enter on arm (the fingerprint alone is the entry signal).
    pub entry_reqs: Vec<MetricReq>,
    /// Completing-print event reqs. Empty ⇒ no separate event (`entry_reqs` is
    /// the whole gate). Harvest crowd shape lives here.
    pub event_reqs: Vec<MetricReq>,
    /// Slot lock for `event_reqs`. `None` ⇒ level-AND on every print.
    pub entry_lock: Option<crate::rule_params::EntryLock>,
    /// Flattened exit reqs (desugared TP/SL then authored) — window collection,
    /// readout listing, sweep columns. The **combinator** is [`exit_clauses`].
    pub exit_reqs: Vec<MetricReq>,
    /// Exit combinator: OR of AND-clauses. Object-form `exit` compiles to one
    /// singleton clause per req (today's flat OR). Array-form compiles one clause
    /// per element. TP/SL prepend as singleton clauses.
    pub exit_clauses: Vec<Vec<MetricReq>>,
    /// `m_position.arm_above_pct` from the exit side, if authored. Latches
    /// [`EnteredCtx::armed`]. `None` ⇒ `armed` reads 1.
    pub trail_arm_pct: Option<f64>,
    /// Ordered scale-out stages (empty = legacy full-close only). Evaluated via
    /// [`stage_fired`](Self::stage_fired) only when no global exit fired.
    pub scale_out: Vec<CompiledStage>,
    /// Distinct `m_flow_window` spans this rule reads across both sides — drive
    /// [`TokenTrack::ensure_window`].
    ///
    /// **One bucket per backing buffer**, because a span registered on the wrong one
    /// is a deque folded on every trade for a metric nobody reads. The four are
    /// disjoint by group, and a rule pays only for the ones its metrics select.
    ///
    /// [`TokenTrack::ensure_window`]: crate::metrics::track::TokenTrack::ensure_window
    pub flow_windows: SmallVec<[crate::metrics::WindowSpec; 2]>,
    /// Distinct `m_crowd_window` spans — drive
    /// [`TokenTrack::ensure_crowd_window`](crate::metrics::track::TokenTrack::ensure_crowd_window).
    /// Separate from [`flow_windows`](Self::flow_windows) because the crowd deque
    /// carries the WALLET column and nothing else needs it.
    pub crowd_windows: SmallVec<[crate::metrics::WindowSpec; 2]>,
    /// Distinct `m_price_window` spans — drive
    /// [`TokenTrack::ensure_price_window`](crate::metrics::track::TokenTrack::ensure_price_window).
    pub price_windows: SmallVec<[crate::metrics::WindowSpec; 2]>,
    /// Distinct `m_flow_ix_window` spans — drive
    /// [`TokenTrack::ensure_flow`](crate::metrics::track::TokenTrack::ensure_flow),
    /// which opens one deque **per fingerprint**. Passing the aggregate-flow spans
    /// here instead multiplied that: every configured fingerprint opened a buffer for
    /// every `m_flow_window` span in the whole rule set, folded on every trade, read
    /// by nothing.
    pub ix_windows: SmallVec<[crate::metrics::WindowSpec; 2]>,
    /// Distinct `m_dump_ix_window` spans — drive
    /// [`TokenTrack::ensure_dump`](crate::metrics::track::TokenTrack::ensure_dump),
    /// which opens one deque per fingerprint on its OWN build list. Separate from
    /// [`ix_windows`](Self::ix_windows) for the same reason that bucket is separate
    /// from `flow_windows`: the two groups read different lists into different
    /// buffers, and a rule reading one must not open the other.
    pub dump_windows: SmallVec<[crate::metrics::WindowSpec; 2]>,
    /// Whether any condition on this rule reads a window counted in SLOTS.
    ///
    /// A slot window advances only on [`TradeLite::slot`](crate::metrics::TradeLite::slot),
    /// and that field defaults to `0` ("not supplied") on any source that predates
    /// it — an event-log line written before the field existed replays with every
    /// trade in slot `0`, so the cursor never moves and the window holds its opening
    /// content forever. That failure is silent and reads exactly like a strict gate
    /// that simply never fires, which is why the answer is precomputed here for a
    /// loader to check its source against rather than left for each caller to
    /// rediscover.
    pub needs_slot: bool,
    /// Per entry-metric mono kills (for derived-unsatisfiability disarm).
    pub mono_kills: SmallVec<[MonoMetricKill; 2]>,
    /// How long this rule's readings can still move without a trade — see
    /// [`ClockHorizons`]. Consumed by [`crate::reduce`] to skip provably-static
    /// ticks on a quiet token.
    pub clock_horizons: ClockHorizons,
    /// Re-entry lifecycle (plan Ph4). `None` ⇒ one-shot: a closed position ends the
    /// (token, rule) forever (`Done`). `Some` ⇒ a normal strategy exit re-arms the
    /// token into [`ArmState::Cooldown`] up to the episode cap.
    pub reentry: Option<ReEntry>,
    /// Skip entry while any OTHER arm on the token holds a position. See
    /// [`RuleParams::exclusive`](crate::rule_params::RuleParams::exclusive).
    pub exclusive: bool,
    /// Visit order for [`crate::reduce`]'s per-event arm sweep (higher first) — the
    /// tiebreak between two contesting `exclusive` rules. The **sweep ignores both**
    /// (documented divergence, `docs/plans/sweep/sim-parity.md`).
    pub priority: i32,
    /// When `false`, skip arming and new entries; exits on held positions still run.
    pub entry_enabled: bool,
}

impl CompiledRule {
    /// Pre-chew a loaded rule. Its `params` are already parsed + validated, so this
    /// is a pure structural walk (no failure path).
    pub fn compile(rule: &LoadedRule) -> Self {
        let entry_reqs = rule
            .params
            .entry
            .as_ref()
            .map(|s| build_reqs(s, rule.fingerprint_id))
            .unwrap_or_default();
        let event_reqs = rule
            .params
            .entry_event
            .as_ref()
            .map(|s| build_reqs(s, rule.fingerprint_id))
            .unwrap_or_default();
        let entry_lock = rule.params.entry_lock;
        let authored_clauses: Vec<Vec<MetricReq>> = match &rule.params.exit {
            None => Vec::new(),
            Some(crate::rule_params::ExitSide::Any(s)) => {
                build_reqs(s, rule.fingerprint_id).into_iter().map(|r| vec![r]).collect()
            }
            Some(crate::rule_params::ExitSide::Dnf(clauses)) => clauses
                .iter()
                .map(|s| build_reqs(s, rule.fingerprint_id))
                .collect(),
        };

        // DESUGAR the `take_profit` / `stop_loss` sugar into position-scoped `pnl`
        // exit clauses — one exit-evaluation path (`pnl >= tp` / `pnl <= -sl`) shared
        // with authored `m_position.pnl` conditions. PREPENDED as singleton clauses
        // so a catastrophe stop still ranks above softer metric exits.
        let mut exit_clauses: Vec<Vec<MetricReq>> = Vec::new();
        if let Some(sl) = rule.params.stop_loss {
            exit_clauses.push(vec![pnl_req(Operator::Lte, -sl, ReqOrigin::StopLoss)]);
        }
        if let Some(tp) = rule.params.take_profit {
            exit_clauses.push(vec![pnl_req(Operator::Gte, tp, ReqOrigin::TakeProfit)]);
        }
        exit_clauses.extend(authored_clauses);
        let exit_reqs: Vec<MetricReq> = exit_clauses.iter().flatten().cloned().collect();

        let trail_arm_pct = rule.params.exit.as_ref().and_then(extract_trail_arm_pct);

        // Scale-out stages: same TP desugar + build_reqs per stage.
        let scale_out: Vec<CompiledStage> = rule
            .params
            .scale_out
            .as_ref()
            .map(|stages| {
                stages
                    .iter()
                    .map(|s| {
                        let mut reqs = build_reqs(&s.conditions, rule.fingerprint_id);
                        if let Some(tp) = s.take_profit {
                            let mut with_tp = vec![pnl_req(Operator::Gte, tp, ReqOrigin::TakeProfit)];
                            with_tp.append(&mut reqs);
                            reqs = with_tp;
                        }
                        CompiledStage { sell_bps: s.sell_bps, reqs }
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Distinct windows across both sides + every scale-out stage (dynamic
        // metrics only), bucketed by which buffer they drive. The routing is by
        // GROUP, off the registry, so a new dynamic group lands in its own bucket
        // rather than silently in the flow one.
        let mut flow_windows: SmallVec<[crate::metrics::WindowSpec; 2]> = SmallVec::new();
        let mut crowd_windows: SmallVec<[crate::metrics::WindowSpec; 2]> = SmallVec::new();
        let mut price_windows: SmallVec<[crate::metrics::WindowSpec; 2]> = SmallVec::new();
        let mut ix_windows: SmallVec<[crate::metrics::WindowSpec; 2]> = SmallVec::new();
        let mut dump_windows: SmallVec<[crate::metrics::WindowSpec; 2]> = SmallVec::new();
        let mut needs_slot = false;
        let stage_reqs = scale_out.iter().flat_map(|s| s.reqs.iter());
        for r in entry_reqs
            .iter()
            .chain(event_reqs.iter())
            .chain(exit_reqs.iter())
            .chain(stage_reqs)
        {
            let bucket = match group_of(r.metric).id {
                MetricGroupId::PriceWindow => &mut price_windows,
                MetricGroupId::CrowdWindow => &mut crowd_windows,
                MetricGroupId::FlowIxWindow => &mut ix_windows,
                MetricGroupId::DumpIxWindow => &mut dump_windows,
                _ => &mut flow_windows,
            };
            // Both axes: a two-window group needs a buffer for each of them, and
            // registering only the primary would leave the second read as NaN.
            for w in [r.window.primary, r.window.secondary].into_iter().flatten() {
                if !bucket.contains(&w) {
                    bucket.push(w);
                }
            }
            needs_slot |= r.window.needs_slot();
            needs_slot |= group_of(r.metric).id == MetricGroupId::BurstSlot;
        }

        // Tick horizons — every clock this rule reads, on EVERY side (entry, exit,
        // and each scale-out stage), because any of them can flip a decision on a
        // bare tick. Derived from the same req list the fold evaluates, so a newly
        // authored condition widens the horizon automatically.
        let mut clock_horizons = ClockHorizons::default();
        for r in entry_reqs
            .iter()
            .chain(event_reqs.iter())
            .chain(exit_reqs.iter())
            .chain(scale_out.iter().flat_map(|s| s.reqs.iter()))
        {
            clock_horizons.absorb_req(r);
        }

        // Monotonic entry kills — per metric, OR of arm upper bounds.
        let mut mono_kills: SmallVec<[MonoMetricKill; 2]> = SmallVec::new();
        for r in entry_reqs.iter().chain(event_reqs.iter()) {
            if !metric_spec(r.metric).monotonic {
                continue;
            }
            let half = r.tolerance / 2.0;
            let mut arms: SmallVec<[Option<MonoBound>; 2]> = SmallVec::new();
            let mut any_bound = false;
            for arm in &r.conds {
                let bound = arm_mono_upper(arm, half, r.metric, r.window);
                if bound.is_some() {
                    any_bound = true;
                }
                arms.push(bound);
            }
            if any_bound {
                mono_kills.push(MonoMetricKill {
                    metric: r.metric,
                    window: r.window,
                    fingerprint: r.fingerprint,
                    arms,
                });
            }
        }

        Self {
            id: rule.id,
            fingerprint_id: rule.fingerprint_id,
            trade_mode: rule.trade_mode,
            buy_amount_lamports: rule.buy_amount_lamports,
            buy_pct_of_vsol: rule.params.buy_pct_of_vsol,
            concurrent_cap: rule.concurrent_cap(),
            max_total: rule.total_cap(),
            take_profit: rule.params.take_profit,
            stop_loss: rule.params.stop_loss,
            entry_reqs,
            event_reqs,
            entry_lock,
            exit_reqs,
            exit_clauses,
            trail_arm_pct,
            scale_out,
            flow_windows,
            crowd_windows,
            price_windows,
            ix_windows,
            dump_windows,
            needs_slot,
            mono_kills,
            clock_horizons,
            reentry: rule.params.reentry,
            exclusive: rule.params.exclusive,
            priority: rule.params.priority,
            entry_enabled: rule.entry_enabled,
        }
    }

    /// Whether arming alone is the entry signal (no entry conditions authored).
    pub fn enter_on_arm(&self) -> bool {
        self.entry_reqs.is_empty() && self.event_reqs.is_empty()
    }

    /// Whether every `entry` filter holds at `now` (AND). Vacuous when empty.
    pub fn entry_satisfied(&self, track: &TokenTrack, now: Ts) -> bool {
        reqs_satisfied(&self.entry_reqs, track, now)
    }

    /// Whether the completing-print event holds at `now`. Vacuous when empty.
    pub fn event_satisfied(&self, track: &TokenTrack, now: Ts) -> bool {
        reqs_satisfied(&self.event_reqs, track, now)
    }

    /// Whether this rule has metric exit conditions at all.
    pub fn has_exit_metrics(&self) -> bool {
        !self.exit_reqs.is_empty()
    }

    /// Whether the exit fires at `now`. Exit is OR of AND-clauses ([`exit_clauses`]):
    /// object-form compiles to one singleton clause per req (today's flat OR);
    /// array-form compiles one clause per element. Within a metric the expr is DNF
    /// (`,` AND / `|` OR). `false` when the rule authored no exit metrics — the
    /// caller falls back to death only (TP/SL already live as prepended clauses).
    pub fn exit_metrics_satisfied(&self, track: &TokenTrack, now: Ts) -> bool {
        self.exit_metrics_fired(track, now).is_some()
    }

    /// First exit metric that holds at `now`, with the first satisfied condition
    /// (operator + authored threshold) — stamped on [`ExitReason::Metrics`].
    /// Token-scoped only: reads through the track, so position metrics (which need a
    /// [`PositionCtx`]) read `NaN` here. Used by the pre-entry [`can_enter`] gate,
    /// where there is no position — the held-side exit uses [`exit_fired`].
    ///
    /// [`can_enter`]: Self::can_enter
    /// [`exit_fired`]: Self::exit_fired
    pub fn exit_metrics_fired(
        &self,
        track: &TokenTrack,
        now: Ts,
    ) -> Option<(MetricId, Operator, f64)> {
        if !self.has_exit_metrics() {
            return None;
        }
        clauses_first_fired(&self.exit_clauses, track, now)
    }

    /// The **held-side** exit decision: the first exit *clause* that fires at `now`,
    /// as a labelled [`ExitReason`]. Walks [`exit_clauses`] in order (desugared TP/SL
    /// prepended as singleton clauses). Inside a clause every req must hold (AND);
    /// clauses OR. Trailing skip applies only to a **singleton** trailing clause
    /// (object-form); a DNF trail that ANDs `armed` explicitly is not skipped.
    pub fn exit_fired(
        &self,
        track: &TokenTrack,
        ctx: &PositionCtx,
        now: Ts,
    ) -> Option<ExitReason> {
        clauses_exit_fired(&self.exit_clauses, track, ctx, now)
    }

    /// Whether the current scale-out stage fires at `now`. Only the stage at
    /// `stage` is evaluated; past the ladder ⇒ `None` (position continues under
    /// the global exit side alone). Same req walk as [`exit_fired`].
    pub fn stage_fired(
        &self,
        stage: u8,
        track: &TokenTrack,
        ctx: &PositionCtx,
        now: Ts,
    ) -> Option<ExitReason> {
        let s = self.scale_out.get(stage as usize)?;
        reqs_exit_fired(&s.reqs, track, ctx, now)
    }

    /// The compiled stage at `stage`, if any (for the fold to read `sell_bps`).
    pub fn stage_at(&self, stage: u8) -> Option<&CompiledStage> {
        self.scale_out.get(stage as usize)
    }

    /// Whether the armed side may submit a buy at `now`.
    ///
    /// Entry conditions must hold, and exit metrics (if any) must **not** already
    /// hold. Level-triggered exit OR would otherwise sell on the next event after
    /// fill — a worthless round-trip when params overlap. TP/SL are not part of
    /// this gate (they need an entry price).
    /// Entry + event hold, and exit metrics do not. Used by buy-retry (lock is
    /// already spent on the completing print that submitted). [`try_enter`] is
    /// the Armed-side gate that also spends a slot.
    pub fn can_enter(&self, track: &TokenTrack, now: Ts) -> bool {
        self.event_satisfied(track, now)
            && self.entry_satisfied(track, now)
            && !self.exit_metrics_satisfied(track, now)
    }

    /// Armed-side entry: with [`entry_lock`](Self::entry_lock) `slot`, the first
    /// print this slot that makes `event_reqs` true is the only candidate.
    pub fn try_enter(
        &self,
        track: &TokenTrack,
        now: Ts,
        locked_slot: Option<u64>,
    ) -> EntryVerdict {
        use crate::rule_params::EntryLock;
        match self.entry_lock {
            None => {
                if self.can_enter(track, now) {
                    EntryVerdict::Enter
                } else {
                    EntryVerdict::No
                }
            }
            Some(EntryLock::Slot) => {
                let slot = track.cur_slot();
                if locked_slot == Some(slot) && slot != 0 {
                    return EntryVerdict::No;
                }
                if !self.event_satisfied(track, now) {
                    return EntryVerdict::No;
                }
                if self.entry_satisfied(track, now) && !self.exit_metrics_satisfied(track, now) {
                    EntryVerdict::Enter
                } else {
                    EntryVerdict::SpendSlot
                }
            }
        }
    }

    /// The monotonic entry bound that is permanently crossed at `now`, if any —
    /// the entry can never re-satisfy, so the arm should disarm
    /// ([`DisarmReason::Unsatisfiable`]). Returns the bound rather than a `bool`
    /// so the disarm can name its own deadline; the hot path only tests `is_some`.
    pub fn entry_unsatisfiable(&self, track: &TokenTrack, now: Ts) -> Option<MonoBound> {
        self.mono_kills.iter().find_map(|k| {
            k.binding_bound(track.value(k.metric, k.window, k.fingerprint, now))
        })
    }

    /// Every entry req still unmet at `now`, beside the deadline that killed the
    /// arm — [`EntryBlockers`]'s payload.
    ///
    /// **Cold path.** Called once per episode, from the disarm itself, never per
    /// event: it walks the entry reqs without short-circuiting (the point is the
    /// whole failing set, not the first member) and clones each authored DNF.
    /// [`entry_satisfied`](Self::entry_satisfied) stays the hot-path test.
    ///
    /// The req `killed_by` came from is excluded — at this instant it is failing
    /// *by construction*, and listing it beside the real blockers is what makes a
    /// disarm read as two problems when it is one.
    pub fn entry_blockers(
        &self,
        track: &TokenTrack,
        now: Ts,
        killed_by: MonoBound,
    ) -> EntryBlockers {
        let unmet = self
            .event_reqs
            .iter()
            .chain(self.entry_reqs.iter())
            .filter(|r| !(r.metric == killed_by.metric && r.window == killed_by.window))
            .filter_map(|r| {
                let value = track.value(r.metric, r.window, r.fingerprint, now);
                (!eval(&r.conds, value, r.tolerance)).then(|| BlockedReq {
                    metric: r.metric,
                    window: r.window,
                    value,
                    conds: r.conds.clone(),
                })
            })
            .collect();
        EntryBlockers { killed_by, unmet }
    }
}

/// Armed-side entry result. [`SpendSlot`] is a completing print whose filters
/// failed — that slot must not retry on a later print.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryVerdict {
    No,
    Enter,
    SpendSlot,
}

/// Shared exit-req walk for the global side and each scale-out stage.
fn reqs_exit_fired(
    reqs: &[MetricReq],
    track: &TokenTrack,
    ctx: &PositionCtx,
    now: Ts,
) -> Option<ExitReason> {
    let price = track.current_price();
    for r in reqs {
        // A trailing stop held off until the position is `arm_above_pct` in
        // profit. Only ever set on trailing reqs, so TP/SL and every other exit
        // metric are untouched.
        if !trailing_armed(r.arm_above_pct, ctx, price) {
            continue;
        }
        if let Some(reason) = req_exit_reason(r, track, ctx, now, price) {
            return Some(reason);
        }
    }
    None
}

/// One req's held-side reason if it fires. Does **not** apply the trailing skip —
/// the clause walker decides that (singleton-only).
fn req_exit_reason(
    r: &MetricReq,
    track: &TokenTrack,
    ctx: &PositionCtx,
    now: Ts,
    price: f64,
) -> Option<ExitReason> {
    let reading = if r.position_scoped {
        position_value(r.metric, ctx, price, now)
    } else {
        track.value(r.metric, r.window, r.fingerprint, now)
    };
    let c = first_satisfied_cond(&r.conds, reading, r.tolerance)?;
    Some(match r.origin {
        ReqOrigin::TakeProfit => ExitReason::TakeProfit,
        ReqOrigin::StopLoss => ExitReason::StopLoss,
        ReqOrigin::Authored => ExitReason::Metrics {
            metric: r.metric,
            operator: c.operator,
            value: c.value,
            window: r.window.primary,
        },
    })
}

/// DNF exit: OR of AND-clauses. Trailing skip only on a singleton trailing clause.
fn clauses_exit_fired(
    clauses: &[Vec<MetricReq>],
    track: &TokenTrack,
    ctx: &PositionCtx,
    now: Ts,
) -> Option<ExitReason> {
    let price = track.current_price();
    for clause in clauses {
        if clause.is_empty() {
            continue;
        }
        let singleton_trail = clause.len() == 1 && clause[0].arm_above_pct.is_some();
        if singleton_trail && !trailing_armed(clause[0].arm_above_pct, ctx, price) {
            continue;
        }
        let mut first = None;
        let mut all = true;
        for r in clause {
            match req_exit_reason(r, track, ctx, now, price) {
                Some(reason) => {
                    if first.is_none() {
                        first = Some(reason);
                    }
                }
                None => {
                    all = false;
                    break;
                }
            }
        }
        if all {
            return first;
        }
    }
    None
}

/// AND every metric read in `reqs` at `now` (the **entry** combinator). Empty ⇒
/// `true` (vacuous).
fn reqs_satisfied(reqs: &[MetricReq], track: &TokenTrack, now: Ts) -> bool {
    reqs.iter().all(|r| {
        eval(
            &r.conds,
            track.value(r.metric, r.window, r.fingerprint, now),
            r.tolerance,
        )
    })
}

/// Pre-entry exit veto: a clause fires (blocks entry) only when every req in it
/// fires. Position-scoped reqs read `NaN` off the track, so a DNF trail/death
/// clause that ANDs `m_position` does not block entry. Object-form singleton
/// token-scoped exits still do.
fn clauses_first_fired(
    clauses: &[Vec<MetricReq>],
    track: &TokenTrack,
    now: Ts,
) -> Option<(MetricId, Operator, f64)> {
    for clause in clauses {
        if clause.is_empty() {
            continue;
        }
        let mut first = None;
        let mut all = true;
        for r in clause {
            let reading = track.value(r.metric, r.window, r.fingerprint, now);
            match first_satisfied_cond(&r.conds, reading, r.tolerance) {
                Some(c) => {
                    if first.is_none() {
                        first = Some((r.metric, c.operator, c.value));
                    }
                }
                None => {
                    all = false;
                    break;
                }
            }
        }
        if all {
            return first;
        }
    }
    None
}

/// `m_position.arm_above_pct` from the authored exit, first clause that names it.
fn extract_trail_arm_pct(exit: &crate::rule_params::ExitSide) -> Option<f64> {
    for side in exit.clauses() {
        if let Some(instances) = side.0.get(&MetricGroupId::Position) {
            for g in instances {
                if let Some(v) = g.strict_param("arm_above_pct") {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// A desugared TP/SL exit req: a single `m_position.pnl <op> value` condition,
/// tagged with the ladder origin so its exit reason stays `TakeProfit`/`StopLoss`.
fn pnl_req(op: Operator, value: f64, origin: ReqOrigin) -> MetricReq {
    MetricReq {
        metric: MetricId::Pnl,
        window: Windows::NONE,
        fingerprint: None,
        tolerance: metric_spec(MetricId::Pnl).eq_tolerance,
        conds: vec![vec![Condition { operator: op, value }]],
        position_scoped: true,
        origin,
        // TP/SL are `pnl` reqs, never trailing — a stop-loss gated on already being
        // in profit would never fire.
        arm_above_pct: None,
    }
}

/// Flatten one side's parsed conditions into [`MetricReq`]s. A dynamic group's
/// metrics carry its `window_size_sec`; static groups carry `None`. Flow metrics
/// are scoped to `fingerprint_id`.
fn build_reqs(
    side: &crate::rule_params::SideConditions,
    fingerprint_id: FingerprintId,
) -> Vec<MetricReq> {
    let mut out = Vec::new();
    for (group_id, instances) in &side.0 {
        let is_dynamic = group_spec(*group_id).kind == MetricKind::Dynamic;
        let position_scoped = group_spec(*group_id).scope == MetricScope::Position;
        // One instance per window (static groups carry exactly one, window-less).
        for group in instances {
            // Both axes, read by AXIS rather than by group: a slice param is absent
            // on every group that does not declare it, so this stays one line of
            // vocabulary instead of a per-group branch that a new window basis would
            // have to be remembered into.
            let primary =
                is_dynamic.then(|| group.window_spec(&crate::metrics::WINDOW_AXIS)).flatten();
            // The slice axis rides the SAME clock as the reference (that pair IS the
            // two-window basis), so it takes the group's unit and lag and differs only
            // in size. Attached PER METRIC below, never to the instance: it sits on
            // `m_flow_window` beside the single-window metrics, and a `gross_flow`
            // requirement carrying a span it never reads would be a different
            // requirement IDENTITY for the same read — two rules gating on
            // `gross_flow(30s)` would stop sharing one buffer because one of them also
            // happens to gate on `trade_share`.
            let slice = is_dynamic
                .then(|| group.window_spec(&crate::metrics::flow_slice::SLICE_AXIS))
                .flatten();
            // Attached below to this instance's trailing metrics only.
            let arm_above = group.strict_param("arm_above_pct");
            for (metric_id, conds) in &group.metrics {
                out.push(MetricReq {
                    metric: *metric_id,
                    window: Windows {
                        primary,
                        secondary: is_two_window(*metric_id).then_some(slice).flatten(),
                    },
                    fingerprint: is_fingerprint_scoped(*metric_id).then_some(fingerprint_id),
                    tolerance: metric_spec(*metric_id).eq_tolerance,
                    conds: conds.clone(),
                    position_scoped,
                    origin: ReqOrigin::Authored,
                    arm_above_pct: is_trailing(*metric_id).then_some(arm_above).flatten(),
                });
            }
        }
    }
    out
}

/// Held-position snapshot shared by [`ArmState::Entered`] and a partial
/// [`ArmState::ExitPending`] (restored on fill so peak/trough/entry are not
/// reseeded). `stage` / `sold_bps` are 0 on legacy (no scale-out) positions.
#[derive(Debug, Clone, PartialEq)]
pub struct EnteredCtx {
    pub position: PositionId,
    pub entry_price: f64,
    pub entered_at: Ts,
    pub peak_price: f64,
    pub trough_price: f64,
    /// Index of the next scale-out stage to evaluate (`0` = first / no ladder).
    pub stage: u8,
    /// Cumulative bps of the initial bag already sold via scale-out legs.
    pub sold_bps: u16,
    /// Trail latch — see [`PositionCtx::armed`](crate::metrics::position::PositionCtx::armed).
    pub armed: bool,
    pub trail_arm_pct: Option<f64>,
}

impl EnteredCtx {
    /// Seed a fresh entry fill: peak/trough start at the fill price; stage ladder
    /// at 0.
    pub fn at_fill(position: PositionId, fill_price: f64, at: Ts, trail_arm_pct: Option<f64>) -> Self {
        Self {
            position,
            entry_price: fill_price,
            entered_at: at,
            peak_price: fill_price,
            trough_price: fill_price,
            stage: 0,
            sold_bps: 0,
            armed: trail_arm_pct.is_none(),
            trail_arm_pct,
        }
    }

    /// Build the [`PositionCtx`] position-scoped metrics read.
    pub fn position_ctx(&self) -> PositionCtx {
        PositionCtx {
            entry_price: self.entry_price,
            peak_price: self.peak_price,
            trough_price: self.trough_price,
            entered_at: self.entered_at,
            armed: self.armed,
            trail_arm_pct: self.trail_arm_pct,
        }
    }
}

/// Per-(token, rule) arming lifecycle. `attempts` counts submit tries so a bounded
/// retry policy can give up (plan §3.3 fill-failure handling).
#[derive(Debug, Clone, PartialEq)]
pub enum ArmState {
    /// Matched a fingerprint's instant axes but that fingerprint also has a
    /// first-slot axis not yet settled — awaiting `FirstSlotSettled`.
    PendingFirstSlot,
    /// Armed and evaluating entry conditions on every trade/tick.
    Armed,
    /// A buy is in flight for `intent` (the `attempts`-th try); the position row
    /// already exists (`BuySubmitted`) so a fill just flips it to held.
    /// `lamports` is the submitted buy size, frozen at submit so retries resize
    /// identically (manual episodes have no rule row to re-read; `0` ⇒ fall back
    /// to the rule's configured amount, as boot-adopted arms do).
    EntryPending { intent: IntentId, position: PositionId, attempts: u32, lamports: u64 },
    /// Entry filled; the position is held and evaluating exit / scale-out.
    Entered(EnteredCtx),
    /// A sell is in flight for `intent` (the `attempts`-th try), closing (a
    /// portion of) the bag for `reason`. `held` is the Entered snapshot —
    /// restored on a partial fill; discarded on a full close.
    ExitPending {
        intent: IntentId,
        reason: ExitReason,
        attempts: u32,
        portion: Portion,
        held: EnteredCtx,
    },
    /// A re-entry rule closed a position and is waiting out its cooldown before
    /// re-arming: the fold promotes it back to [`Armed`](Self::Armed) once a
    /// trade/tick carries `now >= until` (plan Ph4). **Non-terminal** — the token
    /// must stay tracked so it can re-arm, even with no open position.
    Cooldown { until: Ts },
    /// Terminal: the position closed (or the token is done forever for this rule).
    Done,
    /// Terminal: disarmed before entry for `reason`.
    Disarmed(DisarmReason),
}

impl ArmState {
    /// Whether this arm still needs the token tracked (non-terminal). A token with
    /// no active arms and no open position can be pruned. [`Cooldown`](Self::Cooldown)
    /// counts as active — it is awaiting re-arm, not finished.
    pub fn is_active(&self) -> bool {
        !matches!(self, ArmState::Done | ArmState::Disarmed(_))
    }

    /// The position this arm owns, if any — the key a manual episode's one-off exit
    /// rule is stored under ([`EngineState::rule_for`](crate::state::EngineState::rule_for)).
    /// `EntryPending` counts: its row exists and its rule must resolve.
    pub fn position(&self) -> Option<PositionId> {
        match self {
            ArmState::EntryPending { position, .. } => Some(*position),
            ArmState::Entered(ctx) | ArmState::ExitPending { held: ctx, .. } => Some(ctx.position),
            _ => None,
        }
    }

    /// The held-position snapshot when a bag is open — `Entered`, or `ExitPending`
    /// whose `held` is exactly that snapshot (a partial fill restores it). The one
    /// place a reader gets a [`PositionCtx`] from an arm, so a caller that adds a
    /// held-side state cannot silently miss it.
    pub fn held(&self) -> Option<&EnteredCtx> {
        match self {
            ArmState::Entered(ctx) | ArmState::ExitPending { held: ctx, .. } => Some(ctx),
            _ => None,
        }
    }

    /// Short stable tag for a UI / log line (never parsed back into a state).
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
mod tests {
    use crate::metrics::WindowSpec;
    use super::*;
    use crate::event::RuleId;
    use crate::fingerprint::FingerprintId;
    use crate::rule_params::RuleParams;
    use serde_json::json;
    use uuid::Uuid;

    fn rule(params: serde_json::Value) -> LoadedRule {
        LoadedRule {
            id: RuleId(Uuid::from_u128(1)),
            fingerprint_id: FingerprintId(Uuid::from_u128(2)),
            trade_mode: TradeMode::Paper,
            buy_amount_lamports: 1_000_000_000,
            max_concurrent_tokens: 1,
            max_total_tokens: 0,
            params: RuleParams::parse(&params).unwrap(),
            entry_enabled: true,
        }
    }

    /// The slot flag must answer for EVERY side a condition can sit on, and stay
    /// `false` for the two other window bases — a loader that read it as "any window"
    /// would refuse a perfectly loadable seconds/prints rule.
    #[test]
    fn needs_slot_is_true_for_a_slot_window_on_any_side() {
        let seconds = CompiledRule::compile(&rule(json!({
            "entry": { "m_flow_window": { "net_flow": [{ "operator": ">=", "value": 1 }], "window_size_sec": 30 } }
        })));
        assert!(!seconds.needs_slot, "a seconds window advances on the clock");

        let prints = CompiledRule::compile(&rule(json!({
            "entry": { "m_flow_window": { "net_flow": [{ "operator": ">=", "value": 1 }], "window_size_prints": 5 } }
        })));
        assert!(!prints.needs_slot, "a print window advances on the fold counter");

        let on_entry = CompiledRule::compile(&rule(json!({
            "entry": { "m_flow_window": { "net_flow": [{ "operator": ">=", "value": 1 }], "window_size_slots": 3 } }
        })));
        assert!(on_entry.needs_slot);

        let on_exit = CompiledRule::compile(&rule(json!({
            "exit": { "m_price_window": { "trail": [{ "operator": ">=", "value": 5 }], "window_size_slots": 3 } }
        })));
        assert!(on_exit.needs_slot, "an exit-only slot window still needs the column");
    }

    #[test]
    fn enter_on_arm_when_no_entry_conditions() {
        let c = CompiledRule::compile(&rule(json!({ "take_profit": 100 })));
        assert!(c.enter_on_arm());
        assert!(c.entry_reqs.is_empty());
        assert!(c.mono_kills.is_empty());
    }

    #[test]
    fn tp_sl_desugar_into_prepended_origin_tagged_pnl_reqs() {
        // take_profit / stop_loss expand to position `pnl` reqs, PREPENDED (SL, TP)
        // ahead of authored exit metrics, each tagged with its ladder origin.
        let c = CompiledRule::compile(&rule(json!({
            "take_profit": 100,
            "stop_loss": 30,
            "exit": { "m_position": { "retrace": [{ "operator": ">=", "value": 3 }] } }
        })));
        assert_eq!(c.exit_reqs.len(), 3, "SL + TP + authored retrace");
        // SL first (catastrophe stop outranks softer exits), then TP, then authored.
        assert_eq!(c.exit_reqs[0].metric, MetricId::Pnl);
        assert_eq!(c.exit_reqs[0].origin, ReqOrigin::StopLoss);
        assert!(c.exit_reqs[0].position_scoped);
        assert_eq!(
            c.exit_reqs[0].conds,
            vec![vec![Condition { operator: Operator::Lte, value: -30.0 }]]
        );
        assert_eq!(c.exit_reqs[1].metric, MetricId::Pnl);
        assert_eq!(c.exit_reqs[1].origin, ReqOrigin::TakeProfit);
        assert_eq!(
            c.exit_reqs[1].conds,
            vec![vec![Condition { operator: Operator::Gte, value: 100.0 }]]
        );
        // The authored retrace metric keeps Authored origin and is position-scoped.
        assert_eq!(c.exit_reqs[2].metric, MetricId::Retrace);
        assert_eq!(c.exit_reqs[2].origin, ReqOrigin::Authored);
        assert!(c.exit_reqs[2].position_scoped);
        // The sugar fields survive for the sweep / FE parallel impl.
        assert_eq!(c.take_profit, Some(100.0));
        assert_eq!(c.stop_loss, Some(30.0));
    }

    #[test]
    fn windows_deduped_across_sides() {
        let c = CompiledRule::compile(&rule(json!({
            "entry": { "m_flow_window": { "window_size_sec": 10, "buy": [{"operator": ">", "value": 1}] } },
            "exit":  { "m_flow_window": { "window_size_sec": 10, "sell": [{"operator": ">", "value": 1}] } }
        })));
        assert_eq!(c.flow_windows.as_slice(), &[WindowSpec::secs(10.0)]);
        assert!(c.price_windows.is_empty());
    }

    #[test]
    fn multi_window_group_compiles_to_distinct_reqs_and_windows() {
        // Two m_flow_window clauses on entry (30 s gross gate + 2 s net gate) become
        // two independent entry reqs, each carrying its own window; both windows are
        // registered so `ensure_window` opens both buffers.
        let c = CompiledRule::compile(&rule(json!({
            "entry": { "m_flow_window": [
                { "window_size_sec": 30, "gross_flow": [{"operator": ">=", "value": 10}] },
                { "window_size_sec": 2,  "net_flow":   [{"operator": ">=", "value": 0}]  }
            ] }
        })));
        assert_eq!(c.entry_reqs.len(), 2);
        let gross = c.entry_reqs.iter().find(|r| r.metric == MetricId::GrossFlow).unwrap();
        let net = c.entry_reqs.iter().find(|r| r.metric == MetricId::NetFlow).unwrap();
        assert_eq!(gross.window, Windows::secs(30.0));
        assert_eq!(net.window, Windows::secs(2.0));
        // Both distinct windows collected for ensure_window (order = req order).
        assert_eq!(c.flow_windows.len(), 2);
        assert!(c.flow_windows.contains(&WindowSpec::secs(30.0)) && c.flow_windows.contains(&WindowSpec::secs(2.0)));
        assert!(c.price_windows.is_empty());
    }

    /// Each dynamic group registers on ITS OWN buffer and no other.
    ///
    /// The buckets are how a rule pays only for the deques it reads, so a span in the
    /// wrong one is either a fold for nothing (`m_crowd_window`'s wallet map on a
    /// `gross_flow` rule) or, for `ix_windows`, that fold multiplied by every
    /// configured fingerprint — `ensure_flow` opens one deque each.
    #[test]
    fn every_dynamic_group_registers_on_its_own_buffer_alone() {
        let c = CompiledRule::compile(&rule(json!({
            "entry": {
                "m_flow_window":    { "window_size_sec": 10, "gross_flow": [{"operator": ">=", "value": 1}] },
                "m_crowd_window":   { "window_size_sec": 20, "unique_wallets": [{"operator": ">=", "value": 3}] },
                "m_price_window":   { "window_size_sec": 30, "trail": [{"operator": ">=", "value": 5}] },
                "m_flow_ix_window": { "window_size_sec": 40, "tagged_buy": [{"operator": ">=", "value": 1}] }
            }
        })));
        assert_eq!(c.flow_windows.as_slice(), &[WindowSpec::secs(10.0)]);
        assert_eq!(c.crowd_windows.as_slice(), &[WindowSpec::secs(20.0)]);
        assert_eq!(c.price_windows.as_slice(), &[WindowSpec::secs(30.0)]);
        assert_eq!(c.ix_windows.as_slice(), &[WindowSpec::secs(40.0)]);
    }

    /// A crowd gate reads through the buffer it registered — the end-to-end property
    /// the split has to preserve. A `m_flow_window` registration alone must NOT serve
    /// it: that would mean the wallet column is still riding the flow deque.
    #[test]
    fn a_crowd_gate_reads_only_through_the_crowd_buffer_it_registered() {
        use crate::metrics::track::TokenTrack;
        use crate::metrics::{Side, TradeLite};
        use chrono::{Duration, TimeZone, Utc};
        let t0 = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let at = |s: f64| t0 + Duration::milliseconds((s * 1000.0) as i64);

        let c = CompiledRule::compile(&rule(json!({
            "entry": { "m_crowd_window": {
                "window_size_sec": 10,
                "unique_wallets": [{"operator": ">=", "value": 3}]
            } }
        })));
        assert!(c.flow_windows.is_empty(), "a crowd gate opens no flow deque");

        let fold = |track: &mut TokenTrack| {
            for (i, wallet) in [7u64, 7, 8, 9].into_iter().enumerate() {
                track.on_trade(TradeLite {
                    side: Side::Buy,
                    sol: 1.0,
                    price: 1.0,
                    reserve_sol: 100.0,
                    at: at(i as f64),
                    wallet_hash: wallet,
                    ..Default::default()
                });
            }
        };
        let r = &c.entry_reqs[0];

        let mut registered = TokenTrack::new(t0);
        for &w in &c.crowd_windows {
            registered.ensure_crowd_window(w);
        }
        fold(&mut registered);
        assert_eq!(registered.value(r.metric, r.window, r.fingerprint, at(3.0)), 3.0);

        // The same span on the FLOW buffer answers nothing here.
        let mut wrong_buffer = TokenTrack::new(t0);
        for &w in &c.crowd_windows {
            wrong_buffer.ensure_window(w);
        }
        fold(&mut wrong_buffer);
        assert!(wrong_buffer.value(r.metric, r.window, r.fingerprint, at(3.0)).is_nan());
    }

    /// A two-window group must reach the track with BOTH axes, and must register a
    /// buffer for each. Losing the second axis here is silent: the read returns NaN,
    /// the gate never fires, and the rule looks merely strict.
    #[test]
    fn a_two_window_group_carries_and_registers_both_axes() {
        let c = CompiledRule::compile(&rule(json!({
            "entry": { "m_flow_window": {
                "window_size_sec": 60, "slice_size_sec": 3,
                "trade_share": [{"operator": ">=", "value": 7.69}]
            } }
        })));
        assert_eq!(c.entry_reqs.len(), 1);
        assert_eq!(c.entry_reqs[0].metric, MetricId::SliceTradeShare);
        assert_eq!(c.entry_reqs[0].window, Windows::two(WindowSpec::secs(60.0), WindowSpec::secs(3.0)));
        // One buffer per axis, both on the flow side (neither is a price window).
        assert_eq!(c.flow_windows.len(), 2);
        assert!(c.flow_windows.contains(&WindowSpec::secs(60.0)) && c.flow_windows.contains(&WindowSpec::secs(3.0)));
        assert!(c.price_windows.is_empty());
        // The tick horizon covers the LONGER axis — a grid that only reached 3 s
        // would stop ticking while the 60 s reference was still draining.
        assert!(c.clock_horizons.max_window_secs >= 60.0);
    }

    /// The whole path end to end: compile a slice rule, register its windows on a
    /// real track, fold a tape, and read the gate. This is the property the metric
    /// exists for — it is the only reading here that separates a clustered tape from
    /// an evenly spread one carrying identical volume.
    #[test]
    fn a_slice_gate_reads_through_the_track_it_registered() {
        use crate::metrics::track::TokenTrack;
        use crate::metrics::{Side, TradeLite};
        use chrono::{Duration, TimeZone, Utc};
        let t0 = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let at = |s: f64| t0 + Duration::milliseconds((s * 1000.0) as i64);

        let c = CompiledRule::compile(&rule(json!({
            "entry": { "m_flow_window": {
                "window_size_sec": 60, "slice_size_sec": 3,
                "trade_share": [{"operator": ">=", "value": 40}]
            } }
        })));
        let read = |offsets: &[f64], now: f64| {
            let mut track = TokenTrack::new(t0);
            for &w in &c.flow_windows {
                track.ensure_window(w);
            }
            for &o in offsets {
                track.on_trade(TradeLite {
                    side: Side::Buy,
                    sol: 1.0,
                    price: 1.0,
                    reserve_sol: 100.0,
                    at: at(o),
                    ..Default::default()
                });
            }
            let r = &c.entry_reqs[0];
            track.value(r.metric, r.window, r.fingerprint, at(now))
        };
        // Clustered: 6 of 10 trades inside the last 3 s ⇒ 60, gate holds.
        let clustered = [10.0, 20.0, 30.0, 40.0, 57.5, 58.0, 58.5, 59.0, 59.5, 60.0];
        assert_eq!(read(&clustered, 60.0), 60.0);
        // Evenly spread: same count, same SOL, 1 of 10 inside ⇒ 10, gate does not.
        let spread = [6.0, 12.0, 18.0, 24.0, 30.0, 36.0, 42.0, 48.0, 54.0, 60.0];
        assert_eq!(read(&spread, 60.0), 10.0);
    }

    #[test]
    fn time_upper_bound_becomes_mono_kill() {
        let c = CompiledRule::compile(&rule(json!({
            "entry": { "m_state": { "time": [{"operator": "<", "value": 30}] } }
        })));
        assert_eq!(c.mono_kills.len(), 1);
        let k = &c.mono_kills[0];
        assert_eq!(k.metric, MetricId::Time);
        assert_eq!(k.arms.len(), 1);
        let b = k.arms[0].unwrap();
        assert_eq!(b.threshold, 30.0);
        assert!(b.cross_at_ge);
        assert!(!k.permanently_false(29.9));
        assert!(k.permanently_false(30.0));
    }

    #[test]
    fn or_with_open_arm_never_mono_kills() {
        // `time < 30 | time >= 70` — after 30 the first arm dies but the second lives.
        let c = CompiledRule::compile(&rule(json!({
            "entry": { "m_state": { "time": [
                [{"operator": "<", "value": 30}],
                [{"operator": ">=", "value": 70}]
            ] } }
        })));
        assert_eq!(c.mono_kills.len(), 1);
        let k = &c.mono_kills[0];
        assert_eq!(k.arms.len(), 2);
        assert!(k.arms[0].is_some());
        assert!(k.arms[1].is_none());
        assert!(!k.permanently_false(30.0));
        assert!(!k.permanently_false(100.0));
    }

    #[test]
    fn or_of_two_upper_bounds_kills_when_both_crossed() {
        let c = CompiledRule::compile(&rule(json!({
            "entry": { "m_state": { "time": [
                [{"operator": "<", "value": 30}],
                [{"operator": "<", "value": 50}]
            ] } }
        })));
        let k = &c.mono_kills[0];
        assert!(!k.permanently_false(40.0)); // arm2 still live
        assert!(k.permanently_false(50.0));
    }

    #[test]
    fn lower_bound_on_monotonic_metric_is_not_a_mono_bound() {
        let c = CompiledRule::compile(&rule(json!({
            "entry": { "m_state": { "time": [{"operator": ">", "value": 10}] } }
        })));
        assert!(c.mono_kills.is_empty());
    }

    #[test]
    fn non_monotonic_metric_never_produces_a_mono_bound() {
        let c = CompiledRule::compile(&rule(json!({
            "entry": { "m_state": { "liquidity": [{"operator": "<", "value": 5}] } }
        })));
        assert!(c.mono_kills.is_empty());
    }

    #[test]
    fn exit_metrics_or_across_metrics_but_entry_ands() {
        use crate::metrics::{Side, TradeLite};
        use chrono::{Duration, TimeZone, Utc};

        let conds = json!({
            "m_state": {
                "time":      [{"operator": ">", "value": 1000}],
                "liquidity": [{"operator": ">", "value": 999}]
            }
        });
        let compiled = CompiledRule::compile(&rule(json!({
            "entry": conds, "exit": conds, "take_profit": 100
        })));

        let created = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let now = created + Duration::seconds(10);
        let mut track = TokenTrack::new(created);
        track.on_trade(TradeLite { side: Side::Buy, sol: 1.0, price: 1.0, reserve_sol: 2000.0, at: now , ..Default::default() });

        assert!(compiled.exit_metrics_satisfied(&track, now));
        assert!(!compiled.entry_satisfied(&track, now));
        assert!(!compiled.can_enter(&track, now));

        let mut cold = TokenTrack::new(created);
        cold.on_trade(TradeLite { side: Side::Buy, sol: 1.0, price: 1.0, reserve_sol: 5.0, at: now , ..Default::default() });
        assert!(!compiled.exit_metrics_satisfied(&cold, now));
    }

    /// `arm_above_pct` holds the trailing stop off until the position is in profit.
    ///
    /// This is the whole reason the param exists: exit metrics OR across metrics, so
    /// `retrace >= 3 AND pnl >= 2` is otherwise unauthorable, and an unarmed
    /// `retrace` doubles as a hard −3% stop from entry (the peak seeds at the fill).
    /// Measured on omego's own 2,974 episodes, that unarmed trail turns 21% of his
    /// winners into losers — see `docs/plans/strategies/armed-trailing-stop.md`.
    #[test]
    fn arm_above_pct_holds_the_trailing_stop_until_in_profit() {
        use crate::metrics::{Side, TradeLite};
        use chrono::{Duration, TimeZone, Utc};

        let params = |gate: serde_json::Value| {
            let mut pos = serde_json::Map::new();
            pos.insert("retrace".into(), json!([{"operator": ">=", "value": 3}]));
            if !gate.is_null() {
                pos.insert("arm_above_pct".into(), gate);
            }
            json!({ "stop_loss": 20, "exit": { "m_position": pos } })
        };
        let armed = CompiledRule::compile(&rule(params(json!(2))));
        let unarmed = CompiledRule::compile(&rule(params(serde_json::Value::Null)));

        // The gate rides on the trailing req only — never on the desugared stop-loss,
        // which would otherwise be disabled by requiring profit first.
        let trailing = armed.exit_reqs.iter().find(|r| r.metric == MetricId::Retrace);
        assert_eq!(trailing.unwrap().arm_above_pct, Some(2.0));
        let sl = armed.exit_reqs.iter().find(|r| r.origin == ReqOrigin::StopLoss);
        assert_eq!(sl.unwrap().arm_above_pct, None);

        let created = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let now = created + Duration::seconds(10);
        let mut track = TokenTrack::new(created);
        track.on_trade(TradeLite {
            side: Side::Buy, sol: 1.0, price: 1.0, reserve_sol: 60.0, at: now,
            ..Default::default()
        });

        // Entered at 1.0, ran to 1.10, now back at 1.05: retrace 4.5% off the peak,
        // and pnl +5% clears the gate → both rules sell.
        let ctx = PositionCtx {
            entry_price: 1.0, peak_price: 1.10, trough_price: 1.0, entered_at: created,
            armed: true, trail_arm_pct: None,
        };
        track.on_trade(TradeLite {
            side: Side::Sell, sol: 1.0, price: 1.05, reserve_sol: 60.0, at: now,
            ..Default::default()
        });
        assert!(matches!(armed.exit_fired(&track, &ctx, now), Some(ExitReason::Metrics { .. })));
        assert!(matches!(unarmed.exit_fired(&track, &ctx, now), Some(ExitReason::Metrics { .. })));

        // Entered at 1.0 and it only ever fell — the peak IS the fill, so retrace 4%
        // = pnl −4%. The unarmed trail sells at a loss; the armed one holds, leaving
        // the position to the stop-loss.
        let sunk = PositionCtx {
            entry_price: 1.0, peak_price: 1.0, trough_price: 0.96, entered_at: created,
            armed: true, trail_arm_pct: None,
        };
        let mut down = TokenTrack::new(created);
        down.on_trade(TradeLite {
            side: Side::Sell, sol: 1.0, price: 0.96, reserve_sol: 60.0, at: now,
            ..Default::default()
        });
        assert!(matches!(
            unarmed.exit_fired(&down, &sunk, now),
            Some(ExitReason::Metrics { .. })
        ));
        assert_eq!(armed.exit_fired(&down, &sunk, now), None);

        // ...and the stop-loss still fires through the gate at −20%.
        let blown = PositionCtx {
            entry_price: 1.0, peak_price: 1.0, trough_price: 0.75, entered_at: created,
            armed: true, trail_arm_pct: None,
        };
        let mut crash = TokenTrack::new(created);
        crash.on_trade(TradeLite {
            side: Side::Sell, sol: 1.0, price: 0.75, reserve_sol: 60.0, at: now,
            ..Default::default()
        });
        assert_eq!(armed.exit_fired(&crash, &blown, now), Some(ExitReason::StopLoss));
    }

    /// Array-form DNF ANDs `armed` with `retrace` and does **not** skip the trail
    /// when PnL has fallen back under the gate. Object-form singleton retrace still
    /// skips — that is the load-bearing difference: the latch must outlive +10%.
    #[test]
    fn dnf_armed_retrace_fires_after_pnl_falls_under_the_gate() {
        use crate::metrics::{Side, TradeLite};
        use chrono::{Duration, TimeZone, Utc};

        let object = CompiledRule::compile(&rule(json!({
            "exit": { "m_position": {
                "retrace": [{"operator": ">=", "value": 18}],
                "arm_above_pct": 10
            } }
        })));
        let dnf = CompiledRule::compile(&rule(json!({
            "exit": [{ "m_position": {
                "armed": [{"operator": "=", "value": 1}],
                "retrace": [{"operator": ">=", "value": 18}],
                "arm_above_pct": 10
            } }]
        })));
        assert_eq!(object.exit_clauses.len(), 1);
        assert_eq!(object.exit_clauses[0].len(), 1);
        assert_eq!(dnf.exit_clauses.len(), 1);
        assert_eq!(dnf.exit_clauses[0].len(), 2);
        assert_eq!(dnf.trail_arm_pct, Some(10.0));

        let created = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let now = created + Duration::seconds(10);
        // Entered 1.0, peaked 1.20 (latch), now 0.984 → retrace 18%, pnl −1.6%.
        let ctx = PositionCtx {
            entry_price: 1.0,
            peak_price: 1.20,
            trough_price: 0.984,
            entered_at: created,
            armed: true,
            trail_arm_pct: Some(10.0),
        };
        let mut track = TokenTrack::new(created);
        track.on_trade(TradeLite {
            side: Side::Sell, sol: 1.0, price: 0.984, reserve_sol: 60.0, at: now,
            ..Default::default()
        });
        assert_eq!(object.exit_fired(&track, &ctx, now), None, "object-form skips while pnl < gate");
        assert!(matches!(dnf.exit_fired(&track, &ctx, now), Some(ExitReason::Metrics { .. })));
    }

    /// `arm_above_pct: 0` means "arm at break-even", NOT "off" — the two must stay
    /// distinguishable, so an absent param is the only thing that disables the gate.
    #[test]
    fn arm_above_pct_zero_is_a_real_setting_not_the_off_sentinel() {
        let zero = CompiledRule::compile(&rule(json!({
            "exit": { "m_position": {
                "retrace": [{"operator": ">=", "value": 3}], "arm_above_pct": 0
            } }
        })));
        let trailing = zero.exit_reqs.iter().find(|r| r.metric == MetricId::Retrace);
        assert_eq!(trailing.unwrap().arm_above_pct, Some(0.0));

        let absent = CompiledRule::compile(&rule(json!({
            "exit": { "m_position": { "retrace": [{"operator": ">=", "value": 3}] } }
        })));
        let trailing = absent.exit_reqs.iter().find(|r| r.metric == MetricId::Retrace);
        assert_eq!(trailing.unwrap().arm_above_pct, None);
    }

    #[test]
    fn can_enter_refuses_when_exit_metrics_already_hold() {
        use crate::metrics::{Side, TradeLite};
        use chrono::{Duration, TimeZone, Utc};

        // Overlapping bands: entry liquidity > 50, exit liquidity > 40 — both true
        // at reserve 60. Buying would immediately qualify for a metrics exit.
        let compiled = CompiledRule::compile(&rule(json!({
            "entry": { "m_state": { "liquidity": [{"operator": ">", "value": 50}] } },
            "exit":  { "m_state": { "liquidity": [{"operator": ">", "value": 40}] } },
            "take_profit": 100
        })));

        let created = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let now = created + Duration::seconds(1);
        let mut overlap = TokenTrack::new(created);
        overlap.on_trade(TradeLite {
            side: Side::Buy, sol: 1.0, price: 1.0, reserve_sol: 60.0, at: now,
            ..Default::default()
        });
        assert!(compiled.entry_satisfied(&overlap, now));
        assert!(compiled.exit_metrics_satisfied(&overlap, now));
        assert!(!compiled.can_enter(&overlap, now));

        // Liquidity between the bands: entry false, exit true → still no enter.
        let mut exit_only = TokenTrack::new(created);
        exit_only.on_trade(TradeLite {
            side: Side::Buy, sol: 1.0, price: 1.0, reserve_sol: 45.0, at: now,
            ..Default::default()
        });
        assert!(!compiled.entry_satisfied(&exit_only, now));
        assert!(compiled.exit_metrics_satisfied(&exit_only, now));
        assert!(!compiled.can_enter(&exit_only, now));

        // Below both: neither side → no enter.
        let mut cold = TokenTrack::new(created);
        cold.on_trade(TradeLite {
            side: Side::Buy, sol: 1.0, price: 1.0, reserve_sol: 10.0, at: now,
            ..Default::default()
        });
        assert!(!compiled.can_enter(&cold, now));
    }

    #[test]
    fn can_enter_when_entry_holds_and_exit_does_not() {
        use crate::metrics::{Side, TradeLite};
        use chrono::{Duration, TimeZone, Utc};

        // Entry liquidity > 50; exit liquidity < 20 — disjoint.
        let compiled = CompiledRule::compile(&rule(json!({
            "entry": { "m_state": { "liquidity": [{"operator": ">", "value": 50}] } },
            "exit":  { "m_state": { "liquidity": [{"operator": "<", "value": 20}] } },
            "take_profit": 100
        })));

        let created = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let now = created + Duration::seconds(1);
        let mut track = TokenTrack::new(created);
        track.on_trade(TradeLite {
            side: Side::Buy, sol: 1.0, price: 1.0, reserve_sol: 80.0, at: now,
            ..Default::default()
        });
        assert!(compiled.can_enter(&track, now));
    }

    /// The disarm must name the condition the entry was SHORT OF, not the clock.
    /// `time` is the only monotonic metric, so the kill is always the deadline —
    /// reporting it as the reason answers nothing.
    #[test]
    fn unsatisfiable_disarm_names_what_the_entry_was_short_of() {
        use crate::metrics::{Side, TradeLite};
        use chrono::{Duration, TimeZone, Utc};

        let c = CompiledRule::compile(&rule(json!({
            "entry": {
                "m_state": {
                    "time": [{"operator": ">", "value": 10}, {"operator": "<", "value": 50}],
                    "liquidity": [{"operator": ">", "value": 10}, {"operator": "<", "value": 45}]
                },
                "m_flow_window": {
                    "window_size_sec": 60,
                    "gross_flow": [{"operator": ">=", "value": 40}]
                }
            }
        })));

        let created = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let mut track = TokenTrack::new(created);
        for w in &c.flow_windows {
            track.ensure_window(*w);
        }
        // One burst and then silence: liquidity lands inside its band and stays
        // there, gross_flow tops out at half what the entry asks for.
        track.on_trade(TradeLite {
            side: Side::Buy, sol: 20.0, price: 1.0, reserve_sol: 20.0, at: created,
            ..Default::default()
        });

        // Inside the entry window there is nothing to give up on yet.
        assert!(c.entry_unsatisfiable(&track, created + Duration::seconds(30)).is_none());

        let late = created + Duration::seconds(50);
        let killed = c.entry_unsatisfiable(&track, late).expect("time < 50 crossed at 50s");
        assert_eq!(killed.metric, MetricId::Time);
        assert_eq!(killed.threshold, 50.0);

        let b = c.entry_blockers(&track, late, killed);
        // `time` fails by construction at this instant and `liquidity` held the
        // whole way, so exactly one condition is the answer.
        assert_eq!(b.unmet.len(), 1, "only gross_flow blocked entry: {:?}", b.unmet);
        assert_eq!(b.unmet[0].metric, MetricId::GrossFlow);
        assert_eq!(b.unmet[0].window, Windows::secs(60.0));
        assert_eq!(b.unmet[0].value, 20.0);
    }

    /// Every other condition held and the arm still died: the token qualified too
    /// late. An EMPTY blocker set is that finding — a different one from "it never
    /// came close" — so it must not be filled in with the deadline.
    #[test]
    fn ran_out_of_clock_with_nothing_else_unmet_reports_no_blocker() {
        use crate::metrics::{Side, TradeLite};
        use chrono::{Duration, TimeZone, Utc};

        let c = CompiledRule::compile(&rule(json!({
            "entry": {
                "m_state": {
                    "time": [{"operator": "<", "value": 50}],
                    "liquidity": [{"operator": ">", "value": 10}]
                }
            }
        })));
        let created = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let mut track = TokenTrack::new(created);
        track.on_trade(TradeLite {
            side: Side::Buy, sol: 20.0, price: 1.0, reserve_sol: 20.0, at: created,
            ..Default::default()
        });

        let late = created + Duration::seconds(50);
        let killed = c.entry_unsatisfiable(&track, late).expect("crossed");
        assert!(c.entry_blockers(&track, late, killed).unmet.is_empty());
    }

    /// Two upper bounds on one metric: the deadline is the one that crosses LAST,
    /// because that is the crossing the episode actually died on.
    #[test]
    fn binding_bound_is_the_last_to_cross() {
        let c = CompiledRule::compile(&rule(json!({
            "entry": { "m_state": { "time": [
                [{"operator": "<", "value": 20}],
                [{"operator": "<", "value": 40}]
            ] } }
        })));
        let k = &c.mono_kills[0];
        assert!(k.binding_bound(25.0).is_none(), "the 40s arm is still open at 25s");
        assert_eq!(k.binding_bound(40.0).expect("both crossed").threshold, 40.0);
    }

    #[test]
    fn eq_on_time_bounds_at_upper_edge() {
        let c = CompiledRule::compile(&rule(json!({
            "entry": { "m_state": { "time": [{"operator": "=", "value": 20}] } }
        })));
        let b = c.mono_kills[0].arms[0].unwrap();
        assert_eq!(b.threshold, 20.25);
        assert!(!b.cross_at_ge);
        assert!(!b.crossed(20.25));
        assert!(b.crossed(20.26));
    }

    #[test]
    fn scale_out_compiles_stages_and_merges_windows() {
        let c = CompiledRule::compile(&rule(json!({
            "scale_out": [
                { "sell_bps": 7000, "take_profit": 25 },
                {
                    "sell_bps": 2000,
                    "conditions": {
                        "m_flow_window": {
                            "window_size_sec": 5,
                            "net_flow": [{ "operator": "<=", "value": 0 }]
                        }
                    }
                },
                { "conditions": { "m_position": { "held": [{ "operator": ">=", "value": 30 }] } } }
            ]
        })));
        assert_eq!(c.scale_out.len(), 3);
        assert_eq!(c.scale_out[0].sell_bps, Some(7000));
        assert_eq!(c.scale_out[0].reqs.len(), 1);
        assert_eq!(c.scale_out[0].reqs[0].origin, ReqOrigin::TakeProfit);
        assert_eq!(c.scale_out[1].sell_bps, Some(2000));
        assert_eq!(c.scale_out[2].sell_bps, None, "remainder");
        assert!(c.flow_windows.contains(&WindowSpec::secs(5.0)), "stage windows merge into rule");
        assert!(c.exit_reqs.is_empty(), "global side empty — stages alone");
    }
}
