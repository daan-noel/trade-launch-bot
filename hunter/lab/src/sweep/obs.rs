//! Sweep memory/timing observability (Phase 0.3).
//!
//! The memory plan judges **every** later phase against the Phase 0 baseline on
//! *two* axes — peak resident MB **and** total seconds — so a memory win that
//! silently regresses wall-clock is caught. This module is the shared, best-effort
//! instrument: process RSS (via the cross-platform `memory-stats` crate) and a
//! monotonic wall-clock, logged at each sweep milestone (admitted, corpus loaded,
//! partitioned, done). It never fails the sweep — an unreadable RSS just logs
//! `None`.
//!
//! Host physical memory (total / available) feeds the grouped-sweep admission
//! ceiling so a run leaves headroom for the OS + desktop UI.

use std::time::Instant;

/// Resident-set size of this process in bytes, or `None` if the platform read
/// failed. Best-effort: callers log it, never branch on it for correctness.
pub fn process_rss_bytes() -> Option<u64> {
    memory_stats::memory_stats().map(|s| s.physical_mem as u64)
}

/// Process RSS in whole MB (the unit the milestone logs report), or `None` when
/// unavailable. Used so a memory regression on the live box shows up directly in
/// the sweep logs without a profiler attached.
pub fn process_rss_mb() -> Option<u64> {
    process_rss_bytes().map(|b| b / (1024 * 1024))
}

/// Host physical RAM as `(total_bytes, available_bytes)`, or `None` when the
/// platform read fails. Best-effort — admission falls back to the configured
/// `SWEEP_ADMISSION_BUDGET_MB` alone when this is unavailable.
pub fn host_memory_bytes() -> Option<(u64, u64)> {
    #[cfg(windows)]
    {
        host_memory_windows()
    }
    #[cfg(target_os = "linux")]
    {
        host_memory_linux()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        None
    }
}

/// Host physical RAM in whole MB as `(total_mb, available_mb)`.
pub fn host_memory_mb() -> Option<(u64, u64)> {
    host_memory_bytes().map(|(t, a)| (t / (1024 * 1024), a / (1024 * 1024)))
}

#[cfg(windows)]
fn host_memory_windows() -> Option<(u64, u64)> {
    use std::mem::{size_of, MaybeUninit};

    // MEMORYSTATUSEX — layout matches winbase.h / sysinfoapi.h.
    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalMemoryStatusEx(status: *mut MemoryStatusEx) -> i32;
    }

    unsafe {
        let mut status = MaybeUninit::<MemoryStatusEx>::zeroed();
        let ptr = status.as_mut_ptr();
        (*ptr).length = size_of::<MemoryStatusEx>() as u32;
        if GlobalMemoryStatusEx(ptr) == 0 {
            return None;
        }
        let s = status.assume_init();
        Some((s.total_phys, s.avail_phys))
    }
}

#[cfg(target_os = "linux")]
fn host_memory_linux() -> Option<(u64, u64)> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let mut total_kb: Option<u64> = None;
    let mut avail_kb: Option<u64> = None;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else { continue };
        let Some(val) = parts.next().and_then(|v| v.parse::<u64>().ok()) else {
            continue;
        };
        match key {
            "MemTotal:" => total_kb = Some(val),
            "MemAvailable:" => avail_kb = Some(val),
            _ => {}
        }
        if total_kb.is_some() && avail_kb.is_some() {
            break;
        }
    }
    Some((total_kb? * 1024, avail_kb? * 1024))
}

/// A monotonic clock started at sweep admission. `elapsed_secs` is the wall-clock
/// since `start` — the second axis (with RSS) every milestone reports. `Copy` so
/// it threads cheaply through the handler without an `Arc`.
#[derive(Clone, Copy)]
pub struct SweepClock {
    start: Instant,
}

impl SweepClock {
    /// Start the clock — call once at admission so every milestone's `elapsed_s`
    /// is measured from the same origin.
    pub fn start() -> Self {
        Self { start: Instant::now() }
    }

    /// Seconds elapsed since [`SweepClock::start`].
    pub fn elapsed_secs(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }
}

/// Log one sweep milestone with process RSS and elapsed-since-admission. Keeping
/// the field set identical across milestones makes the per-run RSS/seconds trace
/// trivially greppable (`milestone=corpus_loaded`).
pub fn log_milestone(clock: &SweepClock, milestone: &str) {
    tracing::info!(
        milestone,
        rss_mb = process_rss_mb(),
        elapsed_s = clock.elapsed_secs(),
        "sweep: milestone"
    );
}
