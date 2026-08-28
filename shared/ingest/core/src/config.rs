use std::time::Duration;

/// Commitment level requested from the chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Commitment {
    Processed,
    Confirmed,
    Finalized,
}

/// Wire-neutral tunables for the ingest engine.
///
/// Every timeout, cap, and interval is a field with the current proven value as
/// its `Default`. The crate never reads env — the host builds this struct and
/// passes it to the venue's builder.
///
/// **Only what the engine itself uses lives here.** Anything a single wire needs
/// (a dial timeout, HTTP/2 keepalive, a subject name, how long silence is allowed
/// on *that* wire) belongs to that feed crate's own config, so a new transport
/// cannot grow this struct.
#[derive(Debug, Clone)]
pub struct IngestConfig {
    // ── reconnect ────────────────────────────────────────────────────────────
    /// Base reconnect delay (reset on any attempt that made progress).
    pub reconnect_base: Duration,
    /// Upper bound on the exponential reconnect backoff.
    pub reconnect_max_backoff: Duration,
    /// How often the idle-reconnect check fires.
    pub idle_check_interval: Duration,
    /// Hard cap on how long to wait handing an update to a decode lane before
    /// the feed's back-pressure verdict applies (shed, or drop the connection).
    pub pipeline_send_timeout: Duration,
    /// Quiet window for coalescing a burst of pool-set changes into one
    /// resubscribe.
    pub resubscribe_debounce: Duration,
    /// Commitment level requested on every subscription.
    pub commitment: Commitment,

    // ── channels ─────────────────────────────────────────────────────────────
    /// Capacity of the internal feed→decode channel.
    pub update_channel_cap: usize,
    /// Capacity of the output mpsc channel to the host consumer.
    pub event_channel_cap: usize,

    // ── pool tracking (AMM, post-migration) ──────────────────────────────────
    /// When false: never subscribe to AMM pools, skip AMM decode entirely.
    pub track_amm: bool,

    // ── cross-feed dedupe ────────────────────────────────────────────────────
    /// How long a signature stays in the cross-feed dedupe window.
    ///
    /// Two feeds can legitimately deliver the same transaction: during a scope
    /// switch (both briefly overlap), and in steady state for a migration tx that
    /// touches both the venue program and a tracked pool. Must comfortably exceed
    /// the switch overlap. Armed only when more than one feed is running.
    pub dedupe_window: Duration,
}

impl Default for IngestConfig {
    fn default() -> Self {
        Self {
            reconnect_base: Duration::from_secs(1),
            reconnect_max_backoff: Duration::from_secs(30),
            idle_check_interval: Duration::from_secs(2),
            pipeline_send_timeout: Duration::from_secs(10),
            resubscribe_debounce: Duration::from_millis(250),
            commitment: Commitment::Processed,
            update_channel_cap: 4096,
            event_channel_cap: 4096,
            track_amm: true,
            dedupe_window: Duration::from_secs(30),
        }
    }
}
