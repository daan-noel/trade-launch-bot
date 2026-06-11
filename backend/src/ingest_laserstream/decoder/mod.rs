use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde_json::{json, Value};
use tracing::{debug, warn};

use crate::config::constants::{
    MIGRATE_INSTRUCTION_DISCRIMINATOR, MIGRATE_V2_INSTRUCTION_DISCRIMINATOR, PUMP_SWAP_PROGRAM_ID,
};
use crate::models::{
    events::{InternalEvent, TokenMigratedEvent, TradeExecutedEvent},
    trade::{Trade, TradeType},
    transaction::RawTransaction,
};

mod create;
mod instructions;
mod parse;
mod trade;

pub use self::instructions::InstructionKind;

use self::create::decode_create_events_from_logs;
use self::instructions::{
    build_instruction_labels, collect_instruction_kinds, determine_instruction_type,
    instruction_data_bytes,
};
use self::parse::{
    extract_account_keys, extract_balances, extract_logs, find_pump_ixs_anywhere,
    is_pump_create_ix, resolve_pump_accounts,
};
use self::trade::{
    build_amm_trade, decode_pump_swap_trades_from_logs, decode_trade_events_from_inner_ixs,
    decode_trade_events_from_logs, DecodedAmmTrade,
};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub struct HeliusDecoder {
    pump_program_id: String,
    /// Shared pool→mint index for resolving live PumpSwap (AMM) swaps, whose
    /// events carry the pool but not the base mint. `None` in contexts that
    /// decode AMM trades with an explicit pool already in hand (token sync).
    pool_index: Option<Arc<DashMap<String, String>>>,
}

/// Outcome of decoding one raw Helius WebSocket message.
pub enum DecodeOutput {
    /// Subscription was acknowledged; contains the subscription ID.
    Subscribed(u64),
    /// A Pump.fun transaction was decoded successfully.
    Transaction {
        raw_tx: RawTransaction,
        events: Vec<InternalEvent>,
    },
    /// Message was not relevant (other program, ping, unrecognised format, etc.)
    Ignored,
}

impl HeliusDecoder {
    pub fn new(pump_program_id: String) -> Self {
        Self {
            pump_program_id,
            pool_index: None,
        }
    }

    /// Attach a shared pool→mint index, enabling the live decode path to
    /// recognise post-migration PumpSwap (AMM) swaps and attribute them back to
    /// the tracked mint that owns the pool.
    pub fn with_pool_index(mut self, index: Arc<DashMap<String, String>>) -> Self {
        self.pool_index = Some(index);
        self
    }

    /// Entry point: decode one raw WS text frame.
    pub fn decode(&self, raw: &str) -> DecodeOutput {
        let value: Value = match serde_json::from_str(raw) {
            Ok(v) => v,
            Err(e) => {
                warn!("Malformed Helius message (not JSON): {e}");
                return DecodeOutput::Ignored;
            }
        };

        // Subscription confirmation: {"jsonrpc":"2.0","result":12345,"id":1}
        if value.get("method").is_none() {
            if let Some(id) = value.get("result").and_then(|r| r.as_u64()) {
                return DecodeOutput::Subscribed(id);
            }
            return DecodeOutput::Ignored;
        }

        if value["method"].as_str() != Some("transactionNotification") {
            return DecodeOutput::Ignored;
        }

        self.decode_result(&value["params"]["result"])
    }

