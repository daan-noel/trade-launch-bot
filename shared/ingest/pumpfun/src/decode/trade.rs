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

use super::instructions::FeeBudget;
use crate::event::{Reserves, Side, Trade, Venue};
use crate::protocol::{Protocol, PUMP_SWAP_VIRTUAL_QUOTE_LAMPORTS};

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

/// The fields `TradeEvent` carries past [`RawTradeEvent`]'s, up to the curve
/// creator: `fee_recipient` (32), `fee_basis_points` (u64), `fee` (u64).
const TRADE_EVENT_FEE_FIELDS_LEN: usize = 32 + 8 + 8;

/// The curve creator from the bytes that follow a Borsh-decoded [`RawTradeEvent`]:
/// `TradeEvent.creator`, right after the fee fields. `None` when the event is too
/// short to carry it (events from before the venue added creator fees).
fn trailing_curve_creator(rest: &[u8]) -> Option<[u8; 32]> {
    rest.get(TRADE_EVENT_FEE_FIELDS_LEN..TRADE_EVENT_FEE_FIELDS_LEN + 32)?
        .try_into()
        .ok()
}

/// Decode one `TradeEvent` body (the bytes after its discriminator): the Borsh
/// prefix plus the trailing curve creator. SSOT for the log-line and the
/// inner-instruction paths.
pub(super) fn decode_trade_event_body(
    body: &[u8],
    lamports_per_sol: f64,
) -> std::io::Result<DecodedTradeEvent> {
    let mut rest = body;
    let raw = RawTradeEvent::deserialize(&mut rest)?;
    let mut ev = DecodedTradeEvent::from_raw(raw, lamports_per_sol);
    ev.curve_creator = trailing_curve_creator(rest);
    Ok(ev)
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
    /// [`Trade::curve_creator`].
    pub(super) curve_creator: Option<[u8; 32]>,
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
            curve_creator: None,
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
        match decode_trade_event_body(&bytes[8..], lamports_per_sol) {
            Ok(ev) => events.push(ev),
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
    /// Exact quote lamports the user side moved — the raw on-chain `u64`, mirror
    /// of `quote_amount`: what a buy paid, fees included; what a sell received,
    /// fees taken.
    pub(super) quote_amount_lamports: u64,
    pub(super) pool: String,
    pub(super) user: String,
    /// Post-swap base-token reserves — raw `u64` units.
    pub(super) pool_base_reserves: u64,
    /// Post-swap quote reserve the pool PRICES with: its quote vault plus
    /// [`PUMP_SWAP_VIRTUAL_QUOTE_LAMPORTS`]. Human SOL.
    pub(super) pool_quote_reserves: f64,
    /// Exact raw-`u64` lamport mirror of `pool_quote_reserves`.
    pub(super) pool_quote_reserves_lamports: u64,
    /// Post-swap quote vault balance — the pool's real SOL. Human SOL.
    pub(super) pool_quote_vault: f64,
    /// Exact raw-`u64` lamport mirror of `pool_quote_vault`.
    pub(super) pool_quote_vault_lamports: u64,
    /// [`Trade::venue_fee_bps`]: the fees the user side paid over the pool's own
    /// constant-product amount.
    pub(super) fee_bps: Option<f64>,
}

/// Fees over the pool's own constant-product amount, in bps; `None` for a zero
/// pool amount.
fn venue_fee_bps(fees: u64, pool_amount: u64) -> Option<f64> {
    (pool_amount > 0).then(|| fees as f64 / pool_amount as f64 * 10_000.0)
}

/// The quote reserve a PumpSwap pool prices with, from its vault balance.
fn priced_quote(vault_lamports: u64) -> u64 {
    vault_lamports.saturating_add(PUMP_SWAP_VIRTUAL_QUOTE_LAMPORTS)
}

/// Turn a decoded PumpSwap `BuyEvent` into the neutral [`DecodedAmmTrade`]. SSOT for
/// the buy reserve math — shared by the log-line path and the inner-instruction
/// recovery so the two can't drift.
///
/// The two buy instructions fill the event's quote fields the opposite way round:
/// `buy` (base out) reports the pool's amount as `quote_amount_in` and the user's
/// fee-inclusive spend as `user_quote_amount_in`; `buy_exact_quote_in` reports the
/// spend as `quote_amount_in` and the pool's amount as `user_quote_amount_in`. The
/// fees sit on top of the pool's amount either way, so the spend is the larger of
/// the two and the pool's amount the smaller.
fn amm_buy_trade(e: RawPumpSwapBuyEvent, lps: f64) -> DecodedAmmTrade {
    let paid = e.quote_amount_in.max(e.user_quote_amount_in);
    let pool_amount = e.quote_amount_in.min(e.user_quote_amount_in);
    let post_base = e.pool_base_token_reserves.saturating_sub(e.base_amount_out);
    let post_vault = e
        .pool_quote_token_reserves
        .saturating_add(e.quote_amount_in_with_lp_fee);
    let post_quote = priced_quote(post_vault);
    DecodedAmmTrade {
        is_buy: true,
        base_amount: e.base_amount_out,
        quote_amount: paid as f64 / lps,
        quote_amount_lamports: paid,
        pool: bs58::encode(e.pool).into_string(),
        user: bs58::encode(e.user).into_string(),
        pool_base_reserves: post_base,
        pool_quote_reserves: post_quote as f64 / lps,
        pool_quote_reserves_lamports: post_quote,
        pool_quote_vault: post_vault as f64 / lps,
        pool_quote_vault_lamports: post_vault,
        fee_bps: venue_fee_bps(paid - pool_amount, pool_amount),
    }
}

/// Turn a decoded PumpSwap `SellEvent` into the neutral [`DecodedAmmTrade`]. SSOT for
/// the sell reserve math — shared by the log-line and inner-instruction paths.
fn amm_sell_trade(e: RawPumpSwapSellEvent, lps: f64) -> DecodedAmmTrade {
    let post_base = e.pool_base_token_reserves.saturating_add(e.base_amount_in);
    let post_vault = e
        .pool_quote_token_reserves
        .saturating_sub(e.quote_amount_out_without_lp_fee);
    let post_quote = priced_quote(post_vault);
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
        pool_quote_vault: post_vault as f64 / lps,
        pool_quote_vault_lamports: post_vault,
        fee_bps: venue_fee_bps(
            e.quote_amount_out.saturating_sub(e.user_quote_amount_out),
            e.quote_amount_out,
        ),
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
    fee_budget: FeeBudget,
    payer_net_lamports: Option<i64>,
    payer: &str,
    is_proxied: Option<bool>,
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
        payer: payer.to_string(),
        is_proxied,
        side,
        sol: ev.quote_amount,
        sol_lamports: ev.quote_amount_lamports,
        tokens: ev.base_amount,
        price,
        fee_lamports: fee_budget.fee_lamports,
        cu_limit: fee_budget.cu_limit,
        cu_price: fee_budget.cu_price,
        tip_lamports: fee_budget.tip_lamports,
        payer_net_lamports,
        venue_fee_bps: ev.fee_bps,
        signature: signature.to_string(),
        tx_index,
        leg_index,
        slot,
        block_time,
        received_at,
        // The priced pair carries the pool's virtual quote, like the curve's; the
        // real pair is the vault, the SOL a seller can actually take out.
        reserves: Reserves {
            virtual_sol: Some(ev.pool_quote_reserves),
            virtual_token: Some(ev.pool_base_reserves),
            real_sol: Some(ev.pool_quote_vault),
            real_token: Some(ev.pool_base_reserves),
            virtual_sol_lamports: Some(ev.pool_quote_reserves_lamports),
            real_sol_lamports: Some(ev.pool_quote_vault_lamports),
        },
        venue: Venue::Amm,
        instruction_type: if ev.is_buy { "Buy".to_string() } else { "Sell".to_string() },
        instruction_labels,
        amm_swap_accounts,
        curve_creator: None,
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

/// An account's absolute lamport balance delta (exact `u64`) — the raw basis
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

    /// A real `buy_exact_quote_in` and the sell one slot later on the same pool
    /// (`jAMSvc…`, 2026-09-14, base mint `BP6JJ…pump`). The buy's spend is
    /// `quote_amount_in` (the payer's WSOL fell by exactly that), its fees 90 bps
    /// on the pool's amount; the vault after it is the chain's post balance; and
    /// both legs are constant-product on the vault plus the virtual quote.
    #[test]
    fn real_pumpswap_legs_reproduce_the_chain() {
        let buy = RawPumpSwapBuyEvent {
            timestamp: 1_789_369_149,
            base_amount_out: 12_745_168_642,
            max_quote_amount_in: 220_811_531,
            user_base_token_reserves: 0,
            user_quote_token_reserves: 220_811_531,
            pool_base_token_reserves: 32_035_340_155_435,
            pool_quote_token_reserves: 532_262_059_461,
            quote_amount_in: 220_811_531,
            lp_fee_basis_points: 20,
            lp_fee: 437_684,
            protocol_fee_basis_points: 5,
            protocol_fee: 109_421,
            quote_amount_in_with_lp_fee: 219_279_637,
            user_quote_amount_in: 218_841_953,
            pool: [1; 32],
            user: [2; 32],
        };
        let (b0, q0) = (buy.pool_base_token_reserves, buy.pool_quote_token_reserves);
        let t = amm_buy_trade(buy, 1e9);
        assert_eq!(t.quote_amount_lamports, 220_811_531, "what the payer spent");
        assert!((t.fee_bps.unwrap() - 90.0).abs() < 1e-3, "20 lp + 5 protocol + 65 creator");
        assert_eq!(t.pool_quote_vault_lamports, 532_481_339_098, "the vault's post balance");
        assert_eq!(t.pool_quote_reserves_lamports, 532_481_339_098 + PUMP_SWAP_VIRTUAL_QUOTE_LAMPORTS);
        let priced = u128::from(q0 + PUMP_SWAP_VIRTUAL_QUOTE_LAMPORTS);
        let cp_tokens = u128::from(b0) * 218_841_953 / (priced + 218_841_953);
        assert!(cp_tokens.abs_diff(12_745_168_642) < 100, "{cp_tokens}");

        let sell = RawPumpSwapSellEvent {
            timestamp: 1_789_369_150,
            base_amount_in: 11_473_905_413,
            min_quote_amount_out: 175_582_084,
            user_base_token_reserves: 14_621_664_643,
            user_quote_token_reserves: 0,
            pool_base_token_reserves: 32_022_517_675_720,
            pool_quote_token_reserves: 532_482_672_640,
            quote_amount_out: 197_022_551,
            lp_fee_basis_points: 20,
            lp_fee: 394_046,
            protocol_fee_basis_points: 5,
            protocol_fee: 98_512,
            quote_amount_out_without_lp_fee: 196_628_505,
            user_quote_amount_out: 195_249_346,
            pool: [1; 32],
            user: [2; 32],
        };
        let (b0, q0, sold) =
            (sell.pool_base_token_reserves, sell.pool_quote_token_reserves, sell.base_amount_in);
        let t = amm_sell_trade(sell, 1e9);
        assert_eq!(t.quote_amount_lamports, 195_249_346);
        assert!((t.fee_bps.unwrap() - 90.0).abs() < 1e-3);
        let priced = u128::from(q0 + PUMP_SWAP_VIRTUAL_QUOTE_LAMPORTS);
        assert_eq!(priced * u128::from(sold) / u128::from(b0 + sold), 197_022_551, "lamport-exact");
    }

    #[test]
    fn amm_fee_is_the_user_amount_against_the_pool_amount() {
        let mut buy = buy_event(1_000, 1_000_000, 1, 2);
        buy.user_quote_amount_in = 1_009_500; // pool amount + 95 bps of fees
        let t = amm_buy_trade(buy, 1e9);
        assert!((t.fee_bps.unwrap() - 95.0).abs() < 1e-9);
        assert_eq!(t.quote_amount_lamports, 1_009_500, "sol stays the user side");

        let mut sell = sell_event(1_000, 1_000_000, 1, 2);
        sell.user_quote_amount_out = 990_500;
        assert!((amm_sell_trade(sell, 1e9).fee_bps.unwrap() - 95.0).abs() < 1e-9);

        assert_eq!(amm_buy_trade(buy_event(1_000, 0, 1, 2), 1e9).fee_bps, None);
    }

    /// A mainnet curve-sell `TradeEvent` (375 bytes with its discriminator), captured
    /// off the relay together with the creator vault its transaction passed.
    const LIVE_SELL_EVENT_B64: &str = "vdt/007mYe7M7j7o1ndiTdplLHrllmiqDDLVslhveGtzlJGSJRz1PImjwhEAAAAAvF5W3SsBAAAAqzhgZIe/K2nyw15NMbml0dVdqur4+NtGiuU7/nbRVp+QmKlqAAAAAOP26g4UAAAA0HqusNVTAQDjSscSDQAAANDim2REVQAArRHmpPwpRKT6glG++BVCbhv7KMa2ZGZ3YHxq2fVmpkZfAAAAAAAAAG0xKwAAAAAAhw7y/rKeVTDAXHkPYV5JscPXZHB4SKxoYbLRWzXEJ5MeAAAAAAAAANKjDQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAAAAHNlbGwAAAAAAAAAAAAAAAAAAAAAAIgTAAAAAAAAtpgVAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACJo8IRAAAAAOP26g4UAAAA40rHEg0AAAAAAAAAAAAAAAAAAAAAAAAA";
    const LIVE_SELL_CREATOR_VAULT: &str = "HF3UQ2FKdWoXoFmuveb5BmUjBEvmWaSxdouahuK2NqRL";
    const PUMP_PROGRAM: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

    fn creator_vault(creator: [u8; 32]) -> String {
        use solana_sdk::pubkey::Pubkey;
        use std::str::FromStr;
        let program = Pubkey::from_str(PUMP_PROGRAM).unwrap();
        Pubkey::find_program_address(&[b"creator-vault", &creator], &program).0.to_string()
    }

    /// The decoded creator is the one the venue validated the swap against: its
    /// creator-vault PDA is the vault the transaction actually passed.
    #[test]
    fn trade_event_creator_derives_the_vault_the_swap_passed() {
        let p = Protocol::pump_fun();
        let line = format!("Program data: {LIVE_SELL_EVENT_B64}");
        let events = decode_trade_events_from_logs(&[line.as_str()], &p.discriminators.trade_event, 1e9);
        assert_eq!(events.len(), 1);
        assert!(!events[0].is_buy);
        let creator = events[0].curve_creator.expect("a 375-byte event carries the creator");
        assert_eq!(creator_vault(creator), LIVE_SELL_CREATOR_VAULT);
    }

    /// The inner-instruction path decodes the same body the same way.
    #[test]
    fn inner_trade_event_carries_the_same_creator() {
        let bytes = STANDARD.decode(LIVE_SELL_EVENT_B64).unwrap();
        let from_log = decode_trade_event_body(&bytes[8..], 1e9).unwrap();
        let p = Protocol::pump_fun();
        let data = inner_ix(&p.discriminators.anchor_event_cpi, &p.discriminators.trade_event, &bytes[8..]);
        let from_inner = decode_trade_event_body(&data[16..], 1e9).unwrap();
        assert_eq!(from_log.curve_creator, from_inner.curve_creator);
        assert!(from_inner.curve_creator.is_some());
    }

    /// An event one byte short of the creator still decodes its trade, without one.
    #[test]
    fn trade_event_too_short_for_the_creator_decodes_without_it() {
        let bytes = STANDARD.decode(LIVE_SELL_EVENT_B64).unwrap();
        let prefix = 121; // mint .. real_token_reserves
        let body = &bytes[8..8 + prefix + TRADE_EVENT_FEE_FIELDS_LEN + 31];
        let ev = decode_trade_event_body(body, 1e9).unwrap();
        assert_eq!(ev.curve_creator, None);
        assert!(ev.token_amount > 0);
    }
}
