//! Domain C — the feed (raw_txs source-of-truth + trades typed projection).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A typed trade row. Amounts are exact base units (`amount_quote` quote,
/// `amount_base` token); `reserve_quote`/`reserve_base` are the venue-neutral
/// price pair. `launchpad_id`/`market_kind`/`quote_asset_id` are denormalized so
/// the hot read never joins. `wallet_ref` is a soft ref into `wallet_dict` — the
/// interned network-wallet id, NOT a managed-wallet UUID.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Trade {
    pub mint_address: String,
    pub wallet_ref: i32,
    pub launchpad_id: i16,
    pub market_kind: String,
    pub quote_asset_id: i16,
    pub trade_type: String,
    pub amount_quote: i64,
    pub amount_base: i64,
    pub reserve_quote: Option<i64>,
    pub reserve_base: Option<i64>,
    pub slot: i64,
    pub tx_index: i32,
    pub leg_index: i16,
    pub block_time: DateTime<Utc>,
    pub tx_signature: Vec<u8>,
    /// The trading wallet's whole-tx signed SOL flow, native lamports (post - pre:
    /// swap, venue fee, signature/priority fee, tip; its own token-account rent
    /// nets out). Per-TRANSACTION — every leg of the tx repeats it, so collapse by
    /// `tx_signature` before summing. `None` unless the tx's fee payer is the
    /// trade's wallet (or when the source carried no balances).
    pub payer_net_lamports: Option<i64>,
}

/// Spot price of a trade's post-trade reserve pair, as a raw ratio (quote base
/// units per base base unit): `reserve_quote / reserve_base`. Curve rows carry the
/// curve's virtual reserves, AMM rows the pool's priced reserves (quote vault +
/// PumpSwap's virtual quote, base vault), so the one
/// ratio prices both venues. `None` when either side is missing or the base side is
/// not positive. The Rust twin of the `trades_priced.spot_price_quote` view column
/// (same expression), for writers that price a row before it is read back.
pub fn spot_price_quote(reserve_quote: Option<i64>, reserve_base: Option<i64>) -> Option<f64> {
    match (reserve_quote, reserve_base) {
        (Some(q), Some(b)) if b > 0 => Some(q as f64 / b as f64),
        _ => None,
    }
}

/// Insert form of [`Trade`]. `wallet_ref` is already interned (call
/// `WalletDictRepo::intern` first). `leg_index` defaults to 0.
#[derive(Debug, Clone)]
pub struct NewTrade {
    pub mint_address: String,
    pub wallet_ref: i32,
    pub launchpad_id: i16,
    pub market_kind: String,
    pub quote_asset_id: i16,
    pub trade_type: String,
    pub amount_quote: i64,
    pub amount_base: i64,
    pub reserve_quote: Option<i64>,
    pub reserve_base: Option<i64>,
    pub slot: i64,
    pub tx_index: i32,
    pub leg_index: i16,
    pub block_time: DateTime<Utc>,
    pub tx_signature: Vec<u8>,
    /// See [`Trade::payer_net_lamports`].
    pub payer_net_lamports: Option<i64>,
}

/// One of a wallet's fills of a mint, as the position replay reads it (canonical
/// order). `payer_net_lamports` is the tx's whole wallet flow when the tx paid by
/// this wallet traded no other mint, else `None` (the leg's `amount_quote` books).
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct WalletFill {
    pub address: String,
    pub trade_type: String,
    pub amount_quote: i64,
    pub amount_base: i64,
    pub tx_signature: Vec<u8>,
    pub payer_net_lamports: Option<i64>,
}

/// `trades_priced` view row: a [`Trade`] plus the quote's decimals/USD rate and
/// derived prices. `exec_price_quote`/`spot_price_quote` are raw ratios;
/// `amount_usd` is the cross-quote numeraire.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TradePriced {
    pub mint_address: String,
    pub wallet_ref: i32,
    /// Canonical on-chain wallet identity, resolved from `wallet_dict` in the
    /// view (falls back to `#<wallet_ref>` if the interned row is missing).
    /// Render THIS, not `wallet_ref` (the internal interned key).
    pub wallet_address: String,
    pub launchpad_id: i16,
    pub market_kind: String,
    pub quote_asset_id: i16,
    pub trade_type: String,
    pub amount_quote: i64,
    pub amount_base: i64,
    pub reserve_quote: Option<i64>,
    pub reserve_base: Option<i64>,
    pub slot: i64,
    pub tx_index: i32,
    pub leg_index: i16,
    pub block_time: DateTime<Utc>,
    pub tx_signature: Vec<u8>,
    pub quote_symbol: String,
    pub quote_decimals: i16,
    pub quote_usd_rate: Option<f64>,
    pub exec_price_quote: Option<f64>,
    pub spot_price_quote: Option<f64>,
    pub amount_quote_display: Option<f64>,
    pub amount_usd: Option<f64>,
}

/// Source-of-truth unparsed feed row (BYTEA payload). `trades` is a typed
/// projection of this — replayable; the feed table IS the transport.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RawTx {
    pub tx_signature: Vec<u8>,
    pub slot: i64,
    pub block_time: DateTime<Utc>,
    pub tx_index: i32,
    pub payload: Vec<u8>,
    pub source: i16,
}
