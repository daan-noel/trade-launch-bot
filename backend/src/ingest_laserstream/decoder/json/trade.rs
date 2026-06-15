//! `Value`-path trade helpers: the inner-instruction `TradeEvent` self-CPI
//! decoder and the balance-delta fallback, reading the Helius `jsonParsed`
//! `transaction`/`meta` JSON. Siblings of the protobuf versions in
//! [`super::super::grpc::trade`]; the Borsh leaves ([`RawTradeEvent`]) and
//! `compute_sol_change` are shared from [`super::super::trade`].

use std::sync::Arc;

use borsh::BorshDeserialize;
use chrono::{DateTime, Utc};
use serde_json::Value;
use tracing::warn;

use crate::config::constants::{ANCHOR_EVENT_CPI_DISCRIMINATOR, TRADE_EVENT_DISCRIMINATOR};
use crate::models::{
    events::{InternalEvent, TradeExecutedEvent},
    trade::{Trade, TradeType},
    transaction::RawTransaction,
};

use super::super::instructions::InstructionKind;
use super::super::trade::{compute_sol_change, DecodedTradeEvent, RawTradeEvent};
use super::super::HeliusDecoder;
use super::parse::{compute_token_change, find_pump_ixs_anywhere};

/// Scan inner instructions for pump.fun's `emit_cpi!` TradeEvent self-CPI and
/// decode each one. The instruction data is the 8-byte Anchor event-CPI tag,
/// followed by the 8-byte TradeEvent discriminator, followed by the same
/// Borsh-encoded `RawTradeEvent` carried in the "Program data:" log. This is the
/// truncation-proof source used when the log decode finds nothing because Solana
/// truncated the transaction's logs.
pub(super) fn decode_trade_events_from_inner_ixs(
    message: &Value,
    meta: &Value,
    account_keys: &[&str],
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

impl HeliusDecoder {
    /// Balance-delta fallback for Buy/Sell when no "Program data:" TradeEvent
    /// was found (unusual, but guards against edge cases).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn decode_trade_from_balances(
        &self,
        kind: InstructionKind,
        signature: &str,
        slot: u64,
        block_time: DateTime<Utc>,
        pump_accounts: &[String],
        account_keys: &[&str],
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
