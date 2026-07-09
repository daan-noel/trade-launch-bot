//! Resolve the matched mint set for a rule — shared by matched handlers and
//! backtests (via [`super::candidate_cache`] directly).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::models::Token;
use crate::state::analysis_cache::{AnalysisCache, AnalysisCacheKey};
use crate::state::local_state::LocalState;
use crate::strategies::candidate_cache::get_or_scan_candidates;
use trading_core::models::StrategyRule;
use trading_core::strategies::match_keys::fingerprint_key;

/// Materialize (or reuse) the matched mint set for `strategy_rule` over
/// `[since, until)`.
pub async fn resolve_matched_mints(
    state: &LocalState,
    strategy_rule: &StrategyRule,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    scan: Pin<Box<dyn Future<Output = Result<Vec<Token>>> + Send>>,
) -> Result<Arc<Vec<String>>> {
    let key = AnalysisCacheKey::new(
        &strategy_rule.strategy_id,
        fingerprint_key(strategy_rule),
        since,
        until,
    );
    let tokens = get_or_scan_candidates(&state.analysis_cache, key, scan).await?;
    Ok(AnalysisCache::mints_from(&tokens))
}
