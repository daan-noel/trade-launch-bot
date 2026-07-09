//! Lab-only schema migrations.
//!
//! These tables (grouped-sweep results, and future analysis-only tables) are
//! written **only by `lab`** — never by `live`/EC2. They therefore live in the
//! lab-owned migration set under `lab/migrations/`, applied here through a
//! **lab-private `_lab_migrations` ledger** that is fully independent of the
//! shared core `_sqlx_migrations` (run by `trading_core::storage::postgres::connect`).
//!
//! We reuse sqlx's `migrate!` macro purely as a compile-time embedder/iterator
//! (ordered files + checksums); we never call its `.run()`, so it never touches
//! core's `_sqlx_migrations` table. Add a lab-only table by dropping a new
//! `NNNN_<name>.sql` file into `lab/migrations/`.

use sqlx::{Executor, PgPool};

/// Embeds `lab/migrations/` at compile time (relative to the `lab` crate's
/// `CARGO_MANIFEST_DIR` — distinct from `trading_core/migrations/`).
static LAB_MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Apply pending lab-only migrations, tracked in `_lab_migrations`. Idempotent:
/// safe to run on every boot. Independent of core's `_sqlx_migrations`.
pub async fn run(pool: &PgPool) -> anyhow::Result<()> {
    pool.execute(
        "CREATE TABLE IF NOT EXISTS _lab_migrations (
             version    BIGINT      PRIMARY KEY,
             name       TEXT        NOT NULL,
             checksum   BYTEA       NOT NULL,
             applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
         )",
    )
    .await?;

    for m in LAB_MIGRATOR.iter() {
        let prev: Option<Vec<u8>> =
            sqlx::query_scalar("SELECT checksum FROM _lab_migrations WHERE version = $1")
                .bind(m.version)
                .fetch_optional(pool)
                .await?;

        match prev {
            Some(cs) if cs.as_slice() == m.checksum.as_ref() => continue, // already applied
            Some(_) => anyhow::bail!("lab migration {} body changed after it was applied", m.version),
            None => {
                // Multi-statement DDL — run unprepared via the simple-query path
                // (sqlx 0.6 has no `raw_sql`; `Executor::execute(&str)` allows it).
                pool.execute(m.sql.as_ref()).await?;
                sqlx::query(
                    "INSERT INTO _lab_migrations (version, name, checksum) VALUES ($1, $2, $3)",
                )
                .bind(m.version)
                .bind(&*m.description)
                .bind(&m.checksum[..])
                .execute(pool)
                .await?;
                tracing::info!(version = m.version, name = %m.description, "applied lab migration");
            }
        }
    }
    Ok(())
}
