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

/// Progress of a long-running manage action (a per-wallet "sell all", a sweep, a
/// consolidate). Emitted at start (`running`, `done = 0`), after each leg/wallet
/// (`running`, `done` bumped), and once at the terminal outcome (`done` | `partial`
/// | `failed`) — so the operator sees live "N of M" status on the triggering surface
/// instead of a frozen spinner. `action_id` ties every frame of one action together;
/// `mint_address` scopes sell actions to their token (a `None` scope is broadcast to
/// every subscriber, for wallet-wide actions like sweep/consolidate).
#[derive(Debug, Clone)]
pub struct ActionProgressEvent {
    pub action_id: Uuid,
    pub mint_address: Option<String>,
    /// Action family: `"sell"`, `"consolidate"`, `"sweep"`, … Drives the label tone.
    pub kind: String,
    /// One of `running` | `partial` | `done` | `failed`.
    pub status: String,
    pub done: usize,
    pub total: usize,
    /// Set on `partial`/`failed` (a short human message).
    pub error: Option<String>,
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

    /// A long-running manage action advanced (start / per-leg / terminal). See
    /// [`ActionProgressEvent`]. Best-effort like the rest of the sink.
    fn action_progress(&self, ev: &ActionProgressEvent);
}
