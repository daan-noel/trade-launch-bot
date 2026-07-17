//! Recovery reaper (plan 4.7) — the safety backstop for `strategy_positions` rows
//! the engine loop can't resolve on its own: a sell task that died leaving a row
//! stuck in `ExitPending`, or a buy whose outcome stayed ambiguous leaving a row
//! in `BuySubmitted`/`Arming` that never entered.
//!
//! The executor adapters already resolve the *common* cases to definitive
//! `FillConfirmed`/`FillFailed` events (feed-confirm + RPC watchdog), so this timer
//! only ever touches rows that have been stranded far longer than a normal
//! submit/confirm cycle. It operates on PG directly (the durable source of truth);
//! the engine's in-memory arm for such a stranded position is already terminal or
//! abandoned, and a rule reload / restart re-syncs the counters.
//!
//! NOTE (vs. the plan's "emit `ManualClose`/`FillFailed` events" wording): the
//! generic engine's opaque intents aren't reconstructible from a bare PG row, so a
//! reaper can't mint the matching event for an engine-tracked position. This
//! backstop instead settles the *durable* row directly, well past the point the
//! engine could still be acting on it — preserving the safety property (no stranded
//! Holding / phantom re-sell) without the event round-trip.

use std::time::Duration;

use tokio::task::JoinHandle;
use tracing::{info, warn};

use trading_core::storage::repositories::strategy_repo::StrategyRepo;

/// How often the reaper sweeps.
const INTERVAL: Duration = Duration::from_secs(60);
/// A sell stuck in `ExitPending` this long has lost its executor task — book it
/// failed (far longer than the executor's own sell window).
const EXIT_PENDING_STALE: Duration = Duration::from_secs(300);
/// A `BuySubmitted`/`Arming` row that never entered in this long is abandoned.
const UNENTERED_STALE: Duration = Duration::from_secs(600);

/// Spawn the reaper loop (first tick after `INTERVAL`, then every `INTERVAL`).
pub fn spawn_reaper(repo: StrategyRepo) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        tick.tick().await; // consume the immediate first tick — no boot-time sweep
        loop {
            tick.tick().await;
            for mode in ["real", "paper"] {
                match repo.fail_stale_exit_pending(mode, EXIT_PENDING_STALE).await {
                    Ok(n) if n > 0 => info!(mode, n, "reaper: failed stale ExitPending rows"),
                    Ok(_) => {}
                    Err(e) => warn!(mode, "reaper: fail_stale_exit_pending: {e}"),
                }
                match repo.delete_stale_unentered(mode, UNENTERED_STALE).await {
                    Ok(n) if n > 0 => info!(mode, n, "reaper: deleted stale unentered rows"),
                    Ok(_) => {}
                    Err(e) => warn!(mode, "reaper: delete_stale_unentered: {e}"),
                }
            }
        }
    })
}
