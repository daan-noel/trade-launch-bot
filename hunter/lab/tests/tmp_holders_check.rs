//! Temporary: real-data check of the top-holder reads. Deleted after the check.

use std::collections::HashMap;
use std::time::Instant;

use chrono::{Duration, Utc};
use lab::api::handlers::entry_context::{read_entry_context, EntryContextBody};
use sqlx::postgres::PgPoolOptions;
use trading_core::storage::repositories::trade_repo::{SlotWindow, TradeRepo};

#[tokio::test]
#[ignore]
async fn tmp_holders() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let wallet = "8dtx2tr4TuJsYpri2suggFu1pg3DVjFLBBVmhtDy1MEF";
    let pool = PgPoolOptions::new().max_connections(2).connect(&url).await.expect("connect");
    let repo = TradeRepo::new(pool);
    let body: EntryContextBody = serde_json::from_value(serde_json::json!({
        "from": (Utc::now() - Duration::days(3)).to_rfc3339(),
        "window_secs": 30,
        "probe_slots": 25,
    }))
    .expect("body");
    let t = Instant::now();
    let r = read_entry_context(&repo, wallet, &body).await.expect("read");
    eprintln!("{} entries in {:.1}s", r.entries.len(), t.elapsed().as_secs_f64());
    let with = r.entries.iter().filter(|e| e.holders.is_some()).count();
    eprintln!("with a holder read: {with}");
    for e in r.entries.iter().filter(|e| e.holders.is_some()).take(0) {
        let h = e.holders.unwrap();
        eprintln!(
            "{} holders {:4}  top1 {:5.1}  top10 {:5.1}  top1% {:5.1}  top10% {:5.1}",
            &e.mint_address[..8],
            h.holders,
            h.top1_drop_pct,
            h.top10_drop_pct,
            h.top1pct_drop_pct,
            h.top10pct_drop_pct
        );
    }

    // Completeness: on the curve, every token traders hold left the curve, so the
    // bags of everyone (him included) add up to the initial virtual tokens minus the
    // curve's token reserve.
    let mut mints: Vec<&str> = r.entries.iter().filter(|e| e.holders.is_some()).map(|e| e.mint_address.as_str()).collect();
    mints.sort();
    mints.dedup();
    let windows: Vec<SlotWindow> = mints
        .iter()
        .take(40)
        .map(|m| SlotWindow {
            mint_address: m.to_string(),
            lo_slot: 0,
            hi_slot: i64::MAX,
            lo_time: Utc::now() - Duration::days(30),
            hi_time: Utc::now(),
        })
        .collect();
    let tape = repo.holder_tape(&windows, None).await.expect("tape");
    let mut ok = 0;
    let mut checked = 0;
    for legs in tape.chunk_by(|a, b| a.mint_address == b.mint_address) {
        let mut bags: HashMap<i32, i64> = HashMap::new();
        let mut last_pool = None;
        let mut on_curve = true;
        let mut unmatched: i64 = 0;
        let mut sellers_without = 0;
        for p in legs {
            if let Some(h) = p.holder_id {
                let b = bags.entry(h).or_default();
                let next = *b + if p.is_buy { p.token_amount } else { -p.token_amount };
                if next < 0 {
                    unmatched += -next;
                    sellers_without += 1;
                }
                *b = next.max(0);
            }
            if let (Some(l), Some(t)) = (p.reserve_lamports, p.reserve_token) {
                last_pool = Some(t);
                on_curve = l < 120_000_000_000 && t > 200_000_000_000_000;
            }
        }
        let Some(pool) = last_pool else { continue };
        if !on_curve {
            continue;
        }
        let held: i64 = bags.values().sum();
        let sold = 1_073_000_000_000_000i64 - pool;
        checked += 1;
        let err = (held - sold) as f64 / sold.max(1) as f64 * 100.0;
        if err.abs() < 1.0 {
            ok += 1;
        }
        let fixed = (held - unmatched - sold) as f64 / sold.max(1) as f64 * 100.0;
        eprintln!("{} legs {:6}  diff {:+9.2}%  sold-without-bag {:.3e} by {:3} legs  diff after taking it off {:+8.2}%", &legs[0].mint_address[..8], legs.len(), err, unmatched as f64, sellers_without, fixed);
    }
    eprintln!("curve mints within 1%: {ok} of {checked}");

    // At his own seats: has a wallet sold tokens it never bought before his buy?
    let mut all: Vec<&str> = r.entries.iter().map(|e| e.mint_address.as_str()).collect();
    all.sort();
    all.dedup();
    let windows: Vec<SlotWindow> = all
        .iter()
        .map(|m| SlotWindow {
            mint_address: m.to_string(),
            lo_slot: 0,
            hi_slot: i64::MAX,
            lo_time: Utc::now() - Duration::days(30),
            hi_time: Utc::now(),
        })
        .collect();
    let mut tape = Vec::new();
    for c in windows.chunks(100) {
        tape.extend(repo.holder_tape(c, Some(wallet)).await.expect("tape"));
    }
    let mut by: HashMap<&str, Vec<_>> = HashMap::new();
    for p in &tape {
        by.entry(p.mint_address.as_str()).or_default().push(p);
    }
    let (mut seats, mut dirty, mut big) = (0, 0, 0);
    for e in &r.entries {
        let Some(legs) = by.get(e.mint_address.as_str()) else { continue };
        let mut bags: HashMap<i32, i64> = HashMap::new();
        let mut unmatched = 0i64;
        let mut held_total = 0i64;
        for p in legs.iter().take_while(|p| (p.slot, p.tx_index) < (e.slot, e.tx_index)) {
            let Some(h) = p.holder_id else { continue };
            let b = bags.entry(h).or_default();
            let next = *b + if p.is_buy { p.token_amount } else { -p.token_amount };
            if next < 0 {
                unmatched += -next;
            }
            *b = next.max(0);
        }
        held_total += bags.values().sum::<i64>();
        seats += 1;
        if unmatched > 0 {
            dirty += 1;
            if unmatched as f64 > 0.05 * held_total as f64 {
                big += 1;
            }
        }
    }
    eprintln!("his seats: {seats}, with a sell of never-bought tokens before him: {dirty}, of them over 5% of held: {big}");
}
