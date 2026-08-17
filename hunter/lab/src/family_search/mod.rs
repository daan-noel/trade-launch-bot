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
//! Reference: [family-search.md] — purpose, decisions D1-D12, evidence, how to run.
//!
//! [family-search.md]: ../../docs/roadmap/family-search.md

pub mod attribution;
pub mod dto;
pub mod enrich;
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
/// the ungated control's score when it was scored alongside.
pub type CohortScored = (Vec<CohortScore>, Vec<f64>, Option<CohortScore>);

/// A fast-tier archive row as a [`CohortScore`].
///
/// `n_wins` is reconstructed from the row's own win **rate** — the fold keeps a rate,
/// not a count — and rounding it back to a count is exact for any rate the fold can
/// produce, since the rate is `wins / n_closed` with both integers.
fn archive_score(a: &crate::rule_search::scorer::ArchiveRow, buy: f64) -> CohortScore {
    CohortScore {
        pnl_sol: a.total_pnl_sol,
        entry_sol: a.n_tokens as f64 * buy,
        n_closed: a.n_closed,
        n_wins: (a.win_rate * a.n_closed as f64).round() as u64,
    }
}

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
    standing: &[generator::StandingTerm],
) -> QuotaOutcome<Candidate> {
    let cuts = build_cut_table(tokens, flow, fp);
    generator::generate(&cuts, cfg, standing)
}

/// The cohort's own cut table — the enrich stage's menu comes from the same earning
/// pass the generator used, never from a wider registry sweep.
pub fn cut_table(
    tokens: &[CorpusToken],
    flow: Option<&FlowPatterns>,
    fp: FingerprintId,
) -> crate::rule_search::cuts::CutTable {
    build_cut_table(tokens, flow, fp)
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
    /// The ungated control on this cohort (D6) — `None` when it was not scored. Its
    /// **win rate** is the entry side's bar: a gate that does not enter more safely
    /// than buying everything is not filtering anything (D11).
    pub ungated: Option<CohortScore>,
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
    let scores: Vec<CohortScore> = archive.iter().take(n).map(|a| archive_score(a, buy)).collect();
    let enter_pct: Vec<f64> = archive.iter().take(n).map(|a| a.enter_pct(n_matched)).collect();
    let ungated = extra.map(|_| archive_score(&archive[n], buy));
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
    let (mut n_closed, mut n_wins) = (0u64, 0u64);
    for po in &outcomes_raw {
        let Some(&ti) = index.get(po.mint.as_str()) else { continue };
        mints.insert(po.mint.as_str());
        let o = replay_to_outcome(po, &labels, cfg.pricing.buy_amount_sol, &cfg.pricing.cost);
        // Realized only — an open mark must not read as money the rule made, and an
        // open position has not won anything yet either.
        if o.exit != ExitCode::Open {
            pnl_sol += o.pnl_sol as f64;
            n_closed += 1;
            if o.pnl_sol > 0.0 {
                n_wins += 1;
            }
        }
        outcomes.push(o);
        token_idx.push(ti);
    }
    let n_tokens = mints.len() as u64;
    Authority {
        outcomes,
        token_idx,
        score: CohortScore {
            pnl_sol,
            entry_sol: n_tokens as f64 * cfg.pricing.buy_amount_sol,
            n_closed,
            n_wins,
        },
        n_tokens,
    }
}

/// A rule's authored exit slots, numbered exactly as the sweep numbers them.
fn authored_exit_labels(loaded: &LoadedRule) -> Vec<ExitSlotLabel> {
    exit_metric_labels(&CompiledRule::compile(loaded).exit_reqs)
}

/// The share of a rule's entries that never had a profitable exit available — the
/// **entry**'s own score, readable with no exit rule at all (D3).
///
/// An entry gate's job is to admit fewer of these. Against the ungated control's share
/// it says, in one number, whether the gate is filtering losers or just trading less.
pub fn no_upside_pct(c: &oracle::Capture) -> Option<f64> {
    let n = c.n_entered();
    (n > 0).then(|| 100.0 * c.n_no_upside as f64 / n as f64)
}

/// The capture ratio for one authority pass (D3).
pub fn capture_of(tokens: &[CorpusToken], a: &Authority, pricing: &Pricing) -> oracle::Capture {
    let mut acc = CaptureAcc::default();
    for (o, &ti) in a.outcomes.iter().zip(&a.token_idx) {
        acc.record(&tokens[ti], o, pricing);
    }
    acc.finish()
}

