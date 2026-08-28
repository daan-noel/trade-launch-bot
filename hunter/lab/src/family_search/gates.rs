//! Refuse gates — the checks a run fails *before* it can report a number.
//!
//! Four of them, in the order a run meets them:
//!
//! 1. **freshness** (D7) — the requested range outruns the data.
//! 2. **cost clearance** (D8) — the cohort's *best available* exits do not clear the
//!    round trip, so no rule can exist there. The cheapest refusal in the pipeline:
//!    it costs one corpus load and saves the whole search.
//! 3. **axis duplication** (plan §2c) — an entry clause that is really a second
//!    reading of the fingerprint axis the family varies.
//! 4. **entry timing** (plan §4d) — a *diagnostic*, never a refusal: an entry clause
//!    that binds the entry instant and correlates with low capture is created by the
//!    move it is trying to precede.

use chrono::{DateTime, Utc};

use crate::family_search::score::spearman;
use crate::sweep::corpus::Corpus;

/// Default tolerance between a request's `until` and the lake's newest print. A lake
/// export seals whole days, so the tail is routinely minutes-to-an-hour behind "now"
/// without the run being wrong; two days behind is a silently shorter range.
pub const DEFAULT_FRESHNESS_SLACK_SECS: i64 = 3_600;

/// How fresh the data a run actually scanned is, against the range it asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Freshness {
    /// `Corpus::last_trade_at` — the newest print anywhere in the loaded corpus.
    /// `None` for a trade-less corpus, which is itself a refusal.
    pub last_trade_at: Option<DateTime<Utc>>,
    /// The upper bound the request **named**. `None` for an open-ended range, which
    /// asks for whatever the lake holds and so cannot outrun it.
    pub requested_until: Option<DateTime<Utc>>,
    /// Seconds the request outruns the data. `0` when the corpus reaches the bound,
    /// and always `0` for an open-ended range.
    pub shortfall_secs: i64,
    /// The tolerance this run allowed.
    pub slack_secs: i64,
}

impl Freshness {
    /// Whether the run's range is silently shorter than requested by more than the
    /// slack it set. A corpus with no trades at all is stale under **any** slack —
    /// there is no range for it to be fresh for.
    ///
    /// An **open-ended** request is never stale on shortfall: it named no bound, so
    /// the lake's tail *is* the range and nothing was silently shortened. Measuring it
    /// against `now` instead refuses every open-ended run, because a lake export seals
    /// whole days and its tail is always hours behind the wall clock.
    pub fn stale(&self) -> bool {
        self.last_trade_at.is_none() || self.shortfall_secs > self.slack_secs
    }

    /// The refusal, phrased so an operator can act on it: what was asked, what exists,
    /// and by how much. `None` when the run may proceed.
    ///
    /// D7 is a **gate**, not a footnote: a window that ends two days early produces
    /// numbers that look ordinary and answer a different question, and nothing
    /// downstream can detect it.
    pub fn refuse_reason(&self) -> Option<String> {
        if !self.stale() {
            return None;
        }
        let hours = self.shortfall_secs as f64 / 3_600.0;
        Some(match (self.last_trade_at, self.requested_until) {
            (Some(last), Some(until)) => format!(
                "lake data ends {last} but the request runs to {until} — {hours:.1}h \
                 short (slack {}s). Re-run `scripts/db-incremental-sync.ps1 \
                 -IncludeToday -ExportLake`, or lower the range's upper bound.",
                self.slack_secs
            ),
            // Unreachable while an open-ended range reports no shortfall, but the
            // message stays honest about which bound it is talking about.
            (Some(last), None) => format!(
                "lake data ends {last}, {hours:.1}h short of the run's horizon \
                 (slack {}s).",
                self.slack_secs
            ),
            (None, Some(until)) => format!(
                "the corpus holds no trades at all, so nothing covers the requested \
                 range ending {until}."
            ),
            (None, None) => "the corpus holds no trades at all, so there is no range \
                 for it to cover."
                .to_string(),
        })
    }
}

