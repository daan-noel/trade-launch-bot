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
//! Each sub-corpus is a refcount-clone of `CorpusToken` (`trades` is an `Arc`, so
//! no trade buffer is copied). Empty `fields` ⇒ a single "ALL" group ⇒ identical
//! to a global ungrouped sweep.

use std::cmp::Ordering::{self, Equal, Greater, Less};
use std::collections::{HashMap, HashSet};

use anyhow::{bail, Result};
use rayon::prelude::*;
use serde_json::Value;

use crate::sweep::aggregate::{ComboAgg, ComboMetrics};
use crate::sweep::corpus::Corpus;
use crate::sweep::engine::{combo_batch_size, run_sweep};
use crate::sweep::grouping::{group_key, GroupField, GroupKey, SolPrecision};
use crate::sweep::progress::SweepObserver;
use crate::sweep::strategy::{RefineSpec, Strategy, TokenOutcome};

/// A group with at least `this × pool_threads` tokens is swept with **intra-group**
/// parallelism (its own `run_sweep` saturates the pool); smaller groups are swept
/// **across** groups in parallel instead. The factor gives a large group enough
/// tokens that rayon's per-token parallelism pays for its fold-thread overhead.
const LARGE_GROUP_TOKEN_FACTOR: usize = 4;

/// Per-combo coverage floor for the group winner: a combo must fire on
/// `max(min_fired_abs, ceil(fire_frac · group_tokens))` tokens before it's
/// eligible to be crowned `best_combo`. Stops a combo that fired on a lucky 2
/// tokens out of 200 from out-ranking a combo proven over 150 — the over-fit
/// failure mode this floor exists to prevent.
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
/// ranked metrics, and the winning combo. The winner is chosen on the checklist
/// `score` (the same metric the drill-in table sorts by) among combos clearing
/// the [`CoverageFloor`] — see [`best_combo`].
pub struct GroupResult {
    pub key: GroupKey,
    pub token_count: usize,
    /// The per-combo ranked metrics. **Emptied after a [`GroupSink`] persists the
    /// group** (see [`free_persisted_metrics`]) — so on the persisted path only the
    /// headline fields below survive into the returned vector; read `metrics` inside
    /// the sink's `group_done`, not after the sweep. Populated throughout when no
    /// sink consumes the group (the coarse refine pass / tests using `NoopSink`).
    pub metrics: Vec<ComboMetrics>,
    /// Combo id maximising the checklist `score` among combos clearing the
    /// coverage floor (see [`best_combo`]).
    pub best_combo_id: u32,
    /// The winning combo's checklist `score`, or `None` when it never fired.
    /// The page's headline metric.
    pub best_score: Option<f64>,
    /// The winning combo's expectancy per trade — a secondary readout, not the
    /// ranking metric.
    pub best_expectancy_sol: f64,
    /// Pass-2 winners: `combo_id -> ExitStage[]` for every combo where
    /// [`Strategy::post_group_rescore`](crate::sweep::strategy::Strategy::post_group_rescore)
    /// found a candidate ladder that beat that combo's OWN Pass-1 baseline. Absent
    /// entry ⇒ either not rescored, or every candidate ladder lost to the combo's
    /// own exit (kept as-is). Read by [`retained_combo_params`] to bake each
    /// combo's own winning ladder directly into its persisted `params` — **not**
    /// a run-wide ladder, since the grid search picks independently per combo.
    /// See `docs/arch/sweep.md` (*Pass-2 overlay*).
    pub scale_out_winners: HashMap<u32, serde_json::Value>,
}

/// Sink for **incremental** per-group results (incremental persistence). The
/// engine calls [`begin`](GroupSink::begin) once — after the surviving group set
/// and the final (possibly refine-expanded) combo set are fixed, before any group
/// fires — then [`group_done`](GroupSink::group_done) once per **fully-folded**
/// group. A half-folded group is never emitted (the engine bails on cancel before
/// the emit), so a sink may treat every `group_done` as a complete, persistable
/// group.
///
/// `group_done` fires in deterministic order for large groups but in arrival
/// order for the cross-group small-group phase, so it may be called concurrently
/// from rayon workers — implementors must be `Sync` and self-serialize (e.g. hand
/// off to a single writer task). Each call carries the deterministic `group_index`
/// regardless of arrival order. `combo_params[combo_id]` is the param JSON for
/// each ranked combo in the group's metrics.
pub trait GroupSink: Sync {
    /// Whether the caller wants per-group emits at all. A `false` sink (e.g. the
    /// silent coarse refine pass) lets the engine skip building the per-combo
    /// param JSON and the emit calls entirely. Defaults to `true`.
    fn wants_groups(&self) -> bool {
        true
    }
    /// Fired once before the first group with surviving group/combo counts.
    /// Combo JSON is **not** materialised here — only retained survivors are
    /// inserted per [`group_done`](GroupSink::group_done).
    fn begin(&self, _group_count: usize, _combo_count: usize) {}
    /// Fired once per fully-folded group. `retained_params` is `(combo_id, params_json)`
    /// for the retention set (+ best) only — never the full grid.
    fn group_done(
        &self,
        group_index: usize,
        group: &GroupResult,
        retained_params: &[(u32, Value)],
    );

    /// One group could **not** be folded, and the run carried on without it.
    ///
    /// The driver isolates non-cancel per-group errors instead of aborting the whole
    /// sweep (see [`run_grouped_sweep`]), so this is how a sink learns the run is
    /// honestly incomplete — a run that silently dropped a group and still reported
    /// `completed` would be worse than the abort it replaced. Cancellation is not a
    /// group failure and never reaches here.
    fn group_failed(&self, _group_index: usize, _err: &str) {}
}

/// A sink that discards every group — the silent coarse refine pass and any
/// caller that doesn't persist incrementally use this.
pub struct NoopSink;
impl GroupSink for NoopSink {
    fn wants_groups(&self) -> bool {
        false
    }
    fn group_done(
        &self,
        _index: usize,
        _group: &GroupResult,
        _retained_params: &[(u32, Value)],
    ) {
    }
}

