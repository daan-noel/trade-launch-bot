//! The Yellowstone (LaserStream) gRPC ingest feed.
//!
//! One wire, and nothing else: connect with `x-token`, place a `Subscribe`, pull
//! `SubscribeUpdate`s, resubscribe in place. Every policy that used to sit
//! alongside it — the reconnect ramp, the replay anchor, the idle guard, the
//! decode lanes — belongs to `ingest-core`'s supervisor, which drives this
//! through [`ingest_core::feed::Feed`].
//!
//! **Knows no venue.** The accounts to watch, the filter-map key and the
//! classify all arrive inside a neutral [`Subscription`].
//!
//! What this wire can do that a broadcast relay cannot, and what the supervisor
//! reads off [`CAPS`]: it replays from a slot, it applies our account filter
//! server-side, and it accepts a new filter on the open stream.

pub mod client;
pub mod subscribe;

use std::time::Duration;

use ingest_core::feed::{
    Feed, FeedCaps, FeedConn, FeedError, FeedUpdate, Subscription,
};
use ingest_core::proto::geyser::subscribe_update::UpdateOneof;
use ingest_core::proto::geyser::{SubscribeRequest, SubscribeUpdate};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::metadata::{Ascii, MetadataValue};
use tonic::service::interceptor::InterceptedService;
use tonic::service::Interceptor;
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};
use tonic::{Request, Status, Streaming};

use client::geyser_client::GeyserClient;
pub use subscribe::{build_subscribe_request, commitment_level, request_of};

/// Outbound request queue depth. Only ever holds the initial subscribe plus a
/// resubscribe or two.
const REQUEST_QUEUE_CAP: usize = 16;

/// What this wire can do. Read by the supervisor instead of naming the transport.
pub const CAPS: FeedCaps = FeedCaps {
    replay: true,
    server_filter: true,
    in_place_resubscribe: true,
};

// ── Provider-as-config ────────────────────────────────────────────────────────

/// Transport authentication for the Yellowstone provider.
///
/// Every Yellowstone provider (Helius, Triton, Shyft, a self-hosted geyser)
/// speaks the same wire protocol and differs only in endpoint + auth. Swapping
/// providers is therefore a config change — no new crate.
#[derive(Debug, Clone)]
pub enum Auth {
    /// `x-token` metadata header (Helius / Triton / Shyft).
    XToken(String),
    /// No auth (self-hosted / local validator geyser).
    None,
}

/// Everything this wire needs and no other wire has.
///
/// Deliberately separate from `ingest_core::IngestConfig`: an HTTP/2 keepalive
/// is not a property of ingest, it is a property of gRPC, and a knob that lives
/// in the engine is a knob every future transport has to pretend to honour.
#[derive(Debug, Clone)]
pub struct GrpcConfig {
    pub endpoint: String,
    pub auth: Auth,
    /// TCP/TLS connect timeout.
    pub connect_timeout: Duration,
    /// HTTP/2 keepalive interval.
    pub http2_keepalive: Duration,
    /// TCP keepalive interval.
    pub tcp_keepalive: Duration,
    /// Max gRPC message size (bytes).
    pub max_decoding_message_size: usize,
    /// How long this subscription may go silent before the supervisor forces a
    /// reconnect. Short, because a Yellowstone subscription that includes the
    /// venue program is a firehose.
    pub idle_reconnect_timeout: Duration,
}

impl Default for GrpcConfig {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            auth: Auth::None,
            connect_timeout: Duration::from_secs(10),
            http2_keepalive: Duration::from_secs(30),
            tcp_keepalive: Duration::from_secs(30),
            max_decoding_message_size: 64 * 1024 * 1024,
            idle_reconnect_timeout: Duration::from_secs(10),
        }
    }
}

// ── Connection ────────────────────────────────────────────────────────────────

pub type LaserStreamClient = GeyserClient<InterceptedService<Channel, XTokenInterceptor>>;

