//! Signature-earned candidates + the per-family diversity quota (plan §3).
//!
//! Two rules govern what may enter a candidate set.
//!
//! **Earned, never nudged (charter D5).** Every threshold comes from this cohort's
//! own paths, through the signature machinery in
//! [`rule_search::cuts`](crate::rule_search::cuts) — read-only, never forked. No
//! `RuleParams` from the `rules` table, no threshold/metric/window/structure off one,
//! no buy size or cap read from one. Delete every row in `rules` and the output is
//! identical, because no code path here can see them: this module imports no rule
//! repo and takes no incumbent. That is not a claim about a search over the
//! neighbourhood of X — a search over the neighbourhood of X can only conclude X.
//!
//! **Diverse by quota (D5, fourth line).** Greedy expansion around a run's own top-3
//! amplifies whichever end-event family the initial library happened to favour, with
//! no incumbent anywhere in sight. So candidates bucket by end-event family and no
//! family may exceed [`FAMILY_QUOTA`] of the slots — applied at generation **and** at
//! every expansion stage. Three thresholds of one metric is one thesis, not three.

use std::collections::{BTreeMap, BTreeSet};

use hunter_engine::metrics::{group_of, MetricFamily, MetricId};

use crate::rule_search::cuts::CutTable;
use crate::rule_search::generator::{
    assemble, clause_label, generate as earned_combos, Clause, EntryFilling, ExitBag,
    GeneratedCombo,
};

/// The end-event families a diversity quota buckets on. An exit fires on one of a
/// few genuinely different kinds of alarm; two thresholds of the same kind are one
/// thesis, and a candidate set made of one kind is one bet dressed as many.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EndFamily {
    /// Windowed or lifetime SOL flow — the crowd leaving.
    Flow,
    /// The flow **split** (vol / nonvol) — organic activity as against bot volume.
    Organic,
    /// A clock: `stall` (token quiet) or `held` (our own fill).
    StallClock,
    /// `liquidity` — the pool draining out from under the position.
    LiquidityCeiling,
    /// Any price-path term: `trail`, `retrace`, `bounce`, `rise`, `pnl`.
    PriceTrail,
}

impl EndFamily {
    pub fn label(self) -> &'static str {
        match self {
            Self::Flow => "flow",
            Self::Organic => "organic",
            Self::StallClock => "stall-clock",
            Self::LiquidityCeiling => "liquidity-ceiling",
            Self::PriceTrail => "price-trail",
        }
    }
}

/// No end-event family may take more than this share of a candidate set's slots.
pub const FAMILY_QUOTA: f64 = 0.40;

/// Default candidate slots per cohort — the plan's budget of a family of 6 × ~40.
pub const DEFAULT_SLOTS: usize = 40;

/// Which end-event family an exit metric belongs to.
pub fn end_family(metric: MetricId) -> EndFamily {
    match metric {
        MetricId::Stall | MetricId::Held | MetricId::Time => EndFamily::StallClock,
        MetricId::Liquidity => EndFamily::LiquidityCeiling,
        MetricId::Trail
        | MetricId::WinTrail
        | MetricId::Retrace
        | MetricId::Bounce
        | MetricId::LifeRise
        | MetricId::WinRise
        | MetricId::Pnl => EndFamily::PriceTrail,
        _ => match group_of(metric).family {
            MetricFamily::FlowSplit => EndFamily::Organic,
            MetricFamily::Flow => EndFamily::Flow,
            // A registry row with no family of its own reads as a price-path term
            // rather than silently inflating one of the two flow buckets.
            _ => EndFamily::PriceTrail,
        },
    }
}

/// The refuted price-trail terms specifically — `trail` / `win_trail` against the
/// token's own ATH. Adding `trail >= 15 @10s` to a working exit took spend=5 from
/// +30.7% to −15.3% and spend=4 from +40.8% to −14.1%. It stays **in** the library,
/// flagged: a library that cannot express a refuted term cannot re-refute it on the
/// next family.
pub fn is_price_trail_term(metric: MetricId) -> bool {
    matches!(metric, MetricId::Trail | MetricId::WinTrail)
}

