//! Grouped sweep: partition the corpus by exact-value fingerprint key, then sweep
//! each group's combos × tokens, surfacing each group's best combo.
//!
//! **Pool utilisation (two-phase driver).** Naively sweeping each group with its
//! own `run_sweep` left most cores idle on the common many-small-groups case (a
//! 3-token group uses 3 threads on an N-thread pool). The fix routes groups by
//! size against the pool:
//!   - **Large groups** (`≥ LARGE_GROUP_TOKEN_FACTOR × threads` tokens) are swept
//!     one at a time via [`run_sweep`], whose inner `par_iter` already saturates
//!     the pool on a single group — so big/few-group runs (incl. the "ALL" case)
//!     are unchanged.
//!   - **Small groups** are swept **across groups in parallel** (`par_iter` over
//!     the groups), each folded **single-threaded** — so the pool stays busy even
//!     when every group is tiny.
//!
//! This is the "fan the outer loop with rayon, sharing the bounded pool" design.
//! It is deliberately **not** a full fold-time partition (one `par_iter` over the
//! whole corpus routing into per-group accumulators): that keeps *every* group's
//! `combos × ComboAgg` resident at once — at default settings (~1k groups × 5k
//! combos) tens of GB — whereas this driver holds at most `threads × combos`
//! accumulators (each group finalised to small `ComboMetrics` then freed). No
//! pools are nested (large groups fold serially inside their own `run_sweep`;
//! small groups fold serially inside the cross-group `par_iter`).
//!
//! Each sub-corpus is a refcount-clone of `TokenTrades` (`trades` is an `Arc`, so
//! no trade buffer is copied). Empty `fields` ⇒ a single "ALL" group ⇒ identical
//! to a global ungrouped sweep.

use std::cmp::Ordering::{self, Equal, Greater, Less};
use std::collections::{HashMap, HashSet};

use anyhow::{bail, Result};
use rayon::prelude::*;

use crate::sweep::aggregate::{ComboAgg, ComboMetrics};
use crate::sweep::corpus::Corpus;
use crate::sweep::engine::{fill_outcomes, run_sweep};
use crate::sweep::grouping::{group_key, GroupField, GroupKey};
use crate::sweep::progress::{CancelOnly, SweepObserver};
use crate::sweep::strategy::{RefineSpec, Strategy, TokenOutcome};

/// A group with at least `this × pool_threads` tokens is swept with **intra-group**
/// parallelism (its own `run_sweep` saturates the pool); smaller groups are swept
/// **across** groups in parallel instead. The factor gives a large group enough
/// tokens that rayon's per-token parallelism pays for its fold-thread overhead.
const LARGE_GROUP_TOKEN_FACTOR: usize = 4;

/// Per-combo coverage floor for the group winner: a combo must fire on
/// `max(min_fired_abs, ceil(fire_frac · group_tokens))` tokens before it's
/// eligible to be crowned `best_combo`. Stops a combo that fired on a lucky 2
/// tokens out of 200 from out-ranking a combo proven over 150 — the
/// over-fit failure mode the headline pick used to have.
#[derive(Clone, Copy, Debug)]
pub struct CoverageFloor {
    /// Absolute minimum fired tokens (e.g. 10).
    pub min_fired_abs: u64,
    /// Fraction of the group's tokens that must fire (e.g. 0.05 = 5%).
    pub fire_frac: f64,
}

impl Default for CoverageFloor {
    fn default() -> Self {
        Self { min_fired_abs: 10, fire_frac: 0.05 }
    }
}

impl CoverageFloor {
    /// The absolute fired-token threshold for a group of `group_tokens` tokens.
    /// Always ≥ 1 (a combo that never fired can never be the winner).
    fn threshold(&self, group_tokens: usize) -> u64 {
        let frac = (self.fire_frac.max(0.0) * group_tokens as f64).ceil() as u64;
        self.min_fired_abs.max(frac).max(1)
    }
}