/// Yellowstone auth interceptor. Inserts the `x-token` header when the provider
/// [`Auth`] carries one; a no-auth provider (self-hosted geyser) inserts nothing.
#[derive(Clone)]
pub struct XTokenInterceptor {
    token: Option<MetadataValue<Ascii>>,
}

impl Interceptor for XTokenInterceptor {
    fn call(&mut self, mut req: Request<()>) -> Result<Request<()>, Status> {
        if let Some(token) = &self.token {
            req.metadata_mut().insert("x-token", token.clone());
        }
        Ok(req)
    }
}

/// Dial a Yellowstone endpoint. Public because the one-shot replay service opens
/// its own short-lived stream outside any `Feed`.
pub async fn connect(cfg: &GrpcConfig) -> Result<LaserStreamClient, FeedError> {
    let token: Option<MetadataValue<Ascii>> = match &cfg.auth {
        Auth::XToken(key) => Some(
            key.parse()
                .map_err(|_| FeedError::Connect("API key is not a valid gRPC metadata value".into()))?,
        ),
        Auth::None => None,
    };

    let channel = Endpoint::from_shared(cfg.endpoint.clone())
        .map_err(|e| FeedError::Connect(format!("invalid endpoint: {e}")))?
        .tls_config(ClientTlsConfig::new())
        .map_err(|e| FeedError::Connect(e.to_string()))?
        .connect_timeout(cfg.connect_timeout)
        .http2_keep_alive_interval(cfg.http2_keepalive)
        .keep_alive_while_idle(true)
        .tcp_keepalive(Some(cfg.tcp_keepalive))
        .connect()
        .await
        .map_err(|e| FeedError::Connect(e.to_string()))?;

    Ok(
        GeyserClient::with_interceptor(channel, XTokenInterceptor { token })
            .max_decoding_message_size(cfg.max_decoding_message_size),
    )
}

/// A gRPC `Status` in the terms the reconnect policy is written in.
///
/// `ResourceExhausted` is the billing-shaped one: the provider is refusing us
/// for capacity reasons, and replaying into it re-causes it.
fn feed_error_of(status: Status) -> FeedError {
    if status.code() == tonic::Code::ResourceExhausted {
        FeedError::Exhausted(status.to_string())
    } else {
        FeedError::Stream(status.to_string())
    }
}

// ── The feed ──────────────────────────────────────────────────────────────────

/// The LaserStream gRPC feed.
pub struct GrpcFeed {
    cfg: GrpcConfig,
}

impl GrpcFeed {
    pub fn new(cfg: GrpcConfig) -> Self {
        Self { cfg }
    }

    /// This feed's idle allowance, for the supervisor's `FeedPolicy`.
    pub fn idle_reconnect_timeout(&self) -> Duration {
        self.cfg.idle_reconnect_timeout
    }
}

impl Feed for GrpcFeed {
    type Conn = GrpcConn;

    fn name(&self) -> &'static str {
        "laserstream"
    }

    fn caps(&self) -> FeedCaps {
        CAPS
    }

    async fn connect(&self, sub: Subscription) -> Result<Self::Conn, FeedError> {
        let mut client = connect(&self.cfg).await?;

        let (req_tx, req_rx) = mpsc::channel::<SubscribeRequest>(REQUEST_QUEUE_CAP);
        if req_tx.send(request_of(sub)).await.is_err() {
            return Err(FeedError::Subscribe(
                "request channel closed before the initial subscribe".into(),
            ));
        }

        let response = client
            .subscribe(ReceiverStream::new(req_rx))
            .await
            .map_err(feed_error_of)?;

        Ok(GrpcConn {
            stream: response.into_inner(),
            req_tx,
        })
    }
}

/// One open `Subscribe` stream plus the request channel that feeds it.
pub struct GrpcConn {
    stream: Streaming<SubscribeUpdate>,
    req_tx: mpsc::Sender<SubscribeRequest>,
}

