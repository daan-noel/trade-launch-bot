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
    DisarmReason, ExitReason, IntentId, LoadedRule, PositionId, RuleId, TradeMode,
};
use crate::fingerprint::FingerprintId;
use crate::metrics::evaluator::{eval, first_satisfied_cond, Condition, ConditionExpr, Operator};
use crate::metrics::position::{is_trailing, position_value, trailing_armed, PositionCtx};
use crate::metrics::track::TokenTrack;
use crate::metrics::{
    group_of, group_spec, is_flow_metric, metric_spec, MetricGroupId, MetricId, MetricKind,
    MetricScope, Ts,
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
    pub window: Option<f64>,
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
    pub window: Option<f64>,
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
    pub window: Option<f64>,
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
}

/// Tightest mono upper bound in one AND arm, if any.
fn arm_mono_upper(
    arm: &[Condition],
    half_tol: f64,
    metric: MetricId,
    window: Option<f64>,
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

/// A [`LoadedRule`] pre-chewed for the hot path. Metric reads are flattened, the
/// distinct windows are listed (split flow vs price) for
/// [`TokenTrack::ensure_window`] / [`TokenTrack::ensure_price_window`], and the
/// entry monotonic kills are precomputed for derived-unsatisfiability disarm.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledRule {
    pub id: RuleId,
    pub fingerprint_id: FingerprintId,
    pub trade_mode: TradeMode,
    pub buy_amount_lamports: u64,
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
    /// The held-side exit conditions the fold evaluates via [`exit_fired`](Self::exit_fired):
    /// desugared TP/SL (`pnl`, prepended) followed by authored exit metrics
    /// (token- or position-scoped). Empty ⇒ only a dead-token verdict closes.
    pub exit_reqs: Vec<MetricReq>,
    /// Distinct **flow** `window_size_sec` values this rule reads across both sides
    /// (`m_flow_window` + `m_flow_split_window`) — drive `ensure_window` / `ensure_flow`.
    pub flow_windows: SmallVec<[f64; 2]>,
    /// Distinct **price** `window_size_sec` values (`m_price_window`) — drive
    /// `ensure_price_window`. Split from [`flow_windows`](Self::flow_windows) so a
    /// rule pays only for the deques its metrics actually read.
    pub price_windows: SmallVec<[f64; 2]>,
    /// Per entry-metric mono kills (for derived-unsatisfiability disarm).
    pub mono_kills: SmallVec<[MonoMetricKill; 2]>,
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
        let mut exit_reqs = rule
            .params
            .exit
            .as_ref()
            .map(|s| build_reqs(s, rule.fingerprint_id))
            .unwrap_or_default();

        // DESUGAR the `take_profit` / `stop_loss` sugar into position-scoped `pnl`
        // exit reqs — one exit-evaluation path (`pnl >= tp` / `pnl <= -sl`) shared
        // with authored `m_position.pnl` conditions, so the two can never drift. The
        // top-level fields stay (the sweep + FE + stored rules read them, and this is
        // outcome-identical to the old `entry_price * (1 ± x/100)` branch), but the
        // fold now decides every price-based exit through the one `exit_fired` loop.
        // PREPENDED so a catastrophe stop still ranks above softer metric exits
        // (preserving the old `Dead > StopLoss > TakeProfit > Metrics` order:
        // SL then TP then authored). Origin-tagged so the persisted reason still reads
        // `TakeProfit` / `StopLoss`, not `pnl <op> value`.
        let mut desugared: Vec<MetricReq> = Vec::new();
        if let Some(sl) = rule.params.stop_loss {
            desugared.push(pnl_req(Operator::Lte, -sl, ReqOrigin::StopLoss));
        }
        if let Some(tp) = rule.params.take_profit {
            desugared.push(pnl_req(Operator::Gte, tp, ReqOrigin::TakeProfit));
        }
        if !desugared.is_empty() {
            desugared.append(&mut exit_reqs);
            exit_reqs = desugared;
        }

        // Distinct windows across both sides (dynamic metrics only), split by which
        // buffer they drive: price-extrema deques vs flow ring buffers.
        let mut flow_windows: SmallVec<[f64; 2]> = SmallVec::new();
        let mut price_windows: SmallVec<[f64; 2]> = SmallVec::new();
        for r in entry_reqs.iter().chain(exit_reqs.iter()) {
            if let Some(w) = r.window {
                let bucket = if group_of(r.metric).id == MetricGroupId::PriceWindow {
                    &mut price_windows
                } else {
                    &mut flow_windows
                };
                if !bucket.contains(&w) {
                    bucket.push(w);
                }
            }
        }

        // Monotonic entry kills — per metric, OR of arm upper bounds.
        let mut mono_kills: SmallVec<[MonoMetricKill; 2]> = SmallVec::new();
        for r in &entry_reqs {
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
            concurrent_cap: rule.concurrent_cap(),
            max_total: rule.total_cap(),
            take_profit: rule.params.take_profit,
            stop_loss: rule.params.stop_loss,
            entry_reqs,
            exit_reqs,
            flow_windows,
            price_windows,
            mono_kills,
            reentry: rule.params.reentry,
            exclusive: rule.params.exclusive,
            priority: rule.params.priority,
        }
    }

    /// Whether arming alone is the entry signal (no entry conditions authored).
    pub fn enter_on_arm(&self) -> bool {
        self.entry_reqs.is_empty()
    }

    /// Whether every entry condition holds at `now` (AND across all metrics). For an
    /// `enter_on_arm` rule this is vacuously `true`.
    pub fn entry_satisfied(&self, track: &TokenTrack, now: Ts) -> bool {
        reqs_satisfied(&self.entry_reqs, track, now)
    }

    /// Whether this rule has metric exit conditions at all.
    pub fn has_exit_metrics(&self) -> bool {
        !self.exit_reqs.is_empty()
    }

    /// Whether the exit fires at `now`. Exit metrics **OR** across metrics (any one
    /// authored reason to bail fires the sell) — asymmetric with entry's AND, and
    /// consistent with TP/SL/dead, which already OR alongside this. Within a single
    /// metric its condition expr is DNF (`,` AND / `|` OR). `false` when the rule
    /// authored no exit metrics — the caller falls back to TP/SL/dead only.
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
        reqs_first_fired(&self.exit_reqs, track, now)
    }

    /// The **held-side** exit decision: the first exit req that fires at `now`, as a
    /// labelled [`ExitReason`]. The one exit-evaluation path (Dead is decided before
    /// this by the fold) — it walks `exit_reqs` in order (desugared TP/SL prepended),
    /// reading position-scoped reqs from `ctx` and token-scoped reqs from the track,
    /// and maps a fired req's [`ReqOrigin`] to its reason (TP/SL keep their labels;
    /// authored metrics stamp [`ExitReason::Metrics`]).
    pub fn exit_fired(
        &self,
        track: &TokenTrack,
        ctx: &PositionCtx,
        now: Ts,
    ) -> Option<ExitReason> {
        let price = track.current_price();
        for r in &self.exit_reqs {
            // A trailing stop held off until the position is `arm_above_pct` in
            // profit. Only ever set on trailing reqs, so TP/SL and every other exit
            // metric are untouched.
            if !trailing_armed(r.arm_above_pct, ctx, price) {
                continue;
            }
            let reading = if r.position_scoped {
                position_value(r.metric, ctx, price, now)
            } else {
                track.value(r.metric, r.window, r.fingerprint, now)
            };
            if let Some(c) = first_satisfied_cond(&r.conds, reading, r.tolerance) {
                return Some(match r.origin {
                    ReqOrigin::TakeProfit => ExitReason::TakeProfit,
                    ReqOrigin::StopLoss => ExitReason::StopLoss,
                    ReqOrigin::Authored => ExitReason::Metrics {
                        metric: r.metric,
                        operator: c.operator,
                        value: c.value,
                    },
                });
            }
        }
        None
    }

    /// Whether the armed side may submit a buy at `now`.
    ///
    /// Entry conditions must hold, and exit metrics (if any) must **not** already
    /// hold. Level-triggered exit OR would otherwise sell on the next event after
    /// fill — a worthless round-trip when params overlap. TP/SL are not part of
    /// this gate (they need an entry price).
    pub fn can_enter(&self, track: &TokenTrack, now: Ts) -> bool {
        self.entry_satisfied(track, now) && !self.exit_metrics_satisfied(track, now)
    }

    /// Whether a monotonic entry metric is permanently unsatisfiable at `now` —
    /// the entry can never re-satisfy, so the arm should disarm
    /// ([`DisarmReason::Unsatisfiable`]).
    pub fn entry_unsatisfiable(&self, track: &TokenTrack, now: Ts) -> bool {
        self.mono_kills.iter().any(|k| {
            k.permanently_false(track.value(k.metric, k.window, k.fingerprint, now))
        })
    }
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

