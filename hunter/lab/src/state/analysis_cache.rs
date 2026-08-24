//! Tiered analysis caches for matched + simulate.
//!
//! **Candidate cache** — materialized `Token` rows from the whole-table fingerprint
//! scan, keyed by `(strategy_id, fingerprint_key, from, to)` so rules that share a
//! token filter reuse one scan. **History cache** — lake trade histories for that
//! same key (identical mint set). TTL/GC mirrors [`SimResults`](super::sim_results).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use tokio::sync::OnceCell;

use crate::models::Token;
use crate::sweep::projection::CorpusTrade;

/// The lake trade histories for one fingerprint's candidate set: `mint → trades`.
type Histories = Arc<HashMap<String, Arc<Vec<CorpusTrade>>>>;

/// Candidate / history working-set TTL. Independent of [`SimResults`](super::sim_results)
/// (those now persist on disk until re-sim / config change). 60 min is enough for
/// a "Simulate All" batch + follow-up clicks without reloading the lake.
const CACHE_TTL: Duration = Duration::from_secs(3600);

/// Cache key for a fingerprint-scoped analysis window. `fingerprint_key` comes
/// from [`trading_core::strategies::match_keys::fingerprint_key`].
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct AnalysisCacheKey {
    pub strategy_id: String,
    pub fingerprint_key: String,
    from: Option<i64>,
    to: Option<i64>,
}

impl AnalysisCacheKey {
    pub fn new(
        strategy_id: impl Into<String>,
        fingerprint_key: impl Into<String>,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            strategy_id: strategy_id.into(),
            fingerprint_key: fingerprint_key.into(),
            from: from.map(|t| t.timestamp()),
            to: to.map(|t| t.timestamp()),
        }
    }
}

#[derive(Default)]
pub struct AnalysisCache {
    candidates: DashMap<AnalysisCacheKey, (Instant, Arc<Vec<Token>>)>,
    histories: DashMap<AnalysisCacheKey, (Instant, Histories)>,
    /// Single-flight cells for an in-progress candidate scan, keyed identically to
    /// `candidates`. Concurrent same-fingerprint rules (a "Simulate All" batch)
    /// collapse onto one scan — the leader runs it, followers await the same cell —
    /// so a cold cache can't stampede the batch pool with N identical whole-table
    /// scans. Entries are transient: removed once the leader resolves (the result
    /// lands in the TTL `candidates` map, which later arrivals then hit directly).
    candidates_inflight: DashMap<AnalysisCacheKey, Arc<OnceCell<Arc<Vec<Token>>>>>,
    /// Single-flight cells for an in-progress lake-history load — the history twin
    /// of `candidates_inflight`. Same fingerprint ⇒ same mint set ⇒ one lake read.
    histories_inflight: DashMap<AnalysisCacheKey, Arc<OnceCell<Histories>>>,
}

