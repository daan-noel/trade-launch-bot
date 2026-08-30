//! Protobuf-native decode of `SubscribeUpdateTransaction` updates.
//!
//! Source-agnostic on purpose, and named for the format rather than the wire
//! that first carried it: a JSON feed converts to this same protobuf in
//! `ingest-core::convert` before it gets here, so gRPC frames, relay frames and
//! RPC backfill results all decode through this one file.

use std::cell::OnceCell;

use borsh::BorshDeserialize;
use chrono::{DateTime, Utc};
use tracing::warn;

use crate::event::{fee_lamports_opt, IngestEvent, Reserves, Side, TokenMigrated, Trade, Venue};
use crate::pool::register_pool;
use crate::protocol::Protocol;

use super::create::decode_create_events_from_logs;
use super::instructions::{
    classify_pump_ix, determine_instruction_type, label_instruction, system_transfer_lamports,
    FeeBudget,
    InstructionKind,
};
use super::trade::{
    build_amm_trade, compute_sol_change, compute_sol_change_lamports,
    decode_pump_swap_trades_from_inner, decode_pump_swap_trades_from_logs,
    decode_trade_events_from_logs, DecodedAmmTrade, DecodedTradeEvent, RawTradeEvent,
};
use super::{DecodeOutput, Decoder, TxRelevance};

use crate::proto::geyser::{SubscribeUpdateTransaction, SubscribeUpdateTransactionInfo};
use crate::proto::solana::storage::confirmed_block as scb;

// ── Internal protobuf instruction helpers ─────────────────────────────────────

/// One pump.fun instruction in borrowed protobuf form.
struct PbIx<'a> {
    accounts: &'a [u8],
    data: &'a [u8],
}

/// Lazy memoizing base58 encoder for the tx's account-key list.
struct LazyKeys<'a> {
    raw: Vec<&'a [u8]>,
    encoded: Vec<OnceCell<String>>,
}

/// Resolve account index `i` against the tx's key space **without allocating** —
/// the same `account_keys ++ loaded_writable ++ loaded_readonly` ordering
/// [`LazyKeys`] materialises, which is the order `program_id_index` indexes into.
///
/// Duplicated (not shared) on purpose: [`LazyKeys`] must collect, because it
/// carries a parallel `OnceCell` per key for the base58 memoisation, and the
/// pre-filter runs on **every** delivered tx on the feed supervisor task, where
/// a per-tx `Vec` of 40+ fat pointers is exactly the per-event alloc the hot-path
/// rule forbids. `key_at_matches_lazykeys_ordering` guards the two against drift.
fn key_at<'a>(
    message: &'a scb::Message,
    meta: &'a scb::TransactionStatusMeta,
    i: usize,
) -> Option<&'a [u8]> {
    let n = message.account_keys.len();
    if i < n {
        return Some(message.account_keys[i].as_slice());
    }
    let i = i - n;
    let n = meta.loaded_writable_addresses.len();
    if i < n {
        return Some(meta.loaded_writable_addresses[i].as_slice());
    }
    meta.loaded_readonly_addresses
        .get(i - n)
        .map(|k| k.as_slice())
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

    fn raw(&self, i: usize) -> Option<&'a [u8]> {
        self.raw.get(i).copied()
    }

    fn get(&self, i: usize) -> &str {
        match self.raw.get(i) {
            Some(_) => self.encoded[i]
                .get_or_init(|| bs58::encode(self.raw[i]).into_string())
                .as_str(),
            None => "",
        }
    }

    fn all(&self) -> Vec<&str> {
        (0..self.raw.len()).map(|i| self.get(i)).collect()
    }
}

// ── Decoder entry points ──────────────────────────────────────────────────────

impl Decoder {
    /// Self-classifying entry — scans logs then dispatches. Used by backfill.
    pub fn decode_protobuf(
        &self,
        update: &SubscribeUpdateTransaction,
        received_at: DateTime<Utc>,
    ) -> DecodeOutput {
        let meta = match update.transaction.as_ref().and_then(|i| i.meta.as_ref()) {
            Some(m) => m,
            None => return DecodeOutput::Ignored,
        };
        match self.classify_logs(&meta.log_messages) {
            Some(rel) => self.decode_relevant_pb(update, rel, received_at),
            None => DecodeOutput::Ignored,
        }
    }

    /// Hot-path entry — caller pre-classified the tx, no log re-scan.
    pub fn decode_relevant_pb(
        &self,
        update: &SubscribeUpdateTransaction,
        relevance: TxRelevance,
        received_at: DateTime<Utc>,
    ) -> DecodeOutput {
        let info = match update.transaction.as_ref() { Some(i) => i, None => return DecodeOutput::Ignored };
        let tx = match info.transaction.as_ref() { Some(t) => t, None => return DecodeOutput::Ignored };
        let message = match tx.message.as_ref() { Some(m) => m, None => return DecodeOutput::Ignored };
        let meta = match info.meta.as_ref() { Some(m) => m, None => return DecodeOutput::Ignored };

        // `Create` is `Curve` plus a routing hint — one decode path, so the tag
        // can never change what gets decoded.
        if relevance.is_curve() {
            self.decode_curve_pb(info, message, meta, update.slot, received_at)
        } else {
            self.decode_amm_live_pb(info, message, meta, update.slot, received_at)
        }
    }

    /// **Hot-path pre-filter.** Classify a delivered tx off its *message* —
    /// 32-byte program-key compares — and decide in the same pass whether it is a
    /// token `create`.
    ///
    /// Replaces a substring scan of every log line of every transaction for a
    /// 44-char base58 program id ([`Decoder::classify_logs`]). That scan
    /// re-derived what the subscription had already proven: `account_include` is
    /// set to the pump program + tracked pool PDAs, so a delivered tx names one
    /// of them by construction. The scan ran on the single feed supervisor task that
    /// gates every create's arrival, ahead of everything else.
    ///
    /// Two behaviours to know:
    /// - **Curve wins over Amm**, same precedence as the log scan: a tx that
    ///   names both programs is `Curve`/`Create` (a pool PDA can deliver an AMM
    ///   tx, so the distinction still has to be made).
    /// - It is **strictly more complete** than the log scan: a validator that
    ///   truncates or drops a tx's logs hides the program id from
    ///   `classify_logs` but not from the account keys. Such a tx is now
    ///   forwarded to decode, where it resolves to `Ignored` if there is nothing
    ///   in it — no extra events, no extra `raw_txs` rows.
    pub(crate) fn classify_accounts(
        &self,
        info: &SubscribeUpdateTransactionInfo,
    ) -> Option<TxRelevance> {
        let message = info.transaction.as_ref()?.message.as_ref()?;
        let meta = info.meta.as_ref()?;

        let pump = self.protocol.programs.pump_fun.bytes.as_slice();
        let swap = self.protocol.programs.pump_swap.bytes.as_slice();

        let mut has_swap = false;
        for key in message
            .account_keys
            .iter()
            .chain(meta.loaded_writable_addresses.iter())
            .chain(meta.loaded_readonly_addresses.iter())
        {
            let key = key.as_slice();
            if key == pump {
                return Some(if self.has_create_ix(message, meta) {
                    TxRelevance::Create
                } else {
                    TxRelevance::Curve
                });
            }
            has_swap |= key == swap;
        }
        has_swap.then_some(TxRelevance::Amm)
    }

