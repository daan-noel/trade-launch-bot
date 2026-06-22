//! Protobuf-native decode of LaserStream transaction updates (`decode_protobuf`).
//!
//! The single decode path for both live ingest and token_sync (which lowers its
//! base64 RPC results to the same `SubscribeUpdateTransaction` via
//! [`super::super::adapter_rpc::rpc_to_protobuf`]). Reads the Yellowstone protobuf
//! structs directly, so the hot path never builds a heap-heavy `Value`; the
//! persisted raw blob is synthesised off-thread in the DbWriter (see
//! [`super::super::adapter::build_raw_blob`]). Borrowed/byte-level leaves (Borsh
//! event decoders, `classify_pump_ix`, `label_instruction`,
//! `determine_instruction_type`, `build_amm_trade`, `decode_create`) are shared
//! with the root decoder module.

use std::cell::OnceCell;
use std::sync::{Arc, OnceLock};

use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::config::constants::{
    CREATE_INSTRUCTION_DISCRIMINATOR, CREATE_V2_INSTRUCTION_DISCRIMINATOR,
    MIGRATE_INSTRUCTION_DISCRIMINATOR, MIGRATE_V2_INSTRUCTION_DISCRIMINATOR, PUMP_FUN_PROGRAM_ID,
    PUMP_SWAP_PROGRAM_ID,
};
use crate::models::{
    events::{InternalEvent, TradeExecutedEvent},
    trade::{Trade, TradeType},
    transaction::RawTransaction,
};

use super::super::proto::geyser::{SubscribeUpdateTransaction, SubscribeUpdateTransactionInfo};
use super::super::proto::solana::storage::confirmed_block as scb;

use super::create::decode_create_events_from_logs;
use super::instructions::{
    classify_pump_ix, determine_instruction_type, extract_compute_budget, label_instruction,
    InstructionKind,
};
use super::trade::{
    build_amm_trade, decode_pump_swap_trades_from_logs, decode_trade_events_from_logs,
    DecodedAmmTrade,
};
use super::{DecodeOutput, HeliusDecoder, TxRelevance};

mod trade;
use trade::decode_trade_events_from_inner_pb;

/// One pump.fun instruction in borrowed protobuf form: the index of its program
/// id in the account-key list plus its raw `data` / `accounts` index bytes,
/// borrowed straight from the protobuf — no base58 round-trip, no `Value`. The
/// program id stays an index (not a resolved `&str`) so the hot path can compare
/// it on raw bytes via [`LazyKeys`] and only base58-encode the few keys it
/// actually emits. Private to the `grpc` subtree — `grpc::trade` reads it as a
/// descendant module (and only touches `data`).
struct PbIx<'a> {
    program_id_index: u32,
    accounts: &'a [u8],
    data: &'a [u8],
}

/// The transaction's account-key list (`static ++ loaded writable ++ loaded
/// readonly` — the canonical order, a wrong shuffle of which misattributes
/// wallets/mints) kept as borrowed raw 32-byte slices, base58-encoded **lazily**
/// and memoized per index. The decode hot path (a log-derived trade) reads the
/// mint/user from the event payload, never the account keys — only program-id
/// *comparisons* (done on raw bytes here) and the rare create/migrate/balance
/// paths dereference them — so eagerly encoding every key was pure waste.
struct LazyKeys<'a> {
    raw: Vec<&'a [u8]>,
    encoded: Vec<OnceCell<String>>,
}

impl<'a> LazyKeys<'a> {
    fn new(message: &'a scb::Message, meta: &'a scb::TransactionStatusMeta) -> Self {
        let raw: Vec<&[u8]> = message
            .account_keys
            .iter()
            .chain(meta.loaded_writable_addresses.iter())
            .chain(meta.loaded_readonly_addresses.iter())
            .map(|k| k.as_slice())
            .collect();
        let encoded = raw.iter().map(|_| OnceCell::new()).collect();
        Self { raw, encoded }
    }

    /// Raw 32-byte key at `i`, for byte-level program-id comparison (no encode).
    fn raw(&self, i: usize) -> Option<&'a [u8]> {
        self.raw.get(i).copied()
    }

    /// Base58 of the key at `i` (memoized); `""` if the index is out of range —
    /// matching the old `keys.get(i).copied().unwrap_or("")`.
    fn get(&self, i: usize) -> &str {
        match self.raw.get(i) {
            Some(bytes) => self.encoded[i]
                .get_or_init(|| bs58::encode(bytes).into_string())
                .as_str(),
            None => "",
        }
    }

    /// Force-encode every key into an owned `&[&str]` view, for the rare paths
    /// (create, balance-delta fallback) that resolve accounts by string search.
    fn all(&self) -> Vec<&str> {
        (0..self.raw.len()).map(|i| self.get(i)).collect()
    }
}

