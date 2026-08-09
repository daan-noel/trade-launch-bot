//! The sweep core: `rayon`-parallel **over tokens**, all combos evaluated inner
//! so each token's trade slice stays cache-resident across the whole combo set.
//! `simulate` returns a `Copy` [`TokenOutcome`] — no per-call heap. A single
//! writer thread folds outcomes into one [`ComboAgg`] per combo, so combos ×
//! tokens rows are never buffered: the sweep yields `combos` ranked rows.

use anyhow::Result;
use rayon::prelude::*;

use crate::sweep::aggregate::{ComboAgg, ComboMetrics};
use crate::sweep::corpus::Corpus;
use crate::sweep::progress::SweepObserver;
use crate::sweep::projection::CorpusTrade;
use crate::sweep::strategy::{Strategy, TokenOutcome};

/// How many combos one token folds between cancel polls. Small enough that a
/// cancel lands sub-100ms even on a huge combo set, large enough that the atomic
/// load is amortised to noise against the inner `simulate` work.
const CANCEL_CHECK_STRIDE: usize = 256;

/// Resolve every combo's outcome for one token into `out` (cleared first), using
/// a **precomputed** [`Strategy::TokenState`]. Shared by the wave driver (series
/// once per token) and the legacy `fill_outcomes` wrapper.
///
/// **Every combo resolves its own entry** ([`Strategy::resolve_entry_from`]); only
/// the exit-independent Stage-A candidates ([`Strategy::entry_candidates`]) are
/// cached per [`Strategy::entry_key`]. Caching the *resolved* entry by that key —
/// which this fn did until 2026-07-26 — was the entry-cache poisoning bug: the
/// engine's `can_enter` veto ("never buy while the exit conditions already hold")
/// makes the entry exit-dependent, so the first combo of each entry class donated
/// its entered set to every sibling. See the [`Strategy`] trait doc and
/// `docs/plans/sweep/sim-parity.md`; the lock is
/// `generic::guard::fold_gives_each_exit_variant_its_own_entry`.
pub(crate) fn fill_outcomes_with_state<S: Strategy>(
    strategy: &S,
    params: &[S::Params],
    bound: &[S::BoundParams],
    token: &crate::sweep::corpus::CorpusToken,
    token_state: &S::TokenState,
    observer: &dyn SweepObserver,
    out: &mut Vec<TokenOutcome>,
) -> std::result::Result<(), ()> {
    assert_eq!(params.len(), bound.len());
    out.clear();
    let trades: &[CorpusTrade] = &token.trades;
    // Recycled Stage-A candidates + the key they were computed for. Both are reused
    // in place across the combo loop, so an entry class costs one walk, not one
    // alloc per combo.
    let mut cands = S::EntryCands::default();
    let mut cands_key: Option<S::EntryKey> = None;
    // Recycled per-token exit context (generic: prefix-extrema hulls). Rebuilt in
    // place when the entry the combo resolved needs a different one — NOT when the
    // entry key changes: entries now move within a class, and the hulls are anchored
    // on the fill row.
    let mut exit_ctx = S::ExitCtx::default();
    let mut ctx_key: Option<S::ExitCtxKey> = None;
    for (chunk_i, chunk) in params.chunks(CANCEL_CHECK_STRIDE).enumerate() {
        if observer.cancelled() {
            return Err(());
        }
        let base = chunk_i * CANCEL_CHECK_STRIDE;
        for (j, p) in chunk.iter().enumerate() {
            let i = base + j;
            let key = strategy.entry_key(p);
            if cands_key.as_ref() != Some(&key) {
                if observer.cancelled() {
                    return Err(());
                }
                strategy.entry_candidates(trades, token_state, &bound[i], p, &mut cands);
                cands_key = Some(key);
            }
            let entry = strategy.resolve_entry_from(trades, token_state, &bound[i], p, &mut cands);
            let key = strategy.exit_ctx_key(&bound[i], &entry);
            if ctx_key.as_ref() != Some(&key) {
                strategy.build_exit_ctx(trades, token_state, &bound[i], &entry, p, &mut exit_ctx);
                ctx_key = Some(key);
            }
            out.push(strategy.resolve_exit(trades, token_state, &bound[i], &entry, p, &exit_ctx));
        }
    }
    Ok(())
}

/// Resolve every combo's outcome for one token's `trades` into `out` (cleared
/// first), one [`TokenOutcome`] per combo in `params` order. Calls
/// [`Strategy::prepare_token`] once then [`fill_outcomes_with_state`]. Prefer the
/// wave driver in [`run_sweep`] for heavy `TokenState` (generic `MetricSeries`) so
/// series is not rebuilt per combo pass.
///
/// A convenience wrapper used by the engine's own unit tests; the production
/// drivers call [`fill_outcomes_with_state`] directly so they can hoist
/// `prepare_token` out of the combo loop.
#[cfg(test)]
pub(crate) fn fill_outcomes<S: Strategy>(
    strategy: &S,
    params: &[S::Params],
    token: &crate::sweep::corpus::CorpusToken,
    observer: &dyn SweepObserver,
    out: &mut Vec<TokenOutcome>,
) -> std::result::Result<(), ()> {
    let bound: Vec<S::BoundParams> = params.iter().map(|p| strategy.bind_param(p)).collect();
    let token_state = strategy.prepare_token(token);
    fill_outcomes_with_state(strategy, params, &bound, token, &token_state, observer, out)
}

