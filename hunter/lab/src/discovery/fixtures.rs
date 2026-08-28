//! Shared test fixtures for the discovery pipeline — one synthetic cohort every
//! layer's tests run against, so a screen test and a family test are looking at the
//! same corpus and their numbers are comparable.

use std::sync::Arc;

use chrono::{DateTime, Duration, TimeZone, Utc};

use crate::sweep::corpus::{Corpus, CorpusToken};
use crate::sweep::generic::Pricing;
use crate::sweep::projection::CorpusTrade;
use trading_core::strategies::kernel::CostModel;
use trading_core::strategies::paper_fill::FillModel;

/// Honest pricing: a fill model that already charges execution slippage paired with
/// the fee-only cost model, so the round-trip isn't double-charged.
pub fn pricing() -> Pricing {
    Pricing {
        buy_amount_sol: 1.0,
        fill_model: FillModel::FirstInWindow,
        cost: CostModel::pumpfun_fee_only(),
    }
}

pub fn created_at() -> DateTime<Utc> {
    Utc.timestamp_opt(1_700_000_000, 0).unwrap()
}

pub fn trade(secs: i64, price: f64, reserve: f64, is_buy: bool) -> CorpusTrade {
    CorpusTrade {
        flow: crate::sweep::projection::FlowKeys::default(),
        block_time: created_at() + Duration::seconds(secs),
        amount_sol: 1.0,
        token_amount: 1.0,
        price_per_token: price,
        reserve_sol: Some(reserve),
        reserve_token: Some(1_000.0),
        real_reserve_sol: Some(reserve),
        real_token_reserves: None,
        slot: secs as u64,
        tx_index: 0,
        leg_index: 0,
        is_buy,
        tx_signature: None,
        ix_labels: None,
        wallet: None,
    }
}

/// A token whose price rises then falls, so both TP and SL get exercised and the
/// price-path metrics (`trail`, `rise`, `stall`) take a real range of values.
///
/// Trade slots are **consecutive** (1 s apart): the paper fill only looks one slot
/// ahead ([`MAX_FILL_WAIT_SLOTS`](trading_core::config::constants::MAX_FILL_WAIT_SLOTS)
/// = 3), so a wider spacing would leave every enter-on-arm trigger unable to find a
/// qualifying buy in its window and no combo would ever fire. Buys land on 2 of every
/// 3 trades so a sell-slot trigger still has an adjacent buy to fill against.
pub fn token(mint: &str, n: usize) -> CorpusToken {
    let trades: Vec<CorpusTrade> = (0..n)
        .map(|i| {
            let f = i as f64;
            let price = if i < n / 2 { 1.0 + f * 0.2 } else { 1.0 + (n as f64 - f) * 0.1 };
            trade(i as i64, price, 40.0 + f, i % 3 != 0)
        })
        .collect();
    CorpusToken {
        mint: mint.into(),
        symbol: mint.into(),
        created_at: created_at(),
        trades: Arc::new(trades),
        fp: Default::default(),
        identity: None,
        peak_after: None,
    }
}

pub fn corpus(n_tokens: usize) -> Corpus {
    Corpus {
        tokens: (0..n_tokens).map(|i| token(&format!("m{i}"), 20 + i % 7)).collect(),
        hash: "discovery-test".into(),
        has_fingerprints: false,
        candidates_capped: false,
    }
}
