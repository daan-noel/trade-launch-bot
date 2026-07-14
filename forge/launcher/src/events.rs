//! Push-event seam (audit Phase A: poll → push).
//!
//! The launcher's background workers — the bundle-confirm watcher
//! ([`crate::confirm`]) and the wallet funder ([`crate::wallet_funding`]) — advance
//! state the frontend would otherwise POLL for (launch/bundle status transitions,
//! funding progress). They can't reach the live bin's SSE layer directly (that
//! `SseHub` lives in `forge-live`, and the dep only runs one way — the bin depends
//! on this crate, not the reverse), so they publish through this trait and
//! `forge-live` provides the SSE-backed impl.
//!
//! Every emit is best-effort: an `Option<Arc<dyn EventSink>>` at each call site is
//! `None` on the CLI paths and in tests, where publishing is simply skipped.

use uuid::Uuid;

/// A launch (and, when known, its bundle) reached a new status. Emitted from the
/// confirm watcher's terminal transitions so the Launch Console reflects
/// created/failed/partial without polling `/api/launches/{id}/status`.
#[derive(Debug, Clone)]
pub struct LaunchStatusEvent {
    pub launch_id: Uuid,
    pub mint_address: String,
    pub launch_status: String,
    pub bundle_id: Option<Uuid>,
    pub bundle_status: Option<String>,
}

/// Sink for launcher-side push events, implemented by the live bin over its
/// `SseHub`. Object-safe so call sites hold an `Option<Arc<dyn EventSink>>`.
pub trait EventSink: Send + Sync {
    /// A launch/bundle status transition (confirm watcher terminal outcome).
    fn launch_status(&self, ev: &LaunchStatusEvent);

    /// The managed-wallet pool changed — a funding pass moved SOL / promoted
    /// wallets. A coarse signal: the Wallet Pool page refetches `GET
    /// /api/wallet_pool` off it rather than blind-polling every few seconds.
    fn wallet_pool_changed(&self);
}