impl HeliusDecoder {
    /// Protobuf-native decode of a live LaserStream transaction update.
    ///
    /// `received_at` is the single ingest clock for this tx — used for the
    /// trades' `received_at`, the (null-data) raw-tx carrier, and (since
    /// protobuf tx updates carry no `blockTime`, exactly as the `Value` live
    /// path) the `block_time` fallback. The DbWriter is handed the same
    /// `received_at` so the persisted blob keeps the real ingest time, not
    /// flush time.
    pub fn decode_protobuf(
        &self,
        update: &SubscribeUpdateTransaction,
        received_at: DateTime<Utc>,
    ) -> DecodeOutput {
        // Self-contained entry (token_sync's RPC backfill, replay, tests): classify
        // the tx by scanning its logs, then dispatch. The live pipeline instead
        // forwards the client's already-computed verdict via `decode_relevant_pb`,
        // so the per-tx log scan isn't run twice on the hot path.
        let Some(meta) = update
            .transaction
            .as_ref()
            .and_then(|info| info.meta.as_ref())
        else {
            return DecodeOutput::Ignored;
        };
        match self.classify_logs(&meta.log_messages) {
            Some(relevance) => self.decode_relevant_pb(update, relevance, received_at),
            None => DecodeOutput::Ignored,
        }
    }

    /// Classify a tx by its log messages, mirroring the LaserStream client's
    /// pre-filter: a bonding-curve tx mentions the pump.fun program (curve takes
    /// priority); otherwise a post-migration PumpSwap (AMM) swap, but only when a
    /// `pool_index` is attached to resolve it. `None` = not relevant, drop it.
    pub(crate) fn classify_logs(&self, logs: &[String]) -> Option<TxRelevance> {
        if logs.iter().any(|l| l.contains(&self.pump_program_id)) {
            Some(TxRelevance::Curve)
        } else if self.pool_index.is_some()
            && logs.iter().any(|l| l.contains(PUMP_SWAP_PROGRAM_ID))
        {
            Some(TxRelevance::Amm)
        } else {
            None
        }
    }

    /// Decode a tx whose relevance the caller already determined — the live
    /// pipeline forwards the LaserStream client's log-gate verdict, so the curve-
    /// vs-AMM log scan `decode_protobuf` performs is not repeated here. See
    /// [`Self::decode_protobuf`] for the `received_at` ingest-clock contract.
    pub fn decode_relevant_pb(
        &self,
        update: &SubscribeUpdateTransaction,
        relevance: TxRelevance,
        received_at: DateTime<Utc>,
    ) -> DecodeOutput {
        let Some(info) = update.transaction.as_ref() else {
            return DecodeOutput::Ignored;
        };
        let Some(tx) = info.transaction.as_ref() else {
            return DecodeOutput::Ignored;
        };
        let Some(message) = tx.message.as_ref() else {
            return DecodeOutput::Ignored;
        };
        let Some(meta) = info.meta.as_ref() else {
            return DecodeOutput::Ignored;
        };

        let slot = update.slot;
        let block_time = received_at;

        match relevance {
            TxRelevance::Curve => {
                self.decode_curve_pb(info, message, meta, slot, block_time, received_at)
            }
            TxRelevance::Amm => {
                self.decode_amm_live_pb(info, message, meta, slot, block_time, received_at)
            }
        }
    }

