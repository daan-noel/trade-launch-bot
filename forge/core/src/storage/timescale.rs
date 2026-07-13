//! TimescaleDB continuous-aggregate teardown, run once at boot (idempotent).
//!
//! **Dead CAggs (audit 2026-07-13).** Earlier scaffold created OHLCV candle
//! continuous aggregates (`trades_ohlcv_1m/5m/1h`) with refresh policies, but no
//! reader ever consumed them — every trade/candle read goes through the raw
//! `trades_priced` view. On the RAM-constrained 2vCPU/4GB box the 1m policy
//! re-scanned recent chunks every minute and materialized three hypertables for
//! nothing. Until a consumer exists, we do NOT create them; and because already
//! deployed DBs still carry the live views + policies, we drop them at boot so
//! the refresh load is reclaimed there too. The drop is idempotent
//! (`IF EXISTS`), so a fresh DB (nothing to drop) and a redeployed one both end
//! in the same state.
//!
//! When a candle endpoint is actually wired up, re-introduce the CAgg creation
//! here (it must stay out of the migration transaction — CAggs can't be created
//! inside one) and point the endpoint at the rollup.

use sqlx::PgPool;

/// Drop the dead OHLCV continuous aggregates + their refresh policies if present.
///
/// Dropping a continuous aggregate also removes its refresh policy, so no
/// separate policy teardown is needed. Rollups (`5m`/`1h`) are built on the `1m`
/// base, so they are dropped first to avoid needing `CASCADE`.
pub async fn teardown_dead_caggs(pool: &PgPool) -> anyhow::Result<()> {
    for view in ["trades_ohlcv_1h", "trades_ohlcv_5m", "trades_ohlcv_1m"] {
        sqlx::query(&format!("DROP MATERIALIZED VIEW IF EXISTS {view}"))
            .execute(pool)
            .await?;
    }
    Ok(())
}