/// The flag a price-trail candidate carries onto the board.
pub const TRAIL_FLAG: &str =
    "price trail: refuted on the reference family (destroyed a working exit) — kept so \
     it can be re-refuted here, never promoted unflagged";

/// One generated candidate plus the two facts the quota and the board need.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub combo: GeneratedCombo,
    /// Every end-event family the exit bag draws on. An OR spanning two families is
    /// the shape that pays — the quota buckets on [`Self::primary_family`], not on
    /// this set, so a diverse OR is never penalised for being diverse.
    pub families: BTreeSet<EndFamily>,
    /// Non-empty ⇒ the board shows the flag beside the row.
    pub flags: Vec<&'static str>,
}

impl Candidate {
    fn new(combo: GeneratedCombo) -> Self {
        let families: BTreeSet<EndFamily> =
            combo.exit.clauses.iter().map(|c| end_family(c.metric)).collect();
        let flags = if combo.exit.clauses.iter().any(|c| is_price_trail_term(c.metric)) {
            vec![TRAIL_FLAG]
        } else {
            Vec::new()
        };
        Self { combo, families, flags }
    }

    /// The family this candidate is *counted against*: the first exit clause's, in
    /// menu order. `None` only for an exit-less diagnostic, which is never a
    /// candidate.
    pub fn primary_family(&self) -> Option<EndFamily> {
        self.combo.exit.clauses.first().map(|c| end_family(c.metric))
    }

    /// Stable text key — the deterministic tie-break, so two runs over one cut table
    /// produce byte-identical candidate sets.
    pub fn key(&self) -> String {
        let part = |cs: &[Clause]| {
            let mut v: Vec<String> = cs.iter().map(clause_label).collect();
            v.sort();
            v.join(" AND ")
        };
        format!("{} | {}", part(&self.combo.entry.clauses), part(&self.combo.exit.clauses))
    }
}

/// How many candidates to generate and how hard to hold the quota.
#[derive(Clone, Copy, Debug)]
pub struct GeneratorConfig {
    pub slots: usize,
    /// Per-family share cap. `1.0` disables the quota — a diagnostic setting, not a
    /// run setting.
    pub family_quota: f64,
}

impl Default for GeneratorConfig {
    fn default() -> Self {
        Self { slots: DEFAULT_SLOTS, family_quota: FAMILY_QUOTA }
    }
}

impl GeneratorConfig {
    /// Slots one family may occupy. At least 1, so a quota can never generate an
    /// empty set out of a non-empty library.
    pub fn cap(&self) -> usize {
        ((self.slots as f64 * self.family_quota).floor() as usize).max(1)
    }
}

/// What a quota pass kept, and what it refused. `dropped` is reported, never
/// silent — a truncation nobody sees reads as "the library had nothing else".
#[derive(Clone, Debug, PartialEq)]
pub struct QuotaOutcome<T> {
    pub kept: Vec<T>,
    /// Candidates a family cap turned away (as against ones that simply did not fit
    /// in the remaining slots).
    pub dropped_by_quota: usize,
    pub by_family: BTreeMap<EndFamily, usize>,
}

/// Fill `cfg.slots` from `ranked` (best first), letting no family exceed
/// [`GeneratorConfig::cap`]. Order within a family is preserved, so the quota
/// re-shapes the mix without re-ranking inside it.
pub fn apply_quota<T>(
    ranked: Vec<T>,
    cfg: &GeneratorConfig,
    family_of: impl Fn(&T) -> Option<EndFamily>,
) -> QuotaOutcome<T> {
    let cap = cfg.cap();
    let mut by_family: BTreeMap<EndFamily, usize> = BTreeMap::new();
    let mut kept = Vec::new();
    let mut dropped_by_quota = 0usize;
    for item in ranked {
        if kept.len() >= cfg.slots {
            break;
        }
        // A familyless item (an exit-less diagnostic) bypasses the quota rather
        // than being bucketed into a family it is not part of.
        if let Some(f) = family_of(&item) {
            let n = by_family.entry(f).or_insert(0);
            if *n >= cap {
                dropped_by_quota += 1;
                continue;
            }
            *n += 1;
        }
        kept.push(item);
    }
    QuotaOutcome { kept, dropped_by_quota, by_family }
}

