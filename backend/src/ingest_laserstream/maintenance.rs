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

/// Retention: keep 7 days of daily partitions. Shared by `raw_transactions`
/// (partitioned on `received_at`) and `trades` (partitioned on `block_time`).
/// 7 days covers the daily dump + safety margin; smaller live table means
/// indexes fit in the 256MB buffer pool, reducing disk thrash on the 4GB box.
const KEEP_DAYS: i64 = 7;
/// How often to roll partitions forward / drop expired ones.
const MAINT_INTERVAL_SECS: u64 = 6 * 3600;

/// Spawn-ready loop. First tick fires immediately so partitions are ensured at
/// startup (the migrations also pre-create them); then every 6h.
pub async fn run_partition_maintenance(pool: PgPool) {
    let mut tick = tokio::time::interval(Duration::from_secs(MAINT_INTERVAL_SECS));
    loop {
        tick.tick().await;
        let today: chrono::prelude::NaiveDate = Utc::now().date_naive();

        // Ensure today plus the next two days exist (ahead of ingest), and drop
        // every existing partition past the retention cutoff — for both the
        // raw-blob table and the trades table (same daily cadence).
        let cutoff = today - ChronoDuration::days(KEEP_DAYS);
        for (table, ensure_fn, drop_fn, parent) in [
            ("raw_tx", "ensure_raw_partition", "drop_raw_partition", "raw_transactions"),
            ("trades", "ensure_trades_partition", "drop_trades_partition", "trades"),
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

            // Self-healing retention: enumerate the *actual* daily partitions from
            // the catalog and drop any whose day is at/before the cutoff. A fixed
            // relative window (the old `today-30..today-43`) only cleaned a few
            // days, so >14 days of downtime permanently orphaned older partitions
            // → unbounded disk. Enumerating from `pg_inherits` re-targets every
            // stale partition regardless of how long the task was down.
            match expired_partitions(&pool, parent, cutoff).await {
                Ok(days) => {
                    for day in days {
                        if let Err(e) = sqlx::query(&format!("SELECT {drop_fn}($1)"))
                            .bind(day)
                            .execute(&pool)
                            .await
                        {
                            warn!("{table} partition drop {day}: {e}");
                        }
                    }
                }
                Err(e) => warn!("{table} partition enumerate for drop: {e}"),
            }
        }

        info!("raw_transactions + trades: daily partitions maintained (retain {KEEP_DAYS} days)");
    }
}

/// List the start-day of every existing daily partition of `parent` whose day is
/// at or before `cutoff` (i.e. past the retention window). Reads `pg_inherits` so
/// the maintenance task self-heals after any length of downtime instead of only
/// re-targeting a fixed relative window. Partition names follow the
/// `{parent}_YYYY_MM_DD` convention created by the `ensure_*_partition` SQL fns.
async fn expired_partitions(
    pool: &PgPool,
    parent: &str,
    cutoff: chrono::NaiveDate,
) -> Result<Vec<chrono::NaiveDate>, sqlx::Error> {
    let prefix = format!("{parent}_");
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT c.relname \
         FROM pg_inherits i \
         JOIN pg_class c ON c.oid = i.inhrelid \
         JOIN pg_class p ON p.oid = i.inhparent \
         WHERE p.relname = $1",
    )
    .bind(parent)
    .fetch_all(pool)
    .await?;

    let days = rows
        .into_iter()
        .filter_map(|(relname,)| expired_partition_day(&relname, &prefix, cutoff))
        .collect();
    Ok(days)
}

/// Parse the day out of a `{prefix}YYYY_MM_DD` partition name and return it iff it
/// is at/before `cutoff` (past retention). `None` for a non-matching name or a
/// day still within the window. Pure half of [`expired_partitions`] (testable
/// without a DB).
fn expired_partition_day(
    relname: &str,
    prefix: &str,
    cutoff: chrono::NaiveDate,
) -> Option<chrono::NaiveDate> {
    let suffix = relname.strip_prefix(prefix)?;
    let day = chrono::NaiveDate::parse_from_str(suffix, "%Y_%m_%d").ok()?;
    (day <= cutoff).then_some(day)
}

#[cfg(test)]
mod tests {
    use super::expired_partition_day;
    use chrono::NaiveDate;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn drops_partitions_at_or_before_cutoff() {
        let cutoff = d(2026, 5, 17); // today(2026-06-16) - 30
        // Older than cutoff → dropped.
        assert_eq!(
            expired_partition_day("trades_2026_05_01", "trades_", cutoff),
            Some(d(2026, 5, 1))
        );
        // Exactly the cutoff day → dropped (inclusive).
        assert_eq!(
            expired_partition_day("trades_2026_05_17", "trades_", cutoff),
            Some(d(2026, 5, 17))
        );
        // Within the retention window → kept.
        assert_eq!(
            expired_partition_day("trades_2026_06_10", "trades_", cutoff),
            None
        );
    }

    #[test]
    fn ignores_non_matching_names() {
        let cutoff = d(2026, 5, 17);
        // Wrong prefix (e.g. the parent table or a sibling partition set).
        assert_eq!(
            expired_partition_day("raw_transactions_2026_05_01", "trades_", cutoff),
            None
        );
        // Bare parent name, no date suffix.
        assert_eq!(expired_partition_day("trades", "trades_", cutoff), None);
        // Garbage suffix that isn't a date.
        assert_eq!(
            expired_partition_day("trades_default", "trades_", cutoff),
            None
        );
    }
}
