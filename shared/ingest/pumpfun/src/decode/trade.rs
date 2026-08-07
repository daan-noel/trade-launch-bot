//! Shared trade leaves used by the grpc decode path:
//! - Borsh RawTradeEvent / RawPumpSwapBuyEvent / RawPumpSwapSellEvent decode
//! - log-line TradeEvent / PumpSwap BuyEvent / SellEvent decode
//! - AMM trade builder
//! - SOL balance-delta helper

use base64::{engine::general_purpose::STANDARD, Engine};
use borsh::BorshDeserialize;
use chrono::{DateTime, Utc};
use tracing::warn;

#[cfg(test)]
use borsh::BorshSerialize;

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
    /// Exact quote lamports — the raw on-chain `u64`, mirror of `sol_amount`.
    pub(super) sol_amount_lamports: u64,
    /// Raw token units — exact on-chain `u64` (never cast through `f64`).
    pub(super) token_amount: u64,
    pub(super) is_buy: bool,
    pub(super) user: String,
    pub(super) virtual_sol_reserves: f64,
    /// Exact raw-`u64` lamport mirror of `virtual_sol_reserves`.
    pub(super) virtual_sol_reserves_lamports: u64,
    pub(super) virtual_token_reserves: u64,
    pub(super) real_sol_reserves: f64,
    /// Exact raw-`u64` lamport mirror of `real_sol_reserves`.
    pub(super) real_sol_reserves_lamports: u64,
    pub(super) real_token_reserves: u64,
}

impl DecodedTradeEvent {
    pub(super) fn from_raw(raw: RawTradeEvent, lamports_per_sol: f64) -> Self {
        Self {
            mint: bs58::encode(raw.mint).into_string(),
            sol_amount: raw.sol_amount as f64 / lamports_per_sol,
            sol_amount_lamports: raw.sol_amount,
            token_amount: raw.token_amount,
            is_buy: raw.is_buy,
            user: bs58::encode(raw.user).into_string(),
            virtual_sol_reserves: raw.virtual_sol_reserves as f64 / lamports_per_sol,
            virtual_sol_reserves_lamports: raw.virtual_sol_reserves,
            virtual_token_reserves: raw.virtual_token_reserves,
            real_sol_reserves: raw.real_sol_reserves as f64 / lamports_per_sol,
            real_sol_reserves_lamports: raw.real_sol_reserves,
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
#[cfg_attr(test, derive(BorshSerialize))]
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
#[cfg_attr(test, derive(BorshSerialize))]
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
    /// Exact quote lamports — the raw on-chain `u64`, mirror of `quote_amount`.
    pub(super) quote_amount_lamports: u64,
    pub(super) pool: String,
    pub(super) user: String,
    /// Post-swap base-token reserves — raw `u64` units.
    pub(super) pool_base_reserves: u64,
    pub(super) pool_quote_reserves: f64,
    /// Exact raw-`u64` lamport mirror of `pool_quote_reserves`.
    pub(super) pool_quote_reserves_lamports: u64,
}

/// Turn a decoded PumpSwap `BuyEvent` into the neutral [`DecodedAmmTrade`]. SSOT for
/// the buy reserve math — shared by the log-line path and the inner-instruction
/// recovery so the two can't drift.
fn amm_buy_trade(e: RawPumpSwapBuyEvent, lps: f64) -> DecodedAmmTrade {
    let post_base = e.pool_base_token_reserves.saturating_sub(e.base_amount_out);
    let post_quote = e
        .pool_quote_token_reserves
        .saturating_add(e.quote_amount_in_with_lp_fee);
    DecodedAmmTrade {
        is_buy: true,
        base_amount: e.base_amount_out,
        quote_amount: e.user_quote_amount_in as f64 / lps,
        quote_amount_lamports: e.user_quote_amount_in,
        pool: bs58::encode(e.pool).into_string(),
        user: bs58::encode(e.user).into_string(),
        pool_base_reserves: post_base,
        pool_quote_reserves: post_quote as f64 / lps,
        pool_quote_reserves_lamports: post_quote,
    }
}

/// Turn a decoded PumpSwap `SellEvent` into the neutral [`DecodedAmmTrade`]. SSOT for
/// the sell reserve math — shared by the log-line and inner-instruction paths.
fn amm_sell_trade(e: RawPumpSwapSellEvent, lps: f64) -> DecodedAmmTrade {
    let post_base = e.pool_base_token_reserves.saturating_add(e.base_amount_in);
    let post_quote = e
        .pool_quote_token_reserves
        .saturating_sub(e.quote_amount_out_without_lp_fee);
    DecodedAmmTrade {
        is_buy: false,
        base_amount: e.base_amount_in,
        quote_amount: e.user_quote_amount_out as f64 / lps,
        quote_amount_lamports: e.user_quote_amount_out,
        pool: bs58::encode(e.pool).into_string(),
        user: bs58::encode(e.user).into_string(),
        pool_base_reserves: post_base,
        pool_quote_reserves: post_quote as f64 / lps,
        pool_quote_reserves_lamports: post_quote,
    }
}

pub(super) fn decode_pump_swap_trades_from_logs(
    logs: &[&str],
    protocol: &Protocol,
) -> Vec<DecodedAmmTrade> {
    let mut out = Vec::new();
    let buy_disc = &protocol.discriminators.pump_swap_buy_event;
    let sell_disc = &protocol.discriminators.pump_swap_sell_event;
    let lps = protocol.lamports_per_sol;

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
                Ok(e) => out.push(amm_buy_trade(e, lps)),
                Err(e) => warn!("Failed to Borsh-decode PumpSwap BuyEvent: {e}"),
            }
        } else if disc == sell_disc {
            match RawPumpSwapSellEvent::deserialize(&mut buf) {
                Ok(e) => out.push(amm_sell_trade(e, lps)),
                Err(e) => warn!("Failed to Borsh-decode PumpSwap SellEvent: {e}"),
            }
        }
    }
    out
}