/// Partition token indices by exact-value group key at bucket `width` (the per-run
/// partition width for the continuous SOL fields; see [`group_key`]). Pure
/// `O(tokens)` pass.
pub fn partition(corpus: &Corpus, fields: &[GroupField], precision: SolPrecision) -> HashMap<GroupKey, Vec<usize>> {
    let mut groups: HashMap<GroupKey, Vec<usize>> = HashMap::new();
    for (i, tt) in corpus.tokens.iter().enumerate() {
        groups.entry(group_key(&tt.fp, fields, precision)).or_default().push(i);
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
// One extra param (`sink`) past clippy's threshold; threading it through a struct
// would only add indirection for the two internal call sites.
#[allow(clippy::too_many_arguments)]
pub fn run_grouped_sweep<S: Strategy>(
    strategy: &S,
    params: &[S::Params],
    corpus: &Corpus,
    fields: &[GroupField],
    precision: SolPrecision,
    min_tokens: usize,
    coverage: CoverageFloor,
    observer: &dyn SweepObserver,
    sink: &dyn GroupSink,
) -> Result<Vec<GroupResult>> {
    let floor = min_tokens.max(1);
    // Decorate with the tie-break key once (JSON-serializing inside the comparator
    // re-ran it O(n log n) times); sort, then drop the decoration.
    let surviving: Vec<(GroupKey, Vec<usize>)> = {
        let _stage = crate::sweep::obs::Stage::start("partition");
        let mut surviving: Vec<(String, GroupKey, Vec<usize>)> = partition(corpus, fields, precision)
            .into_iter()
            .filter(|(_, idx)| idx.len() >= floor)
            .map(|(key, idx)| (key.to_json().to_string(), key, idx))
            .collect();
        // Deterministic group order: most-populated first, ties broken by key JSON.
        surviving.sort_by(|a, b| b.2.len().cmp(&a.2.len()).then_with(|| a.0.cmp(&b.0)));
        surviving.into_iter().map(|(_, key, idx)| (key, idx)).collect()
    };

    let threads = rayon::current_num_threads().max(1);
    let large_min = LARGE_GROUP_TOKEN_FACTOR * threads;

    // sweep the combo space in budget-sized batches so peak accumulator
    // memory is `threads × batch × ComboAgg`, independent of the total combo count.
    // The batch is global (same for every group + pass), but it — and how `run_sweep`
    // shards the combo space — are **memory-timed** decisions the engine re-makes
    // against live free RAM as it folds. So progress is measured in the one unit that
    // is invariant to all that chunking: **(token, combo) evaluations**. Every combo
    // is evaluated against every token exactly once, so the total is simply
    // `Σ group_tokens × n_combos` regardless of how RAM split it into batches/shards,
    // and `token_done`'s per-chunk increments always sum back to it (no more predicting
    // the shard plan up front, which drifted from the real plan and overran the bar).
    let batch = combo_batch_size(params.len(), threads);
    let n_combos = params.len();
    let total_tokens: usize = surviving.iter().map(|(_, idx)| idx.len()).sum();
    observer.set_total(total_tokens, n_combos);

    // RSS at the structural peak (corpus + the partition map resident)
    // and a wall-clock origin for the per-sweep duration, both logged again at done.
    let sweep_started = std::time::Instant::now();
    tracing::info!(
        groups = surviving.len(),
        n_fields = fields.len(),
        min_tokens = floor,
        combos = n_combos,
        tokens = total_tokens,
        batch,
        eval_total = (total_tokens as u64).saturating_mul(n_combos as u64),
        rss_mb = crate::sweep::obs::process_rss_mb(),
        "grouped sweep: partitioned corpus, sweeping each group"
    );

    // Fill survivor slots by position so the deterministic group order survives
    // regardless of which phase produced each result.
    let mut results: Vec<Option<GroupResult>> = (0..surviving.len()).map(|_| None).collect();

    // per-group incremental emit. Announce counts only; combo JSON is
    // built per group for retained survivors (~660) — never the full N-combo grid.
    let emit = sink.wants_groups();
    if emit {
        sink.begin(surviving.len(), params.len());
    }

    // Phase 1 — large groups: intra-group parallel, one group at a time.
    let failed = std::sync::atomic::AtomicUsize::new(0);
    // Slowest single group, rather than a line per group: at ~1k groups, per-group
    // logging is noise that buries the one number worth acting on. Locked only on a
    // new maximum, once per group — never on the fold path.
    let slowest = SlowestGroup::default();
    for (pos, (key, idx)) in surviving.iter().enumerate() {
        if idx.len() < large_min {
            continue;
        }
        if observer.cancelled() {
            bail!("sweep cancelled");
        }
        let sub = sub_corpus(corpus, idx);
        let group_started = std::time::Instant::now();
        // Per-group failure isolation: a group that cannot fold costs the run *that
        // group*, not the whole sweep. Aborting here would discard every group
        // already folded — strictly worse than finishing the other groups and
        // reporting the run honestly partial.
        let metrics = match run_sweep(strategy, params, &sub, observer, batch) {
            Ok((_stats, metrics)) => metrics,
            Err(e) => {
                // Cancel is not a group failure — it must still abort the run.
                if observer.cancelled() {
                    bail!("sweep cancelled");
                }
                note_group_failure(pos, idx.len(), &e, observer, sink, emit, &failed);
                continue;
            }
        };
        // A cancel mid-group leaves the just-swept metrics partial — discard.
        if observer.cancelled() {
            bail!("sweep cancelled");
        }
        let mut gr = make_group_result(key.clone(), idx.len(), metrics, coverage);
        // Pass 2 only on the persist path — coarse refine seeds neighborhoods from
        // baseline ranks (no need to pay staged resolve twice).
        if emit {
            strategy.post_group_rescore(params, corpus, idx, &mut gr, coverage, observer)?;
            let retained = retained_combo_params(strategy, params, &gr);
            sink.group_done(pos, &gr, &retained);
            free_persisted_metrics(&mut gr);
        }
        // Timed through retention + emit, not just the fold: retention runs in the
        // same worker and is a real per-group cost (see `retained_combo_params`).
        slowest.observe(group_started.elapsed().as_secs_f64(), pos, idx.len());
        results[pos] = Some(gr);
    }

    // Phase 2 — small groups: parallel across groups (each folded single-threaded),
    // so the pool stays saturated even when every group is tiny.
    let small: Vec<(usize, &GroupKey, &Vec<usize>)> = surviving
        .iter()
        .enumerate()
        .filter(|(_, (_, idx))| idx.len() < large_min)
        .map(|(pos, (key, idx))| (pos, key, idx))
        .collect();
    // Same isolation as the large phase: `Ok(None)` = this group failed and was
    // skipped; `Err` is reserved for cancellation, which still aborts the run.
    let small_results: Vec<Result<(usize, Option<GroupResult>)>> = small
        .par_iter()
        .map(|&(pos, key, idx)| {
            let group_started = std::time::Instant::now();
            let metrics = match sweep_group_serial(strategy, params, corpus, idx, observer, batch) {
                Ok(m) => m,
                Err(e) => {
                    if observer.cancelled() {
                        return Err(e);
                    }
                    note_group_failure(pos, idx.len(), &e, observer, sink, emit, &failed);
                    return Ok((pos, None));
                }
            };
            let mut gr = make_group_result(key.clone(), idx.len(), metrics, coverage);
            // Pass 2 only on the persist path (see large-group branch).
            if emit {
                strategy.post_group_rescore(params, corpus, idx, &mut gr, coverage, observer)?;
                let retained = retained_combo_params(strategy, params, &gr);
                sink.group_done(pos, &gr, &retained);
                free_persisted_metrics(&mut gr);
            }
            slowest.observe(group_started.elapsed().as_secs_f64(), pos, idx.len());
            Ok((pos, Some(gr)))
        })
        .collect();
    for r in small_results {
        let (pos, gr) = r?;
        results[pos] = gr;
    }

    // Failed groups leave their slot empty, so the survivor vec is filtered, not
    // indexed — `expect`ing a full vec here would turn an isolated group failure back
    // into the run-wide panic this isolation exists to prevent.
    let groups: Vec<GroupResult> = results.into_iter().flatten().collect();
    let failed = failed.load(std::sync::atomic::Ordering::Relaxed);
    if failed > 0 {
        tracing::warn!(
            groups = groups.len(),
            failed,
            "grouped sweep: some groups failed to fold — run is honestly partial"
        );
    }
    let (slow_secs, slow_pos, slow_tokens) = slowest.take().unwrap_or((0.0, 0, 0));
    tracing::info!(
        groups = groups.len(),
        failed,
        // The one per-group number worth surfacing: a single group taking a large
        // share of the run is the signature of a skewed partition, and it points at
        // exactly which group to reproduce.
        slowest_group_secs = slow_secs,
        slowest_group_index = slow_pos,
        slowest_group_tokens = slow_tokens,
        rss_mb = crate::sweep::obs::process_rss_mb(),
        elapsed_s = sweep_started.elapsed().as_secs_f64(),
        "grouped sweep: all groups folded"
    );
    Ok(groups)
}

/// Run a grouped sweep, optionally with a coarse→refine second pass.
///
/// Without `refine`, this is a single [`run_grouped_sweep`] over `coarse`. With
/// it: sweep `coarse` using `coarse_observer` (so the coarse pass has its own
/// progress bar), take each group's top-`top_k` combos, ask the strategy for a
/// neighborhood around them ([`Strategy::refine`]), then sweep the deduped union
/// of the coarse combos and every neighborhood (this final pass uses `observer`).
/// The union is capped at `cap` — coarse combos are kept first, so the cap only
/// ever trims refinement.
///
/// `coarse_observer` is only meaningful when `refine.is_some()`; in the no-refine
/// path `observer` is used and `coarse_observer` is ignored. Returns the final
/// combo list (its index is the `combo_id` of the per-group results) and the
/// per-group results, so the caller can emit one `params_json` per surviving combo.
/// Order is deterministic: coarse-then-neighborhood, both in a deterministic
/// order, deduped first-seen.
// One orchestration fn threading the same params the dispatch already carries;
// bundling them into a struct would only add indirection for two call sites.
#[allow(clippy::too_many_arguments)]
pub fn run_grouped_with_refine<S: Strategy>(
    strategy: &S,
    mut coarse: Vec<S::Params>,
    refine: Option<RefineSpec>,
    corpus: &Corpus,
    fields: &[GroupField],
    precision: SolPrecision,
    min_tokens: usize,
    coverage: CoverageFloor,
    cap: usize,
    coarse_observer: &dyn SweepObserver,
    observer: &dyn SweepObserver,
    sink: &dyn GroupSink,
) -> Result<(Vec<S::Params>, Vec<GroupResult>)> {
    // Group same-entry combos contiguously so the engine's single-slot entry cache
    // hits maximally regardless of sampler (grid/random/lhs/refine) — a no-op for
    // param-free entries. Decision-neutral: only the evaluation order changes, and
    // combo_id (= position) stays consistent across this run's groups.
    strategy.order_for_entry_cache(&mut coarse);
    let Some(spec) = refine else {
        // No refine: the single pass is the final one — persist its groups.
        //
        // P0 gate (AVX-512 exit-scan plan): time the fold as its own stage so a
        // grid/random run — the common case — logs the corpus-load-vs-fold split,
        // not just an undifferentiated block between `corpus_loaded` and `done`.
        // The exit-scan speedup is Amdahl-capped by this stage's share of wall-clock,
        // so this is the number that decides whether vectorizing the scan is worth it.
        let _stage = crate::sweep::obs::Stage::start("sweep_pass");
        let groups = run_grouped_sweep(
            strategy, &coarse, corpus, fields, precision, min_tokens, coverage, observer, sink,
        )?;
        return Ok((coarse, groups));
    };

    // Coarse pass — only used to locate each group's promising region. Reports
    // progress through `coarse_observer` so the frontend can show a "Coarse sweep"
    // bar. Its groups are throwaway (the combo-id space isn't final yet), so they
    // are NOT persisted: a cancel during coarse leaves no checkpointable group → a
    // full cancel.
    // A refine run sweeps the whole corpus TWICE. Timing the two passes separately is
    // the only way to tell which half a slow run spent its time in; timed together
    // they collapse into one block between `corpus_loaded` and `done`.
    let coarse_groups = {
        let _stage = crate::sweep::obs::Stage::start("refine_coarse_pass");
        run_grouped_sweep(
            strategy,
            &coarse,
            corpus,
            fields,
            precision,
            min_tokens,
            coverage,
            coarse_observer,
            &NoopSink,
        )?
    };

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
    // Free the coarse metrics NOW, before the final pass allocates its own.
    //
    // The coarse pass runs with `NoopSink`, so `free_persisted_metrics` (which only
    // fires on the emit path) never trims these — `coarse_groups` holds
    // `n_groups × n_combos` `ComboMetrics` and would otherwise stay resident straight
    // through the final, memory-heaviest sweep. Everything still needed from it
    // (`survivors`) has been extracted above.
    drop(coarse_groups);
    drop(seen_survivors);
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
    // Re-group the grown union so the final (persisted, combo-id-fixed) pass keeps
    // its same-entry combos contiguous too.
    strategy.order_for_entry_cache(&mut union);

    tracing::info!(
        survivors = survivors.len(),
        combos = union.len(),
        cap,
        "coarse→refine: re-sweeping the union of coarse + per-group neighborhoods"
    );

    // Final pass owns the bar and produces the persistable groups (combo-id space
    // now fixed), so it carries the real sink.
    let groups = {
        let _stage = crate::sweep::obs::Stage::start("refine_final_pass");
        run_grouped_sweep(
            strategy, &union, corpus, fields, precision, min_tokens, coverage, observer, sink,
        )?
    };
    Ok((union, groups))
}

/// The `k` best combo ids in a group's ranked metrics — ranked exactly as
/// [`best_combo`] ranks (robust `score`, then fired count, then marked PnL), among
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
        has_fingerprints: corpus.has_fingerprints,
        candidates_capped: corpus.candidates_capped,
    }
}

