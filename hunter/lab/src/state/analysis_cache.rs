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
use dashmap::DashMap;

use crate::models::Token;
use crate::sweep::projection::CorpusTrade;

/// Kept in lock-step with [`SimResults`](super::sim_results)' `RESULT_TTL` (60 min):
/// a re-opened rule whose per-token result survived must also find its shared
/// candidate scan + lake histories still warm, so the fingerprint fast path holds.
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
    histories: DashMap<AnalysisCacheKey, (Instant, Arc<HashMap<String, Arc<Vec<CorpusTrade>>>>)>,
}

impl AnalysisCache {
    pub fn new() -> Self {
        Self::default()
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
        self.candidates
            .retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
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
        self.histories
            .retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
        let arc = Arc::new(histories);
        self.histories
            .insert(key, (Instant::now(), Arc::clone(&arc)));
        arc
    }

    /// Mint addresses derived from a cached or freshly scanned candidate set.
    pub fn mints_from(tokens: &Arc<Vec<Token>>) -> Arc<Vec<String>> {
        Arc::new(tokens.iter().map(|t| t.mint_address.clone()).collect())
    }
}