/// One group's full sweep: its key, how many tokens fell into it, the per-combo
/// ranked metrics, and the winning combo. The winner is chosen on the **robust,
/// realized** `score` (the same metric the drill-in table sorts by) among combos
/// clearing the [`CoverageFloor`] — see [`best_combo`].
pub struct GroupResult {
    pub key: GroupKey,
    pub token_count: usize,
    pub metrics: Vec<ComboMetrics>,
    /// Combo id maximising the robust realized `score` among combos clearing the
    /// coverage floor (see [`best_combo`]).
    pub best_combo_id: u32,
    /// The winning combo's robust `score` (`μ−Z·σ/√n` over closed trades), or
    /// `None` when it has < 2 closed trades. The page's headline metric.
    pub best_score: Option<f64>,
    /// The winning combo's expectancy per trade — kept as a secondary readout
    /// (no longer the ranking metric).
    pub best_expectancy_sol: f64,
}

/// Partition token indices by exact-value group key. Pure `O(tokens)` pass.
pub fn partition(corpus: &Corpus, fields: &[GroupField]) -> HashMap<GroupKey, Vec<usize>> {
    let mut groups: HashMap<GroupKey, Vec<usize>> = HashMap::new();
    for (i, tt) in corpus.tokens.iter().enumerate() {
        groups.entry(group_key(&tt.fp, fields)).or_default().push(i);
    }
    groups
}

/// Group the corpus, drop groups below `min_tokens`, and sweep each surviving
/// group. Returns groups in a deterministic order (largest first, then by key)
/// so re-runs assign the same `group_index`.
///
/// `observer` is told the total surviving-token count up front (so the progress
/// bar is determinate from the first frame) and polled for cancellation between
/// groups; a cancel bails with an `Err` the caller maps to a cancelled response.
pub fn run_grouped_sweep<S: Strategy>(
    strategy: &S,
    params: &[S::Params],
    corpus: &Corpus,
    fields: &[GroupField],
    min_tokens: usize,
    coverage: CoverageFloor,
    observer: &dyn SweepObserver,
) -> Result<Vec<GroupResult>> {
    let floor = min_tokens.max(1);
    // Decorate with the tie-break key once (JSON-serializing inside the comparator
    // re-ran it O(n log n) times); sort, then drop the decoration.
    let mut surviving: Vec<(String, GroupKey, Vec<usize>)> = partition(corpus, fields)
        .into_iter()
        .filter(|(_, idx)| idx.len() >= floor)
        .map(|(key, idx)| (key.to_json().to_string(), key, idx))
        .collect();
    // Deterministic group order: most-populated first, ties broken by key JSON.
    surviving.sort_by(|a, b| b.2.len().cmp(&a.2.len()).then_with(|| a.0.cmp(&b.0)));
    let surviving: Vec<(GroupKey, Vec<usize>)> =
        surviving.into_iter().map(|(_, key, idx)| (key, idx)).collect();

    // Total work unit = tokens across all surviving groups; lets the bar show a
    // real percentage that climbs smoothly through every group's per-token fold.
    let total_tokens: usize = surviving.iter().map(|(_, idx)| idx.len()).sum();
    observer.set_total(total_tokens);

    tracing::info!(
        groups = surviving.len(),
        n_fields = fields.len(),
        min_tokens = floor,
        combos = params.len(),
        "grouped sweep: partitioned corpus, sweeping each group"
    );

    let threads = rayon::current_num_threads().max(1);
    let large_min = LARGE_GROUP_TOKEN_FACTOR * threads;

    // Fill survivor slots by position so the deterministic group order survives
    // regardless of which phase produced each result.
    let mut results: Vec<Option<GroupResult>> = (0..surviving.len()).map(|_| None).collect();

    // Phase 1 — large groups: intra-group parallel, one group at a time.
    for (pos, (key, idx)) in surviving.iter().enumerate() {
        if idx.len() < large_min {
            continue;
        }
        if observer.cancelled() {
            bail!("sweep cancelled");
        }
        let sub = sub_corpus(corpus, idx);
        let (_stats, metrics) = run_sweep(strategy, params, &sub, observer)?;
        // A cancel mid-group leaves the just-swept metrics partial — discard.
        if observer.cancelled() {
            bail!("sweep cancelled");
        }
        results[pos] = Some(make_group_result(key.clone(), idx.len(), metrics, coverage));
    }

    // Phase 2 — small groups: parallel across groups (each folded single-threaded),
    // so the pool stays saturated even when every group is tiny.
    let small: Vec<(usize, &GroupKey, &Vec<usize>)> = surviving
        .iter()
        .enumerate()
        .filter(|(_, (_, idx))| idx.len() < large_min)
        .map(|(pos, (key, idx))| (pos, key, idx))
        .collect();
    let small_results: Vec<Result<(usize, GroupResult)>> = small
        .par_iter()
        .map(|&(pos, key, idx)| {
            let metrics = sweep_group_serial(strategy, params, corpus, idx, observer)?;
            Ok((pos, make_group_result(key.clone(), idx.len(), metrics, coverage)))
        })
        .collect();
    for r in small_results {
        let (pos, gr) = r?;
        results[pos] = Some(gr);
    }

    Ok(results
        .into_iter()
        .map(|g| g.expect("every survivor slot filled by one phase"))
        .collect())
}