/// Re-score the finalist on the target cohort with each term dropped in turn — the
/// **narrow** re-check. A term worth nothing across the family can be worth ten points
/// here, and only this stage sees it.
///
/// Both sides are ablated, because each is graded in its own currency (D11): an entry
/// idea earns its place by lifting the **win rate**, an exit alarm by lifting the
/// **return**. Grading an entry clause on return alone is the recorded reason a search
/// deletes every entry condition.
///
/// Standing terms are never dropped: a mechanical exit is not a finding, and a board
/// row asking whether to keep "sell at migration" is noise.
pub fn narrow_recheck(
    tokens: &[CorpusToken],
    fp: &EngineFingerprint,
    finalist: &GeneratedCombo,
    n_standing: usize,
    cfg: &RunConfig,
    full: CohortScore,
) -> Vec<score::TermContribution> {
    use crate::rule_search::generator::{assemble, EntryFilling, ExitBag};
    let mut dropped = Vec::new();
    for i in 0..finalist.entry.clauses.len() {
        let mut clauses = finalist.entry.clauses.clone();
        let removed = clause_label(&clauses.remove(i));
        let params = assemble(&EntryFilling { clauses }, &finalist.exit);
        dropped.push((removed, true, authority(tokens, fp, &params, cfg).score));
    }
    let searched = finalist.exit.clauses.len().saturating_sub(n_standing);
    for i in 0..searched {
        let mut clauses = finalist.exit.clauses.clone();
        let removed = clause_label(&clauses.remove(i));
        let params = assemble(&finalist.entry, &ExitBag { clauses });
        dropped.push((removed, false, authority(tokens, fp, &params, cfg).score));
    }
    score::narrow_recheck(full, &dropped)
}

/// The **enrich** stage (D12): offer the finalist every earned idea it does not carry
/// and keep what pays, each side judged in its own currency.
///
/// The inverse of [`narrow_recheck`], and the only stage in the job that can make a
/// rule denser. Bounded at [`enrich::MAX_TRIALS`] + [`enrich::MAX_ACCEPTED`] replays of
/// one rule over the already-resident target cohort.
pub fn narrow_enrich(
    tokens: &[CorpusToken],
    fp: &EngineFingerprint,
    finalist: &Candidate,
    cuts: &crate::rule_search::cuts::CutTable,
    standing: &[generator::StandingTerm],
    min_closed: u64,
    cfg: &RunConfig,
) -> enrich::Enriched {
    let (entry_menu, exit_menu) = generator::unused_clauses(
        cuts,
        &finalist.combo.entry.clauses,
        finalist.searched_exit(),
        standing,
    );
    let score = |e: &crate::rule_search::generator::EntryFilling,
                 x: &crate::rule_search::generator::ExitBag|
     -> CohortScore {
        authority(tokens, fp, &generator::reassemble(e, x), cfg).score
    };
    enrich::enrich(
        &finalist.combo,
        finalist.n_standing,
        &entry_menu,
        &exit_menu,
        min_closed,
        score,
    )
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

/// The same rule and the same taken set, priced two ways (charter D8, corollary).
///
/// A dump-scalp family read −7.30%/trade at worst-fill + impact and −0.37%/trade at
/// first-fill + fee-only on the identical 5,872 entries. The signal was near
/// breakeven; execution was the entire loss. A finalist whose edge is smaller than
/// this spread is priced on fill luck, not on signal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spread {
    /// The run's own pricing — the number the board reports everywhere else.
    pub authority_ret_pct: f64,
    /// [`FILL_OPTIMISTIC`] + fee-only cost: the best pricing an honest reading allows.
    pub optimistic_ret_pct: f64,
    /// Optimistic minus authority, in percentage points. Always ≥ 0 in practice.
    pub spread_pp: f64,
    /// Positions both passes took — the population both numbers are computed over.
    pub n_common: u64,
    /// Positions only one pass took. Non-zero means the two passes did not decide
    /// alike and the comparison is no longer "the same taken set": the fill layer
    /// fixes *eligibility* across models, but a price-sensitive exit can still shift
    /// the close, and a concurrency cap can then let a different token in.
    pub n_authority_only: u64,
    pub n_optimistic_only: u64,
}

