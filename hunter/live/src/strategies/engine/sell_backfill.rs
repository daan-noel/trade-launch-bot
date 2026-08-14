//! Heal a **landed** sell that the `trades` feed never delivered.
//!
//! Sell-confirm is feed-based by design (no per-attempt RPC), and that is right
//! for the hot path. But the feed can miss a leg — post-migration AMM coverage is
//! the sharp edge: a pool's swaps only reach `trades` once it is in the ingest
//! subscription, so our own first sell into a freshly graduated pool can land
//! seconds before the feed starts carrying it. When that happens every downstream
//! reader is wrong in the same direction: `confirm_sell` times out, the PG bag net
//! still shows the tokens, and the reaper — which correctly deduces from the empty
//! on-chain bag that the sell landed — then asks that same blind feed what we were
//! paid and books **zero proceeds** (a −100% close on a winning trade, observed
//! 2026-08-14 on `FfuX44…pump`: +0.213 SOL recorded as −0.049).
//!
//! So heal the feed rather than guess around it. The submitted signature is the
//! one durable piece of evidence we always hold, and `getTransaction` + the ONE
//! ingest decoder turn it back into exactly the `trades` rows live ingest would
//! have written — same `Trade` shape, same `user_quote_amount_out` convention, so
//! a healed fill stays comparable with every feed-confirmed one. Repairing
//! `trades` (rather than booking a one-off number onto the position) also fixes
//! the bag net, the sibling close, and the token's chart in the same write.
//!
//! Helius: one batched `getTransaction` per heal, only on a path that has already
//! failed its feed poll. Never called from the decision or snipe hot path.

use std::sync::Arc;

use chrono::Utc;
use tracing::{info, warn};

use ingest_laserstream::{
    backfill::{get_transactions_batch, rpc_to_protobuf, wrap_transaction_result},
    decode::{DecodeOutput, HeliusDecoder},
    event::{IngestEvent, Side},
    Protocol,
};

use trading_core::models::trade::Trade;
use trading_core::storage::repositories::trade_repo::TradeRepo;

use crate::services::token_sync::trade_from_ingest_event;
use crate::trader::PumpFunTrader;

/// Fetch `sigs`, decode them with the live ingest decoder, and insert any sell
/// legs belonging to `(wallet, mint)` that the feed missed. Returns the number of
/// legs written.
///
/// Idempotent: `insert_many` is `ON CONFLICT DO NOTHING` on
/// `(block_time, tx_signature, leg_index)`, so re-running after the feed catches
/// up (or across two reaper ticks) writes nothing twice.
///
/// Best-effort by contract — every failure returns 0 and logs. A heal that cannot
/// run must leave the caller exactly where it was, never worse.
pub async fn heal_missing_sell_legs(
    trader: &Arc<PumpFunTrader>,
    trade_repo: &TradeRepo,
    mint: &str,
    sigs: &[String],
) -> usize {
    let sigs: Vec<String> = sigs.iter().filter(|s| !s.is_empty()).cloned().collect();
    if sigs.is_empty() {
        return 0;
    }
    let wallet = trader.wallet_pubkey();

    let results = match get_transactions_batch(trader.rpc_url(), &sigs).await {
        Ok(r) => r,
        Err(e) => {
            warn!(mint = %mint, "sell heal: getTransaction batch failed: {e}");
            return 0;
        }
    };

    let decoder = decoder_for(trader, mint);
    let mut legs: Vec<Trade> = Vec::new();
    for (sig, result) in sigs.iter().zip(results) {
        // `None` = missing, errored, or reverted on-chain — nothing to heal.
        let Some(result) = result else { continue };
        let Some(update) = rpc_to_protobuf(&wrap_transaction_result(sig, &result)) else {
            warn!(mint = %mint, sig = %sig, "sell heal: tx did not lower to protobuf");
            continue;
        };
        let DecodeOutput::Events(events) = decoder.decode_protobuf(&update, Utc::now()) else {
            continue;
        };
        legs.extend(
            events
                .iter()
                .filter_map(|e| match e {
                    IngestEvent::Trade(t) => Some(t),
                    _ => None,
                })
                .filter(|t| t.side == Side::Sell && t.mint == mint && t.wallet == wallet)
                .map(trade_from_ingest_event),
        );
    }

    if legs.is_empty() {
        return 0;
    }
    let n = legs.len();
    if let Err(e) = trade_repo.insert_many(&legs).await {
        warn!(mint = %mint, "sell heal: insert of {n} recovered leg(s) failed: {e}");
        return 0;
    }
    let sol: f64 = legs.iter().map(|t| t.amount_sol).sum();
    info!(
        mint = %mint, legs = n, sol,
        "sell heal: recovered landed sell leg(s) the feed missed"
    );
    n
}

/// A decoder wired exactly like live ingest, with the pool index seeded for this
/// one mint.
///
/// The AMM path resolves a swap's pool → mint through that index and emits
/// nothing for a pool it does not know (`decode_amm_live_pb`), so a heal on a
/// migrated token needs the pool up front. Both known sources are seeded: the
/// executor's harvested [`AmmPoolFacts`](pump_trader::types::AmmPoolFacts) pool —
/// authoritative, and the only one right for a `pool_v2` coin — and the canonical
/// PDA derivation as the fallback when the facts cache is cold. A curve sell needs
/// neither; `decode_protobuf` self-classifies and routes.
fn decoder_for(trader: &Arc<PumpFunTrader>, mint: &str) -> HeliusDecoder {
    let protocol = Arc::new(Protocol::pump_fun());
    let index: ingest_laserstream::PoolIndex = Arc::new(dashmap::DashMap::new());
    if let Some(facts) = trader.amm_pool_facts_snapshot(mint) {
        index.insert(facts.pool, mint.to_string());
    }
    if let Some(pool) = ingest_laserstream::pool::derive_pool(mint, &protocol) {
        index.insert(pool, mint.to_string());
    }
    HeliusDecoder::new(protocol).with_pool_index(index)
}
