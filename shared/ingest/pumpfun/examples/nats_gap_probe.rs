//! Gap diagnostic for the NATS curve feed — where a no-trade hole comes from.
//!
//! Mirrors the production shape exactly (reader task -> bounded queue -> parser
//! task, shed on full) so the numbers it reports are the numbers the live feed
//! sees. Answers, in one run, which of the four gap sources is active:
//!
//! 1. **The socket went quiet** — arrival gaps, i.e. relay or network.
//! 2. **The reader shed** — the parser could not keep up on one core.
//! 3. **The parser fell behind** — arrival -> parse lag, which shifts the
//!    `received_at` a trade is stamped with and moves prints on the chart.
//! 4. **The converter rejected the frame** — per-stage rejection counts.
//!
//! Slot coverage is the ground truth: the bonding curve trades in essentially
//! every slot, so a missing slot inside the observed range is lost data.
//!
//! ```powershell
//! $env:NATS_URL="nats://3.78.182.30:4222"
//! cargo run -p ingest-pumpfun --features nats --example nats_gap_probe -- 60
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use ingest_core::config::IngestConfig;
use ingest_core::convert;
use ingest_core::nats::NatsConn;
use ingest_core::venue::IngestVenue;
use ingest_laserstream::{protocol::Protocol, venue::PumpFunVenue};
use tokio::sync::mpsc;

/// Same default the live `NatsConfig` uses.
const FRAME_CHANNEL_CAP: usize = 8192;

