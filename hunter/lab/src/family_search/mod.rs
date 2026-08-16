//! Family search — a **from-scratch** lab job that finds, for one launch style, the
//! metric combination that works on both sides (entry and exit) across a fingerprint's
//! sibling family.
//!
//! Sibling of [rule search](crate::rule_search), never a rewrite of it: nothing here
//! modifies that module, its handler, or its report, and every change to shared sweep
//! code is additive — a new opt-in field, a new function, a widened visibility.
//!
//! The load-bearing constraint is charter D5: **the result depends on no existing
//! rule.** Delete every row in the `rules` table and the output is identical. Buy
//! size, caps, fill, cost and the copycat setting all come from the request; a
//! promoted rule may not supply any of them, because cost is U-shaped under
//! `pumpfun_impact` and the caps change which tokens are entered at all.
//!
//! **Two tiers, on purpose.** The fit stage stops at the archive fold
//! ([`score_cohort`]) — it only needs a ranking, and that is what
//! `score_combos` produces while walking each token's trades once. The authority pass
//! ([`authority`]) runs the full `run_replay` on the **target cohort and the finalists
//! only**. This is the existing two-tier design, not a new approximation.
//!
//! **Two corpora resident, never six.** The target cohort stays loaded (the fit
//! ranking, the level, the capture, the attribution and the narrow re-check all read
//! it); the fit siblings iterate one at a time. The load phase is the RAM spike, and
//! six concurrent corpora is how a run OOMs.
//!
//! Charter: [family-search.md] · plan: [family-search-plan.md].
//!
//! [family-search.md]: ../../docs/roadmap/family-search.md
//! [family-search-plan.md]: ../../docs/roadmap/family-search-plan.md

pub mod attribution;
pub mod dto;
pub mod family;
pub mod gates;
pub mod generator;
pub mod oracle;
pub mod report;
pub mod score;

#[cfg(test)]
pub mod fixtures;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use hunter_engine::arm::CompiledRule;
use hunter_engine::event::{ExitReason, LoadedRule};
use hunter_engine::fingerprint::{Fingerprint as EngineFingerprint, FingerprintId};
use hunter_engine::metrics::evaluator::Operator;
use hunter_engine::metrics::flow_split::FlowPatterns;
use hunter_engine::metrics::MetricId;
use hunter_engine::rule_params::RuleParams;
use trading_core::strategies::kernel::{CostModel, ExitCode};
use trading_core::strategies::paper_fill::FillModel;

use crate::rule_search::cuts::build_cut_table;
use crate::rule_search::generator::{clause_label, GeneratedCombo};
use crate::rule_search::scorer::{loaded_from_params, score_combos, to_replay_tokens, ScoreConfig};
use crate::strategies::replay::{run_replay, PositionOutcome, ReplayConfig};
use crate::sweep::corpus::CorpusToken;
use crate::sweep::generic::strategy::exit_metric_labels;
use crate::sweep::generic::Pricing;
use crate::sweep::progress::SweepObserver;
use crate::sweep::strategy::TokenOutcome;

use generator::{Candidate, GeneratorConfig, QuotaOutcome};
use oracle::CaptureAcc;
use score::CohortScore;

/// One authored exit req's bind-time label: `(metric, operator, value, window, slot)`.
/// `None` for a desugared TP/SL req, which occupies no authored slot.
pub type ExitSlotLabel = Option<(MetricId, Operator, f64, Option<f64>, u8)>;

/// One cohort's fit-tier output: per-candidate scores, per-candidate admit rate, and
/// the ungated control's return when it was scored alongside.
pub type CohortScored = (Vec<CohortScore>, Vec<f64>, Option<f64>);

/// Everything a run reads, all of it from the request (D5). No field here may be
/// populated from a `rules` row.
#[derive(Clone)]
pub struct RunConfig {
    pub pricing: Pricing,
    /// Deadness "now", frozen once at session open so every cohort and every
    /// candidate shares one horizon.
    pub as_of: DateTime<Utc>,
    pub skip_duplicate_identity: bool,
    pub duplicate_identity_window_hours: u64,
    pub max_concurrent_tokens: u32,
    pub max_total_tokens: u32,
    pub generator: GeneratorConfig,
}

