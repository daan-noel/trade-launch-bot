//! Shared get-or-compute helpers for the tiered analysis caches.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;

use crate::models::Token;
use crate::state::analysis_cache::{AnalysisCacheKey, AnalysisCache};
use crate::state::local_state::LocalState;
use crate::strategies::sim_fetch::fetch_sim_histories;
use crate::sweep::projection::CorpusTrade;

/// Return the materialized candidate `Token` rows for `key`, running `scan` on a
/// cache miss.
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
    let tokens = scan.await?;
    tracing::info!(
        strategy = %key.strategy_id,
        n = tokens.len(),
        "analysis cache: candidate miss — scanned tokens table"
    );
    Ok(cache.insert_candidates(key, tokens))
}

/// Return lake trade histories for the candidates at `key`, loading from Parquet
/// on a cache miss.
pub async fn get_or_fetch_histories(
    cache: &AnalysisCache,
    key: AnalysisCacheKey,
    tokens: &Arc<Vec<Token>>,
) -> Result<Arc<HashMap<String, Arc<Vec<CorpusTrade>>>>> {
    if let Some(cached) = cache.get_histories(&key) {
        tracing::debug!(
            strategy = %key.strategy_id,
            n = cached.len(),
            "analysis cache: history hit"
        );
        return Ok(cached);
    }
    let mints: Vec<String> = tokens.iter().map(|t| t.mint_address.clone()).collect();
    let histories = fetch_sim_histories(&mints, false).await?;
    tracing::info!(
        strategy = %key.strategy_id,
        n = histories.len(),
        "analysis cache: history miss — loaded from lake"
    );
    Ok(cache.insert_histories(key, histories))
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
) -> Result<Arc<HashMap<String, Arc<Vec<CorpusTrade>>>>> {
    get_or_fetch_histories(&state.analysis_cache, key, tokens).await
}
