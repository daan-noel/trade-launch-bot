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

use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::Notify;

type Key = (String, String);

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
#[derive(Default)]
pub struct TradeSignals {
    slots: DashMap<Key, Slot>,
}

impl TradeSignals {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register interest in `(wallet, mint)`. Hold the returned guard for the
    /// lifetime of the wait and await [`WaitGuard::notified`]; dropping it frees
    /// the slot.
    pub fn register(self: &Arc<Self>, wallet: &str, mint: &str) -> WaitGuard {
        let key = (wallet.to_string(), mint.to_string());
        let notify = {
            let mut slot = self.slots.entry(key.clone()).or_insert_with(|| Slot {
                notify: Arc::new(Notify::new()),
                waiters: 0,
                seq: 0,
            });
            slot.waiters += 1;
            slot.notify.clone()
        };
        WaitGuard {
            signals: self.clone(),
            key,
            notify,
        }
    }

    /// Wake every waiter on `(wallet, mint)`, if any. A no-op (one map lookup, and
    /// a cheap `is_empty` short-circuit when nothing is waiting) for un-watched
    /// keys — i.e. for virtually every trade flowing through ingest.
    pub fn notify(&self, wallet: &str, mint: &str) {
        if self.slots.is_empty() {
            return;
        }
        // The map only holds this bot's in-flight (wallet, mint) keys — at most a
        // handful at once — so compare borrowed keys in a short scan instead of
        // allocating two owned `String`s per committed trade for a `get` that
        // almost always misses. (`(String, String)` can't be looked up by
        // `(&str, &str)` without a Borrow impl, hence the scan.)
        for mut entry in self.slots.iter_mut() {
            let (key, slot) = entry.pair_mut();
            if key.0 == wallet && key.1 == mint {
                // Mark that a new trade landed for this key, then wake waiters.
                slot.seq = slot.seq.wrapping_add(1);
                slot.notify.notify_waiters();
                return;
            }
        }
    }
}

/// RAII handle for one registered wait. Drop releases the slot.
pub struct WaitGuard {
    signals: Arc<TradeSignals>,
    key: Key,
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
        self.signals.slots.get(&self.key).map(|s| s.seq).unwrap_or(0)
    }
}

impl Drop for WaitGuard {
    fn drop(&mut self) {
        use dashmap::mapref::entry::Entry;
        if let Entry::Occupied(mut e) = self.signals.slots.entry(self.key.clone()) {
            e.get_mut().waiters -= 1;
            if e.get().waiters == 0 {
                e.remove();
            }
        }
    }
}
