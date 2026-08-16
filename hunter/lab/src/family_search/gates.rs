//! Refuse gates — the checks a run fails *before* it can report a number.
//!
//! Two of them: **freshness** (charter D7), which refuses a run whose range outruns
//! the data, and **axis duplication** (plan §2c), which refuses an entry clause that
//! is really a second reading of the fingerprint axis the family varies.

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
    /// The upper bound the request named.
    pub requested_until: DateTime<Utc>,
    /// Seconds the request outruns the data. `0` when the corpus reaches the bound.
    pub shortfall_secs: i64,
    /// The tolerance this run allowed.
    pub slack_secs: i64,
}

impl Freshness {
    /// Whether the run's range is silently shorter than requested by more than the
    /// slack it set. A corpus with no trades at all is stale under **any** slack —
    /// there is no range for it to be fresh for.
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
        Some(match self.last_trade_at {
            Some(last) => format!(
                "lake data ends {last} but the request runs to {} — {hours:.1}h short \
                 (slack {}s). Re-run `scripts/db-incremental-sync.ps1 -IncludeToday \
                 -ExportLake`, or lower the range's upper bound.",
                self.requested_until, self.slack_secs
            ),
            None => format!(
                "the corpus holds no trades at all, so nothing covers the requested \
                 range ending {}.",
                self.requested_until
            ),
        })
    }
}

/// Measure a loaded corpus against the range the request asked for.
pub fn freshness(corpus: &Corpus, requested_until: DateTime<Utc>, slack_secs: i64) -> Freshness {
    let last_trade_at = corpus.last_trade_at();
    let shortfall_secs = match last_trade_at {
        Some(last) => (requested_until - last).num_seconds().max(0),
        // No data at all: the whole request is shortfall, and it always refuses.
        None => i64::MAX,
    };
    Freshness { last_trade_at, requested_until, shortfall_secs, slack_secs: slack_secs.max(0) }
}

/// [`freshness`] then refuse — the one call the orchestrator makes.
pub fn check_freshness(
    corpus: &Corpus,
    requested_until: DateTime<Utc>,
    slack_secs: i64,
) -> anyhow::Result<Freshness> {
    let f = freshness(corpus, requested_until, slack_secs);
    match f.refuse_reason() {
        Some(why) => anyhow::bail!("family search refused on freshness: {why}"),
        None => Ok(f),
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
        let f = freshness(&c, until, DEFAULT_FRESHNESS_SLACK_SECS);
        assert_eq!(f.shortfall_secs, 2 * 86_400);
        assert!(f.stale());
        assert!(check_freshness(&c, until, DEFAULT_FRESHNESS_SLACK_SECS).is_err());
        // The refusal names the shortfall, so it is actionable rather than a bare no.
        assert!(f.refuse_reason().unwrap().contains("48.0h short"));
    }

    #[test]
    fn a_range_the_lake_covers_passes_and_reports_its_own_tail() {
        let c = corpus_of(vec![token_from_prices(&[1.0, 2.0, 3.0])]);
        let last = created_at() + Duration::seconds(2);
        // Inside the slack: a sealed-day export is routinely minutes behind.
        let f = check_freshness(&c, last + Duration::minutes(20), 3_600).expect("passes");
        assert!(!f.stale());
        assert_eq!(f.shortfall_secs, 20 * 60);
        assert_eq!(f.last_trade_at, Some(last));
        // A bound the data fully covers reports no shortfall at all, never a negative.
        let past = freshness(&c, last - Duration::hours(1), 0);
        assert_eq!(past.shortfall_secs, 0);
        assert!(!past.stale());
    }

    #[test]
    fn a_trade_less_corpus_always_refuses() {
        // Zero slack or a year of it — with no data there is no range to be fresh for.
        let c = corpus_of(vec![]);
        assert!(check_freshness(&c, created_at(), i64::MAX).is_err());
        assert_eq!(freshness(&c, created_at(), 0).last_trade_at, None);
    }

    /// The charter's own numbers: `liquidity > 20` on the reference family, ordered
    /// spend = 1 / 1.5 / 2 / 3 / 4 / 5.
    const SPEND: [f64; 6] = [1.0, 1.5, 2.0, 3.0, 4.0, 5.0];
    const LIQ_ADMIT: [f64; 6] = [0.36, 0.40, 0.42, 0.44, 0.84, 0.66];

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
        let d = axis_duplication("nonvol_buy >= 1.6", &admit, &SPEND);
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