/// Run a grouped sweep, optionally with a coarse→refine second pass.
///
/// Without `refine`, this is a single [`run_grouped_sweep`] over `coarse`. With
/// it: sweep `coarse` **silently** (the bar tracks only the final pass) but
/// cancellably, take each group's top-`top_k` combos, ask the strategy for a
/// neighborhood around them ([`Strategy::refine`]), then sweep the deduped union
/// of the coarse combos and every neighborhood (this final pass drives the
/// progress bar). The union is capped at `cap` — coarse combos are kept first, so
/// the cap only ever trims refinement.
///
/// Returns the final combo list (its index is the `combo_id` of the per-group
/// results) and the per-group results, so the caller can emit one `params_json`
/// per surviving combo. Order is deterministic: coarse-then-neighborhood, both in
/// a deterministic order, deduped first-seen.
// One orchestration fn threading the same params the dispatch already carries;
// bundling them into a struct would only add indirection for two call sites.
#[allow(clippy::too_many_arguments)]
pub fn run_grouped_with_refine<S: Strategy>(
    strategy: &S,
    coarse: Vec<S::Params>,
    refine: Option<RefineSpec>,
    corpus: &Corpus,
    fields: &[GroupField],
    min_tokens: usize,
    coverage: CoverageFloor,
    cap: usize,
    observer: &dyn SweepObserver,
) -> Result<(Vec<S::Params>, Vec<GroupResult>)> {
    let Some(spec) = refine else {
        let groups =
            run_grouped_sweep(strategy, &coarse, corpus, fields, min_tokens, coverage, observer)?;
        return Ok((coarse, groups));
    };

    // Coarse pass — only used to locate each group's promising region, so it folds
    // silently (the final pass owns the bar) while still honouring a cancel.
    let coarse_groups = run_grouped_sweep(
        strategy,
        &coarse,
        corpus,
        fields,
        min_tokens,
        coverage,
        &CancelOnly(observer),
    )?;

    // Seed the neighborhood from each group's top-K coarse combos, deduped across
    // groups by params_json (groups overlap heavily on their best combos).
    let mut survivors: Vec<S::Params> = Vec::new();
    let mut seen_survivors: HashSet<String> = HashSet::new();
    for g in &coarse_groups {
        for id in top_combo_ids(&g.metrics, spec.top_k) {
            if let Some(p) = coarse.get(id as usize) {
                if seen_survivors.insert(strategy.params_json(p).to_string()) {
                    survivors.push(p.clone());
                }
            }
        }
    }
    let neighbors = strategy.refine(&survivors);

    // Union = coarse ++ neighborhood, deduped by params_json, capped (coarse kept
    // first so the cap only trims refinement, never the baseline coverage).
    let mut union: Vec<S::Params> = Vec::with_capacity(coarse.len() + neighbors.len());
    let mut seen: HashSet<String> = HashSet::new();
    for p in coarse.into_iter().chain(neighbors) {
        if union.len() >= cap {
            break;
        }
        if seen.insert(strategy.params_json(&p).to_string()) {
            union.push(p);
        }
    }

    tracing::info!(
        survivors = survivors.len(),
        combos = union.len(),
        cap,
        "coarse→refine: re-sweeping the union of coarse + per-group neighborhoods"
    );

    let groups =
        run_grouped_sweep(strategy, &union, corpus, fields, min_tokens, coverage, observer)?;
    Ok((union, groups))
}

