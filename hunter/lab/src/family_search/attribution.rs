//! Per-alarm attribution (charter D4) — which authored exit term did the work.
//!
//! Everything needed already exists and nothing was aggregated: `ExitReason::Metrics`
//! carries `{ metric, operator, value, window }`, the sweep resolves an authored slot
//! per exit req at **bind time**, and [`TokenOutcome`] already carries both. This
//! rolls them up. Deletion-ablation costs one re-run per term and is replaced by one
//! run — and attribution is the hard prerequisite for partial exits, since a ladder
//! cannot fire on a signal it cannot name.
//!
//! Count alone is misleading: a term that fires 200× for −0.4◎ and one that fires 20×
//! for +1.1◎ read the same. Every row therefore carries `Σpnl_sol` **and**
//! `Σentry_sol`, so the percentage is money-over-capital — the one PnL % definition.

use hunter_engine::event::format_metric_exit_label;
use trading_core::strategies::kernel::{weighted_return_pct, ExitCode};

use crate::sweep::strategy::{TokenOutcome, N_EXIT_METRIC_SLOTS};

/// One authored exit term's share of the outcome.
#[derive(Clone, Debug, PartialEq)]
pub struct AlarmRow {
    /// 0-based position among the rule's OWN authored exit reqs — the same slot the
    /// sweep's `n_exit_metrics_by_slot` buckets on.
    pub slot: u8,
    /// `metric[(Ws)] op value`, through the label SSOT. `None` only if every outcome
    /// in the slot arrived without its condition triple (it never should). The window
    /// rides on the name because a dynamic group and its lifetime twin share
    /// `metric.name()` — without it a 2s burst reads as the lifetime term.
    pub label: Option<String>,
    /// Positions this term closed.
    pub n: u64,
    /// Realized SOL over those positions.
    pub pnl_sol: f64,
    /// Capital those positions committed — the percentage's denominator.
    pub entry_sol: f64,
}

impl AlarmRow {
    /// Money over capital, the one PnL % definition. Never a mean of per-trade
    /// percents and never a count share.
    pub fn pnl_pct(&self) -> f64 {
        weighted_return_pct(self.pnl_sol, self.entry_sol)
    }
}

/// The per-alarm rollup for one (rule, cohort).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Attribution {
    /// Occupied slots only, ascending. A rule's unfired terms simply do not appear.
    pub by_slot: Vec<AlarmRow>,
    /// Closes that were **not** authored-metric exits (TP/SL, timeout, `Dead`,
    /// `End`). Reported beside the slots so the counts visibly account for the whole
    /// closed population instead of the alarms silently covering part of it.
    pub n_other: u64,
    pub other_pnl_sol: f64,
    pub other_entry_sol: f64,
}

impl Attribution {
    /// Every position the rollup saw close.
    pub fn n_closed(&self) -> u64 {
        self.by_slot.iter().map(|r| r.n).sum::<u64>() + self.n_other
    }
}

/// Streaming rollup: one pass over outcomes already in hand, O(1) state.
///
/// `buy_amount_sol` is the run's notional — every position commits exactly it, so
/// `Σentry_sol` is `n × buy`. It comes from the **request**, never from an incumbent
/// rule (charter D5).
#[derive(Clone, Debug)]
pub struct AttributionAcc {
    buy_amount_sol: f64,
    slots: [SlotAcc; N_EXIT_METRIC_SLOTS],
    n_other: u64,
    other_pnl_sol: f64,
}

#[derive(Clone, Copy, Debug, Default)]
struct SlotAcc {
    n: u64,
    pnl_sol: f64,
    label: Option<LabelKey>,
}

/// The condition triple an outcome stamps, kept `Copy` so the accumulator allocates
/// nothing per position — the label string is formatted once, at `finish`.
#[derive(Clone, Copy, Debug)]
struct LabelKey {
    metric: hunter_engine::metrics::MetricId,
    operator: hunter_engine::metrics::evaluator::Operator,
    value: f64,
    window: Option<f64>,
}

impl AttributionAcc {
    pub fn new(buy_amount_sol: f64) -> Self {
        Self {
            buy_amount_sol,
            slots: [SlotAcc::default(); N_EXIT_METRIC_SLOTS],
            n_other: 0,
            other_pnl_sol: 0.0,
        }
    }

    /// Fold one outcome. Bucketing mirrors
    /// [`ComboAgg::record`](crate::sweep::aggregate::ComboAgg::record) exactly —
    /// `ExitCode::Metrics` **and** a resolved slot, index capped at
    /// `N_EXIT_METRIC_SLOTS - 1` — so the two counts cannot drift.
    pub fn record(&mut self, o: &TokenOutcome) {
        if !o.fired || o.exit == ExitCode::Open || o.exit == ExitCode::NoEntry {
            return;
        }
        let slot = (o.exit == ExitCode::Metrics).then_some(o.exit_metric_slot).flatten();
        let Some(slot) = slot else {
            self.n_other += 1;
            self.other_pnl_sol += o.pnl_sol as f64;
            return;
        };
        let idx = (slot as usize).min(N_EXIT_METRIC_SLOTS - 1);
        let s = &mut self.slots[idx];
        s.n += 1;
        s.pnl_sol += o.pnl_sol as f64;
        if s.label.is_none() {
            if let (Some(metric), Some(operator), Some(value)) =
                (o.exit_metric, o.exit_operator, o.exit_metric_value)
            {
                s.label = Some(LabelKey { metric, operator, value, window: o.exit_metric_window });
            }
        }
    }

