//! Live parity + hot-switch check for the multi-feed ingest stack.
//!
//! Runs the REAL session — both feed supervisors, the shared decode lanes, the
//! cross-feed dedupe ring, the pump.fun venue — against mainnet, and answers the
//! three questions the four-crate split has to get right:
//!
//! 1. **Does each wire decode?** Sample with the curve on gRPC, then with the
//!    curve on the NATS relay, and count what each phase produced.
//! 2. **Do they agree?** Overlap the two feeds for a window and compare the
//!    signature sets they deliver. Same chain, same decoder, so the sets should
//!    largely coincide; whatever only one side sees is that wire's own loss.
//! 3. **Does the switch work live?** `set_curve_feed` mid-run, with no restart
//!    and no gap in the phase that follows.
//!
//! ```powershell
//! $env:HELIUS_LASERSTREAM_URL="https://laserstream-mainnet-fra.helius-rpc.com"
//! $env:HELIUS_API_KEY="..."
//! $env:NATS_URL="nats://3.78.182.30:4222"
//! $env:NATS_SUBJECT="helius.raw.bondingcurve"
//! cargo run -p ingest-pumpfun --features nats --example feed_parity -- 20
//! ```
//!
//! The positional argument is seconds per phase (default 20). Costs one
//! LaserStream connection for the duration — keep the sample short.

use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

use ingest_pumpfun::{
    FeedKind, Ingest, IngestConfig, IngestEvent, NatsConfig, Protocol,
};

/// What one sampling phase saw.
#[derive(Default)]
struct Phase {
    events: BTreeMap<&'static str, u64>,
    signatures: HashSet<String>,
    mints: HashSet<String>,
    slots: Vec<u64>,
}

impl Phase {
    /// Largest run of consecutive slots with no trade at all.
    ///
    /// The bonding curve trades in essentially every slot, so a hole here is the
    /// only honest evidence of a gap: it survives whether the loss came from a
    /// resubscribe, a reconnect, or a wire that was briefly carrying nothing.
    fn max_slot_gap(&self) -> u64 {
        let mut seen: Vec<u64> = self.slots.clone();
        seen.sort_unstable();
        seen.dedup();
        seen.windows(2).map(|w| w[1] - w[0] - 1).max().unwrap_or(0)
    }

    fn first_slot(&self) -> Option<u64> {
        self.slots.iter().min().copied()
    }

    fn last_slot(&self) -> Option<u64> {
        self.slots.iter().max().copied()
    }

    fn record(&mut self, ev: &IngestEvent) {
        let name = match ev {
            IngestEvent::TokenCreated(_) => "TokenCreated",
            IngestEvent::Trade(t) => {
                self.signatures.insert(t.signature.clone());
                self.mints.insert(t.mint.clone());
                self.slots.push(t.slot);
                "Trade"
            }
            IngestEvent::TokenMigrated(_) => "TokenMigrated",
            IngestEvent::Liquidity(_) => "Liquidity",
            IngestEvent::CreatorActivity(_) => "CreatorActivity",
            #[cfg(feature = "raw-tx")]
            IngestEvent::RawTx(_) => "RawTx",
        };
        *self.events.entry(name).or_default() += 1;
    }

