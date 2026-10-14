//! Replace the volume tag's wallet list. Off the decision loop, on the batch pool.
//!
//! An instruction list that passes is appended by [`super::volume_save`] on the
//! print's writer. A wallet is not. This task rereads the launch group and writes
//! the addresses that still come back inside the recent window, then asks the
//! loop to reload. A fingerprint whose window cannot be measured is left as it is.

use std::time::Duration;

use sqlx::PgPool;
use tokio::sync::broadcast;
use tracing::info;

use trading_core::models::ingest::SseEvent;
use trading_core::storage::repositories::fingerprint_repo::FingerprintRepo;
use trading_core::storage::repositories::volume_wallet_repo;

use super::EngineHandle;

/// How often the stored wallet lists are replaced. A new launch moves the window
/// by one coin; this is short enough to drop a finished address and long enough
/// that the group scan is not on the print.
const EVERY: Duration = Duration::from_secs(300);

pub async fn run(
    engine: EngineHandle,
    sse_tx: broadcast::Sender<SseEvent>,
    batch: PgPool,
    repo: FingerprintRepo,
) {
    loop {
        match rebuild(&batch, &repo).await {
            Ok(changed) if changed > 0 => engine.schedule_reload(sse_tx.clone()),
            Ok(_) => {}
            Err(e) => tracing::warn!("volume wallet rebuild failed: {e:#}"),
        }
        tokio::time::sleep(EVERY).await;
    }
}

async fn rebuild(batch: &PgPool, repo: &FingerprintRepo) -> anyhow::Result<u32> {
    let rows = volume_wallet_repo::volume_fingerprints(batch).await?;
    let mut changed = 0u32;
    for row in &rows {
        let Some(next) = volume_wallet_repo::coming_back(batch, row).await? else {
            continue;
        };
        if repo.replace_volume_wallets(row.id, &next.wallets).await? {
            info!(
                fingerprint = %row.id,
                coins = next.coins,
                window = next.window,
                wallets = next.wallets.len(),
                "volume wallets replaced"
            );
            changed += 1;
        }
    }
    Ok(changed)
}