impl Spread {
    /// The taken sets matched, so the two numbers describe one population.
    pub fn clean(&self) -> bool {
        self.n_authority_only == 0 && self.n_optimistic_only == 0
    }

    /// The edge is smaller than the run's own uncertainty about the fill. Sign-agnostic
    /// on purpose: an edge of +2pp under a 7pp spread is fill luck whichever way the
    /// authority number leans.
    pub fn fill_luck(&self) -> bool {
        self.authority_ret_pct <= self.spread_pp
    }
}

/// The optimistic twin of [`authority`]: the same rule under [`FILL_OPTIMISTIC`] and
/// a fee-only cost model.
///
/// It is a second fold, not a repricing of outcomes in hand — the fill price is chosen
/// *inside* the kernel, and reconstructing it outside would be a second implementation
/// of the fill model, which the one-kernel rule forbids. It costs one replay of one
/// rule over the already-resident target cohort, against a fit tier that folds every
/// candidate across every cohort; call it once, on the finalist.
pub fn optimistic(
    tokens: &[CorpusToken],
    fp: &EngineFingerprint,
    params: &RuleParams,
    cfg: &RunConfig,
) -> Authority {
    let mut opt = cfg.clone();
    opt.pricing.fill_model = FILL_OPTIMISTIC;
    opt.pricing.cost = CostModel::pumpfun_fee_only();
    authority(tokens, fp, params, &opt)
}

/// Pair an authority pass with its optimistic twin over the **positions both took**.
///
/// The intersection is the point: quoting two returns computed over two different
/// populations would fold "different trades" into a number that is supposed to mean
/// "same trades, different fills".
pub fn spread(tokens: &[CorpusToken], auth: &Authority, opt: &Authority, buy: f64) -> Spread {
    let mints = |a: &Authority| -> std::collections::HashMap<&str, f64> {
        a.outcomes
            .iter()
            .zip(&a.token_idx)
            .filter(|(o, _)| o.exit != ExitCode::Open)
            .filter_map(|(o, &ti)| tokens.get(ti).map(|t| (t.mint.as_str(), o.pnl_sol as f64)))
            .collect()
    };
    let (a, o) = (mints(auth), mints(opt));
    let (mut a_sol, mut o_sol, mut n) = (0.0, 0.0, 0u64);
    for (mint, pnl) in &a {
        if let Some(other) = o.get(mint) {
            a_sol += pnl;
            o_sol += other;
            n += 1;
        }
    }
    let entry_sol = n as f64 * buy;
    // Return only: the spread is one taken set priced twice, and a win count over the
    // intersection would invite reading it as a second, differently-scoped grade.
    let authority_ret_pct =
        CohortScore { pnl_sol: a_sol, entry_sol, ..Default::default() }.ret_pct();
    let optimistic_ret_pct =
        CohortScore { pnl_sol: o_sol, entry_sol, ..Default::default() }.ret_pct();
    Spread {
        authority_ret_pct,
        optimistic_ret_pct,
        spread_pp: optimistic_ret_pct - authority_ret_pct,
        n_common: n,
        n_authority_only: (a.len() as u64).saturating_sub(n),
        n_optimistic_only: (o.len() as u64).saturating_sub(n),
    }
}

/// Mean seconds from a token's creation to its entry fill, over an authority pass.
/// `None` when nothing entered.
fn mean_entry_delay_secs(tokens: &[CorpusToken], a: &Authority) -> Option<f64> {
    let (mut sum, mut n) = (0.0f64, 0u64);
    for (o, &ti) in a.outcomes.iter().zip(&a.token_idx) {
        let (Some(at), Some(t)) = (o.entry_time, tokens.get(ti)) else { continue };
        sum += (at - t.created_at).num_milliseconds() as f64 / 1000.0;
        n += 1;
    }
    (n > 0).then(|| sum / n as f64)
}

