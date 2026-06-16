//! Daily partition maintenance for `raw_transactions` and `trades`.
//!
//! Rolls the daily partitions forward (so inserts always have a target) and
//! drops partitions past the retention window. The partition naming/bounds live
//! in SQL functions (`ensure_raw_partition`/`drop_raw_partition` in migration
//! 0001; `ensure_trades_partition`/`drop_trades_partition` in migration 0002);
//! this task just passes anchor dates.

use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use sqlx::PgPool;
use tracing::{info, warn};

/// Retention: keep one month (30 days) of daily partitions. Shared by
/// `raw_transactions` (partitioned on `received_at`) and `trades` (partitioned
/// on `block_time`). Must match the pre-created window in migrations 0001/0003.
const KEEP_DAYS: i64 = 30;
/// How often to roll partitions forward / drop expired ones.
const MAINT_INTERVAL_SECS: u64 = 6 * 3600;

/// Spawn-ready loop. First tick fires immediately so partitions are ensured at
/// startup (the migrations also pre-create them); then every 6h.
pub async fn run_partition_maintenance(pool: PgPool) {
    let mut tick = tokio::time::interval(Duration::from_secs(MAINT_INTERVAL_SECS));
    loop {
        tick.tick().await;
        let today = Utc::now().date_naive();

        // Ensure today plus the next two days exist (ahead of ingest), and drop a
        // window of days past the retention cutoff — for both the raw-blob table
        // and the trades table (same daily cadence). The drop window is wide
        // enough to clean up days missed during downtime.
        for (table, ensure_fn, drop_fn) in [
            ("raw_tx", "ensure_raw_partition", "drop_raw_partition"),
            ("trades", "ensure_trades_partition", "drop_trades_partition"),
        ] {
            for k in 0i64..3 {
                let anchor = today + ChronoDuration::days(k);
                if let Err(e) = sqlx::query(&format!("SELECT {ensure_fn}($1)"))
                    .bind(anchor)
                    .execute(&pool)
                    .await
                {
                    warn!("{table} partition ensure {anchor}: {e}");
                }
            }

            for k in 0i64..14 {
                let anchor = today - ChronoDuration::days(KEEP_DAYS + k);
                if let Err(e) = sqlx::query(&format!("SELECT {drop_fn}($1)"))
                    .bind(anchor)
                    .execute(&pool)
                    .await
                {
                    warn!("{table} partition drop {anchor}: {e}");
                }
            }
        }

        info!("raw_transactions + trades: daily partitions maintained (retain {KEEP_DAYS} days)");
    }
}