    /// Decode a bonding-curve (pump.fun program) tx into its trade / create /
    /// migrate events. Reached once the tx is known to touch the curve program.
    fn decode_curve_pb(
        &self,
        info: &SubscribeUpdateTransactionInfo,
        message: &scb::Message,
        meta: &scb::TransactionStatusMeta,
        slot: u64,
        block_time: DateTime<Utc>,
        received_at: DateTime<Utc>,
    ) -> DecodeOutput {
        let signature = bs58::encode(&info.signature).into_string();

        // accountKeys = static ++ loaded writable ++ loaded readonly, same order
        // the adapter/`Value` path used (a wrong order misattributes wallets/mints).
        // Encoded lazily — the common log-derived trade never dereferences them.
        let keys = LazyKeys::new(message, meta);

        // Null-data raw-tx carrier: events embed an `Arc<RawTransaction>` but
        // never read it — the persisted blob is built off-thread in the DbWriter.
        // The carrier exists only to give events their shared pointer + the real
        // ingest clock, without the `Value` synthesis this plan moves off-thread.
        let raw_tx = Arc::new(raw_tx_carrier(signature.clone(), slot, block_time, received_at));

        let logs: Vec<&str> = meta.log_messages.iter().map(String::as_str).collect();
        let pre_balances: &[u64] = &meta.pre_balances;
        let post_balances: &[u64] = &meta.post_balances;

        let outer_ixs: Vec<PbIx> = message
            .instructions
            .iter()
            .map(|ix| pb_ix(ix.program_id_index, &ix.accounts, &ix.data))
            .collect();

        // ── Step 1a: TradeEvents from "Program data:" logs, with the inner-ix
        // self-CPI fallback when logs were truncated. ───────────────────────────
        let mut decoded_events = decode_trade_events_from_logs(&logs);
        if decoded_events.is_empty() {
            let pump_ixs = find_pump_pb_ixs(message, meta, &keys, &self.pump_program_id_bytes);
            decoded_events = decode_trade_events_from_inner_pb(&pump_ixs);
        }

        let decoded_create_events = decode_create_events_from_logs(&logs);

        // ── Step 1b: instruction kinds ─────────────────────────────────────────
        let kinds = collect_kinds_pb(&outer_ixs, meta, &keys);
        let instruction_type = determine_instruction_type(&kinds, &decoded_events);

        // ── Step 2: instruction-order labels (+ compute budget) ─────────────────
        let (instruction_labels, cu_limit, cu_price) = build_labels_pb(&outer_ixs, &keys);
        let mut labels_json = json!(instruction_labels);

        // ── Step 3: build InternalEvents ───────────────────────────────────────
        let mut events = Vec::new();

        // A create tx (almost always bundled with the dev's initial buy) reads
        // `labels_json` *after* the trade loop to stamp the token's labels, so the
        // loop must not consume it via `mem::take` in that case — otherwise the
        // token's `instruction_labels` end up Null. Computed before the loop so the
        // last-leg branch can decide whether the take is safe.
        let has_create = kinds.iter().any(|k| matches!(k, InstructionKind::Create));

        // 3a. One TradeExecuted per decoded TradeEvent (covers nested-CPI buys/sells).
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
            // Move the labels into the final leg (single-leg = zero clones), but
            // only when no Create follows — the Create path reads `labels_json`
            // afterwards to stamp the token's labels, and a `take` would null it.
            trade.instruction_labels = if leg_index == last_leg && !has_create {
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

        // Resolve the pump-instruction walk at most once, only for the paths that
        // need it (balance fallback, Create, Migrate) — never the pure-trade tx.
        // `has_create` was computed before the trade loop (it gates the label take).
        let has_migrate = kinds.iter().any(|k| matches!(k, InstructionKind::Migrate));
        let needs_pump_ixs = (decoded_events.is_empty()
            && kinds
                .iter()
                .any(|k| matches!(k, InstructionKind::Buy | InstructionKind::Sell)))
            || has_create
            || has_migrate;
        let pump_ixs = if needs_pump_ixs {
            find_pump_pb_ixs(message, meta, &keys, &self.pump_program_id_bytes)
        } else {
            Vec::new()
        };

        // 3b. Balance-delta fallback (rare: no decodable TradeEvent). Resolving the
        // trader from per-account balance deltas needs the full key view, so this
        // rare path is the one place every key gets encoded.
        if decoded_events.is_empty() {
            let all_keys = keys.all();
            for kind in kinds
                .iter()
                .filter(|k| matches!(k, InstructionKind::Buy | InstructionKind::Sell))
            {
                if let Some(pump_ix) = pump_ixs.first() {
                    let ix_accounts = resolve_pump_accounts_pb(pump_ix, &keys);
                    if let Some(ev) = self.decode_trade_from_balances_pb(
                        *kind,
                        &signature,
                        slot,
                        block_time,
                        &ix_accounts,
                        &all_keys,
                        pre_balances,
                        post_balances,
                        &meta.pre_token_balances,
                        &meta.post_token_balances,
                        &labels_json,
                        &raw_tx,
                    ) {
                        events.extend(ev);
                    }
                }
            }
        }

        // 3c. Create.
        if has_create {
            if let Some(create_ix) = pump_ixs.iter().find(|ix| is_create_pb(ix)) {
                let ix_accounts = resolve_pump_accounts_pb(create_ix, &keys);
                let all_keys = keys.all();
                let pump_ix_datas: Vec<&[u8]> = pump_ixs.iter().map(|ix| ix.data).collect();
                events.extend(self.decode_create(
                    &signature,
                    slot,
                    block_time,
                    create_ix.data,
                    &ix_accounts,
                    &all_keys,
                    &decoded_events,
                    &decoded_create_events,
                    &instruction_type,
                    &labels_json,
                    cu_limit,
                    cu_price,
                    &raw_tx,
                    &pump_ix_datas,
                ));
            }
        }

        // Migrate.
        if has_migrate {
            if let Some(migrate_ix) = pump_ixs.iter().find(|ix| is_migrate_pb(ix)) {
                let ix_accounts = resolve_pump_accounts_pb(migrate_ix, &keys);
                if let Some(ev) =
                    self.decode_migrate(&signature, slot, block_time, &ix_accounts, &raw_tx)
                {
                    events.push(ev);
                }
            }
        }

        // TokenCreated must precede TradeExecuted for the same mint so the
        // pipeline's `token_cache` gate doesn't drop the dev buy on create+buy txs.
        events.sort_by_key(|e| match e {
            InternalEvent::TokenCreated(_) => 0,
            InternalEvent::CreatorActivityDetected(_) => 1,
            _ => 2,
        });

        DecodeOutput::Transaction { raw_tx, events }
    }

    /// AMM-only decode for an explicit-pool backfill (token_sync's AMM loop):
    /// decode PumpSwap swaps for a tracked pool **regardless** of whether the tx
    /// also touches the bonding-curve program. The protobuf analogue of the
    /// removed `decode_pump_swap_result` — unlike [`Self::decode_protobuf`], there
    /// is no curve-priority gate, so an aggregator tx that trades another token on
    /// the curve *and* swaps our pool still yields our AMM trade. Requires a seeded
    /// `pool_index`; returns `Ignored` without one or when the tx has no swap for a
    /// tracked pool.
    pub fn decode_amm_protobuf(
        &self,
        update: &SubscribeUpdateTransaction,
        received_at: DateTime<Utc>,
    ) -> DecodeOutput {
        let Some(info) = update.transaction.as_ref() else {
            return DecodeOutput::Ignored;
        };
        let Some(tx) = info.transaction.as_ref() else {
            return DecodeOutput::Ignored;
        };
        let Some(message) = tx.message.as_ref() else {
            return DecodeOutput::Ignored;
        };
        let Some(meta) = info.meta.as_ref() else {
            return DecodeOutput::Ignored;
        };
        self.decode_amm_live_pb(info, message, meta, update.slot, received_at, received_at)
    }

    /// Decode a PumpSwap (AMM) tx: resolve each swap's
    /// pool to a tracked mint via the shared index, dropping swaps for pools we
    /// don't track. Reached only when the tx touches the AMM program but not the
    /// curve program.
    fn decode_amm_live_pb(
        &self,
        info: &SubscribeUpdateTransactionInfo,
        message: &scb::Message,
        meta: &scb::TransactionStatusMeta,
        slot: u64,
        block_time: DateTime<Utc>,
        received_at: DateTime<Utc>,
    ) -> DecodeOutput {
        let Some(index) = self.pool_index.as_ref() else {
            return DecodeOutput::Ignored;
        };

        // Resolve pool→mint first; bail before any base58/label work if none of
        // the swap pools are tracked.
        let logs: Vec<&str> = meta.log_messages.iter().map(String::as_str).collect();
        let resolved: Vec<(DecodedAmmTrade, String)> = decode_pump_swap_trades_from_logs(&logs)
            .into_iter()
            .filter(|ev| !Trade::is_dust(ev.quote_amount))
            .filter_map(|ev| index.get(&ev.pool).map(|m| (ev, m.value().clone())))
            .collect();
        if resolved.is_empty() {
            return DecodeOutput::Ignored;
        }

        let signature = bs58::encode(&info.signature).into_string();
        let keys = LazyKeys::new(message, meta);

        let raw_tx = Arc::new(raw_tx_carrier(signature.clone(), slot, block_time, received_at));

        let outer_ixs: Vec<PbIx> = message
            .instructions
            .iter()
            .map(|ix| pb_ix(ix.program_id_index, &ix.accounts, &ix.data))
            .collect();
        let (labels, _, _) = build_labels_pb(&outer_ixs, &keys);
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
                received_at,
            );
            events.push(InternalEvent::TradeExecuted(TradeExecutedEvent {
                trade,
                tx_signature: signature.clone(),
                slot,
                timestamp: received_at,
                raw_tx: raw_tx.clone(),
            }));
        }

