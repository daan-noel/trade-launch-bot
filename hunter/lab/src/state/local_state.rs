use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use dashmap::DashMap;
use uuid::Uuid;
use tokio::sync::{RwLock, Semaphore};

use super::core_state::CoreState;
use super::job_progress::ProgressCell;
use super::analysis_cache::AnalysisCache;
use super::sim_results::SimResults;
use super::sim_summary::SimSummaryCache;
use crate::strategies::flow_discovery::DiscoveryResult;
use crate::sweep::corpus::CorpusToken;

/// Option A: the fully-loaded corpus (trades + fingerprints) from the most recent
/// sweep run, keyed by its corpus hash. Lets `list_token_results` skip both the
/// Parquet read and the `attach_fingerprints` DB queries on the warm path — the
/// common case where a user drills into a combo right after running a sweep.
pub struct SweepCorpusCache {
    pub corpus_hash: String,
    /// All tokens with fingerprints already attached, as `Arc<Vec<_>>` so
    /// cloning into the handler is a refcount bump (no copy of the trade data).
    pub tokens: Arc<Vec<CorpusToken>>,
    /// See [`crate::sweep::corpus::Corpus::candidates_capped`].
    pub candidates_capped: bool,
}

/// Max concurrent backtests across both strategies. Each one streams the `tokens`
/// table and batched trade history off the `batch` pool (16 connections by
/// default), so unbounded overlap drains it. Raised from 2 to 4 now that the
/// `analysis_cache` single-flights the candidate scan + lake load: a "Simulate All"
/// batch of same-fingerprint rules no longer issues one whole-table scan per
/// concurrent rule (they collapse onto one shared scan), so the extra slots turn
/// into real parallelism — distinct-fingerprint scans and the per-rule exit walk —
/// rather than duplicated pool pressure. 4 keeps well under the 16-conn pool and
/// leaves headroom for dashboard reads / a running grouped sweep.
const MAX_CONCURRENT_BACKTESTS: usize = 4;

/// Analysis (local) state: the shared [`CoreState`] plus the handles only the
/// backtest/sweep process needs — sweep + sim gates/progress/results, and the
/// warm sweep corpus cache. `Deref`s to
/// `CoreState` so local handlers reach core fields/accessors transparently.
pub struct LocalState {
    pub core: Arc<CoreState>,
    /// Single-flight gate for the CPU-heavy grouped sweep
    /// (`POST /api/strategies/sweeps`): while one is running the handler returns
    /// 409, so a sweep can never pile more rayon work onto the process while one
    /// is already in flight.
    pub sweep_running: Arc<AtomicBool>,
    /// Cooperative cancel flag for the in-flight grouped sweep. The cancel
    /// endpoint sets it; the sweep engine polls it between groups / in the
    /// per-token loop and bails. Reset to `false` when a sweep starts. One flag
    /// suffices because the sweep is single-flight (see `sweep_running`).
    pub sweep_cancel: Arc<AtomicBool>,
    /// Per-rule cooperative cancel flags for in-flight simulations (backtests).
    /// The simulate handler inserts a flag when a run starts and removes it when
    /// it ends; the cancel endpoint flips it; `run_backtest` polls it per chunk.
    pub sim_cancels: Arc<DashMap<Uuid, Arc<AtomicBool>>>,
    /// Caps concurrent backtests (both tpsl1 and tpsl2 share it) so overlapping
    /// simulations can't saturate the `batch` pool's connections — the contention
    /// that surfaced as `candidate token scan failed: pool timed out`.
    pub backtest_sem: Arc<Semaphore>,
    /// Live `processed / total` snapshot of the in-flight grouped sweep, written
    /// by `SweepProgress` alongside its SSE frames and read by `/api/jobs/status`
    /// so a dashboard that mounts mid-run (or after a refresh) can recover the
    /// bar. Reset to `0 / 0` when the sweep ends.
    pub sweep_progress: Arc<ProgressCell>,
    /// Per-rule `processed / total` snapshots of in-flight simulations, the
    /// per-rule analogue of `sweep_progress`. The simulate handler inserts an
    /// entry when a run starts and removes it when it ends (keyed like
    /// `sim_cancels`); `/api/jobs/status` reads them for recovery.
    pub sim_progress: Arc<DashMap<Uuid, Arc<ProgressCell>>>,
    /// Last-simulation store (disk-backed for saved rules). The detached backtest
    /// writes its terminal outcome here; the client pages it after
    /// `simulation_finished` SSE. Durable `Done` results live under
    /// `$SWEEP_LAKE_DIR/sim-results/` and survive lab restarts; RAM holds only the
    /// meta index + one working-set row payload. See [`super::sim_results`].
    pub sim_results: Arc<SimResults>,
    /// Legacy in-RAM rollup slot (unused by the disk-backed path — unfiltered
    /// summaries now ride on [`SimResults`] meta). Kept so existing wiring
    /// compiles; prefer `sim_results.cached_summary_json`.
    pub last_sim_summary: Arc<SimSummaryCache>,
    /// Option A: single-entry in-memory corpus cache. Written after each sweep
    /// completes (trades + fingerprints in hand); read by `list_token_results` to
    /// skip Parquet I/O and `attach_fingerprints` on the warm path. Single entry —
    /// a new sweep overwrites the previous.
    pub sweep_corpus_cache: Arc<RwLock<Option<SweepCorpusCache>>>,
    /// Tiered analysis caches: fingerprint-scoped candidate scans + lake history
    /// loads shared across rules with the same token filter and analysis window.
    /// Matched paging reads the mint set derived from the candidate cache.
    pub analysis_cache: Arc<AnalysisCache>,
    /// Single-flight gate for flow-discovery (mutual exclusion with
    /// [`Self::sweep_running`] — both are Duck/RAM hungry).
    pub discovery_running: Arc<AtomicBool>,
    /// Cooperative cancel for the in-flight discovery job.
    pub discovery_cancel: Arc<AtomicBool>,
    /// Progress cell for `/api/jobs/status` recovery (discovery phase).
    pub discovery_progress: Arc<ProgressCell>,
    /// Ephemeral last discovery result (keyed by `run_id`). Overwritten on the
    /// next successful run; cleared on process restart — authoring aid only.
    pub discovery_result: Arc<RwLock<Option<(Uuid, DiscoveryResult)>>>,
}

impl LocalState {
    pub fn new(core: Arc<CoreState>) -> Self {
        Self {
            core,
            sweep_running: Arc::new(AtomicBool::new(false)),
            sweep_cancel: Arc::new(AtomicBool::new(false)),
            sim_cancels: Arc::new(DashMap::new()),
            backtest_sem: Arc::new(Semaphore::new(MAX_CONCURRENT_BACKTESTS)),
            sweep_progress: Arc::new(ProgressCell::default()),
            sim_progress: Arc::new(DashMap::new()),
            sim_results: Arc::new(SimResults::new()),
            last_sim_summary: Arc::new(SimSummaryCache::new()),
            sweep_corpus_cache: Arc::new(RwLock::new(None)),
            analysis_cache: Arc::new(AnalysisCache::new()),
            discovery_running: Arc::new(AtomicBool::new(false)),
            discovery_cancel: Arc::new(AtomicBool::new(false)),
            discovery_progress: Arc::new(ProgressCell::default()),
            discovery_result: Arc::new(RwLock::new(None)),
        }
    }
}

impl std::ops::Deref for LocalState {
    type Target = CoreState;
    fn deref(&self) -> &CoreState {
        &self.core
    }
}