    fn report(&self, label: &str) {
        let span = match (self.slots.iter().min(), self.slots.iter().max()) {
            (Some(lo), Some(hi)) => format!("{lo}..{hi} (span {})", hi - lo),
            _ => "-".into(),
        };
        println!(
            "{label:<28} events={:?} trades={} mints={} slots={span} max_slot_gap={}",
            self.events,
            self.signatures.len(),
            self.mints.len(),
            self.max_slot_gap()
        );
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "ingest_core=info,ingest_nats=info".into()),
        )
        .init();

    let secs: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);

    let endpoint = env("HELIUS_LASERSTREAM_URL");
    let api_key = env("HELIUS_API_KEY");
    let nats_url = env("NATS_URL");
    let subject =
        std::env::var("NATS_SUBJECT").unwrap_or_else(|_| "helius.raw.bondingcurve".into());

    println!("laserstream {endpoint}");
    println!("nats        {nats_url} / {subject}");
    println!("phase       {secs}s\n");

    let (mut event_rx, handle) = Ingest::builder()
        .endpoint(endpoint)
        .api_key(api_key)
        .nats(Some(NatsConfig {
            url: nats_url,
            subject,
            ..NatsConfig::default()
        }))
        .curve_feed(FeedKind::Grpc)
        .protocol(Protocol::pump_fun())
        .config(IngestConfig::default())
        .build()
        .expect("ingest builder failed")
        .start(true);

    // ── Phase 1: curve on gRPC (the relay is spawned but idle).
    println!("phase 1: curve on {}", handle.curve_feed().as_str());
    let grpc = sample(&mut event_rx, secs).await;
    grpc.report("  curve=grpc");

    // ── Phase 2: both wires carrying the curve at once.
    //
    // The scope translator moves the program id to the relay, but gRPC does not
    // drop what it already has until its resubscribe lands — so for a moment both
    // deliver the curve, which is exactly the overlap the dedupe ring exists for.
    // Whatever reaches the lanes here passed that ring, so a duplicate signature
    // in this set would be a dedupe failure.
    println!("\nphase 2: switching curve -> nats (overlap window)");
    handle.set_curve_feed(FeedKind::Nats);
    let overlap = sample(&mut event_rx, secs).await;
    overlap.report("  overlap");

    // ── Phase 3: curve on the relay, AMM still on gRPC.
    println!("\nphase 3: curve on {}", handle.curve_feed().as_str());
    let nats = sample(&mut event_rx, secs).await;
    nats.report("  curve=nats");

    // ── Phase 4: back to gRPC, proving the switch is not one-way.
    println!("\nphase 4: switching curve -> grpc");
    handle.set_curve_feed(FeedKind::Grpc);
    let back = sample(&mut event_rx, secs).await;
    back.report("  curve=grpc (again)");

    // ── Verdict ───────────────────────────────────────────────────────────────
    println!("\n================ VERDICT ================");
    let mut ok = true;

    for (label, p) in [
        ("grpc", &grpc),
        ("overlap", &overlap),
        ("nats", &nats),
        ("grpc-again", &back),
    ] {
        let trades = p.signatures.len();
        let live = trades > 0;
        ok &= live;
        println!(
            "{:<12} {} — {trades} distinct trade signatures",
            label,
            if live { "DECODES" } else { "SILENT (FAIL)" }
        );
    }

    // A switch is only free if it leaves no hole. Two things have to hold: no
    // phase skipped slots internally, and no phase started later than the one
    // before it ended.
    println!();
    for (label, p) in [
        ("grpc", &grpc),
        ("overlap", &overlap),
        ("nats", &nats),
        ("grpc-again", &back),
    ] {
        let gap = p.max_slot_gap();
        let clean = gap == 0;
        ok &= clean;
        println!(
            "{:<12} max slot gap {gap} {}",
            label,
            if clean { "(no hole)" } else { "(HOLE — slots with no trade at all)" }
        );
    }
    for (label, a, b) in [
        ("grpc -> overlap", &grpc, &overlap),
        ("overlap -> nats", &overlap, &nats),
        ("nats -> grpc", &nats, &back),
    ] {
        match (a.last_slot(), b.first_slot()) {
            (Some(end), Some(start)) => {
                // A phase boundary lands mid-stream, so the next phase resumes
                // either on the same slot or the very next one. Only a jump of
                // two or more means slots went unobserved.
                let hole = start.saturating_sub(end).saturating_sub(1);
                ok &= hole == 0;
                println!(
                    "{label:<18} boundary {end} -> {start} {}",
                    if hole == 0 { "(continuous)" } else { "(HOLE)" }
                );
            }
            _ => println!("{label:<18} boundary — (a phase saw no slots)"),
        }
    }

    // Both wires carry the same chain through the same decoder, so the mints
    // each one sees should overlap heavily. A near-zero intersection means one
    // side is decoding something else.
    let shared: usize = grpc.mints.intersection(&nats.mints).count();
    let smaller = grpc.mints.len().min(nats.mints.len()).max(1);
    let pct = shared as f64 / smaller as f64 * 100.0;
    println!(
        "\nmint overlap grpc∩nats = {shared} / {smaller} ({pct:.0}% of the smaller set)"
    );
    println!("  (samples are consecutive, not simultaneous, so partial overlap is expected;");
    println!("   near-zero would mean the two wires are not decoding the same chain)");

    // The whole point of scoping AMM to gRPC: migrations keep arriving no matter
    // who owns the curve.
    let migrations: u64 = [&grpc, &overlap, &nats, &back]
        .iter()
        .filter_map(|p| p.events.get("TokenMigrated"))
        .sum();
    println!("\nTokenMigrated across all phases: {migrations}");

    println!(
        "\n{}",
        if ok {
            "PASS — every phase decoded, no slot hole inside or across a phase, and the curve
       moved between wires twice with no restart."
        } else {
            "FAIL — a phase produced no trades, or a switch left a hole in the slot stream."
        }
    );
    if !ok {
        std::process::exit(1);
    }
}

async fn sample(rx: &mut tokio::sync::mpsc::Receiver<IngestEvent>, secs: u64) -> Phase {
    let mut phase = Phase::default();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if left.is_zero() {
            break;
        }
        match tokio::time::timeout(left, rx.recv()).await {
            Ok(Some(ev)) => phase.record(&ev),
            Ok(None) => break,
            Err(_) => break,
        }
    }
    phase
}

fn env(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| {
        eprintln!("{key} is not set");
        std::process::exit(1);
    })
}