        DecodeOutput::Transaction { raw_tx, events }
    }
}

/// Lightweight raw-tx carrier with `raw_data: Null` — the real Helius-shaped
/// blob is synthesised off-thread in the DbWriter from the protobuf.
fn raw_tx_carrier(
    signature: String,
    slot: u64,
    block_time: DateTime<Utc>,
    received_at: DateTime<Utc>,
) -> RawTransaction {
    RawTransaction {
        id: Uuid::new_v4(),
        signature,
        slot,
        block_time,
        raw_data: Value::Null,
        received_at,
    }
}

/// Borrow one compiled instruction's `accounts`/`data` bytes into a [`PbIx`],
/// keeping the program id as an index (resolved lazily via [`LazyKeys`]).
fn pb_ix<'a>(program_id_index: u32, accounts: &'a [u8], data: &'a [u8]) -> PbIx<'a> {
    PbIx {
        program_id_index,
        accounts,
        data,
    }
}

/// Raw 32-byte form of the canonical pump.fun program id, base58-decoded once.
fn pump_fun_id_bytes() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| bs58::decode(PUMP_FUN_PROGRAM_ID).into_vec().unwrap_or_default())
}

/// Pump.fun instructions (outer + inner), borrowed straight from the protobuf.
/// Protobuf-native analogue of `parse::find_pump_ixs_anywhere`. Matches the
/// program id on raw 32-byte bytes so no key is base58-encoded.
fn find_pump_pb_ixs<'a>(
    message: &'a scb::Message,
    meta: &'a scb::TransactionStatusMeta,
    keys: &LazyKeys,
    program_id_bytes: &[u8],
) -> Vec<PbIx<'a>> {
    let mut out = Vec::new();
    let is_pump = |idx: u32| keys.raw(idx as usize) == Some(program_id_bytes);

    for ix in &message.instructions {
        if is_pump(ix.program_id_index) {
            out.push(PbIx {
                program_id_index: ix.program_id_index,
                accounts: &ix.accounts,
                data: &ix.data,
            });
        }
    }

    for group in &meta.inner_instructions {
        for ix in &group.instructions {
            if is_pump(ix.program_id_index) {
                out.push(PbIx {
                    program_id_index: ix.program_id_index,
                    accounts: &ix.accounts,
                    data: &ix.data,
                });
            }
        }
    }

    out
}

/// Map pump.fun instructions (outer + inner) to [`InstructionKind`]. Mirrors
/// `instructions::collect_instruction_kinds`.
fn collect_kinds_pb(
    outer: &[PbIx],
    meta: &scb::TransactionStatusMeta,
    keys: &LazyKeys,
) -> Vec<InstructionKind> {
    let mut kinds = Vec::new();

    for p in outer {
        if let Some(kind) = classify_pump_ix(keys.get(p.program_id_index as usize), Some(p.data)) {
            kinds.push(kind);
        }
    }

    for group in &meta.inner_instructions {
        for ix in &group.instructions {
            // Raw-byte match against the canonical pump program; only on a hit do
            // we hand `classify_pump_ix` the known `&str` constant — no per-inner
            // -ix base58 encode of the program id.
            if keys.raw(ix.program_id_index as usize) != Some(pump_fun_id_bytes()) {
                continue;
            }
            if let Some(kind) = classify_pump_ix(PUMP_FUN_PROGRAM_ID, Some(&ix.data)) {
                kinds.push(kind);
            }
        }
    }

    kinds
}

