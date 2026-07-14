//! Restore step 3 — reconcile `token_positions` for every backfilled mint.
//!
//! For each mint a managed wallet held or traded, run the launcher's on-chain
//! position reconciler. It probes every managed wallet's canonical ATA
//! (`getMultipleAccounts`) and seeds a `token_positions` row for any wallet holding
//! the mint — including co-buys of tokens we didn't launch — then writes the
//! balance + feed-derived cost basis / realized proceeds (resolvable now because
//! step 2 backfilled the trades). Own-launch mints already have their `launches`
//! row (step 2), so the ATA token-program selection is correct for them.
//!
//! Idempotent (`token_positions` upserts on `(mint, wallet)`), best-effort: a
//! per-mint failure is logged and skipped so one bad mint never aborts the restore.

use std::collections::HashSet;

use anyhow::Result;
use sqlx::PgPool;
use tracing::{info, warn};

use launcher::{reconcile_positions, LauncherSettings};

use crate::sse::SseHub;

/// Reconcile positions for all `mints`. Returns the count of mints reconciled
/// without error.
pub async fn reconcile(
    pool: &PgPool,
    settings: &LauncherSettings,
    mints: &HashSet<String>,
    sse: &SseHub,
) -> Result<usize> {
    let total = mints.len();
    let mut reconciled = 0usize;
    sse.restore_progress("positions", 0, total);
    for (i, mint) in mints.iter().enumerate() {
        match reconcile_positions(pool, settings, mint).await {
            Ok(()) => reconciled += 1,
            Err(e) => warn!(%mint, ?e, "restore positions: reconcile failed — skipping mint"),
        }
        sse.restore_progress("positions", i + 1, total);
    }
    info!(reconciled, total, "restore positions: complete");
    Ok(reconciled)
}
