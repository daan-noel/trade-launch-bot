use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use dashmap::DashMap;
use uuid::Uuid;
use tokio::sync::{RwLock, Semaphore};

use super::core_state::CoreState;
use super::job_progress::ProgressCell;
use super::analysis_cache::AnalysisCache;
use super::sim_results::SimResults;
use super::swing_results::SwingResults;
use super::swing_run_cache::SwingRunCache;
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
}

/// Max concurrent backtests across both strategies. Each one streams the `tokens`
/// table and batched trade history off the `batch` pool (16 connections by
/// default), so unbounded overlap drains it. Two keeps the dashboard's backtests
/// responsive without letting them starve each other or a running grouped sweep.
const MAX_CONCURRENT_BACKTESTS: usize = 2;

/// Analysis (local) state: the shared [`CoreState`] plus the handles only the
/// backtest/sweep/swing process needs — sweep + sim + swing run
/// gates/progress/results, and the warm sweep corpus cache. `Deref`s to
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
    /// Finished-simulation outcomes awaiting collection, keyed by `rule_id`. The
    /// detached backtest stores its terminal result here and the client fetches
    /// it via `GET /api/jobs/simulations/{rule_id}/result` once the
    /// `simulation_finished` SSE fires, so a long run's result is never tied to
    /// the lifetime of the starting request (the old `FETCH_ERROR` source).
    pub sim_results: Arc<SimResults>,
    /// Per-run cooperative cancel flags for in-flight "Swing Detection All" runs,
    /// the swing analogue of `sim_cancels`. Keyed by the client run id (`String`)
    /// like [`Self::swing_runs`], not a rule `Uuid`. The start handler inserts a
    /// flag; the cancel endpoint flips it; the detached scan polls it between mints.
    pub swing_cancels: Arc<DashMap<String, Arc<AtomicBool>>>,
    /// Per-run `processed / total` snapshots of in-flight swing runs, the swing
    /// analogue of `sim_progress`. The start handler inserts an entry (removed when
    /// the run ends, keyed like `swing_cancels`); `/api/jobs/status` reads them so
    /// a reconnecting Swing Detection page recovers its progress bar.
    pub swing_progress: Arc<DashMap<String, Arc<ProgressCell>>>,
    /// Finished swing-run outcomes awaiting collection, keyed by client run id. The
    /// detached scan stores its terminal result here and the client fetches it via
    /// `GET /api/jobs/swings/{run_id}/result` once the `swing_detection_finished`
    /// SSE fires. Swing twin of `sim_results`.
    pub swing_results: Arc<SwingResults>,
    /// Raw swings from recent "Swing Detection All" runs, keyed by client run id.
    /// Lets the server-side-paged tokens list sort by the chain columns and
    /// re-group on chain-latency changes without re-running detection.
    pub swing_runs: Arc<SwingRunCache>,
    /// Option A: single-entry in-memory corpus cache. Written after each sweep
    /// completes (trades + fingerprints in hand); read by `list_token_results` to
    /// skip Parquet I/O and `attach_fingerprints` on the warm path. Single entry —
    /// a new sweep overwrites the previous.
    pub sweep_corpus_cache: Arc<RwLock<Option<SweepCorpusCache>>>,
    /// Tiered analysis caches: fingerprint-scoped candidate scans + lake history
    /// loads shared across rules with the same token filter and analysis window.
    /// Matched paging reads the mint set derived from the candidate cache.
    pub analysis_cache: Arc<AnalysisCache>,
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
            swing_cancels: Arc::new(DashMap::new()),
            swing_progress: Arc::new(DashMap::new()),
            swing_results: Arc::new(SwingResults::new()),
            // Keep a few recent runs so re-runs / multiple tabs don't accumulate.
            swing_runs: Arc::new(SwingRunCache::new(3)),
            sweep_corpus_cache: Arc::new(RwLock::new(None)),
            analysis_cache: Arc::new(AnalysisCache::new()),
        }
    }
}

impl std::ops::Deref for LocalState {
    type Target = CoreState;
    fn deref(&self) -> &CoreState {
        &self.core
    }
}
