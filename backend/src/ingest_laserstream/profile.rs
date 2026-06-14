//! Opt-in timing for the ingest decode hot path (H1 profiling).
//!
//! Answers the open question in `h1-typed-decode-plan.md`: is building the
//! `serde_json::Value` (base58 of signature + every account key + every
//! instruction `data`, plus the `json!` tree) in [`super::adapter::update_tx_to_value`]
//! actually a measured ingest-thread cost, or is it dwarfed by the fact that the
//! pre-filter only builds it for relevant pump/PumpSwap txs?
//!
//! Enabled by `INGEST_PROFILE=1` (or `true`). When disabled every hook is a
//! single relaxed atomic load and **no** `Instant::now()` is taken, so the
//! default ingest path pays nothing. The two stages run on different tasks —
//! the adapter (Value build) on the gRPC client task, `decode_result` on the
//! pipeline task — so they're timed at their call sites and aggregated here via
//! process-global atomics (relaxed; diagnostic, not a correctness signal).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

use tracing::info;

/// Emit a cumulative report every N decoded txs (cadence driven off the decode
/// counter, since that's the per-relevant-tx hot path).
const REPORT_EVERY: u64 = 5000;

// Adapter (`update_tx_to_value`) — split so the per-relevant-tx *build* cost is
// not diluted by the cheap pre-filter rejections of the irrelevant firehose.
static ADAPTER_BUILT_COUNT: AtomicU64 = AtomicU64::new(0);
static ADAPTER_BUILT_NANOS: AtomicU64 = AtomicU64::new(0);
static ADAPTER_BUILT_MAX: AtomicU64 = AtomicU64::new(0);
static ADAPTER_FILTERED_COUNT: AtomicU64 = AtomicU64::new(0);
static ADAPTER_FILTERED_NANOS: AtomicU64 = AtomicU64::new(0);

// Decode (`decode_result`) — runs once per Value the adapter built.
static DECODE_COUNT: AtomicU64 = AtomicU64::new(0);
static DECODE_NANOS: AtomicU64 = AtomicU64::new(0);
static DECODE_MAX: AtomicU64 = AtomicU64::new(0);

#[inline]
fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        let on = std::env::var("INGEST_PROFILE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        if on {
            info!(
                "ingest decode profiler ENABLED (INGEST_PROFILE) — cumulative report every {REPORT_EVERY} decoded txs"
            );
        }
        on
    })
}

/// Start a timing span. Returns `None` (and takes no clock reading) when the
/// profiler is off, so the disabled path costs one relaxed atomic load.
#[inline]
pub fn start() -> Option<Instant> {
    if enabled() {
        Some(Instant::now())
    } else {
        None
    }
}

/// Record one `update_tx_to_value` call. `built` is whether it produced a `Value`
/// (passed the pump/PumpSwap pre-filter) vs. cheaply rejected the tx.
#[inline]
pub fn record_adapter(start: Option<Instant>, built: bool) {
    let Some(start) = start else { return };
    let nanos = start.elapsed().as_nanos() as u64;
    if built {
        ADAPTER_BUILT_COUNT.fetch_add(1, Ordering::Relaxed);
        ADAPTER_BUILT_NANOS.fetch_add(nanos, Ordering::Relaxed);
        ADAPTER_BUILT_MAX.fetch_max(nanos, Ordering::Relaxed);
    } else {
        ADAPTER_FILTERED_COUNT.fetch_add(1, Ordering::Relaxed);
        ADAPTER_FILTERED_NANOS.fetch_add(nanos, Ordering::Relaxed);
    }
}

/// Record one `decode_result` call, and emit a report every `REPORT_EVERY` calls.
#[inline]
pub fn record_decode(start: Option<Instant>) {
    let Some(start) = start else { return };
    let nanos = start.elapsed().as_nanos() as u64;
    DECODE_NANOS.fetch_add(nanos, Ordering::Relaxed);
    DECODE_MAX.fetch_max(nanos, Ordering::Relaxed);
    let n = DECODE_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    if n.is_multiple_of(REPORT_EVERY) {
        report();
    }
}

fn report() {
    let us = |sum: u64, cnt: u64| if cnt > 0 { sum as f64 / cnt as f64 / 1000.0 } else { 0.0 };

    let built = ADAPTER_BUILT_COUNT.load(Ordering::Relaxed);
    let built_nanos = ADAPTER_BUILT_NANOS.load(Ordering::Relaxed);
    let built_max = ADAPTER_BUILT_MAX.load(Ordering::Relaxed);
    let filtered = ADAPTER_FILTERED_COUNT.load(Ordering::Relaxed);
    let filtered_nanos = ADAPTER_FILTERED_NANOS.load(Ordering::Relaxed);
    let seen = built + filtered;

    let decodes = DECODE_COUNT.load(Ordering::Relaxed);
    let decode_nanos = DECODE_NANOS.load(Ordering::Relaxed);
    let decode_max = DECODE_MAX.load(Ordering::Relaxed);

    info!(
        target: "ingest_profile",
        adapter_seen = seen,
        adapter_built = built,
        built_ratio = if seen > 0 { built as f64 / seen as f64 } else { 0.0 },
        value_build_avg_us = us(built_nanos, built),
        value_build_max_us = built_max as f64 / 1000.0,
        prefilter_reject_avg_us = us(filtered_nanos, filtered),
        decode_calls = decodes,
        decode_avg_us = us(decode_nanos, decodes),
        decode_max_us = decode_max as f64 / 1000.0,
        "ingest decode profile (cumulative)"
    );
}