/// Recover PumpSwap (AMM) swaps from the pump_swap program's **inner instructions**
/// — the anchor self-CPI events (`anchor_event_cpi` disc + `BuyEvent`/`SellEvent`
/// disc + Borsh payload). This is the AMM twin of the curve path's
/// `decode_trade_events_from_inner_pb`: inner instructions are NOT subject to the
/// validator's per-tx log byte limit, so a multi-swap AMM bundle whose trailing
/// `"Program data:"` lines were truncated is recovered here in full. `ix_datas` is
/// the raw instruction data of every pump_swap inner ix (venue-neutral byte slices,
/// so this stays independent of the protobuf `PbIx` type in `grpc.rs`).
pub(super) fn decode_pump_swap_trades_from_inner(
    ix_datas: &[&[u8]],
    protocol: &Protocol,
) -> Vec<DecodedAmmTrade> {
    let anchor = &protocol.discriminators.anchor_event_cpi;
    let buy_disc = &protocol.discriminators.pump_swap_buy_event;
    let sell_disc = &protocol.discriminators.pump_swap_sell_event;
    let lps = protocol.lamports_per_sol;
    let mut out = Vec::new();

    for data in ix_datas {
        // Anchor event CPI: [event_cpi disc (8)] [event disc (8)] [borsh payload].
        if data.len() < 16 || &data[..8] != anchor {
            continue;
        }
        let disc = &data[8..16];
        let mut buf: &[u8] = &data[16..];
        if disc == buy_disc {
            match RawPumpSwapBuyEvent::deserialize(&mut buf) {
                Ok(e) => out.push(amm_buy_trade(e, lps)),
                Err(e) => warn!("Failed to Borsh-decode inner PumpSwap BuyEvent: {e}"),
            }
        } else if disc == sell_disc {
            match RawPumpSwapSellEvent::deserialize(&mut buf) {
                Ok(e) => out.push(amm_sell_trade(e, lps)),
                Err(e) => warn!("Failed to Borsh-decode inner PumpSwap SellEvent: {e}"),
            }
        }
    }
    out
}