/// Headline counts for a completed sweep. Read by the engine's correctness tests
/// (row/fired counts); production callers discard it.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug)]
pub struct SweepStats {
    pub tokens: usize,
    pub combos: usize,
    pub rows: u64,
    pub fired: u64,
}

/// Largest combo batch whose fold peak stays within the memory budget .
///
/// With the series-once wave driver, peak for one pass ≈
///   `batch × sizeof(ComboAgg)` (folder)
/// + `inflight × batch × sizeof(TokenOutcome)` (wave producers + bounded queue)
/// + `wave × TokenState` (held across all combo passes for that wave)
///
/// Batch sizing still accounts for ComboAgg + outcome buffers; series are capped
/// separately by [`crate::sweep::registry::series_wave_size`]. Hard-capped at
/// [`HARD_MAX_COMBO_BATCH`].
pub fn combo_batch_size(n_combos: usize, threads: usize) -> usize {
    if n_combos == 0 {
        return 1;
    }
    let threads = threads.max(1) as u64;
    let inflight = threads.saturating_mul(3).max(4);
    let per = (std::mem::size_of::<ComboAgg>() as u64)
        .saturating_add(inflight.saturating_mul(std::mem::size_of::<TokenOutcome>() as u64))
        .max(1);
    let budget = crate::sweep::registry::sweep_memory_budget_bytes();
    let max_batch = (budget / per).max(1) as usize;
    let preferred = crate::sweep::registry::preferred_max_combo_batch();
    max_batch.min(n_combos).min(preferred).max(1)
}

/// Resident fold-buffer bytes for **one worker** at `batch` combos: the `batch`
/// `ComboAgg` accumulators plus the single `batch`-sized `TokenOutcome` scratch
/// buffer the serial fold reuses across the group's tokens.
///
/// The cross-group grouped driver ([`crate::sweep::grouped_engine`]) runs one such
/// serial fold per worker concurrently, so the admission guard multiplies this by
/// `threads`. This is the **actual** residency of the small-group path (one accumulator
/// vec + one outcome vec per worker) — deliberately NOT `combo_batch_size`'s per-combo
/// *sizing* model, whose `inflight × TokenOutcome` term reserves the large path's
/// producer/consumer channel depth. Folding that inflight factor in here and then
/// multiplying by `threads` double-counts it (it made the guard estimate a fold peak
/// ~`threads×` too large and false-reject runs that fit fine). The large path's channel
/// is bounded separately (`channel_depth ≤ 32`, one group at a time) and RAM-admitted
/// via `max_parallel_shards`. Strategy `BoundParams` (e.g. a `CompiledRule`) are also
/// batch-resident but opaque to this generic engine; the guard's alloc slack absorbs
/// them for the generic sweep's rule sizes.
pub fn fold_footprint_bytes(batch: usize) -> u64 {
    let per = (std::mem::size_of::<ComboAgg>() as u64)
        .saturating_add(std::mem::size_of::<TokenOutcome>() as u64);
    (batch as u64).saturating_mul(per)
}

/// Absolute ceiling on one fold batch's combo count — independent of the byte
/// budget. Prevents a single `vec![ComboAgg; batch]` from demanding hundreds of MB
/// of contiguous address space on a RAM-fragmented workstation.
pub(crate) const HARD_MAX_COMBO_BATCH: usize = 65_536;

/// Number of `batch`-sized passes needed to cover `n_combos` combos (≥ 1) — the loop
/// count for the fold's combo chunking (progress is tracked separately, in combo
/// evaluations, so it no longer depends on this).
pub fn combo_batch_count(n_combos: usize, batch: usize) -> usize {
    n_combos.div_ceil(batch.max(1)).max(1)
}

/// Whether holding `n_combos` [`ComboAgg`]s + their bound params + one series wave
/// fits usable RAM. When true, [`run_sweep`] uses **wave-outer** (series once per
/// token).
///
/// `bound_bytes_per_combo` prices the `BoundParams` (a `CompiledRule` for the generic
/// sweep) that both wave-outer paths hold for the **whole** combo set — the shard-wide
/// `bound_all` here, and the group-wide `bound` in the grouped driver's token-outer
/// fold. Pass `size_of::<S::BoundParams>()`. That is the inline size only; a
/// `SmallVec` that spills to the heap is absorbed by the 256 MB slack below, the same
/// way this term used to be absorbed entirely when it was merely batch-resident.
pub fn full_combo_aggs_fit(
    n_combos: usize,
    wave: usize,
    max_series_bytes: usize,
    bound_bytes_per_combo: usize,
) -> bool {
    let per_combo =
        (std::mem::size_of::<ComboAgg>() as u64).saturating_add(bound_bytes_per_combo as u64);
    let agg = (n_combos as u64).saturating_mul(per_combo);
    let series = (wave as u64).saturating_mul(max_series_bytes as u64);
    let fold = crate::sweep::registry::sweep_memory_budget_bytes();
    let need = agg.saturating_add(series).saturating_add(fold);
    match crate::sweep::registry::usable_host_bytes() {
        Some(usable) => need <= usable.saturating_sub(256 * 1024 * 1024),
        // Unknown host RAM: only fit small full-agg sets (legacy-safe).
        None => agg <= 64 * 1024 * 1024,
    }
}

