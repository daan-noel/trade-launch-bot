//! Narrow **enrich** (charter D12) — the missing inverse of the narrow re-check.
//!
//! Every other stage of this job can only *remove*. The broad fit ranks whole
//! candidates, the ablation drops one term at a time, and the quota turns candidates
//! away. Nothing adds one. Combined with a measured fact — a broad fit is **blind** to
//! a term only one cohort needs (dropping `nonvol_buy >= 1.6 @2s` left two sibling
//! cohorts byte-identical and cost 10 points on the target) — the pipeline converges
//! on the sparse portable core by construction, and every cohort-specific idea is
//! washed out broad and never earned back.
//!
//! So after the fit picks a skeleton, this stage tries **adding** each earned idea the
//! finalist does not already carry, on the target cohort alone, and keeps what pays.
//!
//! **Each side is judged in its own currency (D11).** An entry idea earns its place by
//! making the rule *safer* — it must lift the win rate and must not cost return; an
//! exit alarm earns its place by making the rule *richer*. Judging both on return is
//! the recorded reason a search deletes every entry condition. Nothing is added on
//! faith: every accepted term arrives with the two numbers that bought it, and
//! everything tried and refused is reported too, so "more metrics" can never mean
//! "more knobs nobody checked".

use crate::rule_search::generator::{clause_label, Clause, EntryFilling, ExitBag, GeneratedCombo};
use crate::family_search::generator::reassemble;
use crate::family_search::score::CohortScore;

/// Ideas evaluated against the skeleton in one round. Bounds the authority passes:
/// each trial is one replay of one rule over the already-resident target cohort.
pub const MAX_TRIALS: usize = 12;

/// Terms a run may accept. Past three the rule stops being a thesis and starts being
/// a fit to one cohort's noise.
pub const MAX_ACCEPTED: usize = 3;

/// Win-rate points an **entry** idea must add before it is worth a clause.
pub const MIN_WIN_GAIN_PP: f64 = 1.0;

/// Return points an **exit** alarm must add before it is worth a clause.
pub const MIN_RET_GAIN_PP: f64 = 1.0;

/// Return points an entry idea may cost while buying safety. An entry gate trades a
/// little money for a lot of certainty; past this it is buying nothing.
pub const ENTRY_RET_TOLERANCE_PP: f64 = 2.0;

/// One idea offered to the skeleton, with what it did and whether it was taken.
#[derive(Clone, Debug, PartialEq)]
pub struct Trial {
    pub label: String,
    /// Which side it was offered to — the two are judged by different rules.
    pub is_entry: bool,
    pub ret_before_pct: f64,
    pub ret_after_pct: f64,
    pub win_before_pct: Option<f64>,
    pub win_after_pct: Option<f64>,
    pub n_closed_after: u64,
    pub accepted: bool,
    /// Why it was refused. `None` when accepted.
    pub refused: Option<&'static str>,
}

impl Trial {
    pub fn ret_delta_pct(&self) -> f64 {
        self.ret_after_pct - self.ret_before_pct
    }

    pub fn win_delta_pp(&self) -> Option<f64> {
        Some(self.win_after_pct? - self.win_before_pct?)
    }
}

/// Judge one trial in the currency of the side it sits on.
///
/// Returns `None` when the idea earns its place, or the reason it does not.
pub fn refusal(
    is_entry: bool,
    ret_delta_pct: f64,
    win_delta_pp: Option<f64>,
    n_closed_after: u64,
    min_closed: u64,
) -> Option<&'static str> {
    // An idea that trades the rule down to a handful of positions has not made it
    // safer, it has made it unmeasurable.
    if n_closed_after < min_closed {
        return Some("left too few closes to measure");
    }
    if is_entry {
        match win_delta_pp {
            None => Some("no win rate to compare"),
            Some(w) if w < MIN_WIN_GAIN_PP => Some("did not make the entry safer"),
            Some(_) if ret_delta_pct < -ENTRY_RET_TOLERANCE_PP => {
                Some("bought safety at too much return")
            }
            Some(_) => None,
        }
    } else if ret_delta_pct < MIN_RET_GAIN_PP {
        Some("did not make the exit richer")
    } else {
        None
    }
}

/// What one enrich round produced.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Enriched {
    /// The skeleton plus every accepted idea. `None` when nothing was accepted.
    pub combo: Option<GeneratedCombo>,
    /// Every idea offered, accepted or not, best first within each side.
    pub trials: Vec<Trial>,
    pub n_accepted: usize,
}

/// The scorer the enrich loop calls: replay one rule on the target cohort.
///
/// A generic parameter rather than a trait object so the closure may borrow the
/// resident corpus, and passed in rather than imported so this module stays testable
/// with no lake, no DB and no engine — the acceptance rules are the part worth pinning.
pub trait Score: Fn(&EntryFilling, &ExitBag) -> CohortScore {}
impl<F: Fn(&EntryFilling, &ExitBag) -> CohortScore> Score for F {}