/// Fold one small group's tokens **single-threaded** into per-combo metrics. Used
/// inside the cross-group `par_iter`, so this must stay serial (no nested pool).
/// Resolves entries via the shared [`fill_outcomes_with_state`], so it matches
/// `run_sweep`'s decisions exactly. Bails on cancel (the caller discards the run).
///
/// **Series is built → folded → dropped per token** — never a whole-group cache.
/// Each token's `TokenState` (the generic engine's heavy `MetricSeries`) lives only
/// for its own fold, so this fn holds **one** series at a time and the cross-group
/// `par_iter` peaks at `threads × one series` (mirrors the large path's bounded
/// wave). The old code cached every token's series to group-end (`group_tokens ×
/// series`), so with `threads` groups in flight the pool held ~`4·threads²` series
/// at once — the RAM blow-up that OOM'd 16 GB boxes on the redesigned engine (with
/// the legacy `()` state the cache was free, so it was safe before).
///
/// **Loop order is picked per group** (the small-path analogue of the large path's
/// wave-outer/pass-outer choice):
/// - **Token-outer** when every worker can hold the full `n_combos × ComboAgg`
///   accumulator set at once (`full_combo_aggs_fit` scaled by `threads`, since
///   `threads` of these serial folds run concurrently): each token's series is
///   built **exactly once**, combos folded over it in `batch`-sized chunks (the
///   chunking only bounds the `TokenOutcome` scratch buffer). This avoids the
///   multi-batch series rebuild — an `n_batches×` multiplier on the dominant
///   series-build cost.
/// - **Batch-outer fallback** when the full accumulator set does NOT fit: peak
///   accumulator memory stays `batch × ComboAgg` per worker, at the cost of
///   rebuilding each series once per batch — the same CPU-for-RAM trade the large
///   path's pass-outer branch makes. (With a single batch the two orders are
///   identical, so the fallback only ever pays on over-RAM multi-batch runs.)
///
/// Either way combo ids stay global (`offset + local`) and progress is reported in
/// evaluations (`Σ token_done(chunk) = group_tokens × n_combos`), so the observer
/// total is loop-order-invariant.
fn sweep_group_serial<S: Strategy>(
    strategy: &S,
    params: &[S::Params],
    corpus: &Corpus,
    idx: &[usize],
    observer: &dyn SweepObserver,
    batch: usize,
) -> Result<Vec<ComboMetrics>> {
    use crate::sweep::engine::{fill_outcomes_with_state, full_combo_aggs_fit};

    let n_combos = params.len();
    // Sizing, pinned for this group (see `run_sweep_unsharded`): the `aggs` vec and every
    // combo chunk below are sized from it, so it is read once, not per pass.
    let batch =
        batch.clamp(1, n_combos.max(1).min(crate::sweep::registry::preferred_max_combo_batch()));
    let n_batches = n_combos.div_ceil(batch.max(1)).max(1);

    // Token-outer (series once per token) whenever the whole accumulator set fits
    // across all concurrently-folding workers. Only a multi-batch group differs
    // between the orders, but token-outer is also the natural single-batch shape,
    // so it is the primary path.
    if n_batches == 1 || {
        let threads = rayon::current_num_threads().max(1);
        let max_series = idx
            .iter()
            .map(|&i| strategy.token_state_bytes_estimate(&corpus.tokens[i]))
            .max()
            .unwrap_or(0);
        // Scale combos by `threads` so the agg term models every worker's resident
        // set; the `threads` wave models one live series per worker. The token-outer
        // branch binds the whole combo set per worker, so its `BoundParams` are priced
        // alongside the accumulators rather than left to the alloc slack.
        let fits = full_combo_aggs_fit(
            n_combos.saturating_mul(threads),
            threads,
            max_series,
            std::mem::size_of::<S::BoundParams>(),
        );
        // The fit inputs ride along on both arms: when the fallback fires, "which
        // term blew the budget" is the only question worth asking, and recomputing
        // it from the message alone is impossible.
        if fits {
            tracing::debug!(
                group_tokens = idx.len(),
                combos = n_combos,
                n_batches,
                threads,
                max_series_kb = max_series / 1024,
                "grouped sweep: small-group token-outer fold (series built once per token)"
            );
        } else {
            tracing::debug!(
                group_tokens = idx.len(),
                combos = n_combos,
                n_batches,
                threads,
                max_series_kb = max_series / 1024,
                agg_mb = (n_combos.saturating_mul(threads).saturating_mul(
                    std::mem::size_of::<ComboAgg>() + std::mem::size_of::<S::BoundParams>(),
                )) / (1024 * 1024),
                series_mb = threads.saturating_mul(max_series) / (1024 * 1024),
                fold_budget_mb = crate::sweep::registry::sweep_memory_budget_bytes() / (1024 * 1024),
                usable_mb = crate::sweep::registry::usable_host_bytes()
                    .map(|b| b / (1024 * 1024))
                    .unwrap_or(0),
                "grouped sweep: small-group batch-outer fallback (full aggs over-RAM; \
                 series rebuilt per batch)"
            );
        }
        fits
    } {
        let bound: Vec<S::BoundParams> = params.iter().map(|p| strategy.bind_param(p)).collect();
        let mut aggs = vec![ComboAgg::default(); n_combos];
        let mut outs: Vec<TokenOutcome> = Vec::with_capacity(batch.min(n_combos));
        for &ti in idx.iter() {
            if observer.cancelled() {
                bail!("sweep cancelled");
            }
            // Build the token's series ONCE; every combo chunk folds over it, then
            // it drops — peak resident series stays one per worker.
            let state = strategy.prepare_token(&corpus.tokens[ti]);
            for (b, chunk) in params.chunks(batch).enumerate() {
                let offset = b * batch;
                if fill_outcomes_with_state(
                    strategy,
                    chunk,
                    &bound[offset..offset + chunk.len()],
                    &corpus.tokens[ti],
                    &state,
                    observer,
                    &mut outs,
                )
                .is_err()
                {
                    bail!("sweep cancelled");
                }
                for (j, o) in outs.iter().enumerate() {
                    aggs[offset + j].record(o);
                }
                // One token × `chunk` combos evaluated; sums to the same
                // `group_tokens × n_combos` total as the batch-outer order.
                observer.token_done(chunk.len());
            }
        }
        return Ok(aggs
            .into_iter()
            .enumerate()
            .map(|(i, a)| a.finalize(i as u32))
            .collect());
    }

    // Batch-outer fallback: bounded accumulators (`batch × ComboAgg`), series
    // rebuilt once per batch.
    let mut metrics: Vec<ComboMetrics> = Vec::with_capacity(n_combos);
    for (b, chunk) in params.chunks(batch).enumerate() {
        let offset = b * batch;
        let bound: Vec<S::BoundParams> = chunk.iter().map(|p| strategy.bind_param(p)).collect();
        let mut aggs = vec![ComboAgg::default(); chunk.len()];
        let mut outs: Vec<TokenOutcome> = Vec::with_capacity(chunk.len());
        for &ti in idx.iter() {
            if observer.cancelled() {
                bail!("sweep cancelled");
            }
            // Build the token's series for this fold only; it is dropped at the end
            // of the iteration, so peak resident series is one per worker.
            let state = strategy.prepare_token(&corpus.tokens[ti]);
            if fill_outcomes_with_state(
                strategy,
                chunk,
                &bound,
                &corpus.tokens[ti],
                &state,
                observer,
                &mut outs,
            )
            .is_err()
            {
                bail!("sweep cancelled");
            }
            for (combo_id, o) in outs.iter().enumerate() {
                aggs[combo_id].record(o);
            }
            // Report this fold's evaluations (one token × `chunk` combos); summed over
            // the group's tokens and batches this equals `group_tokens × n_combos`.
            observer.token_done(chunk.len());
        }
        metrics.extend(
            aggs.into_iter()
                .enumerate()
                .map(|(i, a)| a.finalize((offset + i) as u32)),
        );
    }
    Ok(metrics)
}