/// Run every combo against every token; fold into one ranked [`ComboMetrics`] per combo.
///
/// Large combo spaces are **sharded** ([`crate::sweep::shard`]): each shard runs
/// wave-outer or pass-outer under the RAM ceiling, spills metrics to disk, then
/// shards merge. Pass-outer also streams finalized batches through
/// [`crate::sweep::spill`] so peak metrics RAM stays O(batch).
pub fn run_sweep<S: Strategy>(
    strategy: &S,
    params: &[S::Params],
    corpus: &Corpus,
    observer: &dyn SweepObserver,
    batch: usize,
) -> Result<(SweepStats, Vec<ComboMetrics>)> {
    let n_combos = params.len();
    if n_combos > HARD_MAX_COMBOS_GUARD {
        anyhow::bail!(
            "combo count {n_combos} exceeds hard guard {HARD_MAX_COMBOS_GUARD} — \
             refuse to allocate fold buffers"
        );
    }
    if n_combos == 0 {
        return Ok((
            SweepStats {
                tokens: corpus.token_count(),
                combos: 0,
                rows: 0,
                fired: 0,
            },
            Vec::new(),
        ));
    }

    let threads = rayon::current_num_threads().max(1);
    let max_series = corpus
        .tokens
        .iter()
        .map(|t| strategy.token_state_bytes_estimate(t))
        .max()
        .unwrap_or(0);
    let wave = crate::sweep::registry::series_wave_size(max_series, threads);
    let shards = crate::sweep::shard::plan_shards(
        n_combos,
        wave,
        max_series,
        std::mem::size_of::<S::BoundParams>(),
    );

    if shards.len() == 1 {
        return run_sweep_unsharded(strategy, params, corpus, observer, batch, 0);
    }

    let shard_len = shards.first().map(|r| r.len()).unwrap_or(1);
    let parallel =
        crate::sweep::shard::max_parallel_shards(shard_len, wave, max_series, threads);
    tracing::info!(
        combos = n_combos,
        shards = shards.len(),
        parallel,
        wave,
        max_series_mb = max_series / (1024 * 1024),
        "sweep: combo sharding (spill+merge)"
    );

    let mut spill_paths = Vec::with_capacity(shards.len());
    let mut total_rows = 0u64;
    let mut total_fired = 0u64;

    for chunk in shards.chunks(parallel) {
        if observer.cancelled() {
            break;
        }
        let chunk_results: Vec<Result<(u64, u64, std::path::PathBuf)>> = chunk
            .par_iter()
            .map(|range| {
                let (stats, metrics) = run_sweep_unsharded(
                    strategy,
                    &params[range.clone()],
                    corpus,
                    observer,
                    batch,
                    range.start as u32,
                )?;
                let path = crate::sweep::spill::write_metrics_spill(&metrics)?;
                Ok((stats.rows, stats.fired, path))
            })
            .collect();

        for r in chunk_results {
            let (rows, fired, path) = r?;
            total_rows += rows;
            total_fired += fired;
            spill_paths.push(path);
        }
    }

    let metrics = crate::sweep::spill::merge_spill_paths(spill_paths)?;
    Ok((
        SweepStats {
            tokens: corpus.token_count(),
            combos: n_combos,
            rows: total_rows,
            fired: total_fired,
        },
        metrics,
    ))
}

