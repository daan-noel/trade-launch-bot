//! Trade decoding: bonding-curve `TradeEvent`s (from logs and inner CPI),
//! PumpSwap (AMM) `BuyEvent`/`SellEvent`s, the shared AMM `Trade` builder, and
//! the balance-delta fallback for buys/sells with no decodable event.

use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD, Engine};
use borsh::BorshDeserialize;
use chrono::{DateTime, Utc};
use serde_json::Value;
use tracing::warn;

use crate::config::constants::{
    ANCHOR_EVENT_CPI_DISCRIMINATOR, LAMPORTS_PER_SOL, PUMP_SWAP_BUY_EVENT_DISCRIMINATOR,
    PUMP_SWAP_SELL_EVENT_DISCRIMINATOR, TRADE_EVENT_DISCRIMINATOR,
};
use crate::models::{
    events::{InternalEvent, TradeExecutedEvent},
    trade::{Trade, TradeType},
    transaction::RawTransaction,
};

use super::instructions::InstructionKind;
use super::parse::{compute_sol_change, compute_token_change, find_pump_ixs_anywhere};
use super::HeliusDecoder;

// ---------------------------------------------------------------------------
// Step 1a — decode TradeEvent from "Program data:" log lines (base64 + Borsh)
// ---------------------------------------------------------------------------

/// Borsh layout of the pump.fun on-chain TradeEvent (emitted via `emit!`).
/// Discriminator (8 bytes) is stripped before passing to `deserialize`.
#[derive(BorshDeserialize)]
struct RawTradeEvent {
    mint: [u8; 32],
    sol_amount: u64,
    token_amount: u64,
    is_buy: bool,
    user: [u8; 32],
    #[allow(dead_code)]
    timestamp: i64,
    virtual_sol_reserves: u64,
    virtual_token_reserves: u64,
    real_sol_reserves: u64,
    real_token_reserves: u64,
}

pub(super) struct DecodedTradeEvent {
    pub(super) mint: String,
    pub(super) sol_amount: f64,
    pub(super) token_amount: f64,
    pub(super) is_buy: bool,
    pub(super) user: String,
    pub(super) virtual_sol_reserves: f64,
    pub(super) virtual_token_reserves: f64,
    pub(super) real_sol_reserves: f64,
    pub(super) real_token_reserves: f64,
}

impl DecodedTradeEvent {
    /// Convert a Borsh-decoded `RawTradeEvent` into the decoder's normalized
    /// form: lamport amounts are scaled to SOL; token amounts stay in raw base
    /// units. Shared by the log-line and inner-instruction decode paths.
    fn from_raw(raw: RawTradeEvent) -> Self {
        Self {
            mint: bs58::encode(raw.mint).into_string(),
            sol_amount: raw.sol_amount as f64 / 1_000_000_000.0,
            token_amount: raw.token_amount as f64,
            is_buy: raw.is_buy,
            user: bs58::encode(raw.user).into_string(),
            virtual_sol_reserves: raw.virtual_sol_reserves as f64 / 1_000_000_000.0,
            virtual_token_reserves: raw.virtual_token_reserves as f64,
            real_sol_reserves: raw.real_sol_reserves as f64 / 1_000_000_000.0,
            real_token_reserves: raw.real_token_reserves as f64,
        }
    }
}

/// Scan every "Program data: <base64>" log line, try to decode each as a
/// TradeEvent, and return all successfully decoded events in log order.
pub(super) fn decode_trade_events_from_logs(logs: &[&str]) -> Vec<DecodedTradeEvent> {
    let mut events = Vec::new();

    for log in logs {
        let Some(encoded) = log.strip_prefix("Program data: ") else {
            continue;
        };

        let bytes = match STANDARD.decode(encoded) {
            Ok(b) => b,
            Err(_) => continue,
        };

        if bytes.len() < 8 || bytes[..8] != TRADE_EVENT_DISCRIMINATOR {
            continue;
        }

        let mut buf: &[u8] = &bytes[8..];
        let raw = match RawTradeEvent::deserialize(&mut buf) {
            Ok(r) => r,
            Err(e) => {
                warn!("Failed to Borsh-decode TradeEvent: {e}");
                continue;
            }
        };

        events.push(DecodedTradeEvent::from_raw(raw));
    }

    events
}

