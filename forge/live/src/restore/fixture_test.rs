//! Offline decode→map proof for the keystore-restore backfill (plan verification
//! §1). Two REAL mainnet pump.fun transactions — a token create (with its fused dev
//! buy) and a bonding-curve swap — captured once as `getTransaction(base64)` results
//! and embedded here. Each is routed through the EXACT restore path
//! (`wrap_transaction_result` → `rpc_to_protobuf` → `Decoder::decode_protobuf` →
//! `map::*`), and the resulting `NewToken` / `NewTrade` rows are asserted against the
//! decoded event. No network, no DB — the map reuse is pure.
//!
//! The fixtures are the raw per-tx results the pager's `get_transactions_batch`
//! yields; re-capture with the (deleted) capture harness if the pump.fun program or
//! decoder layout ever changes.

use std::sync::Arc;

use serde_json::Value;

use ingest_pumpfun::backfill::{rpc_to_protobuf, wrap_transaction_result};
use ingest_pumpfun::decode::{DecodeOutput, Decoder};
use ingest_pumpfun::{IngestEvent, Protocol};

use launcher::variant_for_token_program;

use super::backfill::block_time_of;
use crate::ingest::map;
use crate::ingest::pumpfun::PumpFunAdapter;

const CREATE_JSON: &str = include_str!("fixtures/create.json");
const SWAP_JSON: &str = include_str!("fixtures/swap.json");
const CREATE_SIG: &str = "5HJxEPFYSMaWUC1jnfeH47uVBptAsmQLCntYPmtiDHyJ5UT6ry6MJQGjx8UD93DxhSgJRPWp2JB5BggUZ2Zoe9mF";
const SWAP_SIG: &str = "2FCTuQ8Kfh6D3BHf8bP1otMVYW49gWAEVzg7W4JtDGzCBn2H75Dzn4x817u9BL58AvJz2rtvz6YZpeVcELjYtxbi";

/// Decode a captured fixture through the restore path, returning its events.
fn decode(raw_json: &str, sig: &str) -> (Vec<IngestEvent>, chrono::DateTime<chrono::Utc>) {
    let raw: Value = serde_json::from_str(raw_json).expect("fixture JSON");
    let block_time = block_time_of(&raw).expect("fixture has blockTime");
    let wrapped = wrap_transaction_result(sig, &raw);
    let update = rpc_to_protobuf(&wrapped).expect("rpc_to_protobuf lowered the tx");
    match Decoder::new(Arc::new(Protocol::pump_fun())).decode_protobuf(&update, block_time) {
        DecodeOutput::Events(e) => (e, block_time),
        DecodeOutput::Ignored => panic!("fixture decoded to Ignored — decoder layout changed?"),
    }
}

#[test]
fn create_fixture_maps_to_token_and_launch() {
    let (events, block_time) = decode(CREATE_JSON, CREATE_SIG);
    let tc = events
        .iter()
        .find_map(|e| match e {
            IngestEvent::TokenCreated(tc) => Some(tc),
            _ => None,
        })
        .expect("create tx decodes to a TokenCreated");

    // The event carries the REAL historical block_time (fed in as received_at), not
    // "now" — it's part of the trades dedup PK downstream.
    assert_eq!(tc.block_time, block_time);
    assert_eq!(tc.signature, CREATE_SIG);
    assert!(!tc.mint.is_empty());
    assert!(!tc.creator.is_empty());

    // The pure map reuse: NewToken mirrors the decoded create.
    let adapter = PumpFunAdapter::for_test(1, 1);
    let row = map::token_created_to_row(&adapter, tc);
    assert_eq!(row.mint_address, tc.mint);
    assert_eq!(row.creator_wallet, tc.creator);
    assert_eq!(row.launchpad_id, 1);
    assert_eq!(row.quote_asset_id, 1);
    assert_eq!(row.decimals, map::PUMP_TOKEN_DECIMALS);
    assert_eq!(row.creation_tx_signature, tc.signature);
    // Ingest inserts is_own_launch=false; the launcher/restore flips it separately.
    assert!(!row.is_own_launch);

    // The launch-variant label the restore backfill would stamp is derived from the
    // decoded token program (v1 = legacy SPL Token, else v2). A create names one.
    let variant = variant_for_token_program(tc.token_program_id.as_deref());
    assert!(
        variant == "pumpfun.create_v1" || variant == "pumpfun.create_v2",
        "unexpected variant {variant}"
    );
}

#[test]
fn swap_fixture_maps_to_trades() {
    let (events, block_time) = decode(SWAP_JSON, SWAP_SIG);
    let trades: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            IngestEvent::Trade(t) => Some(t),
            _ => None,
        })
        .collect();
    assert!(!trades.is_empty(), "swap tx decodes to at least one Trade");

    let adapter = PumpFunAdapter::for_test(1, 1);
    let want_sig = bs58::decode(SWAP_SIG).into_vec().unwrap();
    for t in trades.iter() {
        assert_eq!(t.block_time, block_time, "trade carries the real historical block_time");
        assert_eq!(t.signature, SWAP_SIG);
        assert!(!t.mint.is_empty());
        assert!(!t.wallet.is_empty());

        // The pure map reuse: NewTrade mirrors the decoded trade, exact-integer amounts.
        let row = map::trade_to_row(&adapter, 42, t).expect("trade_to_row");
        assert_eq!(row.wallet_ref, 42);
        assert_eq!(row.mint_address, t.mint);
        assert_eq!(row.amount_quote, t.sol_lamports as i64);
        assert_eq!(row.amount_base, t.tokens as i64);
        assert_eq!(row.launchpad_id, 1);
        assert_eq!(row.quote_asset_id, 1);
        assert_eq!(row.block_time, block_time);
        assert_eq!(row.leg_index as u32, t.leg_index);
        // Signature roundtrips base58 → BYTEA (matches raw_txs), and reserves populate.
        assert_eq!(row.tx_signature, want_sig);
        assert!(row.reserve_quote.is_some());
        assert!(row.reserve_base.is_some());
    }
}