    /// Does this tx carry a pump.fun `create` / `create_v2` instruction?
    ///
    /// Scans top-level instructions *and* inner CPIs — a bundled/router launch
    /// invokes `create` as an inner instruction, and those are precisely the
    /// creates worth routing first. The 8-byte discriminator is tested before the
    /// program-key lookup so the overwhelmingly common case (a buy or sell) exits
    /// on a cheap byte compare. The discriminator rule itself is
    /// [`is_create_disc`] — the same predicate the decode path uses, not a copy.
    fn has_create_ix(&self, message: &scb::Message, meta: &scb::TransactionStatusMeta) -> bool {
        let is_create = |data: &[u8]| is_create_disc(data, &self.protocol);
        let pump = self.protocol.programs.pump_fun.bytes.as_slice();
        let is_pump = |idx: u32| key_at(message, meta, idx as usize) == Some(pump);

        message
            .instructions
            .iter()
            .any(|ix| is_create(&ix.data) && is_pump(ix.program_id_index))
            || meta.inner_instructions.iter().any(|group| {
                group
                    .instructions
                    .iter()
                    .any(|ix| is_create(&ix.data) && is_pump(ix.program_id_index))
            })
    }

    /// AMM-only decode for explicit-pool backfill (token_sync AMM loop).
    pub fn decode_amm_protobuf(
        &self,
        update: &SubscribeUpdateTransaction,
        received_at: DateTime<Utc>,
    ) -> DecodeOutput {
        let info = match update.transaction.as_ref() { Some(i) => i, None => return DecodeOutput::Ignored };
        let tx = match info.transaction.as_ref() { Some(t) => t, None => return DecodeOutput::Ignored };
        let message = match tx.message.as_ref() { Some(m) => m, None => return DecodeOutput::Ignored };
        let meta = match info.meta.as_ref() { Some(m) => m, None => return DecodeOutput::Ignored };
        self.decode_amm_live_pb(info, message, meta, update.slot, received_at)
    }

    // ── Curve decode ──────────────────────────────────────────────────────────

    fn decode_curve_pb(
        &self,
        info: &SubscribeUpdateTransactionInfo,
        message: &scb::Message,
        meta: &scb::TransactionStatusMeta,
        slot: u64,
        received_at: DateTime<Utc>,
    ) -> DecodeOutput {
        let p = &self.protocol;
        let signature = bs58::encode(&info.signature).into_string();
        let keys = LazyKeys::new(message, meta);
        let logs: Vec<&str> = meta.log_messages.iter().map(String::as_str).collect();

        // Step 1a: TradeEvents. The primary source is the "Program data:" log lines,
        // but the validator TRUNCATES a transaction's logs past a byte limit — a
        // multi-buy bundle (several pump buys in one tx) can exceed it and lose its
        // trailing TradeEvent log lines (observed: 4-buy launch bundles emit only 3
        // "Program data:" lines + a "Log truncated" marker, so the 4th buy vanished).
        // The anchor self-CPI event *inner instructions* carry the COMPLETE set (inner
        // instructions are NOT subject to the log byte limit), so fall back to them
        // when the log scan is empty OR the logs were truncated, taking whichever
        // source yields more events. (A plain `is_empty()` fallback missed PARTIAL
        // truncation — 3 of 4 events is non-empty, so the recovery never ran.)
        let mut decoded_events = decode_trade_events_from_logs(&logs, &p.discriminators.trade_event, p.lamports_per_sol);
        let logs_truncated = logs.iter().any(|l| l.contains("Log truncated"));
        if should_consult_inner_events(decoded_events.len(), logs_truncated) {
            let pump_ixs = find_program_pb_ixs(message, meta, &keys, &p.programs.pump_fun.bytes);
            let inner_events = decode_trade_events_from_inner_pb(&pump_ixs, p);
            if inner_events.len() > decoded_events.len() {
                decoded_events = inner_events;
            }
        }

        let decoded_create_events = decode_create_events_from_logs(&logs, &p.discriminators.create_event);

        // Step 1b: instruction kinds.
        let kinds = collect_kinds_pb(message, meta, &keys, p);
        let instruction_type = determine_instruction_type(&kinds, &decoded_events);

        // Step 2: labels + compute budget.
        let (instruction_labels, fee_budget) = build_labels_pb(message, meta, &keys, p);

        // Step 3: build IngestEvents.
        let mut events: Vec<IngestEvent> = Vec::new();
        let has_create = kinds.iter().any(|k| matches!(k, InstructionKind::Create));

        // 3a: one Trade per decoded TradeEvent.
        for (leg_index, ev) in decoded_events.iter().enumerate() {
            if ev.sol_amount < p.min_trade_sol {
                continue;
            }
            let side = if ev.is_buy { Side::Buy } else { Side::Sell };
            let price = if ev.token_amount > 0 { ev.sol_amount / ev.token_amount as f64 } else { 0.0 };
            events.push(IngestEvent::Trade(Trade {
                mint: ev.mint.clone(),
                wallet: ev.user.clone(),
                side,
                sol: ev.sol_amount,
                sol_lamports: ev.sol_amount_lamports,
                tokens: ev.token_amount,
                price,
                fee_lamports: fee_budget.fee_lamports,
                cu_limit: fee_budget.cu_limit,
                cu_price: fee_budget.cu_price,
                tip_lamports: fee_budget.tip_lamports,
                signature: signature.clone(),
                tx_index: info.index as u32,
                leg_index: leg_index as u32,
                slot,
                block_time: received_at,
                received_at,
                reserves: Reserves {
                    virtual_sol: Some(ev.virtual_sol_reserves),
                    virtual_token: Some(ev.virtual_token_reserves),
                    real_sol: Some(ev.real_sol_reserves),
                    real_token: Some(ev.real_token_reserves),
                    virtual_sol_lamports: Some(ev.virtual_sol_reserves_lamports),
                    real_sol_lamports: Some(ev.real_sol_reserves_lamports),
                },
                venue: Venue::Curve,
                instruction_type: if ev.is_buy { "Buy".to_string() } else { "Sell".to_string() },
                instruction_labels: instruction_labels.clone(),
                amm_swap_accounts: None,
            }));
        }

        // 3b: balance-delta fallback (no TradeEvent log, but has Buy/Sell instruction).
        if decoded_events.is_empty() {
            let pump_ixs = find_program_pb_ixs(message, meta, &keys, &p.programs.pump_fun.bytes);
            let all_keys = keys.all();
            for kind in &kinds {
                if !matches!(kind, InstructionKind::Buy | InstructionKind::Sell) {
                    continue;
                }
                if let Some(pump_ix) = pump_ixs.first() {
                    let ix_accounts = resolve_pump_accounts_pb(pump_ix, &keys);
                    if let Some(ev) = self.decode_trade_from_balances_pb(
                        *kind, &signature, info.index as u32, slot, received_at,
                        &ix_accounts, &all_keys,
                        &meta.pre_balances, &meta.post_balances,
                        &meta.pre_token_balances, &meta.post_token_balances,
                        instruction_labels.clone(),
                        fee_budget,
                    ) {
                        events.push(ev);
                    }
                    break;
                }
            }
        }

        // 3c: Create.
        if has_create {
            let pump_ixs = find_program_pb_ixs(message, meta, &keys, &p.programs.pump_fun.bytes);
            if let Some(create_ix) = pump_ixs.iter().find(|ix| is_create_pb(ix, p)) {
                let ix_accounts = resolve_pump_accounts_pb(create_ix, &keys);
                let all_keys = keys.all();
                let pump_ix_datas: Vec<&[u8]> = pump_ixs.iter().map(|ix| ix.data).collect();
                events.extend(self.decode_create(
                    &signature, slot, received_at, received_at,
                    create_ix.data, &ix_accounts, &all_keys,
                    &decoded_events, &decoded_create_events,
                    &instruction_type, instruction_labels.clone(),
                    fee_budget.cu_limit, fee_budget.cu_price, &pump_ix_datas,
                ));
            }
        }

        // 3d: Migrate.
        if kinds.iter().any(|k| matches!(k, InstructionKind::Migrate)) {
            let pump_ixs = find_program_pb_ixs(message, meta, &keys, &p.programs.pump_fun.bytes);
            if let Some(migrate_ix) = pump_ixs.iter().find(|ix| is_migrate_pb(ix, p)) {
                let ix_accounts = resolve_pump_accounts_pb(migrate_ix, &keys);
                if let Some(ev) = self.emit_migrate(&signature, slot, received_at, &ix_accounts) {
                    events.push(ev);
                }
            }
        }

        if events.is_empty() {
            return DecodeOutput::Ignored;
        }

        // Sort: TokenCreated → CreatorActivity → Trade/Migrate/Liquidity.
        events.sort_by_key(|e| match e {
            IngestEvent::TokenCreated(_) => 0,
            IngestEvent::CreatorActivity(_) => 1,
            _ => 2,
        });

        DecodeOutput::Events(events)
    }