impl RunConfig {
    fn score_config<'a>(
        &self,
        flow: Option<&'a FlowPatterns>,
        fp: FingerprintId,
    ) -> ScoreConfig<'a> {
        ScoreConfig {
            pricing: self.pricing,
            as_of: self.as_of,
            flow,
            flow_fp: fp,
            skip_duplicate_identity: self.skip_duplicate_identity,
            duplicate_identity_window_hours: self.duplicate_identity_window_hours,
            max_concurrent_tokens: self.max_concurrent_tokens,
            max_total_tokens: self.max_total_tokens,
        }
    }

    fn replay_config(&self) -> ReplayConfig {
        ReplayConfig {
            as_of: self.as_of,
            fill_model: self.pricing.fill_model,
            skip_duplicate_identity: self.skip_duplicate_identity,
            duplicate_identity_window_hours: self.duplicate_identity_window_hours,
            fill_delay_ms: 0,
        }
    }
}

/// Earn the candidate menu from a cohort's **own paths** — the signature machinery,
/// then the per-family diversity quota.
///
/// The menu is earned once, from the **target** cohort, and then scored unchanged
/// across the family. Two reasons: the target is the launch style the run reports a
/// level for, and a per-cohort menu would make the fit ranking meaningless (there
/// would be no common candidate to rank). Pooling every cohort's paths to earn from
/// would need all six corpora resident, which is the RAM spike this job is built to
/// avoid.
pub fn earn_candidates(
    tokens: &[CorpusToken],
    flow: Option<&FlowPatterns>,
    fp: FingerprintId,
    cfg: &GeneratorConfig,
) -> QuotaOutcome<Candidate> {
    let cuts = build_cut_table(tokens, flow, fp);
    generator::generate(&cuts, cfg)
}

/// One cohort's contribution: the per-candidate ranking numbers, nothing else.
#[derive(Clone, Debug)]
pub struct CohortRun {
    pub fp_id: Uuid,
    pub name: String,
    pub axis_value: Option<f64>,
    pub is_target: bool,
    /// Tokens the fingerprint matched — the hand-count guard.
    pub n_matched: u64,
    /// Per candidate, in candidate order.
    pub scores: Vec<CohortScore>,
    /// Per candidate: share of matched tokens admitted **before** the guards. This is
    /// what the axis-duplication gate reads, and it costs zero extra runs.
    pub enter_pct: Vec<f64>,
    /// The ungated control on this cohort (D6) — `None` when it was not scored.
    pub ungated_ret_pct: Option<f64>,
}

/// Score a fixed candidate menu on one cohort — the **fit tier**. Stops at the
/// archive fold: this stage needs a ranking, and `score_combos` folds every candidate
/// while walking each token's trades once, so candidates are near-free against the
/// token walk. One HTTP simulate per candidate is the shape to avoid — it re-loads the
/// corpus every time.
///
/// `extra` is scored alongside (the ungated control), so it costs no second walk.
#[allow(clippy::too_many_arguments)]
pub fn score_cohort(
    tokens: &[CorpusToken],
    candidates: &[Candidate],
    extra: Option<&GeneratedCombo>,
    flow: Option<&FlowPatterns>,
    fp: FingerprintId,
    cfg: &RunConfig,
    observer: &dyn SweepObserver,
) -> anyhow::Result<CohortScored> {
    let mut combos: Vec<GeneratedCombo> = candidates.iter().map(|c| c.combo.clone()).collect();
    let n = combos.len();
    if let Some(e) = extra {
        combos.push(e.clone());
    }
    if combos.is_empty() {
        return Ok((Vec::new(), Vec::new(), None));
    }
    let sc = cfg.score_config(flow, fp);
    let archive = score_combos(tokens, &combos, &sc, observer)?;
    let n_matched = tokens.len() as u64;
    let buy = cfg.pricing.buy_amount_sol;
    let scores: Vec<CohortScore> = archive
        .iter()
        .take(n)
        .map(|a| CohortScore { pnl_sol: a.total_pnl_sol, entry_sol: a.n_tokens as f64 * buy })
        .collect();
    let enter_pct: Vec<f64> = archive.iter().take(n).map(|a| a.enter_pct(n_matched)).collect();
    let ungated = extra.map(|_| {
        let a = &archive[n];
        score::CohortScore { pnl_sol: a.total_pnl_sol, entry_sol: a.n_tokens as f64 * buy }
            .ret_pct()
    });
    Ok((scores, enter_pct, ungated))
}

