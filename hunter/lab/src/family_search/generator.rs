//! Signature-earned candidates: the **g4 shape**, composed rather than enumerated.
//!
//! Three rules govern what may enter a candidate set.
//!
//! **Earned, never nudged (charter D5).** Every threshold comes from this cohort's
//! own paths, through the signature machinery in
//! [`rule_search::cuts`](crate::rule_search::cuts) — read-only, never forked. No
//! `RuleParams` from the `rules` table, no threshold/metric/window/structure off one,
//! no buy size or cap read from one. Delete every row in `rules` and the output is
//! identical, because no code path here can see them: this module imports no rule
//! repo and takes no incumbent.
//!
//! **Composed to the working shape (charter D9).** A promoted rule that works in
//! practice carries **3–4 entry quantities** and an OR of **3–5 exit alarms of
//! different kinds**. Two facts make that the shape to generate rather than a shape
//! to hope for:
//!
//! * A **band** (floor + ceiling on one quantity) is ONE idea written as two clauses.
//!   Counting clauses instead of quantities makes a 3-idea entry look like a 5-metric
//!   one and makes "prefer fewer clauses" delete bands first — so this module counts
//!   [`EntryQuantity`], and prefers **more** of them.
//! * An exit OR pays because it fires at the earliest of several **independent**
//!   alarms: on the reference family the OR pays +31% while every term alone is worse.
//!   Two thresholds of one quantity are not two alarms, so a bag takes **at most one
//!   clause per [`EndFamily`]** and spans 2–5 of them.
//!
//! **Diverse by quota (D5, fourth line).** Greedy expansion around a run's own top-3
//! amplifies whichever end-event family the initial library happened to favour, with
//! no incumbent anywhere in sight. So candidates bucket by end-event family and no
//! family may exceed [`FAMILY_QUOTA`] of the slots — applied at generation **and** at
//! every expansion stage.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use hunter_engine::event::parse_metric_exit_label;
use hunter_engine::metrics::evaluator::Operator;
use hunter_engine::metrics::{group_of, MetricFamily, MetricId};

use crate::rule_search::cuts::{Cut, CutPhase, CutTable};
use crate::rule_search::generator::{
    assemble, clause_label, is_band_pair, Clause, EntryFilling, ExitBag, GeneratedCombo,
};
use crate::rule_search::roles::{entry_compete, entry_role, CompeteKey, EntryRole};

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

/// Default candidate slots per cohort.
pub const DEFAULT_SLOTS: usize = 40;

/// Most **quantities** one entry AND may carry. Four ideas (a time band, a state
/// band, an activity floor, one start event) is the working shape; a fifth turns a
/// filter into a fit.
pub const MAX_ENTRY_QUANTITIES: usize = 4;

/// Quantities the composer draws those subsets from, best signature first.
const ENTRY_POOL: usize = 6;

/// Entry fillings kept after ranking. Three are reserved for sparser shapes so the
/// archive always carries the contrast that proves density earned its place.
const MAX_ENTRY_FILLINGS: usize = 32;

/// Fewest alarms an exit OR may carry. One alarm is not an OR, and the edge is
/// getting out on the **earliest** of several independent signals.
pub const MIN_ALARMS: usize = 2;

/// Most alarms an exit OR may carry — one per [`EndFamily`], so this is also the
/// number of families that exist.
pub const MAX_ALARMS: usize = 5;

/// Thresholds kept per end-event family, so one family contributes threshold variety
/// without contributing two alarms.
const EXIT_REPS_PER_FAMILY: usize = 2;

/// Exit bags kept after ranking.
const MAX_EXIT_BAGS: usize = 64;

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
            MetricFamily::FlowIx => EndFamily::Organic,
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

// ─────────────────────────── standing terms (charter D10) ─────────────────────
//
// `liquidity >= 85` on a promoted rule is not a discovered edge — it is "sell at
// migration", added by hand because the pool is about to change underneath the
// position. It has to be present in every simulation or the numbers describe a rule
// nobody would run, and it must never be searched, credited, dropped by the ablation,
// or counted toward the diversity quota. Mechanics are not findings.

/// A mechanical exit the operator always wants. Rides into every candidate, the
/// ungated control included; never generated, never ablated, never credited.
#[derive(Clone, Debug, PartialEq)]
pub struct StandingTerm {
    pub clause: Clause,
    /// The label as the attribution table prints it — the same text the operator
    /// typed, round-tripped through the one label SSOT.
    pub label: String,
}

/// Parse `metric[(Ws)] op value` (`liquidity >= 85`, `untagged_buy(2s) >= 0.9`) through
/// [`parse_metric_exit_label`] — the same parser the persisted exit reason uses, so a
/// standing term is written exactly as the board prints one.
pub fn parse_standing(s: &str) -> anyhow::Result<StandingTerm> {
    let (metric, op, value, window) = parse_metric_exit_label(s)
        .ok_or_else(|| anyhow::anyhow!("standing exit term `{s}` is not `metric[(Ws)] op value`"))?;
    Ok(StandingTerm {
        clause: Clause {
            group: group_of(metric).id,
            metric,
            window,
            op,
            threshold: value,
            // Declared by the operator, not earned from a path — the phase says so.
            phase: CutPhase::Declared,
        },
        label: hunter_engine::event::format_metric_exit_label(metric, op, value, window),
    })
}

