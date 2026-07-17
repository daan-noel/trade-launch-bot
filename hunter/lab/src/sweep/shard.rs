//! Combo-space sharding — split a large param vec into RAM-sized ranges, run
//! each shard (optionally a few in parallel), spill shard metrics to disk, merge.
//!
//! This is the in-process equivalent of multi-process workers: each shard's peak
//! is bounded; parallel shard count is capped by usable host RAM.

use std::ops::Range;

use crate::sweep::aggregate::ComboAgg;
use crate::sweep::engine::{full_combo_aggs_fit, HARD_MAX_COMBO_BATCH};

/// How many combos one shard may hold so that either wave-outer (`N × ComboAgg`)
/// or pass-outer metrics (`N × ComboMetrics`) stay inside a fraction of usable RAM.
pub fn max_combos_per_shard(wave: usize, max_series_bytes: usize) -> usize {
    let fold = crate::sweep::registry::sweep_memory_budget_bytes();
    let series = (wave as u64).saturating_mul(max_series_bytes as u64);
    let usable = crate::sweep::registry::usable_host_bytes().unwrap_or(512 * 1024 * 1024);
    // Leave room for corpus already resident + one fold peak + slack.
    let budget = usable
        .saturating_sub(fold)
        .saturating_sub(series)
        .saturating_sub(256 * 1024 * 1024);
    let per_agg = std::mem::size_of::<ComboAgg>() as u64;
    let per_metrics = std::mem::size_of::<crate::sweep::aggregate::ComboMetrics>() as u64;
    // Prefer a shard that can wave-outer (agg-sized); fall back to metrics-sized.
    let by_agg = if per_agg == 0 {
        HARD_MAX_COMBO_BATCH
    } else {
        (budget / per_agg).max(1) as usize
    };
    let by_metrics = if per_metrics == 0 {
        HARD_MAX_COMBO_BATCH
    } else {
        (budget / per_metrics).max(1) as usize
    };
    // Cap at hard batch × 4 so a shard isn't enormous even on a fresh box.
    let cap = HARD_MAX_COMBO_BATCH.saturating_mul(4).max(8_192);
    by_agg.min(by_metrics).min(cap).max(1)
}

/// Inclusive-exclusive combo index ranges covering `0..n_combos`.
pub fn plan_shards(n_combos: usize, wave: usize, max_series_bytes: usize) -> Vec<Range<usize>> {
    if n_combos == 0 {
        return Vec::new();
    }
    let per = max_combos_per_shard(wave, max_series_bytes);
    // Single shard when the whole set fits wave-outer or is under `per`.
    if n_combos <= per || full_combo_aggs_fit(n_combos, wave, max_series_bytes) {
        return vec![0..n_combos];
    }
    let mut out = Vec::new();
    let mut start = 0;
    while start < n_combos {
        let end = (start + per).min(n_combos);
        out.push(start..end);
        start = end;
    }
    out
}

/// How many shards may run concurrently given per-shard peak estimate.
pub fn max_parallel_shards(
    shard_combos: usize,
    wave: usize,
    max_series_bytes: usize,
    threads: usize,
) -> usize {
    let _ = shard_combos; // reserved for tighter per-shard sizing later
    let series = (wave as u64).saturating_mul(max_series_bytes as u64);
    let fold = crate::sweep::registry::sweep_memory_budget_bytes();
    // Peak per concurrent shard ≈ series wave + fold budget (batch-sized).
    let per_shard = series.saturating_add(fold).max(1);
    let usable = crate::sweep::registry::usable_host_bytes().unwrap_or(fold);
    let by_ram = (usable / per_shard).max(1) as usize;
    // Never more parallel shards than half the thread pool (each shard saturates
    // rayon for its token wave).
    by_ram.min(threads.max(1)).min(4).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_shards_single_when_small() {
        let shards = plan_shards(100, 4, 0);
        assert_eq!(shards, vec![0..100]);
    }

    #[test]
    fn plan_shards_covers_full_range() {
        let n = 12_345;
        let shards = plan_shards(n, 4, 1024);
        assert!(!shards.is_empty());
        assert_eq!(shards.first().unwrap().start, 0);
        assert_eq!(shards.last().unwrap().end, n);
        for w in shards.windows(2) {
            assert_eq!(w[0].end, w[1].start);
        }
        assert!(max_combos_per_shard(4, 1024) >= 1);
    }

    #[test]
    fn parallel_at_least_one() {
        assert!(max_parallel_shards(10_000, 4, 1024, 8) >= 1);
    }
}
