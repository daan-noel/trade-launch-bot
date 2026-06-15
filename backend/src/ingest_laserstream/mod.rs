//! LaserStream (Yellowstone gRPC) ingest — the live ingest transport. Per
//! project convention these modules keep a self-contained copy of the ingest
//! logic (decoder, pipeline, db_writer) rather than importing from `ingest/`;
//! only live shared state (token cache, trader, strategy/SSE channels, repos,
//! constants) is reused. The `ingest/` module now retains only the pieces shared
//! with `services::token_sync` (the decoder + `TokenMetricsWrite`).
#![allow(dead_code)]

// Committed prost/tonic bindings generated from `proto/geyser.proto` (+
// solana-storage.proto). There is no build-time protoc step; regenerate via the
// documented Docker one-shot only if the `.proto` files change.
#[allow(clippy::all)]
#[rustfmt::skip]
pub mod proto {
    include!("generated/mod.rs");
}

pub mod adapter;
pub mod adapter_rpc;
pub mod client;
pub mod db_writer;
pub mod decoder;
pub mod maintenance;
pub mod pipeline;