    /// Decode a Helius `params.result` object (WebSocket notification or wrapped RPC `getTransaction`).
    pub fn decode_result(&self, result: &Value) -> DecodeOutput {
        let signature = match result["signature"].as_str() {
            Some(s) => s.to_string(),
            None => {
                warn!("transactionNotification missing signature");
                return DecodeOutput::Ignored;
            }
        };

        let slot = result["slot"].as_u64().unwrap_or(0);

        // blockTime is a Unix timestamp (seconds). Helius may place it at
        // different nesting levels depending on the version; try both.
        let block_time = result["blockTime"]
            .as_i64()
            .or_else(|| result["transaction"]["blockTime"].as_i64())
            .or_else(|| result["transaction"]["meta"]["blockTime"].as_i64())
            .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
            .unwrap_or_else(Utc::now);

        let meta = &result["transaction"]["meta"];
        let message = &result["transaction"]["transaction"]["message"];

        let logs = extract_logs(meta);

        // Fast-path: a bonding-curve transaction mentions the pump.fun program.
        // If it doesn't, it may still be a post-migration PumpSwap (AMM) swap for
        // a token we track — those touch the AMM program instead and carry the
        // pool rather than the base mint, so they're resolved via `pool_index`.
        if !logs.iter().any(|l| l.contains(&self.pump_program_id)) {
            if self.pool_index.is_some()
                && logs.iter().any(|l| l.contains(PUMP_SWAP_PROGRAM_ID))
            {
                return self.decode_amm_live(result, &logs);
            }
            return DecodeOutput::Ignored;
        }

        let raw_tx = RawTransaction::new(signature.clone(), slot, block_time, result.clone());

        let account_keys = extract_account_keys(message);
        let pre_balances = extract_balances(&meta["preBalances"]);
        let post_balances = extract_balances(&meta["postBalances"]);

        // ── Step 1a: decode all TradeEvents from "Program data:" log lines ────
        // Emitted by pump.fun via `emit!` for EVERY buy/sell, regardless of
        // whether the pump.fun instruction is outer or an inner CPI call.
        // They carry authoritative on-chain amounts + virtual reserve snapshot.
        let mut decoded_events = decode_trade_events_from_logs(&logs);

        // Solana truncates a transaction's logs once they exceed its size limit
        // (common on large/bundled txs), which can drop the "Program data:"
        // TradeEvent line entirely. Pump.fun also emits the SAME event via
        // `emit_cpi!` as an inner instruction to the event authority, and inner
        // instructions are never truncated — so recover the events there before
        // resorting to the lossy balance-delta fallback (step 3b). Balance
        // deltas conflate the swap with other SOL/token movement in the tx and
        // carry no reserve snapshot, producing mispriced trades.
        if decoded_events.is_empty() {
            decoded_events = decode_trade_events_from_inner_ixs(
                message,
                meta,
                &account_keys,
                &self.pump_program_id,
            );
        }

        let decoded_create_events = decode_create_events_from_logs(&logs);

        // ── Step 1b: identify high-level instruction kinds from message.instructions
        // Resolve program IDs correctly from each instruction and classify by
        // known Pump.fun discriminators instead of relying on log text.
        // Note: Both SPL Token and Token-2022 use identical discriminators; token standard
        // is determined by examining instruction accounts, not the discriminator itself.
        let kinds = collect_instruction_kinds(message, meta, &account_keys);

        // Determine the primary instruction type for this transaction.
        // If both buy and sell are present, choose whichever side moved more SOL.
        let instruction_type = determine_instruction_type(&kinds, &decoded_events);

        // ── Step 2: build instruction-order labels from message.instructions ───
        let (instruction_labels, cu_limit, cu_price) =
            build_instruction_labels(message, &account_keys);
        let labels_json = json!(instruction_labels);

        debug!(
            sig = %signature,
            instruction_type = %instruction_type,
            trade_events = decoded_events.len(),
            labels = ?instruction_labels,
            cu_limit = ?cu_limit,
            cu_price = ?cu_price,
            "Decoded Pump.fun transaction"
        );

        // ── Step 3: build InternalEvents ─────────────────────────────────────

        let mut events = Vec::new();

        // 3a. For each decoded TradeEvent: emit TradeExecuted.
        //     Covers Buy/Sell even when pump.fun is a nested CPI call.
        for (leg_index, ev) in decoded_events.iter().enumerate() {
            if Trade::is_dust(ev.sol_amount) {
                continue;
            }

            let trade_type = if ev.is_buy {
                TradeType::Buy
            } else {
                TradeType::Sell
            };

            let mut trade = Trade::new(
                ev.mint.clone(),
                ev.user.clone(),
                trade_type,
                ev.sol_amount,
                ev.token_amount,
                signature.clone(),
                slot,
                block_time,
            );
            trade.virtual_sol_reserves = Some(ev.virtual_sol_reserves);
            trade.virtual_token_reserves = Some(ev.virtual_token_reserves);
            trade.real_sol_reserves = Some(ev.real_sol_reserves);
            trade.real_token_reserves = Some(ev.real_token_reserves);
            trade.instruction_type = match trade.trade_type {
                TradeType::Buy => "Buy".to_string(),
                TradeType::Sell => "Sell".to_string(),
            };
            trade.instruction_labels = labels_json.clone();
            trade.leg_index = leg_index as u32;
            trade.received_at = raw_tx.received_at;

            events.push(InternalEvent::TradeExecuted(TradeExecutedEvent {
                trade,
                tx_signature: signature.clone(),
                slot,
                timestamp: raw_tx.received_at,
                raw_tx: raw_tx.clone(),
            }));
        }

        // 3b. If no TradeEvent was decoded but logs indicate a buy/sell (rare
        //     edge case), fall back to balance-delta extraction.
        if decoded_events.is_empty() {
            for kind in kinds
                .iter()
                .filter(|k| matches!(k, InstructionKind::Buy | InstructionKind::Sell))
            {
                let pump_ixs =
                    find_pump_ixs_anywhere(message, meta, &account_keys, &self.pump_program_id);
                if let Some(pump_ix) = pump_ixs.first() {
                    let ix_accounts = resolve_pump_accounts(pump_ix, &account_keys);
                    if let Some(ev) = self.decode_trade_from_balances(
                        *kind,
                        &signature,
                        slot,
                        block_time,
                        &ix_accounts,
                        &account_keys,
                        &pre_balances,
                        &post_balances,
                        meta,
                        &labels_json,
                        &raw_tx,
                    ) {
                        events.extend(ev);
                    }
                }
            }
        }

        // 3c. Handle Create — look for the Create ix in outer OR inner instructions.
        let has_create = kinds.iter().any(|k| matches!(k, InstructionKind::Create));
        if has_create {
            let pump_ixs =
                find_pump_ixs_anywhere(message, meta, &account_keys, &self.pump_program_id);
            if let Some(create_ix) = pump_ixs.iter().find(|ix| is_pump_create_ix(ix)) {
                let ix_accounts = resolve_pump_accounts(create_ix, &account_keys);
                events.extend(self.decode_create(
                    &signature,
                    slot,
                    block_time,
                    create_ix,
                    &ix_accounts,
                    &account_keys,
                    &decoded_events,
                    &decoded_create_events,
                    &instruction_type,
                    &labels_json,
                    cu_limit,
                    cu_price,
                    &raw_tx,
                    message,
                    meta,
                ));
            }
        }

        let has_migrate = kinds.iter().any(|k| matches!(k, InstructionKind::Migrate));
        if has_migrate {
            let pump_ixs =
                find_pump_ixs_anywhere(message, meta, &account_keys, &self.pump_program_id);
            if let Some(migrate_ix) = pump_ixs.iter().find(|ix| {
                if let Some(bytes) = instruction_data_bytes(ix).as_deref() {
                    bytes.len() >= 8
                        && (bytes[..8] == MIGRATE_INSTRUCTION_DISCRIMINATOR
                            || bytes[..8] == MIGRATE_V2_INSTRUCTION_DISCRIMINATOR)
                } else {
                    false
                }
            }) {
                let ix_accounts = resolve_pump_accounts(migrate_ix, &account_keys);
                if let Some(ev) = self.decode_migrate(
                    &signature,
                    slot,
                    block_time,
                    migrate_ix,
                    &ix_accounts,
                    &raw_tx,
                ) {
                    events.push(ev);
                }
            }
        }

        if events.is_empty() && instruction_type == "Unknown" {
            debug!("Pump.fun tx {signature}: no decodable events (may be FeeOp or new ix type)");
        }

        DecodeOutput::Transaction { raw_tx, events }
    }

