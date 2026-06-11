//! Weekly partition maintenance for `raw_transactions` (WS ingest).
//!
//! Rolls the weekly partitions forward (so inserts always have a target) and
//! drops partitions past the retention window. The partition naming/bounds live
//! in SQL functions (`ensure_raw_partition` / `drop_raw_partition`, migration
//! 0012); this task just passes anchor dates. Sibling of
//! `ingest_laserstream::maintenance`, kept separate per the clone convention.

use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use sqlx::PgPool;
use tracing::{info, warn};

/// Retention: keep roughly two months of weekly partitions. Must match the
/// KEEP_WEEKS the 0012 migration pre-creates against.
const KEEP_WEEKS: i64 = 9;
/// How often to roll partitions forward / drop expired ones.
const MAINT_INTERVAL_SECS: u64 = 6 * 3600;

/// Spawn-ready loop. First tick fires immediately so partitions are ensured at
/// startup (the migration also pre-creates them); then every 6h.
pub async fn run_partition_maintenance(pool: PgPool) {
    let mut tick = tokio::time::interval(Duration::from_secs(MAINT_INTERVAL_SECS));
    loop {
        tick.tick().await;
        let today = Utc::now().date_naive();

        // Ensure the current week plus the next two exist (ahead of ingest).
        for k in 0i64..3 {
            let anchor = today + ChronoDuration::weeks(k);
            if let Err(e) = sqlx::query("SELECT ensure_raw_partition($1)")
                .bind(anchor)
                .execute(&pool)
                .await
            {
                warn!("raw partition ensure {anchor}: {e}");
            }
        }

        // Drop a small window of weeks past the retention cutoff.
        for k in 0i64..4 {
            let anchor = today - ChronoDuration::weeks(KEEP_WEEKS + k);
            if let Err(e) = sqlx::query("SELECT drop_raw_partition($1)")
                .bind(anchor)
                .execute(&pool)
                .await
            {
                warn!("raw partition drop {anchor}: {e}");
            }
        }

        info!("raw_transactions: weekly partitions maintained (retain ~{KEEP_WEEKS} weeks)");
    }
}