/// OR across metric reads in `reqs` at `now` (the **exit** combinator — any one
/// satisfied metric fires). Returns that metric + first satisfied condition.
fn reqs_first_fired(
    reqs: &[MetricReq],
    track: &TokenTrack,
    now: Ts,
) -> Option<(MetricId, Operator, f64)> {
    for r in reqs {
        let reading = track.value(r.metric, r.window, r.fingerprint, now);
        if let Some(c) = first_satisfied_cond(&r.conds, reading, r.tolerance) {
            return Some((r.metric, c.operator, c.value));
        }
    }
    None
}

/// A desugared TP/SL exit req: a single `m_position.pnl <op> value` condition,
/// tagged with the ladder origin so its exit reason stays `TakeProfit`/`StopLoss`.
fn pnl_req(op: Operator, value: f64, origin: ReqOrigin) -> MetricReq {
    MetricReq {
        metric: MetricId::Pnl,
        window: None,
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
            let window = if is_dynamic {
                group.strict_param("window_size_sec")
            } else {
                None
            };
            // Attached below to this instance's trailing metrics only.
            let arm_above = group.strict_param("arm_above_pct");
            for (metric_id, conds) in &group.metrics {
                out.push(MetricReq {
                    metric: *metric_id,
                    window,
                    fingerprint: is_flow_metric(*metric_id).then_some(fingerprint_id),
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
    /// Entry filled; the position is held and evaluating exit. `entry_price` is the
    /// fill price `pnl` measures against, `entered_at` the fill time (`held`),
    /// `peak_price` the highest price since entry (`retrace`), and `trough_price`
    /// the lowest (`bounce`) — both folded forward each event into the
    /// [`PositionCtx`] the position-scoped exit metrics read.
    Entered {
        position: PositionId,
        entry_price: f64,
        entered_at: Ts,
        peak_price: f64,
        trough_price: f64,
    },
    /// A sell is in flight for `intent` (the `attempts`-th try), closing for `reason`.
    ExitPending { position: PositionId, intent: IntentId, reason: ExitReason, attempts: u32 },
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
}

#[cfg(test)]
mod tests {
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
        }
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
        assert_eq!(c.flow_windows.as_slice(), &[10.0]);
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
        assert_eq!(gross.window, Some(30.0));
        assert_eq!(net.window, Some(2.0));
        // Both distinct windows collected for ensure_window (order = req order).
        assert_eq!(c.flow_windows.len(), 2);
        assert!(c.flow_windows.contains(&30.0) && c.flow_windows.contains(&2.0));
        assert!(c.price_windows.is_empty());
    }

    #[test]
    fn time_upper_bound_becomes_mono_kill() {
        let c = CompiledRule::compile(&rule(json!({
            "entry": { "m_snapshot": { "time": [{"operator": "<", "value": 30}] } }
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
            "entry": { "m_snapshot": { "time": [
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
            "entry": { "m_snapshot": { "time": [
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
            "entry": { "m_snapshot": { "time": [{"operator": ">", "value": 10}] } }
        })));
        assert!(c.mono_kills.is_empty());
    }

    #[test]
    fn non_monotonic_metric_never_produces_a_mono_bound() {
        let c = CompiledRule::compile(&rule(json!({
            "entry": { "m_snapshot": { "liquidity": [{"operator": "<", "value": 5}] } }
        })));
        assert!(c.mono_kills.is_empty());
    }

    #[test]
    fn exit_metrics_or_across_metrics_but_entry_ands() {
        use crate::metrics::{Side, TradeLite};
        use chrono::{Duration, TimeZone, Utc};

        let conds = json!({
            "m_snapshot": {
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
    /// winners into losers — see `docs/roadmap/flow-scalper-build-plan.md`.
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
        };
        let mut crash = TokenTrack::new(created);
        crash.on_trade(TradeLite {
            side: Side::Sell, sol: 1.0, price: 0.75, reserve_sol: 60.0, at: now,
            ..Default::default()
        });
        assert_eq!(armed.exit_fired(&crash, &blown, now), Some(ExitReason::StopLoss));
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
            "entry": { "m_snapshot": { "liquidity": [{"operator": ">", "value": 50}] } },
            "exit":  { "m_snapshot": { "liquidity": [{"operator": ">", "value": 40}] } },
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
            "entry": { "m_snapshot": { "liquidity": [{"operator": ">", "value": 50}] } },
            "exit":  { "m_snapshot": { "liquidity": [{"operator": "<", "value": 20}] } },
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

    #[test]
    fn eq_on_time_bounds_at_upper_edge() {
        let c = CompiledRule::compile(&rule(json!({
            "entry": { "m_snapshot": { "time": [{"operator": "=", "value": 20}] } }
        })));
        let b = c.mono_kills[0].arms[0].unwrap();
        assert_eq!(b.threshold, 20.25);
        assert!(!b.cross_at_ge);
        assert!(!b.crossed(20.25));
        assert!(b.crossed(20.26));
    }
}
