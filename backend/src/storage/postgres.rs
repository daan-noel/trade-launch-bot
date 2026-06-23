use std::time::Duration;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::config::Settings;

/// Applied to **every** pooled connection (all three pools): a transaction left
/// idle — e.g. a handler that errored after `BEGIN` without committing/rolling
/// back — is aborted and its connection reclaimed after this long. Cheap
/// insurance against a leaked transaction permanently holding a pool slot; no
/// legitimate path idles inside a transaction this long.
const IDLE_IN_TX_TIMEOUT: Duration = Duration::from_secs(30);

/// Per-statement ceiling on the **API** pool only. Interactive dashboard reads
/// should be fast; under disk pressure a pathological query is killed at this
/// point (returning an error the handler surfaces) instead of holding its
/// connection until the `acquire_timeout` drains the whole pool for everyone
/// else. Deliberately *below* `db_acquire_timeout` (10s) so the slow query frees
/// its slot before queued requests time out. Not applied to `hot` (must never
/// kill money-critical batched trade/migration writes) or `batch` (backtests run
/// long by design — bounded by job-level concurrency, not a statement timeout).
const API_STATEMENT_TIMEOUT: Duration = Duration::from_secs(8);

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

/// Build one pool with the given sizing and per-session hold-time guards. Every
/// new connection gets `idle_in_transaction_session_timeout` (all pools) and,
/// when `statement_timeout` is `Some`, a `statement_timeout` — so a single slow
/// query or leaked transaction can't hold a slot past the acquire timeout and
/// drain the pool. `None` leaves `statement_timeout` at the Postgres default
/// (disabled).
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
        // Runs once per physical connection (incl. reconnects), so the guards
        // always apply. `SET` is session-local — scoped to this pooled connection.
        .after_connect(move |conn, _meta| {
            let guard_sql = guard_sql.clone();
            Box::pin(async move {
                // `guard_sql` is two `SET`s in one string. Run it through the
                // simple-query protocol (`Executor::execute(&str)`), which allows
                // multiple commands per round-trip; `sqlx::query(..)` would prepare
                // it and Postgres rejects multi-statement prepared statements (42601).
                use sqlx::Executor;
                conn.execute(guard_sql.as_str()).await?;
                Ok(())
            })
        })
        .connect(&settings.database_url)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to PostgreSQL: {e}"))
}

/// The `SET` run on every new connection: `statement_timeout` (in ms; `0` = the
/// Postgres default, disabled, when `statement_timeout` is `None`) plus the
/// always-on `idle_in_transaction_session_timeout`. Pure so the millisecond
/// conversion + unit wiring is verified without a DB — a future edit can't
/// silently turn `Duration::from_secs(8)` into `8` ms or drop the idle guard.
fn session_guard_sql(statement_timeout: Option<Duration>) -> String {
    let stmt_ms: i64 = statement_timeout.map_or(0, |d| d.as_millis() as i64);
    let idle_ms: i64 = IDLE_IN_TX_TIMEOUT.as_millis() as i64;
    format!(
        "SET statement_timeout = {stmt_ms}; \
         SET idle_in_transaction_session_timeout = {idle_ms}"
    )
}

/// Connect all three pools and apply pending migrations once (on the `hot` pool —
/// it's built first at boot and owns schema setup for all three).
pub async fn connect(settings: &Settings) -> anyhow::Result<DbPools> {
    // `hot` runs batched, money-critical ingest writes — no statement timeout, so a
    // slow write under load is never killed mid-batch.
    let hot = build_pool(
        settings,
        settings.db_max_connections,
        settings.db_min_connections,
        None,
    )
    .await?;

    sqlx::migrate!("./migrations")
        .run(&hot)
        .await
        .map_err(|e| anyhow::anyhow!("Migration failed: {e}"))?;

    // `api` serves interactive reads — bound each statement so one slow query under
    // disk pressure can't drain the pool the dashboard depends on.
    let api = build_pool(
        settings,
        settings.db_api_max_connections,
        settings.db_api_min_connections,
        Some(API_STATEMENT_TIMEOUT),
    )
    .await?;
    // `batch` runs long backtests/sweeps by design — no per-statement ceiling;
    // concurrency is bounded at the job level instead.
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
    fn api_pool_guard_sets_8s_statement_and_30s_idle() {
        // The api pool passes `Some(API_STATEMENT_TIMEOUT)` (8s) → 8000ms, and the
        // always-on idle guard is 30s → 30000ms. Pins the seconds→ms conversion so a
        // refactor can't regress the units (8s must not become `8`).
        assert_eq!(
            session_guard_sql(Some(API_STATEMENT_TIMEOUT)),
            "SET statement_timeout = 8000; SET idle_in_transaction_session_timeout = 30000"
        );
    }

    #[test]
    fn hot_and_batch_pools_disable_statement_timeout_but_keep_idle_guard() {
        // hot/batch pass `None` → `statement_timeout = 0` (Postgres default: disabled),
        // so a long batched write / backtest statement is never killed — but the idle
        // guard still reaps a leaked transaction.
        assert_eq!(
            session_guard_sql(None),
            "SET statement_timeout = 0; SET idle_in_transaction_session_timeout = 30000"
        );
    }
}
