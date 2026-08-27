//! Reliability diagnostics on the finalist (Slice 7) — depth without selection.
//!
//! Every stage here **grades** the draft; none may pick or tune it. The target
//! cohort is the held-out set: the ranking is fitted on the siblings and only
//! validated here, so every extra decision the target is allowed to make — a
//! threshold nudged, a clause kept because it scores well *here* — leaks the
//! held-out set and quietly converts validation numbers back into fit numbers.
//! That is why enrich is capped at three accepts, and why everything in this
//! module reports a verdict instead of feeding one back into the search. A failed
//! diagnostic means "do not promote" or "re-run on another family", never
//! "auto-tune".
//!
//! Four instruments, each answering a question no earlier stage sees:
//!
//! 1. **Threshold ladders** — is each clause's cut a *plateau* (neighbours score
//!    alike ⇒ the clause captures something real) or a *spike* (one lucky value)?
//!    The recorded failure mode is "the entry band is usually the cost", and a
//!    single-point test cannot see it.
//! 2. **Alarm regret** — was firing *right*? Each alarm's closes against the best
//!    exit still available afterwards (forfeited) and against holding to the last
//!    print (saved). Attribution says which alarm made the money; this says
//!    whether it fired at the right time.
//! 3. **Entry redundancy** — solo scores and veto-set overlap. Drop-one ablation
//!    is first-order only: two correlated clauses each look near-worthless alone
//!    because the other covers for them.
//! 4. **Per-clause fill sensitivity** — a clause whose measured contribution
//!    evaporates or flips under the optimistic fill is selecting fill luck, not
//!    signal. The whole-rule spread already exists; this is the same question per
//!    clause.
//!
//! Cost: ~`6·(E+X)` ladder replays plus `2·(E+X) + E + 1` variant replays of one
//! rule over the already-resident target cohort — the tier where replays are
//! near-free. The Wilson bound on the win bar lives in
//! [`score::wilson_low_pct`](super::score::wilson_low_pct) and costs arithmetic.

use std::collections::{BTreeMap, HashSet};

use hunter_engine::event::format_metric_exit_label;
use hunter_engine::fingerprint::Fingerprint as EngineFingerprint;
use hunter_engine::metrics::MetricId;
use trading_core::strategies::kernel::{weighted_return_pct, CostModel, ExitCode};

use crate::rule_search::generator::{assemble, clause_label, EntryFilling, ExitBag};
use crate::sweep::corpus::CorpusToken;
use crate::sweep::generic::Pricing;

use super::generator::Candidate;
use super::oracle::{best_after_pnl_sol, terminal_pnl_sol};
use super::score::CohortScore;
use super::{authority, Authority, RunConfig, FILL_OPTIMISTIC};

// ── Threshold ladders ───────────────────────────────────────────────────────

/// Multipliers on the chosen threshold, the chosen value excluded (it is read off
/// the finalist's own authority pass for free). Multiplicative so a negative level
/// (`pnl <= -8`) ladders through negative neighbours instead of crossing zero.
pub const LADDER_FACTORS: [f64; 6] = [0.5, 0.7, 0.85, 1.15, 1.3, 1.5];

/// Grade span (max − min over the ladder, in the side's own currency) below which
/// the clause is *flat* — the cohort barely responds to the threshold at all.
pub const FLAT_SPAN_PP: f64 = 1.0;

/// One threshold tried, with both currencies so a reader can see the trade.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LadderPoint {
    pub threshold: f64,
    pub ret_pct: f64,
    pub win_pct: Option<f64>,
    pub n_closed: u64,
    /// The finalist's own value — the point the verdict is about.
    pub chosen: bool,
}

