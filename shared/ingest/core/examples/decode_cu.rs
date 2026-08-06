//! One-shot: decode `compute_units_consumed` from base64 `raw_txs.payload` files.
//!
//! Usage: `cargo run -p ingest-core --example decode_cu -- <dir-of-*.b64>`

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use base64::Engine;
use prost::Message;

use ingest_core::proto::geyser::SubscribeUpdateTransaction;

fn main() -> ExitCode {
    let dir = match std::env::args().nth(1) {
        Some(d) => PathBuf::from(d),
        None => {
            eprintln!("usage: decode_cu <dir-with-*.b64>");
            return ExitCode::from(2);
        }
    };
    let mut cus: Vec<u64> = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .expect("read dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("b64"))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for ent in entries {
        let b64 = fs::read_to_string(ent.path()).unwrap_or_default();
        // psql may wrap long encode() output; drop all whitespace.
        let b64: String = b64.chars().filter(|c| !c.is_whitespace()).collect();
        if b64.is_empty() {
            continue;
        }
        let bytes = match base64::engine::general_purpose::STANDARD.decode(b64) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{}: b64 decode failed: {e}", ent.path().display());
                continue;
            }
        };
        let update = match SubscribeUpdateTransaction::decode(bytes.as_slice()) {
            Ok(u) => u,
            Err(e) => {
                eprintln!("{}: prost decode failed: {e}", ent.path().display());
                continue;
            }
        };
        let cu = update
            .transaction
            .as_ref()
            .and_then(|i| i.meta.as_ref())
            .and_then(|m| m.compute_units_consumed);
        let sig = ent
            .path()
            .with_extension("sig")
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        match cu {
            Some(c) => {
                println!("{c}\t{sig}");
                cus.push(c);
            }
            None => eprintln!("{}\t(no CU)", sig),
        }
    }

    if cus.is_empty() {
        eprintln!("no CU samples");
        return ExitCode::from(1);
    }
    cus.sort_unstable();
    let n = cus.len();
    let min = cus[0];
    let max = cus[n - 1];
    let p50 = cus[n / 2];
    let p90 = cus[(n * 9) / 10];
    let p99 = cus[n.saturating_sub(1).min((n * 99) / 100)];
    println!("---");
    println!("n={n} min={min} p50={p50} p90={p90} p99={p99} max={max}");
    // Suggested limit: p99 + 20% margin, floored to 1k, never below max sample.
    let suggested = ((p99 as f64) * 1.20).ceil() as u64;
    let suggested = suggested.max(max).div_ceil(1_000) * 1_000;
    println!("suggested_curve_buy_cu={suggested} (current default=150000)");
    ExitCode::SUCCESS
}
