//! Shared trade leaves used by the grpc decode path:
//! - Borsh RawTradeEvent / RawPumpSwapBuyEvent / RawPumpSwapSellEvent decode
//! - log-line TradeEvent / PumpSwap BuyEvent / SellEvent decode
//! - AMM trade builder
//! - SOL balance-delta helper

use base64::{engine::general_purpose::STANDARD, Engine};
use borsh::BorshDeserialize;
use chrono::{DateTime, Utc};
use tracing::warn;

use crate::event::{Reserves, Side, Trade, Venue};
use crate::protocol::Protocol;

// ── Step 1a — TradeEvent from "Program data:" log lines ──────────────────────

#[derive(BorshDeserialize)]
pub(super) struct RawTradeEvent {
    pub(super) mint: [u8; 32],
    pub(super) sol_amount: u64,
    pub(super) token_amount: u64,
    pub(super) is_buy: bool,
    pub(super) user: [u8; 32],
    #[allow(dead_code)]
    timestamp: i64,
    pub(super) virtual_sol_reserves: u64,
    pub(super) virtual_token_reserves: u64,
    pub(super) real_sol_reserves: u64,
    pub(super) real_token_reserves: u64,
}

pub(super) struct DecodedTradeEvent {
    pub(super) mint: String,
    pub(super) sol_amount: f64,
    /// Raw token units — exact on-chain `u64` (never cast through `f64`).
    pub(super) token_amount: u64,
    pub(super) is_buy: bool,
    pub(super) user: String,
    pub(super) virtual_sol_reserves: f64,
    pub(super) virtual_token_reserves: u64,
    pub(super) real_sol_reserves: f64,
    pub(super) real_token_reserves: u64,
}

impl DecodedTradeEvent {
    pub(super) fn from_raw(raw: RawTradeEvent, lamports_per_sol: f64) -> Self {
        Self {
            mint: bs58::encode(raw.mint).into_string(),
            sol_amount: raw.sol_amount as f64 / lamports_per_sol,
            token_amount: raw.token_amount,
            is_buy: raw.is_buy,
            user: bs58::encode(raw.user).into_string(),
            virtual_sol_reserves: raw.virtual_sol_reserves as f64 / lamports_per_sol,
            virtual_token_reserves: raw.virtual_token_reserves,
            real_sol_reserves: raw.real_sol_reserves as f64 / lamports_per_sol,
            real_token_reserves: raw.real_token_reserves,
        }
    }
}

/// Scan "Program data:" log lines for TradeEvents (base64 + Borsh).
pub(super) fn decode_trade_events_from_logs(
    logs: &[&str],
    disc: &[u8; 8],
    lamports_per_sol: f64,
) -> Vec<DecodedTradeEvent> {
    let mut events = Vec::new();
    for log in logs {
        let Some(encoded) = log.strip_prefix("Program data: ") else {
            continue;
        };
        let bytes = match STANDARD.decode(encoded) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if bytes.len() < 8 || &bytes[..8] != disc {
            continue;
        }
        let mut buf: &[u8] = &bytes[8..];
        match RawTradeEvent::deserialize(&mut buf) {
            Ok(r) => events.push(DecodedTradeEvent::from_raw(r, lamports_per_sol)),
            Err(e) => warn!("Failed to Borsh-decode TradeEvent: {e}"),
        }
    }
    events
}

// ── PumpSwap BuyEvent / SellEvent from "Program data:" ───────────────────────

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
    user_quote_amount_in: u64,
    pool: [u8; 32],
    user: [u8; 32],
}

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
    user_quote_amount_out: u64,
    pool: [u8; 32],
    user: [u8; 32],
}

pub(super) struct DecodedAmmTrade {
    pub(super) is_buy: bool,
    /// Raw base-token units — exact on-chain `u64`.
    pub(super) base_amount: u64,
    pub(super) quote_amount: f64,
    pub(super) pool: String,
    pub(super) user: String,
    /// Post-swap base-token reserves — raw `u64` units.
    pub(super) pool_base_reserves: u64,
    pub(super) pool_quote_reserves: f64,
}