/// Convert a replay position to the sweep's outcome shape, **populating** the
/// `exit_metric*` fields from the `ExitReason` the engine returned (D4).
///
/// The slot is numbered by the shared bind-time
/// [`exit_metric_labels`](crate::sweep::generic::strategy::exit_metric_labels), not by
/// a second implementation — otherwise a replay-sourced attribution could disagree
/// with the sweep's `n_exit_metrics_by_slot` on the very same rule.
pub fn replay_to_outcome(
    po: &PositionOutcome,
    labels: &[ExitSlotLabel],
    buy_sol: f64,
    cost: &CostModel,
) -> TokenOutcome {
    let (pnl_sol, pnl_pct) = po.pnl_with_costs(buy_sol, cost);
    let metrics = match po.exit_reason {
        Some(ExitReason::Metrics { metric, operator, value, window }) => {
            let slot = labels
                .iter()
                .flatten()
                // Match on (metric, window) first: a dynamic group and its lifetime
                // twin share `metric.name()`, and a multi-arm DNF can report a
                // different arm's operator/value than the one bound here.
                .find(|(m, _, _, w, _)| *m == metric && *w == window)
                .or_else(|| labels.iter().flatten().find(|(m, _, _, _, _)| *m == metric))
                .map(|(_, _, _, _, s)| *s);
            Some((metric, operator, value, window, slot))
        }
        _ => None,
    };
    TokenOutcome {
        fired: true,
        holding_secs: po.exit_time.map(|t| (t - po.entry_time).num_seconds()).unwrap_or(0),
        pnl_percent: pnl_pct as f32,
        pnl_sol: pnl_sol as f32,
        exit: match po.exit_reason {
            None => ExitCode::Open,
            Some(ExitReason::TakeProfit) => ExitCode::TakeProfit,
            Some(ExitReason::StopLoss) => ExitCode::StopLoss,
            Some(ExitReason::Metrics { .. }) => ExitCode::Metrics,
            Some(ExitReason::Dead) => ExitCode::Dead,
            Some(ExitReason::Manual | ExitReason::Migrated) => ExitCode::Open,
        },
        exit_metric: metrics.map(|(m, _, _, _, _)| m),
        exit_operator: metrics.map(|(_, o, _, _, _)| o),
        exit_metric_value: metrics.map(|(_, _, v, _, _)| v),
        exit_metric_window: metrics.and_then(|(_, _, _, w, _)| w),
        exit_metric_slot: metrics.and_then(|(_, _, _, _, s)| s),
        entry_time: Some(po.entry_time),
        entry_price: Some(po.entry_price),
        entry_slot: None,
        exit_time: po.exit_time,
        exit_price: po.exit_price,
        exit_slot: None,
    }
}

/// One rule replayed on one cohort under the run's authority fill — the **authority
/// tier**. Returns the per-token outcomes with their authored exit slots stamped, plus
/// the cohort score.
pub struct Authority {
    pub outcomes: Vec<TokenOutcome>,
    /// The corpus token each outcome belongs to, by index — the oracle's key.
    pub token_idx: Vec<usize>,
    pub score: CohortScore,
    pub n_tokens: u64,
}

/// Replay `params` over `tokens` and stamp every authored exit slot.
pub fn authority(
    tokens: &[CorpusToken],
    fp: &EngineFingerprint,
    params: &RuleParams,
    cfg: &RunConfig,
) -> Authority {
    let loaded = loaded_from_params(
        params.clone(),
        fp.id,
        cfg.pricing.buy_amount_sol,
        cfg.max_concurrent_tokens,
        cfg.max_total_tokens,
    );
    let labels = authored_exit_labels(&loaded);
    let outcomes_raw = run_replay(
        std::slice::from_ref(&loaded),
        std::slice::from_ref(fp),
        to_replay_tokens(tokens),
        cfg.replay_config(),
    );
    let index: std::collections::HashMap<&str, usize> =
        tokens.iter().enumerate().map(|(i, t)| (t.mint.as_str(), i)).collect();

    let mut outcomes = Vec::with_capacity(outcomes_raw.len());
    let mut token_idx = Vec::with_capacity(outcomes_raw.len());
    let mut mints: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut pnl_sol = 0.0f64;
    for po in &outcomes_raw {
        let Some(&ti) = index.get(po.mint.as_str()) else { continue };
        mints.insert(po.mint.as_str());
        let o = replay_to_outcome(po, &labels, cfg.pricing.buy_amount_sol, &cfg.pricing.cost);
        // Realized only — an open mark must not read as money the rule made.
        if o.exit != ExitCode::Open {
            pnl_sol += o.pnl_sol as f64;
        }
        outcomes.push(o);
        token_idx.push(ti);
    }
    let n_tokens = mints.len() as u64;
    Authority {
        outcomes,
        token_idx,
        score: CohortScore { pnl_sol, entry_sol: n_tokens as f64 * cfg.pricing.buy_amount_sol },
        n_tokens,
    }
}