/// Build the per-instruction labels and extract compute-budget values, reusing
/// the shared `label_instruction` (with no `parsed.type`, matching the live
/// adapter's pre-Tier-B output). Mirrors `instructions::build_instruction_labels`.
/// Resolves each outer ix's program id via [`LazyKeys`] (memoized — duplicate
/// program ids encode once).
fn build_labels_pb(outer: &[PbIx], keys: &LazyKeys) -> (Vec<String>, Option<u64>, Option<u64>) {
    let mut cu_limit: Option<u64> = None;
    let mut cu_price: Option<u64> = None;

    let labels = outer
        .iter()
        .map(|p| {
            let pid = keys.get(p.program_id_index as usize);
            extract_compute_budget(pid, Some(p.data), &mut cu_limit, &mut cu_price);
            label_instruction(pid, None, Some(p.data))
        })
        .collect();

    (labels, cu_limit, cu_price)
}

/// Resolve a pump instruction's account pubkeys from its index bytes. Protobuf
/// instructions always carry numeric indices (never resolved strings), so this
/// is a straight index into the account-key list. Mirrors
/// `parse::resolve_pump_accounts`.
fn resolve_pump_accounts_pb(ix: &PbIx, keys: &LazyKeys) -> Vec<String> {
    ix.accounts
        .iter()
        .filter_map(|&i| keys.raw(i as usize).map(|_| keys.get(i as usize).to_string()))
        .collect()
}

fn is_create_pb(ix: &PbIx) -> bool {
    ix.data.len() >= 8
        && (ix.data[..8] == CREATE_INSTRUCTION_DISCRIMINATOR
            || ix.data[..8] == CREATE_V2_INSTRUCTION_DISCRIMINATOR)
}

fn is_migrate_pb(ix: &PbIx) -> bool {
    ix.data.len() >= 8
        && (ix.data[..8] == MIGRATE_INSTRUCTION_DISCRIMINATOR
            || ix.data[..8] == MIGRATE_V2_INSTRUCTION_DISCRIMINATOR)
}

#[cfg(test)]
mod tests {
    //! `decode_protobuf` unit tests. Each builds one synthetic protobuf tx and
    //! asserts the decoded events (trades/migrations) match the expected
    //! attribution — covering the cases that historically differed between the
    //! protobuf and `Value` paths (log-emitted vs inner-CPI trade events, account
    //! ordering). Create decode is additionally covered by the shared,
    //! source-agnostic `decode_create`.

    use base64::{engine::general_purpose::STANDARD, Engine};
    use borsh::BorshSerialize;
    use chrono::Utc;

    use super::super::super::proto::geyser::{
        SubscribeUpdateTransaction, SubscribeUpdateTransactionInfo,
    };
    use super::super::super::proto::solana::storage::confirmed_block as scb;
    use super::super::{DecodeOutput, HeliusDecoder};
    use crate::config::constants::{
        ANCHOR_EVENT_CPI_DISCRIMINATOR, BUY_DISCRIMINATOR, COMPUTE_BUDGET_PROGRAM_ID,
        CREATE_EVENT_DISCRIMINATOR, CREATE_INSTRUCTION_DISCRIMINATOR,
        MIGRATE_INSTRUCTION_DISCRIMINATOR, PUMP_FUN_PROGRAM_ID, TOKEN_PROGRAM_ID,
        TRADE_EVENT_DISCRIMINATOR,
    };
    use crate::models::events::InternalEvent;

    fn id_bytes(s: &str) -> Vec<u8> {
        bs58::decode(s).into_vec().unwrap()
    }

    fn pk(b: u8) -> [u8; 32] {
        [b; 32]
    }

    /// Borsh layout of pump.fun's on-chain TradeEvent (serialize side).
    #[derive(BorshSerialize)]
    struct TestTradeEvent {
        mint: [u8; 32],
        sol_amount: u64,
        token_amount: u64,
        is_buy: bool,
        user: [u8; 32],
        timestamp: i64,
        virtual_sol_reserves: u64,
        virtual_token_reserves: u64,
        real_sol_reserves: u64,
        real_token_reserves: u64,
    }

    fn trade_event_bytes(mint: [u8; 32], user: [u8; 32], is_buy: bool) -> Vec<u8> {
        let ev = TestTradeEvent {
            mint,
            sol_amount: 1_500_000_000,    // 1.5 SOL
            token_amount: 42_000_000_000, // raw units
            is_buy,
            user,
            timestamp: 0,
            virtual_sol_reserves: 30_000_000_000,
            virtual_token_reserves: 1_000_000_000_000,
            real_sol_reserves: 5_000_000_000,
            real_token_reserves: 800_000_000_000,
        };
        let mut bytes = TRADE_EVENT_DISCRIMINATOR.to_vec();
        bytes.extend(borsh::to_vec(&ev).unwrap());
        bytes
    }