fn pct(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[i]
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let secs: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://3.78.182.30:4222".into());
    let subject =
        std::env::var("NATS_SUBJECT").unwrap_or_else(|_| "helius.raw.bondingcurve".into());

    println!("connecting to {url}, subject {subject}, sampling {secs}s");
    let mut conn = match NatsConn::connect(&url, "nats-gap-probe", Duration::from_secs(10)).await {
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

    let t0 = Instant::now();
    let deadline = t0 + Duration::from_secs(secs);

    // -- reader task: production shape - socket -> bounded queue, shed on full --
    let (frame_tx, mut frame_rx) = mpsc::channel::<(Instant, Vec<u8>)>(FRAME_CHANNEL_CAP);
    let reader = tokio::spawn(async move {
        let mut shed = 0u64;
        let mut gaps: Vec<(f64, f64)> = Vec::new();
        let mut last = Instant::now();
        let mut err: Option<String> = None;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            let payload = match tokio::time::timeout(left, conn.next_message()).await {
                Ok(Ok(p)) => p,
                Ok(Err(e)) => {
                    err = Some(format!("{e}"));
                    break;
                }
                Err(_) => break,
            };
            let now = Instant::now();
            let gap = now.duration_since(last).as_secs_f64() * 1000.0;
            if gap > 400.0 {
                gaps.push((now.duration_since(t0).as_secs_f64(), gap));
            }
            last = now;
            if frame_tx.try_send((now, payload)).is_err() {
                shed += 1;
            }
        }
        (shed, gaps, err)
    });

    // -- parser task: exactly what handle_frame does, on one core --------------
    let venue = PumpFunVenue::new(Protocol::pump_fun(), &IngestConfig::default());

    let (mut frames, mut bytes) = (0u64, 0u64);
    let (mut not_json, mut no_result, mut failed, mut unconvertible, mut irrelevant) =
        (0u64, 0u64, 0u64, 0u64, 0u64);
    let mut relevant = 0u64;
    let mut encodings: BTreeMap<&str, u64> = BTreeMap::new();
    let mut slots: BTreeSet<u64> = BTreeSet::new();
    let mut slot_frames: BTreeMap<u64, u64> = BTreeMap::new();
    let mut lag_ms: Vec<f64> = Vec::new();
    let mut parse_us: Vec<f64> = Vec::new();
    let mut sigs: BTreeSet<Vec<u8>> = BTreeSet::new();
    let mut delivered: Vec<(u64, Vec<u8>)> = Vec::new();
    let mut dupes = 0u64;

    while let Some((arrived, payload)) = frame_rx.recv().await {
        frames += 1;
        bytes += payload.len() as u64;
        let started = Instant::now();
        lag_ms.push(started.duration_since(arrived).as_secs_f64() * 1000.0);

        let Ok(envelope) = serde_json::from_slice::<serde_json::Value>(&payload) else {
            not_json += 1;
            continue;
        };
        let result = envelope
            .get("params")
            .and_then(|p| p.get("result"))
            .or_else(|| envelope.get("result"))
            .unwrap_or(&envelope);
        if result.get("transaction").is_none() {
            no_result += 1;
            continue;
        }

        let enc = match result
            .get("transaction")
            .and_then(|t| t.get("transaction"))
            .map(|t| t.is_array())
        {
            Some(true) => "base64",
            Some(false) => "jsonParsed",
            None => "?",
        };
        *encodings.entry(enc).or_default() += 1;

        if convert::json_tx_failed(result) {
            failed += 1;
            parse_us.push(started.elapsed().as_secs_f64() * 1e6);
            continue;
        }
        let Some(update) = convert::json_tx_to_protobuf(result) else {
            unconvertible += 1;
            parse_us.push(started.elapsed().as_secs_f64() * 1e6);
            continue;
        };
        parse_us.push(started.elapsed().as_secs_f64() * 1e6);

        slots.insert(update.slot);
        *slot_frames.entry(update.slot).or_default() += 1;
        if let Some(t) = update.transaction.as_ref() {
            delivered.push((update.slot, t.signature.clone()));
            if !sigs.insert(t.signature.clone()) {
                dupes += 1;
            }
        }

        if venue.classify(&update).is_some() {
            relevant += 1;
        } else {
            irrelevant += 1;
        }
    }

    let (shed, gaps, read_err) = reader.await.unwrap();
    let elapsed = t0.elapsed().as_secs_f64();

    println!("\n================ ARRIVAL (socket) ================");
    println!(
        "frames        {frames}  ({:.0}/s, {:.1} KB avg)",
        frames as f64 / elapsed,
        bytes as f64 / frames.max(1) as f64 / 1024.0
    );
    println!("shed          {shed}   (reader could not hand off - parser is the bottleneck)");
    if let Some(e) = read_err {
        println!("READ ERROR    {e}   <-- the connection died mid-sample");
    }
    println!("arrival gaps  {} over 400ms", gaps.len());
    for (at, ms) in gaps.iter().take(15) {
        println!("                t+{at:7.2}s  quiet for {ms:8.1} ms");
    }

    println!("\n================ PARSER (one core) ================");
    lag_ms.sort_by(f64::total_cmp);
    parse_us.sort_by(f64::total_cmp);
    println!(
        "queue lag ms  p50 {:.1}  p90 {:.1}  p99 {:.1}  max {:.1}",
        pct(&lag_ms, 0.50),
        pct(&lag_ms, 0.90),
        pct(&lag_ms, 0.99),
        pct(&lag_ms, 1.0)
    );
    println!(
        "parse us      p50 {:.0}  p90 {:.0}  p99 {:.0}  max {:.0}",
        pct(&parse_us, 0.50),
        pct(&parse_us, 0.90),
        pct(&parse_us, 0.99),
        pct(&parse_us, 1.0)
    );
    let total_parse_s: f64 = parse_us.iter().sum::<f64>() / 1e6;
    println!(
        "core busy     {:.1}%  ({total_parse_s:.1}s of parsing in {elapsed:.1}s)",
        total_parse_s / elapsed * 100.0
    );
    println!("encodings     {encodings:?}");

    println!("\n================ REJECTIONS ================");
    println!("not json      {not_json}");
    println!("no result     {no_result}   (envelope shape not recognised - SILENT in prod)");
    println!("failed tx     {failed}   (screened, as gRPC does server-side)");
    println!("unconvertible {unconvertible}   (converter rejected the frame)");
    println!("irrelevant    {irrelevant}   (classified as not pump.fun)");
    println!("relevant      {relevant}");
    println!("duplicate sig {dupes}");

    println!("\n================ SLOT COVERAGE (ground truth) ================");
    if let (Some(&lo), Some(&hi)) = (slots.iter().next(), slots.iter().next_back()) {
        let span = (hi - lo + 1) as usize;
        let missing: Vec<u64> = (lo..=hi).filter(|s| !slots.contains(s)).collect();
        println!(
            "slots         {lo}..{hi}  span {span}, seen {}, MISSING {}",
            slots.len(),
            missing.len()
        );
        let (mut best, mut best_at, mut run, mut run_at) = (0usize, 0u64, 0usize, 0u64);
        for s in lo..=hi {
            if slots.contains(&s) {
                run = 0;
            } else {
                if run == 0 {
                    run_at = s;
                }
                run += 1;
                if run > best {
                    best = run;
                    best_at = run_at;
                }
            }
        }
        println!(
            "longest hole  {best} slots (~{:.1}s) starting at slot {best_at}",
            best as f64 * 0.4
        );
        let head: Vec<u64> = missing.iter().take(40).copied().collect();
        println!("missing (40)  {head:?}");
        let per: Vec<u64> = slot_frames.values().copied().collect();
        let mut per_sorted = per.clone();
        per_sorted.sort_unstable();
        println!(
            "tx per slot   p50 {}  p90 {}  max {}",
            per_sorted.get(per_sorted.len() / 2).copied().unwrap_or(0),
            per_sorted
                .get(per_sorted.len() * 9 / 10)
                .copied()
                .unwrap_or(0),
            per_sorted.last().copied().unwrap_or(0)
        );
        let thin = per.iter().filter(|&&n| n <= 1).count();
        println!("thin slots    {thin} slots carried <=1 tx");
    } else {
        println!("no slots decoded at all");
    }

    // Per-slot signature sets, for diffing against `getBlock` ground truth.
    if let Ok(path) = std::env::var("DUMP_PATH") {
        let mut out = String::new();
        for (slot, sig) in &delivered {
            out.push_str(&format!("{slot} {}\n", bs58::encode(sig).into_string()));
        }
        std::fs::write(&path, out).expect("dump");
        println!("\ndumped {} slot/signature pairs to {path}", delivered.len());
    }
}
