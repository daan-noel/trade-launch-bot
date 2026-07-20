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

use crate::event::{
    DisarmReason, ExitReason, IntentId, LoadedRule, PositionId, RuleId, TradeMode,
};
use crate::fingerprint::FingerprintId;
use crate::metrics::evaluator::{eval, Condition, ConditionExpr, Operator};
use crate::metrics::track::TokenTrack;
use crate::metrics::{group_spec, is_flow_metric, metric_spec, MetricId, MetricKind, Ts};

/// One metric read a rule side needs: the metric, the window it lives in (dynamic
/// metrics only), its `=`-tolerance, and the DNF condition arms. Precomputed so
/// evaluation is a flat loop of `track.value(..)` + `eval(..)` reads.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricReq {
    pub metric: MetricId,
    pub window: Option<f64>,
    /// Fingerprint scope for flow metrics; `None` for token-scoped groups.
    pub fingerprint: Option<FingerprintId>,
    pub tolerance: f64,
    /// DNF: OR of AND-arms.
    pub conds: ConditionExpr,
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
/// distinct windows are listed for [`TokenTrack::ensure_window`], and the entry
/// monotonic kills are precomputed for derived-unsatisfiability disarm.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledRule {
    pub id: RuleId,
    pub fingerprint_id: FingerprintId,
    pub trade_mode: TradeMode,
    pub buy_amount_lamports: u64,
    pub concurrent_cap: u32,
    /// `0` ⇒ unlimited.
    pub max_total: u32,
    pub take_profit: Option<f64>,
    pub stop_loss: Option<f64>,
    /// Empty ⇒ enter on arm (the fingerprint alone is the entry signal).
    pub entry_reqs: Vec<MetricReq>,
    /// Empty ⇒ no metric exit (only TP/SL/dead close).
    pub exit_reqs: Vec<MetricReq>,
    /// Distinct `window_size_sec` values this rule reads (both sides).
    pub windows: SmallVec<[f64; 2]>,
    /// Per entry-metric mono kills (for derived-unsatisfiability disarm).
    pub mono_kills: SmallVec<[MonoMetricKill; 2]>,
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
        let exit_reqs = rule
            .params
            .exit
            .as_ref()
            .map(|s| build_reqs(s, rule.fingerprint_id))
            .unwrap_or_default();

        // Distinct windows across both sides (dynamic metrics only).
        let mut windows: SmallVec<[f64; 2]> = SmallVec::new();
        for r in entry_reqs.iter().chain(exit_reqs.iter()) {
            if let Some(w) = r.window {
                if !windows.contains(&w) {
                    windows.push(w);
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
            max_total: rule.max_total_tokens,
            take_profit: rule.params.take_profit,
            stop_loss: rule.params.stop_loss,
            entry_reqs,
            exit_reqs,
            windows,
            mono_kills,
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
        self.has_exit_metrics() && reqs_any_satisfied(&self.exit_reqs, track, now)
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
/// satisfied metric fires). Empty ⇒ `false` (no reason to exit).
fn reqs_any_satisfied(reqs: &[MetricReq], track: &TokenTrack, now: Ts) -> bool {
    reqs.iter().any(|r| {
        eval(
            &r.conds,
            track.value(r.metric, r.window, r.fingerprint, now),
            r.tolerance,
        )
    })
}

/// Flatten one side's parsed conditions into [`MetricReq`]s. A dynamic group's
/// metrics carry its `window_size_sec`; static groups carry `None`. Flow metrics
/// are scoped to `fingerprint_id`.
fn build_reqs(
    side: &crate::rule_params::SideConditions,
    fingerprint_id: FingerprintId,
) -> Vec<MetricReq> {
    let mut out = Vec::new();
    for (group_id, group) in &side.0 {
        let window = if group_spec(*group_id).kind == MetricKind::Dynamic {
            group.strict_param("window_size_sec")
        } else {
            None
        };
        for (metric_id, conds) in &group.metrics {
            out.push(MetricReq {
                metric: *metric_id,
                window,
                fingerprint: is_flow_metric(*metric_id).then_some(fingerprint_id),
                tolerance: metric_spec(*metric_id).eq_tolerance,
                conds: conds.clone(),
            });
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
    EntryPending { intent: IntentId, position: PositionId, attempts: u32 },
    /// Entry filled; the position is held and evaluating exit. `entry_price` is the
    /// fill price TP/SL measure against.
    Entered { position: PositionId, entry_price: f64 },
    /// A sell is in flight for `intent` (the `attempts`-th try), closing for `reason`.
    ExitPending { position: PositionId, intent: IntentId, reason: ExitReason, attempts: u32 },
    /// Terminal: the position closed (or the token is done forever for this rule).
    Done,
    /// Terminal: disarmed before entry for `reason`.
    Disarmed(DisarmReason),
}

impl ArmState {
    /// Whether this arm still needs the token tracked (non-terminal). A token with
    /// no active arms and no open position can be pruned.
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
    fn windows_deduped_across_sides() {
        let c = CompiledRule::compile(&rule(json!({
            "entry": { "m_time_window": { "window_size_sec": 10, "buy": [{"operator": ">", "value": 1}] } },
            "exit":  { "m_time_window": { "window_size_sec": 10, "sell": [{"operator": ">", "value": 1}] } }
        })));
        assert_eq!(c.windows.as_slice(), &[10.0]);
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

        let mut cold = TokenTrack::new(created);
        cold.on_trade(TradeLite { side: Side::Buy, sol: 1.0, price: 1.0, reserve_sol: 5.0, at: now , ..Default::default() });
        assert!(!compiled.exit_metrics_satisfied(&cold, now));
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
