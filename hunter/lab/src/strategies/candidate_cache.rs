//! Shared get-or-compute helpers for the tiered analysis caches.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;

use crate::models::Token;
use crate::state::analysis_cache::{AnalysisCacheKey, AnalysisCache};
use crate::state::local_state::LocalState;
use crate::strategies::sim_fetch::fetch_sim_histories_from;
use crate::sweep::projection::CorpusTrade;

/// Return the materialized candidate `Token` rows for `key`, running `scan` on a
/// cache miss.
///
/// Single-flight: concurrent callers for the same `key` (same fingerprint over the
/// same window — the common case in a "Simulate All" batch) collapse onto ONE
/// scan. The leader runs `scan`; followers await its result off the shared cell,
/// so a cold cache never stampedes the batch pool with N identical whole-table
/// scans. The result lands in the TTL `candidates` map inside the cell, so any rule
/// arriving after resolution takes the fast path above instead of the cell.
pub async fn get_or_scan_candidates(
    cache: &AnalysisCache,
    key: AnalysisCacheKey,
    scan: Pin<Box<dyn Future<Output = Result<Vec<Token>>> + Send>>,
) -> Result<Arc<Vec<Token>>> {
    if let Some(cached) = cache.get_candidates(&key) {
        tracing::debug!(
            strategy = %key.strategy_id,
            n = cached.len(),
            "analysis cache: candidate hit"
        );
        return Ok(cached);
    }
    let (cell, leader) = cache.candidate_inflight(&key);
    let insert_key = key.clone();
    let result = cell
        .get_or_try_init(|| async move {
            let tokens = scan.await?;
            tracing::info!(
                strategy = %insert_key.strategy_id,
                n = tokens.len(),
                "analysis cache: candidate miss — scanned tokens table"
            );
            Ok::<_, anyhow::Error>(cache.insert_candidates(insert_key, tokens))
        })
        .await
        .map(Arc::clone);
    if leader {
        cache.clear_candidate_inflight(&key);
    }
    result
}

/// Return lake trade histories for the candidates at `key`, loading from Parquet
/// on a cache miss. Single-flight on `key` exactly like
/// [`get_or_scan_candidates`] — same fingerprint ⇒ same mint set ⇒ one lake read
/// shared by every rule in the group.
pub async fn get_or_fetch_histories(
    cache: &AnalysisCache,
    key: AnalysisCacheKey,
    tokens: &Arc<Vec<Token>>,
    with_flow: bool,
    created_after: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<Arc<HashMap<String, Arc<Vec<CorpusTrade>>>>> {
    if let Some(cached) = cache.get_histories(&key) {
        tracing::debug!(
            strategy = %key.strategy_id,
            n = cached.len(),
            "analysis cache: history hit"
        );
        return Ok(cached);
    }
    let (cell, leader) = cache.history_inflight(&key);
    let insert_key = key.clone();
    let tokens = Arc::clone(tokens);
    let result = cell
        .get_or_try_init(|| async move {
            let mints: Vec<String> = tokens.iter().map(|t| t.mint_address.clone()).collect();
            let histories =
                fetch_sim_histories_from(&mints, false, with_flow, created_after).await?;
            tracing::info!(
                strategy = %insert_key.strategy_id,
                n = histories.len(),
                with_flow,
                "analysis cache: history miss — loaded from lake"
            );
            Ok::<_, anyhow::Error>(cache.insert_histories(insert_key, histories))
        })
        .await
        .map(Arc::clone);
    if leader {
        cache.clear_history_inflight(&key);
    }
    result
}

/// Convenience wrapper reaching `LocalState::analysis_cache`.
pub async fn get_or_scan_candidates_state(
    state: &LocalState,
    key: AnalysisCacheKey,
    scan: Pin<Box<dyn Future<Output = Result<Vec<Token>>> + Send>>,
) -> Result<Arc<Vec<Token>>> {
    get_or_scan_candidates(&state.analysis_cache, key, scan).await
}

pub async fn get_or_fetch_histories_state(
    state: &LocalState,
    key: AnalysisCacheKey,
    tokens: &Arc<Vec<Token>>,
    with_flow: bool,
    created_after: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<Arc<HashMap<String, Arc<Vec<CorpusTrade>>>>> {
    get_or_fetch_histories(&state.analysis_cache, key, tokens, with_flow, created_after)
        .await
}

#[cfg(test)]
mod single_flight_tests {
    //! No-DB guard for the candidate single-flight: concurrent "Simulate All" rules
    //! that share a fingerprint (same key) must collapse onto ONE scan instead of
    //! stampeding the batch pool. Guards the invariant behind
    //! [`MAX_CONCURRENT_BACKTESTS`](crate::state::local_state) > 1.

    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A scan future that counts how many times it actually runs and yields once so
    /// a concurrently-polled follower registers on the shared in-flight cell before
    /// the leader completes.
    fn counting_scan(
        calls: Arc<AtomicUsize>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Token>>> + Send>> {
        Box::pin(async move {
            tokio::task::yield_now().await;
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::<Token>::new())
        })
    }

    #[tokio::test]
    async fn concurrent_same_key_scans_run_once() {
        let cache = AnalysisCache::new();
        let key = AnalysisCacheKey::new("tpsl_sniper_1", "fp_shared", None, None);
        let calls = Arc::new(AtomicUsize::new(0));

        let (a, b) = tokio::join!(
            get_or_scan_candidates(&cache, key.clone(), counting_scan(calls.clone())),
            get_or_scan_candidates(&cache, key.clone(), counting_scan(calls.clone())),
        );
        assert!(a.is_ok() && b.is_ok());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "same-fingerprint scans must collapse onto one"
        );

        // A later arrival takes the TTL fast path — still no new scan.
        let c = get_or_scan_candidates(&cache, key, counting_scan(calls.clone())).await;
        assert!(c.is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn distinct_keys_scan_independently() {
        let cache = AnalysisCache::new();
        let calls = Arc::new(AtomicUsize::new(0));

        let _ = get_or_scan_candidates(
            &cache,
            AnalysisCacheKey::new("s", "fp_a", None, None),
            counting_scan(calls.clone()),
        )
        .await;
        let _ = get_or_scan_candidates(
            &cache,
            AnalysisCacheKey::new("s", "fp_b", None, None),
            counting_scan(calls.clone()),
        )
        .await;
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "distinct fingerprints must each scan"
        );
    }
}
