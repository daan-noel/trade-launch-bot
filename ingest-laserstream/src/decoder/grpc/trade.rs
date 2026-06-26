//! Protobuf-native trade helpers: the inner-instruction `TradeEvent` self-CPI
//! decoder and the balance-delta fallback, reading the typed Yellowstone structs
//! ([`PbIx`], [`scb::TokenBalance`]). The Borsh leaves ([`RawTradeEvent`]) and
//! `compute_sol_change` are shared from [`super::super::trade`].

use std::sync::Arc;

use borsh::BorshDeserialize;
use chrono::{DateTime, Utc};
use serde_json::Value;
use tracing::warn;

use backend_core::config::constants::{ANCHOR_EVENT_CPI_DISCRIMINATOR, TRADE_EVENT_DISCRIMINATOR};
use crate::proto::solana::storage::confirmed_block as scb;
use backend_core::models::{
    events::{InternalEvent, TradeExecutedEvent},
    trade::{Trade, TradeType},
    transaction::RawTransaction,
};

use super::super::instructions::InstructionKind;
use super::super::trade::{compute_sol_change, DecodedTradeEvent, RawTradeEvent};
use super::super::HeliusDecoder;
use super::PbIx;

/// Decode `TradeEvent`s from pump.fun's `emit_cpi!` inner instructions — the
/// truncation-proof source used when the "Program data:" logs were dropped. The
/// pump instructions arrive already resolved into [`PbIx`] (program id + raw
/// `data` bytes), so this reads the Borsh layout directly with no base58 decode.
pub(super) fn decode_trade_events_from_inner_pb(pump_ixs: &[PbIx]) -> Vec<DecodedTradeEvent> {
    let mut events = Vec::new();

    for ix in pump_ixs {
        let bytes = ix.data;
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
                warn!("Failed to Borsh-decode TradeEvent from inner ix (pb): {e}");
                continue;
            }
        };

        events.push(DecodedTradeEvent::from_raw(raw));
    }

    events
}

/// Token-amount delta for `user_ata`, read from the typed `TokenBalance` lists.
/// Returns RAW base units (no decimal scaling), to match the authoritative
/// log-event path.
fn compute_token_change_pb(
    user_ata: &str,
    mint: &str,
    account_keys: &[&str],
    pre: &[scb::TokenBalance],
    post: &[scb::TokenBalance],
) -> f64 {
    let ata_idx = match account_keys.iter().position(|k| *k == user_ata) {
        Some(i) => i as u32,
        None => return 0.0,
    };

    let find_amount = |balances: &[scb::TokenBalance]| -> f64 {
        balances
            .iter()
            .find(|tb| tb.account_index == ata_idx && tb.mint == mint)
            .and_then(|tb| tb.ui_token_amount.as_ref())
            .and_then(|u| u.amount.parse::<f64>().ok())
            .unwrap_or(0.0)
    };

    (find_amount(post) - find_amount(pre)).abs()
}

impl HeliusDecoder {
    /// Balance-delta fallback for the rare tx with no decodable `TradeEvent`,
    /// reading the typed balance/token-balance lists from the protobuf `meta`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn decode_trade_from_balances_pb(
        &self,
        kind: InstructionKind,
        signature: &str,
        slot: u64,
        block_time: DateTime<Utc>,
        pump_accounts: &[String],
        account_keys: &[&str],
        pre_balances: &[u64],
        post_balances: &[u64],
        pre_token_balances: &[scb::TokenBalance],
        post_token_balances: &[scb::TokenBalance],
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
        let token_amount = compute_token_change_pb(
            &user_ata,
            &mint,
            account_keys,
            pre_token_balances,
            post_token_balances,
        );

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