/// Parse a request's standing list, rejecting the whole run on the first bad term:
/// silently dropping one would run a rule the operator did not ask for.
pub fn parse_standing_all(terms: &[String]) -> anyhow::Result<Vec<StandingTerm>> {
    terms.iter().map(|s| parse_standing(s)).collect()
}

/// Does this alarm match a standing term? Keyed on `(metric, window, threshold)` —
/// not on slot order, because `SideConditions` groups clauses by group and window and
/// the compiled req order need not follow the authored one.
pub fn is_standing(
    standing: &[StandingTerm],
    metric: MetricId,
    window: Option<hunter_engine::metrics::WindowSpec>,
    value: f64,
) -> bool {
    standing.iter().any(|s| {
        s.clause.metric == metric
            && s.clause.window == window
            && (s.clause.threshold - value).abs() < f64::EPSILON
    })
}

// ─────────────────────────── entry: quantities, not clauses ───────────────────

/// One entry **idea**. A band (floor + ceiling on one quantity) is one idea written
/// as two clauses — the reason g4's "5 entry metrics" are really 3.
#[derive(Clone, Debug, PartialEq)]
pub struct EntryQuantity {
    /// One clause (a floor or a ceiling) or two (a band).
    pub clauses: Vec<Clause>,
    pub metric: MetricId,
    pub window: Option<hunter_engine::metrics::WindowSpec>,
    /// Menu rank of the strongest signature behind it — lower is stronger.
    pub rank: u8,
    /// At most one quantity per compete slot (one trigger family, one giveback…).
    pub compete: Option<CompeteKey>,
}

impl EntryQuantity {
    /// Two clauses on one quantity: the shape a clause count mistakes for two ideas.
    pub fn is_band(&self) -> bool {
        self.clauses.len() > 1
    }

    fn label(&self) -> String {
        self.clauses.iter().map(clause_label).collect::<Vec<_>>().join(" AND ")
    }
}

/// The entry ideas a cohort's signatures earn, strongest first, prefering a band over
/// a bare floor wherever the winners' distribution has two sides.
pub fn entry_quantities(cuts: &CutTable) -> Vec<EntryQuantity> {
    let ceils: Vec<Clause> = cuts
        .entry
        .iter()
        .filter(|c| c.phase == CutPhase::WinnerCeil)
        .map(|c| Clause::from(*c))
        .collect();

    // Best (lowest menu rank) cut per quantity. `Time`/`Liquidity` selectors and
    // windowed flow floors both land here; wait-only monotones never do.
    let mut best: BTreeMap<(String, (i64, i64, i64)), Cut> = BTreeMap::new();
    for c in &cuts.entry {
        if c.phase == CutPhase::WinnerCeil {
            continue;
        }
        match entry_role(c.metric) {
            Some(EntryRole::Selector | EntryRole::Extra | EntryRole::Trigger(_)) => {}
            _ => continue,
        }
        let key = (format!("{:?}", c.metric), window_key(c.window));
        best.entry(key)
            .and_modify(|prev| {
                if c.phase.menu_rank() < prev.phase.menu_rank() {
                    *prev = *c;
                }
            })
            .or_insert(*c);
    }

    let mut out: Vec<EntryQuantity> = best
        .values()
        .map(|c| {
            let floor = Clause::from(*c);
            // A band only forms from a floor that HAS a ceiling on the same quantity;
            // pairing is the shared `is_band_pair` rule, never a local guess.
            let clauses = match ceils.iter().find(|ce| is_band_pair(&floor, ce)) {
                Some(ceil) => vec![floor, *ceil],
                None => vec![floor],
            };
            EntryQuantity {
                clauses,
                metric: c.metric,
                window: c.window,
                rank: c.phase.menu_rank(),
                compete: entry_compete(c.metric),
            }
        })
        .collect();

    out.sort_by(|a, b| {
        a.rank
            .cmp(&b.rank)
            // A band before a bare floor at equal rank: it is the same idea, bounded.
            .then_with(|| b.is_band().cmp(&a.is_band()))
            .then_with(|| a.label().cmp(&b.label()))
    });
    out.truncate(ENTRY_POOL);
    out
}