/// The **entry**-side twin of [`narrow_recheck`] (plan §4d): drop each entry clause in
/// turn and measure what it was doing to the entry *instant*, not to the return.
///
/// A clause that holds entries back and whose kept entries have less upside left is
/// created by the move it is trying to precede — `gross_flow(60) >= 55` on the scalp
/// family raised volume *and* quality when it was dropped. Diagnostic only, never a
/// refusal: waiting for confirmation is a legitimate edge, and only the pairing of
/// "binds the instant" with "captures less" makes it a finding.
pub fn entry_timing(
    tokens: &[CorpusToken],
    fp: &EngineFingerprint,
    finalist: &GeneratedCombo,
    cfg: &RunConfig,
    full: &Authority,
) -> Vec<gates::EntryTiming> {
    use crate::rule_search::generator::{assemble, EntryFilling};
    let n_matched = tokens.len().max(1) as f64;
    let full_delay = mean_entry_delay_secs(tokens, full);
    let full_capture = capture_of(tokens, full, &cfg.pricing).capture_pct;

    let mut out = Vec::new();
    for i in 0..finalist.entry.clauses.len() {
        let mut clauses = finalist.entry.clauses.clone();
        let removed = clause_label(&clauses.remove(i));
        let params = assemble(&EntryFilling { clauses }, &finalist.exit);
        let without = authority(tokens, fp, &params, cfg);
        let without_delay = mean_entry_delay_secs(tokens, &without);
        let without_capture = capture_of(tokens, &without, &cfg.pricing).capture_pct;
        out.push(gates::EntryTiming {
            clause: removed,
            delay_added_secs: match (full_delay, without_delay) {
                (Some(f), Some(w)) => f - w,
                _ => 0.0,
            },
            capture_delta_pp: full_capture.zip(without_capture).map(|(f, w)| f - w),
            admit_delta_pct: 100.0
                * (without.n_tokens as f64 - full.n_tokens as f64)
                / n_matched,
        });
    }
    out
}