/// The `k` best combo ids in a group's ranked metrics — ranked exactly as
/// [`best_combo`] ranks (robust `score`, then fired count, then total PnL), among
/// combos that fired at least once. The coarse→refine driver seeds each group's
/// neighborhood from these. Fewer than `k` are returned if fewer combos fired.
pub fn top_combo_ids(metrics: &[ComboMetrics], k: usize) -> Vec<u32> {
    let mut ranked: Vec<&ComboMetrics> = metrics.iter().filter(|m| m.n_fired > 0).collect();
    // `rank_combo(a, b) == Greater` means a is the better combo → sort descending.
    ranked.sort_by(|a, b| rank_combo(b, a));
    ranked.into_iter().take(k).map(|m| m.combo_id).collect()
}

/// Refcount-clone a group's tokens into a sub-`Corpus` (Arc trades — no buffer copy).
fn sub_corpus(corpus: &Corpus, idx: &[usize]) -> Corpus {
    Corpus {
        tokens: idx.iter().map(|&i| corpus.tokens[i].clone()).collect(),
        hash: corpus.hash.clone(),
    }
}

/// Fold one small group's tokens **single-threaded** into per-combo metrics. Used
/// inside the cross-group `par_iter`, so this must stay serial (no nested pool).
/// Resolves entries via the shared [`fill_outcomes`], so it matches `run_sweep`'s
/// decisions exactly. Bails on cancel (the caller discards the run).
fn sweep_group_serial<S: Strategy>(
    strategy: &S,
    params: &[S::Params],
    corpus: &Corpus,
    idx: &[usize],
    observer: &dyn SweepObserver,
) -> Result<Vec<ComboMetrics>> {
    let mut aggs = vec![ComboAgg::default(); params.len()];
    let mut outs: Vec<TokenOutcome> = Vec::with_capacity(params.len());
    for &i in idx {
        if observer.cancelled() {
            bail!("sweep cancelled");
        }
        if fill_outcomes(strategy, params, &corpus.tokens[i].trades, observer, &mut outs).is_err() {
            bail!("sweep cancelled");
        }
        for (combo_id, o) in outs.iter().enumerate() {
            aggs[combo_id].record(o);
        }
        observer.token_done();
    }
    Ok(aggs.into_iter().enumerate().map(|(id, a)| a.finalize(id as u32)).collect())
}

/// Assemble a [`GroupResult`] from a group's ranked metrics + the coverage floor.
fn make_group_result(
    key: GroupKey,
    token_count: usize,
    metrics: Vec<ComboMetrics>,
    coverage: CoverageFloor,
) -> GroupResult {
    let (best_combo_id, best_score, best_expectancy_sol) =
        best_combo(&metrics, token_count, coverage);
    GroupResult {
        key,
        token_count,
        metrics,
        best_combo_id,
        best_score,
        best_expectancy_sol,
    }
}

