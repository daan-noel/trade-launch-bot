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
pub(crate) fn fill_outcomes_with_state<S: Strategy>(
    strategy: &S,
    params: &[S::Params],
    token: &crate::sweep::corpus::CorpusToken,
    token_state: &S::TokenState,
    observer: &dyn SweepObserver,
    out: &mut Vec<TokenOutcome>,
) -> std::result::Result<(), ()> {
    out.clear();
    let trades: &[CorpusTrade] = &token.trades;
    let mut entry_cache: Option<(S::EntryKey, S::Entry)> = None;
    for chunk in params.chunks(CANCEL_CHECK_STRIDE) {
        if observer.cancelled() {
            return Err(());
        }
        for p in chunk {
            let key = strategy.entry_key(p);
            let stale = match &entry_cache {
                Some((k, _)) => k != &key,
                None => true,
            };
            if stale {
                if observer.cancelled() {
                    return Err(());
                }
                let entry = strategy.resolve_entry(trades, token_state, p);
                entry_cache = Some((key, entry));
            }
            let entry = &entry_cache.as_ref().unwrap().1;
            out.push(strategy.resolve_exit(trades, token_state, entry, p));
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
/// Shared by the parallel [`run_sweep`] helpers and the serial per-group fold in
/// `grouped_engine`, so both resolve entries identically.
pub(crate) fn fill_outcomes<S: Strategy>(
    strategy: &S,
    params: &[S::Params],
    token: &crate::sweep::corpus::CorpusToken,
    observer: &dyn SweepObserver,
    out: &mut Vec<TokenOutcome>,
) -> std::result::Result<(), ()> {
    let token_state = strategy.prepare_token(token);
    fill_outcomes_with_state(strategy, params, token, &token_state, observer, out)
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

/// Largest combo batch whose fold peak stays within the memory budget (Phase 2.5).
///
/// Peak for one batch ≈
///   `batch × sizeof(ComboAgg)` (folder)
/// + `inflight × batch × sizeof(TokenOutcome)` (rayon producers + bounded queue)
///
/// so `batch = budget / (sizeof(ComboAgg) + inflight × sizeof(TokenOutcome))`.
/// Also hard-capped at [`HARD_MAX_COMBO_BATCH`] so a fragmented 16 GB box never
/// tries a single ~180 MB contiguous alloc that aborts the process.
pub fn combo_batch_size(n_combos: usize, threads: usize) -> usize {
    if n_combos == 0 {
        return 1;
    }
    let threads = threads.max(1) as u64;
    // Producers (~threads) + a few queued buffers (fold_batch channel depth ≤ 32).
    let inflight = threads.saturating_mul(3).max(4);
    let per = (std::mem::size_of::<ComboAgg>() as u64)
        .saturating_add(inflight.saturating_mul(std::mem::size_of::<TokenOutcome>() as u64))
        .max(1);
    let budget = crate::sweep::registry::sweep_memory_budget_bytes();
    let max_batch = (budget / per).max(1) as usize;
    max_batch.min(n_combos).min(HARD_MAX_COMBO_BATCH).max(1)
}

/// Absolute ceiling on one fold batch's combo count — independent of the byte
/// budget. Prevents a single `vec![ComboAgg; batch]` from demanding hundreds of MB
/// of contiguous address space on a RAM-fragmented workstation.
const HARD_MAX_COMBO_BATCH: usize = 65_536;

/// Number of `batch`-sized passes needed to cover `n_combos` combos (≥ 1). Used by
/// the grouped driver to scale the progress total: each token is folded once per
/// batch, so the bar's denominator is `tokens × this`.
pub fn combo_batch_count(n_combos: usize, batch: usize) -> usize {
    n_combos.div_ceil(batch.max(1)).max(1)
}

/// Run every combo against every token; fold the per-(combo, token) outcomes
/// into one ranked [`ComboMetrics`] row per combo. Parallel over tokens; the
/// inner loop runs all combos for one token before moving on. The writer thread
/// owns the accumulators so the hot loop never locks.
///
/// `observer` reports one `token_done` per folded token (for the progress bar)
/// and is polled for cancellation: once `cancelled()` is set, the per-token
/// producers stop scheduling new tokens via `try_for_each_with`, AND the inner
/// combo loop bails between chunks of [`CANCEL_CHECK_STRIDE`] combos. Without the
/// inner check, a single token folds the full combo set (up to `HARD_MAX_COMBOS`)
/// before the next cancel poll — seconds of work per in-flight token — so a cancel
/// couldn't land promptly on a large-combo run. With it, the worst-case stop
/// latency is one chunk × the ≤pool-size in-flight tokens. A token that bails
/// mid-loop produces a short `outs` (fewer than `n_combos`); it is NEVER sent to
/// the folder (which indexes by `combo_id`), and the caller discards the partial
/// aggregates after checking `cancelled()`.
pub fn run_sweep<S: Strategy>(
    strategy: &S,
    params: &[S::Params],
    corpus: &Corpus,
    observer: &dyn SweepObserver,
    batch: usize,
) -> Result<(SweepStats, Vec<ComboMetrics>)> {
    let n_combos = params.len();
    let batch = batch.clamp(1, n_combos.max(1));
    let projected = corpus.token_count() as u64 * n_combos as u64;
    tracing::info!(
        tokens = corpus.token_count(),
        combos = n_combos,
        batch,
        n_batches = combo_batch_count(n_combos, batch),
        projected_evals = projected,
        "sweep: starting (folding to per-combo metrics, batched)"
    );

    // Fold the combo space in budget-sized batches (Phase 2.5): each batch is a full
    // token-fold over only its combos, finalised to `ComboMetrics` and freed before
    // the next, so peak accumulator memory is `batch × ComboAgg`, not the full combo
    // count. Combo ids are kept global (`offset + local`) so the metrics vector is
    // indexed by `combo_id` exactly as a single-batch run would be.
    let mut metrics: Vec<ComboMetrics> = Vec::with_capacity(n_combos);
    let mut total_rows = 0u64;
    let mut total_fired = 0u64;
    for (b, chunk) in params.chunks(batch).enumerate() {
        let offset = b * batch;
        let (rows, fired, aggs) = fold_batch(strategy, chunk, corpus, observer)?;
        total_rows += rows;
        total_fired += fired;
        metrics.extend(
            aggs.into_iter()
                .enumerate()
                .map(|(i, a)| a.finalize((offset + i) as u32)),
        );
    }

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

/// Fold one combo batch over the whole corpus: parallel over tokens, all of this
/// batch's combos inner, a single writer thread accumulating one [`ComboAgg`] per
/// batch combo. Returns the batch's `(rows, fired, aggs)` (aggs indexed by the
/// batch-local combo position). Reports one `token_done` per folded token, so a run
/// of `n_batches` batches reports `tokens × n_batches` units — the grouped driver
/// scales `set_total` to match. Cancel handling is unchanged: a bail leaves the
/// aggs partial and the caller discards them.
fn fold_batch<S: Strategy>(
    strategy: &S,
    params: &[S::Params],
    corpus: &Corpus,
    observer: &dyn SweepObserver,
) -> Result<(u64, u64, Vec<ComboAgg>)> {
    let n_combos = params.len();
    // Bound queued outcome buffers to ~2× pool size (was 256). With large batches,
    // a deep queue of `Vec<TokenOutcome>` of length `batch` can OOM the box before
    // the folder drains them.
    let channel_depth = rayon::current_num_threads().saturating_mul(2).clamp(4, 32);
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<TokenOutcome>>(channel_depth);
    // Buffer-recycle channel (folder → producers): the folder returns each drained
    // outcome buffer here so a producer can refill it instead of allocating a fresh
    // `vec![; n_combos]` (≈ `n_combos × sizeof(TokenOutcome)`) for every token. At
    // big-group scale (thousands of tokens × `n_batches`) that alloc/free churn was
    // pure overhead. Unbounded (the folder only ever holds one buffer at a time);
    // the `Receiver` is shared across producers behind a short-held `Mutex`.
    let (ret_tx, ret_rx) = std::sync::mpsc::channel::<Vec<TokenOutcome>>();
    let ret_rx = std::sync::Mutex::new(ret_rx);

    std::thread::scope(|scope| -> Result<(u64, u64, Vec<ComboAgg>)> {
        // Single fold thread: drains outcomes and accumulates per combo.
        let folder = scope.spawn(move || -> (u64, u64, Vec<ComboAgg>) {
            let mut aggs = vec![ComboAgg::default(); n_combos];
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
                // One folded token = one progress unit. The folder is single-
                // threaded, so this is uncontended (no lock on the hot path).
                observer.token_done();
                // Hand the buffer back for reuse (cleared by `fill_outcomes` on the
                // next refill). Ignore the error if every producer has already gone.
                let _ = ret_tx.send(outs);
            }
            (rows, fired, aggs)
        });

        // Producers: one token at a time, all combos inner (slice stays hot).
        // Cooperative cancel: `try_for_each_with` lets us return `Err` to make
        // rayon stop scheduling further tokens at once, so a cancel aborts the
        // run promptly instead of draining the whole remaining corpus. The
        // caller discards the partial aggregates, so the early exit is safe.
        let _ = corpus
            .tokens
            .par_iter()
            .try_for_each_with(tx.clone(), |tx, tt| {
                if observer.cancelled() {
                    return Err(());
                }
                // Reuse a recycled outcome buffer if one is waiting, else allocate.
                // `fill_outcomes` clears it before refilling, so a stale buffer is
                // safe. The `Mutex` is held only for a non-blocking `try_recv`.
                let mut outs = ret_rx
                    .lock()
                    .ok()
                    .and_then(|rx| rx.try_recv().ok())
                    .unwrap_or_else(|| Vec::with_capacity(n_combos));
                // Fold all combos for this token (entry resolved once per key,
                // cancel polled mid-fold — see `fill_outcomes`). A partial `outs`
                // on bail is never sent; the caller discards the run's aggregates
                // once it sees `cancelled()`.
                fill_outcomes(strategy, params, tt, observer, &mut outs)?;
                // Folder never drops early; ignore only on shutdown races.
                let _ = tx.send(outs);
                Ok(())
            });
        drop(tx);

        Ok(folder.join().expect("sweep folder thread panicked"))
    })
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
        type TokenState = ();
        fn entry_key(&self, _p: &f64) {}
        fn prepare_token(&self, _token: &crate::sweep::corpus::CorpusToken) {}
        fn resolve_entry(&self, trades: &[CorpusTrade], _state: &(), _p: &f64) -> bool {
            !trades.is_empty()
        }
        fn resolve_exit(&self, _trades: &[CorpusTrade], _state: &(), entry: &bool, p: &f64) -> TokenOutcome {
            TokenOutcome {
                fired: *entry,
                holding_secs: 1,
                pnl_percent: *p as f32,
                pnl_sol: *p as f32,
                exit: ExitCode::TakeProfit,
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
        fn set_total(&self, _total: usize) {}
        fn token_done(&self) {}
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
            fn set_total(&self, _total: usize) {}
            fn token_done(&self) {}
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
        type TokenState = ();
        fn entry_key(&self, p: &(i64, f64)) -> i64 {
            p.0
        }
        fn prepare_token(&self, _token: &crate::sweep::corpus::CorpusToken) {}
        fn resolve_entry(&self, _trades: &[CorpusTrade], _state: &(), p: &(i64, f64)) -> i64 {
            p.0
        }
        fn resolve_exit(&self, _trades: &[CorpusTrade], _state: &(), entry: &i64, _p: &(i64, f64)) -> TokenOutcome {
            TokenOutcome {
                fired: true,
                holding_secs: 0,
                pnl_percent: 0.0,
                pnl_sol: *entry as f32,
                exit: ExitCode::TakeProfit,
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

    #[test]
    fn sweep_folds_to_one_row_per_combo() {
        let corpus = Corpus {
            tokens: vec![token("a", 2), token("b", 1)],
            hash: "h".into(),
            has_fingerprints: false,
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
    /// order-independent), and the same `rows`/`fired` totals — proving Phase 2.5
    /// chunking is a pure memory/CPU trade with no effect on results.
    #[test]
    fn batched_fold_matches_single_batch() {
        let corpus = Corpus {
            tokens: vec![token("a", 3), token("b", 2), token("c", 4)],
            hash: "h".into(),
            has_fingerprints: false,
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
}
