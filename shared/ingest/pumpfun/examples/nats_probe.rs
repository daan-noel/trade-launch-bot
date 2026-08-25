//! End-to-end probe for the NATS curve feed: relay -> convert -> classify ->
//! decode, with no database, no session, and no host wiring.
//!
//! Exercises exactly the path the live ingest takes, so it answers the only
//! question that matters before switching a bot over: does this relay's wire
//! format actually decode into pump.fun trades?
//!
//! ```powershell
//! $env:NATS_URL="nats://3.78.182.30:4222"
//! $env:NATS_SUBJECT="helius.raw.bondingcurve"
//! cargo run -p ingest-pumpfun --features nats --example nats_probe -- 20
//! ```
//!
//! The positional argument is how many seconds to sample (default 20).

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use ingest_core::config::IngestConfig;
use ingest_core::convert;
use ingest_core::nats::NatsConn;
use ingest_core::venue::{DecodeOutput, IngestVenue};
use ingest_laserstream::{protocol::Protocol, venue::PumpFunVenue};

#[tokio::main]
async fn main() {
    let secs: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);
    let url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".into());
    let subject =
        std::env::var("NATS_SUBJECT").unwrap_or_else(|_| "helius.raw.bondingcurve".into());

    println!("connecting to {url}, subject {subject}, sampling {secs}s");

    let mut conn = match NatsConn::connect(&url, "nats-probe", Duration::from_secs(10)).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("connect failed: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "server {} v{} (max_payload {})",
        conn.info().server_name,
        conn.info().version,
        conn.info().max_payload
    );
    if let Err(e) = conn.subscribe(&subject, None, 1).await {
        eprintln!("subscribe failed: {e}");
        std::process::exit(1);
    }

    let venue = PumpFunVenue::new(Protocol::pump_fun(), &IngestConfig::default());

    let (mut frames, mut bytes, mut failed, mut unconvertible, mut irrelevant) = (0u64, 0u64, 0u64, 0u64, 0u64);
    let mut relevance: BTreeMap<String, u64> = BTreeMap::new();
    let mut events: BTreeMap<String, u64> = BTreeMap::new();
    let mut slots: Vec<u64> = Vec::new();
    let mut samples: Vec<String> = Vec::new();

    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        let left = deadline.saturating_duration_since(Instant::now());
        let payload = match tokio::time::timeout(left, conn.next_message()).await {
            Ok(Ok(p)) => p,
            Ok(Err(e)) => {
                eprintln!("read failed after {frames} frames: {e}");
                break;
            }
            Err(_) => break,
        };
        frames += 1;
        bytes += payload.len() as u64;

        let Ok(envelope) = serde_json::from_slice::<serde_json::Value>(&payload) else {
            unconvertible += 1;
            continue;
        };
        let result = envelope
            .get("params")
            .and_then(|p| p.get("result"))
            .unwrap_or(&envelope);

        if convert::json_tx_failed(result) {
            failed += 1;
            continue;
        }
        let Some(update) = convert::json_tx_to_protobuf(result) else {
            unconvertible += 1;
            continue;
        };
        slots.push(update.slot);

        let Some(rel) = venue.classify(&update) else {
            irrelevant += 1;
            continue;
        };
        *relevance.entry(format!("{rel:?}")).or_default() += 1;

        if let DecodeOutput::Events(v) = venue.decode(&update, rel, chrono::Utc::now()) {
            for ev in v {
                // Variant name only - the payload is large and not the point here.
                let dbg = format!("{ev:?}");
                let name = dbg.split(['(', ' ']).next().unwrap_or("?").to_string();
                *events.entry(name).or_default() += 1;
                if samples.len() < 3 && !dbg.starts_with("RawTx") {
                    samples.push(dbg.chars().take(400).collect());
                }
            }
        }
    }

    println!("\n================ RESULT ================");
    println!("frames        {frames}  ({:.1} KB avg)", bytes as f64 / frames.max(1) as f64 / 1024.0);
    println!("failed tx     {failed}   (screened, as gRPC does server-side)");
    println!("unconvertible {unconvertible}");
    println!("irrelevant    {irrelevant}   (classified as not pump.fun)");
    println!("classified    {relevance:?}");
    println!("decoded       {events:?}");
    if let (Some(lo), Some(hi)) = (slots.iter().min(), slots.iter().max()) {
        println!("slots         {lo}..{hi}  (span {})", hi - lo);
    }
    for s in &samples {
        println!("\nsample: {s}");
    }

    let converted = frames - unconvertible - failed;
    println!(
        "\nconversion rate {:.1}%  |  relevance rate {:.1}%",
        converted as f64 / frames.max(1) as f64 * 100.0,
        relevance.values().sum::<u64>() as f64 / converted.max(1) as f64 * 100.0
    );
}