    pub fn finish(self) -> Attribution {
        let by_slot = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.n > 0)
            .map(|(i, s)| AlarmRow {
                slot: i as u8,
                label: s
                    .label
                    .map(|l| format_metric_exit_label(l.metric, l.operator, l.value, l.window)),
                n: s.n,
                pnl_sol: s.pnl_sol,
                entry_sol: s.n as f64 * self.buy_amount_sol,
            })
            .collect();
        Attribution {
            by_slot,
            n_other: self.n_other,
            other_pnl_sol: self.other_pnl_sol,
            other_entry_sol: self.n_other as f64 * self.buy_amount_sol,
        }
    }
}

/// [`AttributionAcc`] over a whole cohort's outcomes.
pub fn rollup<'a>(
    outcomes: impl IntoIterator<Item = &'a TokenOutcome>,
    buy_amount_sol: f64,
) -> Attribution {
    let mut acc = AttributionAcc::new(buy_amount_sol);
    for o in outcomes {
        acc.record(o);
    }
    acc.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::family_search::fixtures::metric_exit;
    use crate::sweep::aggregate::ComboAgg;
    use hunter_engine::metrics::evaluator::Operator;
    use hunter_engine::metrics::MetricId;

    /// The SSOT duplication guard required by `../../CLAUDE.md`: this rollup and the
    /// sweep's `n_exit_metrics_by_slot` are two implementations of one fact, so a
    /// shared fixture pins them equal. No DB.
    #[test]
    fn per_slot_counts_equal_the_sweeps_breakdown() {
        let outs = vec![
            metric_exit(0, MetricId::Stall, Operator::Gte, 30.0, None, 0.10),
            metric_exit(0, MetricId::Stall, Operator::Gte, 30.0, None, -0.02),
            metric_exit(1, MetricId::GrossFlow, Operator::Lt, 15.0, Some(10.0), 0.30),
            // Slot beyond the fixed array — both readers clamp it to the last bucket.
            metric_exit(
                N_EXIT_METRIC_SLOTS as u8 + 4,
                MetricId::WinNonvolBuy,
                Operator::Gte,
                1.6,
                Some(2.0),
                0.05,
            ),
            // Not an authored-metric exit: excluded from every slot by both readers.
            crate::family_search::fixtures::tp_exit(0.4),
            TokenOutcome::no_entry(),
        ];

        let mine = rollup(&outs, 0.01);

        let mut agg = ComboAgg::default();
        for o in &outs {
            agg.record(o);
        }
        let theirs = agg.finalize(0).n_exit_metrics_by_slot;

        for (slot, &theirs_n) in theirs.iter().enumerate() {
            let want = theirs_n as u64;
            let got = mine.by_slot.iter().find(|r| r.slot as usize == slot).map_or(0, |r| r.n);
            assert_eq!(got, want, "slot {slot}");
        }
        // The whole closed population is accounted for, not just the alarms.
        assert_eq!(mine.n_other, 1, "the TP close is 'other', never fabricated into a slot");
        assert_eq!(mine.n_closed(), 5);
    }

    #[test]
    fn a_percentage_is_money_over_capital_not_a_count_share() {
        // 200 fires for −0.4◎ total vs 20 fires for +1.1◎ — identical by count,
        // opposite by money. Buy size 0.01◎.
        let mut outs: Vec<TokenOutcome> = Vec::new();
        for _ in 0..200 {
            outs.push(metric_exit(0, MetricId::Retrace, Operator::Gte, 36.0, None, -0.002));
        }
        for _ in 0..20 {
            outs.push(metric_exit(1, MetricId::Stall, Operator::Gte, 30.0, None, 0.055));
        }
        let a = rollup(&outs, 0.01);
        let loud = &a.by_slot[0];
        let quiet = &a.by_slot[1];

        assert_eq!(loud.n, 200);
        assert_eq!(quiet.n, 20);
        // Tolerances are f32-wide: `TokenOutcome::pnl_sol` is f32 by design (the hot
        // loop keeps the outcome register-friendly), so 200 accumulated legs carry
        // f32 rounding into the f64 sum.
        assert!((loud.pnl_sol + 0.4).abs() < 1e-5, "{}", loud.pnl_sol);
        assert!((quiet.pnl_sol - 1.1).abs() < 1e-5, "{}", quiet.pnl_sol);
        // Capital committed: 200 × 0.01 vs 20 × 0.01.
        assert!((loud.entry_sol - 2.0).abs() < 1e-9);
        assert!((quiet.entry_sol - 0.2).abs() < 1e-9);
        // −20% against +550% — the ranking a count-only table hides entirely.
        assert!((loud.pnl_pct() + 20.0).abs() < 1e-3, "{}", loud.pnl_pct());
        assert!((quiet.pnl_pct() - 550.0).abs() < 1e-3, "{}", quiet.pnl_pct());
    }

    /// A dynamic group and its lifetime twin share `metric.name()`, so a label that
    /// omits the window makes a 2s burst term read as the lifetime one — the two
    /// occupy distinct slots and must read distinctly.
    #[test]
    fn same_metric_different_windows_are_distinct_labelled_slots() {
        let outs = vec![
            metric_exit(0, MetricId::NonvolBuy, Operator::Gte, 0.9, None, 0.01),
            metric_exit(1, MetricId::WinNonvolBuy, Operator::Gte, 0.9, Some(2.0), 0.02),
        ];
        let a = rollup(&outs, 0.01);
        assert_eq!(a.by_slot.len(), 2);
        assert_eq!(a.by_slot[0].label.as_deref(), Some("nonvol_buy >= 0.9"));
        assert_eq!(a.by_slot[1].label.as_deref(), Some("nonvol_buy(2s) >= 0.9"));
        assert_ne!(a.by_slot[0].label, a.by_slot[1].label);
    }
}