    /// "Program data: <base64>" TradeEvent log line (emit! path).
    fn trade_event_log(mint: [u8; 32], user: [u8; 32], is_buy: bool) -> String {
        format!("Program data: {}", STANDARD.encode(trade_event_bytes(mint, user, is_buy)))
    }

    /// Build a single-transaction update. `account_keys` are raw 32-byte keys;
    /// `instructions`/`inner` index into them.
    #[allow(clippy::too_many_arguments)]
    fn build_update(
        account_keys: Vec<Vec<u8>>,
        instructions: Vec<scb::CompiledInstruction>,
        inner: Vec<scb::InnerInstructions>,
        logs: Vec<String>,
    ) -> SubscribeUpdateTransaction {
        let message = scb::Message {
            account_keys,
            instructions,
            versioned: true,
            ..Default::default()
        };
        let meta = scb::TransactionStatusMeta {
            inner_instructions: inner,
            log_messages: logs,
            ..Default::default()
        };
        SubscribeUpdateTransaction {
            transaction: Some(SubscribeUpdateTransactionInfo {
                signature: vec![7u8; 64],
                transaction: Some(scb::Transaction {
                    signatures: vec![],
                    message: Some(message),
                }),
                meta: Some(meta),
                ..Default::default()
            }),
            slot: 100,
        }
    }

    /// Decode-order event fingerprints (covers trades + migrations), excluding
    /// timestamps/ids which legitimately differ between the two ingest clocks.
    fn fps(out: &DecodeOutput) -> Vec<String> {
        match out {
            DecodeOutput::Ignored => Vec::new(),
            DecodeOutput::Transaction { events, .. } => events
                .iter()
                .map(|ev| match ev {
                    InternalEvent::TradeExecuted(e) => {
                        let t = &e.trade;
                        format!(
                            "trade|{}|{}|{:?}|{:.9}|{:.3}|{:?}|{:?}|{}|{}|{}",
                            t.mint_address,
                            t.wallet_address,
                            t.trade_type,
                            t.sol_amount,
                            t.token_amount,
                            t.virtual_sol_reserves,
                            t.virtual_token_reserves,
                            t.leg_index,
                            t.venue,
                            t.instruction_labels,
                        )
                    }
                    InternalEvent::TokenMigrated(e) => format!("migrate|{}", e.mint_address),
                    InternalEvent::TokenCreated(e) => format!("create|{}", e.token.mint_address),
                    InternalEvent::CreatorActivityDetected(e) => {
                        format!("creator|{}|{}", e.creator_wallet, e.mint_address)
                    }
                    InternalEvent::LiquidityAdded(e) | InternalEvent::LiquidityRemoved(e) => {
                        format!("liq|{}", e.mint_address)
                    }
                })
                .collect(),
        }
    }

    fn decode_fps(update: &SubscribeUpdateTransaction, decoder: &HeliusDecoder) -> Vec<String> {
        fps(&decoder.decode_protobuf(update, Utc::now()))
    }

    /// A bonding-curve buy whose TradeEvent comes from a "Program data:" log.
    #[test]
    fn parity_bonding_curve_buy_from_logs() {
        let pump = id_bytes(PUMP_FUN_PROGRAM_ID);
        let cbudget = id_bytes(COMPUTE_BUDGET_PROGRAM_ID);
        // keys: 0=fee payer, 1=pump program, 2=compute budget program
        let account_keys = vec![pk(1).to_vec(), pump, cbudget];

        // outer: a compute-budget SetComputeUnitLimit + a pump Buy
        let cb_ix = scb::CompiledInstruction {
            program_id_index: 2,
            accounts: vec![],
            data: {
                let mut d = vec![2u8]; // SetComputeUnitLimit
                d.extend(200_000u32.to_le_bytes());
                d
            },
        };
        let buy_ix = scb::CompiledInstruction {
            program_id_index: 1,
            accounts: vec![0],
            data: {
                let mut d = BUY_DISCRIMINATOR.to_vec();
                d.extend([0u8; 16]); // args (unused by this path)
                d
            },
        };
        let logs = vec![
            format!("Program {PUMP_FUN_PROGRAM_ID} invoke [1]"),
            trade_event_log(pk(9), pk(8), true),
            format!("Program {PUMP_FUN_PROGRAM_ID} success"),
        ];

        let update = build_update(account_keys, vec![cb_ix, buy_ix], vec![], logs);
        let decoder = HeliusDecoder::new(PUMP_FUN_PROGRAM_ID.to_string());
        let fps = decode_fps(&update, &decoder);
        assert_eq!(fps.len(), 1, "expected exactly one trade");
        assert!(fps[0].contains("Pump.Fun: Buy"), "labels should carry the buy");
        assert!(
            fps[0].contains("Compute Budget: SetComputeUnitLimit"),
            "labels should carry the compute-budget ix"
        );
    }