/// Every legal AND of 0..=[`MAX_ENTRY_QUANTITIES`] ideas, **densest first**.
///
/// Sparse fillings are not dropped — three slots are reserved for the best 2-idea,
/// 1-idea and empty entries, so the archive always shows what the extra ideas bought
/// and [`narrow_recheck`](crate::family_search::narrow_recheck) is not the only place
/// density has to justify itself.
pub fn entry_fillings(qs: &[EntryQuantity]) -> Vec<EntryFilling> {
    let n = qs.len().min(ENTRY_POOL);
    let mut by_size: BTreeMap<usize, Vec<(u32, EntryFilling)>> = BTreeMap::new();
    for mask in 0u32..(1u32 << n) {
        let picked: Vec<&EntryQuantity> =
            (0..n).filter(|i| mask & (1 << i) != 0).map(|i| &qs[i]).collect();
        if picked.len() > MAX_ENTRY_QUANTITIES {
            continue;
        }
        // One quantity per compete slot: two trigger families in one AND is not a
        // denser entry, it is a rule that waits for two different things at once.
        let mut seen: HashSet<CompeteKey> = HashSet::new();
        if picked.iter().any(|q| q.compete.is_some_and(|k| !seen.insert(k))) {
            continue;
        }
        let rank_sum: u32 = picked.iter().map(|q| q.rank as u32).sum();
        let clauses: Vec<Clause> = picked.iter().flat_map(|q| q.clauses.clone()).collect();
        by_size.entry(picked.len()).or_default().push((rank_sum, EntryFilling { clauses }));
    }
    for v in by_size.values_mut() {
        v.sort_by(|a, b| {
            a.0.cmp(&b.0).then_with(|| filling_key(&a.1).cmp(&filling_key(&b.1)))
        });
    }

    let mut out: Vec<EntryFilling> = Vec::new();
    let push = |f: &EntryFilling, out: &mut Vec<EntryFilling>| {
        if !out.iter().any(|e| filling_key(e) == filling_key(f)) {
            out.push(f.clone());
        }
    };
    // Densest first, down to the cap...
    for size in (0..=MAX_ENTRY_QUANTITIES).rev() {
        for (_, f) in by_size.get(&size).into_iter().flatten() {
            if out.len() >= MAX_ENTRY_FILLINGS.saturating_sub(3) {
                break;
            }
            push(f, &mut out);
        }
    }
    // ...then the reserved sparse comparators, so the board can always show the cost
    // of every extra idea rather than only the finalist's ablation.
    for size in [2usize, 1, 0] {
        if let Some((_, f)) = by_size.get(&size).and_then(|v| v.first()) {
            push(f, &mut out);
        }
    }
    if out.is_empty() {
        out.push(EntryFilling { clauses: Vec::new() });
    }
    out
}

// ─────────────────────────── exit: alarms of different kinds ──────────────────

/// Up to [`EXIT_REPS_PER_FAMILY`] thresholds per end-event family, strongest first.
/// `skip` drops families a standing term already occupies — searching for a second
/// liquidity ceiling beside a hand-authored one is searching for a duplicate.
pub fn exit_alarms(
    cuts: &CutTable,
    skip: &BTreeSet<EndFamily>,
) -> BTreeMap<EndFamily, Vec<Clause>> {
    let mut ranked: Vec<Cut> = cuts.exit.clone();
    ranked.sort_by(|a, b| {
        a.phase
            .menu_rank()
            .cmp(&b.phase.menu_rank())
            .then_with(|| format!("{:?}", a.metric).cmp(&format!("{:?}", b.metric)))
            .then_with(|| a.threshold.partial_cmp(&b.threshold).unwrap_or(std::cmp::Ordering::Equal))
    });

    let mut out: BTreeMap<EndFamily, Vec<Clause>> = BTreeMap::new();
    for c in &ranked {
        let fam = end_family(c.metric);
        if skip.contains(&fam) {
            continue;
        }
        let bucket = out.entry(fam).or_default();
        if bucket.len() >= EXIT_REPS_PER_FAMILY {
            continue;
        }
        // One threshold per (metric, window): a second is the same alarm re-tuned,
        // and threshold variety is what the per-family reps budget is for.
        if bucket.iter().any(|s| s.metric == c.metric && window_key(s.window) == window_key(c.window))
        {
            continue;
        }
        bucket.push(Clause::from(*c));
    }
    out
}

/// Every OR of [`MIN_ALARMS`]..=[`MAX_ALARMS`] alarms drawn from **distinct**
/// families, widest first. This is the shape the reference family's +31% lives in and
/// the shape a two-term cap cannot express.
pub fn exit_bags(by_family: &BTreeMap<EndFamily, Vec<Clause>>) -> Vec<ExitBag> {
    let fams: Vec<(&EndFamily, &Vec<Clause>)> =
        by_family.iter().filter(|(_, v)| !v.is_empty()).collect();
    let n = fams.len();
    if n == 0 {
        return Vec::new();
    }
    // A cohort earning one family can still be searched — with one alarm, reported
    // as the one alarm it is, rather than refused for a shape it cannot reach.
    let lo = MIN_ALARMS.min(n);
    let hi = MAX_ALARMS.min(n);

    let mut scored: Vec<(usize, u32, String, ExitBag)> = Vec::new();
    for mask in 0u32..(1u32 << n) {
        let picked: Vec<usize> = (0..n).filter(|i| mask & (1 << i) != 0).collect();
        if picked.len() < lo || picked.len() > hi {
            continue;
        }
        // Cartesian product of each chosen family's thresholds.
        let mut bags: Vec<Vec<Clause>> = vec![Vec::new()];
        for &i in &picked {
            let reps = fams[i].1;
            bags = bags
                .into_iter()
                .flat_map(|base| {
                    reps.iter().map(move |c| {
                        let mut b = base.clone();
                        b.push(*c);
                        b
                    })
                })
                .collect();
        }
        for clauses in bags {
            let rank_sum: u32 = clauses.iter().map(|c| c.phase.menu_rank() as u32).sum();
            let bag = ExitBag { clauses };
            scored.push((picked.len(), rank_sum, bag_key(&bag), bag));
        }
    }
    // Widest first, then strongest signature — the ordering the quota then thins.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)).then_with(|| a.2.cmp(&b.2)));
    scored.truncate(MAX_EXIT_BAGS);
    scored.into_iter().map(|(_, _, _, bag)| bag).collect()
}

