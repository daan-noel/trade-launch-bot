//! Tick-cost harness — what the settled-token skip is worth, and the regression
//! check for anything that might quietly take it away.
//!
//! `#[ignore]`d (it is a measurement, not an assertion) and **release-only** — a
//! debug build measures the allocator, not the fold:
//!
//! ```text
//! cargo test -p hunter-engine --release --test tick_bench -- --ignored --nocapture
//! ```
//!
//! The fixture is the shape that dominates a long simulate: tokens that printed for
//! a while and went quiet **without** their reserves dropping under
//! `DEAD_MAX_LIQUIDITY_SOL`, so the dead verdict can never fire, no arm ever
//! disarms, and none of them are ever pruned. Measured 2026-08-07 on the workstation
//! (i9-11900F, release):
//!
//! ```text
//!   dense ticks: 200000 ticks x 500 tokens in   27.05s  (270.5 ns / token-tick)
//! settled ticks: 200000 ticks x 500 tokens in 148.61ms  (  1.5 ns / token-tick)
//! ```
//!
//! ~180x, and the gap widens with the token count because the skip short-circuits
//! the whole map (`EngineState::all_settled_at`) rather than each token.

use std::sync::Arc;
use std::time::Instant;

use chrono::{Duration, TimeZone, Utc};
use hunter_engine::event::{Event, LoadedRule, Mint, RuleId, TradeMode};
use hunter_engine::fingerprint::{AxisId, AxisPredicate, Criteria, Fingerprint, FingerprintId};
use hunter_engine::grouping::TokenFingerprint;
use hunter_engine::metrics::{Side, TradeLite, Ts};
use hunter_engine::reduce::reduce;
use hunter_engine::rule_params::RuleParams;
use hunter_engine::EngineState;
use serde_json::json;
use uuid::Uuid;

fn ts(secs: f64) -> Ts {
    Utc.timestamp_opt(1_700_000_000, 0).unwrap() + Duration::milliseconds((secs * 1000.0) as i64)
}

fn build(dense: bool, n_tokens: usize) -> EngineState {
    let fp = Fingerprint {
        id: FingerprintId(Uuid::from_u128(1)),
        wildcard: false,
        criteria: Criteria::new().with(AxisId::CuLimit, AxisPredicate::exact(200_000)),
        metric_config: json!({ "m_flow_split": { "volume_ix_patterns": [["Pump.Fun: Buy"]] } }),
    };
    let rule = LoadedRule {
        id: RuleId(Uuid::from_u128(1)),
        fingerprint_id: fp.id,
        trade_mode: TradeMode::Paper,
        buy_amount_lamports: 100_000_000,
        // Explicit `1`, not the `0` unlimited sentinel: the measured workload is
        // one open position while the rest of the corpus keeps ticking.
        max_concurrent_tokens: 1,
        max_total_tokens: 0,
        params: RuleParams::parse(&json!({
            "entry": {
                "m_flow_window": { "window_size_sec": 30, "gross_flow": [{"operator": ">", "value": 500.0}] },
                "m_flow_split_window": { "window_size_sec": 30, "vol_share": [{"operator": "<", "value": 5.0}] }
            },
            "exit": { "m_flow_split": { "nonvol_net": [{"operator": "<", "value": -999.0}] } }
        }))
        .unwrap(),
        entry_enabled: true,
    };
    let mut s = EngineState::new();
    s.dense_ticks = dense;
    reduce(&mut s, Event::RulesReloaded { rules: Arc::from(vec![rule]), fps: Arc::from(vec![fp]) });
    for i in 0..n_tokens {
        let mint = Mint::from(format!("mint{i:040}"));
        reduce(
            &mut s,
            Event::TokenCreated {
                mint: mint.clone(),
                fp: Box::new(TokenFingerprint { cu_limit: Some(200_000), ..Default::default() }),
                at: ts(0.0),
                creator_wallet_hash: Some(3),
                identity: None,
            },
        );
        // 40 prints, then silence. Reserves stay above the dead floor, so the token
        // can never be pruned — the shape that dominates a long simulate.
        for k in 0..40 {
            reduce(
                &mut s,
                Event::Trade {
                    mint: mint.clone(),
                    trade: TradeLite {
                       slot: 0,
                       marker_bits: 0,
                        side: if k % 2 == 0 { Side::Buy } else { Side::Sell },
                        sol: 1.0,
                        price: 1.0 + k as f64 * 0.01,
                        reserve_sol: 90.0,
                        priced_reserve_sol: 90.0,
                        at: ts(k as f64 * 0.5),
                        ix_hash: None,
                        wallet_hash: k as u64,
                    },
                },
            );
        }
    }
    s
}

#[test]
#[ignore]
fn dense_vs_settled_ticks() {
    const TOKENS: usize = 500;
    const TICKS: usize = 200_000; // ~11 h of 200 ms grid

    for dense in [true, false] {
        let mut s = build(dense, TOKENS);
        let start = Instant::now();
        for k in 0..TICKS {
            reduce(&mut s, Event::Tick { now: ts(100.0 + k as f64 * 0.2) });
        }
        let el = start.elapsed();
        println!(
            "{:>7} ticks: {TICKS} ticks x {TOKENS} tokens in {:>8.2?}  ({:.1} ns / token-tick)",
            if dense { "dense" } else { "settled" },
            el,
            el.as_nanos() as f64 / (TICKS * TOKENS) as f64,
        );
    }
}