/// Tracks the single slowest group of a run so a pathological group is identifiable
/// without a log line per group (at ~1k groups that would bury the signal).
#[derive(Default)]
struct SlowestGroup {
    /// `(secs, group_index, group_tokens)` of the worst group seen so far.
    worst: std::sync::Mutex<Option<(f64, usize, usize)>>,
}

impl SlowestGroup {
    /// Offer one group's fold time; keeps it only if it is the new maximum. Called
    /// once per group (never from the fold loop), so the lock is uncontended.
    fn observe(&self, secs: f64, pos: usize, tokens: usize) {
        if let Ok(mut w) = self.worst.lock() {
            if w.is_none_or(|(prev, _, _)| secs > prev) {
                *w = Some((secs, pos, tokens));
            }
        }
    }

    fn take(&self) -> Option<(f64, usize, usize)> {
        self.worst.lock().ok().and_then(|w| *w)
    }
}

/// Record one group's fold failure and let the run continue: log it, count it, tell
/// the sink (so the run is finalized honestly partial) and push a notice to the
/// operator's stream (so a thinner result set is explained, not mysterious).
///
/// One SSOT for both driver phases, so a large group and a small group that fail are
/// reported identically. Callers must have already ruled out cancellation.
#[allow(clippy::too_many_arguments)]
fn note_group_failure(
    pos: usize,
    group_tokens: usize,
    err: &anyhow::Error,
    observer: &dyn SweepObserver,
    sink: &dyn GroupSink,
    emit: bool,
    failed: &std::sync::atomic::AtomicUsize,
) {
    let n = failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    tracing::error!(
        group_index = pos,
        group_tokens,
        failed_so_far = n,
        "grouped sweep: group failed to fold — skipping it and continuing: {err}"
    );
    if emit {
        sink.group_failed(pos, &err.to_string());
    }
    // Only the first few reach the operator; a systemic failure would otherwise
    // spam the SSE stream with one toast per group.
    if n <= 3 {
        observer.notice(&format!(
            "group {pos} ({group_tokens} tokens) failed and was skipped — the run continues \
             without it: {err}"
        ));
    }
}

