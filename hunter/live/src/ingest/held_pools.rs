//! Held-position AMM pool retention.
//!
//! `track_post_migration` gates whether *all* migrated pools stay on the LaserStream
//! subscription. Independently of that toggle, every mint with an unsettled **real**
//! position must keep its PumpSwap pool subscribed so:
//! - `observe_amm_swap_accounts` can zero-RPC warm `AmmPoolInfo` (avoiding the
//!   `getSignaturesForAddress` + `getTransaction` cold burst), and
//! - sell-confirm can see the bot's own AMM fill in `trades`.
//!
//! Without this, default settings + every restart silently reintroduce the old
//! AMM prewarm RPC waste on the live exit path.

use std::sync::Arc;

use dashmap::DashSet;
use ingest_pumpfun::IngestHandle;
use tokio::sync::watch;
use tracing::info;

use trading_core::storage::repositories::settings_repo::AppSettings;

/// Process-wide set of mints that must keep their AMM pool on the gRPC filter
/// even when the operator has `track_post_migration` off.
#[derive(Clone)]
pub struct HeldPoolGate {
    held: Arc<DashSet<String>>,
    ingest: Arc<IngestHandle>,
    settings: watch::Receiver<AppSettings>,
}

impl HeldPoolGate {
    pub fn new(ingest: Arc<IngestHandle>, settings: watch::Receiver<AppSettings>) -> Self {
        Self {
            held: Arc::new(DashSet::new()),
            ingest,
            settings,
        }
    }

    /// Record that `mint` has an unsettled real position. Does not subscribe
    /// until [`Self::track_migrated`] / a migration event keeps the pool.
    pub fn note(&self, mint: &str) {
        self.held.insert(mint.to_string());
    }

    /// Note + subscribe the mint's PumpSwap pool (call when the mint is known
    /// migrated, or at boot for held migrated mints).
    pub fn track_migrated(&self, mint: &str) {
        self.held.insert(mint.to_string());
        self.ingest.track_pools(&[mint.to_string()]);
    }

    /// Boot / batch variant of [`Self::track_migrated`].
    pub fn track_migrated_many(&self, mints: &[String]) {
        if mints.is_empty() {
            return;
        }
        for m in mints {
            self.held.insert(m.clone());
        }
        self.ingest.track_pools(mints);
        info!(
            n = mints.len(),
            "Held AMM pools: subscribed {} unsettled migrated mint(s)",
            mints.len()
        );
    }

    pub fn contains(&self, mint: &str) -> bool {
        self.held.contains(mint)
    }

    pub fn snapshot(&self) -> Vec<String> {
        self.held.iter().map(|e| e.key().clone()).collect()
    }

    /// Drop held retention for `mint`. Unsubscribes the pool only when the
    /// operator is not recording all post-migration AMM traffic.
    pub fn release(&self, mint: &str) {
        if self.held.remove(mint).is_none() {
            return;
        }
        if !self.settings.borrow().track_post_migration {
            self.ingest.untrack_pools(&[mint.to_string()]);
        }
    }
}