/// Measure a loaded corpus against the range the request asked for. `requested_until`
/// is the bound the request **named** — `None` for an open-ended range, which has no
/// shortfall to measure.
pub fn freshness(
    corpus: &Corpus,
    requested_until: Option<DateTime<Utc>>,
    slack_secs: i64,
) -> Freshness {
    let last_trade_at = corpus.last_trade_at();
    let shortfall_secs = match (last_trade_at, requested_until) {
        (Some(last), Some(until)) => (until - last).num_seconds().max(0),
        // No bound named: the lake's tail is the range, so nothing is short.
        (Some(_), None) => 0,
        // No data at all: the whole request is shortfall, and it always refuses.
        (None, _) => i64::MAX,
    };
    Freshness { last_trade_at, requested_until, shortfall_secs, slack_secs: slack_secs.max(0) }
}

/// [`freshness`] then refuse — the one call the orchestrator makes.
pub fn check_freshness(
    corpus: &Corpus,
    requested_until: Option<DateTime<Utc>>,
    slack_secs: i64,
) -> anyhow::Result<Freshness> {
    let f = freshness(corpus, requested_until, slack_secs);
    match f.refuse_reason() {
        Some(why) => anyhow::bail!("family search refused on freshness: {why}"),
        None => Ok(f),
    }
}

// ─────────────────────────── cost clearance (charter D8) ──────────────────────
//
// A dump-scalp family lost 6.93 pp per round trip to execution alone while its signal
// was near-breakeven (PF 0.947 at first-fill + fee-only pricing). No threshold fixes
// that: the loss is a ratio, and a strategy targeting 3–12% moves cannot clear a ~6 pp
// cost. The refusal has to come **before** the generator, because a search over a
// cohort whose available moves live inside the execution band is spent on a question
// with no answer.
//
// The instrument is the oracle, which already exists (D3) and is already charged the
// same round trip as a realized exit. Fed the **ungated control's** outcomes it is
// rule-free — it says what the *best possible* exit pays on this cohort, and no exit
// rule can beat the best exit.

/// Default strictness: refuse only when the typical entry's best available exit fails
/// to clear its own round trip. Unarguable — the search would be looking for an exit
/// better than the best one.
///
/// Raise it towards `1.0` for the stricter bar the dump-scalp result sets: the typical
/// best exit must leave at least one further round trip of headroom, since a rule only
/// ever takes a fraction of the oracle.
pub const DEFAULT_COST_CLEARANCE_MARGIN: f64 = 0.0;

/// Whether this cohort can pay for its own execution, measured before any rule exists.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CostClearance {
    /// One round trip's cost at the run's buy size and the cohort's median entry
    /// depth, as a percent of notional. Derived from the run's own cost model — never
    /// a hard-coded figure, so a bigger buy or a fee change moves the bar with it.
    pub band_pct: f64,
    /// Median **net** oracle round trip over every priceable entry, as a percent of
    /// notional. Losers included: a median over the winners alone is positive by
    /// construction and could never refuse anything.
    pub median_move_pct: Option<f64>,
    /// Entries the oracle could price.
    pub n_priced: u64,
    /// Of those, the ones where a profitable exit existed at all.
    pub n_with_upside: u64,
    /// The bar this run set, in bands.
    pub margin: f64,
}

impl CostClearance {
    /// `median_move_pct ÷ band_pct` — how many round trips of headroom the typical
    /// best exit leaves. `None` when the band is zero (a cost-free model).
    pub fn headroom(&self) -> Option<f64> {
        let m = self.median_move_pct?;
        (self.band_pct.abs() > f64::EPSILON).then(|| m / self.band_pct)
    }

    /// The cohort is untradeable at this buy size — refuse before generating.
    ///
    /// An unpriceable cohort never refuses: absence of a measurement is not a verdict,
    /// and the run already fails earlier if it has no entries at all.
    pub fn refuses(&self) -> bool {
        self.median_move_pct.is_some_and(|m| m <= self.margin * self.band_pct)
    }