    /// A sell whose TradeEvent the logs DROPPED (truncation) — recovered from the
    /// inner `emit_cpi!` instruction. This is the path where protobuf differs most
    /// from the `Value` path (raw bytes vs base58), so parity here is the key case.
    #[test]
    fn parity_sell_from_inner_cpi_when_logs_truncated() {
        let pump = id_bytes(PUMP_FUN_PROGRAM_ID);
        let account_keys = vec![pk(1).to_vec(), pump];

        // outer pump Sell ix (so kinds sees a sell); the event is in the inner ix.
        let sell_ix = scb::CompiledInstruction {
            program_id_index: 1,
            accounts: vec![0],
            data: {
                use crate::config::constants::SELL_DISCRIMINATOR;
                let mut d = SELL_DISCRIMINATOR.to_vec();
                d.extend([0u8; 16]);
                d
            },
        };
        // inner self-CPI: [anchor cpi tag][trade disc][borsh event]
        let mut inner_data = ANCHOR_EVENT_CPI_DISCRIMINATOR.to_vec();
        inner_data.extend(trade_event_bytes(pk(9), pk(8), false));
        let inner = vec![scb::InnerInstructions {
            index: 0,
            instructions: vec![scb::InnerInstruction {
                program_id_index: 1,
                accounts: vec![],
                data: inner_data,
                stack_height: None,
            }],
        }];
        // No "Program data:" log line — only the gate line.
        let logs = vec![format!("Program {PUMP_FUN_PROGRAM_ID} invoke [1]")];

        let update = build_update(account_keys, vec![sell_ix], inner, logs);
        let decoder = HeliusDecoder::new(PUMP_FUN_PROGRAM_ID.to_string());
        let fps = decode_fps(&update, &decoder);
        assert_eq!(fps.len(), 1, "expected exactly one trade from the inner CPI");
        assert!(fps[0].starts_with("trade|"));
    }

    /// Borsh layout of pump.fun's on-chain CreateEvent (serialize side), mirroring
    /// `create::RawCreateEvent`.
    #[derive(BorshSerialize)]
    struct TestCreateEvent {
        name: String,
        symbol: String,
        uri: String,
        mint: [u8; 32],
        bonding_curve: [u8; 32],
        user: [u8; 32],
        creator: [u8; 32],
        timestamp: i64,
        virtual_token_reserves: u64,
        virtual_sol_reserves: u64,
        real_token_reserves: u64,
        token_total_supply: u64,
        token_program: [u8; 32],
        is_mayhem_mode: bool,
        is_cashback_enabled: bool,
        quote_mint: [u8; 32],
        virtual_quote_reserves: u64,
    }

    fn arr32(s: &str) -> [u8; 32] {
        id_bytes(s).try_into().unwrap()
    }

    /// "Program data: <base64>" CreateEvent log line (emit! path).
    fn create_event_log(mint: [u8; 32], user: [u8; 32], creator: [u8; 32]) -> String {
        let ev = TestCreateEvent {
            name: "Test".into(),
            symbol: "TST".into(),
            uri: "ipfs://x".into(),
            mint,
            bonding_curve: pk(3),
            user,
            creator,
            timestamp: 0,
            virtual_token_reserves: 1_000_000_000_000,
            virtual_sol_reserves: 30_000_000_000,
            real_token_reserves: 800_000_000_000,
            token_total_supply: 1_000_000_000_000,
            token_program: arr32(TOKEN_PROGRAM_ID),
            is_mayhem_mode: false,
            is_cashback_enabled: false,
            quote_mint: pk(0),
            virtual_quote_reserves: 0,
        };
        let mut bytes = CREATE_EVENT_DISCRIMINATOR.to_vec();
        bytes.extend(borsh::to_vec(&ev).unwrap());
        format!("Program data: {}", STANDARD.encode(bytes))
    }