// ─────────────────────────── candidates ───────────────────────────────────────

/// One generated candidate plus the facts the quota, the ablation and the board need.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub combo: GeneratedCombo,
    /// End-event families the **searched** alarms draw on. Standing terms are
    /// excluded: a mechanical exit is not a discovered kind of alarm.
    pub families: BTreeSet<EndFamily>,
    /// Non-empty ⇒ the board shows the flag beside the row.
    pub flags: Vec<&'static str>,
    /// Entry **ideas**, not clauses. A band counts once — the number that makes g4's
    /// "5 entry metrics" read as the 3 it is.
    pub n_entry_quantities: usize,
    /// Trailing clauses of `combo.exit` that are standing terms, never searched.
    pub n_standing: usize,
}

impl Candidate {
    fn new(
        entry: &EntryFilling,
        n_entry_quantities: usize,
        exit: &ExitBag,
        standing: &[StandingTerm],
    ) -> Self {
        // Standing clauses ride at the END of the bag so a slice separates the two
        // populations everywhere downstream.
        let mut clauses = exit.clauses.clone();
        clauses.extend(standing.iter().map(|s| s.clause));
        let full = ExitBag { clauses };
        let families: BTreeSet<EndFamily> =
            exit.clauses.iter().map(|c| end_family(c.metric)).collect();
        let flags = if exit.clauses.iter().any(|c| is_price_trail_term(c.metric)) {
            vec![TRAIL_FLAG]
        } else {
            Vec::new()
        };
        Self {
            combo: GeneratedCombo {
                params: assemble(entry, &full),
                entry: entry.clone(),
                exit: full,
            },
            families,
            flags,
            n_entry_quantities,
            n_standing: standing.len(),
        }
    }

    /// The searched alarms — the exit bag without its standing tail.
    pub fn searched_exit(&self) -> &[Clause] {
        let n = self.combo.exit.clauses.len().saturating_sub(self.n_standing);
        &self.combo.exit.clauses[..n]
    }

    /// The **shape** the quota counts against: every family this candidate's searched
    /// alarms draw on, sorted.
    ///
    /// Bucketing on the *first* alarm's family is what a single-alarm library could
    /// get away with; a bag that spans three families has no first family in any
    /// meaningful sense, and since the composer emits families in a fixed order, "the
    /// first one" would put almost every candidate in the same bucket and the quota
    /// would stop doing anything at all.
    pub fn family_shape(&self) -> Vec<EndFamily> {
        self.families.iter().copied().collect()
    }