/// Best combo = max robust **realized** `score` (`μ−Z·σ/√n` over closed trades)
/// among combos clearing the [`CoverageFloor`] — the same metric the drill-in
/// table sorts by, so the crowned combo *is* row 1 of its own table. Ties break
/// by fired count, then total PnL. Returns `(combo_id, score, expectancy_sol)`.
///
/// If no combo clears the floor, fall back to the most-fired combo (a low-
/// confidence pick — logged) so the group still surfaces something. `(0, None,
/// 0.0)` only when no combo fired at all.
fn best_combo(
    metrics: &[ComboMetrics],
    group_tokens: usize,
    floor: CoverageFloor,
) -> (u32, Option<f64>, f64) {
    let threshold = floor.threshold(group_tokens);
    let eligible = metrics
        .iter()
        .filter(|m| m.n_fired >= threshold)
        .max_by(|a, b| rank_combo(a, b));
    if let Some(m) = eligible {
        return (m.combo_id, m.score, m.expectancy_sol);
    }

    // Nobody cleared the floor — the group is too thin for a trustworthy pick.
    // Surface the most-fired combo so the row isn't empty, but flag it.
    let fallback = metrics
        .iter()
        .filter(|m| m.n_fired > 0)
        .max_by(|a, b| {
            a.n_fired
                .cmp(&b.n_fired)
                .then_with(|| score_cmp(a.score, b.score))
                .then_with(|| a.total_pnl_sol.partial_cmp(&b.total_pnl_sol).unwrap_or(Equal))
        });
    match fallback {
        Some(m) => {
            tracing::warn!(
                group_tokens,
                threshold,
                combo_id = m.combo_id,
                n_fired = m.n_fired,
                "grouped sweep: no combo cleared the coverage floor; \
                 falling back to most-fired (low-confidence headline pick)"
            );
            (m.combo_id, m.score, m.expectancy_sol)
        }
        None => (0, None, 0.0),
    }
}

/// Rank two floor-clearing combos: higher robust `score` first (an absent score
/// — fewer than 2 closed trades — sorts as worst), then more fired tokens, then
/// higher total PnL.
fn rank_combo(a: &ComboMetrics, b: &ComboMetrics) -> Ordering {
    score_cmp(a.score, b.score)
        .then_with(|| a.n_fired.cmp(&b.n_fired))
        .then_with(|| a.total_pnl_sol.partial_cmp(&b.total_pnl_sol).unwrap_or(Equal))
}