/// Build a [`Trade`] event from a decoded PumpSwap (AMM) swap.
/// `amm_swap_accounts` — the top-level swap ix's resolved account list, when the
/// decode harvested one for this trade's pool (see `decode_amm_live_pb`).
/// (`Box<Vec<..>>` is deliberate: it keeps the common `None` event size flat —
/// see the field docs on [`Trade`].)
#[allow(clippy::too_many_arguments, clippy::box_collection)]
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
    amm_swap_accounts: Option<Box<Vec<String>>>,
    fee_lamports: Option<u64>,
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
        sol_lamports: ev.quote_amount_lamports,
        tokens: ev.base_amount,
        price,
        fee_lamports,
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
            virtual_sol_lamports: Some(ev.pool_quote_reserves_lamports),
            real_sol_lamports: Some(ev.pool_quote_reserves_lamports),
        },
        venue: Venue::Amm,
        instruction_type: if ev.is_buy { "Buy".to_string() } else { "Sell".to_string() },
        instruction_labels,
        amm_swap_accounts,
    }
}

// ── SOL balance-delta helper ──────────────────────────────────────────────────

pub(super) fn compute_sol_change(
    wallet: &str,
    account_keys: &[&str],
    pre: &[u64],
    post: &[u64],
) -> f64 {
    compute_sol_change_lamports(wallet, account_keys, pre, post) as f64 / 1_000_000_000.0
}

