//! Unified wallet-lifecycle tick (audit Phase C1).
//!
//! Folds four formerly-independent background loops that all revolve around the
//! `managed_wallets` lifecycle into ONE task with a shared wake-up:
//!
//! 1. **poll + promote** — RPC-refresh `generated`/`funding` (+ treasury) balances,
//!    promoting to `funded` when SOL lands (drives the adaptive base cadence);
//! 2. **sweep** — reservation + funding TTL sweep of stranded claims (30s);
//! 3. **top-up** — warm-pool funding pass (60s, only when funding is configured);
//! 4. **dust** — hourly sweep of `used` wallets home to the treasury.
//!
//! Previously these ran as four separate `tokio::select!` arms on overlapping
//! fixed cadences — two of them (balance poll + reservation sweep) hit the same
//! table every 30s with no coordination. Collapsing them into a single
//! poll→promote→sweep→top-up→dust tick removes that redundancy; each step keeps
//! its own cadence via a wall-clock gate, so behaviour per step is unchanged —
//! only the scheduling is shared. The two RPC-heavy launcher tasks that are NOT
//! wallet-lifecycle (ladder evaluator, volume scheduler — trading) stay separate.

use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tracing::warn;

use crate::config::LauncherSettings;
use crate::dust_sweep::{run_dust_sweep_pass, DUST_SWEEP_INTERVAL};
use crate::events::EventSink;
use crate::wallet_funding::{run_background_funding_pass, FUND_PASS_INTERVAL};
use crate::wallet_pool::{
    balance_poll_client, poll_balances_once, sweep_reservations_once, BALANCE_POLL_ACTIVE,
    BALANCE_POLL_IDLE, RESERVATION_SWEEP,
};

/// Whether a cadence-gated step is due. `None` (never run) is always due, so each
/// slow step fires on the first tick — matching the old `tokio::interval` tasks,
/// which each ran an immediate first pass.
fn due(last: Option<Instant>, interval: Duration, now: Instant) -> bool {
    last.map_or(true, |t| now.duration_since(t) >= interval)
}

/// Spawn the unified wallet-lifecycle task. Gated at the call site on
/// `launcher_settings.is_some()`; the funding step self-skips when funding isn't
/// configured (`settings.funding.is_none()`).
pub fn spawn_wallet_lifecycle(
    pool: PgPool,
    settings: LauncherSettings,
    sink: Option<Arc<dyn EventSink>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let client = balance_poll_client();
        let funding_enabled = settings.funding.is_some();
        let mut last_reservation: Option<Instant> = None;
        let mut last_funding: Option<Instant> = None;
        let mut last_dust: Option<Instant> = None;

        loop {
            // 1) Balance poll + promote — drives the adaptive base cadence below.
            let pending = match poll_balances_once(&pool, &client, &settings.rpc_url).await {
                Ok(n) => n,
                Err(e) => {
                    warn!(?e, "wallet balance poll failed");
                    0
                }
            };

            let now = Instant::now();
            // 2) Reservation + funding TTL sweep (unchanged 30s cadence).
            if due(last_reservation, RESERVATION_SWEEP, now) {
                sweep_reservations_once(&pool).await;
                last_reservation = Some(now);
            }
            // 3) Warm-pool funding top-up (60s) — only when funding is configured.
            if funding_enabled && due(last_funding, FUND_PASS_INTERVAL, now) {
                run_background_funding_pass(&pool, &settings, sink.as_deref()).await;
                last_funding = Some(now);
            }
            // 4) Hourly dust sweep of `used` wallets → treasury.
            if due(last_dust, DUST_SWEEP_INTERVAL, now) {
                run_dust_sweep_pass(&pool, &settings).await;
                last_dust = Some(now);
            }

            // Adaptive sleep: fast while wallets are mid-funding, idle otherwise —
            // the same cadence the standalone balance poller used.
            let wait = if pending > 0 { BALANCE_POLL_ACTIVE } else { BALANCE_POLL_IDLE };
            tokio::time::sleep(wait).await;
        }
    })
}
