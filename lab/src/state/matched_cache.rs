//! Materialized matched-token **mint set** cache.
//!
//! The matched-token table pages/sorts/filters **server-side** (unified
//! `TableRequest` contract), but the match predicate is a Rust closure
//! (`token_matches_buy_rule` + `!is_mayhem_mode`), not SQL — so we can't page it
//! straight from the DB. Instead the first request runs the whole-table scan
//! ([`analysis::collect_matching_tokens`](trading_core::strategies::analysis)) to
//! materialize the matched **mint set**, caches it here keyed by
//! `(rule_id, from, to)`, and every later page/sort/filter re-queries the DB
//! restricted to `mint = ANY(mints)` — no re-scan.
//!
//! Mirrors [`SimResults`](crate::state::sim_results) TTL/GC: a handful of
//! user-driven entries, lazily evicted past [`MATCHED_TTL`] on every insert. A
//! re-run with the same key just refreshes the entry (rules/data may have moved).

use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use uuid::Uuid;

/// How long a materialized mint set is retained. The scan is expensive (whole
/// `tokens` table) and a user paging/sorting the matched table fires many quick
/// follow-ups, so this must comfortably outlive an interactive session. Kept in
/// line with the sim-result TTL magnitude.
const MATCHED_TTL: Duration = Duration::from_secs(600);

/// Cache key: a rule + its analysis window. `None` bounds fold to a stable key so
/// the all-time scan caches once. Times are truncated to whole seconds so a
/// sub-second wall-clock difference in the request doesn't miss the cache.
#[derive(Clone, PartialEq, Eq, Hash)]
struct MatchedKey {
    rule_id: Uuid,
    from: Option<i64>,
    to: Option<i64>,
}

impl MatchedKey {
    fn new(rule_id: Uuid, from: Option<DateTime<Utc>>, to: Option<DateTime<Utc>>) -> Self {
        Self { rule_id, from: from.map(|t| t.timestamp()), to: to.map(|t| t.timestamp()) }
    }
}

/// Strategy-agnostic store of materialized matched mint sets. The `Arc<Vec<String>>`
/// value makes handing a cached set to the handler a refcount bump, not a copy of
/// (potentially >5k) mints.
#[derive(Default)]
pub struct MatchedCache {
    map: DashMap<MatchedKey, (Instant, Arc<Vec<String>>)>,
}

impl MatchedCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up a previously materialized mint set for `(rule_id, from, to)`. `None`
    /// means the first request for this key (the caller runs the scan then
    /// [`insert`](Self::insert)s the result).
    pub fn get(
        &self,
        rule_id: Uuid,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
    ) -> Option<Arc<Vec<String>>> {
        let key = MatchedKey::new(rule_id, from, to);
        self.map.get(&key).and_then(|entry| {
            let (at, mints) = entry.value();
            (at.elapsed() < MATCHED_TTL).then(|| Arc::clone(mints))
        })
    }

    /// Store a freshly materialized mint set, first evicting any entries older than
    /// [`MATCHED_TTL`] (lazy GC — the map only ever holds a handful of user-driven
    /// rules). Returns the stored `Arc` for immediate use by the caller.
    pub fn insert(
        &self,
        rule_id: Uuid,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
        mints: Vec<String>,
    ) -> Arc<Vec<String>> {
        self.map.retain(|_, (at, _)| at.elapsed() < MATCHED_TTL);
        let key = MatchedKey::new(rule_id, from, to);
        let arc = Arc::new(mints);
        self.map.insert(key, (Instant::now(), Arc::clone(&arc)));
        arc
    }
}