    // ── AMM decode ────────────────────────────────────────────────────────────

    fn decode_amm_live_pb(
        &self,
        info: &SubscribeUpdateTransactionInfo,
        message: &scb::Message,
        meta: &scb::TransactionStatusMeta,
        slot: u64,
        received_at: DateTime<Utc>,
    ) -> DecodeOutput {
        let p = &self.protocol;
        let index = match self.pool_index.as_ref() {
            Some(i) => i,
            None => return DecodeOutput::Ignored,
        };

        let keys = LazyKeys::new(message, meta);
        let logs: Vec<&str> = meta.log_messages.iter().map(String::as_str).collect();

        // Step 1a: PumpSwap Buy/Sell events from the "Program data:" log lines — the
        // cheap primary source. But the validator truncates a tx's logs past a byte
        // limit, so a multi-swap AMM bundle can lose its trailing swap log lines (the
        // exact curve-path regression, see `decode_curve_pb`). The anchor self-CPI
        // event *inner instructions* on the pump_swap program carry the COMPLETE set
        // (inner ixs are NOT subject to the log limit), so consult them when the log
        // scan is empty OR the logs were truncated, taking whichever yields more.
        let mut amm_trades = decode_pump_swap_trades_from_logs(&logs, p);
        let logs_truncated = logs.iter().any(|l| l.contains("Log truncated"));
        if should_consult_inner_events(amm_trades.len(), logs_truncated) {
            let swap_ixs =
                find_program_pb_ixs(message, meta, &keys, &p.programs.pump_swap.bytes);
            let datas: Vec<&[u8]> = swap_ixs.iter().map(|ix| ix.data).collect();
            let inner = decode_pump_swap_trades_from_inner(&datas, p);
            if inner.len() > amm_trades.len() {
                amm_trades = inner;
            }
        }

        let resolved: Vec<(DecodedAmmTrade, String)> = amm_trades
            .into_iter()
            .filter(|ev| ev.quote_amount >= p.min_trade_sol)
            .filter_map(|ev| index.get(&ev.pool).map(|m| (ev, m.value().clone())))
            .collect();

        if resolved.is_empty() {
            return DecodeOutput::Ignored;
        }

        let signature = bs58::encode(&info.signature).into_string();
        let (labels, fee_budget) = build_labels_pb(message, meta, &keys, p);
        // Charged once for the whole tx; stamped on every leg it produced.

        // Passive account-list harvest for the executor's zero-RPC pool warmup:
        // resolve the full (ALT-included) account list of each TOP-LEVEL
        // pump_swap `buy`/`sell` whose pool is one of the trades we're emitting.
        // Inner-CPI-routed swaps are skipped (their account order belongs to the
        // router; the next direct swap of the coin provides). One list per pool
        // per tx, materialized only for tracked pools — the strings are the only
        // per-event alloc this adds.
        let mut harvested: Vec<(String, Vec<String>)> = Vec::new();
        {
            let ps_bytes: &[u8] = p.programs.pump_swap.bytes.as_ref();
            let buy = &p.discriminators.buy;
            let sell = &p.discriminators.sell;
            for ix in &message.instructions {
                if keys.raw(ix.program_id_index as usize) != Some(ps_bytes) || ix.data.len() < 8 {
                    continue;
                }
                if &ix.data[..8] != buy && &ix.data[..8] != sell {
                    continue;
                }
                let Some(&pool_idx) = ix.accounts.first() else { continue };
                let pool = keys.get(pool_idx as usize);
                if pool.is_empty()
                    || harvested.iter().any(|(seen, _)| seen == pool)
                    || !resolved.iter().any(|(ev, _)| ev.pool == pool)
                {
                    continue;
                }
                let list: Vec<String> = ix
                    .accounts
                    .iter()
                    .map(|&i| keys.get(i as usize).to_string())
                    .collect();
                harvested.push((pool.to_string(), list));
            }
        }

        let mut events = Vec::with_capacity(resolved.len());
        for (i, (ev, mint)) in resolved.iter().enumerate() {
            // Attach the harvested list to the FIRST emitted trade of its pool.
            let accounts = harvested
                .iter_mut()
                .find(|(pool, list)| pool == &ev.pool && !list.is_empty())
                .map(|(_, list)| Box::new(std::mem::take(list)));
            events.push(IngestEvent::Trade(build_amm_trade(
                ev, mint, &signature, slot, received_at, received_at,
                labels.clone(), info.index as u32, i as u32, accounts, fee_budget,
            )));
        }

        DecodeOutput::Events(events)
    }