/// A rule's authored exit slots, numbered exactly as the sweep numbers them.
fn authored_exit_labels(loaded: &LoadedRule) -> Vec<ExitSlotLabel> {
    exit_metric_labels(&CompiledRule::compile(loaded).exit_reqs)
}

/// The capture ratio for one authority pass (D3).
pub fn capture_of(tokens: &[CorpusToken], a: &Authority, pricing: &Pricing) -> oracle::Capture {
    let mut acc = CaptureAcc::default();
    for (o, &ti) in a.outcomes.iter().zip(&a.token_idx) {
        acc.record(&tokens[ti], o, pricing);
    }
    acc.finish()
}

/// Re-score the finalist on the target cohort with each exit term dropped in turn —
/// the **narrow** re-check. A term worth nothing across the family can be worth ten
/// points here, and only this stage sees it.
pub fn narrow_recheck(
    tokens: &[CorpusToken],
    fp: &EngineFingerprint,
    finalist: &GeneratedCombo,
    cfg: &RunConfig,
    full: CohortScore,
) -> Vec<score::TermContribution> {
    use crate::rule_search::generator::{assemble, ExitBag};
    let mut dropped = Vec::new();
    for i in 0..finalist.exit.clauses.len() {
        let mut clauses = finalist.exit.clauses.clone();
        let removed = clause_label(&clauses.remove(i));
        let params = assemble(&finalist.entry, &ExitBag { clauses });
        dropped.push((removed, authority(tokens, fp, &params, cfg).score));
    }
    score::narrow_recheck(full, &dropped)
}

/// Measure every entry clause the finalist uses against the family's varied axis
/// (plan §2c). Reads the `enter_pct` the family loop already computed — zero extra
/// runs.
pub fn entry_gates(
    finalist: &Candidate,
    finalist_idx: usize,
    cohorts: &[CohortRun],
) -> Vec<gates::AxisDuplication> {
    let axis_value: Vec<f64> = cohorts.iter().filter_map(|c| c.axis_value).collect();
    if axis_value.len() != cohorts.len() {
        // A family of one, or an axis with no value on some member: nothing to
        // correlate, so the gate stays silent rather than judging what it cannot see.
        return Vec::new();
    }
    let admit: Vec<f64> = cohorts
        .iter()
        .map(|c| c.enter_pct.get(finalist_idx).copied().unwrap_or(0.0))
        .collect();
    // The whole entry AND admits together, so one admit series grades every clause in
    // it — a per-clause series would need a per-clause run, which this gate exists to
    // avoid.
    finalist
        .combo
        .entry
        .clauses
        .iter()
        .map(|c| gates::axis_duplication(clause_label(c), &admit, &axis_value))
        .collect()
}

