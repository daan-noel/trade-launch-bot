//! LaserStream (Yellowstone gRPC) ingest — self-contained clone of the WS ingest
//! transport, built in parallel with `ingest/` and selected via the
//! `INGEST_TRANSPORT` setting. Per project convention these modules duplicate
//! ingest logic rather than importing from `ingest/`; only live shared state
//! (token cache, trader, strategy/SSE channels, repos, constants) is reused.
//!
//! Status: scaffolding (step 1) — committed Geyser protobuf bindings + a gRPC
//! connect skeleton. Adapter / decoder clone / pipeline land in later steps.
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
pub mod client;
pub mod db_writer;
pub mod decoder;
pub mod pipeline;
