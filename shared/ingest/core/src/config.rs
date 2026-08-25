use std::time::Duration;

/// Commitment level requested from the gRPC stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Commitment {
    Processed,
    Confirmed,
    Finalized,
}

/// Which transport supplies **bonding-curve** transactions.
///
/// AMM (post-migration pool) traffic is always served by the gRPC transport: it
/// needs a filter keyed on the pool PDAs this bot tracks, which a broadcast relay
/// cannot provide. Switching this at runtime via
/// [`crate::IngestHandle::set_curve_source`] re-points the curve feed and
/// re-scopes the gRPC subscription in one step; see [`SubscriptionRole`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveSource {
    /// Yellowstone gRPC (LaserStream). Costs provider credits; supports
    /// `from_slot` gap replay.
    Grpc,
    /// A third-party NATS relay broadcasting Helius `transactionNotification`
    /// frames. No credits, no replay, no filter control.
    Nats,
}

/// Which slice of the venue's accounts a subscription covers.
///
/// The gRPC transport asks the venue for its accounts on every (re)subscribe;
/// the role is what makes "curve came from NATS, so stop paying for it here"
/// expressible without a second venue impl.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionRole {
    /// Program id(s) + every tracked pool PDA — the single-transport default.
    All,
    /// Tracked pool PDAs only; the venue program id is deliberately omitted
    /// because another transport is carrying curve traffic.
    AmmOnly,
}

/// Connection settings for the NATS curve feed.
///
/// Core NATS is at-most-once with no replay: a slow consumer is *disconnected*
/// by the server rather than buffered. [`Self::frame_channel_cap`] is the
/// defence — the socket reader only hands raw bytes to a bounded queue and never
/// parses inline, so a decode stall sheds frames instead of stalling the read
/// and getting the connection dropped.
#[derive(Debug, Clone)]
pub struct NatsConfig {
    /// `nats://host:port`. Comma-separated entries seed a cluster.
    pub url: String,
    /// Subject to subscribe to, e.g. `helius.raw.bondingcurve`.
    ///
    /// Prefer an exact subject over a wildcard: a relay that also publishes a
    /// mirror subject (`helius.raw.all`) would otherwise deliver every frame
    /// twice, doubling bandwidth for nothing.
    pub subject: String,
    /// Optional queue group — only for running several consumers that should
    /// *share* the stream. A single bot leaves this `None`.
    pub queue_group: Option<String>,
    /// Depth of the reader -> parser hand-off. Frames beyond this are shed (and
    /// counted) rather than blocking the socket read.
    pub frame_channel_cap: usize,
    /// Force a reconnect if no frame arrives within this window.
    pub idle_reconnect_timeout: Duration,
}

impl Default for NatsConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            subject: "helius.raw.bondingcurve".to_string(),
            queue_group: None,
            frame_channel_cap: 8192,
            idle_reconnect_timeout: Duration::from_secs(30),
        }
    }
}

/// Transport authentication for the Yellowstone gRPC provider.
///
/// Provider-as-config: every Yellowstone provider (Helius, Triton, Shyft, a
/// self-hosted geyser) speaks the same wire protocol and differs only in
/// endpoint + auth. Swapping providers is therefore a config change — no new
/// crate. The current default path is Helius's `x-token` header.
#[derive(Debug, Clone)]
pub enum Auth {
    /// `x-token` metadata header (Helius / Triton / Shyft).
    XToken(String),
    /// No auth (self-hosted / local validator geyser).
    None,
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

    // ── curve source selection ───────────────────────────────────────────────
    /// Initial bonding-curve transport. Changeable at runtime through
    /// [`crate::IngestHandle::set_curve_source`].
    pub curve_source: CurveSource,
    /// NATS relay settings. Required when `curve_source` is
    /// [`CurveSource::Nats`], and also when the host wants to switch to it later
    /// without a restart — the NATS task is spawned (idle) either way.
    pub nats: Option<NatsConfig>,
    /// How long a signature stays in the cross-transport dedupe window.
    ///
    /// Two transports can legitimately deliver the same transaction: during a
    /// source switch (both curve feeds briefly overlap), and in steady state for
    /// a migration tx that touches both the venue program and a tracked pool.
    /// Must comfortably exceed the switch overlap.
    pub dedupe_window: Duration,
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
            // gRPC stays the default: it is the only source that can replay a gap
            // and the only one whose filter this bot controls.
            curve_source: CurveSource::Grpc,
            nats: None,
            dedupe_window: Duration::from_secs(30),
        }
    }
}
