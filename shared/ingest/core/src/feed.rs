//! The feed seam — the trait every ingest transport implements.
//!
//! `ingest-core` owns the *policy* (reconnect ramp, replay anchor, idle guard,
//! lane routing, dedupe) and knows nothing about wires. A **feed** crate
//! (`ingest-laserstream`, `ingest-nats`) owns exactly one wire: how to open it,
//! how to ask it for a slice of the chain, and how to pull the next update off
//! it. Adding a transport is a new crate implementing [`Feed`] + [`FeedConn`]
//! and one arm in the venue's assembly root — no change here, in
//! [`crate::supervisor`], or in any venue.
//!
//! Static dispatch throughout (`supervisor::run<V, F>`), mirroring the
//! [`crate::venue::IngestVenue`] seam: no `Box<dyn>` on the hot path.

use std::fmt;
use std::future::Future;

use crate::config::Commitment;
use crate::proto::geyser::SubscribeUpdateTransaction;

// ── What a feed can do ────────────────────────────────────────────────────────

/// The capabilities the supervisor branches on, so it never has to know which
/// wire it is driving.
///
/// One flag = one thing the transport can or cannot do. Everything the
/// supervisor decides differently per feed reduces to these three, which is what
/// keeps a new transport from adding arms to the policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedCaps {
    /// Can resume from a slot, so a reconnect closes the gap it opened
    /// (Yellowstone `from_slot`). A broadcast relay cannot.
    pub replay: bool,
    /// The server applies our `account_include` filter. False for a broadcast
    /// subject, where the publisher chooses what arrives and the scope is
    /// applied locally by [`crate::venue::IngestVenue::classify`].
    pub server_filter: bool,
    /// A filter change can be applied to the open connection, with no reconnect
    /// and no gap.
    pub in_place_resubscribe: bool,
}

impl FeedCaps {
    /// Whether a stalled decode pipeline should drop the connection.
    ///
    /// Worth it only when the feed can replay: the reconnect then re-requests
    /// exactly the slots the stall cost. Without replay a reconnect loses the
    /// same frames a shed would have lost **and** pays a resubscribe, so
    /// shedding is strictly better and the feed stays up.
    pub fn reconnect_on_backpressure(self) -> bool {
        self.replay
    }
}

// ── What slice of the venue a feed carries ────────────────────────────────────

/// Which slice of the venue's traffic one feed is responsible for.
///
/// Replaces the old `CurveSource` × `SubscriptionRole` pair. That pair grew as
/// the product of (transports × slices), so a third transport meant new arms in
/// the transport, the venue, and the host. A scope is independent of the wire:
/// the assembly root hands each feed one, and every feed reads it the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamScope {
    /// Carry the venue's program id(s) — i.e. bonding-curve traffic.
    pub program: bool,
    /// Carry every tracked pool PDA — i.e. post-migration AMM traffic.
    pub pools: bool,
}

impl StreamScope {
    /// Program id(s) + tracked pools — the single-feed default.
    pub const ALL: Self = Self {
        program: true,
        pools: true,
    };
    /// Program id(s) only: another feed is carrying the pools.
    pub const CURVE: Self = Self {
        program: true,
        pools: false,
    };
    /// Tracked pools only: another feed is carrying the curve, so leaving the
    /// program id in would pay the provider for it twice.
    pub const POOLS: Self = Self {
        program: false,
        pools: true,
    };
    /// Nothing — this feed idles (fully disconnected, costing no bandwidth).
    pub const NONE: Self = Self {
        program: false,
        pools: false,
    };

    pub fn is_empty(self) -> bool {
        !self.program && !self.pools
    }
}

// ── One subscription request, wire-neutral ────────────────────────────────────

/// What to ask the wire for. A feed that cannot honour a field ignores it: a
/// broadcast relay has no filter and no `from_slot`, and says so through
/// [`FeedCaps`] so the supervisor never depends on the difference.
pub struct Subscription {
    /// Filter-map key the venue owns (pump.fun: `"pumpfun"`).
    pub filter_key: &'static str,
    /// Accounts to watch. Empty means "watch nothing" — a feed whose wire reads
    /// an empty filter as "watch everything" must omit the filter entirely.
    pub account_include: Vec<String>,
    /// Resume point. Only ever `Some` for a feed whose [`FeedCaps::replay`] is
    /// true.
    pub from_slot: Option<u64>,
    pub commitment: Commitment,
    /// Add a block-meta feed to this subscription (see [`crate::push::PushHooks`]).
    pub blocks_meta: bool,
    /// Extra account pubkeys to watch for state updates (see
    /// [`crate::push::PushHooks`]).
    pub watch_accounts: Vec<String>,
}

// ── One update off a feed ─────────────────────────────────────────────────────