/// Deterministic pre-score, best first. Exit is the **primary search axis**: the
/// promoted incumbents carry a trivial entry and a multi-term exit, and on the
/// reference family every exit term alone loses while their OR pays +31%. So rank on
/// the exit's signature strength, prefer an OR over a single alarm, and prefer a
/// simpler entry — entry conditions are wait-gates, and each one narrows the cohort.
fn rank_key(c: &Candidate) -> (u8, usize, usize, String) {
    let best_phase =
        c.combo.exit.clauses.iter().map(|x| x.phase.menu_rank()).min().unwrap_or(u8::MAX);
    (
        best_phase,
        // Descending on exit terms: 2 sorts before 1.
        usize::MAX - c.combo.exit.clauses.len(),
        c.combo.entry.clauses.len(),
        c.key(),
    )
}

/// Signature-earned candidates, ranked and cut to the per-family quota.
///
/// Exit-less combos are dropped: an empty exit is a diagnostic, not a draft. The
/// ungated control the board compares against is [`ungated_control`] — a property of
/// the cohort that exists before any rule does (D6), not a candidate.
pub fn generate(cuts: &CutTable, cfg: &GeneratorConfig) -> QuotaOutcome<Candidate> {
    let mut ranked: Vec<Candidate> = earned_combos(cuts)
        .into_iter()
        .filter(|c| !c.exit.clauses.is_empty())
        .map(Candidate::new)
        .collect();
    ranked.sort_by_key(rank_key);
    apply_quota(ranked, cfg, |c| c.primary_family())
}

/// The **ungated control**: what this fingerprint pays with no gate at all. Legitimate
/// as a comparison precisely because it is a property of the cohort — like the oracle,
/// and unlike an incumbent, it exists before any rule does (D6).
pub fn ungated_control() -> GeneratedCombo {
    let (entry, exit) = (EntryFilling { clauses: vec![] }, ExitBag { clauses: vec![] });
    GeneratedCombo { params: assemble(&entry, &exit), entry, exit }
}

