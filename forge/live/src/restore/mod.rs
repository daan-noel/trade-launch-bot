//! Keystore-based DB restore (LIVE box). Rebuilds `managed_wallets`, `launches`,
//! `tokens`, `trades`, and `token_positions` on a fresh server from the copied
//! keystore folder + on-chain history — the recovery path for a redeploy that
//! brings up an empty `forge_bot`.
//!
//! Three phases, each its own submodule:
//! 1. [`wallets`]   — decrypt every `.enc` blob → insert its `managed_wallets` row.
//! 2. [`backfill`]  — page every wallet's signatures, decode each tx through the
//!    live ingest decode+map path, persist trades/tokens/launches.
//! 3. [`positions`] — reconcile `token_positions` for every mint we touched.
//!
//! Driven by `POST /api/wallet_pool/restore` (a `tokio::spawn`ed background job);
//! progress streams over the existing SSE bus. Every write is idempotent, so the
//! whole run is safe to repeat.

pub mod backfill;
pub mod positions;
pub mod wallets;

use anyhow::Result;
use serde::Serialize;
use sqlx::PgPool;
use tracing::info;

use launcher::LauncherSettings;

use crate::ingest::pumpfun::PumpFunAdapter;
use crate::sse::SseHub;

/// What a restore run rebuilt — returned to the caller and logged on completion.
/// Fresh ids everywhere (the keystore holds no lifecycle metadata); on-chain data
/// joins by address, so nothing downstream depends on the original ids.
#[derive(Debug, Clone, Serialize, Default)]
pub struct RestoreSummary {
    /// Wallet rows freshly inserted this run (a re-run reports 0 — already present).
    pub wallets_inserted: usize,
    /// Total managed wallets known after the wallet phase (inserted + pre-existing).
    pub wallets_total: usize,
    /// Trade rows inserted from the on-chain backfill (post-dedup).
    pub trades: u64,
    /// Own-launch `launches` rows discovered + inserted.
    pub launches: usize,
    /// Distinct mints observed in managed-wallet history (the reconcile set size).
    pub mints: usize,
    /// Mints whose `token_positions` reconciled against chain without error.
    pub positions_reconciled: usize,
}

/// Run the full keystore restore end-to-end. Long-running (pages + fetches every
/// wallet's on-chain history) — the HTTP handler `tokio::spawn`s it and returns
/// `202` immediately. Every phase is idempotent, so a failure mid-run is recovered
/// by simply re-invoking.
pub async fn run_restore(
    pool: PgPool,
    settings: LauncherSettings,
    sse: SseHub,
) -> Result<RestoreSummary> {
    info!("keystore restore: starting");

    // Phase 1 — rebuild managed_wallets from the keystore folder.
    sse.restore_progress("wallets", 0, 0);
    let wallets = wallets::restore_wallets(&pool, &settings).await?;
    sse.restore_progress("wallets", wallets.by_address.len(), wallets.by_address.len());

    // Phase 2 — backfill trades / tokens / launches from on-chain history.
    let adapter = PumpFunAdapter::resolve(&pool).await?;
    let backfilled = backfill::backfill(&pool, &settings, &adapter, &wallets, &sse).await?;

    // Phase 3 — reconcile token_positions for every mint we touched.
    let positions_reconciled =
        positions::reconcile(&pool, &settings, &backfilled.mints, &sse).await?;

    let summary = RestoreSummary {
        wallets_inserted: wallets.inserted,
        wallets_total: wallets.by_address.len(),
        trades: backfilled.trades,
        launches: backfilled.launches,
        mints: backfilled.mints.len(),
        positions_reconciled,
    };
    info!(?summary, "keystore restore: complete");
    sse.restore_complete(&serde_json::to_value(&summary).unwrap_or_default());
    Ok(summary)
}
