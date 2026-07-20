//! Cashback-status read service — a short-TTL cache in front of the two-pot
//! `getAccountInfo` read (`PumpFunTrader::cashback_status`). The dashboard polls
//! `GET /api/cashback/status`, but cashback accrues slowly, so a warm read
//! (< [`CASHBACK_TTL`]) is served without touching the chain. A successful claim
//! busts the cache so the post-claim display is immediately correct.

use std::sync::Arc;
use std::time::{Duration, Instant};

use pump_trader::PotStatus;
use tokio::sync::Mutex;

use crate::state::deploy_state::DeployState;

/// How long a cashback read stays warm. Cashback earns slowly (it trails traded
/// volume), so a stale figure for up to this long is harmless — and a claim busts
/// it immediately, so the only staleness is passive accrual, never a claim.
const CASHBACK_TTL: Duration = Duration::from_secs(45);

/// Short-TTL cache of the two cashback pots (see [`CASHBACK_TTL`]). Held on
/// [`DeployState`]; one bot wallet per process, so a single slot suffices.
/// `Default` = empty (cold). `PotStatus` is `Copy`, so the vec clones cheaply.
#[derive(Default)]
pub struct CashbackCache {
    inner: Mutex<Option<(Instant, Arc<Vec<PotStatus>>)>>,
}

impl CashbackCache {
    async fn get_fresh(&self) -> Option<Arc<Vec<PotStatus>>> {
        match self.inner.lock().await.as_ref() {
            Some((at, v)) if at.elapsed() < CASHBACK_TTL => Some(v.clone()),
            _ => None,
        }
    }

    async fn put(&self, v: Arc<Vec<PotStatus>>) {
        *self.inner.lock().await = Some((Instant::now(), v));
    }

    /// Drop the cached read so the next status call re-reads on-chain. Called
    /// after a successful claim (the balances just changed).
    pub async fn invalidate(&self) {
        *self.inner.lock().await = None;
    }
}

/// Cache-first cashback status: reuse a warm read (< [`CASHBACK_TTL`]) or do the
/// two-pot `getAccountInfo` read and warm the cache.
pub async fn status_cached(state: &DeployState) -> anyhow::Result<Arc<Vec<PotStatus>>> {
    if let Some(v) = state.cashback_cache.get_fresh().await {
        return Ok(v);
    }
    let fresh = Arc::new(
        state
            .trader
            .cashback_status()
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?,
    );
    state.cashback_cache.put(fresh.clone()).await;
    Ok(fresh)
}