    /// Decode a wrapped `getTransaction` result as a post-migration PumpSwap
    /// (pump_amm) swap. Unlike [`decode_result`], this path keys off the
    /// PumpSwap program and its `BuyEvent`/`SellEvent` "Program data:" logs
    /// rather than the bonding-curve program.
    ///
    /// `mint` is the token being synced (PumpSwap events don't carry the base
    /// mint), and `pool` is its derived PumpSwap pool — events for any other
    /// pool in the same transaction are ignored. Returns the raw transaction
    /// and one [`Trade`] per matching swap leg, or `None` if the transaction
    /// has no PumpSwap swap for this pool.
    pub fn decode_pump_swap_result(
        &self,
        result: &Value,
        mint: &str,
        pool: &str,
    ) -> Option<(RawTransaction, Vec<Trade>)> {
        let signature = result["signature"].as_str()?.to_string();
        let slot = result["slot"].as_u64().unwrap_or(0);
        let block_time = result["blockTime"]
            .as_i64()
            .or_else(|| result["transaction"]["blockTime"].as_i64())
            .or_else(|| result["transaction"]["meta"]["blockTime"].as_i64())
            .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
            .unwrap_or_else(Utc::now);

        let meta = &result["transaction"]["meta"];
        let message = &result["transaction"]["transaction"]["message"];

        let logs = extract_logs(meta);
        if !logs.iter().any(|l| l.contains(PUMP_SWAP_PROGRAM_ID)) {
            return None;
        }

        let raw_tx = RawTransaction::new(signature.clone(), slot, block_time, result.clone());
        let account_keys = extract_account_keys(message);
        let (labels, _, _) = build_instruction_labels(message, &account_keys);
        let labels_json = json!(labels);

        let mut trades = Vec::new();
        for ev in decode_pump_swap_trades_from_logs(&logs) {
            // Multi-hop routes can touch several pools in one tx; keep only ours.
            if ev.pool != pool {
                continue;
            }
            if Trade::is_dust(ev.quote_amount) {
                continue;
            }
            let trade = build_amm_trade(
                &ev,
                mint,
                &signature,
                slot,
                block_time,
                &labels_json,
                trades.len() as u32,
                raw_tx.received_at,
            );
            trades.push(trade);
        }

        if trades.is_empty() {
            None
        } else {
            Some((raw_tx, trades))
        }
    }

