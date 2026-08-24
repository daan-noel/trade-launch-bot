use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use uuid::Uuid;
use tokio::sync::{RwLock, Semaphore};

use super::core_state::CoreState;
use super::job_progress::ProgressCell;
use super::analysis_cache::AnalysisCache;
use super::discovery_result_cache::DiscoveryResultCache;
use super::sim_results::SimResults;
use super::sim_summary::SimSummaryCache;
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
    /// When this corpus was last written or read, for the idle reaper
    /// ([`crate::state::idle_reaper`]). `Mutex` rather than a plain field because
    /// a *read* must record the touch, and reads hold only `RwLock::read`.
    touched: Mutex<Instant>,
}

impl SweepCorpusCache {
    pub fn new(
        corpus_hash: String,
        tokens: Arc<Vec<CorpusToken>>,
        candidates_capped: bool,
    ) -> Self {
        Self {
            corpus_hash,
            tokens,
            candidates_capped,
            touched: Mutex::new(Instant::now()),
        }
    }

    /// Record a cache hit, so drilling into results keeps the corpus warm.
    pub fn touch(&self) {
        if let Ok(mut t) = self.touched.lock() {
            *t = Instant::now();
        }
    }

    /// How long since the last write or read.
    pub fn idle(&self) -> Duration {
        self.touched
            .lock()
            .map(|t| t.elapsed())
            .unwrap_or(Duration::ZERO)
    }
}

/// Max concurrent backtests across both strategies. Each one streams the `tokens`
/// table and batched trade history off the `batch` pool (16 connections by
/// default), so unbounded overlap drains it. 4 rather than 2 because
/// `analysis_cache` single-flights the candidate scan + lake load: a "Simulate All"
/// batch of same-fingerprint rules collapses onto one shared scan instead of one
/// whole-table scan per concurrent rule, so the extra slots turn into real
/// parallelism — distinct-fingerprint scans and the per-rule exit walk — rather than
/// duplicated pool pressure. 4 keeps well under the 16-conn pool and
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
    /// Last discovery result (keyed by `run_id`), disk-backed so it survives a
    /// `hunter-lab` restart — authoring aid; a corpus fold can take a while and
    /// shouldn't have to re-run just to be looked at again. See
    /// [`super::discovery_result_cache`].
    pub discovery_result: Arc<DiscoveryResultCache>,
    /// Single-flight gate for the metric-combo discovery **pipeline** (screen →
    /// family → validate). Mutually exclusive with [`Self::sweep_running`] and
    /// [`Self::discovery_running`] — all three are Duck/RAM hungry.
    pub metric_discovery_running: Arc<AtomicBool>,
    /// Cooperative cancel for the in-flight pipeline run.
    pub metric_discovery_cancel: Arc<AtomicBool>,
    /// Progress cell for `/api/jobs/status` recovery (pipeline phase).
    pub metric_discovery_progress: Arc<ProgressCell>,
    /// Last pipeline result as its JSON DTO, keyed by `run_id`. In-RAM single-slot
    /// (single-flight run) — an authoring aid, not durable state, so a lab restart
    /// simply re-runs. `None` until the first pipeline completes.
    pub metric_discovery_result: Arc<RwLock<Option<(Uuid, serde_json::Value)>>>,
    /// Single-flight gate for rule search. Mutually exclusive with sweep /
    /// flow-discovery / metric-discovery.
    pub rule_search_running: Arc<AtomicBool>,
    pub rule_search_cancel: Arc<AtomicBool>,
    pub rule_search_progress: Arc<ProgressCell>,
    pub rule_search_result: Arc<super::rule_search_cache::RuleSearchCache>,
    /// Single-flight gate for family search. Mutually exclusive with every other
    /// heavy job — it holds two corpora at once, so it is the RAM-hungriest of them.
    pub family_search_running: Arc<AtomicBool>,
    pub family_search_cancel: Arc<AtomicBool>,
    pub family_search_progress: Arc<ProgressCell>,
    pub family_search_result: Arc<super::family_search_cache::FamilySearchCache>,
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
            discovery_result: Arc::new(DiscoveryResultCache::new()),
            metric_discovery_running: Arc::new(AtomicBool::new(false)),
            metric_discovery_cancel: Arc::new(AtomicBool::new(false)),
            metric_discovery_progress: Arc::new(ProgressCell::default()),
            metric_discovery_result: Arc::new(RwLock::new(None)),
            rule_search_running: Arc::new(AtomicBool::new(false)),
            rule_search_cancel: Arc::new(AtomicBool::new(false)),
            rule_search_progress: Arc::new(ProgressCell::default()),
            rule_search_result: Arc::new(super::rule_search_cache::RuleSearchCache::new()),
            family_search_running: Arc::new(AtomicBool::new(false)),
            family_search_cancel: Arc::new(AtomicBool::new(false)),
            family_search_progress: Arc::new(ProgressCell::default()),
            family_search_result: Arc::new(super::family_search_cache::FamilySearchCache::new()),
        }
    }

    /// Why a heavy analysis job cannot start (`None` = free).
    pub fn heavy_job_block(&self) -> Option<&'static str> {
        use std::sync::atomic::Ordering::Acquire;
        if self.sweep_running.load(Acquire) {
            Some("a grouped sweep is already running — wait or cancel it first")
        } else if self.discovery_running.load(Acquire) {
            Some("a flow-discovery job is already running — wait or cancel it first")
        } else if self.metric_discovery_running.load(Acquire) {
            Some("a metric-discovery pipeline is already running — wait or cancel it first")
        } else if self.rule_search_running.load(Acquire) {
            Some("a rule-search job is already running — wait or cancel it first")
        } else if self.family_search_running.load(Acquire) {
            Some("a family-search job is already running — wait or cancel it first")
        } else {
            None
        }
    }

    /// True when nothing is running that could be reading a cache: no heavy Duck
    /// job **and** no in-flight backtest. The reaper's sole gate.
    ///
    /// `sim_progress` is the backtest term and is not optional: simulations are
    /// gated by `backtest_sem`, not by [`Self::heavy_job_block`], so a check
    /// against the heavy flags alone would happily reap out from under a
    /// "Simulate All" batch.
    pub fn is_idle(&self) -> bool {
        self.heavy_job_block().is_none() && self.sim_progress.is_empty()
    }

    /// This job's single-flight flag — the ONE mapping from job to flag, so a new
    /// `HeavyJob` variant cannot be added to some of the checks and missed by others.
    fn running_flag(&self, kind: HeavyJob) -> &Arc<AtomicBool> {
        match kind {
            HeavyJob::Sweep => &self.sweep_running,
            HeavyJob::Discovery => &self.discovery_running,
            HeavyJob::MetricDiscovery => &self.metric_discovery_running,
            HeavyJob::RuleSearch => &self.rule_search_running,
            HeavyJob::FamilySearch => &self.family_search_running,
        }
    }

    /// Claim this job's running flag. Releases and returns `Err` if another
    /// heavy job snuck in between the check and the claim.
    pub fn claim_heavy(&self, kind: HeavyJob) -> Result<(), &'static str> {
        use std::sync::atomic::Ordering::{AcqRel, Acquire, Release};
        let flag = self.running_flag(kind);
        if flag.compare_exchange(false, true, AcqRel, Acquire).is_err() {
            return Err(kind.busy_msg());
        }
        // Mutual exclusion over EVERY other heavy job, derived from `HeavyJob::ALL`
        // rather than hand-enumerated per variant: the old shape listed the other
        // three inside each arm, so adding a job meant editing every arm and a miss
        // let two RAM-hungry jobs run together.
        let other = HeavyJob::ALL
            .iter()
            .any(|&k| !k.same_as(kind) && self.running_flag(k).load(Acquire));
        if other {
            flag.store(false, Release);
            return Err(self.heavy_job_block().unwrap_or(
                "another analysis job is already running — wait or cancel it first",
            ));
        }
        Ok(())
    }
}

