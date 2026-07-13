//! Postgres connection pools + boot (migrations, continuous aggregates).
//!
//! Workload-isolated pools (carried from hunter): splitting by contention
//! class keeps a long analysis query off the latency-critical ingest path.
//!   - `hot`   — ingest writes, strategy eval, background caches (no stmt timeout).
//!   - `api`   — fast interactive HTTP reads (bounded stmt timeout).
//!   - `batch` — long analysis jobs (no stmt timeout; bounded by job concurrency).
//!
//! Every connection gets an `idle_in_transaction_session_timeout` guard so a
//! leaked transaction can't permanently hold a pool slot.

use std::time::Duration;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::config::Settings;

/// Idle-in-transaction reaper on every pooled connection (all pools).
const IDLE_IN_TX_TIMEOUT: Duration = Duration::from_secs(30);
/// Per-statement ceiling on the `api` pool only (interactive reads stay fast).
const API_STATEMENT_TIMEOUT: Duration = Duration::from_secs(8);

/// The three workload-isolated pools.
pub struct DbPools {
    pub hot: PgPool,
    pub api: PgPool,
    pub batch: PgPool,
}

/// The `SET`s applied on every new physical connection. Pure so the ms
/// conversion + unit wiring is verified without a DB.
fn session_guard_sql(statement_timeout: Option<Duration>) -> String {
    let stmt_ms: i64 = statement_timeout.map_or(0, |d| d.as_millis() as i64);
    let idle_ms: i64 = IDLE_IN_TX_TIMEOUT.as_millis() as i64;
    format!(
        "SET statement_timeout = {stmt_ms}; \
         SET idle_in_transaction_session_timeout = {idle_ms}"
    )
}

async fn build_pool(
    settings: &Settings,
    max_connections: u32,
    min_connections: u32,
    statement_timeout: Option<Duration>,
) -> anyhow::Result<PgPool> {
    let guard_sql = session_guard_sql(statement_timeout);
    PgPoolOptions::new()
        .max_connections(max_connections)
        .min_connections(min_connections)
        .acquire_timeout(settings.db_acquire_timeout)
        .after_connect(move |conn, _meta| {
            let guard_sql = guard_sql.clone();
            Box::pin(async move {
                // Multi-statement `SET` via the simple-query protocol (a prepared
                // statement rejects multiple commands, 42601).
                use sqlx::Executor;
                conn.execute(guard_sql.as_str()).await?;
                Ok(())
            })
        })
        .connect(&settings.database_url)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to PostgreSQL: {e}"))
}

/// Connect all three pools; apply pending migrations + continuous-aggregate setup
/// once on the `hot` pool (built first, owns schema setup for all three).
pub async fn connect(settings: &Settings) -> anyhow::Result<DbPools> {
    // `hot` runs batched, money-critical writes — no statement timeout.
    let hot = build_pool(
        settings,
        settings.db_max_connections,
        settings.db_min_connections,
        None,
    )
    .await?;

    // Migrations live in the forge product root (`forge/migrations/`), one level
    // above this crate; the `sqlx migrate` CLI and this embedded runner read the
    // same directory.
    sqlx::migrate!("../migrations")
        .run(&hot)
        .await
        .map_err(|e| anyhow::anyhow!("Migration failed: {e}"))?;

    // Drop the dead OHLCV CAggs (no reader exists) so already-deployed DBs stop
    // paying their per-minute refresh cost. Idempotent. See `storage::timescale`.
    super::timescale::teardown_dead_caggs(&hot)
        .await
        .map_err(|e| anyhow::anyhow!("TimescaleDB continuous-aggregate teardown failed: {e}"))?;

    let api = build_pool(
        settings,
        settings.db_api_max_connections,
        settings.db_api_min_connections,
        Some(API_STATEMENT_TIMEOUT),
    )
    .await?;
    let batch = build_pool(
        settings,
        settings.db_batch_max_connections,
        settings.db_batch_min_connections,
        None,
    )
    .await?;

    tracing::info!(
        hot = settings.db_max_connections,
        api = settings.db_api_max_connections,
        batch = settings.db_batch_max_connections,
        "PostgreSQL pools connected, migrations applied"
    );

    Ok(DbPools { hot, api, batch })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_guard_sets_stmt_and_idle_ms() {
        assert_eq!(
            session_guard_sql(Some(API_STATEMENT_TIMEOUT)),
            "SET statement_timeout = 8000; SET idle_in_transaction_session_timeout = 30000"
        );
        assert_eq!(
            session_guard_sql(None),
            "SET statement_timeout = 0; SET idle_in_transaction_session_timeout = 30000"
        );
    }
}