    /// Regression: a Create tx bundled with the dev's initial buy must still stamp
    /// the **token's** `instruction_labels`. The trade-leg loop moves labels into
    /// the final leg via `mem::take`; if it does so when a Create follows, the
    /// Create path reads a nulled `labels_json` and the token loses its labels
    /// (the live + Fetch-New bug this guards). Both the trade and the token must
    /// carry the labels here.
    #[test]
    fn create_with_dev_buy_preserves_token_labels() {
        let pump = id_bytes(PUMP_FUN_PROGRAM_ID);
        let mint = pk(9);
        let user = pk(8);
        // keys: 0=fee payer/creator/user, 1=pump program, 2=mint, 3=bonding curve
        let account_keys = vec![user.to_vec(), pump, mint.to_vec(), pk(3).to_vec()];

        // outer Create ix: accounts[0] -> keys[2] = mint (decode_create reads it).
        let create_ix = scb::CompiledInstruction {
            program_id_index: 1,
            accounts: vec![2, 3, 0],
            data: CREATE_INSTRUCTION_DISCRIMINATOR.to_vec(),
        };
        // outer Buy ix (the dev buy), so `kinds` sees both Create and Buy.
        let buy_ix = scb::CompiledInstruction {
            program_id_index: 1,
            accounts: vec![0],
            data: {
                let mut d = BUY_DISCRIMINATOR.to_vec();
                d.extend([0u8; 16]);
                d
            },
        };
        let logs = vec![
            format!("Program {PUMP_FUN_PROGRAM_ID} invoke [1]"),
            create_event_log(mint, user, user),
            trade_event_log(mint, user, true),
            format!("Program {PUMP_FUN_PROGRAM_ID} success"),
        ];

        let update = build_update(account_keys, vec![create_ix, buy_ix], vec![], logs);
        let decoder = HeliusDecoder::new(PUMP_FUN_PROGRAM_ID.to_string());
        let out = decoder.decode_protobuf(&update, Utc::now());

        let DecodeOutput::Transaction { events, .. } = out else {
            panic!("expected a decoded transaction");
        };

        let token = events
            .iter()
            .find_map(|e| match e {
                InternalEvent::TokenCreated(e) => Some(&e.token),
                _ => None,
            })
            .expect("expected a TokenCreated event");
        assert!(
            token.instruction_labels.is_array(),
            "token labels must survive the dev-buy leg's mem::take, got {:?}",
            token.instruction_labels
        );
        let labels = token.instruction_labels.to_string();
        assert!(labels.contains("Pump.Fun: Create"), "labels: {labels}");
        assert!(labels.contains("Pump.Fun: Buy"), "labels: {labels}");

        // The trade leg must also still carry the labels (it clones, not takes).
        let trade = events
            .iter()
            .find_map(|e| match e {
                InternalEvent::TradeExecuted(e) => Some(&e.trade),
                _ => None,
            })
            .expect("expected a TradeExecuted event");
        assert!(
            trade.instruction_labels.is_array(),
            "trade labels: {:?}",
            trade.instruction_labels
        );

        // Ordering: TokenCreated must precede the dev-buy TradeExecuted, else
        // pipeline::on_trade_executed drops the buy (mint not yet in token_cache)
        // and the genesis candle is lost from the chart.
        let created_idx = events
            .iter()
            .position(|e| matches!(e, InternalEvent::TokenCreated(_)))
            .expect("expected a TokenCreated event");
        let trade_idx = events
            .iter()
            .position(|e| matches!(e, InternalEvent::TradeExecuted(_)))
            .expect("expected a TradeExecuted event");
        assert!(
            created_idx < trade_idx,
            "TokenCreated (idx {created_idx}) must come before the dev buy (idx {trade_idx})"
        );
    }

    /// A migrate: `decode_migrate` reads the mint from the pump ix accounts[2].
    #[test]
    fn parity_migrate() {
        let pump = id_bytes(PUMP_FUN_PROGRAM_ID);
        let mint = pk(5);
        // keys: 0=signer, 1=pump program, 2=bonding curve, 3=mint
        let account_keys = vec![pk(1).to_vec(), pump, pk(2).to_vec(), mint.to_vec()];

        let migrate_ix = scb::CompiledInstruction {
            program_id_index: 1,
            accounts: vec![0, 2, 3], // accounts[2] -> keys[3] = mint
            data: MIGRATE_INSTRUCTION_DISCRIMINATOR.to_vec(),
        };
        let logs = vec![format!("Program {PUMP_FUN_PROGRAM_ID} invoke [1]")];

        let update = build_update(account_keys, vec![migrate_ix], vec![], logs);
        let decoder = HeliusDecoder::new(PUMP_FUN_PROGRAM_ID.to_string());
        let fps = decode_fps(&update, &decoder);
        assert_eq!(
            fps,
            vec![format!("migrate|{}", bs58::encode(mint).into_string())]
        );
    }

    /// `LazyKeys` must preserve the canonical `static ++ loaded writable ++ loaded
    /// readonly` order so an account that lives in an address-lookup-table region
    /// still resolves to the right pubkey. Here the migrate mint sits in
    /// `loaded_readonly_addresses`, past the static keys — a regression guard for
    /// the lazy-encode refactor (a wrong concat order would misattribute the mint,
    /// the bug the eager-encode comments warned about).
    #[test]
    fn lazy_keys_resolve_loaded_readonly_address() {
        let pump = id_bytes(PUMP_FUN_PROGRAM_ID);
        let mint = pk(5);
        // static keys: 0=signer, 1=pump program, 2=bonding curve. The mint is
        // loaded from an address table → combined index 3.
        let message = scb::Message {
            account_keys: vec![pk(1).to_vec(), pump, pk(2).to_vec()],
            instructions: vec![scb::CompiledInstruction {
                program_id_index: 1,
                accounts: vec![0, 2, 3], // accounts[2] -> combined keys[3] = mint
                data: MIGRATE_INSTRUCTION_DISCRIMINATOR.to_vec(),
            }],
            versioned: true,
            ..Default::default()
        };
        let meta = scb::TransactionStatusMeta {
            loaded_readonly_addresses: vec![mint.to_vec()],
            log_messages: vec![format!("Program {PUMP_FUN_PROGRAM_ID} invoke [1]")],
            ..Default::default()
        };
        let update = SubscribeUpdateTransaction {
            transaction: Some(SubscribeUpdateTransactionInfo {
                signature: vec![7u8; 64],
                transaction: Some(scb::Transaction {
                    signatures: vec![],
                    message: Some(message),
                }),
                meta: Some(meta),
                ..Default::default()
            }),
            slot: 100,
        };
        let decoder = HeliusDecoder::new(PUMP_FUN_PROGRAM_ID.to_string());
        let fps = decode_fps(&update, &decoder);
        assert_eq!(
            fps,
            vec![format!("migrate|{}", bs58::encode(mint).into_string())]
        );
    }
}
