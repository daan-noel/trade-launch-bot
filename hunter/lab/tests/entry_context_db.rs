//! Entry Context against the real `trades` table: the read the page makes, end to
//! end, on the batch-pool shape (no statement ceiling).
//!
//! Ignored by default: needs `DATABASE_URL`, and `ENTRY_CONTEXT_WALLET` naming a
//! wallet with buys in the last day.

use std::time::Instant;

use chrono::{Duration, Utc};
use lab::api::handlers::entry_context::{
    read_entry_context, read_entry_range, read_entry_scan, EntryContextBody, EntryRangeBody, EntryScanBody,
};
use sqlx::postgres::PgPoolOptions;
use trading_core::storage::repositories::trade_repo::TradeRepo;

#[tokio::test]
#[ignore]
async fn a_real_wallet_reads_every_buy_and_its_window() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let wallet = std::env::var("ENTRY_CONTEXT_WALLET").expect("ENTRY_CONTEXT_WALLET");
    let pool = PgPoolOptions::new().max_connections(2).connect(&url).await.expect("connect");
    let repo = TradeRepo::new(pool);
    let body: EntryContextBody = serde_json::from_value(serde_json::json!({
        "from": (Utc::now() - Duration::days(1)).to_rfc3339(),
        "window_secs": 30,
        "probe_slots": 25,
        "group_by": "program",
        "tag": { "match": { "program": ["Pump.Fun"] }, "side": "buy" },
    }))
    .expect("body");

    let t = Instant::now();
    let r = read_entry_context(&repo, &wallet, &body).await.expect("read");
    let secs = t.elapsed().as_secs_f64();
    let with_buys = r.entries.iter().filter(|e| e.window.buy_tx > 0).count();
    eprintln!(
        "{} entries ({} with buys in the window), truncated {}, {:.1}s",
        r.entries.len(),
        with_buys,
        r.truncated,
        secs
    );
    let before = r.entries.iter().filter(|e| e.probe.nearest.is_some()).count();
    eprintln!("target in the probe's 25 slots before him on {before} entries");
    assert!(!r.entries.is_empty(), "the wallet has buys in the last day");
    for e in &r.entries {
        // A nearest target exists exactly when the probe window holds a tagged leg.
        if let Some(n) = &e.probe.nearest {
            assert!(n.lag_slots >= 0 && n.lag_slots <= 25);
            assert_eq!(n.lag_tx.is_some(), n.lag_slots == 0);
        } else {
            assert_eq!(e.probe.hits, 0);
        }
    }
    for e in r.entries.iter().filter(|e| e.unknown_reason.is_none()) {
        let listed: u32 = e.groups.iter().map(|g| g.buy_tx).sum();
        assert!(listed <= e.window.buy_tx, "breakdown never exceeds the window");
        if e.groups_omitted == 0 {
            assert_eq!(listed, e.window.buy_tx, "breakdown sums to the window");
        }
    }
    // A picked range over the same 30 s, ending one slot before his: the range read
    // is the entry read without his own slot, so it never counts more buys.
    let e = r
        .entries
        .iter()
        .find(|e| e.unknown_reason.is_none() && e.window.buy_tx > 0)
        .expect("an entry with buys in its window");
    let range: EntryRangeBody = serde_json::from_value(serde_json::json!({
        "mint": e.mint_address,
        "from": (e.at - Duration::seconds(30)).to_rfc3339(),
        "to": e.at.to_rfc3339(),
        "end_slot": e.slot - 1,
        "group_by": "program",
        "tag": { "match": { "program": ["Pump.Fun"] }, "side": "buy" },
    }))
    .expect("range body");
    let t = Instant::now();
    let rr = read_entry_range(&repo, &wallet, &range).await.expect("range read");
    eprintln!(
        "range read: {} buys ({} tagged) vs the entry's {} ({:.2}s)",
        rr.read.window.buy_tx,
        rr.read.window.tag_buy_tx,
        e.window.buy_tx,
        t.elapsed().as_secs_f64()
    );
    assert!(rr.read.window.buy_tx <= e.window.buy_tx);

    // The scan: every mint traded in the range, a moment every 30 s, priced after.
    let scan: EntryScanBody = serde_json::from_value(serde_json::json!({
        "from": (Utc::now() - Duration::days(1)).to_rfc3339(),
        "window_secs": 30,
        "probe_slots": 25,
        "tag": { "match": { "program": ["Pump.Fun"] }, "side": "buy" },
    }))
    .expect("scan body");
    let t = Instant::now();
    let sr = read_entry_scan(&repo, &wallet, &scan).await.expect("scan");
    let bytes = serde_json::to_vec(&sr).expect("json").len();
    let followed = sr.moments.iter().filter(|m| m.next_buy_secs.is_some()).count();
    eprintln!(
        "scan: {} moments on {} mints ({} before a buy of his), truncated {}, {:.1}s, {:.1} MB",
        sr.moments.len(),
        sr.mints,
        followed,
        sr.truncated,
        t.elapsed().as_secs_f64(),
        bytes as f64 / 1e6
    );
    assert!(!sr.moments.is_empty());
    assert!(followed > 0, "his mints are part of the market");
    for m in &sr.moments {
        assert!(m.price.is_some() || m.ret_pct == [None, None], "no price, no change after");
        assert!(m.next_buy_secs.is_none_or(|s| s >= 0.0));
    }
}