/// One shard (or the whole combo set when unsharded). `id_base` is added to every
/// finalized `combo_id` so shards stitch into a global index space.
fn run_sweep_unsharded<S: Strategy>(
    strategy: &S,
    params: &[S::Params],
    corpus: &Corpus,
    observer: &dyn SweepObserver,
    batch: usize,
    id_base: u32,
) -> Result<(SweepStats, Vec<ComboMetrics>)> {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let n_combos = params.len();
    // Sizing, read once and **pinned for this shard**: every `params.chunks(batch)`
    // pass below — and the `aggs` vec sized from it — must agree on one width. Live RAM
    // pressure re-reads on the *next* shard/group, which is where degradation belongs.
    let batch =
        batch.clamp(1, n_combos.max(1).min(crate::sweep::registry::preferred_max_combo_batch()));
    let threads = rayon::current_num_threads().max(1);
    let max_series = corpus
        .tokens
        .iter()
        .map(|t| strategy.token_state_bytes_estimate(t))
        .max()
        .unwrap_or(0);
    let wave = crate::sweep::registry::series_wave_size(max_series, threads);
    let wave_outer = full_combo_aggs_fit(
        n_combos,
        wave,
        max_series,
        std::mem::size_of::<S::BoundParams>(),
    );
    let n_batches = combo_batch_count(n_combos, batch);
    tracing::debug!(
        combos = n_combos,
        batch,
        n_batches,
        wave,
        wave_outer,
        id_base,
        "sweep: shard starting"
    );

    let series_builds = AtomicUsize::new(0);
    let mut total_rows = 0u64;
    let mut total_fired = 0u64;

    let metrics = if wave_outer {
        let mut aggs = vec![ComboAgg::default(); n_combos];
        // Bind ONCE for the whole shard, not once per wave.
        //
        // This was nested inside the wave loop, so every combo was re-compiled for
        // every wave: `n_waves × n_combos` calls, where `n_waves = group_tokens /
        // threads`. A 10k-token group on 16 threads with 100k combos re-compiled
        // 62.5M `CompiledRule`s for 100k distinct params — each one allocating a
        // `RuleParams` with nested condition maps. The params don't vary with the
        // token wave, so the work was pure repetition. (The pass-outer branch below
        // already hoisted its bind correctly; only this branch had the loops nested
        // the wrong way.) `wave_outer` is admitted with the bound set priced in, so
        // holding all `n_combos` of them is part of the plan, not a surprise.
        let bound_all: Vec<S::BoundParams> =
            params.iter().map(|p| strategy.bind_param(p)).collect();
        for wave_tokens in corpus.tokens.chunks(wave) {
            if observer.cancelled() {
                break;
            }
            let states: Vec<S::TokenState> = wave_tokens
                .par_iter()
                .map(|t| {
                    series_builds.fetch_add(1, Ordering::Relaxed);
                    strategy.prepare_token(t)
                })
                .collect();
            for (pass_i, chunk) in params.chunks(batch).enumerate() {
                if observer.cancelled() {
                    break;
                }
                let offset = pass_i * batch;
                let (rows, fired) = fold_wave_into(
                    strategy,
                    chunk,
                    &bound_all[offset..offset + chunk.len()],
                    wave_tokens,
                    &states,
                    &mut aggs[offset..offset + chunk.len()],
                    observer,
                )?;
                total_rows += rows;
                total_fired += fired;
            }
        }
        aggs.into_iter()
            .enumerate()
            .map(|(i, a)| a.finalize(id_base + i as u32))
            .collect()
    } else {
        // Stream finalized batches to disk so peak metrics RAM stays O(batch).
        let mut spill = crate::sweep::spill::MetricsSpill::create()?;
        let mut written = 0usize;
        for (pass_i, chunk) in params.chunks(batch).enumerate() {
            if observer.cancelled() {
                let pad: Vec<_> = (written..n_combos)
                    .map(|i| ComboAgg::default().finalize(id_base + i as u32))
                    .collect();
                spill.push_batch(&pad)?;
                written = n_combos;
                break;
            }
            // Static invariant (see `fold_wave_into`): `chunk` comes from the batch this
            // shard already pinned, so it must be judged against the constant, never
            // against a live RAM reading that may have moved since the pin.
            if chunk.len() > HARD_MAX_COMBO_BATCH {
                anyhow::bail!(
                    "combo pass length {} exceeds HARD_MAX_COMBO_BATCH {}",
                    chunk.len(),
                    HARD_MAX_COMBO_BATCH
                );
            }
            let mut aggs = vec![ComboAgg::default(); chunk.len()];
            let offset = pass_i * batch;
            let bound: Vec<S::BoundParams> =
                chunk.iter().map(|p| strategy.bind_param(p)).collect();

            for wave_tokens in corpus.tokens.chunks(wave) {
                if observer.cancelled() {
                    break;
                }
                let states: Vec<S::TokenState> = wave_tokens
                    .par_iter()
                    .map(|t| {
                        series_builds.fetch_add(1, Ordering::Relaxed);
                        strategy.prepare_token(t)
                    })
                    .collect();
                let (rows, fired) = fold_wave_into(
                    strategy,
                    chunk,
                    &bound,
                    wave_tokens,
                    &states,
                    &mut aggs,
                    observer,
                )?;
                total_rows += rows;
                total_fired += fired;
            }

            let batch_metrics: Vec<_> = aggs
                .into_iter()
                .enumerate()
                .map(|(i, a)| a.finalize(id_base + (offset + i) as u32))
                .collect();
            written += batch_metrics.len();
            spill.push_batch(&batch_metrics)?;
        }
        if written < n_combos {
            let pad: Vec<_> = (written..n_combos)
                .map(|i| ComboAgg::default().finalize(id_base + i as u32))
                .collect();
            spill.push_batch(&pad)?;
        }
        spill.finish_load()?
    };

    let builds = series_builds.load(Ordering::Relaxed);
    tracing::debug!(
        series_builds = builds,
        combos = n_combos,
        wave_outer,
        "sweep: shard done"
    );

    Ok((
        SweepStats {
            tokens: corpus.token_count(),
            combos: n_combos,
            rows: total_rows,
            fired: total_fired,
        },
        metrics,
    ))
}

/// Refuse absurd combo counts before any fold alloc (defense in depth vs a
/// wrapping `combo_count` that slipped past the grid guard).
const HARD_MAX_COMBOS_GUARD: usize = 1_000_000;