    /// Clears the bar, but by less than one round trip. Badge, never refuse: a rule
    /// takes a *fraction* of the oracle, so headroom under 1 band is the shape that
    /// looks tradeable offline and is not.
    pub fn thin(&self) -> bool {
        !self.refuses() && self.headroom().is_some_and(|h| h <= 1.0)
    }

    /// The refusal, phrased so an operator can act on it. `None` when the run proceeds.
    pub fn refuse_reason(&self) -> Option<String> {
        let m = self.median_move_pct?;
        self.refuses().then(|| {
            format!(
                "the typical entry's BEST available exit pays {m:+.2}% against a \
                 {:.2}% round trip at this buy size — the cohort cannot pay for its \
                 own execution, and no exit rule beats the best exit. Search refused \
                 before generating. Trade a larger target size, or lower the buy \
                 until the band clears.",
                self.band_pct
            )
        })
    }
}

/// Measure a cohort against its own execution cost.
///
/// `moves_pct` is every priceable entry's net oracle round trip as a percent of
/// notional ([`oracle_moves`](crate::family_search::oracle::oracle_moves)), and
/// `band_pct` is [`execution_band_pct`](crate::family_search::oracle::execution_band_pct)
/// at the cohort's median entry depth.
pub fn cost_clearance(
    moves_pct: &[f64],
    n_with_upside: u64,
    band_pct: f64,
    margin: f64,
) -> CostClearance {
    CostClearance {
        band_pct,
        median_move_pct: crate::family_search::oracle::median(moves_pct),
        n_priced: moves_pct.len() as u64,
        n_with_upside,
        margin: margin.max(0.0),
    }
}

// ─────────────────────────── entry timing (plan §4d) ──────────────────────────
//
// Dropping `gross_flow(60) >= 55` from the scalp family improved quality *and* raised
// volume: it was not selecting good launches, it was selecting *post-move moments* —
// the gate was created by the very move it was supposed to precede, and it was the
// AND-binding clause deciding entry timing.
//
// Diagnostic only. Unlike axis duplication, "this clause is late" is not by itself
// wrong: some edges genuinely wait for confirmation. What makes it a finding is the
// pairing — binds the instant AND the entries it produces have little upside left.

/// How much later a clause makes the entry, and what that costs in available upside.
#[derive(Clone, Debug, PartialEq)]
pub struct EntryTiming {
    pub clause: String,
    /// Seconds the clause adds to the mean entry delay (finalist minus finalist-without
    /// -it). Positive ⇒ it holds entries back.
    pub delay_added_secs: f64,
    /// Percentage points of capture the clause costs (finalist minus without).
    /// Negative ⇒ removing it captured *more* of the available money.
    pub capture_delta_pp: Option<f64>,
    /// Entries the clause removes, as a share of the cohort. Positive ⇒ it filters.
    pub admit_delta_pct: f64,
}

impl EntryTiming {
    /// Binds the entry instant **and** the entries it produces have less upside left —
    /// the signature of a gate the move itself creates.
    pub fn lagging(&self) -> bool {
        self.delay_added_secs > 0.0 && self.capture_delta_pp.is_some_and(|d| d < 0.0)
    }

    /// What it means, for the board. `None` when the clause is not lagging.
    pub fn note(&self) -> Option<String> {
        self.lagging().then(|| {
            format!(
                "`{}` delays entry by {:.1}s on average and the entries it keeps \
                 capture {:.1}pp LESS of the money that was available — it is \
                 selecting moments after the move rather than before it.",
                self.clause,
                self.delay_added_secs,
                -self.capture_delta_pp.unwrap_or(0.0)
            )
        })
    }
}