/// Pick expansion bases **under the quota, not by raw rank** (D5, fourth line).
///
/// `ranked` holds candidate indices best-first by whatever the scoring stage measured.
/// Taking the top-N straight off that list is the leak: if the initial library favoured
/// flow, the top of every round is flow, and every expansion round widens that lead
/// until the run has searched one family and reported it as a search.
pub fn expansion_bases(
    ranked: &[usize],
    candidates: &[Candidate],
    n: usize,
) -> QuotaOutcome<usize> {
    let cfg = GeneratorConfig { slots: n, family_quota: FAMILY_QUOTA };
    let ordered: Vec<usize> = ranked.iter().copied().filter(|&i| i < candidates.len()).collect();
    apply_quota(ordered, &cfg, |&i| candidates[i].primary_family())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule_search::cuts::{Cut, CutPhase};
    use hunter_engine::metrics::evaluator::Operator;
    use hunter_engine::metrics::{group_of, MetricGroupId};

    fn cut(metric: MetricId, op: Operator, threshold: f64, window: Option<f64>) -> Cut {
        Cut {
            group: group_of(metric).id,
            metric,
            window,
            op,
            threshold,
            phase: CutPhase::DumpLead,
        }
    }

    fn entry_cut(metric: MetricId, op: Operator, threshold: f64) -> Cut {
        Cut { phase: CutPhase::Contrast, ..cut(metric, op, threshold, None) }
    }

    /// An earned menu spanning all five end-event families, as a cohort's signatures
    /// would supply it. No lake, no DB.
    fn table() -> CutTable {
        CutTable {
            windows: vec![2.0, 10.0],
            entry: vec![
                entry_cut(MetricId::Liquidity, Operator::Gt, 20.0),
                entry_cut(MetricId::Time, Operator::Gte, 5.0),
            ],
            exit: vec![
                cut(MetricId::GrossFlow, Operator::Lt, 15.0, Some(10.0)),
                cut(MetricId::GrossFlow, Operator::Lt, 25.0, Some(10.0)),
                cut(MetricId::Buy, Operator::Lt, 3.0, Some(10.0)),
                cut(MetricId::NonvolBuy, Operator::Gte, 1.6, None),
                cut(MetricId::WinNonvolBuy, Operator::Gte, 1.6, Some(2.0)),
                cut(MetricId::Stall, Operator::Gte, 30.0, None),
                cut(MetricId::Held, Operator::Gte, 120.0, None),
                cut(MetricId::Liquidity, Operator::Lt, 12.0, None),
                cut(MetricId::Trail, Operator::Gte, 15.0, Some(10.0)),
                cut(MetricId::Retrace, Operator::Gte, 25.0, None),
            ],
            winner_fill: Vec::new(),
            winner_lead: Vec::new(),
            winner_launch: Vec::new(),
        }
    }

    #[test]
    fn end_families_partition_the_exit_menu() {
        assert_eq!(end_family(MetricId::GrossFlow), EndFamily::Flow);
        assert_eq!(end_family(MetricId::Buy), EndFamily::Flow);
        assert_eq!(end_family(MetricId::NonvolBuy), EndFamily::Organic);
        assert_eq!(end_family(MetricId::WinNonvolBuy), EndFamily::Organic);
        assert_eq!(end_family(MetricId::Stall), EndFamily::StallClock);
        assert_eq!(end_family(MetricId::Held), EndFamily::StallClock);
        assert_eq!(end_family(MetricId::Liquidity), EndFamily::LiquidityCeiling);
        assert_eq!(end_family(MetricId::Trail), EndFamily::PriceTrail);
        assert_eq!(end_family(MetricId::Retrace), EndFamily::PriceTrail);
        // Group membership is what decides flow vs organic — a new registry row
        // joins by its family, never by being listed here.
        assert_eq!(group_of(MetricId::NonvolBuy).id, MetricGroupId::FlowSplit);
    }

    #[test]
    fn the_generated_set_spans_at_least_three_end_event_families() {
        let out = generate(&table(), &GeneratorConfig::default());
        assert!(!out.kept.is_empty());
        assert!(
            out.by_family.len() >= 3,
            "spans {} families: {:?}",
            out.by_family.len(),
            out.by_family
        );
        // No family exceeds the quota, and the cap actually bit — otherwise this
        // asserts nothing about the quota, only about the library.
        let cap = GeneratorConfig::default().cap();
        assert!(out.by_family.values().all(|&n| n <= cap), "{:?} cap {cap}", out.by_family);
        assert!(out.dropped_by_quota > 0, "the quota must be reported when it bites");
    }

    #[test]
    fn one_family_cannot_take_the_whole_set() {
        // A library of nothing but flow thresholds — three thresholds of one metric
        // is one thesis, so the quota must refuse to fill 40 slots with them.
        let mut cuts = table();
        cuts.exit.retain(|c| end_family(c.metric) == EndFamily::Flow);
        let cfg = GeneratorConfig { slots: 10, ..Default::default() };
        let out = generate(&cuts, &cfg);
        assert_eq!(out.by_family.keys().copied().collect::<Vec<_>>(), vec![EndFamily::Flow]);
        // 40% of 10 slots — the set stays at the cap, not at the slot budget, so a
        // one-family library cannot present itself as a full search.
        assert_eq!(cfg.cap(), 4);
        assert_eq!(out.kept.len(), 4, "kept {} > cap {}", out.kept.len(), cfg.cap());
        assert!(out.dropped_by_quota > 0, "what the cap turned away is reported, not silent");
    }

    #[test]
    fn a_price_trail_candidate_is_kept_and_flagged() {
        let out = generate(&table(), &GeneratorConfig { slots: 200, ..Default::default() });
        let trailing: Vec<&Candidate> = out
            .kept
            .iter()
            .filter(|c| c.combo.exit.clauses.iter().any(|x| is_price_trail_term(x.metric)))
            .collect();
        // In the library — a library that cannot express a refuted term cannot
        // re-refute it on the next family.
        assert!(!trailing.is_empty(), "price trail must stay available");
        // And never silently: every one of them carries the flag.
        assert!(trailing.iter().all(|c| c.flags.contains(&TRAIL_FLAG)));
        // `retrace` is a price term but NOT the refuted token-ATH trail, so it is
        // unflagged — the flag names one refuted shape, not a whole family.
        let retrace_only = out.kept.iter().find(|c| {
            c.combo.exit.clauses.iter().all(|x| x.metric == MetricId::Retrace)
                && !c.combo.exit.clauses.is_empty()
        });
        assert!(retrace_only.is_some_and(|c| c.flags.is_empty()));
    }

    #[test]
    fn expansion_bases_are_picked_under_the_quota_not_by_raw_rank() {
        // A scoring round whose top six are all flow — the exact shape that makes
        // greedy top-N expansion amplify one family round after round.
        let out = generate(&table(), &GeneratorConfig { slots: 200, ..Default::default() });
        let cands = &out.kept;
        let mut ranked: Vec<usize> = (0..cands.len()).collect();
        ranked.sort_by_key(|&i| {
            (cands[i].primary_family() != Some(EndFamily::Flow), i)
        });
        assert_eq!(
            cands[ranked[0]].primary_family(),
            Some(EndFamily::Flow),
            "the raw ranking leads with flow"
        );

        let bases = expansion_bases(&ranked, cands, 5);
        assert_eq!(bases.kept.len(), 5);
        let fams: BTreeSet<EndFamily> =
            bases.kept.iter().filter_map(|&i| cands[i].primary_family()).collect();
        assert!(fams.len() >= 2, "raw top-5 would be all flow; got {fams:?}");
        // 40% of 5 slots floors to 2 — no family may exceed it.
        assert!(bases.by_family.values().all(|&n| n <= 2), "{:?}", bases.by_family);
    }

    #[test]
    fn generation_is_reproducible_and_reads_no_rule() {
        // The whole input is the cut table (this cohort's own paths) plus the run's
        // slot budget. There is no incumbent parameter to pass, so the "empty the
        // `rules` table" test is carried by the signature: nothing to empty.
        let cuts = table();
        let cfg = GeneratorConfig::default();
        let a = generate(&cuts, &cfg);
        let b = generate(&cuts, &cfg);
        assert_eq!(a.kept.len(), b.kept.len());
        assert!(a.kept.iter().zip(&b.kept).all(|(x, y)| x.key() == y.key()));
        assert_eq!(a.by_family, b.by_family);
    }

    #[test]
    fn an_exit_less_combo_is_a_diagnostic_not_a_candidate() {
        let out = generate(&table(), &GeneratorConfig { slots: 500, ..Default::default() });
        assert!(out.kept.iter().all(|c| !c.combo.exit.clauses.is_empty()));
        // The ungated control still exists — it is a property of the cohort, and
        // unlike an incumbent it is there before any rule is.
        let control = ungated_control();
        assert!(control.entry.clauses.is_empty() && control.exit.clauses.is_empty());
        assert!(control.params.entry.is_none() && control.params.exit.is_none());
    }
}
