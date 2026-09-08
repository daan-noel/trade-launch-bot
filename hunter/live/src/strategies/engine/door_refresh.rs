//! The launch-build door's daily refresh.
//!
//! The two `build_prev_day_*` fingerprint axes are stamped at `TokenCreated` from a
//! map the engine holds, and that map is one UTC day's `launch_build_day_stats`. It
//! is a **creation-time** input: a token keeps the door it was born under, so the map
//! only has to change when the day does.
//!
//! This task is what changes it — once, shortly after 00:00 UTC — and nothing else
//! runs on the decision loop for it. The compute is one `GROUP BY` over one day of
//! `tokens` (never `trades`), off the loop, and the loop only ever swaps a map.
//!
//! A failed refresh keeps the previous day's map and logs at error level: the same
//! contract as an unprimed `prior_launches`, and the safe direction — a stale door
//! arms on yesterday's evidence, while an empty one would fail every door rule closed
//! without saying so.

use std::sync::Arc;
use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};

use super::EngineHandle;

/// How far past midnight to compute. The day's rows are read off `tokens` /
/// `tokens_info`, both of which are still being written for the closing day, so this
/// leaves the ingest flush a moment to land rather than racing it.
const REFRESH_AFTER_MIDNIGHT: ChronoDuration = ChronoDuration::seconds(30);

/// Retry cadence when a refresh fails — short enough that a transient DB blip does
/// not cost the whole day, long enough not to hammer a real outage.
const RETRY_DELAY: Duration = Duration::from_secs(60);

/// Refresh forever: at boot (so a restart mid-day is armed on today's door) and then
/// once per UTC day.
pub async fn run(engine: EngineHandle) {
    if let Err(e) = engine.reload_launch_build_stats().await {
        tracing::error!(
            "launch-build door UNLOADED at boot: {e} - every door rule fails closed \
             until the next refresh succeeds"
        );
    }
    loop {
        tokio::time::sleep(until_next_refresh()).await;
        while let Err(e) = engine.reload_launch_build_stats().await {
            tracing::error!(
                "launch-build door refresh failed: {e} - holding the previous day's map"
            );
            tokio::time::sleep(RETRY_DELAY).await;
        }
    }
}

/// Time from now until the next 00:00:30 UTC.
fn until_next_refresh() -> Duration {
    let now = Utc::now();
    let today = now.date_naive().and_hms_opt(0, 0, 0).expect("midnight exists").and_utc()
        + REFRESH_AFTER_MIDNIGHT;
    let target = if now < today { today } else { today + ChronoDuration::days(1) };
    (target - now).to_std().unwrap_or(Duration::from_secs(1))
}

/// Today's stats, as the engine event. Public so the boot path can fold one before
/// the first rule load rather than waiting on the task's first tick.
pub async fn load_today(
    repo: &trading_core::storage::repositories::launch_build_repo::LaunchBuildRepo,
) -> anyhow::Result<Arc<[hunter_engine::event::LaunchBuildStat]>> {
    use trading_core::storage::repositories::launch_build_repo::LaunchBuildRepo;
    let day = Utc::now().date_naive();
    let rows = repo.load_or_compute_day(day).await?;
    let n = rows.len();
    let stats: Arc<[hunter_engine::event::LaunchBuildStat]> =
        Arc::from(LaunchBuildRepo::to_engine(&rows));
    tracing::info!(%day, builds = n, stamped = stats.len(), "launch-build door loaded");
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wait is always in the future and never more than a day out - the property
    /// that keeps a boot at 23:59:59 from sleeping through the rollover it exists for.
    #[test]
    fn the_next_refresh_is_within_a_day() {
        let d = until_next_refresh();
        assert!(d.as_secs() <= 24 * 3600 + 30);
    }

    /// Midnight-relative, so the refresh lands at a fixed wall-clock instant rather
    /// than drifting with process start time.
    #[test]
    fn the_target_is_thirty_seconds_past_a_midnight() {
        use chrono::Timelike;
        let now = Utc::now();
        let target = now + ChronoDuration::from_std(until_next_refresh()).unwrap();
        assert_eq!(target.num_seconds_from_midnight() % 86_400, 30);
    }
}
