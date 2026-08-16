//! Refuse gates — the checks a run fails *before* it can report a number.
//!
//! Slice 1 lands the freshness gate (charter D7). The axis-duplication gate (D-2c)
//! joins it once the family loop exists.

use chrono::{DateTime, Utc};

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
}
