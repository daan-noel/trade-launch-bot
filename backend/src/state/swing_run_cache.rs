//! Server-side cache of raw swing legs from "Swing Detection All" runs.
//!
//! The Swing Detection page runs detection across the whole filtered token set in
//! chunked `POST /api/tokens/swings/batch` calls. To let the server-side-paged
//! tokens list **sort** by the browser-derived chain columns (Swing Pairs / Max
//! Seq Pairs / Chains), the raw legs each run produces are stashed here keyed by a
//! client-generated run id. The tokens-list handler then recomputes chain stats
//! from these legs at the requested chain latency and orders by them — so changing
//! the latency re-sorts instantly without re-running detection.
//!
//! Bounded to the few most recent runs (older ones evicted) so the cache can't
//! grow without limit across re-runs / multiple tabs. Each run holds at most one
//! filtered page's worth of mints (≤ the list ceiling), a handful of small legs
//! each — a few MB per run at the extreme.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use dashmap::mapref::entry::Entry;
use dashmap::DashMap;

use crate::analyzers::swing_analyzer::SwingLeg;

/// One run's raw swings, indexed by mint. The inner `DashMap` lets the concurrent
/// batch chunks of a single run insert their mints without contending on a lock.
pub struct SwingRun {
    pub mints: DashMap<String, Vec<SwingLeg>>,
    /// Creation instant, used only for eviction ordering.
    created_at: Instant,
}

impl SwingRun {
    fn new() -> Self {
        Self {
            mints: DashMap::new(),
            created_at: Instant::now(),
        }
    }
}

/// Capacity-bounded store of recent swing runs (keyed by client run id).
pub struct SwingRunCache {
    runs: DashMap<String, Arc<SwingRun>>,
    /// Insertion order of live run ids, oldest at the front — drives eviction.
    order: Mutex<VecDeque<String>>,
    cap: usize,
}

impl SwingRunCache {
    pub fn new(cap: usize) -> Self {
        Self {
            runs: DashMap::new(),
            order: Mutex::new(VecDeque::new()),
            cap: cap.max(1),
        }
    }

    /// Fetch the run for `run_id`, creating an empty one if absent. Newly-created
    /// runs are tracked for eviction; once the cache exceeds `cap`, the oldest
    /// run(s) are dropped. The `entry` insert is atomic per shard, so concurrent
    /// first chunks of the same run share one `SwingRun` (no lost chunk).
    pub fn get_or_create(&self, run_id: &str) -> Arc<SwingRun> {
        // Resolve/insert under the entry guard, then DROP it before touching the
        // map again (eviction's `remove`) — holding an entry across another map op
        // on the same shard would deadlock.
        let (run, created) = match self.runs.entry(run_id.to_string()) {
            Entry::Occupied(e) => (e.get().clone(), false),
            Entry::Vacant(e) => {
                let run = Arc::new(SwingRun::new());
                e.insert(run.clone());
                (run, true)
            }
        };
        if created {
            let mut order = self.order.lock().unwrap();
            order.push_back(run_id.to_string());
            while order.len() > self.cap {
                if let Some(old) = order.pop_front() {
                    self.runs.remove(&old);
                }
            }
        }
        run
    }

    /// Look up an existing run without creating one.
    pub fn get(&self, run_id: &str) -> Option<Arc<SwingRun>> {
        self.runs.get(run_id).map(|r| r.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_oldest_beyond_capacity() {
        let cache = SwingRunCache::new(2);
        let a = cache.get_or_create("a");
        a.mints.insert("m1".into(), Vec::new());
        cache.get_or_create("b");
        // Re-fetching an existing run returns the same instance (data preserved),
        // and does not re-track it for eviction.
        assert!(cache.get("a").is_some());
        assert_eq!(cache.get_or_create("a").mints.len(), 1, "existing run preserved");

        // A third distinct run pushes the cache past cap(2), evicting the oldest
        // *by first insertion* ("a"). The newest run is always retained — that's
        // the one an in-flight sort references.
        cache.get_or_create("c");
        assert!(cache.get("a").is_none(), "oldest run evicted");
        assert!(cache.get("b").is_some());
        assert!(cache.get("c").is_some());
    }
}