/// The Duck/RAM-hungry lab jobs — one at a time.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HeavyJob {
    Sweep,
    Discovery,
    MetricDiscovery,
    RuleSearch,
    FamilySearch,
}

impl HeavyJob {
    /// Every variant — the list `claim_heavy` derives mutual exclusion from.
    pub const ALL: [HeavyJob; 5] = [
        HeavyJob::Sweep,
        HeavyJob::Discovery,
        HeavyJob::MetricDiscovery,
        HeavyJob::RuleSearch,
        HeavyJob::FamilySearch,
    ];

    fn same_as(self, other: HeavyJob) -> bool {
        self == other
    }

    fn busy_msg(self) -> &'static str {
        match self {
            HeavyJob::Sweep => "a grouped sweep is already running",
            HeavyJob::Discovery => "a flow-discovery job is already running",
            HeavyJob::MetricDiscovery => "a metric-discovery pipeline is already running",
            HeavyJob::RuleSearch => "a rule-search job is already running",
            HeavyJob::FamilySearch => "a family-search job is already running",
        }
    }
}

impl std::ops::Deref for LocalState {
    type Target = CoreState;
    fn deref(&self) -> &CoreState {
        &self.core
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache() -> SweepCorpusCache {
        SweepCorpusCache::new("hash".into(), Arc::new(Vec::new()), false)
    }

    /// A cache hit resets the idle clock, so a corpus the user is actively
    /// drilling into is never reaped out from under them.
    #[test]
    fn touch_resets_the_idle_clock() {
        let c = cache();
        std::thread::sleep(Duration::from_millis(20));
        let before = c.idle();
        assert!(before >= Duration::from_millis(20));
        c.touch();
        assert!(c.idle() < before, "touch must reset idle");
    }

    /// A freshly written corpus is not idle — the reaper's TTL is measured from
    /// the write, not from process start.
    #[test]
    fn a_fresh_corpus_is_not_idle() {
        assert!(cache().idle() < Duration::from_secs(1));
    }
}
