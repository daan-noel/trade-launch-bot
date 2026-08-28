//! The assembly root — the one place that knows which wires exist.
//!
//! Feed selection lives here because this is the only crate that already depends
//! on every feed crate, which keeps the dependency graph acyclic (`ingest-nats`
//! → `ingest-core`, never the reverse) and keeps the engine free of transport
//! names. Adding a wire is a new crate plus one arm in [`spawn_feeds`].
//!
//! # What "switching the curve" actually is
//!
//! Not a mode, and not a pair of transport states. Each feed runs its own
//! [`ingest_core::supervisor::run`] against its own [`StreamScope`], and moving
//! the curve is one write to a `watch` channel that recomputes every feed's
//! scope.
//!
//! It is a **hand-over, not a cut-over**: the feed gaining the curve is widened
//! first and the one losing it is narrowed only [`HANDOVER`] later, so the chain
//! is never briefly owned by nobody. Both carry it in between and the shared
//! dedupe ring drops the copies. AMM pools never move at all, so open positions
//! keep their feed throughout.

use std::sync::Arc;
use std::time::Duration;

use ingest_core::feed::StreamScope;
use ingest_core::session::FeedLanes;
use ingest_core::supervisor::{self, FeedPolicy};
use ingest_core::IngestConfig;
use ingest_laserstream::{GrpcConfig, GrpcFeed};
use tokio::sync::watch;
use tracing::{info, warn};

use crate::venue::PumpFunVenue;

#[cfg(feature = "nats")]
use ingest_nats::{NatsConfig, NatsFeed};

/// Which wire a feed is. A label shared by the host, this module, and nobody
/// else — `ingest-core` never sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedKind {
    /// Yellowstone gRPC (LaserStream). Costs provider credits; replays a gap;
    /// filters server-side.
    Grpc,
    /// A third-party NATS relay broadcasting Helius `transactionNotification`
    /// frames. No credits, no replay, no filter control.
    Nats,
}

impl FeedKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FeedKind::Grpc => "grpc",
            FeedKind::Nats => "nats",
        }
    }
}

/// One feed's slice of the venue, given who owns the curve.
///
/// The whole selection rule, and it is O(feeds) rather than O(feeds × slices):
///
/// - **Curve** goes to exactly the feed the operator named.
/// - **AMM pools** go to whichever feeds can filter server-side, because you
///   cannot subscribe to one pool PDA on a broadcast subject. (If two such feeds
///   ever run at once both carry pools and the dedupe ring drops the copies.)
///
/// A new wire needs no arm here — only a `caps()` and a constructor.
fn scope_for(kind: FeedKind, curve: FeedKind, server_filter: bool) -> StreamScope {
    StreamScope {
        program: kind == curve,
        pools: server_filter,
    }
}

/// How long both feeds carry the curve during a switch.
///
/// Narrowing the old owner before the new one is actually delivering leaves a
/// hole: a relay connect + subscribe measures ~2.2 s, and cutting over on the
/// spot cost 7-10 slots of trades in each direction. Holding both for this long
/// covers that with headroom, and sits comfortably inside
/// [`IngestConfig::dedupe_window`] (30 s) - the ring is what makes the overlap
/// free, so the two must never cross.
const HANDOVER: Duration = Duration::from_secs(5);

/// Per-feed policy: the engine's neutral tunables plus that wire's own idea of
/// how long silence is allowed to last.
fn policy_of(cfg: &IngestConfig, idle_reconnect_timeout: Duration) -> Arc<FeedPolicy> {
    Arc::new(FeedPolicy {
        reconnect_base: cfg.reconnect_base,
        reconnect_max_backoff: cfg.reconnect_max_backoff,
        idle_reconnect_timeout,
        idle_check_interval: cfg.idle_check_interval,
        pipeline_send_timeout: cfg.pipeline_send_timeout,
        resubscribe_debounce: cfg.resubscribe_debounce,
        commitment: cfg.commitment,
    })
}

/// How many feeds [`spawn_feeds`] will run for this configuration.
///
/// The session needs this before it builds the lanes: more than one feed arms the
/// cross-feed dedupe ring, exactly one skips it entirely.
pub(crate) fn feed_count(nats_configured: bool) -> usize {
    1 + usize::from(nats_configured && cfg!(feature = "nats"))
}

/// Validate the requested curve owner against what was actually built.
///
/// Selecting a wire with nothing behind it would leave the curve feed silently
/// dead — the worst possible failure for an ingest path — so fall back to gRPC
/// loudly instead.
pub(crate) fn resolve_curve(curve: FeedKind, nats_configured: bool) -> FeedKind {
    if curve != FeedKind::Nats {
        return curve;
    }
    if !nats_configured {
        warn!("ingest: curve feed is nats but no relay was configured — using gRPC");
        return FeedKind::Grpc;
    }
    #[cfg(not(feature = "nats"))]
    {
        warn!("ingest: curve feed is nats but the `nats` feature is not compiled in — using gRPC");
        FeedKind::Grpc
    }
    #[cfg(feature = "nats")]
    {
        info!("ingest: bonding curve on the NATS relay; AMM pools stay on gRPC");
        FeedKind::Nats
    }
}