    /// Live-ingest decode of a PumpSwap (AMM) transaction delivered by the WS
    /// subscription. Mirrors [`decode_pump_swap_result`] but, since the WS stream
    /// gives us no mint up front, resolves each swap's `pool` to a tracked mint
    /// via the shared pool→mint index. Swaps for pools we don't track are
    /// dropped. `logs` is reused from the caller to avoid re-extracting it for
    /// the many AMM transactions that don't concern us.
    fn decode_amm_live(&self, result: &Value, logs: &[&str]) -> DecodeOutput {
        let Some(index) = self.pool_index.as_ref() else {
            return DecodeOutput::Ignored;
        };

        // Decode swaps and resolve pool→mint first; bail without cloning the
        // (large) result JSON when none of the pools are ones we track.
        let resolved: Vec<(DecodedAmmTrade, String)> = decode_pump_swap_trades_from_logs(logs)
            .into_iter()
            .filter(|ev| !Trade::is_dust(ev.quote_amount))
            .filter_map(|ev| index.get(&ev.pool).map(|m| (ev, m.value().clone())))
            .collect();
        if resolved.is_empty() {
            return DecodeOutput::Ignored;
        }

        let signature = match result["signature"].as_str() {
            Some(s) => s.to_string(),
            None => return DecodeOutput::Ignored,
        };
        let slot = result["slot"].as_u64().unwrap_or(0);
        let block_time = result["blockTime"]
            .as_i64()
            .or_else(|| result["transaction"]["blockTime"].as_i64())
            .or_else(|| result["transaction"]["meta"]["blockTime"].as_i64())
            .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
            .unwrap_or_else(Utc::now);

        let message = &result["transaction"]["transaction"]["message"];
        let raw_tx = RawTransaction::new(signature.clone(), slot, block_time, result.clone());
        let account_keys = extract_account_keys(message);
        let (labels, _, _) = build_instruction_labels(message, &account_keys);
        let labels_json = json!(labels);

        let mut events = Vec::with_capacity(resolved.len());
        for (ev, mint) in &resolved {
            let trade = build_amm_trade(
                ev,
                mint,
                &signature,
                slot,
                block_time,
                &labels_json,
                events.len() as u32,
                raw_tx.received_at,
            );
            events.push(InternalEvent::TradeExecuted(TradeExecutedEvent {
                trade,
                tx_signature: signature.clone(),
                slot,
                timestamp: raw_tx.received_at,
                raw_tx: raw_tx.clone(),
            }));
        }

        DecodeOutput::Transaction { raw_tx, events }
    }

    fn decode_migrate(
        &self,
        signature: &str,
        slot: u64,
        block_time: DateTime<Utc>,
        _pump_ix: &Value,
        pump_accounts: &[String],
        raw_tx: &RawTransaction,
    ) -> Option<InternalEvent> {
        let mint = pump_accounts.get(2)?.to_string();
        if mint.is_empty() {
            return None;
        }

        Some(InternalEvent::TokenMigrated(TokenMigratedEvent {
            mint_address: mint,
            tx_signature: signature.to_string(),
            slot,
            timestamp: block_time,
            raw_tx: raw_tx.clone(),
        }))
    }
}