/// Scan inner instructions for pump.fun's `emit_cpi!` TradeEvent self-CPI and
/// decode each one. The instruction data is the 8-byte Anchor event-CPI tag,
/// followed by the 8-byte TradeEvent discriminator, followed by the same
/// Borsh-encoded `RawTradeEvent` carried in the "Program data:" log. This is the
/// truncation-proof source used when [`decode_trade_events_from_logs`] finds
/// nothing because Solana truncated the transaction's logs.
pub(super) fn decode_trade_events_from_inner_ixs(
    message: &Value,
    meta: &Value,
    account_keys: &[String],
    pump_program_id: &str,
) -> Vec<DecodedTradeEvent> {
    let mut events = Vec::new();

    for ix in find_pump_ixs_anywhere(message, meta, account_keys, pump_program_id) {
        let Some(data_b58) = ix["data"].as_str() else {
            continue;
        };
        let bytes = match bs58::decode(data_b58).into_vec() {
            Ok(b) => b,
            Err(_) => continue,
        };

        // [8: Anchor event-CPI tag][8: TradeEvent discriminator][Borsh payload]
        if bytes.len() < 16
            || bytes[..8] != ANCHOR_EVENT_CPI_DISCRIMINATOR
            || bytes[8..16] != TRADE_EVENT_DISCRIMINATOR
        {
            continue;
        }

        let mut buf: &[u8] = &bytes[16..];
        let raw = match RawTradeEvent::deserialize(&mut buf) {
            Ok(r) => r,
            Err(e) => {
                warn!("Failed to Borsh-decode TradeEvent from inner ix: {e}");
                continue;
            }
        };

        events.push(DecodedTradeEvent::from_raw(raw));
    }

    events
}

// ---------------------------------------------------------------------------
// Step 1a (cont.) — decode PumpSwap BuyEvent / SellEvent from "Program data:"
// ---------------------------------------------------------------------------

/// Leading fields of the PumpSwap `BuyEvent` (Borsh). Trailing fields added in
/// later program versions (e.g. `coin_creator`) are left unread by `deserialize`.
#[derive(BorshDeserialize)]
struct RawPumpSwapBuyEvent {
    #[allow(dead_code)]
    timestamp: i64,
    base_amount_out: u64,
    #[allow(dead_code)]
    max_quote_amount_in: u64,
    #[allow(dead_code)]
    user_base_token_reserves: u64,
    #[allow(dead_code)]
    user_quote_token_reserves: u64,
    pool_base_token_reserves: u64,
    pool_quote_token_reserves: u64,
    #[allow(dead_code)]
    quote_amount_in: u64,
    #[allow(dead_code)]
    lp_fee_basis_points: u64,
    #[allow(dead_code)]
    lp_fee: u64,
    #[allow(dead_code)]
    protocol_fee_basis_points: u64,
    #[allow(dead_code)]
    protocol_fee: u64,
    quote_amount_in_with_lp_fee: u64,
    /// Total SOL the user spent, including LP + protocol fees.
    user_quote_amount_in: u64,
    pool: [u8; 32],
    user: [u8; 32],
}

