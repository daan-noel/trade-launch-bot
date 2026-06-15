//! Grouped sweep: partition the corpus by exact-value fingerprint key, then run
//! the existing per-combo [`engine::run_sweep`] **once per group**.
//!
//! This is the partition-then-reuse design: the rayon hot loop in [`engine`] is
//! untouched and stays allocation-free; the only added work is an `O(tokens)`
//! grouping pass and `O(groups)` sub-corpus assembly (each sub-corpus is a
//! refcount-clone of `TokenTrades` — `trades` is an `Arc`, so no trade buffer is
//! copied). Groups run **sequentially**; each `run_sweep`'s inner `par_iter`
//! uses the single (bounded) rayon pool, so pools are never nested.
//!
//! Empty `fields` ⇒ a single "ALL" group ⇒ identical to a global ungrouped sweep.

use std::cmp::Ordering::Equal;
use std::collections::HashMap;

use anyhow::{bail, Result};

use crate::sweep::aggregate::ComboMetrics;
use crate::sweep::corpus::Corpus;
use crate::sweep::engine::run_sweep;
use crate::sweep::grouping::{group_key, GroupField, GroupKey};
use crate::sweep::progress::SweepObserver;
use crate::sweep::strategy::Strategy;

/// One group's full sweep: its key, how many tokens fell into it, the per-combo
/// ranked metrics, and the winning combo (max expectancy per trade).
pub struct GroupResult {
    pub key: GroupKey,
    pub token_count: usize,
    pub metrics: Vec<ComboMetrics>,
    /// Combo id maximising expectancy among combos that fired (see [`best_combo`]).
    pub best_combo_id: u32,
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
    observer: &dyn SweepObserver,
) -> Result<Vec<GroupResult>> {
    let floor = min_tokens.max(1);
    let mut surviving: Vec<(GroupKey, Vec<usize>)> = partition(corpus, fields)
        .into_iter()
        .filter(|(_, idx)| idx.len() >= floor)
        .collect();
    // Deterministic group order: most-populated first, ties broken by key JSON.
    surviving.sort_by(|a, b| {
        b.1.len()
            .cmp(&a.1.len())
            .then_with(|| a.0.to_json().to_string().cmp(&b.0.to_json().to_string()))
    });

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

    let mut out = Vec::with_capacity(surviving.len());
    for (key, idx) in surviving {
        if observer.cancelled() {
            bail!("sweep cancelled");
        }
        // Sub-corpus: refcount-clone each token (Arc trades — no buffer copy).
        let sub = Corpus {
            tokens: idx.iter().map(|&i| corpus.tokens[i].clone()).collect(),
            hash: corpus.hash.clone(),
        };
        let token_count = sub.token_count();
        let (_stats, metrics) = run_sweep(strategy, params, &sub, observer)?;
        // A cancel mid-group leaves the just-swept metrics partial — discard.
        if observer.cancelled() {
            bail!("sweep cancelled");
        }
        let (best_combo_id, best_expectancy_sol) = best_combo(&metrics);
        out.push(GroupResult {
            key,
            token_count,
            metrics,
            best_combo_id,
            best_expectancy_sol,
        });
    }
    Ok(out)
}

/// Best combo = max `expectancy_sol` among combos that fired at least once; ties
/// broken by fired count, then total PnL. `(0, 0.0)` when no combo fired.
fn best_combo(metrics: &[ComboMetrics]) -> (u32, f64) {
    metrics
        .iter()
        .filter(|m| m.n_fired > 0)
        .max_by(|a, b| {
            a.expectancy_sol
                .partial_cmp(&b.expectancy_sol)
                .unwrap_or(Equal)
                .then_with(|| a.n_fired.cmp(&b.n_fired))
                .then_with(|| a.total_pnl_sol.partial_cmp(&b.total_pnl_sol).unwrap_or(Equal))
        })
        .map(|m| (m.combo_id, m.expectancy_sol))
        .unwrap_or((0, 0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::trade::{Trade, TradeType};
    use crate::sweep::corpus::TokenTrades;
    use crate::sweep::grouping::TokenFingerprint;
    use crate::sweep::strategy::{ExitCode, ParamSpace, SweepMethod, TokenOutcome};
    use chrono::Utc;
    use std::sync::Arc;

    /// Fires on every token; PnL == the param value, so combo `i` has expectancy
    /// == params[i] and `best_combo` must pick the largest.
    struct Mock;
    impl ParamSpace for Mock {
        type Params = f64;
        fn sample(&self, _m: SweepMethod) -> Vec<f64> {
            vec![1.0, 3.0, 2.0]
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
        TokenTrades {
            mint: mint.into(),
            symbol: mint.into(),
            fp: TokenFingerprint {
                creator_wallet: creator.into(),
                ..Default::default()
            },
            trades: Arc::new(vec![t]),
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
            &crate::sweep::progress::NoopObserver,
        )
        .unwrap();

        assert_eq!(groups.len(), 2, "devA + devB");
        // Largest group (devA, 2 tokens) sorts first.
        assert_eq!(groups[0].token_count, 2);
        assert_eq!(groups[1].token_count, 1);
        // Best combo is params[1] = 3.0 → combo_id 1, expectancy 3.0.
        assert_eq!(groups[0].best_combo_id, 1);
        assert!((groups[0].best_expectancy_sol - 3.0).abs() < 1e-9);
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
            &crate::sweep::progress::NoopObserver,
        )
        .unwrap();
        assert_eq!(groups.len(), 1, "only devA (2 tokens) clears min_tokens=2");
        assert_eq!(groups[0].token_count, 2);
    }

    #[test]
    fn empty_fields_is_single_all_group() {
        let params = Mock.sample(SweepMethod::Grid);
        let groups =
            run_grouped_sweep(&Mock, &params, &corpus(), &[], 1, &crate::sweep::progress::NoopObserver)
                .unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].token_count, 3);
    }
}