/// Fold one combo pass over one wave of tokens that already have `TokenState`.
/// `bound` must match `params` (compiled once per batch). Merges into `aggs`
/// (same length as `params`). Returns `(rows, fired)` for this wave.
fn fold_wave_into<S: Strategy>(
    strategy: &S,
    params: &[S::Params],
    bound: &[S::BoundParams],
    tokens: &[crate::sweep::corpus::CorpusToken],
    states: &[S::TokenState],
    aggs: &mut [ComboAgg],
    observer: &dyn SweepObserver,
) -> Result<(u64, u64)> {
    assert_eq!(tokens.len(), states.len());
    assert_eq!(aggs.len(), params.len());
    assert_eq!(bound.len(), params.len());
    let n_combos = params.len();
    // Static invariant, NOT the live `preferred_max_combo_batch()` sizing preference:
    // `aggs` is already allocated by the caller at this width, so re-judging it against
    // a *live* RAM reading can only abort a fold whose memory is already committed —
    // the mid-run `n_combos 65536 > 8192` failure that killed runs at group 48. Live
    // pressure legitimately shrinks the *next* batch (see `combo_batch_size`); it must
    // not invalidate this one.
    if n_combos > HARD_MAX_COMBO_BATCH {
        anyhow::bail!(
            "fold_wave_into: n_combos {n_combos} > HARD_MAX_COMBO_BATCH {HARD_MAX_COMBO_BATCH}"
        );
    }

    let channel_depth = rayon::current_num_threads().saturating_mul(2).clamp(4, 32);
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<TokenOutcome>>(channel_depth);
    let (ret_tx, ret_rx) = std::sync::mpsc::channel::<Vec<TokenOutcome>>();
    let ret_rx = std::sync::Mutex::new(ret_rx);

    // The folder borrows `aggs` directly rather than round-tripping a copy.
    //
    // This used to `aggs.to_vec()` in and `clone_from_slice` back out, i.e. two full
    // copies of the accumulator array per call. `ComboAgg` is ~640 POD bytes (two
    // fixed-size quantile sketches), so at a 65536 batch that is ~42 MB in and ~42 MB
    // out — per wave, per pass. A scoped thread can hold `&mut [ComboAgg]` for exactly
    // the scope's lifetime, so the copies bought nothing.
    let (rows, fired) = std::thread::scope(|scope| -> Result<(u64, u64)> {
        let folder = scope.spawn(move || -> (u64, u64) {
            let mut rows = 0u64;
            let mut fired = 0u64;
            while let Ok(outs) = rx.recv() {
                for (combo_id, o) in outs.iter().enumerate() {
                    aggs[combo_id].record(o);
                    rows += 1;
                    if o.fired {
                        fired += 1;
                    }
                }
                // Report this token's evaluations = the combos in this fold pass
                // (`params.len()`); summed over tokens/passes/shards this equals
                // `tokens × n_combos`, the plan-invariant progress unit.
                observer.token_done(params.len());
                let _ = ret_tx.send(outs);
            }
            (rows, fired)
        });

        let produce = tokens.par_iter().zip(states.par_iter()).try_for_each(
            |(tt, state)| -> std::result::Result<(), ()> {
                if observer.cancelled() {
                    return Err(());
                }
                let mut outs = ret_rx
                    .lock()
                    .ok()
                    .and_then(|rx| rx.try_recv().ok())
                    .unwrap_or_else(|| Vec::with_capacity(n_combos));
                fill_outcomes_with_state(
                    strategy, params, bound, tt, state, observer, &mut outs,
                )?;
                let _ = tx.send(outs);
                Ok(())
            },
        );
        drop(tx);
        let result = folder.join().expect("sweep folder panicked");
        let _ = produce;
        Ok(result)
    })?;

    Ok((rows, fired))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sweep::corpus::CorpusToken;
    use crate::sweep::projection::CorpusTrade;
    use crate::sweep::strategy::{ExitCode, ParamSpace, SweepMethod, TokenOutcome};
    use crate::models::trade::{Trade, TradeType};
    use chrono::Utc;

    /// Minimal Strategy that exercises the engine without constructing a real
    /// `Tpsl2Rule` — proves the sweep/aggregate plumbing independent of strategy.
    struct Mock;
    impl ParamSpace for Mock {
        type Params = f64;
        fn sample(&self, _m: SweepMethod) -> Vec<f64> {
            vec![1.0, 2.0, 3.0]
        }
    }
    impl Strategy for Mock {
        // Entry = "did the token have any trades" — param-free, so EntryKey is unit
        // and the engine resolves it once per token (exercises the entry cache).
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

    fn token(mint: &str, n: usize) -> CorpusToken {
        let trades: Vec<Trade> = (0..n)
            .map(|i| {
                Trade::new(
                    mint.into(),
                    "w".into(),
                    TradeType::Buy,
                    1.0,
                    1,
                    format!("sig{i}"),
                    i as u64,
                    Utc::now(),
                )
            })
            .collect();
        CorpusToken::from_trades(
            mint.into(),
            mint.into(),
            Utc::now(),
            crate::sweep::grouping::TokenFingerprint::default(),
            &trades,
        )
    }

    /// Observer that reports cancelled from the first poll — proves the producer
    /// short-circuits before folding any token (rows == 0) and the run still
    /// returns `Ok` (no hang/panic) so the caller can map it to a cancellation.
    struct AlwaysCancelled;
    impl crate::sweep::progress::SweepObserver for AlwaysCancelled {
        fn set_total(&self, _total_tokens: usize, _combos_per_token: usize) {}
        fn token_done(&self, _combos_folded: usize) {}
        fn cancelled(&self) -> bool {
            true
        }
    }

    #[test]
    fn cancel_polled_before_each_entry_resolve() {
        // Prove the per-resolve poll (not just the per-chunk one) fires: an observer
        // that returns `false` on its first poll (the chunk-top check) and `true` on
        // its second must make `fill_outcomes` bail BEFORE folding any combo. Without
        // the pre-resolve poll, the single sub-stride chunk would fold every combo.
        struct CancelOnSecondPoll {
            polls: std::sync::atomic::AtomicUsize,
        }
        impl crate::sweep::progress::SweepObserver for CancelOnSecondPoll {
            fn set_total(&self, _total_tokens: usize, _combos_per_token: usize) {}
            fn token_done(&self, _combos_folded: usize) {}
            fn cancelled(&self) -> bool {
                // 0th poll → false (chunk top), 1st poll → true (before 1st resolve).
                self.polls.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 1
            }
        }
        let strat = Mock; // unit entry key ⇒ exactly one resolve per token
        let params = strat.sample(SweepMethod::Grid); // 3 combos, one sub-stride chunk
        let tok = token("m", 3);
        let obs = CancelOnSecondPoll { polls: std::sync::atomic::AtomicUsize::new(0) };
        let mut out = Vec::new();
        let r = fill_outcomes(&strat, &params, &tok, &obs, &mut out);
        assert!(r.is_err(), "must bail on the pre-resolve cancel poll");
        assert!(out.is_empty(), "bailed before folding any combo");
    }

    #[test]
    fn cancel_bails_without_folding_any_token() {
        // A large combo set: without the per-chunk cancel check a single token
        // would fold all 1_000 combos before the next poll. With it (and the
        // top-of-token check) a pre-set cancel folds nothing.
        let params: Vec<f64> = (0..1_000).map(|i| i as f64).collect();
        let corpus = Corpus {
            tokens: vec![token("a", 4), token("b", 4)],
            hash: "h".into(),
            has_fingerprints: false,
            candidates_capped: false,
        };
        let (stats, metrics) =
            run_sweep(&Mock, &params, &corpus, &AlwaysCancelled, params.len()).unwrap();
        assert_eq!(stats.rows, 0, "cancel short-circuits before any combo is folded");
        assert_eq!(stats.fired, 0);
        // Metrics are still shaped (one row per combo), just empty — the caller
        // discards them on cancel.
        assert_eq!(metrics.len(), params.len());
        assert!(metrics.iter().all(|m| m.n_fired == 0));
    }

    /// A strategy whose entry depends on the param's `entry_key`, used to prove
    /// the engine's per-token entry cache hands each combo **its own** entry even
    /// when the key changes mid-token. `resolve_exit` echoes the entry it was given
    /// into `pnl_sol`, so a stale-cache bug (a combo getting a neighbour's entry)
    /// surfaces as a combo whose folded PnL ≠ its declared entry key.
    struct EntryMock;
    impl ParamSpace for EntryMock {
        type Params = (i64, f64);
        fn sample(&self, _m: SweepMethod) -> Vec<(i64, f64)> {
            vec![]
        }
    }
    impl Strategy for EntryMock {
        type Entry = i64;
        type EntryKey = i64;
        type EntryCands = ();
        type TokenState = ();
        type BoundParams = ();
        type ExitCtx = ();
        type ExitCtxKey = ();
        fn entry_key(&self, p: &(i64, f64)) -> i64 {
            p.0
        }
        fn bind_param(&self, _p: &(i64, f64)) {}
        fn exit_ctx_key(&self, _bound: &(), _entry: &i64) {}
        fn prepare_token(&self, _token: &crate::sweep::corpus::CorpusToken) {}
        fn resolve_entry(&self, _trades: &[CorpusTrade], _state: &(), _bound: &(), p: &(i64, f64)) -> i64 {
            p.0
        }
        fn resolve_exit(
            &self,
            _trades: &[CorpusTrade],
            _state: &(),
            _bound: &(),
            entry: &i64,
            _p: &(i64, f64),
            _ctx: &(),
        ) -> TokenOutcome {
            TokenOutcome {
                fired: true,
                holding_secs: 0,
                pnl_percent: 0.0,
                pnl_sol: *entry as f32,
                exit: ExitCode::TakeProfit,
                exit_metric: None,
                exit_operator: None,
                exit_metric_value: None,
                exit_metric_slot: None,
                entry_time: None,
                entry_price: None,
                entry_slot: None,
                exit_time: None,
                exit_price: None,
                exit_slot: None,
            }
        }
        fn params_json(&self, _p: &(i64, f64)) -> serde_json::Value {
            serde_json::json!({})
        }
    }

    #[test]
    fn entry_cache_gives_each_combo_its_own_entry() {
        // Keys ordered as contiguous blocks AND returning to an earlier key (0 →
        // 1 → 0): every transition must recompute, never serve a stale entry.
        let params: Vec<(i64, f64)> =
            vec![(0, 0.0), (0, 0.0), (1, 0.0), (1, 0.0), (1, 0.0), (0, 0.0)];
        let corpus = Corpus {
            tokens: vec![token("a", 1)],
            hash: "h".into(),
            has_fingerprints: false,
            candidates_capped: false,
        };
        let (_stats, metrics) =
            run_sweep(&EntryMock, &params, &corpus, &crate::sweep::progress::NoopObserver, params.len())
                .unwrap();
        // One token fires once per combo, so each combo's total PnL == the entry it
        // was handed, which must equal that combo's own entry key.
        for (i, m) in metrics.iter().enumerate() {
            assert_eq!(
                m.total_pnl_sol, params[i].0 as f64,
                "combo {i} (key {}) received the wrong cached entry",
                params[i].0
            );
        }
    }

    /// A strategy whose entry is **exit-dependent**: the entry candidates come from
    /// the entry key (Stage A), but a combo's exit param vetoes the early ones
    /// (Stage B) — the shape of the real `can_enter` veto. Combos here share one
    /// `EntryKey` and must still resolve *different* entries.
    struct VetoMock;
    /// `(entry_key, veto_below)` — every candidate row `< veto_below` is vetoed.
    type VetoParams = (i64, i64);
    impl ParamSpace for VetoMock {
        type Params = VetoParams;
        fn sample(&self, _m: SweepMethod) -> Vec<VetoParams> {
            vec![]
        }
    }
    impl VetoMock {
        /// Candidate rows for an entry class — exit-independent by construction.
        fn cands_for(key: i64) -> Vec<i64> {
            vec![key * 10, key * 10 + 1, key * 10 + 2]
        }
        /// The honest entry for one combo: first candidate the veto lets through.
        fn honest(p: &VetoParams) -> i64 {
            Self::cands_for(p.0).into_iter().find(|&c| c >= p.1).unwrap_or(-1)
        }
    }
    impl Strategy for VetoMock {
        type Entry = i64;
        type EntryKey = i64;
        type EntryCands = Vec<i64>;
        type TokenState = ();
        type BoundParams = ();
        type ExitCtx = ();
        type ExitCtxKey = i64;
        fn entry_key(&self, p: &VetoParams) -> i64 {
            p.0
        }
        fn bind_param(&self, _p: &VetoParams) {}
        fn exit_ctx_key(&self, _bound: &(), entry: &i64) -> i64 {
            *entry
        }
        fn prepare_token(&self, _token: &crate::sweep::corpus::CorpusToken) {}
        fn resolve_entry(
            &self,
            _trades: &[CorpusTrade],
            _state: &(),
            _bound: &(),
            p: &VetoParams,
        ) -> i64 {
            Self::honest(p)
        }
        fn entry_candidates(
            &self,
            _trades: &[CorpusTrade],
            _state: &(),
            _bound: &(),
            p: &VetoParams,
            out: &mut Vec<i64>,
        ) {
            out.clear();
            out.extend(Self::cands_for(p.0));
        }
        fn resolve_entry_from(
            &self,
            _trades: &[CorpusTrade],
            _state: &(),
            _bound: &(),
            p: &VetoParams,
            cands: &mut Vec<i64>,
        ) -> i64 {
            cands.iter().copied().find(|&c| c >= p.1).unwrap_or(-1)
        }
        fn resolve_exit(
            &self,
            _trades: &[CorpusTrade],
            _state: &(),
            _bound: &(),
            entry: &i64,
            _p: &VetoParams,
            _ctx: &(),
        ) -> TokenOutcome {
            TokenOutcome {
                fired: true,
                holding_secs: 0,
                pnl_percent: 0.0,
                pnl_sol: *entry as f32,
                exit: ExitCode::TakeProfit,
                exit_metric: None,
                exit_operator: None,
                exit_metric_value: None,
                exit_metric_slot: None,
                entry_time: None,
                entry_price: None,
                entry_slot: None,
                exit_time: None,
                exit_price: None,
                exit_slot: None,
            }
        }
        fn params_json(&self, _p: &VetoParams) -> serde_json::Value {
            serde_json::json!({})
        }
    }

    /// **The entry-cache poisoning regression, at the engine layer.**
    ///
    /// Every combo below shares ONE `EntryKey` and differs only on the exit side,
    /// which is precisely the class the old fold collapsed: it cached the first
    /// combo's *resolved* entry and served it to all its siblings. With the
    /// two-stage split the class shares only the candidates, so each combo's veto
    /// still moves its own entry. (`generic::guard` locks the same property for the
    /// real strategy against `scan`/`run_replay`; this one pins the generic fold
    /// plumbing independent of any strategy.)
    #[test]
    fn fold_reresolves_entry_per_exit_variant_within_one_class() {
        // key 7 ⇒ candidates [70, 71, 72]; the veto picks a different one each time.
        let params: Vec<VetoParams> = vec![(7, 0), (7, 71), (7, 72), (7, 71), (7, 0)];
        let corpus = Corpus {
            tokens: vec![token("a", 1)],
            hash: "h".into(),
            has_fingerprints: false,
            candidates_capped: false,
        };
        let (_stats, metrics) = run_sweep(
            &VetoMock,
            &params,
            &corpus,
            &crate::sweep::progress::NoopObserver,
            params.len(),
        )
        .unwrap();
        for (i, m) in metrics.iter().enumerate() {
            assert_eq!(
                m.total_pnl_sol,
                VetoMock::honest(&params[i]) as f64,
                "combo {i} {:?} inherited a sibling's entry (cache poisoning)",
                params[i]
            );
        }
        // Non-vacuity: the class really does resolve to more than one entry, so a
        // fold that shared one entry per class would fail above.
        let distinct: std::collections::BTreeSet<i64> =
            params.iter().map(VetoMock::honest).collect();
        assert!(distinct.len() > 1, "fixture must span several entries");
    }

    #[test]
    fn sweep_folds_to_one_row_per_combo() {
        let corpus = Corpus {
            tokens: vec![token("a", 2), token("b", 1)],
            hash: "h".into(),
            has_fingerprints: false,
            candidates_capped: false,
        };
        let params = Mock.sample(SweepMethod::Grid);
        let (stats, metrics) =
            run_sweep(&Mock, &params, &corpus, &crate::sweep::progress::NoopObserver, params.len())
                .unwrap();

        assert_eq!(stats.tokens, 2);
        assert_eq!(stats.combos, 3);
        assert_eq!(stats.rows, 6, "2 tokens × 3 combos evaluated");
        assert_eq!(stats.fired, 6);
        assert_eq!(metrics.len(), 3, "one ranked row per combo");
        // Both tokens have trades, so every combo fires on both.
        assert!(metrics.iter().all(|m| m.n_fired == 2));
    }

    /// Folding the same corpus in combo batches of 1 must produce byte-identical
    /// per-combo metrics to a single-batch run (combo ids stay global, the fold is
    /// order-independent), and the same `rows`/`fired` totals — proving batched folding
    /// chunking is a pure memory/CPU trade with no effect on results.
    #[test]
    fn batched_fold_matches_single_batch() {
        let corpus = Corpus {
            tokens: vec![token("a", 3), token("b", 2), token("c", 4)],
            hash: "h".into(),
            has_fingerprints: false,
            candidates_capped: false,
        };
        let params = Mock.sample(SweepMethod::Grid); // 3 combos
        let obs = crate::sweep::progress::NoopObserver;

        let (whole_stats, whole) = run_sweep(&Mock, &params, &corpus, &obs, params.len()).unwrap();
        let (batched_stats, batched) = run_sweep(&Mock, &params, &corpus, &obs, 1).unwrap();

        assert_eq!(whole_stats.rows, batched_stats.rows);
        assert_eq!(whole_stats.fired, batched_stats.fired);
        assert_eq!(whole.len(), batched.len());
        for (w, b) in whole.iter().zip(&batched) {
            assert_eq!(w.combo_id, b.combo_id);
            assert_eq!(w.n_fired, b.n_fired);
            assert_eq!(w.total_pnl_sol, b.total_pnl_sol);
        }
    }

    /// Multi-**wave** equivalence: the wave-outer fold accumulates into one `aggs`
    /// array across many token waves, and hoisting the param bind out of that wave
    /// loop must not change a single number.
    ///
    /// `batched_fold_matches_single_batch` above cannot catch a regression here: with
    /// 3 tokens and `wave == threads`, the whole corpus is ONE wave, so the wave loop
    /// runs once and both the hoisted bind and the in-place accumulator merge are
    /// trivially correct. This drives enough tokens to span many waves — the shape
    /// where a misaligned `bound_all` slice or a lost cross-wave merge would show up.
    #[test]
    fn multi_wave_fold_matches_single_batch() {
        // Comfortably more tokens than any plausible thread count, so
        // `corpus.tokens.chunks(wave)` yields many waves.
        let tokens: Vec<_> = (0..97).map(|i| token(&format!("t{i}"), 2 + i % 5)).collect();
        let corpus = Corpus {
            tokens,
            hash: "h".into(),
            has_fingerprints: false,
            candidates_capped: false,
        };
        let params = Mock.sample(SweepMethod::Grid); // 3 combos
        let obs = crate::sweep::progress::NoopObserver;

        // Single batch (one pass over the combo space) vs per-combo batches: both fold
        // over the same multi-wave token stream and must agree exactly.
        let (whole_stats, whole) = run_sweep(&Mock, &params, &corpus, &obs, params.len()).unwrap();
        let (batched_stats, batched) = run_sweep(&Mock, &params, &corpus, &obs, 1).unwrap();

        assert_eq!(whole_stats.rows, batched_stats.rows);
        assert_eq!(whole_stats.fired, batched_stats.fired);
        // Every combo must have folded every token — the cross-wave accumulation.
        assert_eq!(whole_stats.rows, (corpus.token_count() * params.len()) as u64);
        assert!(whole.iter().all(|m| m.n_fired == corpus.token_count() as u64));
        for (w, b) in whole.iter().zip(&batched) {
            assert_eq!(w.combo_id, b.combo_id);
            assert_eq!(w.n_fired, b.n_fired);
            assert_eq!(w.total_pnl_sol, b.total_pnl_sol);
        }
    }

    #[test]
    fn combo_batch_size_floors_at_one_and_caps_at_combos() {
        // Small combo counts fit in one batch; never exceed n_combos or the hard cap.
        assert_eq!(combo_batch_size(5, 4), 5);
        assert_eq!(combo_batch_size(0, 4), 1);
        assert!(combo_batch_size(1_000_000, 8) <= HARD_MAX_COMBO_BATCH);
        assert_eq!(combo_batch_count(7, 3), 3);
        assert_eq!(combo_batch_count(6, 3), 2);
        assert_eq!(combo_batch_count(0, 3), 1);
    }

    /// The live RAM sizing preference must never exceed the static allocation
    /// invariant the fold guards assert against.
    ///
    /// This is what makes a *pinned* batch safe against mid-run RAM movement: a batch
    /// sized at any moment is `≤ preferred_max_combo_batch() ≤ HARD_MAX_COMBO_BATCH`,
    /// so `fold_wave_into`'s guard still holds later even if free RAM has since dipped
    /// under the desktop reserve and the preference dropped to 8192. Regression for the
    /// mid-run `n_combos 65536 > hard_max_combo_batch 8192` abort, which came from
    /// asserting an already-allocated batch against the *live* value.
    #[test]
    fn preferred_batch_never_exceeds_static_invariant() {
        let preferred = crate::sweep::registry::preferred_max_combo_batch();
        assert!(
            preferred <= HARD_MAX_COMBO_BATCH,
            "sizing preference {preferred} exceeds the invariant {HARD_MAX_COMBO_BATCH} — \
             a pinned batch could then fail the fold guard mid-run"
        );
        // Whatever the host's current RAM state, a sized batch clears the static guard.
        for threads in [1usize, 4, 16] {
            assert!(combo_batch_size(1_000_000, threads) <= HARD_MAX_COMBO_BATCH);
        }
    }
}