impl AnalysisCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop every entry past [`CACHE_TTL`] and report how many went. The ONE
    /// retain predicate — both `insert_*` paths and the idle reaper call this, so
    /// the eviction rule cannot drift between them.
    ///
    /// **Free by construction.** `get_candidates` / `get_histories` already reject
    /// an entry past the TTL, so a swept entry could never have served a hit: this
    /// returns dead bytes to the allocator and can never cost a cache hit or slow
    /// a backtest. It is only ever *when* the sweep runs that changes — previously
    /// an insert was the sole trigger, so a lab that finished its last job kept
    /// every expired candidate set and lake history resident indefinitely.
    pub fn gc(&self) -> usize {
        let before = self.candidates.len() + self.histories.len();
        self.candidates.retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
        self.histories.retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
        before.saturating_sub(self.candidates.len() + self.histories.len())
    }

    pub fn get_candidates(&self, key: &AnalysisCacheKey) -> Option<Arc<Vec<Token>>> {
        self.candidates.get(key).and_then(|entry| {
            let (at, tokens) = entry.value();
            (at.elapsed() < CACHE_TTL).then(|| Arc::clone(tokens))
        })
    }

    pub fn insert_candidates(
        &self,
        key: AnalysisCacheKey,
        tokens: Vec<Token>,
    ) -> Arc<Vec<Token>> {
        self.gc();
        let arc = Arc::new(tokens);
        self.candidates
            .insert(key, (Instant::now(), Arc::clone(&arc)));
        arc
    }

    pub fn get_histories(
        &self,
        key: &AnalysisCacheKey,
    ) -> Option<Arc<HashMap<String, Arc<Vec<CorpusTrade>>>>> {
        self.histories.get(key).and_then(|entry| {
            let (at, map) = entry.value();
            (at.elapsed() < CACHE_TTL).then(|| Arc::clone(map))
        })
    }

    pub fn insert_histories(
        &self,
        key: AnalysisCacheKey,
        histories: HashMap<String, Arc<Vec<CorpusTrade>>>,
    ) -> Arc<HashMap<String, Arc<Vec<CorpusTrade>>>> {
        self.gc();
        let arc = Arc::new(histories);
        self.histories
            .insert(key, (Instant::now(), Arc::clone(&arc)));
        arc
    }

    /// Get-or-create the single-flight cell for a candidate scan at `key`. Returns
    /// `(cell, leader)`: exactly one caller gets `leader == true` (it should run the
    /// scan and [`clear_candidate_inflight`](Self::clear_candidate_inflight) after);
    /// the rest await the same cell. The `entry` guard is dropped before returning,
    /// so callers never hold a shard lock across the scan's `.await`.
    pub fn candidate_inflight(
        &self,
        key: &AnalysisCacheKey,
    ) -> (Arc<OnceCell<Arc<Vec<Token>>>>, bool) {
        match self.candidates_inflight.entry(key.clone()) {
            Entry::Occupied(e) => (e.get().clone(), false),
            Entry::Vacant(e) => {
                let cell = Arc::new(OnceCell::new());
                e.insert(cell.clone());
                (cell, true)
            }
        }
    }

    /// Drop the candidate single-flight cell once its scan has resolved. Called by
    /// the leader; late arrivals take the TTL fast path instead.
    pub fn clear_candidate_inflight(&self, key: &AnalysisCacheKey) {
        self.candidates_inflight.remove(key);
    }

    /// History twin of [`candidate_inflight`](Self::candidate_inflight).
    pub fn history_inflight(
        &self,
        key: &AnalysisCacheKey,
    ) -> (Arc<OnceCell<Histories>>, bool) {
        match self.histories_inflight.entry(key.clone()) {
            Entry::Occupied(e) => (e.get().clone(), false),
            Entry::Vacant(e) => {
                let cell = Arc::new(OnceCell::new());
                e.insert(cell.clone());
                (cell, true)
            }
        }
    }

    /// History twin of [`clear_candidate_inflight`](Self::clear_candidate_inflight).
    pub fn clear_history_inflight(&self, key: &AnalysisCacheKey) {
        self.histories_inflight.remove(key);
    }

    /// Mint addresses derived from a cached or freshly scanned candidate set.
    pub fn mints_from(tokens: &Arc<Vec<Token>>) -> Arc<Vec<String>> {
        Arc::new(tokens.iter().map(|t| t.mint_address.clone()).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> AnalysisCacheKey {
        AnalysisCacheKey::new("s", "fp", None, None)
    }

    /// The guarantee the idle reaper leans on: `gc` sweeps only entries a `get`
    /// would already refuse, so running it on a timer can never cost a live hit —
    /// i.e. it can never slow a backtest down. A regression here would turn the
    /// reaper from free memory reclamation into a cache-thrash.
    #[test]
    fn gc_never_evicts_a_live_entry() {
        let cache = AnalysisCache::new();
        cache.insert_candidates(key(), Vec::new());
        cache.insert_histories(key(), HashMap::new());

        assert_eq!(cache.gc(), 0, "fresh entries are inside the TTL");
        assert!(cache.get_candidates(&key()).is_some());
        assert!(cache.get_histories(&key()).is_some());
    }

    /// `gc` is idempotent — a reaper pass on an already-swept cache is a no-op,
    /// not a source of repeated work.
    #[test]
    fn gc_is_idempotent() {
        let cache = AnalysisCache::new();
        cache.insert_candidates(key(), Vec::new());
        assert_eq!(cache.gc(), 0);
        assert_eq!(cache.gc(), 0);
        assert!(cache.get_candidates(&key()).is_some());
    }
}
