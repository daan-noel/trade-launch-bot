use sqlx::postgres::PgPoolOptions;
use sqlx::{Executor, PgPool};

use crate::config::Settings;

/// Build a pool with the given sizing. `db_idle_tx_timeout` (when non-zero) is
/// applied as `idle_in_transaction_session_timeout` on every connection at
/// check-out-from-fresh, so a leaked/stuck *open* transaction can't pin a pool
/// slot forever (the classic pool-drain bug). It never affects a connection
/// actively running a query (e.g. a long sweep corpus scan) — only one sitting
/// idle inside a transaction.
async fn build_pool(
    settings: &Settings,
    max_connections: u32,
    min_connections: u32,
) -> anyhow::Result<PgPool> {
    let opts = PgPoolOptions::new()
        .max_connections(max_connections)
        .min_connections(min_connections)
        .acquire_timeout(settings.db_acquire_timeout);

    let pool = if settings.db_idle_tx_timeout.is_zero() {
        opts.connect(&settings.database_url).await
    } else {
        let ms = settings.db_idle_tx_timeout.as_millis();
        opts.after_connect(move |conn, _meta| {
            Box::pin(async move {
                conn.execute(
                    format!("SET idle_in_transaction_session_timeout = {ms}").as_str(),
                )
                .await?;
                Ok(())
            })
        })
        .connect(&settings.database_url)
        .await
    }
    .map_err(|e| anyhow::anyhow!("Failed to connect to PostgreSQL: {e}"))?;

    Ok(pool)
}

/// Create the **hot-path** connection pool and apply all pending migrations.
/// Backs ingest (`DbWriter`), `StrategyRunner`, maintenance, seeding and the
/// background caches — everything except the HTTP handlers, which get their own
/// pool via [`connect_api_pool`] so a write storm here can't starve the API.
pub async fn connect(settings: &Settings) -> anyhow::Result<PgPool> {
    let pool = build_pool(settings, settings.db_max_connections, settings.db_min_connections).await?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|e| anyhow::anyhow!("Migration failed: {e}"))?;

    tracing::info!(
        max_connections = settings.db_max_connections,
        "PostgreSQL hot-path pool connected, migrations applied"
    );

    Ok(pool)
}

/// Create the dedicated **API** connection pool — backs only the HTTP request
/// handlers (`AppState.db`). Isolated from the hot-path pool so the relentless
/// ingest/strategy traffic can't exhaust the connections the dashboard needs.
/// Migrations are NOT run here; [`connect`] (called first at boot) owns them.
pub async fn connect_api_pool(settings: &Settings) -> anyhow::Result<PgPool> {
    let pool = build_pool(
        settings,
        settings.db_api_max_connections,
        settings.db_api_min_connections,
    )
    .await?;

    tracing::info!(
        max_connections = settings.db_api_max_connections,
        "PostgreSQL API pool connected"
    );

    Ok(pool)
}
