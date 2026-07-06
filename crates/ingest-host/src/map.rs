//! Pure event → row mappers (no DB, so they unit-test without a network).
//!
//! Maps the borrowed `ingest-laserstream` events onto platform-core rows through
//! the pump.fun/SOL adapter. The crate's decoder already divides by
//! `lamports_per_sol`, so `Trade.sol` / `Reserves.virtual_sol` are human SOL —
//! we convert back to base units (lamports) here, since the quote is native SOL
//! (9 decimals). Reserves use the venue-neutral `virtual_*` pair (the decoder
//! copies AMM pool reserves into it), matching the single `reserve_quote/base`.

use ingest_laserstream::event::{
    RawTx as IlRawTx, Side, TokenCreated as IlTokenCreated, Trade as IlTrade, Venue,
};
use serde_json::json;

use platform_core::models::{NewToken, NewTrade, RawTx};
use platform_core::units::sol_to_lamports;
use platform_core::venue::{LaunchpadAdapter, MarketKind};

use crate::pumpfun::PumpFunAdapter;

/// pump.fun tokens mint with 6 decimals (protocol constant).
pub const PUMP_TOKEN_DECIMALS: i16 = 6;

/// Venue → the schema's `market_kind`.
pub fn market_kind_of(v: Venue) -> MarketKind {
    match v {
        Venue::Curve => MarketKind::BondingCurve,
        Venue::Amm => MarketKind::Amm,
    }
}

/// Side → the `trade_type` CHECK string.
pub fn trade_type_str(s: Side) -> &'static str {
    match s {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

/// Decode a base58 signature string to its raw bytes (the trades table stores
/// `tx_signature` as BYTEA, consistent with `raw_txs`).
pub fn decode_sig(b58: &str) -> anyhow::Result<Vec<u8>> {
    Ok(bs58::decode(b58).into_vec()?)
}

/// Map a decoded trade to a `NewTrade`. `wallet_id` is pre-interned by the caller.
pub fn trade_to_row(
    adapter: &PumpFunAdapter,
    wallet_id: i32,
    t: &IlTrade,
) -> anyhow::Result<NewTrade> {
    let kind = market_kind_of(t.venue);
    Ok(NewTrade {
        mint_address: t.mint.clone(),
        wallet_id,
        launchpad_id: adapter.launchpad_id(),
        market_kind: kind.as_str().to_string(),
        quote_asset_id: adapter.quote_asset_id(kind),
        trade_type: trade_type_str(t.side).to_string(),
        amount_quote: sol_to_lamports(t.sol),
        amount_base: t.tokens as i64,
        reserve_quote: t.reserves.virtual_sol.map(sol_to_lamports),
        reserve_base: t.reserves.virtual_token.map(|x| x as i64),
        slot: t.slot as i64,
        tx_index: t.tx_index as i32,
        // legs per tx are tiny; clamp defensively to the SMALLINT column.
        leg_index: t.leg_index.min(i16::MAX as u32) as i16,
        block_time: t.block_time,
        tx_signature: decode_sig(&t.signature)?,
    })
}

/// Map a token-create event to a `NewToken` (identity write-once row).
pub fn token_created_to_row(adapter: &PumpFunAdapter, tc: &IlTokenCreated) -> NewToken {
    NewToken {
        mint_address: tc.mint.clone(),
        launchpad_id: adapter.launchpad_id(),
        quote_asset_id: adapter.quote_asset_id(MarketKind::BondingCurve),
        creator_wallet: tc.creator.clone(),
        is_own_launch: false,
        name: tc.name.clone(),
        symbol: tc.symbol.clone(),
        decimals: PUMP_TOKEN_DECIMALS,
        token_program_id: tc.token_program_id.clone(),
        initial_supply_base: tc.initial_supply.map(|x| x as i64),
        initial_buy_quote: tc.initial_buy_sol.map(sol_to_lamports),
        creation_slot: Some(tc.slot as i64),
        creation_tx_signature: tc.signature.clone(),
        ix_labels: Some(json!(tc.instruction_labels)),
        meta: Some(json!({
            "bonding_curve": tc.bonding_curve,
            "is_mayhem_mode": tc.is_mayhem_mode,
            "is_cashback_enabled": tc.is_cashback_enabled,
            "cu_limit": tc.cu_limit,
            "cu_price": tc.cu_price,
        })),
        created_at: Some(tc.block_time),
    }
}

/// Map the raw-tx passthrough event to a `raw_txs` row. `payload` is the verbatim
/// prost-encoded `SubscribeUpdateTransaction` (parsed on read, never in SQL);
/// `source = 0` (live).
pub fn raw_tx_to_row(r: &IlRawTx) -> RawTx {
    RawTx {
        tx_signature: r.signature.clone(),
        slot: r.slot as i64,
        block_time: r.block_time,
        tx_index: r.tx_index as i32,
        payload: r.payload.clone(),
        source: 0,
    }
}