/// Convenience: the run's authority fill is the request's; the optimistic model is
/// only ever a fill-sensitivity diagnostic.
pub const FILL_OPTIMISTIC: FillModel = FillModel::FirstInWindow;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::family_search::fixtures::pricing;
    use hunter_engine::rule_params::RuleParams;

    fn cfg() -> RunConfig {
        RunConfig {
            pricing: pricing(),
            as_of: Utc::now(),
            skip_duplicate_identity: true,
            duplicate_identity_window_hours: 24,
            max_concurrent_tokens: 0,
            max_total_tokens: 0,
            generator: GeneratorConfig::default(),
        }
    }

    fn fp() -> EngineFingerprint {
        EngineFingerprint {
            id: FingerprintId(Uuid::nil()),
            cu_limit: None,
            cu_price: None,
            ix_labels: None,
            init_buy_lamports: None,
            max_cost_lamports: None,
            spendable_lamports_in: None,
            first_slot_buy_lamports: None,
            first_slot_sell_lamports: None,
            bucket_size_amount: Some(0.1),
            metric_config: serde_json::json!({}),
        }
    }

    /// A rule with two authored exit terms on the SAME metric at different windows —
    /// the shape a slot lookup must not collapse. Both print as `nonvol_buy`, both
    /// carry `MetricId::WinNonvolBuy`, and only the window tells them apart.
    fn two_window_rule() -> RuleParams {
        use crate::rule_search::cuts::CutPhase;
        use crate::rule_search::generator::{assemble, Clause, EntryFilling, ExitBag};
        use hunter_engine::metrics::MetricGroupId;
        let c = |w: f64, v: f64| Clause {
            group: MetricGroupId::FlowSplitWindow,
            metric: MetricId::WinNonvolBuy,
            window: Some(w),
            op: Operator::Gte,
            threshold: v,
            phase: CutPhase::DumpLead,
        };
        assemble(
            &EntryFilling { clauses: vec![] },
            &ExitBag { clauses: vec![c(2.0, 1.6), c(10.0, 0.9)] },
        )
    }

    #[test]
    fn a_replayed_metrics_exit_carries_its_authored_slot() {
        let loaded = loaded_from_params(two_window_rule(), fp().id, 0.01, 0, 0);
        let labels = authored_exit_labels(&loaded);
        let slots: Vec<u8> = labels.iter().flatten().map(|(_, _, _, _, s)| *s).collect();
        assert_eq!(slots, vec![0, 1], "two authored reqs occupy two slots");

        // The engine reports the WINDOWED term. Its slot must be the windowed one,
        // not the lifetime twin's — they share `metric.name()`.
        let po = |window: Option<f64>, value: f64| PositionOutcome {
            mint: "m".into(),
            rule: hunter_engine::event::RuleId(Uuid::nil()),
            target_price: None,
            target_token_amount: None,
            target_time: None,
            target_tx: None,
            entry_price: 1.0,
            entry_token_amount: 1,
            entry_time: Utc::now(),
            entry_tx: String::new(),
            entry_reserve_sol: Some(40.0),
            exit_price: Some(1.2),
            exit_time: Some(Utc::now()),
            exit_tx: None,
            exit_reason: Some(ExitReason::Metrics {
                metric: MetricId::WinNonvolBuy,
                operator: Operator::Gte,
                value,
                window,
            }),
            exit_legs: Vec::new(),
            last_price: 1.2,
        };
        let cost = CostModel::pumpfun_with_impact();
        let burst = replay_to_outcome(&po(Some(2.0), 1.6), &labels, 0.01, &cost);
        let grind = replay_to_outcome(&po(Some(10.0), 0.9), &labels, 0.01, &cost);
        assert_eq!(burst.exit, ExitCode::Metrics);
        assert_eq!(burst.exit_metric_window, Some(2.0));
        assert_eq!(grind.exit_metric_window, Some(10.0));
        assert_ne!(
            burst.exit_metric_slot, grind.exit_metric_slot,
            "two windows of one metric must not share a slot"
        );
        assert!(burst.exit_metric_slot.is_some() && grind.exit_metric_slot.is_some());

        // A non-metric close stamps no slot at all — never a fabricated bucket.
        let mut tp = po(Some(2.0), 1.6);
        tp.exit_reason = Some(ExitReason::TakeProfit);
        let tp = replay_to_outcome(&tp, &labels, 0.01, &cost);
        assert_eq!(tp.exit, ExitCode::TakeProfit);
        assert_eq!(tp.exit_metric_slot, None);
    }

    #[test]
    fn the_authority_pass_runs_the_one_kernel_over_a_corpus() {
        // A tiny synthetic cohort: the point is that the pass wires up and books
        // realized-only money, not what the number is.
        let corpus = crate::discovery::fixtures::corpus(4);
        let a = authority(&corpus.tokens, &fp(), &RuleParams::default(), &cfg());
        assert_eq!(a.outcomes.len(), a.token_idx.len());
        assert!(a.token_idx.iter().all(|&i| i < corpus.tokens.len()));
        assert_eq!(
            a.score.entry_sol,
            a.n_tokens as f64 * cfg().pricing.buy_amount_sol,
            "capital committed is n_tokens x the REQUEST's buy size"
        );
    }

    #[test]
    fn the_entry_gate_stays_silent_on_a_family_of_one() {
        let cand = Candidate {
            combo: generator::ungated_control(),
            families: Default::default(),
            flags: Vec::new(),
        };
        let one = vec![CohortRun {
            fp_id: Uuid::nil(),
            name: "solo".into(),
            axis_value: None,
            is_target: true,
            n_matched: 10,
            scores: vec![CohortScore::default()],
            enter_pct: vec![0.5],
            ungated_ret_pct: None,
        }];
        assert!(entry_gates(&cand, 0, &one).is_empty());
    }
}
