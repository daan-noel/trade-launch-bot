//! Per-`(wallet, mint)` wakeups for the live trade-confirm loops.
//!
//! The snipe buy (and sell) confirm path resolves its fill from the LaserStream-
//! fed `trades` table. Historically it re-read the DB on a fixed timer; this hub
//! lets it instead wake the instant a matching trade is *persisted*, while the
//! caller keeps its own timeout as a fallback. Net effect: same correctness and
//! same worst case, but the common case confirms as soon as the feed lands the
//! row instead of up to one poll interval later.
//!
//! Publish side — [`TradeSignals::notify`] is called by the `DbWriter` for every
//! trade it commits, *after* the row is queryable, so a woken waiter that reads
//! the DB is guaranteed to see it.
//!
//! Wait side — a waiter [`register`](TradeSignals::register)s its key (holding the
//! returned [`WaitGuard`] for the wait) and awaits [`WaitGuard::notified`]. The
//! map only ever holds keys with a live waiter (this bot's in-flight trades), so
//! its cardinality stays tiny and the publish-side miss is a single cheap lookup.
//!
//! Lookup is O(1): the hub is keyed two levels deep — `wallet -> (mint -> Slot)` —
//! and `DashMap<String, _>` borrows `&str` for lookups, so [`notify`](TradeSignals::notify)
//! resolves a committed trade's `(wallet, mint)` with two direct `get`s and no
//! per-trade `String` allocation, instead of a linear shard scan + key compare.

use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::Notify;

struct Slot {
    notify: Arc<Notify>,
    /// Number of live waiters on this key; the slot is removed when it hits 0.
    waiters: usize,
    /// Bumped on every [`notify`](TradeSignals::notify) for this key — i.e. once
    /// per trade the feed persists for this `(wallet, mint)`. A waiter samples it
    /// to tell "a new trade landed" from "a bare fallback tick", so the sell-
    /// confirm loop can skip its net-balance SQL aggregate when nothing changed.
    seq: u64,
}

/// Shared hub mapping an in-flight `(wallet, mint)` to its wakeup primitive.
/// Two-level so the publish-side lookup is `wallet -> mint -> Slot` (two direct
/// `get`s), not a scan: `notify` runs once per committed trade and must stay O(1).
#[derive(Default)]
pub struct TradeSignals {
    wallets: DashMap<String, DashMap<String, Slot>>,
}

impl TradeSignals {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register interest in `(wallet, mint)`. Hold the returned guard for the
    /// lifetime of the wait and await [`WaitGuard::notified`]; dropping it frees
    /// the slot.
    pub fn register(self: &Arc<Self>, wallet: &str, mint: &str) -> WaitGuard {
        let mints = self
            .wallets
            .entry(wallet.to_string())
            .or_insert_with(DashMap::new);
        let notify = {
            let mut slot = mints.entry(mint.to_string()).or_insert_with(|| Slot {
                notify: Arc::new(Notify::new()),
                waiters: 0,
                seq: 0,
            });
            slot.waiters += 1;
            slot.notify.clone()
        };
        WaitGuard {
            signals: self.clone(),
            wallet: wallet.to_string(),
            mint: mint.to_string(),
            notify,
        }
    }

    /// Wake every waiter on `(wallet, mint)`, if any. A no-op (one shard lookup,
    /// short-circuited by an `is_empty` check when nothing is waiting) for
    /// un-watched keys — i.e. for virtually every trade flowing through ingest.
    pub fn notify(&self, wallet: &str, mint: &str) {
        if self.wallets.is_empty() {
            return;
        }
        // Two direct, allocation-free lookups (`DashMap<String, _>` borrows
        // `&str`): the map only holds this bot's in-flight keys, so a committed
        // trade almost always misses on the first `get`.
        if let Some(mints) = self.wallets.get(wallet) {
            if let Some(mut slot) = mints.get_mut(mint) {
                // Mark that a new trade landed for this key, then wake waiters.
                slot.seq = slot.seq.wrapping_add(1);
                slot.notify.notify_waiters();
            }
        }
    }
}

/// RAII handle for one registered wait. Drop releases the slot.
pub struct WaitGuard {
    signals: Arc<TradeSignals>,
    wallet: String,
    mint: String,
    notify: Arc<Notify>,
}

impl WaitGuard {
    /// A future that resolves on the next signal for this key. Create it and call
    /// [`tokio::sync::futures::Notified::enable`] *before* the DB check so a notify
    /// arriving in the gap isn't lost (tokio `notify_waiters` stores no permit).
    pub fn notified(&self) -> tokio::sync::futures::Notified<'_> {
        self.notify.notified()
    }

    /// The current trade-sequence for this key — bumped once per persisted trade
    /// (see [`Slot::seq`]). A waiter that records this value and finds it
    /// unchanged on a later tick knows no new trade landed, so a re-query of the
    /// derived state (e.g. the net-balance SQL aggregate) would be wasted.
    pub fn seq(&self) -> u64 {
        self.signals
            .wallets
            .get(&self.wallet)
            .and_then(|mints| mints.get(&self.mint).map(|s| s.seq))
            .unwrap_or(0)
    }
}

impl Drop for WaitGuard {
    fn drop(&mut self) {
        use dashmap::mapref::entry::Entry;
        // Release this waiter's slot, then prune the now-empty mint and wallet
        // levels so the map's cardinality tracks live waiters only.
        let Entry::Occupied(mut wallet_entry) = self.signals.wallets.entry(self.wallet.clone())
        else {
            return;
        };
        let mints = wallet_entry.get_mut();
        if let Entry::Occupied(mut slot_entry) = mints.entry(self.mint.clone()) {
            slot_entry.get_mut().waiters -= 1;
            if slot_entry.get().waiters == 0 {
                slot_entry.remove();
            }
        }
        if mints.is_empty() {
            wallet_entry.remove();
        }
    }
}
