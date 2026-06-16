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
use crate::sweep::strategy::{Strategy, TokenOutcome};

/// How many combos one token folds between cancel polls. Small enough that a
/// cancel lands sub-100ms even on a huge combo set, large enough that the atomic
/// load is amortised to noise against the inner `simulate` work.
const CANCEL_CHECK_STRIDE: usize = 256;

/// Headline counts for a completed sweep.
#[derive(Clone, Copy, Debug)]
pub struct SweepStats {
    pub tokens: usize,
    pub combos: usize,
    pub rows: u64,
    pub fired: u64,
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
) -> Result<(SweepStats, Vec<ComboMetrics>)> {
    let projected = corpus.token_count() as u64 * params.len() as u64;
    tracing::info!(
        tokens = corpus.token_count(),
        combos = params.len(),
        projected_evals = projected,
        "sweep: starting (folding to per-combo metrics)"
    );

    let n_combos = params.len();
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<TokenOutcome>>(256);

    let (rows, fired, aggs) = std::thread::scope(|scope| -> Result<(u64, u64, Vec<ComboAgg>)> {
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
                // Fold all combos for this token, but poll the cancel flag every
                // CANCEL_CHECK_STRIDE so a cancel interrupts a large combo set
                // mid-token instead of after it. A partial `outs` on bail is never
                // sent (the folder indexes by combo_id); the caller discards the
                // run's aggregates once it sees `cancelled()`.
                let mut outs: Vec<TokenOutcome> = Vec::with_capacity(params.len());
                for chunk in params.chunks(CANCEL_CHECK_STRIDE) {
                    if observer.cancelled() {
                        return Err(());
                    }
                    outs.extend(chunk.iter().map(|p| strategy.simulate(&tt.trades, p)));
                }
                // Folder never drops early; ignore only on shutdown races.
                let _ = tx.send(outs);
                Ok(())
            });
        drop(tx);

        Ok(folder.join().expect("sweep folder thread panicked"))
    })?;

    let metrics: Vec<ComboMetrics> = aggs
        .into_iter()
        .enumerate()
        .map(|(id, a)| a.finalize(id as u32))
        .collect();

    Ok((
        SweepStats {
            tokens: corpus.token_count(),
            combos: params.len(),
            rows,
            fired,
        },
        metrics,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sweep::corpus::TokenTrades;
    use crate::sweep::projection::SweepTrade;
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
        fn id(&self) -> &'static str {
            "mock"
        }
        fn simulate(&self, trades: &[SweepTrade], p: &f64) -> TokenOutcome {
            TokenOutcome {
                fired: !trades.is_empty(),
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

    fn token(mint: &str, n: usize) -> TokenTrades {
        let trades: Vec<Trade> = (0..n)
            .map(|i| {
                Trade::new(
                    mint.into(),
                    "w".into(),
                    TradeType::Buy,
                    1.0,
                    1.0,
                    format!("sig{i}"),
                    i as u64,
                    Utc::now(),
                )
            })
            .collect();
        TokenTrades::from_trades(
            mint.into(),
            mint.into(),
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
    fn cancel_bails_without_folding_any_token() {
        // A large combo set: without the per-chunk cancel check a single token
        // would fold all 1_000 combos before the next poll. With it (and the
        // top-of-token check) a pre-set cancel folds nothing.
        let params: Vec<f64> = (0..1_000).map(|i| i as f64).collect();
        let corpus = Corpus {
            tokens: vec![token("a", 4), token("b", 4)],
            hash: "h".into(),
        };
        let (stats, metrics) = run_sweep(&Mock, &params, &corpus, &AlwaysCancelled).unwrap();
        assert_eq!(stats.rows, 0, "cancel short-circuits before any combo is folded");
        assert_eq!(stats.fired, 0);
        // Metrics are still shaped (one row per combo), just empty — the caller
        // discards them on cancel.
        assert_eq!(metrics.len(), params.len());
        assert!(metrics.iter().all(|m| m.n_fired == 0));
    }

    #[test]
    fn sweep_folds_to_one_row_per_combo() {
        let corpus = Corpus {
            tokens: vec![token("a", 2), token("b", 1)],
            hash: "h".into(),
        };
        let params = Mock.sample(SweepMethod::Grid);
        let (stats, metrics) =
            run_sweep(&Mock, &params, &corpus, &crate::sweep::progress::NoopObserver).unwrap();

        assert_eq!(stats.tokens, 2);
        assert_eq!(stats.combos, 3);
        assert_eq!(stats.rows, 6, "2 tokens × 3 combos evaluated");
        assert_eq!(stats.fired, 6);
        assert_eq!(metrics.len(), 3, "one ranked row per combo");
        // Both tokens have trades, so every combo fires on both.
        assert!(metrics.iter().all(|m| m.n_fired == 2));
    }
}
