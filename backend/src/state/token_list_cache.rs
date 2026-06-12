use std::sync::{Arc, Mutex, RwLock};

use chrono::{DateTime, Utc};

use crate::api::handlers::tokens::TokenSummary;

use super::token_cache::TokenCache;

/// Max age of the shared token-list snapshot before the next reader rebuilds it.
/// The Tokens table polls every 5s (plus an SSE-triggered refetch on new tokens),
/// so a 1s ceiling is invisible to the UI while collapsing every concurrent
/// request within the window onto a single rebuild.
const MAX_SNAPSHOT_STALENESS_MS: i64 = 1_000;

/// Immutable, pre-sorted token-list snapshot shared by every `GET /api/tokens`
/// request. Rows are ordered newest-first by `created_at`, so the default
/// (unsorted) table view needs no per-request sort.
pub struct TokenListSnapshot {
    /// All tracked tokens as compact list rows, newest-first by `created_at`.
    pub rows: Vec<TokenSummary>,
    /// Wall-clock time the snapshot was materialised (drives the staleness check).
    pub built_at: DateTime<Utc>,
}

impl TokenListSnapshot {
    /// Clone the live cache into list rows once and pre-sort newest-first. This is
    /// the single expensive step the old per-request handler ran on every poll of
    /// every client; here it runs at most once per staleness window.
    fn build(cache: &TokenCache, now: DateTime<Utc>) -> Self {
        let mut rows: Vec<TokenSummary> =
            cache.iter().map(|e| TokenSummary::from(e.value())).collect();
        // Newest-first: the table's default order and the basis for the
        // "no re-sort needed" fast path in the list handler.
        rows.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Self { rows, built_at: now }
    }
}

/// Shared, staleness-bounded snapshot of the token list.
///
/// Replaces the old per-request, per-client full-cache clone: each request reads
/// the current `Arc<TokenListSnapshot>` for free; only when it has aged past
/// `MAX_SNAPSHOT_STALENESS_MS` does one reader rebuild it (others block on the
/// rebuild lock and then pick up the fresh result). Total cost is therefore one
/// cache clone per staleness window regardless of how many clients are polling,
/// instead of `clients × cache_size` clones every poll.
pub struct TokenListCache {
    current: RwLock<Arc<TokenListSnapshot>>,
    /// Serialises rebuilds so a burst of stale readers triggers exactly one.
    rebuild_lock: Mutex<()>,
}

impl TokenListCache {
    /// Build an initial snapshot from the (possibly DB-seeded) cache so the first
    /// request is served without a rebuild stall.
    pub fn new(cache: &TokenCache) -> Self {
        let snap = Arc::new(TokenListSnapshot::build(cache, Utc::now()));
        Self {
            current: RwLock::new(snap),
            rebuild_lock: Mutex::new(()),
        }
    }

    /// Return a snapshot no older than `MAX_SNAPSHOT_STALENESS_MS`, rebuilding from
    /// `cache` if the current one has aged out. A rebuild is CPU-heavy (clones the
    /// whole cache), so call this from a blocking context (`web::block`).
    pub fn get(&self, cache: &TokenCache, now: DateTime<Utc>) -> Arc<TokenListSnapshot> {
        if let Some(fresh) = self.fresh(now) {
            return fresh;
        }
        // Stale: one reader rebuilds under the lock; concurrent readers block here
        // and then pick up the rebuilt snapshot via the re-check below (so a burst
        // of polls collapses to a single rebuild).
        let _guard = self.rebuild_lock.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(fresh) = self.fresh(now) {
            return fresh;
        }
        let rebuilt = Arc::new(TokenListSnapshot::build(cache, now));
        *self.current.write().unwrap_or_else(|e| e.into_inner()) = rebuilt.clone();
        rebuilt
    }

    /// The current snapshot if still within the staleness window, else `None`.
    fn fresh(&self, now: DateTime<Utc>) -> Option<Arc<TokenListSnapshot>> {
        let cur = self
            .current
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let age = now.signed_duration_since(cur.built_at).num_milliseconds();
        if age < MAX_SNAPSHOT_STALENESS_MS {
            Some(cur)
        } else {
            None
        }
    }
}