/// What moving one clause's threshold does, everything else held fixed.
#[derive(Clone, Debug, PartialEq)]
pub struct ThresholdLadder {
    pub clause: String,
    pub is_entry: bool,
    /// Ascending by threshold, the chosen point included.
    pub points: Vec<LadderPoint>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LadderVerdict {
    /// The cohort barely responds to this threshold — robust by indifference.
    Flat,
    /// Neighbours hold most of the chosen value's grade — the cut is a region.
    Plateau,
    /// The chosen value is the unique best and one step either way loses more
    /// than half the ladder's whole range — tuned to this cohort's noise.
    Fragile,
    /// Too few measurable points to say (a zero threshold cannot ladder
    /// multiplicatively, or the neighbours closed nothing).
    Sparse,
}

impl LadderVerdict {
    pub fn label(self) -> &'static str {
        match self {
            Self::Flat => "flat",
            Self::Plateau => "plateau",
            Self::Fragile => "fragile",
            Self::Sparse => "sparse",
        }
    }
}

impl ThresholdLadder {
    /// The side's own currency (D11): win rate for an entry clause, return for an
    /// exit alarm. Grading both on return is the recorded way entry logic dies.
    fn grade(&self, p: &LadderPoint) -> Option<f64> {
        if self.is_entry {
            p.win_pct
        } else {
            Some(p.ret_pct)
        }
    }

    pub fn verdict(&self) -> LadderVerdict {
        let graded: Vec<(f64, bool)> = self
            .points
            .iter()
            .filter_map(|p| self.grade(p).map(|g| (g, p.chosen)))
            .collect();
        if graded.len() < 3 || !graded.iter().any(|(_, c)| *c) {
            return LadderVerdict::Sparse;
        }
        let min = graded.iter().map(|(g, _)| *g).fold(f64::INFINITY, f64::min);
        let max = graded.iter().map(|(g, _)| *g).fold(f64::NEG_INFINITY, f64::max);
        let span = max - min;
        if span <= FLAT_SPAN_PP {
            return LadderVerdict::Flat;
        }
        let at = graded.iter().position(|(_, c)| *c).unwrap();
        let g0 = graded[at].0;
        if g0 < max {
            // A neighbour does as well or better: the chosen value is not a spike,
            // whatever else it is.
            return LadderVerdict::Plateau;
        }
        // Chosen is the unique max; fragile only if EVERY immediate neighbour gives
        // back more than half the whole range.
        let steep = |i: usize| g0 - graded[i].0 > 0.5 * span;
        let left_steep = at == 0 || steep(at - 1);
        let right_steep = at + 1 >= graded.len() || steep(at + 1);
        let has_neighbour = at > 0 || at + 1 < graded.len();
        if has_neighbour && left_steep && right_steep {
            LadderVerdict::Fragile
        } else {
            LadderVerdict::Plateau
        }
    }
}

// ── Alarm regret ────────────────────────────────────────────────────────────

/// One alarm's closes, graded against its two counterfactuals — both computed from
/// data already resident (the oracle's suffix peak and the last print), zero
/// replays.
#[derive(Clone, Debug, PartialEq)]
pub struct AlarmRegret {
    pub slot: u8,
    pub label: Option<String>,
    /// A mechanical exit the operator asked for — reported, never a finding.
    pub standing: bool,
    /// Positions this alarm closed.
    pub n: u64,
    /// Of those, the ones with a print after the close to grade against.
    pub n_priced: u64,
    /// Money over capital across the priced closes — realized, and what the best
    /// still-available exit would have paid on the same set.
    pub realized_ret_pct: f64,
    pub best_later_ret_pct: f64,
    /// Closes with a usable last print (the hold-to-the-end counterfactual).
    pub n_terminal: u64,
    /// Realized minus hold-to-the-end, over that set. Positive ⇒ firing beat
    /// holding on.
    pub realized_vs_terminal_pp: f64,
    /// One round trip at this run's buy size — upside inside it is not forfeitable.
    pub band_pct: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegretVerdict {
    /// Nothing beyond a round trip was left after the close.
    Timed,
    /// It left money, but firing still beat holding to the end — early and right.
    Protective,
    /// It left money AND holding on would have paid more — the alarm cuts winners
    /// on this cohort rather than catching dumps.
    Premature,
    /// No close had a print after it to grade against.
    Unmeasured,
}

impl RegretVerdict {
    pub fn label(self) -> &'static str {
        match self {
            Self::Timed => "timed",
            Self::Protective => "protective",
            Self::Premature => "premature",
            Self::Unmeasured => "unmeasured",
        }
    }
}

