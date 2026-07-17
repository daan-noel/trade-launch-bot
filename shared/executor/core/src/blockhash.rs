// ============================================================
// Recent-blockhash cache.
//
// The AMM buy path uses a recent blockhash rather than a durable nonce (a
// nonce-advance would push the largest cashback buy past the 1232-byte tx limit
// — see `amm::tests` and `build_recent_tx`).
//
// Two writers feed it:
//   - a push feed (`store_pushed`) — the host bridges a `blocks_meta` stream
//     (LaserStream gRPC) into the cache, slot-gated so replayed/out-of-order
//     metas never regress the hash. Steady state: ~400 ms fresh, zero RPC.
//   - the engine's refresher task — demoted to a WATCHDOG: it only calls
//     `getLatestBlockhash` when the cache is older than its tick (feed absent
//     or stalled). A host with no push feed gets the old poll behavior.
//
// Reads are freshness-bounded and fall back to a live fetch when the cache is
// empty or stale (startup, or a stalled refresher), so a transaction is never
// built on an expired blockhash.
// ============================================================

use solana_sdk::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Latest recent blockhash + when it was stored. A plain `std::sync::Mutex`:
/// the critical section is a single read/write with no `.await` held.
#[derive(Default)]
pub struct BlockhashCache {
    inner: Mutex<Option<(Hash, Instant)>>,
    /// Highest slot seen on the push path — gates `store_pushed` so a replayed
    /// or out-of-order block meta can never overwrite a newer hash.
    pushed_slot: AtomicU64,
}

impl BlockhashCache {
    /// Store a freshly-fetched blockhash, stamping it with the current time.
    /// (RPC path — `getLatestBlockhash` always returns the tip, so no slot gate.)
    pub fn store(&self, hash: Hash) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = Some((hash, Instant::now()));
        }
    }

    /// Store a feed-pushed `(hash, slot)`; ignored unless `slot` is newer than
    /// every previously pushed slot.
    pub fn store_pushed(&self, hash: Hash, slot: u64) {
        if slot > self.pushed_slot.fetch_max(slot, Ordering::AcqRel) {
            self.store(hash);
        }
    }

    /// Return the cached blockhash if it was stored within `max_age`, else
    /// `None` (→ caller fetches a fresh one on-chain).
    pub fn get_fresh(&self, max_age: Duration) -> Option<Hash> {
        let guard = self.inner.lock().ok()?;
        match *guard {
            Some((hash, at)) if at.elapsed() <= max_age => Some(hash),
            _ => None,
        }
    }

    /// Whether the cache holds a hash stored within `max_age` — the refresher's
    /// "did the push feed already cover this tick?" check.
    pub fn is_fresher_than(&self, max_age: Duration) -> bool {
        self.get_fresh(max_age).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pushed_hash_is_slot_gated() {
        let c = BlockhashCache::default();
        let h1 = Hash::new_unique();
        let h2 = Hash::new_unique();
        let h3 = Hash::new_unique();

        c.store_pushed(h1, 100);
        assert_eq!(c.get_fresh(Duration::from_secs(5)), Some(h1));

        // Older/equal slot (reconnect replay) must not regress the hash.
        c.store_pushed(h2, 99);
        c.store_pushed(h2, 100);
        assert_eq!(c.get_fresh(Duration::from_secs(5)), Some(h1));

        // Newer slot wins.
        c.store_pushed(h3, 101);
        assert_eq!(c.get_fresh(Duration::from_secs(5)), Some(h3));
    }

    #[test]
    fn rpc_store_is_unconditional() {
        let c = BlockhashCache::default();
        c.store_pushed(Hash::new_unique(), 100);
        let fresh = Hash::new_unique();
        // The watchdog only fetches when the feed stalled — its result is by
        // definition at least as fresh as the last push, so no slot gate.
        c.store(fresh);
        assert_eq!(c.get_fresh(Duration::from_secs(5)), Some(fresh));
        assert!(c.is_fresher_than(Duration::from_secs(5)));
    }
}