    // ── Migrate event ─────────────────────────────────────────────────────────

    fn emit_migrate(
        &self,
        signature: &str,
        slot: u64,
        received_at: DateTime<Utc>,
        pump_accounts: &[String],
    ) -> Option<IngestEvent> {
        let mint = pump_accounts.get(2).filter(|s| !s.is_empty())?.to_string();

        // Auto-register the pool in the shared index so future AMM swaps resolve.
        if let Some(index) = &self.pool_index {
            if register_pool(index, &mint, &self.protocol) {
                // Newly registered — signal the feed supervisor to resubscribe.
                if let Some(notify) = &self.pools_changed {
                    notify.notify_one();
                }
            }
        }

        Some(IngestEvent::TokenMigrated(TokenMigrated {
            mint,
            signature: signature.to_string(),
            slot,
            block_time: received_at,
            received_at,
        }))
    }

    // ── Balance-delta fallback ────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn decode_trade_from_balances_pb(
        &self,
        kind: InstructionKind,
        signature: &str,
        tx_index: u32,
        slot: u64,
        received_at: DateTime<Utc>,
        pump_accounts: &[String],
        account_keys: &[&str],
        pre_balances: &[u64],
        post_balances: &[u64],
        pre_token_balances: &[scb::TokenBalance],
        post_token_balances: &[scb::TokenBalance],
        instruction_labels: Vec<String>,
        fee_budget: FeeBudget,
    ) -> Option<IngestEvent> {
        let p = &self.protocol;
        let mint = pump_accounts.get(2).filter(|s| !s.is_empty())?.to_string();
        let user = pump_accounts.get(6).filter(|s| !s.is_empty())?.to_string();
        let user_ata = pump_accounts.get(5).cloned().unwrap_or_default();

        let side = match kind {
            InstructionKind::Buy => Side::Buy,
            InstructionKind::Sell => Side::Sell,
            _ => return None,
        };

        let sol_amount = compute_sol_change(&user, account_keys, pre_balances, post_balances);
        let sol_lamports =
            compute_sol_change_lamports(&user, account_keys, pre_balances, post_balances);
        let token_amount = compute_token_change_pb(&user_ata, &mint, account_keys, pre_token_balances, post_token_balances);

        if sol_amount < p.min_trade_sol {
            return None;
        }

        let price = if token_amount > 0 { sol_amount / token_amount as f64 } else { 0.0 };

        Some(IngestEvent::Trade(Trade {
            mint,
            wallet: user,
            side,
            sol: sol_amount,
            sol_lamports,
            tokens: token_amount,
            price,
            // NOTE: on this fallback `sol_amount` is the payer's own lamport
            // delta, so it already absorbs the fee — unlike the TradeEvent path,
            // where `sol` is the program-emitted swap amount. The fee is still
            // reported as its own value; don't subtract it from `sol` here or the
            // two paths would price the same trade differently.
            fee_lamports: fee_budget.fee_lamports,
            cu_limit: fee_budget.cu_limit,
            cu_price: fee_budget.cu_price,
            tip_lamports: fee_budget.tip_lamports,
            signature: signature.to_string(),
            tx_index,
            leg_index: 0,
            slot,
            block_time: received_at,
            received_at,
            reserves: Reserves::default(),
            venue: Venue::Curve,
            instruction_type: match side { Side::Buy => "Buy".to_string(), Side::Sell => "Sell".to_string() },
            instruction_labels,
            amm_swap_accounts: None,
        }))
    }
}

// ── Token-amount delta helper ─────────────────────────────────────────────────

fn compute_token_change_pb(
    user_ata: &str,
    mint: &str,
    account_keys: &[&str],
    pre: &[scb::TokenBalance],
    post: &[scb::TokenBalance],
) -> u64 {
    let ata_idx = match account_keys.iter().position(|k| *k == user_ata) {
        Some(i) => i as u32,
        None => return 0,
    };
    // `ui_token_amount.amount` is the RAW integer balance as a string (the chain's
    // u64); `uiAmount` is the decimal-scaled view. Parse the raw form to `u64` so the
    // delta stays exact — no `f64` round-trip.
    let find_amount = |balances: &[scb::TokenBalance]| -> u64 {
        balances.iter()
            .find(|tb| tb.account_index == ata_idx && tb.mint == mint)
            .and_then(|tb| tb.ui_token_amount.as_ref())
            .and_then(|u| u.amount.parse::<u64>().ok())
            .unwrap_or(0)
    };
    // Balance moves down on a sell, up on a buy; the magnitude is the trade size.
    find_amount(post).abs_diff(find_amount(pre))
}

// ── Inner-instruction TradeEvent decode ───────────────────────────────────────

fn decode_trade_events_from_inner_pb(pump_ixs: &[PbIx], p: &Protocol) -> Vec<DecodedTradeEvent> {
    let anchor_disc = &p.discriminators.anchor_event_cpi;
    let trade_disc = &p.discriminators.trade_event;
    let mut events = Vec::new();

    for ix in pump_ixs {
        let bytes = ix.data;
        if bytes.len() < 16
            || &bytes[..8] != anchor_disc
            || &bytes[8..16] != trade_disc
        {
            continue;
        }
        let mut buf: &[u8] = &bytes[16..];
        match RawTradeEvent::deserialize(&mut buf) {
            Ok(r) => events.push(DecodedTradeEvent::from_raw(r, p.lamports_per_sol)),
            Err(e) => warn!("Failed to Borsh-decode inner-pb TradeEvent: {e}"),
        }
    }
    events
}

// ── Protobuf helpers ──────────────────────────────────────────────────────────

fn find_program_pb_ixs<'a>(
    message: &'a scb::Message,
    meta: &'a scb::TransactionStatusMeta,
    keys: &LazyKeys,
    pump_id_bytes: &[u8],
) -> Vec<PbIx<'a>> {
    let mut out = Vec::new();
    let is_pump = |idx: u32| keys.raw(idx as usize) == Some(pump_id_bytes);

    for ix in &message.instructions {
        if is_pump(ix.program_id_index) {
            out.push(PbIx { accounts: &ix.accounts, data: &ix.data });
        }
    }
    for group in &meta.inner_instructions {
        for ix in &group.instructions {
            if is_pump(ix.program_id_index) {
                out.push(PbIx { accounts: &ix.accounts, data: &ix.data });
            }
        }
    }
    out
}

