//! Arming state + the compiled form of a rule.
//!
//! [`ArmState`] is the per-(token, rule) lifecycle the fold walks:
//! `PendingFirstSlot → Armed → EntryPending → Entered → ExitPending → End/…` with
//! `Disarmed`/`Done` terminals. [`CompiledRule`] is a [`LoadedRule`] pre-chewed at
//! `RulesReloaded` into exactly what the hot path needs — a flat list of metric
//! reads per side and the derived monotonic bounds that let a hopeless entry
//! disarm itself (plan §2.2, §3.2) — so no JSON/param walking ever happens per
//! event.

use smallvec::SmallVec;

use crate::event::{
    DisarmReason, ExitReason, IntentId, LoadedRule, PositionId, RuleId, TradeMode,
};
use crate::fingerprint::FingerprintId;
use crate::metrics::evaluator::{eval, Condition, Operator};
use crate::metrics::track::TokenTrack;
use crate::metrics::{group_spec, metric_spec, MetricId, MetricKind, Ts};

/// One metric read a rule side needs: the metric, the window it lives in (dynamic
/// metrics only), its `=`-tolerance, and the condition list to AND. Precomputed so
/// evaluation is a flat loop of `track.value(..)` + `eval(..)` reads.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricReq {
    pub metric: MetricId,
    pub window: Option<f64>,
    pub tolerance: f64,
    pub conds: Vec<Condition>,
}

/// A derived monotonic upper bound from an **entry** condition: because a monotonic
/// metric (only `time` today) never decreases, once its value crosses this
/// threshold the condition can never re-satisfy — so the whole entry (all
/// conditions AND) is permanently unsatisfiable and the arm disarms.
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
    /// upper bound (so the entry can never again be satisfied). Non-finite ⇒ not
    /// crossed. Public so the sweep scan can replicate the fold's derived-disarm.
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

/// A [`LoadedRule`] pre-chewed for the hot path. Metric reads are flattened, the
/// distinct windows are listed for [`TokenTrack::ensure_window`], and the entry
/// monotonic bounds are precomputed for derived-unsatisfiability disarm.
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
    /// Entry monotonic upper bounds (for derived-unsatisfiability disarm).
    pub mono_bounds: SmallVec<[MonoBound; 2]>,
}