// ─────────────────────────── axis duplication (plan §2c) ──────────────────────
//
// `liquidity > 20` admits 84% / 66% of the spend=4 / spend=5 cohorts but only 36–44%
// of spend=1 / 1.5 / 2 / 3 — because a larger initial buy mechanically creates the
// liquidity. Such a clause is not an entry filter; it is the fingerprint axis read
// twice, and it will look like a working entry rule on every family that varies that
// axis. Entry logic does not transfer (unlike exit logic), which is exactly why the
// entry side needs a gate the exit side does not.
//
// Costs **zero extra runs**: `enter_pct` per cohort already falls out of the scoring
// the family loop performs.

/// |ρ| at or above which an entry clause is refused as an axis proxy.
pub const AXIS_DUPLICATION_RHO: f64 = 0.8;

/// One entry clause measured against the family's varied axis.
#[derive(Clone, Debug, PartialEq)]
pub struct AxisDuplication {
    pub clause: String,
    /// Spearman(admit rate, varied-axis value) over the family. `None` when it can't
    /// be computed — one cohort, or a constant on either side.
    pub rho: Option<f64>,
}

impl AxisDuplication {
    /// Whether the clause tracks the axis closely enough to be a proxy for it. Both
    /// signs refuse: a clause that anti-selects on the axis re-reads it just as much.
    pub fn duplicates(&self) -> bool {
        self.rho.is_some_and(|r| r.abs() >= AXIS_DUPLICATION_RHO)
    }

    /// Why it was refused, for the board. `None` when the clause stands.
    pub fn refuse_reason(&self) -> Option<String> {
        let r = self.rho?;
        self.duplicates().then(|| {
            format!(
                "`{}` admits in lockstep with the varied fingerprint axis (rho {r:+.2}) — \
                 it re-reads the axis rather than filtering within it. Demote to a \
                 diagnostic.",
                self.clause
            )
        })
    }
}

