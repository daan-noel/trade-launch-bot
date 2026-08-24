//! Idle reaper — returns lab analysis caches to the OS once nothing is using them.
//!
//! **The problem it solves.** [`SweepCorpusCache`](super::local_state::SweepCorpusCache)
//! holds a whole loaded corpus (trades + fingerprints). It is written at the end of
//! every sweep / flow-discovery and, before this module, was only ever cleared by the
//! *next* load finding a different corpus hash. A lab that finished its last job kept
//! the corpus resident forever: on a 16GB workstation that is ~8GB of commit that
//! Windows pages out to disk and then pays to fault back.
//!
//! **Why this cannot slow a backtest.**
//! - It never runs while work is in flight: [`LocalState::is_idle`] gates every pass,
//!   covering both the heavy Duck jobs and in-flight simulations.
//! - The corpus is dropped only after [`CORPUS_IDLE_TTL`] with no read *or* under real
//!   host-memory pressure. A cache nobody has touched in that long is already in the
//!   pagefile, so re-reading it from Parquet is not slower than faulting it back in.
//! - [`AnalysisCache::gc`] only removes entries past the cache's own TTL, which a read
//!   already rejects — dead bytes, never a hit.
//!
//! A live box therefore keeps every cache that is doing work, and only sheds the ones
//! that are pure pagefile ballast.

use std::sync::Arc;
use std::time::Duration;

use super::local_state::LocalState;

/// How often to consider reaping. Cheap when idle (two atomic loads and a `len`),
/// and never runs concurrently with a job.
const SCAN_INTERVAL: Duration = Duration::from_secs(60);

/// Idle time after which the warm corpus stops earning its RAM. Longer than a coffee
/// break so drilling into combos right after a sweep — the case the cache exists for —
/// always hits; short enough that a lab left open overnight is not holding gigabytes.
const CORPUS_IDLE_TTL: Duration = Duration::from_secs(600);

/// Reap the corpus regardless of idle time once host availability drops below this.
/// At this point the corpus is being paged out anyway, so holding the slot buys
/// nothing and costs the rest of the box.
const PRESSURE_FLOOR_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Run the reaper until the process exits. Fire-and-forget from the composition root.
pub async fn run_idle_reaper(state: Arc<LocalState>) {
    tracing::info!(
        scan_interval_s = SCAN_INTERVAL.as_secs(),
        corpus_idle_ttl_s = CORPUS_IDLE_TTL.as_secs(),
        pressure_floor_mb = PRESSURE_FLOOR_BYTES / (1024 * 1024),
        "idle reaper: started"
    );
    let mut ticker = tokio::time::interval(SCAN_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        reap_once(&state).await;
    }
}

/// One pass. Split out so the gate order is readable: idle first, always.
async fn reap_once(state: &LocalState) {
    if !state.is_idle() {
        return;
    }

    // Free by construction — only entries a read would already reject.
    let dropped = state.analysis_cache.gc();
    if dropped > 0 {
        tracing::debug!(entries = dropped, "idle reaper: swept expired analysis cache");
    }

    // Host availability, read through the same helper the sweep sizes itself with.
    let available = crate::sweep::obs::host_memory_bytes().map(|(_, avail)| avail);
    let pressured = available.is_some_and(|a| a < PRESSURE_FLOOR_BYTES);

    // `read()` first: the common case is "nothing to do", and taking the write lock
    // unconditionally would serialise against a handler reading the cache.
    let due = {
        let cache = state.sweep_corpus_cache.read().await;
        match cache.as_ref() {
            None => false,
            Some(c) => c.idle() >= CORPUS_IDLE_TTL || pressured,
        }
    };
    if !due {
        return;
    }

    let mut cache = state.sweep_corpus_cache.write().await;
    // Re-check under the write lock: a handler may have taken the corpus between the
    // two acquisitions, which both refreshes `touched` and makes this stale.
    let Some(c) = cache.as_ref() else { return };
    if !(c.idle() >= CORPUS_IDLE_TTL || pressured) {
        return;
    }
    let (hash, tokens, idle_s) = (c.corpus_hash.clone(), c.tokens.len(), c.idle().as_secs());
    *cache = None;
    tracing::info!(
        corpus_hash = %hash,
        tokens,
        idle_s,
        pressured,
        available_mb = available.map(|a| a / (1024 * 1024)),
        "idle reaper: released warm corpus"
    );
}