/// Order two optional scores, higher = better, with `None` (no realized
/// evidence) treated as strictly worse than any `Some`.
fn score_cmp(a: Option<f64>, b: Option<f64>) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Equal),
        (Some(_), None) => Greater,
        (None, Some(_)) => Less,
        (None, None) => Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::trade::{Trade, TradeType};
    use crate::sweep::corpus::TokenTrades;
    use crate::sweep::grouping::TokenFingerprint;
    use crate::sweep::projection::SweepTrade;
    use crate::sweep::strategy::{ExitCode, ParamSpace, SweepMethod, TokenOutcome};
    use chrono::Utc;

    /// Fires on every token; PnL == the param value, so combo `i` has expectancy
    /// == params[i] and `best_combo` must pick the largest.
    struct Mock;
    impl ParamSpace for Mock {
        type Params = f64;
        fn sample(&self, _m: SweepMethod) -> Vec<f64> {
            vec![1.0, 3.0, 2.0]
        }
        /// Neighborhood = each survivor nudged up by 0.5 (a fresh, higher-PnL combo
        /// so the refine pass measurably changes the winner).
        fn refine(&self, survivors: &[f64]) -> Vec<f64> {
            survivors.iter().map(|p| p + 0.5).collect()
        }
    }
    impl Strategy for Mock {
        type Entry = bool;
        type EntryKey = ();
        type TokenState = ();
        fn id(&self) -> &'static str {
            "mock"
        }
        fn entry_key(&self, _p: &f64) {}
        fn prepare_token(&self, _trades: &[SweepTrade]) {}
        fn resolve_entry(&self, trades: &[SweepTrade], _state: &(), _p: &f64) -> bool {
            !trades.is_empty()
        }
        fn resolve_exit(&self, _trades: &[SweepTrade], entry: &bool, p: &f64) -> TokenOutcome {
            TokenOutcome {
                fired: *entry,
                holding_secs: 1,
                pnl_percent: *p as f32,
                pnl_sol: *p as f32,
                exit: ExitCode::TakeProfit,
            }
        }
        fn params_json(&self, p: &f64) -> serde_json::Value {
            serde_json::json!({ "x": p })
        }
    }

    fn token(mint: &str, creator: &str) -> TokenTrades {
        let t = Trade::new(
            mint.into(),
            "w".into(),
            TradeType::Buy,
            1.0,
            1.0,
            "sig".into(),
            1,
            Utc::now(),
        );
        TokenTrades::from_trades(
            mint.into(),
            mint.into(),
            TokenFingerprint {
                creator_wallet: creator.into(),
                ..Default::default()
            },
            &[t],
        )
    }

    /// A zeroed `ComboMetrics` to spread `..base_metrics()` over in best_combo
    /// tests — only the fields a test sets matter to the ranking.
    fn base_metrics() -> ComboMetrics {
        ComboMetrics {
            combo_id: 0,
            n_fired: 0,
            n_open: 0,
            n_closed: 0,
            win_rate: 0.0,
            total_pnl_sol: 0.0,
            mean_pnl_pct: 0.0,
            median_pnl_pct: 0.0,
            p90_pnl_pct: 0.0,
            best_pnl_pct: 0.0,
            worst_pnl_pct: 0.0,
            std_pnl_pct: 0.0,
            profit_factor: None,
            score: None,
            expectancy_sol: 0.0,
            avg_holding_secs: 0.0,
            median_holding_secs: 0.0,
            n_exit_take_profit: 0,
            n_exit_stop_loss: 0,
            n_exit_trailing: 0,
            n_exit_stall: 0,
            n_exit_time: 0,
            n_exit_liquidity: 0,
            n_exit_cohort: 0,
            n_exit_open: 0,
        }
    }

    fn corpus() -> Corpus {
        Corpus {
            tokens: vec![
                token("a", "devA"),
                token("b", "devA"),
                token("c", "devB"),
            ],
            hash: "h".into(),
        }
    }

    /// A permissive floor (any single fire is eligible) for the small fixtures.
    const OPEN_FLOOR: CoverageFloor = CoverageFloor { min_fired_abs: 1, fire_frac: 0.0 };

    #[test]
    fn groups_by_exact_creator_and_picks_best_combo() {
        use crate::sweep::grouping::GroupField;
        let params = Mock.sample(SweepMethod::Grid);
        let groups = run_grouped_sweep(
            &Mock,
            &params,
            &corpus(),
            &[GroupField::CreatorWallet],
            1,
            OPEN_FLOOR,
            &crate::sweep::progress::NoopObserver,
        )
        .unwrap();

        assert_eq!(groups.len(), 2, "devA + devB");
        // Largest group (devA, 2 tokens) sorts first.
        assert_eq!(groups[0].token_count, 2);
        assert_eq!(groups[1].token_count, 1);
        // Identical per-combo returns (σ=0) ⇒ score == realized mean == param, so
        // the winner is still params[1] = 3.0 → combo_id 1.
        assert_eq!(groups[0].best_combo_id, 1);
        assert_eq!(groups[0].best_score, Some(3.0));
        assert!((groups[0].best_expectancy_sol - 3.0).abs() < 1e-9);
    }

    #[test]
    fn large_and_small_groups_both_swept_in_deterministic_order() {
        // One group clears the large-group threshold (phase 1: intra-group
        // parallel) and one stays small (phase 2: cross-group parallel). Both must
        // be swept correctly and assembled in the deterministic largest-first
        // order regardless of which phase produced each slot.
        use crate::sweep::grouping::GroupField;
        let threads = rayon::current_num_threads().max(1);
        let big = LARGE_GROUP_TOKEN_FACTOR * threads + 1; // clears `large_min`
        let mut tokens: Vec<TokenTrades> =
            (0..big).map(|i| token(&format!("a{i}"), "devA")).collect();
        tokens.push(token("b0", "devB"));
        tokens.push(token("b1", "devB"));
        let corpus = Corpus { tokens, hash: "h".into() };

        let params = Mock.sample(SweepMethod::Grid);
        let groups = run_grouped_sweep(
            &Mock,
            &params,
            &corpus,
            &[GroupField::CreatorWallet],
            1,
            OPEN_FLOOR,
            &crate::sweep::progress::NoopObserver,
        )
        .unwrap();

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].token_count, big, "large group (phase 1) sorts first");
        assert_eq!(groups[1].token_count, 2, "small group (phase 2) second");
        // Both groups see identical per-combo returns ⇒ winner is params[1] = 3.0.
        assert_eq!(groups[0].best_combo_id, 1);
        assert_eq!(groups[1].best_combo_id, 1);
    }

    #[test]
    fn coverage_floor_excludes_thin_combos() {
        // Two combos: A fires on 1 token with a huge score, B on 2 with a modest
        // one. A floor of 2 fired tokens makes A ineligible → B wins despite the
        // lower score (the over-fit guard).
        let metrics = vec![
            ComboMetrics { combo_id: 0, n_fired: 1, score: Some(999.0), expectancy_sol: 9.0,
                total_pnl_sol: 9.0, ..base_metrics() },
            ComboMetrics { combo_id: 1, n_fired: 2, score: Some(5.0), expectancy_sol: 1.0,
                total_pnl_sol: 2.0, ..base_metrics() },
        ];
        let floor = CoverageFloor { min_fired_abs: 2, fire_frac: 0.0 };
        let (id, score, _) = best_combo(&metrics, 100, floor);
        assert_eq!(id, 1, "thin combo 0 is below the floor");
        assert_eq!(score, Some(5.0));
    }

    #[test]
    fn coverage_floor_falls_back_to_most_fired_when_none_clear() {
        // No combo clears the floor → fall back to the most-fired one.
        let metrics = vec![
            ComboMetrics { combo_id: 0, n_fired: 1, score: Some(1.0), expectancy_sol: 1.0,
                ..base_metrics() },
            ComboMetrics { combo_id: 1, n_fired: 3, score: Some(0.5), expectancy_sol: 0.5,
                ..base_metrics() },
        ];
        let floor = CoverageFloor { min_fired_abs: 10, fire_frac: 0.0 };
        let (id, _, _) = best_combo(&metrics, 100, floor);
        assert_eq!(id, 1, "most-fired fallback");
    }

    #[test]
    fn best_combo_ranks_on_score_not_expectancy() {
        // Combo 0 has the higher expectancy but a worse robust score (dispersion);
        // ranking on score must crown combo 1.
        let metrics = vec![
            ComboMetrics { combo_id: 0, n_fired: 50, score: Some(1.0), expectancy_sol: 9.0,
                ..base_metrics() },
            ComboMetrics { combo_id: 1, n_fired: 50, score: Some(4.0), expectancy_sol: 2.0,
                ..base_metrics() },
        ];
        let (id, score, exp) = best_combo(&metrics, 100, CoverageFloor::default());
        assert_eq!(id, 1);
        assert_eq!(score, Some(4.0));
        assert!((exp - 2.0).abs() < 1e-9);
    }

    #[test]
    fn min_tokens_drops_small_groups_before_sweeping() {
        use crate::sweep::grouping::GroupField;
        let params = Mock.sample(SweepMethod::Grid);
        let groups = run_grouped_sweep(
            &Mock,
            &params,
            &corpus(),
            &[GroupField::CreatorWallet],
            2,
            OPEN_FLOOR,
            &crate::sweep::progress::NoopObserver,
        )
        .unwrap();
        assert_eq!(groups.len(), 1, "only devA (2 tokens) clears min_tokens=2");
        assert_eq!(groups[0].token_count, 2);
    }

    #[test]
    fn top_combo_ids_ranks_by_score_and_skips_unfired() {
        let metrics = vec![
            ComboMetrics { combo_id: 0, n_fired: 5, score: Some(1.0), ..base_metrics() },
            ComboMetrics { combo_id: 1, n_fired: 5, score: Some(9.0), ..base_metrics() },
            ComboMetrics { combo_id: 2, n_fired: 0, score: None, ..base_metrics() },
            ComboMetrics { combo_id: 3, n_fired: 5, score: Some(4.0), ..base_metrics() },
        ];
        // Best→worst among fired combos: 1 (9), 3 (4), 0 (1); combo 2 never fired.
        assert_eq!(top_combo_ids(&metrics, 2), vec![1, 3]);
        assert_eq!(top_combo_ids(&metrics, 10), vec![1, 3, 0]);
    }

    #[test]
    fn refine_none_is_a_plain_grouped_sweep() {
        let coarse = Mock.sample(SweepMethod::Grid);
        let (final_params, groups) = run_grouped_with_refine(
            &Mock,
            coarse,
            None,
            &corpus(),
            &[crate::sweep::grouping::GroupField::CreatorWallet],
            1,
            OPEN_FLOOR,
            100,
            &crate::sweep::progress::NoopObserver,
        )
        .unwrap();
        assert_eq!(final_params.len(), 3, "combo set unchanged without refine");
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn refine_grows_combo_set_and_resweeps_union() {
        use crate::sweep::strategy::RefineSpec;
        // Single ALL group of 3 tokens. Coarse best = 3.0 (combo 1); top_k=1 seeds
        // the neighborhood with 3.0 → Mock.refine adds 3.5; union = [1,3,2,3.5].
        let coarse = Mock.sample(SweepMethod::Grid);
        let (final_params, groups) = run_grouped_with_refine(
            &Mock,
            coarse,
            Some(RefineSpec { top_k: 1 }),
            &corpus(),
            &[],
            1,
            OPEN_FLOOR,
            100,
            &crate::sweep::progress::NoopObserver,
        )
        .unwrap();
        assert_eq!(final_params.len(), 4, "neighborhood combo appended to the union");
        assert_eq!(groups.len(), 1);
        // The refined combo (3.5, index 3) now wins.
        assert_eq!(groups[0].best_combo_id, 3);
        assert_eq!(groups[0].best_score, Some(3.5));
    }

    #[test]
    fn refine_union_is_capped_coarse_kept_first() {
        use crate::sweep::strategy::RefineSpec;
        // cap = 3 == coarse size, so the neighborhood can't fit — the union is just
        // the coarse combos (kept first), proving the cap never trims baseline cover.
        let coarse = Mock.sample(SweepMethod::Grid);
        let (final_params, _groups) = run_grouped_with_refine(
            &Mock,
            coarse,
            Some(RefineSpec { top_k: 1 }),
            &corpus(),
            &[],
            1,
            OPEN_FLOOR,
            3,
            &crate::sweep::progress::NoopObserver,
        )
        .unwrap();
        assert_eq!(final_params.len(), 3, "cap trims refinement, not the coarse combos");
    }

    #[test]
    fn empty_fields_is_single_all_group() {
        let params = Mock.sample(SweepMethod::Grid);
        let groups = run_grouped_sweep(
            &Mock,
            &params,
            &corpus(),
            &[],
            1,
            OPEN_FLOOR,
            &crate::sweep::progress::NoopObserver,
        )
        .unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].token_count, 3);
    }
}