fn collect_kinds_pb(
    message: &scb::Message,
    meta: &scb::TransactionStatusMeta,
    keys: &LazyKeys,
    p: &Protocol,
) -> Vec<InstructionKind> {
    let pump_bytes = p.programs.pump_fun.bytes.as_ref();
    let mut kinds = Vec::new();

    for ix in &message.instructions {
        let is_pump = keys.raw(ix.program_id_index as usize) == Some(pump_bytes);
        if let Some(kind) = classify_pump_ix(is_pump, Some(&ix.data), p) {
            kinds.push(kind);
        }
    }
    for group in &meta.inner_instructions {
        for ix in &group.instructions {
            let is_pump = keys.raw(ix.program_id_index as usize) == Some(pump_bytes);
            if is_pump {
                if let Some(kind) = classify_pump_ix(true, Some(&ix.data), p) {
                    kinds.push(kind);
                }
            }
        }
    }
    kinds
}

/// Label every top-level instruction and read the transaction's fee budget off the
/// same single walk.
///
/// **Top-level only, for the tip as well as the labels.** An inner (CPI) transfer is
/// the venue moving its own protocol fee, not the sender buying priority, so widening
/// this walk would book the curve's rake as urgency. It also keeps the tip on exactly
/// the instruction list `ix_labels` already exposes, so a `System Program: Transfer`
/// label and a `tip_lamports` value always describe the same instructions.
fn build_labels_pb(
    message: &scb::Message,
    meta: &scb::TransactionStatusMeta,
    keys: &LazyKeys,
    p: &Protocol,
) -> (Vec<String>, FeeBudget) {
    // Charged once for the whole tx; stamped on every leg it produced.
    let mut budget = FeeBudget { fee_lamports: fee_lamports_opt(meta.fee), ..Default::default() };
    let cb_bytes = p.programs.compute_budget.bytes.as_ref();
    let sys_bytes = p.programs.system.bytes.as_ref();

    let labels = message
        .instructions
        .iter()
        .map(|ix| {
            let raw_pid = keys.raw(ix.program_id_index as usize);
            budget.note_compute_budget(raw_pid == Some(cb_bytes), Some(&ix.data));
            if let Some(lamports) =
                system_transfer_lamports(raw_pid == Some(sys_bytes), Some(&ix.data))
            {
                // Transfer accounts are [from, to]; the tip is where it LANDS.
                let dest = ix.accounts.get(1).and_then(|&i| keys.raw(i as usize));
                budget.note_transfer(lamports, dest.is_some_and(|d| p.is_tip_account(d)));
            }
            let pid = keys.get(ix.program_id_index as usize);
            label_instruction(pid, None, Some(&ix.data), p)
        })
        .collect();

    (labels, budget)
}

fn resolve_pump_accounts_pb(ix: &PbIx, keys: &LazyKeys) -> Vec<String> {
    ix.accounts
        .iter()
        .filter_map(|&i| {
            let s = keys.get(i as usize);
            if s.is_empty() { None } else { Some(s.to_string()) }
        })
        .collect()
}

/// Is this instruction data a pump.fun `create` / `create_v2`? The ONE reader of
/// the create discriminators on the protobuf path — shared by the decode-side
/// [`is_create_pb`] and by the transport pre-filter's `Decoder::has_create_ix`,
/// so the "what counts as a create" rule cannot drift between classify and decode.
fn is_create_disc(data: &[u8], p: &Protocol) -> bool {
    let d = &p.discriminators;
    data.starts_with(&d.create_ix) || data.starts_with(&d.create_v2_ix)
}

fn is_create_pb(ix: &PbIx, p: &Protocol) -> bool {
    is_create_disc(ix.data, p)
}

fn is_migrate_pb(ix: &PbIx, p: &Protocol) -> bool {
    let d = &p.discriminators;
    ix.data.len() >= 8 && (&ix.data[..8] == d.migrate_ix || &ix.data[..8] == d.migrate_v2_ix)
}