impl FeedConn for GrpcConn {
    async fn next(&mut self) -> Result<FeedUpdate, FeedError> {
        match self.stream.message().await {
            Ok(Some(update)) => Ok(match update.update_oneof {
                Some(UpdateOneof::Transaction(tx)) => FeedUpdate::Transaction(tx),
                Some(UpdateOneof::BlockMeta(meta)) => FeedUpdate::BlockMeta {
                    slot: meta.slot,
                    blockhash: meta.blockhash,
                    block_time: meta.block_time.map(|t| t.timestamp),
                },
                Some(UpdateOneof::Account(acc)) => match acc.account {
                    Some(info) => FeedUpdate::Account {
                        slot: acc.slot,
                        pubkey: bs58::encode(&info.pubkey).into_string(),
                        lamports: info.lamports,
                        data: info.data,
                    },
                    None => FeedUpdate::Tick,
                },
                // Slots, pings, everything else: liveness evidence only.
                _ => FeedUpdate::Tick,
            }),
            Ok(None) => Err(FeedError::Closed),
            Err(status) => Err(feed_error_of(status)),
        }
    }

    /// Send a fresh `SubscribeRequest` down the open request stream. Yellowstone
    /// applies it in place, so a filter change costs no reconnect and leaves no
    /// gap on the traffic that is not changing.
    async fn resubscribe(&mut self, sub: Subscription) -> Result<(), FeedError> {
        self.req_tx
            .send(request_of(sub))
            .await
            .map_err(|_| FeedError::Closed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Provider-as-config: the `x-token` header is inserted only when the
    /// provider [`Auth`] carries one, so a no-auth (self-hosted) provider is a
    /// pure config swap — no crate change.
    #[test]
    fn interceptor_inserts_x_token_only_when_present() {
        let mut with = XTokenInterceptor {
            token: Some("secret".parse().unwrap()),
        };
        let req = with.call(Request::new(())).unwrap();
        assert!(req.metadata().get("x-token").is_some());

        let mut without = XTokenInterceptor { token: None };
        let req = without.call(Request::new(())).unwrap();
        assert!(req.metadata().get("x-token").is_none());
    }

    /// Swapping Helius → Triton/Shyft → self-hosted is only different data
    /// (endpoint + `Auth`); it type-checks with no change to this crate.
    #[test]
    fn provider_swap_is_config() {
        let _helius = GrpcConfig {
            endpoint: "https://mainnet.helius-rpc.com".into(),
            auth: Auth::XToken("k".into()),
            ..GrpcConfig::default()
        };
        let _triton = GrpcConfig {
            endpoint: "https://grpc.triton.one".into(),
            auth: Auth::XToken("k2".into()),
            ..GrpcConfig::default()
        };
        let _selfhosted = GrpcConfig {
            endpoint: "http://localhost:10000".into(),
            auth: Auth::None,
            ..GrpcConfig::default()
        };
    }

    /// A provider shedding us for capacity reasons must reach the supervisor as
    /// the billing-shaped variant — it is the one reason that forbids a replay.
    #[test]
    fn resource_exhausted_is_the_billing_shaped_error() {
        let exhausted = feed_error_of(Status::resource_exhausted("slow down"));
        assert!(matches!(exhausted, FeedError::Exhausted(_)));
        let ordinary = feed_error_of(Status::unavailable("try again"));
        assert!(matches!(ordinary, FeedError::Stream(_)));
    }

    /// The capability set is what the supervisor branches on, so it is part of
    /// this crate's contract, not an implementation detail.
    #[test]
    fn this_wire_replays_filters_and_resubscribes_in_place() {
        assert!(CAPS.replay);
        assert!(CAPS.server_filter);
        assert!(CAPS.in_place_resubscribe);
        assert!(CAPS.reconnect_on_backpressure());
    }
}