/// Measure one entry clause against the family's varied axis.
///
/// `admit_rate[i]` is the clause's admit rate on family member `i`; `axis_value[i]` is
/// that member's varied-axis value, in the same order. Both are already in hand from
/// the family loop's own scoring.
pub fn axis_duplication(
    clause: impl Into<String>,
    admit_rate: &[f64],
    axis_value: &[f64],
) -> AxisDuplication {
    AxisDuplication { clause: clause.into(), rho: spearman(admit_rate, axis_value) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::family_search::fixtures::{created_at, token_from_prices};
    use crate::sweep::corpus::{Corpus, CorpusToken};
    use chrono::Duration;

    fn corpus_of(tokens: Vec<CorpusToken>) -> Corpus {
        Corpus { tokens, hash: "fs-test".into(), has_fingerprints: false, candidates_capped: false }
    }

    #[test]
    fn a_request_that_outruns_the_lake_is_refused() {
        // Five prints, one per second from `created_at` — the lake ends at +4s.
        let c = corpus_of(vec![token_from_prices(&[1.0, 2.0, 3.0, 4.0, 5.0])]);
        let last = created_at() + Duration::seconds(4);
        assert_eq!(c.last_trade_at(), Some(last));

        // Two days past the data, one hour of slack.
        let until = last + Duration::days(2);
        let f = freshness(&c, Some(until), DEFAULT_FRESHNESS_SLACK_SECS);
        assert_eq!(f.shortfall_secs, 2 * 86_400);
        assert!(f.stale());
        assert!(check_freshness(&c, Some(until), DEFAULT_FRESHNESS_SLACK_SECS).is_err());
        // The refusal names the shortfall, so it is actionable rather than a bare no.
        assert!(f.refuse_reason().unwrap().contains("48.0h short"));
    }

    #[test]
    fn a_range_the_lake_covers_passes_and_reports_its_own_tail() {
        let c = corpus_of(vec![token_from_prices(&[1.0, 2.0, 3.0])]);
        let last = created_at() + Duration::seconds(2);
        // Inside the slack: a sealed-day export is routinely minutes behind.
        let f = check_freshness(&c, Some(last + Duration::minutes(20)), 3_600).expect("passes");
        assert!(!f.stale());
        assert_eq!(f.shortfall_secs, 20 * 60);
        assert_eq!(f.last_trade_at, Some(last));
        // A bound the data fully covers reports no shortfall at all, never a negative.
        let past = freshness(&c, Some(last - Duration::hours(1)), 0);
        assert_eq!(past.shortfall_secs, 0);
        assert!(!past.stale());
    }

    /// The regression: an open-ended range names no upper bound, so the lake's tail
    /// IS the range. Measuring it against `now` refuses every such run — a sealed-day
    /// export is permanently hours behind the wall clock.
    #[test]
    fn an_open_ended_range_is_fresh_however_old_the_lake_is() {
        let c = corpus_of(vec![token_from_prices(&[1.0, 2.0, 3.0])]);
        let f = check_freshness(&c, None, 0).expect("an unbounded range cannot outrun the lake");
        assert_eq!(f.shortfall_secs, 0);
        assert_eq!(f.requested_until, None);
        assert!(!f.stale());
        assert_eq!(f.refuse_reason(), None);
    }

    #[test]
    fn a_trade_less_corpus_always_refuses() {
        // Zero slack or a year of it — with no data there is no range to be fresh for.
        let c = corpus_of(vec![]);
        assert!(check_freshness(&c, Some(created_at()), i64::MAX).is_err());
        assert_eq!(freshness(&c, Some(created_at()), 0).last_trade_at, None);
        // Open-ended is no escape hatch: with no data there is nothing to cover.
        assert!(check_freshness(&c, None, i64::MAX).is_err());
    }

    /// The charter's own numbers: `liquidity > 20` on the reference family, ordered
    /// spend = 1 / 1.5 / 2 / 3 / 4 / 5.
    const SPEND: [f64; 6] = [1.0, 1.5, 2.0, 3.0, 4.0, 5.0];
    const LIQ_ADMIT: [f64; 6] = [0.36, 0.40, 0.42, 0.44, 0.84, 0.66];

    /// The dump-scalp shape: the typical entry's *best available* exit does not clear
    /// the round trip, so no exit rule can exist there and the search must not run.
    #[test]
    fn a_cohort_whose_best_exits_sit_inside_the_cost_band_is_refused() {
        // Net oracle moves, in percent of notional. The median is −0.4%: the typical
        // token's best exit after the fill still loses money.
        let moves = [-9.0, -3.1, -0.9, 0.1, 2.0, 8.0];
        let band = 6.93; // the measured round trip on that family

        let c = cost_clearance(&moves, 3, band, DEFAULT_COST_CLEARANCE_MARGIN);
        assert!((c.median_move_pct.unwrap() + 0.4).abs() < 1e-9);
        assert!(c.refuses(), "median {:?} must not clear zero", c.median_move_pct);
        // The refusal names both numbers, so it is actionable rather than a bare no.
        let why = c.refuse_reason().unwrap();
        assert!(why.contains("6.93") && why.contains("-0.40"), "{why}");

        // Losers are IN the sample. Filtering to the winners makes the median positive
        // by construction, and a gate that can never fire is not a gate.
        let winners: Vec<f64> = moves.iter().copied().filter(|m| *m > 0.0).collect();
        assert!(!cost_clearance(&winners, 3, band, 0.0).refuses());
    }

    #[test]
    fn a_cohort_with_room_is_admitted_and_the_margin_moves_the_bar() {
        // Median +14%: two round trips of headroom at a 6.93% band.
        let moves = [-4.0, 6.0, 12.0, 16.0, 30.0, 44.0];
        let band = 6.93;

        let ok = cost_clearance(&moves, 5, band, 0.0);
        assert!(!ok.refuses());
        assert!(!ok.thin(), "headroom {:?} is over one band", ok.headroom());
        assert!((ok.headroom().unwrap() - 14.0 / band).abs() < 1e-9);

        // The strict bar — one further round trip of headroom — still admits it, and
        // so does two (14% is 2.02 bands)...
        assert!(!cost_clearance(&moves, 5, band, 1.0).refuses());
        assert!(!cost_clearance(&moves, 5, band, 2.0).refuses());
        // ...but three does not. Same data; the operator moved the line.
        assert!(cost_clearance(&moves, 5, band, 3.0).refuses());

        // Clears, but by less than one round trip: badge, never refuse.
        let tight = cost_clearance(&[1.0, 4.0, 5.0, 9.0], 4, band, 0.0);
        assert!(!tight.refuses() && tight.thin());
    }

    #[test]
    fn an_unmeasurable_cohort_is_never_refused() {
        // Absence of a measurement is not a verdict. (A cohort with no entries at all
        // fails earlier, on the corpus.)
        let c = cost_clearance(&[], 0, 6.93, 1.0);
        assert_eq!(c.median_move_pct, None);
        assert!(!c.refuses() && !c.thin());
        assert_eq!(c.refuse_reason(), None);
    }

    /// A clause that holds entries back **and** whose kept entries have less upside
    /// left — `gross_flow(60) >= 55`, which was created by the dump it was gating on.
    #[test]
    fn a_clause_the_move_itself_creates_reads_as_lagging() {
        let late = EntryTiming {
            clause: "gross_flow(60s) >= 55".into(),
            delay_added_secs: 9.4,
            capture_delta_pp: Some(-6.2),
            admit_delta_pct: 19.0,
        };
        assert!(late.lagging());
        assert!(late.note().unwrap().contains("after the move"));

        // Delays entry but the entries it keeps capture MORE — a real wait-gate.
        let patient = EntryTiming { capture_delta_pp: Some(4.0), ..late.clone() };
        assert!(!patient.lagging());
        assert_eq!(patient.note(), None);

        // Costs capture but does not bind the instant — not this diagnostic's finding.
        let early = EntryTiming { delay_added_secs: -0.2, ..late.clone() };
        assert!(!early.lagging());

        // Unmeasurable capture never accuses.
        assert!(!EntryTiming { capture_delta_pp: None, ..late }.lagging());
    }

    #[test]
    fn an_entry_clause_tracking_the_varied_axis_is_refused() {
        let d = axis_duplication("liquidity > 20", &LIQ_ADMIT, &SPEND);
        assert!(d.rho.unwrap() >= AXIS_DUPLICATION_RHO, "rho {:?}", d.rho);
        assert!(d.duplicates());
        assert!(d.refuse_reason().unwrap().contains("liquidity > 20"));
    }

    #[test]
    fn a_clause_that_anti_selects_on_the_axis_is_refused_too() {
        // A perfect inverse re-reads the axis exactly as much as a perfect match.
        let inverted: Vec<f64> = LIQ_ADMIT.iter().rev().copied().collect();
        let d = axis_duplication("liquidity < 20", &inverted, &SPEND);
        assert!(d.rho.unwrap() < 0.0);
        assert!(d.duplicates(), "both signs refuse: rho {:?}", d.rho);
    }

    #[test]
    fn a_clause_independent_of_the_axis_stands() {
        // Admits scattered against the axis — a real filter, not a second reading.
        let admit = [0.55, 0.30, 0.61, 0.28, 0.58, 0.33];
        let d = axis_duplication("untagged_buy >= 1.6", &admit, &SPEND);
        assert!(!d.duplicates(), "rho {:?}", d.rho);
        assert_eq!(d.refuse_reason(), None);
    }

    #[test]
    fn one_cohort_cannot_measure_duplication_and_never_refuses() {
        // A family of one: nothing to correlate, so the gate stays silent rather
        // than refusing (or clearing) a clause it cannot judge.
        let d = axis_duplication("liquidity > 20", &[0.84], &[5.0]);
        assert_eq!(d.rho, None);
        assert!(!d.duplicates());
        assert_eq!(d.refuse_reason(), None);
    }
}