impl AlarmRegret {
    /// Points the best still-available exit beat the realized close by. Negative
    /// means the alarm out-timed everything that printed after it.
    pub fn forfeit_pp(&self) -> f64 {
        self.best_later_ret_pct - self.realized_ret_pct
    }

    pub fn verdict(&self) -> RegretVerdict {
        if self.n_priced == 0 {
            return RegretVerdict::Unmeasured;
        }
        if self.forfeit_pp() <= self.band_pct {
            RegretVerdict::Timed
        } else if self.realized_vs_terminal_pp > 0.0 {
            RegretVerdict::Protective
        } else {
            RegretVerdict::Premature
        }
    }
}

/// `(metric, window identity, threshold-in-µ)` — the standing-term identity,
/// float-keyed the same way the engine keys windows so `2.0` never sorts apart from
/// itself. The window half carries the whole span: two terms that differ only in
/// unit or lag read DIFFERENT tape and must not collapse onto one key.
fn skey(
    metric: MetricId,
    window: Option<hunter_engine::metrics::WindowSpec>,
    value: f64,
) -> (MetricId, i64, i64, i64, i64) {
    let (unit, size, lag) = match window {
        Some(w) => (
            match w.unit {
                hunter_engine::metrics::WindowUnit::Sec => 0,
                hunter_engine::metrics::WindowUnit::Slot => 1,
            },
            hunter_engine::metrics::quantize(w.size) as i64,
            hunter_engine::metrics::quantize(w.lag) as i64,
        ),
        None => (-1, -1, -1),
    };
    (metric, unit, size, lag, (value * 1_000_000.0).round() as i64)
}

/// Fold one authority pass into per-alarm regret rows. Pure over outcomes in hand.
pub fn alarm_regret(
    tokens: &[CorpusToken],
    auth: &Authority,
    pricing: &Pricing,
    band_pct: f64,
    standing_keys: &[(MetricId, Option<hunter_engine::metrics::WindowSpec>, f64)],
) -> Vec<AlarmRegret> {
    #[derive(Default)]
    struct Acc {
        label: Option<String>,
        standing: bool,
        n: u64,
        n_priced: u64,
        realized_priced_sol: f64,
        best_later_sol: f64,
        n_terminal: u64,
        realized_term_sol: f64,
        terminal_sol: f64,
    }
    let standing: HashSet<(MetricId, i64, i64, i64, i64)> =
        standing_keys.iter().map(|&(m, w, v)| skey(m, w, v)).collect();
    let mut by_slot: BTreeMap<u8, Acc> = BTreeMap::new();

    for (o, &ti) in auth.outcomes.iter().zip(&auth.token_idx) {
        if o.exit != ExitCode::Metrics {
            continue;
        }
        let (Some(slot), Some(metric), Some(op), Some(value)) =
            (o.exit_metric_slot, o.exit_metric, o.exit_operator, o.exit_metric_value)
        else {
            continue;
        };
        let acc = by_slot.entry(slot).or_default();
        acc.n += 1;
        if acc.label.is_none() {
            acc.label = Some(format_metric_exit_label(metric, op, value, o.exit_metric_window));
            acc.standing = standing.contains(&skey(metric, o.exit_metric_window, value));
        }
        let (Some(token), Some(exit_at)) = (tokens.get(ti), o.exit_time) else { continue };
        if let Some(best) = best_after_pnl_sol(token, o, exit_at, pricing) {
            acc.n_priced += 1;
            acc.realized_priced_sol += o.pnl_sol as f64;
            acc.best_later_sol += best;
        }
        if let Some(term) = terminal_pnl_sol(token, o, pricing) {
            acc.n_terminal += 1;
            acc.realized_term_sol += o.pnl_sol as f64;
            acc.terminal_sol += term;
        }
    }

    by_slot
        .into_iter()
        .map(|(slot, a)| {
            let priced_entry = a.n_priced as f64 * pricing.buy_amount_sol;
            let term_entry = a.n_terminal as f64 * pricing.buy_amount_sol;
            AlarmRegret {
                slot,
                label: a.label,
                standing: a.standing,
                n: a.n,
                n_priced: a.n_priced,
                realized_ret_pct: weighted_return_pct(a.realized_priced_sol, priced_entry),
                best_later_ret_pct: weighted_return_pct(a.best_later_sol, priced_entry),
                n_terminal: a.n_terminal,
                realized_vs_terminal_pp: weighted_return_pct(a.realized_term_sol, term_entry)
                    - weighted_return_pct(a.terminal_sol, term_entry),
                band_pct,
            }
        })
        .collect()
}