/// The wallet's absolute lamport balance delta (exact `u64`) — the raw basis
/// `compute_sol_change` divides to human SOL. Carried alongside so a native-SOL
/// host persists the exact integer without an `f64` round-trip.
pub(super) fn compute_sol_change_lamports(
    wallet: &str,
    account_keys: &[&str],
    pre: &[u64],
    post: &[u64],
) -> u64 {
    account_keys
        .iter()
        .position(|k| *k == wallet)
        .map(|idx| {
            let pre_bal = pre.get(idx).copied().unwrap_or(0);
            let post_bal = post.get(idx).copied().unwrap_or(0);
            pre_bal.abs_diff(post_bal)
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buy_event(base_out: u64, quote_in: u64, pool: u8, user: u8) -> RawPumpSwapBuyEvent {
        RawPumpSwapBuyEvent {
            timestamp: 0,
            base_amount_out: base_out,
            max_quote_amount_in: quote_in,
            user_base_token_reserves: 0,
            user_quote_token_reserves: 0,
            pool_base_token_reserves: 1_000_000,
            pool_quote_token_reserves: 2_000_000_000,
            quote_amount_in: quote_in,
            lp_fee_basis_points: 0,
            lp_fee: 0,
            protocol_fee_basis_points: 0,
            protocol_fee: 0,
            quote_amount_in_with_lp_fee: quote_in,
            user_quote_amount_in: quote_in,
            pool: [pool; 32],
            user: [user; 32],
        }
    }

    fn sell_event(base_in: u64, quote_out: u64, pool: u8, user: u8) -> RawPumpSwapSellEvent {
        RawPumpSwapSellEvent {
            timestamp: 0,
            base_amount_in: base_in,
            min_quote_amount_out: quote_out,
            user_base_token_reserves: 0,
            user_quote_token_reserves: 0,
            pool_base_token_reserves: 1_000_000,
            pool_quote_token_reserves: 2_000_000_000,
            quote_amount_out: quote_out,
            lp_fee_basis_points: 0,
            lp_fee: 0,
            protocol_fee_basis_points: 0,
            protocol_fee: 0,
            quote_amount_out_without_lp_fee: quote_out,
            user_quote_amount_out: quote_out,
            pool: [pool; 32],
            user: [user; 32],
        }
    }

    /// Encode a PumpSwap event as an anchor self-CPI inner-instruction data blob:
    /// `[event_cpi disc (8)] [event disc (8)] [borsh payload]`.
    fn inner_ix(event_cpi: &[u8; 8], event_disc: &[u8; 8], payload: &[u8]) -> Vec<u8> {
        let mut d = Vec::with_capacity(16 + payload.len());
        d.extend_from_slice(event_cpi);
        d.extend_from_slice(event_disc);
        d.extend_from_slice(payload);
        d
    }

    /// The AMM twin of the curve log-truncation fix: a multi-swap bundle whose
    /// trailing swaps were dropped from the logs is recovered in full from the
    /// pump_swap inner instructions — buys and sells, with base/quote/pool/user
    /// preserved and matching the log-path reserve math.
    #[test]
    fn inner_amm_recovery_decodes_buys_and_sells() {
        let p = Protocol::pump_fun();
        let cpi = &p.discriminators.anchor_event_cpi;

        let buy = inner_ix(
            cpi,
            &p.discriminators.pump_swap_buy_event,
            &borsh::to_vec(&buy_event(1_000, 500_000_000, 7, 9)).unwrap(),
        );
        let sell = inner_ix(
            cpi,
            &p.discriminators.pump_swap_sell_event,
            &borsh::to_vec(&sell_event(2_000, 250_000_000, 7, 11)).unwrap(),
        );
        // A non-pump_swap inner ix (wrong CPI disc) must be ignored.
        let noise = inner_ix(&[0xAA; 8], &p.discriminators.pump_swap_buy_event, &[0u8; 8]);

        let datas: Vec<&[u8]> = vec![buy.as_slice(), sell.as_slice(), noise.as_slice()];
        let trades = decode_pump_swap_trades_from_inner(&datas, &p);

        assert_eq!(trades.len(), 2, "one trade per real swap event, noise ignored");
        assert!(trades[0].is_buy);
        assert_eq!(trades[0].base_amount, 1_000);
        assert!((trades[0].quote_amount - 0.5).abs() < 1e-9);
        assert!(!trades[1].is_buy);
        assert_eq!(trades[1].base_amount, 2_000);
        assert!((trades[1].quote_amount - 0.25).abs() < 1e-9);
        // Buy/sell come from the same pool → identical decoded pool id.
        assert_eq!(trades[0].pool, trades[1].pool);
    }

    /// The inner recovery yields the SAME `DecodedAmmTrade` a complete log scan
    /// would — so swapping to the recovered set on truncation never changes values.
    #[test]
    fn inner_and_log_paths_agree() {
        let p = Protocol::pump_fun();
        let raw = buy_event(4_242, 777_000_000, 3, 4);
        let payload = borsh::to_vec(&raw).unwrap();

        // Log path: base64 "Program data:" line.
        let mut logline = Vec::new();
        logline.extend_from_slice(&p.discriminators.pump_swap_buy_event);
        logline.extend_from_slice(&payload);
        let encoded = format!("Program data: {}", STANDARD.encode(&logline));
        let from_logs = decode_pump_swap_trades_from_logs(&[encoded.as_str()], &p);

        // Inner path: anchor CPI inner ix.
        let ix = inner_ix(
            &p.discriminators.anchor_event_cpi,
            &p.discriminators.pump_swap_buy_event,
            &payload,
        );
        let from_inner = decode_pump_swap_trades_from_inner(&[ix.as_slice()], &p);

        assert_eq!(from_logs.len(), 1);
        assert_eq!(from_inner.len(), 1);
        assert_eq!(from_logs[0].base_amount, from_inner[0].base_amount);
        assert_eq!(from_logs[0].pool, from_inner[0].pool);
        assert_eq!(from_logs[0].user, from_inner[0].user);
        assert_eq!(from_logs[0].pool_base_reserves, from_inner[0].pool_base_reserves);
        assert!((from_logs[0].quote_amount - from_inner[0].quote_amount).abs() < 1e-9);
    }
}
