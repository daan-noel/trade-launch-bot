// ============================================================
// Recent-blockhash cache.
//
// The AMM buy path uses a recent blockhash rather than a durable nonce (a
// nonce-advance would push the largest cashback buy past the 1232-byte tx limit
// — see `amm::tests` and `build_recent_tx`). To avoid a `getLatestBlockhash` RPC
// on every buy, a background task refreshes this cache every few seconds and the
// trade path reads the hot value.
//
// Reads are freshness-bounded and fall back to a live fetch when the cache is
// empty or stale (startup, or a stalled refresher), so a transaction is never
// built on an expired blockhash.
// ============================================================

use solana_sdk::hash::Hash;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Latest recent blockhash + when it was fetched. A plain `std::sync::Mutex`:
/// the critical section is a single read/write with no `.await` held.
#[derive(Default)]
pub struct BlockhashCache {
    inner: Mutex<Option<(Hash, Instant)>>,
}

impl BlockhashCache {
    /// Store a freshly-fetched blockhash, stamping it with the current time.
    pub fn store(&self, hash: Hash) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = Some((hash, Instant::now()));
        }
    }

    /// Return the cached blockhash if it was fetched within `max_age`, else
    /// `None` (→ caller fetches a fresh one on-chain).
    pub fn get_fresh(&self, max_age: Duration) -> Option<Hash> {
        let guard = self.inner.lock().ok()?;
        match *guard {
            Some((hash, at)) if at.elapsed() <= max_age => Some(hash),
            _ => None,
        }
    }
}
