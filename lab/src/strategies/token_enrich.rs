//! Simulated-table enrichment — a thin `lab` wrapper over the shared
//! [`trading_core::storage::token_enrichment`] SSOT. The backtest rows are built in
//! Rust (no SQL to extend), so after `select_simulated_tokens` narrows to the final
//! set we do one bounded batch fetch over exactly those mints and attach the shared
//! [`TokenEnrichment`] onto each row (`#[serde(flatten)]`). See that module for the
//! canonical field set shared with the Matched / Positions / Sweep tables.

use std::collections::HashMap;

use sqlx::PgPool;

pub use trading_core::storage::token_enrichment::TokenEnrichment;
use trading_core::storage::token_enrichment::{self, TokenEnrichmentRow};

/// Batch-fetch enrichment for `mints` (bounded; never a table scan) and key the
/// **rows** by mint for an O(1) merge into each result row. The full
/// [`TokenEnrichmentRow`] is returned (not the flattened [`TokenEnrichment`]) so a
/// host can also read the row-owned `ath_price` off it — the Simulated table sets
/// its ATH from `tokens_info` here, exactly like Positions/Sweep, rather than
/// recomputing it from the trade corpus. Empty input short-circuits.
pub async fn fetch_enrichment(
    pool: &PgPool,
    mints: &[String],
) -> anyhow::Result<HashMap<String, TokenEnrichmentRow>> {
    let rows = token_enrichment::fetch_by_mints(pool, mints).await?;
    Ok(rows
        .into_iter()
        .map(|r: TokenEnrichmentRow| (r.mint_address.clone(), r))
        .collect())
}
