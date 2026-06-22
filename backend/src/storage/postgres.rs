use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::config::Settings;

/// The three workload-isolated Postgres connection pools. Splitting by workload
/// keeps each contention class off the others' connections:
///
/// - `hot`   — ingest `DbWriter`, `StrategyRunner`, maintenance, seed, background caches.
/// - `api`   — fast HTTP handlers: dashboard list/detail/count reads, settings, mutations.
/// - `batch` — long, DB-heavy jobs: grouped-sweep corpus load + per-group writer, tpsl backtests.
///
/// A sweep or backtest can saturate `batch` without starving the dashboard (`api`)
/// or the latency-critical ingest path (`hot`) — the contention that surfaced as
/// "pool timed out while waiting for an open connection" when the sweep shared the
/// API pool.
pub struct DbPools {
    pub hot: PgPool,
    pub api: PgPool,
    pub batch: PgPool,
}

/// Build one pool with the given sizing. No per-connection setup — sizing +
/// acquire-timeout only.
async fn build_pool(
    settings: &Settings,
    max_connections: u32,
    min_connections: u32,
) -> anyhow::Result<PgPool> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .min_connections(min_connections)
        .acquire_timeout(settings.db_acquire_timeout)
        .connect(&settings.database_url)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to PostgreSQL: {e}"))
}

/// Connect all three pools and apply pending migrations once (on the `hot` pool —
/// it's built first at boot and owns schema setup for all three).
pub async fn connect(settings: &Settings) -> anyhow::Result<DbPools> {
    let hot = build_pool(settings, settings.db_max_connections, settings.db_min_connections).await?;

    sqlx::migrate!("./migrations")
        .run(&hot)
        .await
        .map_err(|e| anyhow::anyhow!("Migration failed: {e}"))?;

    let api = build_pool(
        settings,
        settings.db_api_max_connections,
        settings.db_api_min_connections,
    )
    .await?;
    let batch = build_pool(
        settings,
        settings.db_batch_max_connections,
        settings.db_batch_min_connections,
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