/// A single update, in the currency every decoder already speaks.
///
/// A JSON feed converts to [`SubscribeUpdateTransaction`] in its own crate (via
/// the one shared [`crate::convert`] adapter) before it gets here, so a frame
/// decodes identically no matter which wire delivered it.
///
/// The transaction variant dwarfs the others, and deliberately stays unboxed:
/// it is the hot payload, arriving hundreds of times a second, and a `Box` here
/// would buy back a move by adding the per-event allocation the hot path forbids.
/// The supervisor `Arc`s it once, at the point it is actually shared.
#[allow(clippy::large_enum_variant)]
pub enum FeedUpdate {
    Transaction(SubscribeUpdateTransaction),
    BlockMeta {
        slot: u64,
        blockhash: String,
        /// The chain's own clock for the slot, whole seconds. The ONLY chain-time
        /// reference on the stream — a transaction frame carries a slot but no
        /// block time.
        block_time: Option<i64>,
    },
    Account {
        slot: u64,
        pubkey: String,
        lamports: u64,
        data: Vec<u8>,
    },
    /// A frame arrived carrying nothing this pipeline wants (a ping, a slot
    /// notification, relay noise, a failed transaction).
    ///
    /// Delivered rather than swallowed because it is **liveness evidence**: the
    /// idle guard judges a broadcast subject by frames, and a subject that goes
    /// quiet is a dead relay whether or not its last frames were relevant.
    Tick,
}

// ── Why a connection ended ────────────────────────────────────────────────────

/// A feed failure, in terms the supervisor's reconnect policy is written in.
#[derive(Debug)]
pub enum FeedError {
    /// Could not open the wire.
    Connect(String),
    /// Opened, but the subscription was rejected.
    Subscribe(String),
    /// Failed mid-stream.
    Stream(String),
    /// The provider is shedding us for capacity or billing reasons. Never replay
    /// into this: the backlog is what caused it, so re-requesting it is
    /// self-reinforcing.
    Exhausted(String),
    /// The server closed cleanly. Reconnect, but nothing went wrong.
    Closed,
}

impl FeedError {
    pub fn label(&self) -> &'static str {
        match self {
            FeedError::Connect(_) => "connect",
            FeedError::Subscribe(_) => "subscribe",
            FeedError::Stream(_) => "stream",
            FeedError::Exhausted(_) => "exhausted",
            FeedError::Closed => "closed",
        }
    }
}

impl fmt::Display for FeedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FeedError::Connect(m) => write!(f, "connect failed: {m}"),
            FeedError::Subscribe(m) => write!(f, "subscribe failed: {m}"),
            FeedError::Stream(m) => write!(f, "stream error: {m}"),
            FeedError::Exhausted(m) => write!(f, "provider exhausted: {m}"),
            FeedError::Closed => write!(f, "server closed the stream"),
        }
    }
}

impl std::error::Error for FeedError {}

// ── The seam ──────────────────────────────────────────────────────────────────

/// One transport = one wire. Constructed by the venue's assembly root, driven by
/// [`crate::supervisor::run`].
pub trait Feed: Send + Sync + 'static {
    type Conn: FeedConn;

    /// Short label for logs and metrics (`"laserstream"`, `"nats"`).
    fn name(&self) -> &'static str;

    fn caps(&self) -> FeedCaps;

    /// Open the wire and place the subscription. One call per connection
    /// attempt; the supervisor owns retry and backoff.
    fn connect(
        &self,
        sub: Subscription,
    ) -> impl Future<Output = Result<Self::Conn, FeedError>> + Send;
}

/// One live connection. Dropped by the supervisor to disconnect.
pub trait FeedConn: Send + 'static {
    /// The next update, or the error that ended this connection.
    ///
    /// Must be cancel-safe: the supervisor races this against the control
    /// watches and the idle tick, so the future is dropped whenever another
    /// branch wins. An update that has been *returned* is never lost; work in
    /// flight at an `await` point must be resumable.
    fn next(&mut self) -> impl Future<Output = Result<FeedUpdate, FeedError>> + Send;

    /// Apply a new filter set to the open connection.
    ///
    /// Only called when [`FeedCaps::in_place_resubscribe`] is true — a feed that
    /// cannot do it is reconnected instead, and may leave this `unimplemented`
    /// in spirit by returning [`FeedError::Closed`].
    fn resubscribe(
        &mut self,
        sub: Subscription,
    ) -> impl Future<Output = Result<(), FeedError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Backpressure policy is a consequence of replay, not a separate knob: a
    /// reconnect only helps if it can re-request what the stall cost.
    #[test]
    fn only_a_replaying_feed_reconnects_on_backpressure() {
        let grpc = FeedCaps {
            replay: true,
            server_filter: true,
            in_place_resubscribe: true,
        };
        let relay = FeedCaps {
            replay: false,
            server_filter: false,
            in_place_resubscribe: false,
        };
        assert!(grpc.reconnect_on_backpressure());
        assert!(!relay.reconnect_on_backpressure());
    }

    /// The scope constants are the four the assembly root hands out, and only
    /// `NONE` reads as "idle".
    #[test]
    fn only_the_empty_scope_is_empty() {
        assert!(StreamScope::NONE.is_empty());
        assert!(!StreamScope::ALL.is_empty());
        assert!(!StreamScope::CURVE.is_empty());
        assert!(!StreamScope::POOLS.is_empty());
    }
}