/// The cancel check the cohort loop makes **between** sibling loads (plan §4e).
///
/// A corpus load cannot be cancelled mid-flight — a known simulate weakness, out of
/// scope here — so the loop's own checkpoints are the whole cancellation story. Miss
/// them and the difference is a 30-second abort versus killing the process.
pub fn check_cancelled(observer: &dyn SweepObserver) -> anyhow::Result<()> {
    anyhow::ensure!(!observer.cancelled(), "cancelled");
    Ok(())
}

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

    /// The whole dump-scalp finding on one assertion: the SAME closes, priced two
    /// ways, and the gap is the answer.
    #[test]
    fn the_spread_prices_one_taken_set_twice_and_excludes_anything_only_one_side_took() {
        use crate::family_search::fixtures::{outcome_at, token_from_prices};

        let mut tokens = Vec::new();
        for i in 0..3 {
            let mut t = token_from_prices(&[1.0, 2.0]);
            t.mint = format!("m{i}");
            tokens.push(t);
        }
        let at = tokens[0].trades[0].block_time;
        let buy = 0.01;

        // Both passes take m0 and m1; only the authority pass reaches m2.
        let auth = Authority {
            outcomes: vec![
                outcome_at(at, 1.0, -0.002),
                outcome_at(at, 1.0, -0.001),
                outcome_at(at, 1.0, -0.005),
            ],
            token_idx: vec![0, 1, 2],
            score: CohortScore::default(),
            n_tokens: 3,
        };
        let opt = Authority {
            outcomes: vec![outcome_at(at, 1.0, 0.0), outcome_at(at, 1.0, 0.001)],
            token_idx: vec![0, 1],
            score: CohortScore::default(),
            n_tokens: 2,
        };

        let s = spread(&tokens, &auth, &opt, buy);
        // m2 is in neither number — a return over a different population is not the
        // same taken set, which is the only thing this measurement claims to be.
        assert_eq!(s.n_common, 2);
        assert_eq!(s.n_authority_only, 1);
        assert_eq!(s.n_optimistic_only, 0);
        assert!(!s.clean(), "a drifted taken set must say so, not average over it");

        // −0.003 over 0.02 committed = −15%; +0.001 over 0.02 = +5%. 20pp apart.
        assert!((s.authority_ret_pct + 15.0).abs() < 1e-4, "{}", s.authority_ret_pct);
        assert!((s.optimistic_ret_pct - 5.0).abs() < 1e-4, "{}", s.optimistic_ret_pct);
        assert!((s.spread_pp - 20.0).abs() < 1e-4);
        // The edge is smaller than the run's own uncertainty about the fill.
        assert!(s.fill_luck());
    }

    #[test]
    fn an_edge_larger_than_its_own_spread_is_not_fill_luck() {
        let s = Spread {
            authority_ret_pct: 31.0,
            optimistic_ret_pct: 37.9,
            spread_pp: 6.9,
            n_common: 180,
            n_authority_only: 0,
            n_optimistic_only: 0,
        };
        assert!(s.clean() && !s.fill_luck());
    }

    /// A corpus load cannot be cancelled mid-flight, so the loop's checkpoint between
    /// one sibling's fold and the next one's load is the whole cancellation story.
    #[test]
    fn a_cancel_between_sibling_loads_stops_the_run() {
        struct Flag(std::sync::atomic::AtomicBool);
        impl SweepObserver for Flag {
            fn set_total(&self, _: usize, _: usize) {}
            fn token_done(&self, _: usize) {}
            fn cancelled(&self) -> bool {
                self.0.load(std::sync::atomic::Ordering::Acquire)
            }
        }
        let f = Flag(std::sync::atomic::AtomicBool::new(false));
        assert!(check_cancelled(&f).is_ok(), "an uncancelled run proceeds");
        f.0.store(true, std::sync::atomic::Ordering::Release);
        let e = check_cancelled(&f).expect_err("a raised cancel stops the loop");
        assert_eq!(e.to_string(), "cancelled");
    }

    /// A win rate is a rate, so it has to survive the trip through the fast tier's
    /// rate-only archive row and come back as the counts a pooled rate needs.
    #[test]
    fn the_fast_tier_round_trips_a_win_rate_back_into_counts() {
        let row = crate::rule_search::scorer::ArchiveRow {
            combo_idx: 0,
            n_fired: 253,
            n_closed: 253,
            n_tokens: 253,
            n_fired_unguarded: 300,
            total_pnl_sol: 0.31,
            block_pnl_sol: [0.0; crate::rule_search::scorer::N_BLOCKS],
            profit_factor: Some(1.4),
            win_rate: 62.0 / 253.0,
        };
        let s = archive_score(&row, 0.01);
        assert_eq!(s.n_closed, 253);
        assert_eq!(s.n_wins, 62, "the rate is wins/closes, so it rounds back exactly");
        assert!((s.win_rate_pct().unwrap() - 100.0 * 62.0 / 253.0).abs() < 1e-9);
        assert!((s.entry_sol - 2.53).abs() < 1e-9);
    }

    /// The band has to be derived rather than measured once and hard-coded, because
    /// it is **U-shaped in buy size**: the fixed per-leg cost shrinks as a share of a
    /// larger notional while our own price impact grows into the same pool, so the
    /// bar a cohort must clear moves — in both directions — with the run's own knobs.
    #[test]
    fn the_execution_band_is_derived_from_the_run_s_own_cost_model() {
        use crate::family_search::oracle::execution_band_pct;
        let depth = 40.0;
        let band_at = |buy: f64| {
            execution_band_pct(&Pricing { buy_amount_sol: buy, ..pricing() }, Some(depth))
        };
        assert!(band_at(0.01) > 0.0, "a round trip is never free");

        // `sqrt(fixed_per_leg x depth)` is the cost-minimising buy — either side of it
        // costs more, and a constant band would be wrong on both sides.
        let optimal = (0.000225f64 * depth).sqrt();
        let floor = band_at(optimal);
        assert!(band_at(optimal / 10.0) > floor, "a tiny buy is all fixed cost");
        assert!(band_at(optimal * 10.0) > floor, "a large buy is all price impact");

        // And it responds to the pool it trades into.
        assert!(
            band_at_depth(optimal, 4.0) > band_at_depth(optimal, depth),
            "a thinner pool costs more impact for the same buy"
        );
    }

    fn band_at_depth(buy: f64, depth: f64) -> f64 {
        crate::family_search::oracle::execution_band_pct(
            &Pricing { buy_amount_sol: buy, ..pricing() },
            Some(depth),
        )
    }

    #[test]
    fn the_entry_gate_stays_silent_on_a_family_of_one() {
        let cand = Candidate {
            combo: generator::ungated_control(&[]),
            families: Default::default(),
            flags: Vec::new(),
            n_entry_quantities: 0,
            n_standing: 0,
        };
        let one = vec![CohortRun {
            fp_id: Uuid::nil(),
            name: "solo".into(),
            axis_value: None,
            is_target: true,
            n_matched: 10,
            scores: vec![CohortScore::default()],
            enter_pct: vec![0.5],
            ungated: None,
        }];
        assert!(entry_gates(&cand, 0, &one).is_empty());
    }
}
