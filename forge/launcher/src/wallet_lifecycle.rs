//! Reservation-TTL sweep — the sole surviving wallet-lifecycle background loop.
//!
//! All fund/balance work is now **operator-triggered** (the "Fund pool" and
//! "Refresh balances" buttons). The former automatic loops — the balance poll,
//! the warm-pool auto-funder, and the hourly dust sweep — were removed so the box
//! makes **zero idle Helius RPC calls**. What remains is one DB-only safety timer:
//! it releases wallets stranded `reserved` (an aborted/crashed launch) or `funding`
//! (a treasury send that never landed) past their TTL, returning them to the
//! claimable pool. No RPC, no SOL movement.

use std::time::Duration;

use sqlx::PgPool;
use tokio::task::JoinHandle;

use crate::wallet_pool::sweep_reservations_once;

/// How often the DB-only reservation/funding TTL sweep runs. The TTL itself is 15
/// minutes (`wallet_pool::RESERVATION_TTL`); a 60s cadence reclaims a stranded
/// wallet promptly at no on-chain cost.
const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

/// Spawn the reservation-TTL sweep. Gated at the call site on
/// `launcher_settings.is_some()` (no launcher config ⇒ no managed wallets to
/// sweep). Pure DB work: `release_expired_reservations` + `revert_stale_funding`
/// over the tiny `reserved`/`funding` sets (partial-indexed in migration `0004`).
pub fn spawn_wallet_lifecycle(pool: PgPool) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(SWEEP_INTERVAL);
        loop {
            tick.tick().await;
            sweep_reservations_once(&pool).await;
        }
    })
}