impl CompiledRule {
    /// Pre-chew a loaded rule. Its `params` are already parsed + validated, so this
    /// is a pure structural walk (no failure path).
    pub fn compile(rule: &LoadedRule) -> Self {
        let entry_reqs =
            rule.params.entry.as_ref().map(build_reqs).unwrap_or_default();
        let exit_reqs = rule.params.exit.as_ref().map(build_reqs).unwrap_or_default();

        // Distinct windows across both sides (dynamic metrics only).
        let mut windows: SmallVec<[f64; 2]> = SmallVec::new();
        for r in entry_reqs.iter().chain(exit_reqs.iter()) {
            if let Some(w) = r.window {
                if !windows.contains(&w) {
                    windows.push(w);
                }
            }
        }

        // Monotonic entry bounds — every upper bound any entry condition places on a
        // monotonic metric. Crossing any one makes the AND-ed entry hopeless.
        let mut mono_bounds: SmallVec<[MonoBound; 2]> = SmallVec::new();
        for r in &entry_reqs {
            if !metric_spec(r.metric).monotonic {
                continue;
            }
            let half = r.tolerance / 2.0;
            for c in &r.conds {
                let bound = match c.operator {
                    Operator::Lt => Some((c.value, true)),
                    Operator::Lte => Some((c.value, false)),
                    // `= v` holds only up to v + tol/2; past that a rising metric is done.
                    Operator::Eq => Some((c.value + half, false)),
                    Operator::Gt | Operator::Gte | Operator::Ne => None,
                };
                if let Some((threshold, cross_at_ge)) = bound {
                    mono_bounds.push(MonoBound {
                        metric: r.metric,
                        window: r.window,
                        threshold,
                        cross_at_ge,
                    });
                }
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
            mono_bounds,
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

    /// Whether every exit metric condition holds at `now` (AND). `false` when the
    /// rule authored no exit metrics — the caller falls back to TP/SL/dead only.
    pub fn exit_metrics_satisfied(&self, track: &TokenTrack, now: Ts) -> bool {
        self.has_exit_metrics() && reqs_satisfied(&self.exit_reqs, track, now)
    }

    /// Whether a monotonic entry bound is permanently crossed at `now` — the entry
    /// can never re-satisfy, so the arm should disarm ([`DisarmReason::Unsatisfiable`]).
    pub fn entry_unsatisfiable(&self, track: &TokenTrack, now: Ts) -> bool {
        self.mono_bounds.iter().any(|b| b.crossed(track.value(b.metric, b.window, now)))
    }
}

/// AND every metric read in `reqs` at `now`. Empty ⇒ `true` (vacuous).
fn reqs_satisfied(reqs: &[MetricReq], track: &TokenTrack, now: Ts) -> bool {
    reqs.iter().all(|r| eval(&r.conds, track.value(r.metric, r.window, now), r.tolerance))
}

/// Flatten one side's parsed conditions into [`MetricReq`]s. A dynamic group's
/// metrics carry its `window_size_sec`; static groups carry `None`.
fn build_reqs(side: &crate::rule_params::SideConditions) -> Vec<MetricReq> {
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
        assert!(c.mono_bounds.is_empty());
    }

    #[test]
    fn windows_deduped_across_sides() {
        // Both sides read the same 10 s window → one distinct window.
        let c = CompiledRule::compile(&rule(json!({
            "entry": { "m_time_window": { "window_size_sec": 10, "buy": [{"operator": ">", "value": 1}] } },
            "exit":  { "m_time_window": { "window_size_sec": 10, "sell": [{"operator": ">", "value": 1}] } }
        })));
        assert_eq!(c.windows.as_slice(), &[10.0]);
    }

    #[test]
    fn time_upper_bound_becomes_mono_bound() {
        // entry time < 30 → a monotonic bound crossed at value >= 30.
        let c = CompiledRule::compile(&rule(json!({
            "entry": { "m_snapshot": { "time": [{"operator": "<", "value": 30}] } }
        })));
        assert_eq!(c.mono_bounds.len(), 1);
        let b = c.mono_bounds[0];
        assert_eq!(b.metric, MetricId::Time);
        assert_eq!(b.threshold, 30.0);
        assert!(b.cross_at_ge);
        assert!(!b.crossed(29.9));
        assert!(b.crossed(30.0));
    }

    #[test]
    fn lower_bound_on_monotonic_metric_is_not_a_mono_bound() {
        // `time > 10` never becomes unsatisfiable on a rising clock.
        let c = CompiledRule::compile(&rule(json!({
            "entry": { "m_snapshot": { "time": [{"operator": ">", "value": 10}] } }
        })));
        assert!(c.mono_bounds.is_empty());
    }

    #[test]
    fn non_monotonic_metric_never_produces_a_mono_bound() {
        // liquidity has an upper bound here but is not monotonic → no derived disarm.
        let c = CompiledRule::compile(&rule(json!({
            "entry": { "m_snapshot": { "liquidity": [{"operator": "<", "value": 5}] } }
        })));
        assert!(c.mono_bounds.is_empty());
    }

    #[test]
    fn eq_on_time_bounds_at_upper_edge() {
        // time = 20 (tol 0.5) is done once time passes 20.25.
        let c = CompiledRule::compile(&rule(json!({
            "entry": { "m_snapshot": { "time": [{"operator": "=", "value": 20}] } }
        })));
        let b = c.mono_bounds[0];
        assert_eq!(b.threshold, 20.25);
        assert!(!b.cross_at_ge);
        assert!(!b.crossed(20.25));
        assert!(b.crossed(20.26));
    }
}
