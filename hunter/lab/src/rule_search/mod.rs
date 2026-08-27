//! Rule search — lab job that finds one champion `RuleParams` for a fingerprint
//! and a datetime range. Sibling of grouped sweep / flow discovery / metric
//! discovery, not a sweep mode.
//!
//! Method: [`hunter/docs/plans/strategies/rule-search-method.md`].
//! Job wiring: [`hunter/docs/plans/strategies/rule-search.md`].

pub mod cuts;
pub mod dto;
pub mod generator;
pub mod report;
pub mod roles;
pub mod scorer;

use chrono::{DateTime, Utc};
use hunter_engine::fingerprint::Fingerprint as EngineFingerprint;
use hunter_engine::metrics::flow_split::FlowPatterns;
use hunter_engine::rule_params::RuleParams;

use crate::sweep::corpus::Corpus;
use crate::sweep::generic::Pricing;
use crate::sweep::progress::SweepObserver;
use trading_core::strategies::kernel::CostModel;
use trading_core::strategies::paper_fill::FillModel;

use cuts::build_cut_table;
use generator::{extra_or_candidates, generate, retune_candidates};
use report::{build_report, Report, ReplayOpts};
use scorer::{score_combos, to_replay_tokens, ScoreConfig};

pub struct SearchInput<'a> {
    pub corpus: &'a Corpus,
    pub fp: &'a EngineFingerprint,
    pub pricing: Pricing,
    pub as_of: DateTime<Utc>,
    pub flow: Option<&'a FlowPatterns>,
    pub skip_duplicate_identity: bool,
    pub duplicate_identity_window_hours: u64,
    pub max_concurrent_tokens: u32,
    pub max_total_tokens: u32,
    pub incumbent: Option<RuleParams>,
    pub fill_authority: FillModel,
    pub fill_optimistic: FillModel,
    pub cost: CostModel,
}

/// Load cuts → generate → score → extra-OR on top 3 → `run_replay` report.
pub fn run_search(
    input: SearchInput<'_>,
    observer: &dyn SweepObserver,
) -> anyhow::Result<Report> {
    observer.notice("cuts");
    let cuts = build_cut_table(&input.corpus.tokens, input.flow, input.fp.id);
    if observer.cancelled() {
        anyhow::bail!("cancelled");
    }

    observer.notice("generate");
    let mut combos = generate(&cuts);
    if combos.is_empty() {
        anyhow::bail!("generator produced no combos");
    }

    observer.notice("score");
    let cfg = ScoreConfig {
        pricing: input.pricing,
        as_of: input.as_of,
        flow: input.flow,
        flow_fp: input.fp.id,
        skip_duplicate_identity: input.skip_duplicate_identity,
        duplicate_identity_window_hours: input.duplicate_identity_window_hours,
        max_concurrent_tokens: input.max_concurrent_tokens,
        max_total_tokens: input.max_total_tokens,
    };
    let mut archive = score_combos(&input.corpus.tokens, &combos, &cfg, observer)?;

    // Extra OR on the top 5 full rules ([method] §3). Ranked by the trimmed
    // (worst-blocks) SOL so a one-burst edge cannot pick every expansion base.
    let mut ranked: Vec<usize> = (0..archive.len()).collect();
    ranked.sort_by(|&a, &b| {
        archive[b]
            .robust_sol()
            .partial_cmp(&archive[a].robust_sol())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut extras = Vec::new();
    for &i in ranked.iter().take(5) {
        extras.extend(extra_or_candidates(&combos[i], &cuts));
    }
    if !extras.is_empty() && !observer.cancelled() {
        observer.notice("extra-or");
        let extra_archive = score_combos(&input.corpus.tokens, &extras, &cfg, observer)?;
        let best = archive
            .iter()
            .map(|a| a.robust_sol())
            .fold(f64::NEG_INFINITY, f64::max);
        for (row, combo) in extra_archive.into_iter().zip(extras) {
            if row.robust_sol() > best {
                let mut r = row;
                r.combo_idx = combos.len();
                archive.push(r);
                combos.push(combo);
            }
        }
    }

    ranked = (0..archive.len()).collect();
    ranked.sort_by(|&a, &b| {
        archive[b]
            .robust_sol()
            .partial_cmp(&archive[a].robust_sol())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut retunes = Vec::new();
    for &i in ranked.iter().take(3) {
        retunes.extend(retune_candidates(&combos[i], &cuts));
    }
    if !retunes.is_empty() && !observer.cancelled() {
        observer.notice("retune");
        let retune_archive = score_combos(&input.corpus.tokens, &retunes, &cfg, observer)?;
        let best = archive
            .iter()
            .map(|a| a.robust_sol())
            .fold(f64::NEG_INFINITY, f64::max);
        for (row, combo) in retune_archive.into_iter().zip(retunes) {
            if row.robust_sol() > best {
                let mut r = row;
                r.combo_idx = combos.len();
                archive.push(r);
                combos.push(combo);
            }
        }
    }

    observer.notice("replay");
    let replay_tokens = to_replay_tokens(&input.corpus.tokens);
    let incumbent_loaded = input.incumbent.as_ref().map(|p| {
        scorer::loaded_from_params(
            p.clone(),
            input.fp.id,
            input.pricing.buy_amount_sol,
            input.max_concurrent_tokens,
            input.max_total_tokens,
        )
    });
    let report = build_report(
        &replay_tokens,
        input.fp,
        &combos,
        &archive,
        input.corpus.tokens.len() as u64,
        incumbent_loaded,
        &ReplayOpts {
            as_of: input.as_of,
            fill_authority: input.fill_authority,
            fill_optimistic: input.fill_optimistic,
            cost: input.cost,
            skip_duplicate_identity: input.skip_duplicate_identity,
            duplicate_identity_window_hours: input.duplicate_identity_window_hours,
            buy_sol: input.pricing.buy_amount_sol,
            range: scorer::corpus_range(input.corpus.tokens.iter().map(|t| t.created_at)),
        },
    );
    Ok(report)
}

#[cfg(test)]
mod tests {
    use hunter_engine::fingerprint::{AxisId, AxisPredicate, Criteria};
    use super::*;
    use crate::discovery::fixtures;
    use crate::sweep::progress::NoopObserver;
    use hunter_engine::fingerprint::{Fingerprint, FingerprintId};
    use trading_core::strategies::kernel::CostModel;
    use uuid::Uuid;

    fn fp() -> Fingerprint {
        Fingerprint {
            id: FingerprintId(Uuid::nil()),
            metric_config: serde_json::json!({}),
            wildcard: false,
            criteria: Criteria::new(),
        }
    }

    #[test]
    fn search_on_tiny_corpus_finishes() {
        let corpus = fixtures::corpus(3);
        let fp = fp();
        let pricing = fixtures::pricing();
        let report = run_search(
            SearchInput {
                corpus: &corpus,
                fp: &fp,
                pricing,
                as_of: Utc::now(),
                flow: None,
                skip_duplicate_identity: true,
                duplicate_identity_window_hours: 24,
                max_concurrent_tokens: 0,
                max_total_tokens: 0,
                incumbent: None,
                fill_authority: FillModel::WorstCase,
                fill_optimistic: FillModel::FirstInWindow,
                cost: CostModel::pumpfun_with_impact(),
            },
            &NoopObserver,
        )
        .expect("search");
        assert!(report.n_combos >= 1);
        assert_eq!(report.n_matched, 3);
        assert!(matches!(
            report.verdict,
            report::Verdict::Refuse | report::Verdict::Ungated | report::Verdict::Candidate
        ));
    }
}