/// Offer each unused idea to the skeleton and keep what pays.
///
/// Two passes, deliberately. The first scores every trial against the **same**
/// skeleton so the numbers are comparable and the ordering is not an artifact of
/// which idea happened to be tried first. The second re-scores the best ones **in
/// sequence**, because two ideas that each pay alone can be the same idea twice — and
/// only a confirming re-score on the growing rule can tell.
///
/// Cost is bounded at `MAX_TRIALS + MAX_ACCEPTED` replays of one rule over one
/// already-resident cohort, against a fit tier that folds every candidate across
/// every cohort.
pub fn enrich<S: Score>(
    skeleton: &GeneratedCombo,
    n_standing: usize,
    entry_menu: &[Clause],
    exit_menu: &[Clause],
    min_closed: u64,
    score: S,
) -> Enriched {
    let base = score(&skeleton.entry, &skeleton.exit);
    // Standing terms ride at the end of the bag and are never touched.
    let searched = skeleton.exit.clauses.len().saturating_sub(n_standing);
    let standing: Vec<Clause> = skeleton.exit.clauses[searched..].to_vec();

    // Interleave the two menus so a long list on one side cannot starve the other of
    // trials — safety and profit each get a fair share of the budget.
    let offers: Vec<(bool, Clause)> = interleave(entry_menu, exit_menu, MAX_TRIALS);

    let mut trials: Vec<Trial> = offers
        .iter()
        .map(|&(is_entry, c)| {
            let (entry, exit) = with_added(skeleton, &standing, searched, is_entry, &[c]);
            let s = score(&entry, &exit);
            let ret_after_pct = s.ret_pct();
            let win_after_pct = s.win_rate_pct();
            let ret_delta = ret_after_pct - base.ret_pct();
            let win_delta = win_after_pct.zip(base.win_rate_pct()).map(|(a, b)| a - b);
            Trial {
                label: clause_label(&c),
                is_entry,
                ret_before_pct: base.ret_pct(),
                ret_after_pct,
                win_before_pct: base.win_rate_pct(),
                win_after_pct,
                n_closed_after: s.n_closed,
                accepted: false,
                refused: refusal(is_entry, ret_delta, win_delta, s.n_closed, min_closed),
            }
        })
        .collect();

    // Best first in each side's own currency.
    let mut order: Vec<usize> = (0..trials.len()).collect();
    order.sort_by(|&a, &b| {
        let gain = |t: &Trial| {
            if t.is_entry {
                t.win_delta_pp().unwrap_or(f64::MIN)
            } else {
                t.ret_delta_pct()
            }
        };
        gain(&trials[b])
            .partial_cmp(&gain(&trials[a]))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| trials[a].label.cmp(&trials[b].label))
    });

    // Confirming pass: accept one at a time against the rule as it grows.
    let mut added_entry: Vec<Clause> = Vec::new();
    let mut added_exit: Vec<Clause> = Vec::new();
    let mut running = base;
    let mut n_accepted = 0usize;
    for &i in &order {
        if n_accepted >= MAX_ACCEPTED {
            trials[i].refused = trials[i].refused.or(Some("past the enrich budget"));
            continue;
        }
        if trials[i].refused.is_some() {
            continue;
        }
        let cand = offers[i].1;
        let is_entry = offers[i].0;
        let mut try_entry = added_entry.clone();
        let mut try_exit = added_exit.clone();
        if is_entry {
            try_entry.push(cand);
        } else {
            try_exit.push(cand);
        }
        let (entry, exit) =
            with_both(skeleton, &standing, searched, &try_entry, &try_exit);
        let s = score(&entry, &exit);
        let ret_delta = s.ret_pct() - running.ret_pct();
        let win_delta = s.win_rate_pct().zip(running.win_rate_pct()).map(|(a, b)| a - b);
        match refusal(is_entry, ret_delta, win_delta, s.n_closed, min_closed) {
            // It paid alone but adds nothing on top of what is already in — the same
            // idea twice, which only the confirming pass can see.
            Some(_) => trials[i].refused = Some("adds nothing beside the terms already accepted"),
            None => {
                added_entry = try_entry;
                added_exit = try_exit;
                running = s;
                trials[i].accepted = true;
                trials[i].ret_after_pct = s.ret_pct();
                trials[i].win_after_pct = s.win_rate_pct();
                trials[i].n_closed_after = s.n_closed;
                n_accepted += 1;
            }
        }
    }

    let combo = (n_accepted > 0).then(|| {
        let (entry, exit) =
            with_both(skeleton, &standing, searched, &added_entry, &added_exit);
        GeneratedCombo { params: reassemble(&entry, &exit), entry, exit }
    });
    // Report in the order the confirming pass considered them, so the board reads
    // top-down as the run decided.
    let ordered: Vec<Trial> = order.iter().map(|&i| trials[i].clone()).collect();
    Enriched { combo, trials: ordered, n_accepted }
}