/// Leading fields of the PumpSwap `SellEvent` (Borsh). See `RawPumpSwapBuyEvent`.
#[derive(BorshDeserialize)]
struct RawPumpSwapSellEvent {
    #[allow(dead_code)]
    timestamp: i64,
    base_amount_in: u64,
    #[allow(dead_code)]
    min_quote_amount_out: u64,
    #[allow(dead_code)]
    user_base_token_reserves: u64,
    #[allow(dead_code)]
    user_quote_token_reserves: u64,
    pool_base_token_reserves: u64,
    pool_quote_token_reserves: u64,
    #[allow(dead_code)]
    quote_amount_out: u64,
    #[allow(dead_code)]
    lp_fee_basis_points: u64,
    #[allow(dead_code)]
    lp_fee: u64,
    #[allow(dead_code)]
    protocol_fee_basis_points: u64,
    #[allow(dead_code)]
    protocol_fee: u64,
    quote_amount_out_without_lp_fee: u64,
    /// Total SOL the user received, net of LP + protocol fees.
    user_quote_amount_out: u64,
    pool: [u8; 32],
    user: [u8; 32],
}

/// A decoded PumpSwap swap. `base_amount` is raw token units (matching the
/// bonding-curve `token_amount` convention); `quote_amount` and reserve fields
/// are in SOL (lamports / 1e9).
pub(super) struct DecodedAmmTrade {
    pub(super) is_buy: bool,
    pub(super) base_amount: f64,
    pub(super) quote_amount: f64,
    pub(super) pool: String,
    pub(super) user: String,
    pub(super) pool_base_reserves: f64,
    pub(super) pool_quote_reserves: f64,
}

/// Scan "Program data:" log lines for PumpSwap Buy/Sell events, in log order.
pub(super) fn decode_pump_swap_trades_from_logs(logs: &[&str]) -> Vec<DecodedAmmTrade> {
    let mut out = Vec::new();
    let lamports = LAMPORTS_PER_SOL as f64;

    for log in logs {
        let Some(encoded) = log.strip_prefix("Program data: ") else {
            continue;
        };
        let bytes = match STANDARD.decode(encoded) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if bytes.len() < 8 {
            continue;
        }

        let disc = &bytes[..8];
        let mut buf: &[u8] = &bytes[8..];

        if disc == PUMP_SWAP_BUY_EVENT_DISCRIMINATOR {
            match RawPumpSwapBuyEvent::deserialize(&mut buf) {
                Ok(e) => {
                    // PumpSwap events snapshot PRE-swap pool reserves. Roll them
                    // forward to the POST-swap state so chart spot reflects this
                    // trade's own price: base tokens leave the pool, quote (incl.
                    // the LP fee retained in the pool) enters it.
                    let post_base = e.pool_base_token_reserves.saturating_sub(e.base_amount_out);
                    let post_quote = e.pool_quote_token_reserves + e.quote_amount_in_with_lp_fee;
                    out.push(DecodedAmmTrade {
                        is_buy: true,
                        base_amount: e.base_amount_out as f64,
                        quote_amount: e.user_quote_amount_in as f64 / lamports,
                        pool: bs58::encode(e.pool).into_string(),
                        user: bs58::encode(e.user).into_string(),
                        pool_base_reserves: post_base as f64,
                        pool_quote_reserves: post_quote as f64 / lamports,
                    });
                }
                Err(e) => warn!("Failed to Borsh-decode PumpSwap BuyEvent: {e}"),
            }
        } else if disc == PUMP_SWAP_SELL_EVENT_DISCRIMINATOR {
            match RawPumpSwapSellEvent::deserialize(&mut buf) {
                Ok(e) => {
                    // PRE-swap -> POST-swap: base tokens enter the pool, quote
                    // (excl. the LP fee retained in the pool) leaves it.
                    let post_base = e.pool_base_token_reserves + e.base_amount_in;
                    let post_quote = e
                        .pool_quote_token_reserves
                        .saturating_sub(e.quote_amount_out_without_lp_fee);
                    out.push(DecodedAmmTrade {
                        is_buy: false,
                        base_amount: e.base_amount_in as f64,
                        quote_amount: e.user_quote_amount_out as f64 / lamports,
                        pool: bs58::encode(e.pool).into_string(),
                        user: bs58::encode(e.user).into_string(),
                        pool_base_reserves: post_base as f64,
                        pool_quote_reserves: post_quote as f64 / lamports,
                    });
                }
                Err(e) => warn!("Failed to Borsh-decode PumpSwap SellEvent: {e}"),
            }
        }
    }

    out
}

