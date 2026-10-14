//! Cashback-status read service — a short-TTL cache in front of the two-pot
//! `getAccountInfo` read (`PumpFunTrader::cashback_status`). The dashboard polls
//! `GET /api/cashback/status`, but cashback accrues slowly, so a warm read
//! (< [`CASHBACK_TTL`]) is served without touching the chain. A successful claim
//! busts the cache so the post-claim display is immediately correct.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use pump_trader::PotStatus;
use tokio::sync::Mutex;

use crate::state::deploy_state::DeployState;
use crate::trader::PumpFunTrader;

/// How long a cashback read stays warm. Cashback earns slowly (it trails traded
/// volume), so a stale figure for up to this long is harmless — and a claim busts
/// it immediately, so the only staleness is passive accrual, never a claim.
const CASHBACK_TTL: Duration = Duration::from_secs(45);

/// Short-TTL cache of the two cashback pots (see [`CASHBACK_TTL`]), one slot per
/// wallet pubkey. Held on [`DeployState`]. `Default` = empty (cold). `PotStatus`
/// is `Copy`, so the vec clones cheaply.
type CashbackSlot = (Instant, Arc<Vec<PotStatus>>);

#[derive(Default)]
pub struct CashbackCache {
    inner: Mutex<HashMap<String, CashbackSlot>>,
}

impl CashbackCache {
    async fn get_fresh(&self, wallet: &str) -> Option<Arc<Vec<PotStatus>>> {
        match self.inner.lock().await.get(wallet) {
            Some((at, v)) if at.elapsed() < CASHBACK_TTL => Some(v.clone()),
            _ => None,
        }
    }

    async fn put(&self, wallet: &str, v: Arc<Vec<PotStatus>>) {
        self.inner.lock().await.insert(wallet.to_string(), (Instant::now(), v));
    }

    /// Drop the cached reads so the next status call re-reads on-chain. Called
    /// after a successful claim (the balances just changed).
    pub async fn invalidate(&self) {
        self.inner.lock().await.clear();
    }
}

/// Cache-first cashback status for `trader`'s wallet: reuse a warm read
/// (< [`CASHBACK_TTL`]) or do the two-pot `getAccountInfo` read and warm the cache.
pub async fn status_cached(
    state: &DeployState,
    trader: &PumpFunTrader,
) -> anyhow::Result<Arc<Vec<PotStatus>>> {
    let wallet = trader.wallet_pubkey();
    if let Some(v) = state.cashback_cache.get_fresh(&wallet).await {
        return Ok(v);
    }
    let fresh = Arc::new(
        trader
            .cashback_status()
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?,
    );
    state.cashback_cache.put(&wallet, fresh.clone()).await;
    Ok(fresh)
}
