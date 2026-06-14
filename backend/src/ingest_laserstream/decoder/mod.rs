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
    instruction_data_bytes, prepare_instructions,
};
use self::parse::{
    extract_account_keys, extract_balances, extract_logs, find_pump_ixs_anywhere,
    is_pump_create_ix, log_lines, resolve_pump_accounts,
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

/// Outcome of decoding one LaserStream transaction update.
pub enum DecodeOutput {
    /// A Pump.fun transaction was decoded successfully.
    Transaction {
        /// Shared so each embedded event clones a pointer, not the full JSON.
        raw_tx: Arc<RawTransaction>,
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

    /// Decode a Helius `params.result` object (LaserStream update or wrapped RPC `getTransaction`).
    ///
    /// Takes `result` **by value** and moves it into the shared
    /// [`RawTransaction`]; the decode then borrows the `meta`/`message` subtrees
    /// out of that stored copy, so the (large) JSON tree is moved once instead of
    /// deep-cloned to keep it borrowable.
    pub fn decode_result(&self, result: Value) -> DecodeOutput {
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

        // Gate on logs before taking ownership of the JSON. A bonding-curve tx
        // mentions the pump.fun program; if it doesn't, it may still be a
        // post-migration PumpSwap (AMM) swap for a token we track — those touch
        // the AMM program instead and carry the pool rather than the base mint,
        // so they're resolved via `pool_index`. The borrow of `result` ends with
        // this block so the Value can then be moved.
        {
            let meta = &result["transaction"]["meta"];
            if !log_lines(meta).any(|l| l.contains(&self.pump_program_id)) {
                if self.pool_index.is_some()
                    && log_lines(meta).any(|l| l.contains(PUMP_SWAP_PROGRAM_ID))
                {
                    return self.decode_amm_live(result);
                }
                return DecodeOutput::Ignored;
            }
        }

        // Move the JSON into the shared raw tx and borrow its subtrees from there.
        let raw_tx = Arc::new(RawTransaction::new(signature.clone(), slot, block_time, result));
        let meta = &raw_tx.raw_data["transaction"]["meta"];
        let message = &raw_tx.raw_data["transaction"]["transaction"]["message"];
        let logs = extract_logs(meta);

        let account_keys = extract_account_keys(message);
        let pre_balances = extract_balances(&meta["preBalances"]);
        let post_balances = extract_balances(&meta["postBalances"]);

        // Decode each outer instruction's base58 `data` once, reused below for
        // both kind-classification and label-building.
        let outer_ixs = prepare_instructions(message, &account_keys);

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
        let kinds = collect_instruction_kinds(&outer_ixs, meta, &account_keys);

        // Determine the primary instruction type for this transaction.
        // If both buy and sell are present, choose whichever side moved more SOL.
        let instruction_type = determine_instruction_type(&kinds, &decoded_events);

        // ── Step 2: build instruction-order labels from message.instructions ───
        let (instruction_labels, cu_limit, cu_price) = build_instruction_labels(&outer_ixs);
        let mut labels_json = json!(instruction_labels);

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
        let last_leg = decoded_events.len().saturating_sub(1);
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
            // The same labels JSON applies to every leg of the tx. Move it into
            // the final leg instead of cloning again — the common single-leg tx
            // (most trades) then does zero clones; a multi-leg bundle saves the
            // last one. `labels_json` is unused after the loop.
            trade.instruction_labels = if leg_index == last_leg {
                std::mem::take(&mut labels_json)
            } else {
                labels_json.clone()
            };
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

        // The pump-instruction walk (`find_pump_ixs_anywhere`) is needed only by
        // the balance-delta fallback (3b), Create (3c), and Migrate below — never
        // by the common pure-trade tx, whose events all came from logs/inner-ixs.
        // Resolve it at most once here and reuse it, instead of re-walking every
        // instruction up to 3× per tx.
        let has_create = kinds.iter().any(|k| matches!(k, InstructionKind::Create));
        let has_migrate = kinds.iter().any(|k| matches!(k, InstructionKind::Migrate));
        let needs_pump_ixs = (decoded_events.is_empty()
            && kinds
                .iter()
                .any(|k| matches!(k, InstructionKind::Buy | InstructionKind::Sell)))
            || has_create
            || has_migrate;
        let pump_ixs = if needs_pump_ixs {
            find_pump_ixs_anywhere(message, meta, &account_keys, &self.pump_program_id)
        } else {
            Vec::new()
        };

        // 3b. If no TradeEvent was decoded but logs indicate a buy/sell (rare
        //     edge case), fall back to balance-delta extraction.
        if decoded_events.is_empty() {
            for kind in kinds
                .iter()
                .filter(|k| matches!(k, InstructionKind::Buy | InstructionKind::Sell))
            {
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
        if has_create {
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

        if has_migrate {
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
        let (labels, _, _) = build_instruction_labels(&prepare_instructions(message, &account_keys));
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

    /// Live-ingest decode of a PumpSwap (AMM) transaction delivered by the
    /// LaserStream subscription. Mirrors [`decode_pump_swap_result`] but, since
    /// the stream gives us no mint up front, resolves each swap's `pool` to a tracked mint
    /// via the shared pool→mint index. Swaps for pools we don't track are
    /// dropped. Takes `result` **by value** and moves it into the shared raw tx
    /// (no deep clone), borrowing its subtrees from there for the decode.
    fn decode_amm_live(&self, result: Value) -> DecodeOutput {
        let Some(index) = self.pool_index.as_ref() else {
            return DecodeOutput::Ignored;
        };

        // Decode swaps and resolve pool→mint first; bail before moving the JSON
        // into the raw tx when none of the pools are ones we track. The `logs`
        // borrow ends with this block so the Value can then be moved.
        let resolved: Vec<(DecodedAmmTrade, String)> = {
            let logs = extract_logs(&result["transaction"]["meta"]);
            decode_pump_swap_trades_from_logs(&logs)
                .into_iter()
                .filter(|ev| !Trade::is_dust(ev.quote_amount))
                .filter_map(|ev| index.get(&ev.pool).map(|m| (ev, m.value().clone())))
                .collect()
        };
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

        let raw_tx = Arc::new(RawTransaction::new(signature.clone(), slot, block_time, result));
        let message = &raw_tx.raw_data["transaction"]["transaction"]["message"];
        let account_keys = extract_account_keys(message);
        let (labels, _, _) = build_instruction_labels(&prepare_instructions(message, &account_keys));
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
        raw_tx: &Arc<RawTransaction>,
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