/// Gate for the log-truncation event recovery — shared by BOTH the curve
/// (`TradeEvent`) and AMM (`PumpSwap Buy/Sell`) paths.
///
/// Returns `true` when the cheap log-scanned event set may be incomplete and the
/// complete-but-costlier inner-CPI events should be consulted. Recovery is needed
/// both when the log scan is **empty** (fully truncated / no `Program data:` lines)
/// AND when the logs were **partially truncated** — the latter is the regression
/// this guards: a 4-swap bundle that logs only 3 events + a `"Log truncated"` marker
/// is non-empty, so a plain `is_empty()` check skipped the recovery and permanently
/// dropped the 4th leg. Keeping the gate cheap (a length + a bool) means the hot
/// path only decodes the inner instructions when it must.
fn should_consult_inner_events(log_event_count: usize, logs_truncated: bool) -> bool {
    log_event_count == 0 || logs_truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::should_consult_inner_events;
    use crate::protocol::Protocol;
    use std::sync::Arc;

    // ── classify fixtures ─────────────────────────────────────────────────────

    /// Decoder with a pool index attached, so `classify_logs`' Amm arm is live
    /// and the two classifiers are compared on equal footing.
    fn decoder() -> Decoder {
        Decoder::new(Arc::new(Protocol::pump_fun()))
            .with_pool_index(Arc::new(dashmap::DashMap::new()))
    }

    fn pump_key() -> Vec<u8> {
        Protocol::pump_fun().programs.pump_fun.bytes.to_vec()
    }
    fn swap_key() -> Vec<u8> {
        Protocol::pump_fun().programs.pump_swap.bytes.to_vec()
    }
    /// Any 32-byte key that is neither program.
    fn other_key(seed: u8) -> Vec<u8> {
        vec![seed; 32]
    }

    fn disc(name: &str) -> Vec<u8> {
        let d = Protocol::pump_fun().discriminators;
        let bytes = match name {
            "create" => d.create_ix,
            "create_v2" => d.create_v2_ix,
            "buy" => d.buy,
            "sell" => d.sell,
            other => panic!("unknown discriminator {other}"),
        };
        // Discriminator + a little payload, like a real ix.
        bytes.iter().copied().chain([0u8; 16]).collect()
    }

    #[derive(Default)]
    struct Tx {
        keys: Vec<Vec<u8>>,
        loaded_writable: Vec<Vec<u8>>,
        loaded_readonly: Vec<Vec<u8>>,
        ixs: Vec<scb::CompiledInstruction>,
        inner: Vec<scb::InnerInstructions>,
        logs: Vec<String>,
        fee_lamports: u64,
    }

    impl Tx {
        fn key(mut self, k: Vec<u8>) -> Self {
            self.keys.push(k);
            self
        }
        fn loaded_readonly(mut self, k: Vec<u8>) -> Self {
            self.loaded_readonly.push(k);
            self
        }
        fn ix(mut self, program_id_index: u32, data: Vec<u8>) -> Self {
            self.ixs.push(scb::CompiledInstruction {
                program_id_index,
                accounts: vec![],
                data,
            });
            self
        }
        /// A top-level ix that names accounts - a transfer's `[from, to]` is the
        /// only thing separating a tip from a router's own rake.
        fn ix_with(mut self, program_id_index: u32, accounts: Vec<u8>, data: Vec<u8>) -> Self {
            self.ixs.push(scb::CompiledInstruction { program_id_index, accounts, data });
            self
        }
        fn fee(mut self, lamports: u64) -> Self {
            self.fee_lamports = lamports;
            self
        }
        fn inner_ix(mut self, program_id_index: u32, data: Vec<u8>) -> Self {
            self.inner.push(scb::InnerInstructions {
                index: 0,
                instructions: vec![scb::InnerInstruction {
                    program_id_index,
                    accounts: vec![],
                    data,
                    stack_height: Some(2),
                }],
            });
            self
        }
        /// A realistic `Program <id> invoke [1]` line — what the old classify scanned.
        fn invoke_log(mut self, program: &[u8]) -> Self {
            self.logs.push(format!(
                "Program {} invoke [1]",
                bs58::encode(program).into_string()
            ));
            self
        }
        fn build(self) -> SubscribeUpdateTransactionInfo {
            SubscribeUpdateTransactionInfo {
                signature: vec![7u8; 64],
                is_vote: false,
                transaction: Some(scb::Transaction {
                    signatures: vec![vec![7u8; 64]],
                    message: Some(scb::Message {
                        account_keys: self.keys,
                        instructions: self.ixs,
                        ..Default::default()
                    }),
                }),
                meta: Some(scb::TransactionStatusMeta {
                    fee: self.fee_lamports,
                    log_messages: self.logs,
                    inner_instructions: self.inner,
                    loaded_writable_addresses: self.loaded_writable,
                    loaded_readonly_addresses: self.loaded_readonly,
                    ..Default::default()
                }),
                index: 3,
            }
        }
    }

    fn logs_of(info: &SubscribeUpdateTransactionInfo) -> Vec<String> {
        info.meta.as_ref().unwrap().log_messages.clone()
    }

    /// Collapse `Create` back to `Curve` — the axis on which the two classifiers
    /// must agree. `Create` is a routing hint the log scan never produced.
    fn family(r: Option<TxRelevance>) -> Option<TxRelevance> {
        r.map(|r| {
            if r.is_curve() {
                TxRelevance::Curve
            } else {
                TxRelevance::Amm
            }
        })
    }

    /// **Parity guard for the log-scan → account-key classify swap.** Every
    /// fixture is a shape the transport actually receives; on all of them the new
    /// message-based classify must land on the same family the old log scan did,
    /// so the swap cannot silently change what gets ingested.
    #[test]
    fn account_key_classify_agrees_with_the_log_scan() {
        let d = decoder();
        let corpus: Vec<(&str, SubscribeUpdateTransactionInfo)> = vec![
            (
                "curve buy",
                Tx::default()
                    .key(other_key(1))
                    .key(pump_key())
                    .ix(1, disc("buy"))
                    .invoke_log(&pump_key())
                    .build(),
            ),
            (
                "curve sell",
                Tx::default()
                    .key(other_key(2))
                    .key(pump_key())
                    .ix(1, disc("sell"))
                    .invoke_log(&pump_key())
                    .build(),
            ),
            (
                "top-level create",
                Tx::default()
                    .key(other_key(3))
                    .key(pump_key())
                    .ix(1, disc("create"))
                    .invoke_log(&pump_key())
                    .build(),
            ),
            (
                "create_v2 via inner CPI (bundled launch)",
                Tx::default()
                    .key(other_key(4))
                    .key(other_key(9))
                    .key(pump_key())
                    .ix(1, vec![0xAA; 12])
                    .inner_ix(2, disc("create_v2"))
                    .invoke_log(&pump_key())
                    .build(),
            ),
            (
                "amm swap",
                Tx::default()
                    .key(other_key(5))
                    .key(swap_key())
                    .ix(1, vec![0xBB; 12])
                    .invoke_log(&swap_key())
                    .build(),
            ),
            (
                "both programs — curve wins",
                Tx::default()
                    .key(other_key(6))
                    .key(swap_key())
                    .key(pump_key())
                    .ix(2, disc("buy"))
                    .invoke_log(&swap_key())
                    .invoke_log(&pump_key())
                    .build(),
            ),
            (
                "pump program only in an ALT-loaded readonly address",
                Tx::default()
                    .key(other_key(7))
                    .loaded_readonly(pump_key())
                    .ix(1, disc("buy"))
                    .invoke_log(&pump_key())
                    .build(),
            ),
            (
                "unrelated program",
                Tx::default()
                    .key(other_key(8))
                    .key(other_key(20))
                    .ix(1, vec![0xCC; 12])
                    .invoke_log(&other_key(20))
                    .build(),
            ),
        ];

        for (name, info) in &corpus {
            let old = d.classify_logs(&logs_of(info));
            let new = d.classify_accounts(info);
            assert_eq!(
                family(new),
                old,
                "{name}: account-key classify {new:?} disagrees with log scan {old:?}"
            );
        }
    }

    /// The create tag is the whole point of the rewrite: it must fire on the
    /// create shapes and on nothing else. (A missed create only costs a routing
    /// hint — but a create tag on a plain swap would mis-prioritise the lane.)
    #[test]
    fn create_is_tagged_only_on_actual_creates() {
        let d = decoder();
        let top_level = Tx::default()
            .key(other_key(1))
            .key(pump_key())
            .ix(1, disc("create"))
            .build();
        let inner = Tx::default()
            .key(other_key(1))
            .key(pump_key())
            .ix(1, vec![0xAA; 12])
            .inner_ix(1, disc("create_v2"))
            .build();
        let buy = Tx::default()
            .key(other_key(1))
            .key(pump_key())
            .ix(1, disc("buy"))
            .build();
        // A create discriminator on a NON-pump program is not our create.
        let impostor = Tx::default()
            .key(other_key(1))
            .key(pump_key())
            .key(other_key(30))
            .ix(2, disc("create"))
            .ix(1, disc("buy"))
            .build();

        assert_eq!(d.classify_accounts(&top_level), Some(TxRelevance::Create));
        assert_eq!(d.classify_accounts(&inner), Some(TxRelevance::Create));
        assert_eq!(d.classify_accounts(&buy), Some(TxRelevance::Curve));
        assert_eq!(d.classify_accounts(&impostor), Some(TxRelevance::Curve));
        // Both tags decode down the same path — the tag can never change output.
        assert!(TxRelevance::Create.is_curve() && TxRelevance::Curve.is_curve());
        assert!(!TxRelevance::Amm.is_curve());
    }

    /// Deliberate, documented divergence: a validator that truncates or drops a
    /// tx's logs hides the program id from the log scan but not from the account
    /// keys. The new classify keeps such a tx (decode then decides); the old one
    /// dropped it before decode ever saw it.
    #[test]
    fn account_key_classify_survives_dropped_logs() {
        let d = decoder();
        let no_logs = Tx::default()
            .key(other_key(1))
            .key(pump_key())
            .ix(1, disc("buy"))
            .build();
        assert_eq!(d.classify_logs(&logs_of(&no_logs)), None);
        assert_eq!(d.classify_accounts(&no_logs), Some(TxRelevance::Curve));
    }

    /// `key_at` is a zero-alloc re-implementation of `LazyKeys`' index space.
    /// Both must resolve every index — including the ALT-loaded tail — to the
    /// same bytes, or `program_id_index` would mean different things in the
    /// pre-filter and in the decoder.
    #[test]
    fn key_at_matches_lazykeys_ordering() {
        let info = Tx::default()
            .key(other_key(1))
            .key(pump_key())
            .loaded_readonly(swap_key())
            .loaded_readonly(other_key(2))
            .build();
        let message = info
            .transaction
            .as_ref()
            .unwrap()
            .message
            .as_ref()
            .unwrap();
        let meta = info.meta.as_ref().unwrap();
        let lazy = LazyKeys::new(message, meta);
        assert_eq!(lazy.raw.len(), 4);
        for i in 0..lazy.raw.len() + 2 {
            assert_eq!(key_at(message, meta, i), lazy.raw(i), "index {i}");
        }
    }

    /// Regression guard for the log-truncation dropped-legs fix: inner-CPI
    /// recovery must fire on PARTIAL truncation, not only on empty logs.
    #[test]
    fn inner_recovery_fires_on_partial_and_full_truncation() {
        // Fully truncated / no TradeEvent log lines → consult inner.
        assert!(should_consult_inner_events(0, false));
        assert!(should_consult_inner_events(0, true));
        // Partial truncation (3-of-4 bundle: non-empty yet incomplete) → consult
        // inner. This is the exact case a plain `is_empty()` check missed.
        assert!(should_consult_inner_events(3, true));
        // Complete, untruncated logs → trust the log scan, skip the inner decode
        // (hot-path fast path — most txs land here).
        assert!(!should_consult_inner_events(3, false));
        assert!(!should_consult_inner_events(1, false));
    }

    mod fee_budget {
        //! What a sender paid to be early, read off the same instruction walk that
        //! labels the transaction.
        //!
        //! These drive the real [`build_labels_pb`] against protobuf messages shaped
        //! like the ones on the feed, because every interesting case here is a
        //! DISCRIMINATION - tip vs router rake, top-level vs CPI, transfer vs some other
        //! system instruction - and a unit test on the byte decoder alone would pass
        //! while the walk fed it the wrong instructions.

        // `super` is `tests` (the shared `Tx` builder); `super::super`
        // is the decode module under test.
        use super::super::*;
        use super::Tx;
        use crate::protocol::Protocol;

        /// `SetComputeUnitLimit(u32)` - borsh enum tag 2, then the limit.
        fn cu_limit_ix(limit: u32) -> Vec<u8> {
            let mut d = vec![2u8];
            d.extend_from_slice(&limit.to_le_bytes());
            d
        }

        /// `SetComputeUnitPrice(u64)` - borsh enum tag 3, then micro-lamports per CU.
        fn cu_price_ix(price: u64) -> Vec<u8> {
            let mut d = vec![3u8];
            d.extend_from_slice(&price.to_le_bytes());
            d
        }

        /// `SystemInstruction::Transfer` - a FOUR-byte bincode tag, then the lamports.
        fn transfer_ix(lamports: u64) -> Vec<u8> {
            let mut d = vec![2u8, 0, 0, 0];
            d.extend_from_slice(&lamports.to_le_bytes());
            d
        }

        /// `AdvanceNonceAccount` - tag 4, and deliberately as long as a transfer. Its
        /// low byte is not 2, but a decoder reading only one byte of the tag (the way
        /// `system_ix_name` can afford to) would still have to get here to be wrong.
        fn advance_nonce_ix() -> Vec<u8> {
            let mut d = vec![4u8, 0, 0, 0];
            d.extend_from_slice(&99_999u64.to_le_bytes());
            d
        }

        fn budget_of(info: &SubscribeUpdateTransactionInfo, p: &Protocol) -> FeeBudget {
            let tx = info.transaction.as_ref().unwrap();
            let message = tx.message.as_ref().unwrap();
            let meta = info.meta.as_ref().unwrap();
            let keys = LazyKeys::new(message, meta);
            build_labels_pb(message, meta, &keys, p).1
        }

        /// Index 0 = ComputeBudget, 1 = System, 2 = the fee payer, 3 = a real tip
        /// account, 4 = a stranger. Fixed so every test below reads the same way.
        fn tx_with(p: &Protocol) -> Tx {
            Tx::default()
                .key(p.programs.compute_budget.bytes.to_vec())
                .key(p.programs.system.bytes.to_vec())
                .key(vec![0xAA; 32])
                .key(p.tip_accounts[0].to_vec())
                .key(vec![0xBB; 32])
        }

        /// The compute rail's share of the priority spend, as the chain bills it.
        fn compute_rail(b: &FeeBudget) -> u128 {
            (u128::from(b.cu_limit.unwrap()) * u128::from(b.cu_price.unwrap())).div_ceil(1_000_000)
        }

        // -- the compute rail -----------------------------------------------------

        #[test]
        fn both_compute_budget_instructions_are_read() {
            let p = Protocol::pump_fun();
            let info = tx_with(&p)
                .ix(0, cu_limit_ix(300_000))
                .ix(0, cu_price_ix(3_333_333))
                .build();

            let b = budget_of(&info, &p);
            assert_eq!(b.cu_limit, Some(300_000));
            assert_eq!(b.cu_price, Some(3_333_333));
        }

        /// The reason `cu_price` is never read alone. All three of these are one
        /// decision - "spend 0.001 SOL to be early" - and they agree in no part.
        #[test]
        fn different_pairs_encode_the_same_spend() {
            let p = Protocol::pump_fun();
            let spend = |limit: u32, price: u64| {
                let info = tx_with(&p)
                    .ix(0, cu_limit_ix(limit))
                    .ix(0, cu_price_ix(price))
                    .build();
                compute_rail(&budget_of(&info, &p))
            };
            // The most common cu_price on the live tape, and this is why: it is
            // "0.001 SOL" divided by the 300k limit beside it.
            assert_eq!(spend(300_000, 3_333_333), 1_000_000);
            assert_eq!(spend(100_000, 10_000_000), 1_000_000);
            assert_eq!(spend(1_000_000, 1_000_000), 1_000_000);
        }

        #[test]
        fn a_transaction_that_sets_no_compute_budget_reads_none() {
            let p = Protocol::pump_fun();
            let info = tx_with(&p).ix_with(1, vec![2, 3], transfer_ix(5_000)).build();
            let b = budget_of(&info, &p);
            assert_eq!(b.cu_limit, None);
            assert_eq!(b.cu_price, None);
        }

        // -- the tip rail ---------------------------------------------------------

        #[test]
        fn a_transfer_to_a_known_tip_account_is_a_tip() {
            let p = Protocol::pump_fun();
            // accounts = [from = payer(2), to = tip(3)]
            let info = tx_with(&p)
                .ix_with(1, vec![2, 3], transfer_ix(1_500_000))
                .build();
            assert_eq!(budget_of(&info, &p).tip_lamports, Some(1_500_000));
        }

        #[test]
        fn a_transfer_to_a_stranger_reads_zero_not_none() {
            let p = Protocol::pump_fun();
            // A router paying its own rake, or a tip rail the list does not know yet.
            // Zero is a READING: "transfers happened, none were tips" - the bucket that
            // measures how far behind TIP_ACCOUNT_IDS has fallen.
            let info = tx_with(&p)
                .ix_with(1, vec![2, 4], transfer_ix(900_000))
                .build();
            assert_eq!(budget_of(&info, &p).tip_lamports, Some(0));
        }

        #[test]
        fn no_transfer_at_all_reads_none() {
            let p = Protocol::pump_fun();
            let info = tx_with(&p).ix(0, cu_limit_ix(200_000)).build();
            assert_eq!(budget_of(&info, &p).tip_lamports, None);
        }

        /// The shape the live tape is full of: a router tx carrying TWO transfers, one
        /// buying priority and one paying the router itself. Only the first is urgency.
        #[test]
        fn a_rake_beside_a_tip_counts_only_the_tip() {
            let p = Protocol::pump_fun();
            let info = tx_with(&p)
                .ix_with(1, vec![2, 3], transfer_ix(1_000_000)) // -> tip account
                .ix_with(1, vec![2, 4], transfer_ix(7_000_000)) // -> the router's own wallet
                .build();
            assert_eq!(budget_of(&info, &p).tip_lamports, Some(1_000_000));
        }

        #[test]
        fn two_tips_in_one_transaction_sum() {
            let p = Protocol::pump_fun();
            let info = tx_with(&p)
                .ix_with(1, vec![2, 3], transfer_ix(400_000))
                .ix_with(1, vec![2, 3], transfer_ix(600_000))
                .build();
            assert_eq!(budget_of(&info, &p).tip_lamports, Some(1_000_000));
        }

        /// An inner transfer is the VENUE moving its own protocol fee, not the sender
        /// buying priority - counting it would book the curve's rake as urgency.
        #[test]
        fn an_inner_transfer_is_never_a_tip() {
            let p = Protocol::pump_fun();
            let info = tx_with(&p)
                .ix(0, cu_limit_ix(200_000))
                .inner_ix(1, transfer_ix(2_000_000))
                .build();
            assert_eq!(budget_of(&info, &p).tip_lamports, None);
        }

        /// The four-byte tag is checked in full. A system instruction that merely
        /// shares a low tag byte must not have its bytes read as money.
        #[test]
        fn another_system_instruction_is_not_a_transfer() {
            let p = Protocol::pump_fun();
            let info = tx_with(&p)
                .ix_with(1, vec![2, 3], advance_nonce_ix())
                .build();
            assert_eq!(budget_of(&info, &p).tip_lamports, None);
        }

        /// A transfer whose destination index is missing decodes to "not a tip" - never
        /// a panic, and never a tip by default.
        #[test]
        fn a_transfer_with_no_destination_is_not_a_tip() {
            let p = Protocol::pump_fun();
            let info = tx_with(&p)
                .ix_with(1, vec![2], transfer_ix(1_000_000))
                .build();
            assert_eq!(budget_of(&info, &p).tip_lamports, Some(0));
        }

        /// A tip is only a tip because of WHERE it lands, so the whole registry has to
        /// be reachable - not just the first entry every other test here uses.
        #[test]
        fn every_registered_tip_account_is_recognised() {
            let p = Protocol::pump_fun();
            assert_eq!(p.tip_accounts.len(), 20);
            for (i, acct) in p.tip_accounts.iter().enumerate() {
                let info = Tx::default()
                    .key(p.programs.compute_budget.bytes.to_vec())
                    .key(p.programs.system.bytes.to_vec())
                    .key(vec![0xAA; 32])
                    .key(acct.to_vec())
                    .ix_with(1, vec![2, 3], transfer_ix(1_000))
                    .build();
                assert_eq!(
                    budget_of(&info, &p).tip_lamports,
                    Some(1_000),
                    "tip account #{i} is in the registry but does not read as one",
                );
            }
        }

        // -- the charged fee, beside the chosen budget ----------------------------

        #[test]
        fn the_charged_fee_comes_from_meta_and_zero_means_unknown() {
            let p = Protocol::pump_fun();
            let paid = tx_with(&p).ix(0, cu_limit_ix(200_000)).fee(5_000).build();
            assert_eq!(budget_of(&paid, &p).fee_lamports, Some(5_000));

            // A landed tx always pays the base fee, so 0 is "not captured" - the whole
            // reason `fee_lamports_opt` exists.
            let unknown = tx_with(&p).ix(0, cu_limit_ix(200_000)).build();
            assert_eq!(budget_of(&unknown, &p).fee_lamports, None);
        }

        /// Both rails at once, which is what the sum is for.
        #[test]
        fn a_sender_can_pay_on_both_rails() {
            let p = Protocol::pump_fun();
            let info = tx_with(&p)
                .ix(0, cu_limit_ix(300_000))
                .ix(0, cu_price_ix(3_333_333))
                .ix_with(1, vec![2, 3], transfer_ix(2_000_000))
                .fee(1_005_000)
                .build();

            let b = budget_of(&info, &p);
            assert_eq!(compute_rail(&b), 1_000_000);
            assert_eq!(b.tip_lamports, Some(2_000_000));
            // priority spend = compute rail + tip rail = 0.003 SOL.
            assert_eq!(compute_rail(&b) + u128::from(b.tip_lamports.unwrap()), 3_000_000);
        }
    }
}