    /// Stable text key — the deterministic tie-break, so two runs over one cut table
    /// produce byte-identical candidate sets. Standing terms are constant across the
    /// whole set and stay out of it.
    pub fn key(&self) -> String {
        let part = |cs: &[Clause]| {
            let mut v: Vec<String> = cs.iter().map(clause_label).collect();
            v.sort();
            v.join(" AND ")
        };
        format!("{} | {}", part(&self.combo.entry.clauses), part(self.searched_exit()))
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
    /// Candidates a shape cap turned away (as against ones that simply did not fit
    /// in the remaining slots).
    pub dropped_by_quota: usize,
    /// **Coverage**: how many kept candidates draw on each family. A bag spanning
    /// three families counts in all three, because the question a board asks is "was
    /// this kind of alarm searched at all", not "which bucket did the cap use".
    pub by_family: BTreeMap<EndFamily, usize>,
}

/// Fill `cfg.slots` from `ranked` (best first), letting no exit **shape** exceed
/// [`GeneratorConfig::cap`]. Order within a shape is preserved, so the quota
/// re-shapes the mix without re-ranking inside it.
///
/// `shape_of` returns the sorted families a candidate's alarms draw on; an empty
/// shape bypasses the cap rather than being bucketed into a family it is not part of.
pub fn apply_quota<T>(
    ranked: Vec<T>,
    cfg: &GeneratorConfig,
    shape_of: impl Fn(&T) -> Vec<EndFamily>,
) -> QuotaOutcome<T> {
    let cap = cfg.cap();
    let mut taken: BTreeMap<Vec<EndFamily>, usize> = BTreeMap::new();
    let mut by_family: BTreeMap<EndFamily, usize> = BTreeMap::new();
    let mut kept = Vec::new();
    let mut dropped_by_quota = 0usize;
    for item in ranked {
        if kept.len() >= cfg.slots {
            break;
        }
        let shape = shape_of(&item);
        if !shape.is_empty() {
            let n = taken.entry(shape.clone()).or_insert(0);
            if *n >= cap {
                dropped_by_quota += 1;
                continue;
            }
            *n += 1;
            for f in shape {
                *by_family.entry(f).or_insert(0) += 1;
            }
        }
        kept.push(item);
    }
    QuotaOutcome { kept, dropped_by_quota, by_family }
}

/// Deterministic pre-score, best first — **denser and wider first**.
///
/// The old ordering preferred a *simpler* entry, which with a bounded slot budget cut
/// every multi-idea entry before scoring ever saw it: "most results have no entry" was
/// the sort order talking, not the data. Exit width leads because the exit is the
/// primary search axis; entry density is then maximised within it.
fn rank_key(c: &Candidate) -> (usize, usize, u8, String) {
    let best_phase =
        c.searched_exit().iter().map(|x| x.phase.menu_rank()).min().unwrap_or(u8::MAX);
    (
        usize::MAX - c.families.len(),
        usize::MAX - c.n_entry_quantities,
        best_phase,
        c.key(),
    )
}

/// Signature-earned candidates, composed to the working shape, ranked and cut to the
/// per-family quota.
///
/// Exit-less combos are dropped: an empty exit is a diagnostic, not a draft. The
/// ungated control the board compares against is [`ungated_control`] — a property of
/// the cohort that exists before any rule does (D6), not a candidate.
pub fn generate(
    cuts: &CutTable,
    cfg: &GeneratorConfig,
    standing: &[StandingTerm],
) -> QuotaOutcome<Candidate> {
    let taken: BTreeSet<EndFamily> =
        standing.iter().map(|s| end_family(s.clause.metric)).collect();
    let quantities = entry_quantities(cuts);
    let fillings = entry_fillings(&quantities);
    let bags = exit_bags(&exit_alarms(cuts, &taken));

    let mut ranked: Vec<Candidate> = Vec::with_capacity(fillings.len() * bags.len());
    for f in &fillings {
        let n_q = count_quantities(f, &quantities);
        for bag in &bags {
            ranked.push(Candidate::new(f, n_q, bag, standing));
        }
    }
    ranked.sort_by_key(rank_key);
    apply_quota(ranked, cfg, |c| c.family_shape())
}

/// The **ungated control**: what this fingerprint pays with no gate at all, under the
/// same standing terms every candidate carries. Legitimate as a comparison precisely
/// because it is a property of the cohort — like the oracle, and unlike an incumbent,
/// it exists before any rule does (D6).
pub fn ungated_control(standing: &[StandingTerm]) -> GeneratedCombo {
    let entry = EntryFilling { clauses: vec![] };
    let exit = ExitBag { clauses: standing.iter().map(|s| s.clause).collect() };
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
    apply_quota(ordered, &cfg, |&i| candidates[i].family_shape())
}

// ── helpers ─────────────────────────────────────────────────────────────────

/// `Option<f64>` is not `Ord`; windows are compared as milli-second integers so a map
/// key is stable and `2.0` never sorts apart from itself.
fn window_key(w: Option<hunter_engine::metrics::WindowSpec>) -> (i64, i64, i64) {
    match w {
        // The unit's own discriminant, not a table restated here: a new basis must
        // key apart from every existing one without this site being remembered into.
        Some(w) => (
            w.unit as i64,
            hunter_engine::metrics::quantize(w.size) as i64,
            hunter_engine::metrics::quantize(w.lag) as i64,
        ),
        None => (-1, -1, -1),
    }
}

fn filling_key(f: &EntryFilling) -> String {
    let mut v: Vec<String> = f.clauses.iter().map(clause_label).collect();
    v.sort();
    v.join(" AND ")
}

fn bag_key(b: &ExitBag) -> String {
    let mut v: Vec<String> = b.clauses.iter().map(clause_label).collect();
    v.sort();
    v.join(" OR ")
}

/// How many *ideas* a filling carries — a band counts once.
fn count_quantities(f: &EntryFilling, qs: &[EntryQuantity]) -> usize {
    qs.iter()
        .filter(|q| {
            q.clauses
                .iter()
                .all(|qc| f.clauses.iter().any(|fc| clause_label(fc) == clause_label(qc)))
        })
        .count()
}

/// Every earned clause a finalist does **not** already use — the enrich stage's menu
/// (plan §5d). Standing terms are excluded: they are already in every rule.
pub fn unused_clauses(
    cuts: &CutTable,
    used_entry: &[Clause],
    used_exit: &[Clause],
    standing: &[StandingTerm],
) -> (Vec<Clause>, Vec<Clause>) {
    let taken = |cs: &[Clause], c: &Clause| {
        cs.iter().any(|u| {
            u.metric == c.metric && window_key(u.window) == window_key(c.window) && u.op == c.op
        })
    };
    let is_standing_clause = |c: &Clause| {
        standing.iter().any(|s| {
            s.clause.metric == c.metric && window_key(s.clause.window) == window_key(c.window)
        })
    };

    let mut entry: Vec<Clause> = entry_quantities(cuts)
        .into_iter()
        // One extra IDEA at a time — a band arrives whole or not at all.
        .filter(|q| !q.clauses.iter().any(|c| taken(used_entry, c)))
        .flat_map(|q| q.clauses)
        .collect();
    entry.dedup_by_key(|c| clause_label(c));

    // An extra alarm has to be a **new kind**, or it is the same bet re-tuned.
    let used_families: BTreeSet<EndFamily> =
        used_exit.iter().map(|c| end_family(c.metric)).collect();
    let exit: Vec<Clause> = exit_alarms(cuts, &used_families)
        .into_values()
        .flatten()
        .filter(|c| !taken(used_exit, c) && !is_standing_clause(c))
        .collect();

    (entry, exit)
}

/// Operator text for a clause, for the enrich table's row key.
pub fn label_of(c: &Clause) -> String {
    clause_label(c)
}

/// Rebuild a candidate's params after the enrich stage changed a side.
pub fn reassemble(entry: &EntryFilling, exit: &ExitBag) -> hunter_engine::rule_params::RuleParams {
    assemble(entry, exit)
}

/// The operator text of a standing term, so `Operator` stays private to this module.
pub fn standing_label(s: &StandingTerm) -> &str {
    &s.label
}

#[allow(unused)]
fn _op_is_used(_: Operator) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule_search::cuts::{Cut, CutPhase};
    use hunter_engine::metrics::evaluator::Operator;
    use hunter_engine::metrics::{group_of, MetricGroupId};

