//! The build-breadth table's daily refresh.
//!
//! `m_holder_book` classes a holder at its first buy from the day's build breadth:
//! distinct wallets per build recipe on the previous UTC day. The engine holds one
//! day's table; the boot path loads today's before the first rule load, and this
//! task swaps in the next day's shortly after 00:00 UTC.
//!
//! The compute is one `GROUP BY` over a day of `trades`, heavier than the launch-build
//! door's, so it runs HERE, off the decision loop, and the loop only swaps the table
//! it is handed ([`EngineHandle::set_build_breadth`]).
//!
//! A failed refresh keeps the previous day's table and logs at error level: holders
//! are then classed on yesterday's breadth, which moves slowly, while an empty table
//! would class every holder as a private bot without saying so.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sqlx::PgPool;

use hunter_engine::event::BuildBreadth;
use trading_core::storage::repositories::build_breadth_repo::BuildBreadthRepo;

use super::EngineHandle;

/// Retry cadence when a refresh fails.
const RETRY_DELAY: Duration = Duration::from_secs(60);

/// Refresh once per UTC day, after the boot load.
pub async fn run(engine: EngineHandle, pool: PgPool) {
    let repo = BuildBreadthRepo::new(pool);
    loop {
        tokio::time::sleep(super::door_refresh::until_next_refresh(Utc::now())).await;
        loop {
            match load_today(&repo).await {
                Ok(breadth) => match engine.set_build_breadth(breadth).await {
                    Ok(()) => break,
                    Err(e) => tracing::error!("build breadth: the engine did not take the table: {e}"),
                },
                Err(e) => tracing::error!(
                    "build breadth refresh failed: {e} - holding the previous day's table"
                ),
            }
            tokio::time::sleep(RETRY_DELAY).await;
        }
    }
}

/// Today's table, as the engine event carries it.
pub async fn load_today(repo: &BuildBreadthRepo) -> anyhow::Result<Arc<[BuildBreadth]>> {
    let day = Utc::now().date_naive();
    let rows = repo.load_or_compute_day(day).await?;
    let breadth: Arc<[BuildBreadth]> = Arc::from(BuildBreadthRepo::to_engine(&rows));
    tracing::info!(%day, recipes = breadth.len(), "build breadth loaded");
    Ok(breadth)
}
