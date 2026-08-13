//! Report columns are `run_replay` ([rule-search.md] Scorer). Champion is the
//! replay winner when the fast archive and replay disagree.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use hunter_engine::event::{ExitReason, LoadedRule};
use hunter_engine::fingerprint::Fingerprint as EngineFingerprint;
use hunter_engine::rule_params::RuleParams;
use trading_core::strategies::kernel::{CostModel, ExitCode};
use trading_core::strategies::paper_fill::FillModel;

use crate::strategies::replay::{run_replay, PositionOutcome, ReplayConfig, ReplayToken};
use crate::sweep::aggregate::ComboAgg;
use crate::sweep::strategy::TokenOutcome;

use super::generator::{is_empty_entry, same_exit_bag, GeneratedCombo};
use super::scorer::{loaded_from_params, ArchiveRow};

pub const MIN_CLOSED: u64 = 8;
pub const TOP_ARCHIVE: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Refuse,
    Ungated,
    Candidate,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ScoredRule {
    pub params: serde_json::Value,
    pub n_fired: u64,
    pub n_closed: u64,
    pub n_tokens_entered: u64,
    pub enter_pct: f64,
    pub enter_pct_unguarded: Option<f64>,
    pub total_pnl_sol: f64,
    pub total_pnl_sol_optimistic: Option<f64>,
    pub profit_factor: Option<f64>,
    pub win_rate: f64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Report {
    pub verdict: Verdict,
    pub n_matched: u64,
    pub n_combos: u64,
    pub champion: Option<ScoredRule>,
    pub empty_entry: Option<ScoredRule>,
    pub incumbent: Option<ScoredRule>,
    pub archive: Vec<ScoredRule>,
    pub archive_replay_disagree: bool,
    pub diagnostics: Vec<String>,
}

pub struct ReplayOpts {
    pub as_of: DateTime<Utc>,
    pub fill_authority: FillModel,
    pub fill_optimistic: FillModel,
    pub cost: CostModel,
    pub skip_duplicate_identity: bool,
    pub duplicate_identity_window_hours: u64,
    pub buy_sol: f64,
}

pub fn build_report(
    tokens: &[ReplayToken],
    fp: &EngineFingerprint,
    combos: &[GeneratedCombo],
    archive: &[ArchiveRow],
    n_matched: u64,
    incumbent: Option<&RuleParams>,
    incumbent_loaded: Option<LoadedRule>,
    opts: &ReplayOpts,
) -> Report {
    let mut diagnostics = Vec::new();
    let ranked = rank_archive(archive, combos);
    let fast_best = ranked.first().copied();

    // Report columns are `run_replay`. Replay the whole board (not a shorter
    // prefix) so a lower-ranked fast row cannot show a series-walk SOL the
    // champion never raced.
    let mut replayed: HashMap<usize, (ScoredRule, f64)> = HashMap::new();
    for &i in ranked.iter().take(TOP_ARCHIVE) {
        replayed.insert(i, replay_combo(tokens, fp, &combos[i].params, opts));
    }

    let incumbent_row = incumbent_loaded.as_ref().map(|loaded| {
        let (auth, opti) = replay_pair(tokens, fp, loaded, opts);
        let mut row = auth;
        row.total_pnl_sol_optimistic = Some(opti);
        row
    });
    let _ = incumbent;

    // Fill-moment rules can crowd the fast-archive head, then lose on replay.
    // One extra slice gives a quieter peak-contrast row a chance to be champion.
    if !slice_pays(&replayed) && ranked.len() > TOP_ARCHIVE {
        diagnostics.push(
            "No paying rule in the top archive slice; scoring the next slice.".into(),
        );
        for &i in ranked.iter().skip(TOP_ARCHIVE).take(TOP_ARCHIVE) {
            replayed.insert(i, replay_combo(tokens, fp, &combos[i].params, opts));
        }
    }

    let replay_best = replay_champion_idx(&replayed);

    let archive_replay_disagree = matches!((fast_best, replay_best), (Some(a), Some(b)) if a != b);
    if archive_replay_disagree {
        diagnostics.push(
            "Fast archive winner and replay winner disagree — champion is the replay one.".into(),
        );
    }

    let champ_idx = replay_best.or(fast_best);
    // Empty-entry shares the champion's exit bag, not the fast-archive winner's.
    let empty_idx = champ_idx.and_then(|i| {
        combos.iter().position(|c| {
            is_empty_entry(&c.entry) && same_exit_bag(&c.exit, &combos[i].exit)
        })
    });
    if let Some(i) = empty_idx {
        replayed
            .entry(i)
            .or_insert_with(|| replay_combo(tokens, fp, &combos[i].params, opts));
    }

    if let Some(i) = champ_idx {
        let phases: Vec<&str> = combos[i]
            .entry
            .clauses
            .iter()
            .chain(combos[i].exit.clauses.iter())
            .map(|c| c.phase.label())
            .collect();
        if !phases.is_empty() {
            diagnostics.push(format!("Champion cut phases: {}.", phases.join(", ")));
        }
        if let Some((row, opti)) = replayed.get(&i) {
            if row.total_pnl_sol < 0.0 && *opti > 0.0 {
                diagnostics.push(
                    "Champion fill window is violent: authority and first-in-window disagree in sign.".into(),
                );
            }
        }
    }
    let mut champion = champ_idx.and_then(|i| {
        replayed.get(&i).map(|(r, o)| {
            let mut row = r.clone();
            row.total_pnl_sol_optimistic = Some(*o);
            if let Some(a) = archive.get(i) {
                row.enter_pct_unguarded = Some(a.enter_pct(n_matched));
            }
            row
        })
    });

    let empty_entry = empty_idx.and_then(|i| {
        replayed.get(&i).map(|(r, o)| {
            let mut row = r.clone();
            row.total_pnl_sol_optimistic = Some(*o);
            row
        })
    });

    let verdict = verdict_of(
        champion.as_ref(),
        empty_entry.as_ref(),
        &replayed,
        combos,
        archive,
    );

    if matches!(verdict, Verdict::Refuse) {
        diagnostics.push("Refuse is a finished run — paper the next launch burst, or pick a new range.".into());
    }
    if matches!(verdict, Verdict::Ungated) {
        diagnostics.push("Juice is ungated: empty-entry (buy everything) is not beaten by a selector.".into());
    }

    let mut archive_out: Vec<ScoredRule> = ranked
        .iter()
        .filter(|i| replayed.contains_key(i))
        .filter_map(|&i| {
            replayed.get(&i).map(|(r, o)| {
                let mut row = r.clone();
                row.total_pnl_sol_optimistic = Some(*o);
                row
            })
        })
        .collect();
    archive_out.sort_by(|a, b| {
        b.total_pnl_sol
            .partial_cmp(&a.total_pnl_sol)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    archive_out.truncate(TOP_ARCHIVE);

    if champion.is_none() {
        champion = archive_out.first().cloned();
    }

    Report {
        verdict,
        n_matched,
        n_combos: combos.len() as u64,
        champion,
        empty_entry,
        incumbent: incumbent_row,
        archive: archive_out,
        archive_replay_disagree,
        diagnostics,
    }
}

fn rank_archive(archive: &[ArchiveRow], _combos: &[GeneratedCombo]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..archive.len()).collect();
    idx.sort_by(|&a, &b| {
        archive[b]
            .total_pnl_sol
            .partial_cmp(&archive[a].total_pnl_sol)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| archive[b].n_closed.cmp(&archive[a].n_closed))
    });
    idx
}

fn replay_champion_idx(replayed: &HashMap<usize, (ScoredRule, f64)>) -> Option<usize> {
    replayed
        .iter()
        .max_by(|a, b| {
            a.1 .0
                .total_pnl_sol
                .partial_cmp(&b.1 .0.total_pnl_sol)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.1 .0.n_closed.cmp(&b.1 .0.n_closed))
        })
        .map(|(&i, _)| i)
}

