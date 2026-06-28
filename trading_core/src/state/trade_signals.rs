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
///
/// A second, mint-only lane (`mints`) wakes watchers that follow the *whole mint*
/// rather than one wallet — the TPSL2 scalp-entry arming, which reads the in-
/// memory token cache for the first trade where its gates hold. It's published by
/// the ingest pipeline ([`notify_mint`](TradeSignals::notify_mint)) the instant a
/// trade is appended to the cache, so a woken waiter sees it. Same `is_empty`
/// short-circuit, so a trade for an un-watched mint is a single cheap check.
#[derive(Default)]
pub struct TradeSignals {
    wallets: DashMap<String, DashMap<String, Slot>>,
    mints: DashMap<String, Slot>,
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

    /// Register interest in *any* trade for `mint` (the mint-only lane). Hold the
    /// returned guard for the wait and await [`MintWaitGuard::notified`].
    pub fn register_mint(self: &Arc<Self>, mint: &str) -> MintWaitGuard {
        let notify = {
            let mut slot = self.mints.entry(mint.to_string()).or_insert_with(|| Slot {
                notify: Arc::new(Notify::new()),
                waiters: 0,
                seq: 0,
            });
            slot.waiters += 1;
            slot.notify.clone()
        };
        MintWaitGuard {
            signals: self.clone(),
            mint: mint.to_string(),
            notify,
        }
    }

    /// Wake every waiter following `mint` (any wallet), if any. A no-op (one
    /// `is_empty` check) for un-watched mints — i.e. for virtually every trade
    /// flowing through ingest. The mint-lane waiter detects new trades via the
    /// token cache's `trade_count`, so there's no `seq` to bump here.
    pub fn notify_mint(&self, mint: &str) {
        if self.mints.is_empty() {
            return;
        }
        if let Some(slot) = self.mints.get(mint) {
            slot.notify.notify_waiters();
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

/// RAII handle for one mint-lane wait (wakes on any wallet's trade for the mint).
/// Drop releases the slot, pruning it when no waiter remains.
pub struct MintWaitGuard {
    signals: Arc<TradeSignals>,
    mint: String,
    notify: Arc<Notify>,
}

impl MintWaitGuard {
    /// A future that resolves on the next trade for this mint. Create it and call
    /// [`tokio::sync::futures::Notified::enable`] *before* the cache check so a
    /// notify arriving in the gap isn't lost (`notify_waiters` stores no permit).
    pub fn notified(&self) -> tokio::sync::futures::Notified<'_> {
        self.notify.notified()
    }
}

impl Drop for MintWaitGuard {
    fn drop(&mut self) {
        use dashmap::mapref::entry::Entry;
        if let Entry::Occupied(mut slot_entry) = self.signals.mints.entry(self.mint.clone()) {
            slot_entry.get_mut().waiters -= 1;
            if slot_entry.get().waiters == 0 {
                slot_entry.remove();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A waiter armed (enabled) before the publish wakes on `notify_mint`, and the
    /// slot is pruned once the only guard drops — the exact pattern the TPSL2
    /// scalp-entry arming relies on (enable → check cache → publish wakes it).
    #[tokio::test]
    async fn mint_lane_wakes_waiter_and_prunes_on_drop() {
        let signals = Arc::new(TradeSignals::new());
        let guard = signals.register_mint("MINT");

        {
            let notified = guard.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            // Publish for the same mint (any wallet) — the enabled waiter wakes.
            signals.notify_mint("MINT");
            tokio::time::timeout(Duration::from_secs(1), notified.as_mut())
                .await
                .expect("mint waiter should wake on notify_mint");
        }

        // Dropping the only waiter prunes the slot, so the map tracks live waiters.
        drop(guard);
        assert!(
            signals.mints.is_empty(),
            "slot should be pruned when no waiter remains"
        );
    }

    /// Publishing for a mint nobody is watching is a no-op (the `is_empty`
    /// short-circuit that keeps it cheap on the per-trade hot path).
    #[tokio::test]
    async fn notify_mint_for_unwatched_mint_is_noop() {
        let signals = Arc::new(TradeSignals::new());
        signals.notify_mint("NOBODY");
        assert!(signals.mints.is_empty());
    }
}