/// Build a `Trade` from a decoded PumpSwap (AMM) swap for `mint`. Shared by the
/// sync path ([`HeliusDecoder::decode_pump_swap_result`], explicit pool) and the
/// live path ([`HeliusDecoder::decode_amm_live`], pool resolved via the index).
#[allow(clippy::too_many_arguments)]
pub(super) fn build_amm_trade(
    ev: &DecodedAmmTrade,
    mint: &str,
    signature: &str,
    slot: u64,
    block_time: DateTime<Utc>,
    labels_json: &Value,
    leg_index: u32,
    received_at: DateTime<Utc>,
) -> Trade {
    let trade_type = if ev.is_buy {
        TradeType::Buy
    } else {
        TradeType::Sell
    };

    let mut trade = Trade::new(
        mint.to_string(),
        ev.user.clone(),
        trade_type,
        ev.quote_amount,
        ev.base_amount,
        signature.to_string(),
        slot,
        block_time,
    );
    // PumpSwap has no virtual reserves; `decode_pump_swap_trades_from_logs`
    // already converted the event's PRE-swap snapshot to POST-swap pool
    // reserves, so chart spot (sol / token) reflects each trade's price.
    trade.virtual_sol_reserves = Some(ev.pool_quote_reserves);
    trade.virtual_token_reserves = Some(ev.pool_base_reserves);
    trade.real_sol_reserves = Some(ev.pool_quote_reserves);
    trade.real_token_reserves = Some(ev.pool_base_reserves);
    trade.instruction_type = match trade.trade_type {
        TradeType::Buy => "Buy".to_string(),
        TradeType::Sell => "Sell".to_string(),
    };
    trade.instruction_labels = labels_json.clone();
    trade.leg_index = leg_index;
    trade.received_at = received_at;
    trade.venue = "amm".to_string();
    trade
}

impl HeliusDecoder {
    /// Balance-delta fallback for Buy/Sell when no "Program data:" TradeEvent
    /// was found (unusual, but guards against edge cases).
    pub(super) fn decode_trade_from_balances(
        &self,
        kind: InstructionKind,
        signature: &str,
        slot: u64,
        block_time: DateTime<Utc>,
        pump_accounts: &[String],
        account_keys: &[String],
        pre_balances: &[u64],
        post_balances: &[u64],
        meta: &Value,
        labels_json: &Value,
        raw_tx: &Arc<RawTransaction>,
    ) -> Option<Vec<InternalEvent>> {
        let mint = pump_accounts.get(2)?.to_string();
        if mint.is_empty() {
            return None;
        }
        let user = pump_accounts.get(6)?.to_string();
        if user.is_empty() {
            return None;
        }
        let user_ata = pump_accounts.get(5).cloned().unwrap_or_default();

        let trade_type = match kind {
            InstructionKind::Buy => TradeType::Buy,
            InstructionKind::Sell => TradeType::Sell,
            _ => return None,
        };

        let sol_amount = compute_sol_change(&user, account_keys, pre_balances, post_balances);
        let token_amount = compute_token_change(&user_ata, &mint, account_keys, meta);

        if Trade::is_dust(sol_amount) {
            return None;
        }

        let mut trade = Trade::new(
            mint,
            user,
            trade_type,
            sol_amount,
            token_amount,
            signature.to_string(),
            slot,
            block_time,
        );
        trade.instruction_type = match trade.trade_type {
            TradeType::Buy => "Buy".to_string(),
            TradeType::Sell => "Sell".to_string(),
        };
        trade.instruction_labels = labels_json.clone();
        trade.received_at = raw_tx.received_at;

        Some(vec![InternalEvent::TradeExecuted(TradeExecutedEvent {
            trade,
            tx_signature: signature.to_string(),
            slot,
            timestamp: raw_tx.received_at,
            raw_tx: raw_tx.clone(),
        })])
    }
}