fn slice_pays(replayed: &HashMap<usize, (ScoredRule, f64)>) -> bool {
    replayed.values().any(|(r, _)| pays(r))
}

fn replay_combo(
    tokens: &[ReplayToken],
    fp: &EngineFingerprint,
    params: &RuleParams,
    opts: &ReplayOpts,
) -> (ScoredRule, f64) {
    let loaded = loaded_from_params(params.clone(), fp.id, opts.buy_sol, 0, 0);
    replay_pair(tokens, fp, &loaded, opts)
}

fn replay_pair(
    tokens: &[ReplayToken],
    fp: &EngineFingerprint,
    loaded: &LoadedRule,
    opts: &ReplayOpts,
) -> (ScoredRule, f64) {
    let auth = replay_one(tokens, fp, loaded, opts, opts.fill_authority);
    let opti = replay_one(tokens, fp, loaded, opts, opts.fill_optimistic);
    (auth, opti.total_pnl_sol)
}

fn replay_one(
    tokens: &[ReplayToken],
    fp: &EngineFingerprint,
    loaded: &LoadedRule,
    opts: &ReplayOpts,
    fill: FillModel,
) -> ScoredRule {
    let outcomes = run_replay(
        std::slice::from_ref(loaded),
        std::slice::from_ref(fp),
        tokens.to_vec(),
        ReplayConfig {
            as_of: opts.as_of,
            fill_model: fill,
            skip_duplicate_identity: opts.skip_duplicate_identity,
            duplicate_identity_window_hours: opts.duplicate_identity_window_hours,
        },
    );
    summarize(&outcomes, tokens.len() as u64, loaded.params.to_value(), opts.buy_sol, &opts.cost)
}

