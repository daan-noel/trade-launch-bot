//! The sweep core: `rayon`-parallel **over tokens**, all combos evaluated inner
//! so each token's trade slice stays cache-resident across the whole combo set.
//! `simulate` returns a `Copy` [`TokenOutcome`] — no per-call heap. A single
//! writer thread folds outcomes into one [`ComboAgg`] per combo, so combos ×
//! tokens rows are never buffered: the sweep yields `combos` ranked rows.

use anyhow::Result;
use rayon::prelude::*;

use crate::analysis::aggregate::{ComboAgg, ComboMetrics};
use crate::analysis::corpus::Corpus;
use crate::analysis::strategy::{Strategy, TokenOutcome};

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
pub fn run_sweep<S: Strategy>(
    strategy: &S,
    params: &[S::Params],
    corpus: &Corpus,
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
            }
            (rows, fired, aggs)
        });

        // Producers: one token at a time, all combos inner (slice stays hot).
        corpus
            .tokens
            .par_iter()
            .for_each_with(tx.clone(), |tx, tt| {
                let outs: Vec<TokenOutcome> = params
                    .iter()
                    .map(|p| strategy.simulate(&tt.trades, p))
                    .collect();
                // Folder never drops early; ignore only on shutdown races.
                let _ = tx.send(outs);
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
    use crate::analysis::corpus::TokenTrades;
    use crate::analysis::strategy::{ExitCode, ParamSpace, SweepMethod, TokenOutcome};
    use crate::models::trade::{Trade, TradeType};
    use chrono::Utc;
    use std::sync::Arc;

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
        fn simulate(&self, trades: &[Trade], p: &f64) -> TokenOutcome {
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
        TokenTrades {
            mint: mint.into(),
            symbol: mint.into(),
            trades: Arc::new(trades),
        }
    }

    #[test]
    fn sweep_folds_to_one_row_per_combo() {
        let corpus = Corpus {
            tokens: vec![token("a", 2), token("b", 1)],
            hash: "h".into(),
        };
        let params = Mock.sample(SweepMethod::Grid);
        let (stats, metrics) = run_sweep(&Mock, &params, &corpus).unwrap();

        assert_eq!(stats.tokens, 2);
        assert_eq!(stats.combos, 3);
        assert_eq!(stats.rows, 6, "2 tokens × 3 combos evaluated");
        assert_eq!(stats.fired, 6);
        assert_eq!(metrics.len(), 3, "one ranked row per combo");
        // Both tokens have trades, so every combo fires on both.
        assert!(metrics.iter().all(|m| m.n_fired == 2));
    }
}
