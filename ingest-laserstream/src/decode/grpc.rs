//! Protobuf-native decode of LaserStream `SubscribeUpdateTransaction` updates.

use std::cell::OnceCell;

use borsh::BorshDeserialize;
use chrono::{DateTime, Utc};
use tracing::warn;

use crate::event::{IngestEvent, Reserves, Side, TokenMigrated, Trade, Venue};
use crate::pool::register_pool;
use crate::protocol::Protocol;

use super::create::decode_create_events_from_logs;
use super::instructions::{
    classify_pump_ix, determine_instruction_type, extract_compute_budget, label_instruction,
    InstructionKind,
};
use super::trade::{
    build_amm_trade, compute_sol_change, decode_pump_swap_trades_from_logs,
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

        match relevance {
            TxRelevance::Curve => self.decode_curve_pb(info, message, meta, update.slot, received_at),
            TxRelevance::Amm => self.decode_amm_live_pb(info, message, meta, update.slot, received_at),
        }
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

        // Step 1a: TradeEvents from logs (or inner-ix fallback).
        let mut decoded_events = decode_trade_events_from_logs(&logs, &p.discriminators.trade_event, p.lamports_per_sol);
        if decoded_events.is_empty() {
            let pump_ixs = find_pump_pb_ixs(message, meta, &keys, &p.programs.pump_fun.bytes);
            decoded_events = decode_trade_events_from_inner_pb(&pump_ixs, p);
        }

        let decoded_create_events = decode_create_events_from_logs(&logs, &p.discriminators.create_event);

        // Step 1b: instruction kinds.
        let kinds = collect_kinds_pb(message, meta, &keys, p);
        let instruction_type = determine_instruction_type(&kinds, &decoded_events);

        // Step 2: labels + compute budget.
        let (instruction_labels, cu_limit, cu_price) = build_labels_pb(message, meta, &keys, p);

        // Step 3: build IngestEvents.
        let mut events: Vec<IngestEvent> = Vec::new();
        let has_create = kinds.iter().any(|k| matches!(k, InstructionKind::Create));

        // 3a: one Trade per decoded TradeEvent.
        for (leg_index, ev) in decoded_events.iter().enumerate() {
            if ev.sol_amount < p.min_trade_sol {
                continue;
            }
            let side = if ev.is_buy { Side::Buy } else { Side::Sell };
            let price = if ev.token_amount > 0.0 { ev.sol_amount / ev.token_amount } else { 0.0 };
            events.push(IngestEvent::Trade(Trade {
                mint: ev.mint.clone(),
                wallet: ev.user.clone(),
                side,
                sol: ev.sol_amount,
                tokens: ev.token_amount,
                price,
                signature: signature.clone(),
                leg_index: leg_index as u32,
                slot,
                block_time: received_at,
                received_at,
                reserves: Reserves {
                    virtual_sol: Some(ev.virtual_sol_reserves),
                    virtual_token: Some(ev.virtual_token_reserves),
                    real_sol: Some(ev.real_sol_reserves),
                    real_token: Some(ev.real_token_reserves),
                },
                venue: Venue::Curve,
                instruction_type: if ev.is_buy { "Buy".to_string() } else { "Sell".to_string() },
                instruction_labels: instruction_labels.clone(),
            }));
        }

        // 3b: balance-delta fallback (no TradeEvent log, but has Buy/Sell instruction).
        if decoded_events.is_empty() {
            let pump_ixs = find_pump_pb_ixs(message, meta, &keys, &p.programs.pump_fun.bytes);
            let all_keys = keys.all();
            for kind in &kinds {
                if !matches!(kind, InstructionKind::Buy | InstructionKind::Sell) {
                    continue;
                }
                if let Some(pump_ix) = pump_ixs.first() {
                    let ix_accounts = resolve_pump_accounts_pb(pump_ix, &keys);
                    if let Some(ev) = self.decode_trade_from_balances_pb(
                        *kind, &signature, slot, received_at,
                        &ix_accounts, &all_keys,
                        &meta.pre_balances, &meta.post_balances,
                        &meta.pre_token_balances, &meta.post_token_balances,
                        instruction_labels.clone(),
                    ) {
                        events.push(ev);
                    }
                    break;
                }
            }
        }

        // 3c: Create.
        if has_create {
            let pump_ixs = find_pump_pb_ixs(message, meta, &keys, &p.programs.pump_fun.bytes);
            if let Some(create_ix) = pump_ixs.iter().find(|ix| is_create_pb(ix, p)) {
                let ix_accounts = resolve_pump_accounts_pb(create_ix, &keys);
                let all_keys = keys.all();
                let pump_ix_datas: Vec<&[u8]> = pump_ixs.iter().map(|ix| ix.data).collect();
                events.extend(self.decode_create(
                    &signature, slot, received_at, received_at,
                    create_ix.data, &ix_accounts, &all_keys,
                    &decoded_events, &decoded_create_events,
                    &instruction_type, instruction_labels.clone(),
                    cu_limit, cu_price, &pump_ix_datas,
                ));
            }
        }

        // 3d: Migrate.
        if kinds.iter().any(|k| matches!(k, InstructionKind::Migrate)) {
            let pump_ixs = find_pump_pb_ixs(message, meta, &keys, &p.programs.pump_fun.bytes);
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

        let logs: Vec<&str> = meta.log_messages.iter().map(String::as_str).collect();
        let resolved: Vec<(DecodedAmmTrade, String)> = decode_pump_swap_trades_from_logs(&logs, p)
            .into_iter()
            .filter(|ev| ev.quote_amount >= p.min_trade_sol)
            .filter_map(|ev| index.get(&ev.pool).map(|m| (ev, m.value().clone())))
            .collect();

        if resolved.is_empty() {
            return DecodeOutput::Ignored;
        }

        let signature = bs58::encode(&info.signature).into_string();
        let keys = LazyKeys::new(message, meta);
        let (labels, _, _) = build_labels_pb(message, meta, &keys, p);

        let mut events = Vec::with_capacity(resolved.len());
        for (i, (ev, mint)) in resolved.iter().enumerate() {
            events.push(IngestEvent::Trade(build_amm_trade(
                ev, mint, &signature, slot, received_at, received_at,
                labels.clone(), i as u32,
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
                // Newly registered — signal the transport task to resubscribe.
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
        slot: u64,
        received_at: DateTime<Utc>,
        pump_accounts: &[String],
        account_keys: &[&str],
        pre_balances: &[u64],
        post_balances: &[u64],
        pre_token_balances: &[scb::TokenBalance],
        post_token_balances: &[scb::TokenBalance],
        instruction_labels: Vec<String>,
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
        let token_amount = compute_token_change_pb(&user_ata, &mint, account_keys, pre_token_balances, post_token_balances);

        if sol_amount < p.min_trade_sol {
            return None;
        }

        let price = if token_amount > 0.0 { sol_amount / token_amount } else { 0.0 };

        Some(IngestEvent::Trade(Trade {
            mint,
            wallet: user,
            side,
            sol: sol_amount,
            tokens: token_amount,
            price,
            signature: signature.to_string(),
            leg_index: 0,
            slot,
            block_time: received_at,
            received_at,
            reserves: Reserves::default(),
            venue: Venue::Curve,
            instruction_type: match side { Side::Buy => "Buy".to_string(), Side::Sell => "Sell".to_string() },
            instruction_labels,
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
) -> f64 {
    let ata_idx = match account_keys.iter().position(|k| *k == user_ata) {
        Some(i) => i as u32,
        None => return 0.0,
    };
    let find_amount = |balances: &[scb::TokenBalance]| -> f64 {
        balances.iter()
            .find(|tb| tb.account_index == ata_idx && tb.mint == mint)
            .and_then(|tb| tb.ui_token_amount.as_ref())
            .and_then(|u| u.amount.parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    (find_amount(post) - find_amount(pre)).abs()
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

fn find_pump_pb_ixs<'a>(
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

fn build_labels_pb(
    message: &scb::Message,
    _meta: &scb::TransactionStatusMeta,
    keys: &LazyKeys,
    p: &Protocol,
) -> (Vec<String>, Option<u64>, Option<u64>) {
    let mut cu_limit: Option<u64> = None;
    let mut cu_price: Option<u64> = None;
    let cb_bytes = p.programs.compute_budget.bytes.as_ref();

    let labels = message
        .instructions
        .iter()
        .map(|ix| {
            let pid = keys.get(ix.program_id_index as usize);
            let is_cb = keys.raw(ix.program_id_index as usize) == Some(cb_bytes);
            extract_compute_budget(is_cb, Some(&ix.data), &mut cu_limit, &mut cu_price);
            label_instruction(pid, None, Some(&ix.data), p)
        })
        .collect();

    (labels, cu_limit, cu_price)
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

fn is_create_pb(ix: &PbIx, p: &Protocol) -> bool {
    let d = &p.discriminators;
    ix.data.len() >= 8 && (&ix.data[..8] == d.create_ix || &ix.data[..8] == d.create_v2_ix)
}

fn is_migrate_pb(ix: &PbIx, p: &Protocol) -> bool {
    let d = &p.discriminators;
    ix.data.len() >= 8 && (&ix.data[..8] == d.migrate_ix || &ix.data[..8] == d.migrate_v2_ix)
}
