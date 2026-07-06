use std::time::Duration;

/// Commitment level requested from the gRPC stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Commitment {
    Processed,
    Confirmed,
    Finalized,
}

/// All dynamic tunables for the ingest crate.
///
/// Every timeout, cap, and interval is a field with the current proven value as
/// its `Default`. The crate never reads env — the host builds this struct from
/// its own settings and passes it to [`crate::Ingest::builder`].
#[derive(Debug, Clone)]
pub struct IngestConfig {
    // ── transport / reconnect ────────────────────────────────────────────────
    /// TCP/TLS connect timeout.
    pub connect_timeout: Duration,
    /// Base reconnect delay (reset on any attempt that made progress).
    pub reconnect_base: Duration,
    /// Upper bound on the exponential reconnect backoff.
    pub reconnect_max_backoff: Duration,
    /// Force a reconnect if no tx arrives within this window (silent-stall guard).
    pub idle_reconnect_timeout: Duration,
    /// How often the idle-reconnect check fires.
    pub idle_check_interval: Duration,
    /// HTTP/2 keepalive interval.
    pub http2_keepalive: Duration,
    /// TCP keepalive interval.
    pub tcp_keepalive: Duration,
    /// Max gRPC message size (bytes).
    pub max_decoding_message_size: usize,
    /// Hard cap on how long to wait handing a tx to the decode task before
    /// dropping the connection and reconnecting (shed + prevent silent freeze).
    pub pipeline_send_timeout: Duration,
    /// Quiet window for coalescing a burst of pool-set changes into one
    /// resubscribe.
    pub resubscribe_debounce: Duration,
    /// gRPC stream commitment level.
    pub commitment: Commitment,

    // ── channels ─────────────────────────────────────────────────────────────
    /// Capacity of the internal transport→decode channel.
    pub update_channel_cap: usize,
    /// Capacity of the output mpsc channel to the host consumer.
    pub event_channel_cap: usize,

    // ── pool tracking (AMM, post-migration) ──────────────────────────────────
    /// When false: never subscribe to AMM pools, skip AMM decode entirely.
    pub track_amm: bool,
}

impl Default for IngestConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            reconnect_base: Duration::from_secs(1),
            reconnect_max_backoff: Duration::from_secs(30),
            idle_reconnect_timeout: Duration::from_secs(10),
            idle_check_interval: Duration::from_secs(2),
            http2_keepalive: Duration::from_secs(30),
            tcp_keepalive: Duration::from_secs(30),
            max_decoding_message_size: 64 * 1024 * 1024,
            pipeline_send_timeout: Duration::from_secs(10),
            resubscribe_debounce: Duration::from_millis(250),
            commitment: Commitment::Processed,
            update_channel_cap: 4096,
            event_channel_cap: 4096,
            track_amm: true,
        }
    }
}