// ── Entry redundancy ────────────────────────────────────────────────────────

/// Share of one clause's vetoes another clause also vetoes, at or above which the
/// first is flagged redundant — its filtering is a subset of a sibling's.
pub const REDUNDANT_OVERLAP_PCT: f64 = 90.0;

/// Vetoes below which overlap is noise, not structure.
pub const MIN_VETOES: u64 = 5;

/// One entry clause's standing in the AND: what it filters alone, and how much of
/// that filtering another clause already does.
#[derive(Clone, Debug, PartialEq)]
pub struct EntryRedundancy {
    pub clause: String,
    /// Tokens this clause alone turns away from the full rule's entry set.
    pub n_vetoed: u64,
    /// The clause by itself (full exit bag kept) — the third point that triangulates
    /// full / drop-one / solo into synergy, redundancy, or dead weight.
    pub solo_ret_pct: f64,
    pub solo_win_pct: Option<f64>,
    pub solo_n_closed: u64,
    /// Largest share of this clause's vetoes another clause also vetoes.
    pub max_overlap_pct: Option<f64>,
    pub overlap_with: Option<String>,
}

impl EntryRedundancy {
    /// Nearly every token this clause turns away, another clause turns away too —
    /// dead weight even when drop-one ablation shows a small delta, because the
    /// sibling covers for it.
    pub fn redundant(&self) -> bool {
        self.n_vetoed >= MIN_VETOES
            && self.max_overlap_pct.is_some_and(|o| o >= REDUNDANT_OVERLAP_PCT)
    }
}

// ── Per-clause fill sensitivity ─────────────────────────────────────────────

/// Deltas below this (in the side's own currency) are noise under either pricing.
pub const FILL_NOISE_PP: f64 = 0.5;

/// A contribution that keeps under this fraction of itself across the two pricings
/// is fill-shaped, not signal-shaped.
pub const FILL_SHRINK_RATIO: f64 = 0.25;

/// One clause's drop-one contribution, measured under both pricings.
#[derive(Clone, Debug, PartialEq)]
pub struct ClauseFill {
    pub clause: String,
    pub is_entry: bool,
    /// Contribution in the side's own currency at the run's authority pricing —
    /// win-rate points for an entry clause, return points for an exit alarm.
    pub delta_authority: Option<f64>,
    /// The same, under [`FILL_OPTIMISTIC`] + fee-only cost.
    pub delta_optimistic: Option<f64>,
}

impl ClauseFill {
    /// The clause's measured contribution depends on the fill model — it flips sign
    /// or keeps under a quarter of itself between the two pricings. Such a clause is
    /// priced on fill luck; the dump-scalp family is the recorded case where the
    /// entire "edge" lived in this gap.
    pub fn fill_dependent(&self) -> bool {
        let (Some(a), Some(o)) = (self.delta_authority, self.delta_optimistic) else {
            return false;
        };
        let (hi, lo) = (a.abs().max(o.abs()), a.abs().min(o.abs()));
        if hi <= FILL_NOISE_PP {
            return false;
        }
        a.signum() != o.signum() || lo < FILL_SHRINK_RATIO * hi
    }
}

