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
            let price = if ev.token_amount > 0 { ev.sol_amount / ev.token_amount as f64 } else { 0.0 };
            events.push(IngestEvent::Trade(Trade {
                mint: ev.mint.clone(),
                wallet: ev.user.clone(),
                side,
                sol: ev.sol_amount,
                sol_lamports: ev.sol_amount_lamports,
                tokens: ev.token_amount,
                price,
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
                    cu_limit, cu_price, &pump_ix_datas,
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
        let (labels, _, _) = build_labels_pb(message, meta, &keys, p);

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
                labels.clone(), info.index as u32, i as u32, accounts,
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
    use super::should_consult_inner_events;

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
}