    fn cut(metric: MetricId, op: Operator, threshold: f64, window: Option<hunter_engine::metrics::WindowSpec>) -> Cut {
        Cut {
            group: group_of(metric).id,
            metric,
            window,
            op,
            threshold,
            phase: CutPhase::DumpLead,
        }
    }

    fn entry_cut(metric: MetricId, op: Operator, threshold: f64, phase: CutPhase) -> Cut {
        Cut { phase, ..cut(metric, op, threshold, None) }
    }

    /// An earned menu spanning all five end-event families plus two entry BANDS —
    /// the g4 shape's raw material. No lake, no DB.
    fn table() -> CutTable {
        CutTable {
            windows: vec![2.0, 10.0],
            entry: vec![
                // A time band: floor + ceiling on one quantity = ONE idea.
                entry_cut(MetricId::Time, Operator::Gte, 20.0, CutPhase::WinnerFloor),
                entry_cut(MetricId::Time, Operator::Lte, 90.0, CutPhase::WinnerCeil),
                // A liquidity band.
                entry_cut(MetricId::Liquidity, Operator::Gt, 30.0, CutPhase::WinnerFloor),
                entry_cut(MetricId::Liquidity, Operator::Lt, 60.0, CutPhase::WinnerCeil),
                // An activity floor, no ceiling — one clause, one idea.
                Cut {
                    phase: CutPhase::Contrast,
                    ..cut(MetricId::GrossFlow, Operator::Gte, 55.0, Some(hunter_engine::metrics::WindowSpec::secs(60.0)))
                },
            ],
            exit: vec![
                cut(MetricId::GrossFlow, Operator::Lt, 15.0, Some(hunter_engine::metrics::WindowSpec::secs(10.0))),
                cut(MetricId::GrossFlow, Operator::Lt, 25.0, Some(hunter_engine::metrics::WindowSpec::secs(10.0))),
                cut(MetricId::Buy, Operator::Lt, 3.0, Some(hunter_engine::metrics::WindowSpec::secs(10.0))),
                cut(MetricId::UntaggedBuy, Operator::Gte, 1.6, None),
                cut(MetricId::WinUntaggedBuy, Operator::Gte, 1.6, Some(hunter_engine::metrics::WindowSpec::secs(2.0))),
                cut(MetricId::Stall, Operator::Gte, 30.0, None),
                cut(MetricId::Held, Operator::Gte, 120.0, None),
                cut(MetricId::Liquidity, Operator::Lt, 12.0, None),
                cut(MetricId::Trail, Operator::Gte, 15.0, Some(hunter_engine::metrics::WindowSpec::secs(10.0))),
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
        assert_eq!(end_family(MetricId::UntaggedBuy), EndFamily::Organic);
        assert_eq!(end_family(MetricId::WinUntaggedBuy), EndFamily::Organic);
        assert_eq!(end_family(MetricId::Stall), EndFamily::StallClock);
        assert_eq!(end_family(MetricId::Held), EndFamily::StallClock);
        assert_eq!(end_family(MetricId::Liquidity), EndFamily::LiquidityCeiling);
        assert_eq!(end_family(MetricId::Trail), EndFamily::PriceTrail);
        assert_eq!(end_family(MetricId::Retrace), EndFamily::PriceTrail);
        // Group membership is what decides flow vs organic — a new registry row
        // joins by its family, never by being listed here.
        assert_eq!(group_of(MetricId::UntaggedBuy).id, MetricGroupId::FlowIx);
    }

    /// The whole "5 entry metrics are really 3" correction, on one assertion.
    #[test]
    fn a_band_is_one_entry_idea_written_as_two_clauses() {
        let qs = entry_quantities(&table());
        // Three ideas from five cuts: time band, liquidity band, activity floor.
        assert_eq!(qs.len(), 3, "{:?}", qs.iter().map(|q| q.metric).collect::<Vec<_>>());
        let time = qs.iter().find(|q| q.metric == MetricId::Time).expect("time band");
        assert!(time.is_band() && time.clauses.len() == 2);
        let flow = qs.iter().find(|q| q.metric == MetricId::GrossFlow).expect("activity floor");
        assert!(!flow.is_band() && flow.clauses.len() == 1);

        // And the densest filling carries all three ideas — five clauses.
        let fills = entry_fillings(&qs);
        let densest = fills.first().expect("a filling");
        assert_eq!(count_quantities(densest, &qs), 3);
        assert_eq!(densest.clauses.len(), 5, "3 ideas = 5 clauses, exactly g4's shape");
    }

    /// The old generator capped the OR at two terms, so the four-alarm shape whose OR
    /// pays +31% could not be produced at all.
    #[test]
    fn an_exit_or_spans_three_to_five_alarms_of_different_kinds() {
        let bags = exit_bags(&exit_alarms(&table(), &BTreeSet::new()));
        assert!(!bags.is_empty());

        let widest = bags.first().expect("a bag");
        assert!(widest.clauses.len() >= 3, "widest bag has {} alarms", widest.clauses.len());
        assert!(bags.iter().any(|b| b.clauses.len() >= 4), "a 4-alarm OR must be reachable");

        // Every bag draws at most one alarm per family: two flow thresholds are one
        // bet, not two alarms.
        for b in &bags {
            let fams: BTreeSet<EndFamily> =
                b.clauses.iter().map(|c| end_family(c.metric)).collect();
            assert_eq!(fams.len(), b.clauses.len(), "duplicate family in {:?}", bag_key(b));
        }
        // Threshold variety still exists — two flow bags at different levels.
        let flow_levels: BTreeSet<String> = bags
            .iter()
            .flat_map(|b| b.clauses.iter())
            .filter(|c| end_family(c.metric) == EndFamily::Flow)
            .map(|c| format!("{}{}", c.metric.name(), c.threshold))
            .collect();
        assert!(flow_levels.len() >= 2, "{flow_levels:?}");
    }

    /// The sort key that produced "no entry + 1-2 exit metrics" every run.
    #[test]
    fn generation_prefers_dense_entries_and_wide_exits() {
        let out = generate(&table(), &GeneratorConfig::default(), &[]);
        assert!(!out.kept.is_empty());
        let top = &out.kept[0];
        assert!(top.n_entry_quantities >= 2, "top candidate has {} entry ideas", top.n_entry_quantities);
        assert!(top.families.len() >= 3, "top candidate has {} alarm kinds", top.families.len());

        // No candidate is empty-entry-and-one-alarm, the shape the old rank produced.
        assert!(out.kept.iter().all(|c| !c.searched_exit().is_empty()));
        let sparse = out
            .kept
            .iter()
            .filter(|c| c.n_entry_quantities == 0 && c.searched_exit().len() < 2)
            .count();
        assert_eq!(sparse, 0, "an empty entry with a single alarm is never a candidate");
    }

    #[test]
    fn the_generated_set_spans_at_least_three_end_event_families() {
        let cfg = GeneratorConfig::default();
        let out = generate(&table(), &cfg, &[]);
        // Coverage: every kind of alarm the cohort earned is actually searched.
        assert!(out.by_family.len() >= 3, "spans {:?}", out.by_family);
        // And no single exit SHAPE fills the search.
        let mut per_shape: BTreeMap<Vec<EndFamily>, usize> = BTreeMap::new();
        for c in &out.kept {
            *per_shape.entry(c.family_shape()).or_default() += 1;
        }
        assert!(per_shape.len() >= 2, "one shape took the whole set: {per_shape:?}");
        assert!(
            per_shape.values().all(|&n| n <= cfg.cap()),
            "{per_shape:?} cap {}",
            cfg.cap()
        );
        assert!(out.dropped_by_quota > 0, "the quota must be reported when it bites");
    }

    #[test]
    fn one_shape_cannot_take_the_whole_set() {
        // A library of nothing but flow and organic: exactly one exit shape exists,
        // so the cap must refuse to fill 10 slots with it.
        let mut cuts = table();
        cuts.exit.retain(|c| {
            matches!(end_family(c.metric), EndFamily::Flow | EndFamily::Organic)
        });
        let cfg = GeneratorConfig { slots: 10, ..Default::default() };
        let out = generate(&cuts, &cfg, &[]);
        let shapes: BTreeSet<Vec<EndFamily>> =
            out.kept.iter().map(|c| c.family_shape()).collect();
        assert_eq!(shapes.len(), 1, "{shapes:?}");
        assert_eq!(cfg.cap(), 4);
        assert_eq!(out.kept.len(), 4, "kept {} > cap {}", out.kept.len(), cfg.cap());
        assert!(out.dropped_by_quota > 0, "what the cap turned away is reported, not silent");
    }

    #[test]
    fn a_price_trail_candidate_is_kept_and_flagged() {
        let out = generate(&table(), &GeneratorConfig { slots: 400, ..Default::default() }, &[]);
        let trailing: Vec<&Candidate> = out
            .kept
            .iter()
            .filter(|c| c.searched_exit().iter().any(|x| is_price_trail_term(x.metric)))
            .collect();
        assert!(!trailing.is_empty(), "price trail must stay available");
        assert!(trailing.iter().all(|c| c.flags.contains(&TRAIL_FLAG)));

        // `retrace` is a price term but NOT the refuted token-ATH trail, so a bag
        // whose only price term is retrace is unflagged.
        let retrace_only = out.kept.iter().find(|c| {
            c.searched_exit().iter().any(|x| x.metric == MetricId::Retrace)
                && !c.searched_exit().iter().any(|x| is_price_trail_term(x.metric))
        });
        assert!(retrace_only.is_some_and(|c| c.flags.is_empty()));
    }

    /// `liquidity >= 85` is "sell at migration", not a discovered edge: present in
    /// every rule, never searched, never counted as an alarm kind.
    #[test]
    fn a_standing_term_rides_every_candidate_without_being_searched() {
        let standing = parse_standing_all(&["liquidity >= 85".to_string()]).expect("parses");
        assert_eq!(standing[0].clause.metric, MetricId::Liquidity);
        assert!((standing[0].clause.threshold - 85.0).abs() < 1e-9);

        let out = generate(&table(), &GeneratorConfig::default(), &standing);
        assert!(!out.kept.is_empty());
        for c in &out.kept {
            // In the executable rule…
            assert_eq!(c.n_standing, 1);
            assert!(c
                .combo
                .exit
                .clauses
                .iter()
                .any(|x| x.metric == MetricId::Liquidity && (x.threshold - 85.0).abs() < 1e-9));
            // …but not in the searched alarms, and not in the quota's families.
            assert!(!c.searched_exit().iter().any(|x| x.metric == MetricId::Liquidity));
            assert!(!c.families.contains(&EndFamily::LiquidityCeiling));
        }
        // The family it occupies is not searched for a duplicate either.
        assert!(!out.by_family.contains_key(&EndFamily::LiquidityCeiling));

        // The control carries it too, or the comparison is against a different rule.
        let control = ungated_control(&standing);
        assert_eq!(control.exit.clauses.len(), 1);
        assert!(control.entry.clauses.is_empty());

        // And it is recognised by (metric, window, value), never by slot order.
        assert!(is_standing(&standing, MetricId::Liquidity, None, 85.0));
        assert!(!is_standing(&standing, MetricId::Liquidity, None, 12.0));
    }

    #[test]
    fn a_malformed_standing_term_fails_the_run_rather_than_being_dropped() {
        assert!(parse_standing("liquidity >= 85").is_ok());
        assert!(parse_standing("untagged_buy(2s) >= 0.9").is_ok());
        assert!(parse_standing_all(&["liquidity >= 85".into(), "nonsense".into()]).is_err());
    }

    #[test]
    fn expansion_bases_are_picked_under_the_quota_not_by_raw_rank() {
        let out = generate(&table(), &GeneratorConfig { slots: 400, ..Default::default() }, &[]);
        let cands = &out.kept;
        // A round whose top is dominated by ONE exit shape — the exact input that
        // makes greedy top-N expansion widen that shape's lead round after round.
        let lead = cands[0].family_shape();
        let mut ranked: Vec<usize> = (0..cands.len()).collect();
        ranked.sort_by_key(|&i| (cands[i].family_shape() != lead, i));
        let raw_top: BTreeSet<Vec<EndFamily>> =
            ranked.iter().take(5).map(|&i| cands[i].family_shape()).collect();
        assert_eq!(raw_top.len(), 1, "the raw ranking leads with one shape");

        let bases = expansion_bases(&ranked, cands, 5);
        let shapes: BTreeSet<Vec<EndFamily>> =
            bases.kept.iter().map(|&i| cands[i].family_shape()).collect();
        assert!(shapes.len() >= 2, "raw top-5 would be one shape; got {shapes:?}");
        // 40% of 5 slots floors to 2 — no shape may exceed it.
        let cap = GeneratorConfig { slots: 5, family_quota: FAMILY_QUOTA }.cap();
        assert_eq!(cap, 2);
        for shape in &shapes {
            let n = bases.kept.iter().filter(|&&i| cands[i].family_shape() == *shape).count();
            assert!(n <= cap, "{shape:?} took {n} of {cap}");
        }
    }

    #[test]
    fn generation_is_reproducible_and_reads_no_rule() {
        let cuts = table();
        let cfg = GeneratorConfig::default();
        let a = generate(&cuts, &cfg, &[]);
        let b = generate(&cuts, &cfg, &[]);
        assert_eq!(a.kept.len(), b.kept.len());
        assert!(a.kept.iter().zip(&b.kept).all(|(x, y)| x.key() == y.key()));
        assert_eq!(a.by_family, b.by_family);
    }

    /// The enrich menu is what the finalist does NOT already carry — and an extra
    /// alarm must be a new KIND, or it is the same bet re-tuned.
    #[test]
    fn the_enrich_menu_offers_only_new_ideas_and_new_alarm_kinds() {
        let cuts = table();
        let out = generate(&cuts, &GeneratorConfig::default(), &[]);
        let top = &out.kept[0];
        let (entry_add, exit_add) =
            unused_clauses(&cuts, &top.combo.entry.clauses, top.searched_exit(), &[]);

        // Nothing already used comes back.
        for c in &exit_add {
            let fam = end_family(c.metric);
            assert!(!top.families.contains(&fam), "{fam:?} is already an alarm on the finalist");
        }
        for c in &entry_add {
            assert!(!top
                .combo
                .entry
                .clauses
                .iter()
                .any(|u| u.metric == c.metric && u.op == c.op));
        }
    }
}