// ── Orchestrator ────────────────────────────────────────────────────────────

/// Everything Slice 7 measures about one finalist on the resident target cohort.
#[derive(Clone, Debug, Default)]
pub struct Diagnostics {
    pub ladders: Vec<ThresholdLadder>,
    pub regret: Vec<AlarmRegret>,
    pub redundancy: Vec<EntryRedundancy>,
    pub fill_sensitivity: Vec<ClauseFill>,
}

/// Run the four instruments. `auth` is the finalist's own authority pass — its score
/// supplies every "full" number, so the chosen ladder point and the drop-one deltas
/// cost no extra replay of the rule as it stands.
pub fn diagnose(
    tokens: &[CorpusToken],
    fp: &EngineFingerprint,
    finalist: &Candidate,
    auth: &Authority,
    cfg: &RunConfig,
    band_pct: f64,
    standing_keys: &[(MetricId, Option<hunter_engine::metrics::WindowSpec>, f64)],
) -> Diagnostics {
    let combo = &finalist.combo;
    let entry = &combo.entry.clauses;
    let searched = finalist.searched_exit().len();
    let full = auth.score;

    let run = |e: &EntryFilling, x: &ExitBag, c: &RunConfig| -> Authority {
        authority(tokens, fp, &assemble(e, x), c)
    };
    let mut opt_cfg = cfg.clone();
    opt_cfg.pricing.fill_model = FILL_OPTIMISTIC;
    opt_cfg.pricing.cost = CostModel::pumpfun_fee_only();
    let opt_full = authority(tokens, fp, &combo.params, &opt_cfg);

    // Drop-one variants, each under both pricings. The entry ones also keep their
    // entered-mint sets — the veto sets the overlap reads.
    let entry_without: Vec<(Authority, CohortScore)> = (0..entry.len())
        .map(|i| {
            let mut cs = entry.clone();
            cs.remove(i);
            let e = EntryFilling { clauses: cs };
            (run(&e, &combo.exit, cfg), run(&e, &combo.exit, &opt_cfg).score)
        })
        .collect();
    let exit_without: Vec<(CohortScore, CohortScore)> = (0..searched)
        .map(|i| {
            let mut cs = combo.exit.clauses.clone();
            cs.remove(i);
            let x = ExitBag { clauses: cs };
            (run(&combo.entry, &x, cfg).score, run(&combo.entry, &x, &opt_cfg).score)
        })
        .collect();

    // ── Fill sensitivity: contribution under both pricings, own currency. ──
    let mut fill_sensitivity = Vec::new();
    for (c, (wa, wo)) in entry.iter().zip(&entry_without) {
        fill_sensitivity.push(ClauseFill {
            clause: clause_label(c),
            is_entry: true,
            delta_authority: full
                .win_rate_pct()
                .zip(wa.score.win_rate_pct())
                .map(|(f, w)| f - w),
            delta_optimistic: opt_full
                .score
                .win_rate_pct()
                .zip(wo.win_rate_pct())
                .map(|(f, w)| f - w),
        });
    }
    for (c, (wa, wo)) in combo.exit.clauses[..searched].iter().zip(&exit_without) {
        fill_sensitivity.push(ClauseFill {
            clause: clause_label(c),
            is_entry: false,
            delta_authority: Some(full.ret_pct() - wa.ret_pct()),
            delta_optimistic: Some(opt_full.score.ret_pct() - wo.ret_pct()),
        });
    }

    // ── Redundancy: solo scores + veto-set overlap off the drop-one passes. ──
    let mints_of = |a: &Authority| -> HashSet<&str> {
        a.token_idx.iter().filter_map(|&ti| tokens.get(ti)).map(|t| t.mint.as_str()).collect()
    };
    let full_mints = mints_of(auth);
    let vetoes: Vec<HashSet<&str>> = entry_without
        .iter()
        .map(|(wa, _)| mints_of(wa).difference(&full_mints).copied().collect())
        .collect();
    let redundancy = entry
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let solo = run(
                &EntryFilling { clauses: vec![*c] },
                &combo.exit,
                cfg,
            )
            .score;
            let (max_overlap_pct, overlap_with) = vetoes
                .iter()
                .enumerate()
                .filter(|&(j, _)| j != i)
                .filter(|_| !vetoes[i].is_empty())
                .map(|(j, v)| {
                    let shared = vetoes[i].intersection(v).count();
                    (100.0 * shared as f64 / vetoes[i].len() as f64, clause_label(&entry[j]))
                })
                .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
                .map_or((None, None), |(o, l)| (Some(o), Some(l)));
            EntryRedundancy {
                clause: clause_label(c),
                n_vetoed: vetoes[i].len() as u64,
                solo_ret_pct: solo.ret_pct(),
                solo_win_pct: solo.win_rate_pct(),
                solo_n_closed: solo.n_closed,
                max_overlap_pct,
                overlap_with,
            }
        })
        .collect();

    // ── Threshold ladders, both sides, chosen point free off `auth`. ───────
    let mut ladders = Vec::new();
    let chosen_point = |t: f64| LadderPoint {
        threshold: t,
        ret_pct: full.ret_pct(),
        win_pct: full.win_rate_pct(),
        n_closed: full.n_closed,
        chosen: true,
    };
    let point = |t: f64, s: CohortScore| LadderPoint {
        threshold: t,
        ret_pct: s.ret_pct(),
        win_pct: s.win_rate_pct(),
        n_closed: s.n_closed,
        chosen: false,
    };
    for (side_entry, i, c) in entry
        .iter()
        .enumerate()
        .map(|(i, c)| (true, i, c))
        .chain(combo.exit.clauses[..searched].iter().enumerate().map(|(i, c)| (false, i, c)))
    {
        let t0 = c.threshold;
        let mut points = vec![chosen_point(t0)];
        if t0 != 0.0 {
            for &f in &LADDER_FACTORS {
                let mut cl = *c;
                cl.threshold = t0 * f;
                let s = if side_entry {
                    let mut cs = entry.clone();
                    cs[i] = cl;
                    run(&EntryFilling { clauses: cs }, &combo.exit, cfg).score
                } else {
                    let mut cs = combo.exit.clauses.clone();
                    cs[i] = cl;
                    run(&combo.entry, &ExitBag { clauses: cs }, cfg).score
                };
                points.push(point(t0 * f, s));
            }
        }
        points.sort_by(|a, b| {
            a.threshold.partial_cmp(&b.threshold).unwrap_or(std::cmp::Ordering::Equal)
        });
        ladders.push(ThresholdLadder { clause: clause_label(c), is_entry: side_entry, points });
    }

    Diagnostics {
        ladders,
        regret: alarm_regret(tokens, auth, &cfg.pricing, band_pct, standing_keys),
        redundancy,
        fill_sensitivity,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ladder(is_entry: bool, grades: &[(f64, f64, bool)]) -> ThresholdLadder {
        ThresholdLadder {
            clause: "t".into(),
            is_entry,
            points: grades
                .iter()
                .map(|&(t, g, chosen)| LadderPoint {
                    threshold: t,
                    ret_pct: g,
                    win_pct: Some(g),
                    n_closed: 100,
                    chosen,
                })
                .collect(),
        }
    }

    /// The distinction this instrument exists for: a cut that is a region versus a
    /// cut that is one lucky value.
    #[test]
    fn a_spike_reads_fragile_and_a_region_reads_plateau() {
        // Neighbours hold most of the grade: a region.
        let plateau =
            ladder(false, &[(0.5, 28.0), (0.85, 30.0), (1.0, 31.0), (1.15, 29.5), (1.5, 24.0)]
                .map(|(t, g)| (t, g, t == 1.0)));
        assert_eq!(plateau.verdict(), LadderVerdict::Plateau);

        // One step either way gives back most of the range: a spike.
        let spike = ladder(false, &[(0.5, 2.0), (0.85, 4.0), (1.0, 31.0), (1.15, 6.0), (1.5, 1.0)]
            .map(|(t, g)| (t, g, t == 1.0)));
        assert_eq!(spike.verdict(), LadderVerdict::Fragile);

        // A neighbour doing BETTER is never a spike — the value is not even the max.
        let beaten = ladder(false, &[(0.5, 2.0), (1.0, 31.0), (1.15, 33.0)]
            .map(|(t, g)| (t, g, t == 1.0)));
        assert_eq!(beaten.verdict(), LadderVerdict::Plateau);
    }

    #[test]
    fn an_indifferent_cohort_reads_flat_and_a_thin_ladder_reads_sparse() {
        let flat = ladder(false, &[(0.5, 30.6), (1.0, 31.0), (1.5, 30.2)]
            .map(|(t, g)| (t, g, t == 1.0)));
        assert_eq!(flat.verdict(), LadderVerdict::Flat);

        let sparse = ladder(false, &[(1.0, 31.0, true), (1.5, 2.0, false)]);
        assert_eq!(sparse.verdict(), LadderVerdict::Sparse);

        // An entry ladder whose neighbours closed nothing has no win rates to
        // compare — sparse, never a verdict invented from the one measurable point.
        let mut no_wins = ladder(true, &[(0.5, 0.0, false), (1.0, 62.0, true), (1.5, 0.0, false)]);
        for p in &mut no_wins.points {
            if !p.chosen {
                p.win_pct = None;
            }
        }
        assert_eq!(no_wins.verdict(), LadderVerdict::Sparse);
    }

    /// An entry ladder grades on WIN RATE — return moving while the win rate holds
    /// still must not accuse the clause of fragility.
    #[test]
    fn an_entry_ladder_grades_on_win_rate_not_return() {
        let mut l = ladder(true, &[(0.5, 0.0, false), (1.0, 0.0, true), (1.5, 0.0, false)]);
        // Return swings wildly; the win rate barely moves.
        l.points[0].ret_pct = -20.0;
        l.points[1].ret_pct = 31.0;
        l.points[2].ret_pct = -15.0;
        l.points[0].win_pct = Some(61.4);
        l.points[1].win_pct = Some(62.0);
        l.points[2].win_pct = Some(61.7);
        assert_eq!(l.verdict(), LadderVerdict::Flat);
    }

    #[test]
    fn regret_grades_an_alarm_by_its_two_counterfactuals() {
        let base = AlarmRegret {
            slot: 0,
            label: Some("stall >= 30".into()),
            standing: false,
            n: 40,
            n_priced: 36,
            realized_ret_pct: 8.0,
            best_later_ret_pct: 10.0,
            n_terminal: 40,
            realized_vs_terminal_pp: 60.0,
            band_pct: 4.0,
        };
        // Left 2pp against a 4pp round trip: nothing forfeitable was left.
        assert_eq!(base.verdict(), RegretVerdict::Timed);

        // Left 20pp, but holding to the end would have lost 60pp more: early, right.
        let early = AlarmRegret { best_later_ret_pct: 28.0, ..base.clone() };
        assert!(early.forfeit_pp() > early.band_pct);
        assert_eq!(early.verdict(), RegretVerdict::Protective);

        // Left 20pp AND holding on would have paid more: the alarm cuts winners.
        let premature = AlarmRegret {
            best_later_ret_pct: 28.0,
            realized_vs_terminal_pp: -5.0,
            ..base.clone()
        };
        assert_eq!(premature.verdict(), RegretVerdict::Premature);

        // No close had a print after it: no verdict, never a fabricated one.
        let unpriced = AlarmRegret { n_priced: 0, ..base };
        assert_eq!(unpriced.verdict(), RegretVerdict::Unmeasured);
    }

    #[test]
    fn the_regret_fold_pools_by_money_and_marks_standing_terms() {
        use crate::family_search::fixtures::{metric_exit, pricing, token_from_prices};
        use chrono::Duration;
        use hunter_engine::metrics::evaluator::Operator;

        // Entry at row 0 (price 1), alarm closes at row 1; the 6.0 peak is still
        // ahead and the token dies to 0.5.
        let t = token_from_prices(&[1.0, 2.0, 6.0, 1.0, 0.5]).with_oracle();
        let mut o = metric_exit(0, MetricId::Stall, Operator::Gte, 30.0, None, 0.008);
        o.entry_time = Some(t.trades[0].block_time);
        o.exit_time = Some(t.trades[0].block_time + Duration::seconds(1));
        // A second close on the standing term.
        let mut s = metric_exit(1, MetricId::Liquidity, Operator::Gte, 85.0, None, 0.001);
        s.entry_time = Some(t.trades[0].block_time);
        s.exit_time = Some(t.trades[0].block_time + Duration::seconds(1));

        let auth = Authority {
            outcomes: vec![o, s],
            token_idx: vec![0, 0],
            score: CohortScore::default(),
            n_tokens: 1,
        };
        let rows = alarm_regret(
            &[t],
            &auth,
            &pricing(),
            4.0,
            &[(MetricId::Liquidity, None, 85.0)],
        );
        assert_eq!(rows.len(), 2);
        let stall = &rows[0];
        assert_eq!(stall.label.as_deref(), Some("stall >= 30"));
        assert!(!stall.standing);
        assert_eq!((stall.n, stall.n_priced, stall.n_terminal), (1, 1, 1));
        // The 6.0 peak was still ahead of the close: a real forfeit...
        assert!(stall.forfeit_pp() > stall.band_pct, "{}", stall.forfeit_pp());
        // ...but the token dies, so firing still beat holding to the end.
        assert!(stall.realized_vs_terminal_pp > 0.0);
        assert_eq!(stall.verdict(), RegretVerdict::Protective);
        assert!(rows[1].standing, "the operator's own mechanic is labelled as one");
    }

    #[test]
    fn a_contribution_that_only_exists_at_one_fill_is_fill_dependent() {
        let cf = |a: Option<f64>, o: Option<f64>| ClauseFill {
            clause: "c".into(),
            is_entry: false,
            delta_authority: a,
            delta_optimistic: o,
        };
        // Holds its size and sign across pricings: signal.
        assert!(!cf(Some(6.0), Some(5.1)).fill_dependent());
        // Flips sign: the "contribution" is the fill model.
        assert!(cf(Some(3.0), Some(-2.0)).fill_dependent());
        // Keeps a fifth of itself: same.
        assert!(cf(Some(5.0), Some(0.9)).fill_dependent());
        // Tiny under both pricings: noise, not a finding.
        assert!(!cf(Some(0.3), Some(-0.2)).fill_dependent());
        // Unmeasurable never accuses.
        assert!(!cf(None, Some(4.0)).fill_dependent());
    }

    #[test]
    fn redundancy_needs_both_overlap_and_enough_vetoes() {
        let r = |n_vetoed: u64, overlap: Option<f64>| EntryRedundancy {
            clause: "liquidity > 30".into(),
            n_vetoed,
            solo_ret_pct: 10.0,
            solo_win_pct: Some(55.0),
            solo_n_closed: 80,
            max_overlap_pct: overlap,
            overlap_with: overlap.map(|_| "time >= 20".into()),
        };
        assert!(r(40, Some(95.0)).redundant());
        assert!(!r(40, Some(60.0)).redundant(), "most of its vetoes are its own");
        assert!(!r(3, Some(100.0)).redundant(), "three vetoes is noise, not structure");
        assert!(!r(40, None).redundant(), "a lone clause has nothing to be redundant with");
    }
}
