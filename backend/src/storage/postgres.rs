use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::config::Settings;

/// Create a connection pool and apply all pending migrations.
pub async fn connect(settings: &Settings) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(settings.db_max_connections)
        .min_connections(settings.db_min_connections)
        .acquire_timeout(settings.db_acquire_timeout)
        .connect(&settings.database_url)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to PostgreSQL: {e}"))?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|e| anyhow::anyhow!("Migration failed: {e}"))?;

    tracing::info!("PostgreSQL connected, migrations applied");

    Ok(pool)
}