pub(super) fn decode_pump_swap_trades_from_logs(
    logs: &[&str],
    protocol: &Protocol,
) -> Vec<DecodedAmmTrade> {
    let mut out = Vec::new();
    let lps = protocol.lamports_per_sol;
    let buy_disc = &protocol.discriminators.pump_swap_buy_event;
    let sell_disc = &protocol.discriminators.pump_swap_sell_event;

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

        if disc == buy_disc {
            match RawPumpSwapBuyEvent::deserialize(&mut buf) {
                Ok(e) => {
                    let post_base = e.pool_base_token_reserves.saturating_sub(e.base_amount_out);
                    let post_quote = e.pool_quote_token_reserves
                        .saturating_add(e.quote_amount_in_with_lp_fee);
                    out.push(DecodedAmmTrade {
                        is_buy: true,
                        base_amount: e.base_amount_out,
                        quote_amount: e.user_quote_amount_in as f64 / lps,
                        pool: bs58::encode(e.pool).into_string(),
                        user: bs58::encode(e.user).into_string(),
                        pool_base_reserves: post_base,
                        pool_quote_reserves: post_quote as f64 / lps,
                    });
                }
                Err(e) => warn!("Failed to Borsh-decode PumpSwap BuyEvent: {e}"),
            }
        } else if disc == sell_disc {
            match RawPumpSwapSellEvent::deserialize(&mut buf) {
                Ok(e) => {
                    let post_base = e.pool_base_token_reserves.saturating_add(e.base_amount_in);
                    let post_quote = e.pool_quote_token_reserves
                        .saturating_sub(e.quote_amount_out_without_lp_fee);
                    out.push(DecodedAmmTrade {
                        is_buy: false,
                        base_amount: e.base_amount_in,
                        quote_amount: e.user_quote_amount_out as f64 / lps,
                        pool: bs58::encode(e.pool).into_string(),
                        user: bs58::encode(e.user).into_string(),
                        pool_base_reserves: post_base,
                        pool_quote_reserves: post_quote as f64 / lps,
                    });
                }
                Err(e) => warn!("Failed to Borsh-decode PumpSwap SellEvent: {e}"),
            }
        }
    }
    out
}

/// Build a [`Trade`] event from a decoded PumpSwap (AMM) swap.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_amm_trade(
    ev: &DecodedAmmTrade,
    mint: &str,
    signature: &str,
    slot: u64,
    block_time: DateTime<Utc>,
    received_at: DateTime<Utc>,
    instruction_labels: Vec<String>,
    tx_index: u32,
    leg_index: u32,
) -> Trade {
    let side = if ev.is_buy { Side::Buy } else { Side::Sell };
    let price = if ev.base_amount > 0 {
        ev.quote_amount / ev.base_amount as f64
    } else {
        0.0
    };
    Trade {
        mint: mint.to_string(),
        wallet: ev.user.clone(),
        side,
        sol: ev.quote_amount,
        tokens: ev.base_amount,
        price,
        signature: signature.to_string(),
        tx_index,
        leg_index,
        slot,
        block_time,
        received_at,
        reserves: Reserves {
            virtual_sol: Some(ev.pool_quote_reserves),
            virtual_token: Some(ev.pool_base_reserves),
            real_sol: Some(ev.pool_quote_reserves),
            real_token: Some(ev.pool_base_reserves),
        },
        venue: Venue::Amm,
        instruction_type: if ev.is_buy { "Buy".to_string() } else { "Sell".to_string() },
        instruction_labels,
    }
}

// ── SOL balance-delta helper ──────────────────────────────────────────────────

pub(super) fn compute_sol_change(
    wallet: &str,
    account_keys: &[&str],
    pre: &[u64],
    post: &[u64],
) -> f64 {
    account_keys
        .iter()
        .position(|k| *k == wallet)
        .map(|idx| {
            let pre_bal = pre.get(idx).copied().unwrap_or(0);
            let post_bal = post.get(idx).copied().unwrap_or(0);
            (pre_bal as f64 - post_bal as f64).abs() / 1_000_000_000.0
        })
        .unwrap_or(0.0)
}