/// Spawn one supervisor per configured feed and return the channel that moves
/// the curve between them.
///
/// A feed that is configured but not selected is spawned anyway: it idles fully
/// disconnected (costing no bandwidth) until its scope becomes non-empty, which
/// is what makes the switch instant and restart-free.
pub(crate) fn spawn_feeds(
    venue: Arc<PumpFunVenue>,
    lanes: FeedLanes<PumpFunVenue>,
    cfg: &IngestConfig,
    grpc_cfg: GrpcConfig,
    #[cfg(feature = "nats")] nats_cfg: Option<NatsConfig>,
    curve: FeedKind,
) -> watch::Sender<FeedKind> {
    let (curve_tx, curve_rx) = watch::channel(curve);
    // (kind, server_filter, scope sender) for every feed spawned below — the
    // translator task recomputes each scope whenever the curve owner moves.
    let mut scopes: Vec<(FeedKind, bool, watch::Sender<StreamScope>)> = Vec::new();

    // ── gRPC: always present. It is the only wire that can filter for a pool
    //    PDA, so AMM traffic has nowhere else to go.
    {
        let feed = GrpcFeed::new(grpc_cfg);
        let policy = policy_of(cfg, feed.idle_reconnect_timeout());
        let server_filter = ingest_laserstream::CAPS.server_filter;
        let (tx, rx) = watch::channel(scope_for(FeedKind::Grpc, curve, server_filter));
        tokio::spawn(supervisor::run(feed, venue.clone(), lanes.clone(), rx, policy));
        scopes.push((FeedKind::Grpc, server_filter, tx));
    }

    // ── NATS relay: spawned whenever it is *configured*, not only when selected.
    #[cfg(feature = "nats")]
    if let Some(nats_cfg) = nats_cfg {
        let feed = NatsFeed::new(nats_cfg);
        let policy = policy_of(cfg, feed.idle_reconnect_timeout());
        let server_filter = ingest_nats::CAPS.server_filter;
        let (tx, rx) = watch::channel(scope_for(FeedKind::Nats, curve, server_filter));
        tokio::spawn(supervisor::run(feed, venue.clone(), lanes.clone(), rx, policy));
        scopes.push((FeedKind::Nats, server_filter, tx));
    }

    // Translator: one write to `curve_tx` becomes one scope per feed, widen
    // before narrow. This is the entire switch mechanism.
    {
        let mut curve_rx = curve_rx;
        tokio::spawn(async move {
            let mut current = curve;
            loop {
                let next = *curve_rx.borrow_and_update();
                if next == current {
                    if curve_rx.changed().await.is_err() {
                        return;
                    }
                    continue;
                }
                info!(
                    from = current.as_str(),
                    to = next.as_str(),
                    "ingest: handing the curve over"
                );

                // 1. Widen first: whoever is gaining the curve gets it now, and
                //    starts connecting while the current owner is still serving.
                for (kind, server_filter, tx) in &scopes {
                    let scope = scope_for(*kind, next, *server_filter);
                    if scope.program {
                        let _ = tx.send(scope);
                    }
                }

                // 2. Hold both. A further switch during the window restarts the
                //    hand-over rather than narrowing against a stale target.
                let interrupted = tokio::select! {
                    _ = tokio::time::sleep(HANDOVER) => false,
                    r = curve_rx.changed() => {
                        if r.is_err() { return; }
                        true
                    }
                };
                if interrupted {
                    continue;
                }

                // 3. Narrow last: the new owner has had the whole window to come
                //    up, so dropping the curve here costs nothing.
                for (kind, server_filter, tx) in &scopes {
                    let scope = scope_for(*kind, next, *server_filter);
                    if !scope.program {
                        let _ = tx.send(scope);
                    }
                }
                current = next;
                info!(curve = next.as_str(), "ingest: curve feed changed");
            }
        });
    }

    curve_tx
}

#[cfg(test)]
mod tests {
    use super::*;

    /// gRPC alone: it carries everything, and the relay (if built) idles.
    #[test]
    fn the_curve_owner_gets_the_program_and_pools_follow_the_filter() {
        assert_eq!(
            scope_for(FeedKind::Grpc, FeedKind::Grpc, true),
            StreamScope::ALL
        );
        assert_eq!(
            scope_for(FeedKind::Nats, FeedKind::Grpc, false),
            StreamScope::NONE
        );
    }

    /// Curve on the relay: gRPC drops the program id (or the provider is paid
    /// for it twice) and keeps the pools it is the only wire able to filter for.
    #[test]
    fn moving_the_curve_to_the_relay_leaves_pools_on_grpc() {
        assert_eq!(
            scope_for(FeedKind::Grpc, FeedKind::Nats, true),
            StreamScope::POOLS
        );
        assert_eq!(
            scope_for(FeedKind::Nats, FeedKind::Nats, false),
            StreamScope::CURVE
        );
    }

    /// Selecting a wire with nothing behind it must never leave the curve dead.
    #[test]
    fn an_unconfigured_relay_falls_back_to_grpc() {
        assert_eq!(resolve_curve(FeedKind::Nats, false), FeedKind::Grpc);
        assert_eq!(resolve_curve(FeedKind::Grpc, true), FeedKind::Grpc);
    }

    /// The hand-over deliberately runs both feeds on the curve at once, so the
    /// dedupe window has to outlast it or the overlap would double-count into
    /// the live volume/flow metrics.
    #[test]
    fn the_handover_fits_inside_the_dedupe_window() {
        assert!(
            HANDOVER < IngestConfig::default().dedupe_window,
            "the ring is what makes the overlap free"
        );
    }

    /// The dedupe ring costs a hash per transaction, so it is armed only when a
    /// second feed can actually deliver a duplicate.
    #[test]
    fn dedupe_is_armed_only_when_a_second_feed_exists() {
        assert_eq!(feed_count(false), 1);
        assert_eq!(feed_count(true), if cfg!(feature = "nats") { 2 } else { 1 });
    }
}