/// Round-robin the two menus into one trial budget.
fn interleave(entry: &[Clause], exit: &[Clause], budget: usize) -> Vec<(bool, Clause)> {
    let mut out = Vec::new();
    let mut i = 0;
    while out.len() < budget && (i < entry.len() || i < exit.len()) {
        if let Some(c) = exit.get(i) {
            out.push((false, *c));
        }
        if out.len() < budget {
            if let Some(c) = entry.get(i) {
                out.push((true, *c));
            }
        }
        i += 1;
    }
    out
}

fn with_added(
    skeleton: &GeneratedCombo,
    standing: &[Clause],
    searched: usize,
    is_entry: bool,
    add: &[Clause],
) -> (EntryFilling, ExitBag) {
    if is_entry {
        with_both(skeleton, standing, searched, add, &[])
    } else {
        with_both(skeleton, standing, searched, &[], add)
    }
}

/// Rebuild the rule with extra clauses on either side, standing terms kept **last**
/// so the searched/standing split downstream stays a slice.
fn with_both(
    skeleton: &GeneratedCombo,
    standing: &[Clause],
    searched: usize,
    add_entry: &[Clause],
    add_exit: &[Clause],
) -> (EntryFilling, ExitBag) {
    let mut entry = skeleton.entry.clauses.clone();
    entry.extend_from_slice(add_entry);
    let mut exit = skeleton.exit.clauses[..searched].to_vec();
    exit.extend_from_slice(add_exit);
    exit.extend_from_slice(standing);
    (EntryFilling { clauses: entry }, ExitBag { clauses: exit })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule_search::cuts::CutPhase;
    use hunter_engine::metrics::evaluator::Operator;
    use hunter_engine::metrics::{group_of, MetricId};

    fn clause(metric: MetricId, op: Operator, threshold: f64, window: Option<f64>) -> Clause {
        Clause { group: group_of(metric).id, metric, window, op, threshold, phase: CutPhase::DumpLead }
    }

    fn skeleton() -> GeneratedCombo {
        let entry = EntryFilling { clauses: vec![clause(MetricId::Liquidity, Operator::Gt, 30.0, None)] };
        let exit = ExitBag {
            clauses: vec![
                clause(MetricId::Stall, Operator::Gte, 30.0, None),
                // The standing tail: sell at migration, never searched, never dropped.
                clause(MetricId::Liquidity, Operator::Gte, 85.0, None),
            ],
        };
        GeneratedCombo { params: reassemble(&entry, &exit), entry, exit }
    }

    fn s(ret_pct: f64, win_pct: f64, n_closed: u64) -> CohortScore {
        CohortScore {
            pnl_sol: ret_pct / 100.0,
            entry_sol: 1.0,
            n_closed,
            n_wins: ((win_pct / 100.0) * n_closed as f64).round() as u64,
        }
    }

    fn has(f: &EntryFilling, m: MetricId) -> bool {
        f.clauses.iter().any(|c| c.metric == m)
    }
    fn bag_has(b: &ExitBag, m: MetricId, v: f64) -> bool {
        b.clauses.iter().any(|c| c.metric == m && (c.threshold - v).abs() < 1e-9)
    }

    /// The whole point of the stage: a term the broad fit is blind to gets added back
    /// on the cohort that needs it — and the entry side earns its place with SAFETY.
    #[test]
    fn an_entry_idea_that_buys_win_rate_is_accepted_even_when_it_costs_a_little_return() {
        let entry_menu = [clause(MetricId::Time, Operator::Gte, 20.0, None)];
        let exit_menu = [clause(MetricId::WinNonvolBuy, Operator::Gte, 1.6, Some(2.0))];

        let score = |e: &EntryFilling, x: &ExitBag| {
            let time = has(e, MetricId::Time);
            let organic = bag_has(x, MetricId::WinNonvolBuy, 1.6);
            match (time, organic) {
                (false, false) => s(21.0, 48.0, 200),
                // +14pp of win rate for −1pp of return: exactly the trade an entry
                // gate exists to make.
                (true, false) => s(20.0, 62.0, 150),
                (false, true) => s(31.0, 50.0, 200),
                (true, true) => s(30.0, 63.0, 150),
            }
        };

        let out = enrich(&skeleton(), 1, &entry_menu, &exit_menu, 8, &score);
        assert_eq!(out.n_accepted, 2, "{:#?}", out.trials);

        let combo = out.combo.expect("an enriched rule");
        assert!(has(&combo.entry, MetricId::Liquidity) && has(&combo.entry, MetricId::Time));
        assert!(bag_has(&combo.exit, MetricId::Stall, 30.0));
        assert!(bag_has(&combo.exit, MetricId::WinNonvolBuy, 1.6));

        // The standing term survives, still last, still exactly once.
        assert_eq!(
            combo.exit.clauses.iter().filter(|c| c.metric == MetricId::Liquidity).count(),
            1
        );
        assert_eq!(combo.exit.clauses.last().unwrap().threshold, 85.0);

        // Both accepted rows carry the numbers that bought them.
        let time = out.trials.iter().find(|t| t.label.contains("time")).expect("time trial");
        assert!(time.accepted && time.win_delta_pp().unwrap() > 13.0);
        assert!(time.ret_delta_pct() < 0.0, "it cost return and was still worth it");
    }

    /// Density has to be earned. An idea that changes nothing is reported as tried
    /// and refused, never quietly welded on.
    #[test]
    fn an_idea_that_pays_nothing_is_refused_with_its_reason() {
        let entry_menu = [clause(MetricId::Time, Operator::Gte, 20.0, None)];
        let exit_menu = [clause(MetricId::GrossFlow, Operator::Lt, 15.0, Some(10.0))];
        // Flat whatever is added.
        let score = |_: &EntryFilling, _: &ExitBag| s(21.0, 48.0, 200);

        let out = enrich(&skeleton(), 1, &entry_menu, &exit_menu, 8, &score);
        assert_eq!(out.n_accepted, 0);
        assert!(out.combo.is_none(), "no accepted term ⇒ the skeleton stands");
        assert_eq!(out.trials.len(), 2);
        assert!(out.trials.iter().all(|t| !t.accepted && t.refused.is_some()));
        assert!(out
            .trials
            .iter()
            .any(|t| t.refused == Some("did not make the exit richer")));
        assert!(out
            .trials
            .iter()
            .any(|t| t.refused == Some("did not make the entry safer")));
    }

    /// Two ideas that each pay alone can be the same idea twice. Only a confirming
    /// re-score on the growing rule catches it.
    #[test]
    fn the_second_of_two_redundant_ideas_is_refused_on_the_confirming_pass() {
        let a = clause(MetricId::GrossFlow, Operator::Lt, 15.0, Some(10.0));
        let b = clause(MetricId::Buy, Operator::Lt, 3.0, Some(10.0));
        let exit_menu = [a, b];

        let score = move |_: &EntryFilling, x: &ExitBag| {
            let has_a = bag_has(x, MetricId::GrossFlow, 15.0);
            let has_b = bag_has(x, MetricId::Buy, 3.0);
            // Either one lifts the rule to +31%; both together add nothing further.
            match (has_a, has_b) {
                (false, false) => s(21.0, 50.0, 200),
                _ => s(31.0, 55.0, 180),
            }
        };

        let out = enrich(&skeleton(), 1, &[], &exit_menu, 8, &score);
        assert_eq!(out.n_accepted, 1, "{:#?}", out.trials);
        let refused = out.trials.iter().find(|t| !t.accepted).expect("one refused");
        assert_eq!(refused.refused, Some("adds nothing beside the terms already accepted"));
        // It looked like a winner in the first pass — that is exactly the trap.
        assert!(refused.ret_delta_pct() > MIN_RET_GAIN_PP);
    }

    /// An entry gate that trades the rule down to a handful of positions has not made
    /// it safer, it has made it unmeasurable.
    #[test]
    fn an_idea_that_starves_the_rule_of_closes_is_refused() {
        assert_eq!(
            refusal(true, 0.0, Some(40.0), 4, 8),
            Some("left too few closes to measure")
        );
        // And buying a lot of safety with too much return is still a refusal.
        assert_eq!(
            refusal(true, -9.0, Some(20.0), 100, 8),
            Some("bought safety at too much return")
        );
        // Inside tolerance it stands.
        assert_eq!(refusal(true, -1.0, Some(20.0), 100, 8), None);
        assert_eq!(refusal(false, 5.0, None, 100, 8), None);
    }

    #[test]
    fn the_trial_budget_is_shared_between_the_two_sides() {
        let e: Vec<Clause> =
            (0..20).map(|i| clause(MetricId::Time, Operator::Gte, i as f64, None)).collect();
        let x: Vec<Clause> = (0..20)
            .map(|i| clause(MetricId::Stall, Operator::Gte, 100.0 + i as f64, None))
            .collect();
        let picked = interleave(&e, &x, MAX_TRIALS);
        assert_eq!(picked.len(), MAX_TRIALS);
        // Neither side starves the other out of the budget.
        assert_eq!(picked.iter().filter(|(is_entry, _)| *is_entry).count(), MAX_TRIALS / 2);
    }
}