fn summarize(
    outcomes: &[PositionOutcome],
    n_matched: u64,
    params: serde_json::Value,
    buy_sol: f64,
    cost: &CostModel,
) -> ScoredRule {
    let mut agg = ComboAgg::default();
    let mut mints = std::collections::HashSet::new();
    for o in outcomes {
        mints.insert(o.mint.as_str());
        agg.record(&replay_to_token(o, buy_sol, cost));
    }
    let m = agg.finalize(0);
    let enter_pct = if n_matched == 0 {
        0.0
    } else {
        mints.len() as f64 / n_matched as f64
    };
    ScoredRule {
        params,
        n_fired: m.n_fired,
        n_closed: m.n_closed,
        n_tokens_entered: mints.len() as u64,
        enter_pct,
        enter_pct_unguarded: None,
        total_pnl_sol: m.total_pnl_sol,
        total_pnl_sol_optimistic: None,
        profit_factor: m.profit_factor,
        win_rate: m.win_rate,
    }
}

fn replay_to_token(po: &PositionOutcome, buy_sol: f64, cost: &CostModel) -> TokenOutcome {
    let (pnl_sol, pnl_pct) = po.pnl_with_costs(buy_sol, cost);
    TokenOutcome {
        fired: true,
        holding_secs: po
            .exit_time
            .map(|t| (t - po.entry_time).num_seconds())
            .unwrap_or(0),
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
        exit_metric: None,
        exit_operator: None,
        exit_metric_value: None,
        exit_metric_window: None,
        exit_metric_slot: None,
        entry_time: Some(po.entry_time),
        entry_price: Some(po.entry_price),
        entry_slot: None,
        exit_time: po.exit_time,
        exit_price: po.exit_price,
        exit_slot: None,
    }
}

fn pays(row: &ScoredRule) -> bool {
    row.n_closed >= MIN_CLOSED
        && row.total_pnl_sol > 0.0
        && row.profit_factor.map(|p| p > 1.0).unwrap_or(true)
}

fn verdict_of(
    champion: Option<&ScoredRule>,
    empty: Option<&ScoredRule>,
    replayed: &HashMap<usize, (ScoredRule, f64)>,
    combos: &[GeneratedCombo],
    archive: &[ArchiveRow],
) -> Verdict {
    let empty_pays = empty.map(pays).unwrap_or(false);
    let mut other_pays = champion.map(pays).unwrap_or(false);
    for (i, (row, _)) in replayed {
        if is_empty_entry(&combos[*i].entry) {
            continue;
        }
        if pays(row) {
            other_pays = true;
            break;
        }
    }
    if !other_pays {
        for (i, a) in archive.iter().enumerate() {
            if is_empty_entry(&combos[i].entry) {
                continue;
            }
            if a.pays(MIN_CLOSED) {
                other_pays = true;
                break;
            }
        }
    }

    if !empty_pays && !other_pays {
        return Verdict::Refuse;
    }
    if other_pays {
        if let (Some(ch), Some(em)) = (champion, empty) {
            if pays(ch) && ch.total_pnl_sol > em.total_pnl_sol + 1e-9 {
                return Verdict::Candidate;
            }
            if empty_pays && !pays(ch) {
                return Verdict::Ungated;
            }
            if pays(ch) && ch.total_pnl_sol <= em.total_pnl_sol + 1e-9 {
                return Verdict::Ungated;
            }
        }
        return Verdict::Candidate;
    }
    Verdict::Ungated
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pnl: f64, closed: u64, pf: f64) -> ScoredRule {
        ScoredRule {
            params: serde_json::json!({}),
            n_fired: closed,
            n_closed: closed,
            n_tokens_entered: closed,
            enter_pct: 0.2,
            enter_pct_unguarded: None,
            total_pnl_sol: pnl,
            total_pnl_sol_optimistic: None,
            profit_factor: Some(pf),
            win_rate: 0.5,
        }
    }

    #[test]
    fn all_lose_is_refuse() {
        let ch = row(-1.0, 20, 0.5);
        let em = row(-2.0, 20, 0.4);
        assert_eq!(
            verdict_of(Some(&ch), Some(&em), &HashMap::new(), &[], &[]),
            Verdict::Refuse
        );
    }

    #[test]
    fn empty_pays_others_lose_is_ungated() {
        let ch = row(-1.0, 20, 0.5);
        let em = row(2.0, 20, 1.5);
        assert_eq!(
            verdict_of(Some(&ch), Some(&em), &HashMap::new(), &[], &[]),
            Verdict::Ungated
        );
    }

    #[test]
    fn selector_beats_empty_is_candidate() {
        let ch = row(5.0, 20, 1.8);
        let em = row(1.0, 20, 1.2);
        assert_eq!(
            verdict_of(Some(&ch), Some(&em), &HashMap::new(), &[], &[]),
            Verdict::Candidate
        );
    }

    #[test]
    fn replay_picks_higher_authority() {
        let mut replayed = HashMap::new();
        replayed.insert(0, (row(-0.12, 20, 0.7), 0.545));
        replayed.insert(1, (row(0.113, 20, 1.2), 0.119));
        assert_eq!(replay_champion_idx(&replayed), Some(1));
        assert!(slice_pays(&replayed));
    }

    #[test]
    fn empty_slice_does_not_pay() {
        let mut replayed = HashMap::new();
        replayed.insert(0, (row(-0.12, 20, 0.7), 0.545));
        assert!(!slice_pays(&replayed));
    }
}