/// Build `(combo_id, params_json)` only for retention survivors (+ best).
fn retained_combo_params<S: Strategy>(
    strategy: &S,
    params: &[S::Params],
    gr: &GroupResult,
) -> Vec<(u32, Value)> {
    let cfg = crate::sweep::retention::RetentionCfg::default();
    let keep = crate::sweep::retention::retained_combo_ids(&gr.metrics, gr.best_combo_id, &cfg);
    let mut out: Vec<(u32, Value)> = keep
        .iter()
        .filter_map(|&id| {
            let p = params.get(id as usize)?;
            let mut v = strategy.params_json(p);
            // Bake this combo's OWN Pass-2 winner (if any) directly into its persisted
            // params — see `GroupResult::scale_out_winners`. Every downstream reader
            // (drill-in, promote, the group's `best_params`) reads this one column, so
            // there is no separate run-wide merge step anymore.
            if let Some(ladder) = gr.scale_out_winners.get(&id) {
                if let Value::Object(obj) = &mut v {
                    obj.insert("scale_out".into(), ladder.clone());
                }
            }
            Some((id, v))
        })
        .collect();
    out.sort_by_key(|(id, _)| *id);
    out.dedup_by_key(|(id, _)| *id);
    out
}

/// Drop a group's per-combo `metrics` once a [`GroupSink`] has persisted them.
///
/// This is what makes the driver's bounded-memory claim true: groups stream out to
/// the sink one at a time, so retaining every emitted group's full `Vec<ComboMetrics>`
/// in the returned vector would hold the whole sweep's combos × groups resident — at
/// a large combo set (a `random:N`/`refine` run near `HARD_MAX_COMBOS`) that's GBs,
/// and was OOM-aborting the process even though every group was already on disk. The
/// sole post-sweep reader of the returned groups (the handler) only wants `.len()`,
/// so the heavy field is freed here; the small headline fields (`token_count`,
/// `best_*`) stay. Only called on the emit path — the coarse refine pass uses
/// `NoopSink` (no emit) and keeps its metrics for `top_combo_ids`.
fn free_persisted_metrics(gr: &mut GroupResult) {
    gr.metrics = Vec::new();
}

