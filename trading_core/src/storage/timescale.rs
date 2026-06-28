//! TimescaleDB continuous-aggregate setup, run once at boot (idempotent).
//!
//! Hypertables + compression/retention policies live in the SQL migration
//! (they are transaction-safe). Continuous aggregates **cannot** be created
//! inside a transaction block, and sqlx 0.6 wraps every migration in one — so the
//! wall-clock OHLCV candle CAggs are created here instead, via autocommit
//! statements after `migrate!` has run. Each step checks existence first, so
//! re-running on every boot (live and lab both call this) is a no-op.
//!
//! - `trades_ohlcv_1m` — base 1-minute candle, bucketed on `block_time` (the
//!   hypertable partition column, which is why `trades` keeps the timestamp).
//!   open/close are by execution order (`slot`); volume is summed lamports.
//! - `trades_ohlcv_5m` / `trades_ohlcv_1h` — cheap **hierarchical** CAggs built
//!   *on* the 1m rollup, not re-scans of `trades`.

use sqlx::PgPool;

/// Create the OHLCV continuous aggregates + their refresh policies if absent.
/// Best-effort per CAgg but errors propagate (a misconfigured TimescaleDB should
/// surface at boot, not silently skip candles).
pub async fn setup_caggs(pool: &PgPool) -> anyhow::Result<()> {
    setup_base_1m(pool).await?;
    setup_rollup(pool, "trades_ohlcv_5m", "trades_ohlcv_1m", "5 minutes").await?;
    setup_rollup(pool, "trades_ohlcv_1h", "trades_ohlcv_1m", "1 hour").await?;
    Ok(())
}

/// True if a continuous aggregate with `view_name` already exists.
async fn cagg_exists(pool: &PgPool, view_name: &str) -> anyhow::Result<bool> {
    let exists: Option<i32> = sqlx::query_scalar(
        "SELECT 1 FROM timescaledb_information.continuous_aggregates \
         WHERE view_name = $1",
    )
    .bind(view_name)
    .fetch_optional(pool)
    .await?;
    Ok(exists.is_some())
}

async fn setup_base_1m(pool: &PgPool) -> anyhow::Result<()> {
    if cagg_exists(pool, "trades_ohlcv_1m").await? {
        return Ok(());
    }
    // open/close ordered by `slot` (block_time is second-resolution and can't order
    // within/between blocks); slot resolves all but same-slot ties (immaterial).
    sqlx::query(
        r#"
        CREATE MATERIALIZED VIEW trades_ohlcv_1m
        WITH (timescaledb.continuous) AS
        SELECT
            mint_address,
            time_bucket(INTERVAL '1 minute', block_time) AS bucket,
            first(sol_amount::double precision / NULLIF(token_amount, 0), slot) AS open_price,
            max(sol_amount::double precision / NULLIF(token_amount, 0))         AS high_price,
            min(sol_amount::double precision / NULLIF(token_amount, 0))         AS low_price,
            last(sol_amount::double precision / NULLIF(token_amount, 0), slot)  AS close_price,
            sum(sol_amount)                                                     AS volume_lamports,
            count(*)                                                           AS trade_count
        FROM trades
        GROUP BY mint_address, time_bucket(INTERVAL '1 minute', block_time)
        WITH NO DATA
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "SELECT add_continuous_aggregate_policy('trades_ohlcv_1m', \
            start_offset => INTERVAL '10 minutes', \
            end_offset   => INTERVAL '1 minute', \
            schedule_interval => INTERVAL '1 minute', \
            if_not_exists => true)",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Hierarchical rollup CAgg `name` built on the finer `source` CAgg at `interval`.
async fn setup_rollup(
    pool: &PgPool,
    name: &str,
    source: &str,
    interval: &str,
) -> anyhow::Result<()> {
    if cagg_exists(pool, name).await? {
        return Ok(());
    }
    sqlx::query(&format!(
        r#"
        CREATE MATERIALIZED VIEW {name}
        WITH (timescaledb.continuous) AS
        SELECT
            mint_address,
            time_bucket(INTERVAL '{interval}', bucket) AS bucket,
            first(open_price, bucket) AS open_price,
            max(high_price)           AS high_price,
            min(low_price)            AS low_price,
            last(close_price, bucket) AS close_price,
            sum(volume_lamports)      AS volume_lamports,
            sum(trade_count)          AS trade_count
        FROM {source}
        GROUP BY mint_address, time_bucket(INTERVAL '{interval}', bucket)
        WITH NO DATA
        "#,
    ))
    .execute(pool)
    .await?;

    sqlx::query(&format!(
        "SELECT add_continuous_aggregate_policy('{name}', \
            start_offset => INTERVAL '3 hours', \
            end_offset   => INTERVAL '{interval}', \
            schedule_interval => INTERVAL '{interval}', \
            if_not_exists => true)",
    ))
    .execute(pool)
    .await?;
    Ok(())
}