/// Assemble a [`GroupResult`] from a group's ranked metrics + the coverage floor.
/// Rewrites each combo's checklist `score` with `matched = token_count` before
/// crowning the winner so fire-rate reflects group coverage.
fn make_group_result(
    key: GroupKey,
    token_count: usize,
    mut metrics: Vec<ComboMetrics>,
    coverage: CoverageFloor,
) -> GroupResult {
    for m in &mut metrics {
        m.rescore_for_group(token_count);
    }
    let (best_combo_id, best_score, best_expectancy_sol) =
        best_combo(&metrics, token_count, coverage);
    GroupResult {
        key,
        token_count,
        metrics,
        best_combo_id,
        best_score,
        best_expectancy_sol,
        scale_out_winners: HashMap::new(),
    }
}

/// Best combo = max checklist `score` among combos clearing the [`CoverageFloor`]
/// — the same metric the drill-in table sorts by, so the crowned combo *is* row 1
/// of its own table. Ties break by fired count, then [`marked_pnl_sol`].
/// Returns `(combo_id, score, expectancy_sol)`.
///
/// If no combo clears the floor, fall back to the most-fired combo (a low-
/// confidence pick — logged) so the group still surfaces something. `(0, None,
/// 0.0)` only when no combo fired at all.
pub(crate) fn best_combo(
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
                .then_with(|| pnl_cmp(a, b))
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

/// Rank two floor-clearing combos: higher checklist `score` first (absent score
/// sorts as worst), then more fired tokens, then higher [`marked_pnl_sol`].
///
/// `pub(crate)` so [`Strategy::post_group_rescore`](crate::sweep::strategy::Strategy::post_group_rescore)
/// impls can compare a Pass-2 candidate against a combo's own baseline with the
/// exact same ordering `best_combo`/`top_combo_ids` use — a strategy re-implementing
/// this comparison could silently drift from the ranking the rest of the sweep uses.
pub(crate) fn rank_combo(a: &ComboMetrics, b: &ComboMetrics) -> Ordering {
    score_cmp(a.score, b.score)
        .then_with(|| a.n_fired.cmp(&b.n_fired))
        .then_with(|| pnl_cmp(a, b))
}

/// A combo's PnL **including** the mark on positions still open at the run's
/// `as_of` — realized `total_pnl_sol` plus unrealized `open_pnl_sol`. Used as the
/// score tie-break (score already folds opens into its MTM% term).
fn marked_pnl_sol(m: &ComboMetrics) -> f64 {
    m.total_pnl_sol + m.open_pnl_sol
}

/// Compare two combos by [`marked_pnl_sol`], higher = better.
fn pnl_cmp(a: &ComboMetrics, b: &ComboMetrics) -> Ordering {
    marked_pnl_sol(a).partial_cmp(&marked_pnl_sol(b)).unwrap_or(Equal)
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
    use crate::sweep::corpus::CorpusToken;
    use crate::sweep::grouping::TokenFingerprint;
    use crate::sweep::projection::CorpusTrade;
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
        type EntryCands = ();
        type TokenState = ();
        type BoundParams = ();
        type ExitCtx = ();
        type ExitCtxKey = ();
        fn entry_key(&self, _p: &f64) {}
        fn bind_param(&self, _p: &f64) {}
        fn exit_ctx_key(&self, _bound: &(), _entry: &bool) {}
        fn prepare_token(&self, _token: &crate::sweep::corpus::CorpusToken) {}
        fn resolve_entry(&self, trades: &[CorpusTrade], _state: &(), _bound: &(), _p: &f64) -> bool {
            !trades.is_empty()
        }
        fn resolve_exit(
            &self,
            _trades: &[CorpusTrade],
            _state: &(),
            _bound: &(),
            entry: &bool,
            p: &f64,
            _ctx: &(),
        ) -> TokenOutcome {
            TokenOutcome {
                fired: *entry,
                holding_secs: 1,
                pnl_percent: *p as f32,
                pnl_sol: *p as f32,
                exit: ExitCode::TakeProfit,
                exit_metric: None,
                exit_operator: None,
                exit_metric_value: None,
                exit_metric_window: None,
                exit_metric_slot: None,
                entry_time: None,
                entry_price: None,
                entry_slot: None,
                exit_time: None,
                exit_price: None,
                exit_slot: None,
            }
        }
        fn params_json(&self, p: &f64) -> serde_json::Value {
            serde_json::json!({ "x": p })
        }
    }

    fn token(mint: &str, program: &str) -> CorpusToken {
        let t = Trade::new(
            mint.into(),
            "w".into(),
            TradeType::Buy,
            1.0,
            1,
            "sig".into(),
            1,
            Utc::now(),
        );
        CorpusToken::from_trades(
            mint.into(),
            mint.into(),
            Utc::now(),
            TokenFingerprint {
                token_program_id: Some(program.into()),
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
            open_pnl_sol: 0.0,
            mean_pnl_pct: 0.0,
            median_pnl_pct: 0.0,
            p90_pnl_pct: 0.0,
            best_pnl_pct: 0.0,
            worst_pnl_pct: 0.0,
            std_pnl_pct: 0.0,
            profit_factor: None,
            mtm_pnl_pct: 0.0,
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
            n_exit_dead: 0,
            n_exit_metrics: 0,
            n_exit_metrics_by_slot: [0; crate::sweep::strategy::N_EXIT_METRIC_SLOTS],
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
            has_fingerprints: false,
            candidates_capped: false,
        }
    }

    /// A permissive floor (any single fire is eligible) for the small fixtures.
    const OPEN_FLOOR: CoverageFloor = CoverageFloor { min_fired_abs: 1, fire_frac: 0.0 };
    /// Default per-run bucket width for these tests (grouping fields here are all
    /// discrete, so the exact width is immaterial — it just satisfies the signature).
    /// These grouping fields are all discrete, so the precision is immaterial here —
    /// it just satisfies the signature.
    const WIDTH: crate::sweep::grouping::SolPrecision =
        crate::sweep::grouping::SolPrecision::Bucket(crate::sweep::grouping::SOL_BUCKET_WIDTH);

    #[test]
    fn groups_by_exact_field_and_picks_best_combo() {
        use crate::sweep::grouping::GroupField;
        let params = Mock.sample(SweepMethod::Grid);
        let groups = run_grouped_sweep(
            &Mock,
            &params,
            &corpus(),
            &[GroupField::TokenProgramId],
            WIDTH,
            1,
            OPEN_FLOOR,
            &crate::sweep::progress::NoopObserver,
            &NoopSink,
        )
        .unwrap();

        assert_eq!(groups.len(), 2, "devA + devB");
        // Largest group (devA, 2 tokens) sorts first.
        assert_eq!(groups[0].token_count, 2);
        assert_eq!(groups[1].token_count, 1);
        // All fire, no opens, WR=1 ⇒ score == mtm_pnl_pct == param, so
        // the winner is still params[1] = 3.0 → combo_id 1.
        assert_eq!(groups[0].best_combo_id, 1);
        assert_eq!(groups[0].best_score, Some(3.0));
        assert!((groups[0].best_expectancy_sol - 3.0).abs() < 1e-9);
    }

    #[test]
    fn sweep_group_serial_is_batch_invariant() {
        // The token-outer fold chunks the combo space in `batch`-sized passes and
        // records each into `aggs[offset + j]`. A single-batch fold and a
        // per-combo (batch=1) fold must produce byte-identical per-combo metrics —
        // proving the offset bookkeeping and the chunked inner loop don't corrupt
        // the aggregation. (Mock's tiny agg + zero series means both runs stay on
        // the token-outer path; batch only changes how the pass loop chunks.)
        let c = corpus(); // 3 tokens; every combo fires on each
        let idx: Vec<usize> = (0..c.tokens.len()).collect();
        let params = Mock.sample(SweepMethod::Grid); // [1.0, 3.0, 2.0]

        let whole =
            sweep_group_serial(&Mock, &params, &c, &idx, &crate::sweep::progress::NoopObserver, 3)
                .unwrap();
        let chunked =
            sweep_group_serial(&Mock, &params, &c, &idx, &crate::sweep::progress::NoopObserver, 1)
                .unwrap();

        assert_eq!(whole.len(), 3);
        assert_eq!(chunked.len(), 3);
        for (w, ch) in whole.iter().zip(chunked.iter()) {
            assert_eq!(w.combo_id, ch.combo_id, "combo_id offsets must match across batching");
            assert_eq!(w.n_fired, ch.n_fired);
            assert_eq!(w.n_closed, ch.n_closed);
            assert!((w.total_pnl_sol - ch.total_pnl_sol).abs() < 1e-9);
            assert_eq!(w.score.is_some(), ch.score.is_some());
        }
        // combo 1 (param 3.0) is still the highest-PnL combo under both batchings.
        assert_eq!(whole[1].combo_id, 1);
        assert!((whole[1].total_pnl_sol - 3.0 * 3.0).abs() < 1e-9, "3 tokens × 3.0 PnL");
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
        let mut tokens: Vec<CorpusToken> =
            (0..big).map(|i| token(&format!("a{i}"), "devA")).collect();
        tokens.push(token("b0", "devB"));
        tokens.push(token("b1", "devB"));
        let corpus = Corpus {
            tokens,
            hash: "h".into(),
            has_fingerprints: false,
            candidates_capped: false,
        };

        let params = Mock.sample(SweepMethod::Grid);
        let groups = run_grouped_sweep(
            &Mock,
            &params,
            &corpus,
            &[GroupField::TokenProgramId],
            WIDTH,
            1,
            OPEN_FLOOR,
            &crate::sweep::progress::NoopObserver,
            &NoopSink,
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
    fn pnl_tiebreak_counts_open_positions() {
        // Same score and fire count, so the PnL tie-break decides. Combo 0 banked
        // more realized PnL but is sitting on a big unrealized loss; combo 1's
        // marked PnL is higher. Ranking realized-only would crown 0.
        let metrics = vec![
            ComboMetrics { combo_id: 0, n_fired: 50, score: Some(1.0),
                total_pnl_sol: 5.0, open_pnl_sol: -20.0, ..base_metrics() },
            ComboMetrics { combo_id: 1, n_fired: 50, score: Some(1.0),
                total_pnl_sol: 2.0, open_pnl_sol: 0.0, ..base_metrics() },
        ];
        assert!(marked_pnl_sol(&metrics[0]) < marked_pnl_sol(&metrics[1]));
        let (id, _, _) = best_combo(&metrics, 100, CoverageFloor::default());
        assert_eq!(id, 1, "open loss must drag the marked PnL below the smaller realized win");
    }

    #[test]
    fn score_still_outranks_marked_pnl() {
        // The open mark only moves the tie-break: a better robust score wins even
        // when the loser's marked PnL is far larger.
        let metrics = vec![
            ComboMetrics { combo_id: 0, n_fired: 50, score: Some(1.0),
                total_pnl_sol: 0.0, open_pnl_sol: 999.0, ..base_metrics() },
            ComboMetrics { combo_id: 1, n_fired: 50, score: Some(4.0),
                total_pnl_sol: 1.0, open_pnl_sol: 0.0, ..base_metrics() },
        ];
        let (id, _, _) = best_combo(&metrics, 100, CoverageFloor::default());
        assert_eq!(id, 1, "score stays the primary key");
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
            &[GroupField::TokenProgramId],
            WIDTH,
            2,
            OPEN_FLOOR,
            &crate::sweep::progress::NoopObserver,
            &NoopSink,
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
            &[crate::sweep::grouping::GroupField::TokenProgramId],
            WIDTH,
            1,
            OPEN_FLOOR,
            100,
            &crate::sweep::progress::NoopObserver,
            &crate::sweep::progress::NoopObserver,
            &NoopSink,
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
            WIDTH,
            1,
            OPEN_FLOOR,
            100,
            &crate::sweep::progress::NoopObserver,
            &crate::sweep::progress::NoopObserver,
            &NoopSink,
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
            WIDTH,
            1,
            OPEN_FLOOR,
            3,
            &crate::sweep::progress::NoopObserver,
            &crate::sweep::progress::NoopObserver,
            &NoopSink,
        )
        .unwrap();
        assert_eq!(final_params.len(), 3, "cap trims refinement, not the coarse combos");
    }

    /// A sink that records every emit so a test can assert the engine fired
    /// `begin` once and one `group_done` per surviving group, with the param JSON.
    #[derive(Default)]
    struct RecordingSink {
        begins: std::sync::Mutex<Vec<(usize, usize)>>,
        groups: std::sync::Mutex<Vec<(usize, usize)>>, // (group_index, combo_params.len())
    }
    impl GroupSink for RecordingSink {
        fn begin(&self, group_count: usize, combo_count: usize) {
            self.begins.lock().unwrap().push((group_count, combo_count));
        }
        fn group_done(
            &self,
            group_index: usize,
            _g: &GroupResult,
            retained_params: &[(u32, Value)],
        ) {
            self.groups
                .lock()
                .unwrap()
                .push((group_index, retained_params.len()));
        }
    }

    #[test]
    fn sink_emits_begin_once_and_one_group_done_per_group() {
        use crate::sweep::grouping::GroupField;
        let params = Mock.sample(SweepMethod::Grid); // 3 combos
        let sink = RecordingSink::default();
        let groups = run_grouped_sweep(
            &Mock,
            &params,
            &corpus(), // devA(2) + devB(1) → 2 surviving groups
            &[GroupField::TokenProgramId],
            WIDTH,
            1,
            OPEN_FLOOR,
            &crate::sweep::progress::NoopObserver,
            &sink,
        )
        .unwrap();

        assert_eq!(groups.len(), 2);
        // begin fired exactly once with (surviving groups, combo count).
        assert_eq!(*sink.begins.lock().unwrap(), vec![(2, 3)]);
        // One group_done per group with retained-only params (≤ full combo count).
        let mut done = sink.groups.lock().unwrap().clone();
        done.sort();
        assert_eq!(done.len(), 2);
        assert_eq!(done[0].0, 0);
        assert_eq!(done[1].0, 1);
        assert!(done[0].1 >= 1 && done[0].1 <= 3);
        assert!(done[1].1 >= 1 && done[1].1 <= 3);
    }

    #[test]
    fn refine_persists_only_final_pass_groups() {
        use crate::sweep::strategy::RefineSpec;
        // Single ALL group, refine on: the coarse pass must NOT emit (its combo-id
        // space is throwaway); only the final union pass persists. So exactly one
        // begin + one group_done, with the *grown* union combo count (4, not 3).
        let coarse = Mock.sample(SweepMethod::Grid); // 3
        let sink = RecordingSink::default();
        let (final_params, groups) = run_grouped_with_refine(
            &Mock,
            coarse,
            Some(RefineSpec { top_k: 1 }),
            &corpus(),
            &[],
            WIDTH,
            1,
            OPEN_FLOOR,
            100,
            &crate::sweep::progress::NoopObserver,
            &crate::sweep::progress::NoopObserver,
            &sink,
        )
        .unwrap();
        assert_eq!(final_params.len(), 4, "union grew by the refined neighbor");
        assert_eq!(groups.len(), 1);
        assert_eq!(*sink.begins.lock().unwrap(), vec![(1, 4)], "only the final pass emits");
        let done = sink.groups.lock().unwrap().clone();
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].0, 0);
        assert!(done[0].1 >= 1 && done[0].1 <= 4);
    }

    #[test]
    fn empty_fields_is_single_all_group() {
        let params = Mock.sample(SweepMethod::Grid);
        let groups = run_grouped_sweep(
            &Mock,
            &params,
            &corpus(),
            &[],
            WIDTH,
            1,
            OPEN_FLOOR,
            &crate::sweep::progress::NoopObserver,
            &NoopSink,
        )
        .unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].token_count, 3);
    }
}
