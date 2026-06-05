use base64::{engine::general_purpose::STANDARD, Engine};
use borsh::BorshDeserialize;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use tracing::{debug, warn};

use crate::config::constants::{
    program_friendly_name, ASSOCIATED_TOKEN_PROGRAM_ID, BUY_DISCRIMINATOR,
    BUY_EXACT_QUOTE_IN_DISCRIMINATOR, BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR,
    BUY_EXACT_SOL_IN_DISCRIMINATOR, BUY_V2_DISCRIMINATOR, COMPUTE_BUDGET_PROGRAM_ID,
    CREATE_EVENT_DISCRIMINATOR, CREATE_INSTRUCTION_DISCRIMINATOR,
    CREATE_V2_INSTRUCTION_DISCRIMINATOR, LAMPORTS_PER_SOL, MIGRATE_INSTRUCTION_DISCRIMINATOR,
    MIGRATE_V2_INSTRUCTION_DISCRIMINATOR,
    PUMP_FUN_PROGRAM_ID, PUMP_SWAP_BUY_EVENT_DISCRIMINATOR, PUMP_SWAP_PROGRAM_ID,
    PUMP_SWAP_SELL_EVENT_DISCRIMINATOR, SELL_DISCRIMINATOR, SYSTEM_PROGRAM_ID,
    TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID, TRADE_EVENT_DISCRIMINATOR,
};
use crate::models::{
    events::{
        CreatorActivityEvent, CreatorActivityKind, InternalEvent, TokenCreatedEvent,
        TokenMigratedEvent, TradeExecutedEvent,
    },
    token::Token,
    trade::{Trade, TradeType},
    transaction::RawTransaction,
};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub struct HeliusDecoder {
    pump_program_id: String,
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
        Self { pump_program_id }
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

        // Fast-path: skip if this transaction doesn't involve Pump.fun
        if !logs.iter().any(|l| l.contains(&self.pump_program_id)) {
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
        let decoded_events = decode_trade_events_from_logs(&logs);
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
            let trade_type = if ev.is_buy {
                TradeType::Buy
            } else {
                TradeType::Sell
            };

            let mut trade = Trade::new(
                mint.to_string(),
                ev.user,
                trade_type,
                ev.quote_amount,
                ev.base_amount,
                signature.clone(),
                slot,
                block_time,
            );
            // PumpSwap has no virtual reserves; record the post-swap pool reserves.
            trade.real_sol_reserves = Some(ev.pool_quote_reserves);
            trade.real_token_reserves = Some(ev.pool_base_reserves);
            trade.instruction_type = match trade.trade_type {
                TradeType::Buy => "Buy".to_string(),
                TradeType::Sell => "Sell".to_string(),
            };
            trade.instruction_labels = labels_json.clone();
            trade.leg_index = trades.len() as u32;
            trade.received_at = raw_tx.received_at;
            trade.venue = "amm".to_string();

            trades.push(trade);
        }

        if trades.is_empty() {
            None
        } else {
            Some((raw_tx, trades))
        }
    }
}

// ---------------------------------------------------------------------------
// Instruction decoders
// ---------------------------------------------------------------------------

impl HeliusDecoder {
    /// Decode a `Create` or `Create_v2` instruction into `TokenCreated` +
    /// `CreatorActivityDetected` events.
    ///
    /// Creator wallet prefers on-chain `CreateEvent.creator`, then the instruction
    /// arg pubkey, then the user signer account (`create`: idx 7, `create_v2`: idx 5).
    fn decode_create(
        &self,
        signature: &str,
        slot: u64,
        block_time: DateTime<Utc>,
        pump_ix: &Value,
        pump_accounts: &[String],
        account_keys: &[String],
        decoded_events: &[DecodedTradeEvent],
        decoded_create_events: &[DecodedCreateEvent],
        instruction_type: &str,
        labels_json: &Value,
        cu_limit: Option<u64>,
        cu_price: Option<u64>,
        raw_tx: &RawTransaction,
        message: &Value,
        meta: &Value,
    ) -> Vec<InternalEvent> {
        let mint = match pump_accounts.first().filter(|s| !s.is_empty()) {
            Some(m) => m.clone(),
            None => {
                warn!("Create: missing mint at accounts[0] for tx {signature}");
                return vec![];
            }
        };

        let ix_info = decode_create_info(pump_ix);
        let create_log = decoded_create_events.iter().find(|e| e.mint == mint);
        let is_v2 = ix_info.as_ref().map(|i| i.is_v2).unwrap_or(false)
            || create_log
                .and_then(|e| e.token_program.as_deref())
                .is_some_and(|p| p == TOKEN_2022_PROGRAM_ID);

        let creator = resolve_creator_wallet(
            ix_info.as_ref(),
            create_log,
            pump_accounts,
            is_v2,
            account_keys,
        );
        if creator.is_empty() {
            warn!("Create: cannot determine creator for tx {signature}");
            return vec![];
        }

        let buy_user = create_log
            .map(|e| e.user.clone())
            .or_else(|| pump_user_account(pump_accounts, is_v2))
            .or_else(|| account_keys.first().cloned())
            .unwrap_or_else(|| creator.clone());

        let initial_create_event = decoded_events
            .iter()
            .find(|ev| ev.is_buy && ev.user == buy_user && ev.mint == mint);

        let initial_supply = initial_create_event.map(|ev| ev.token_amount as u64);
        let initial_buy_sol = initial_create_event.map(|ev| ev.sol_amount);

        let name = create_log
            .map(|e| e.name.clone())
            .or_else(|| ix_info.as_ref().map(|i| i.name.clone()))
            .unwrap_or_else(|| "Unknown".to_string());
        let symbol = create_log
            .map(|e| e.symbol.clone())
            .or_else(|| ix_info.as_ref().map(|i| i.symbol.clone()))
            .unwrap_or_else(|| "UNKNOWN".to_string());
        let is_mayhem_mode = create_log
            .map(|e| e.is_mayhem_mode)
            .or_else(|| ix_info.as_ref().map(|i| i.is_mayhem_mode))
            .unwrap_or(false);
        let is_cashback_enabled = create_log
            .map(|e| e.is_cashback_enabled)
            .or_else(|| ix_info.as_ref().map(|i| i.is_cashback_enabled))
            .unwrap_or(false);
        let create_kind = if is_v2 { "Create_v2" } else { "Create" };
        let bonding_curve = create_log
            .and_then(|e| e.bonding_curve.clone())
            .or_else(|| pump_accounts.get(2).cloned());
        let initial_buy_instruction =
            extract_pump_buy_instruction_data(message, meta, account_keys)
                .map(buy_instruction_data_to_json);

        debug!(
            sig = %signature,
            mint = %mint,
            creator = %creator,
            buy_user = %buy_user,
            name = %name,
            symbol = %symbol,
            create_kind = %create_kind,
            is_mayhem_mode = %is_mayhem_mode,
            is_cashback_enabled = %is_cashback_enabled,
            create_event = create_log.is_some(),
            instruction_type = %instruction_type,
            initial_supply = ?initial_supply,
            labels_json = ?labels_json,
            "Token created"
        );

        let token_program_id = create_log
            .and_then(|e| e.token_program.clone())
            .or_else(|| {
                if is_v2 {
                    Some(TOKEN_2022_PROGRAM_ID.to_string())
                } else {
                    Some(TOKEN_PROGRAM_ID.to_string())
                }
            });

        let token = Token::new(
            mint,
            creator.clone(),
            name,
            symbol,
            token_program_id,
            bonding_curve,
            initial_supply,
            initial_buy_sol,
            initial_buy_instruction,
            cu_limit,
            cu_price,
            is_mayhem_mode,
            is_cashback_enabled,
            labels_json.clone(),
            signature.to_string(),
            block_time,
        );

        let creator_event = CreatorActivityEvent {
            creator_wallet: creator,
            mint_address: token.mint_address.clone(),
            activity_kind: CreatorActivityKind::Create,
            tx_signature: signature.to_string(),
            slot,
            timestamp: block_time,
            raw_tx: raw_tx.clone(),
        };

        vec![
            InternalEvent::TokenCreated(TokenCreatedEvent {
                token,
                tx_signature: signature.to_string(),
                slot,
                timestamp: block_time,
                raw_tx: raw_tx.clone(),
            }),
            InternalEvent::CreatorActivityDetected(creator_event),
        ]
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

    /// Balance-delta fallback for Buy/Sell when no "Program data:" TradeEvent
    /// was found (unusual, but guards against edge cases).
    fn decode_trade_from_balances(
        &self,
        kind: InstructionKind,
        signature: &str,
        slot: u64,
        block_time: DateTime<Utc>,
        pump_accounts: &[String],
        account_keys: &[String],
        pre_balances: &[u64],
        post_balances: &[u64],
        meta: &Value,
        labels_json: &Value,
        raw_tx: &RawTransaction,
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

// ---------------------------------------------------------------------------
// Step 1a — decode TradeEvent from "Program data:" log lines (base64 + Borsh)
// ---------------------------------------------------------------------------

/// Borsh layout of the pump.fun on-chain TradeEvent (emitted via `emit!`).
/// Discriminator (8 bytes) is stripped before passing to `deserialize`.
#[derive(BorshDeserialize)]
struct RawTradeEvent {
    mint: [u8; 32],
    sol_amount: u64,
    token_amount: u64,
    is_buy: bool,
    user: [u8; 32],
    #[allow(dead_code)]
    timestamp: i64,
    virtual_sol_reserves: u64,
    virtual_token_reserves: u64,
    real_sol_reserves: u64,
    real_token_reserves: u64,
}

struct DecodedTradeEvent {
    mint: String,
    sol_amount: f64,
    token_amount: f64,
    is_buy: bool,
    user: String,
    virtual_sol_reserves: f64,
    virtual_token_reserves: f64,
    real_sol_reserves: f64,
    real_token_reserves: f64,
}

/// Scan every "Program data: <base64>" log line, try to decode each as a
/// TradeEvent, and return all successfully decoded events in log order.
fn decode_trade_events_from_logs(logs: &[&str]) -> Vec<DecodedTradeEvent> {
    let mut events = Vec::new();

    for log in logs {
        let Some(encoded) = log.strip_prefix("Program data: ") else {
            continue;
        };

        let bytes = match STANDARD.decode(encoded) {
            Ok(b) => b,
            Err(_) => continue,
        };

        if bytes.len() < 8 || bytes[..8] != TRADE_EVENT_DISCRIMINATOR {
            continue;
        }

        let mut buf: &[u8] = &bytes[8..];
        let raw = match RawTradeEvent::deserialize(&mut buf) {
            Ok(r) => r,
            Err(e) => {
                warn!("Failed to Borsh-decode TradeEvent: {e}");
                continue;
            }
        };

        events.push(DecodedTradeEvent {
            mint: bs58::encode(raw.mint).into_string(),
            sol_amount: raw.sol_amount as f64 / 1_000_000_000.0,
            token_amount: raw.token_amount as f64,
            is_buy: raw.is_buy,
            user: bs58::encode(raw.user).into_string(),
            virtual_sol_reserves: raw.virtual_sol_reserves as f64 / 1_000_000_000.0,
            virtual_token_reserves: raw.virtual_token_reserves as f64,
            real_sol_reserves: raw.real_sol_reserves as f64 / 1_000_000_000.0,
            real_token_reserves: raw.real_token_reserves as f64,
        });
    }

    events
}

// ---------------------------------------------------------------------------
// Step 1a (cont.) — decode PumpSwap BuyEvent / SellEvent from "Program data:"
// ---------------------------------------------------------------------------

/// Leading fields of the PumpSwap `BuyEvent` (Borsh). Trailing fields added in
/// later program versions (e.g. `coin_creator`) are left unread by `deserialize`.
#[derive(BorshDeserialize)]
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
    #[allow(dead_code)]
    quote_amount_in_with_lp_fee: u64,
    /// Total SOL the user spent, including LP + protocol fees.
    user_quote_amount_in: u64,
    pool: [u8; 32],
    user: [u8; 32],
}

/// Leading fields of the PumpSwap `SellEvent` (Borsh). See `RawPumpSwapBuyEvent`.
#[derive(BorshDeserialize)]
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
    #[allow(dead_code)]
    quote_amount_out_without_lp_fee: u64,
    /// Total SOL the user received, net of LP + protocol fees.
    user_quote_amount_out: u64,
    pool: [u8; 32],
    user: [u8; 32],
}

/// A decoded PumpSwap swap. `base_amount` is raw token units (matching the
/// bonding-curve `token_amount` convention); `quote_amount` and reserve fields
/// are in SOL (lamports / 1e9).
struct DecodedAmmTrade {
    is_buy: bool,
    base_amount: f64,
    quote_amount: f64,
    pool: String,
    user: String,
    pool_base_reserves: f64,
    pool_quote_reserves: f64,
}

/// Scan "Program data:" log lines for PumpSwap Buy/Sell events, in log order.
fn decode_pump_swap_trades_from_logs(logs: &[&str]) -> Vec<DecodedAmmTrade> {
    let mut out = Vec::new();
    let lamports = LAMPORTS_PER_SOL as f64;

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

        if disc == PUMP_SWAP_BUY_EVENT_DISCRIMINATOR {
            match RawPumpSwapBuyEvent::deserialize(&mut buf) {
                Ok(e) => out.push(DecodedAmmTrade {
                    is_buy: true,
                    base_amount: e.base_amount_out as f64,
                    quote_amount: e.user_quote_amount_in as f64 / lamports,
                    pool: bs58::encode(e.pool).into_string(),
                    user: bs58::encode(e.user).into_string(),
                    pool_base_reserves: e.pool_base_token_reserves as f64,
                    pool_quote_reserves: e.pool_quote_token_reserves as f64 / lamports,
                }),
                Err(e) => warn!("Failed to Borsh-decode PumpSwap BuyEvent: {e}"),
            }
        } else if disc == PUMP_SWAP_SELL_EVENT_DISCRIMINATOR {
            match RawPumpSwapSellEvent::deserialize(&mut buf) {
                Ok(e) => out.push(DecodedAmmTrade {
                    is_buy: false,
                    base_amount: e.base_amount_in as f64,
                    quote_amount: e.user_quote_amount_out as f64 / lamports,
                    pool: bs58::encode(e.pool).into_string(),
                    user: bs58::encode(e.user).into_string(),
                    pool_base_reserves: e.pool_base_token_reserves as f64,
                    pool_quote_reserves: e.pool_quote_token_reserves as f64 / lamports,
                }),
                Err(e) => warn!("Failed to Borsh-decode PumpSwap SellEvent: {e}"),
            }
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Step 1a (cont.) — decode CreateEvent from "Program data:" log lines
// ---------------------------------------------------------------------------

/// Borsh layout of the pump.fun on-chain CreateEvent (emitted via `emit!`).
#[derive(BorshDeserialize)]
struct RawCreateEvent {
    name: String,
    symbol: String,
    #[allow(dead_code)]
    uri: String,
    mint: [u8; 32],
    bonding_curve: [u8; 32],
    user: [u8; 32],
    creator: [u8; 32],
    #[allow(dead_code)]
    timestamp: i64,
    #[allow(dead_code)]
    virtual_token_reserves: u64,
    #[allow(dead_code)]
    virtual_sol_reserves: u64,
    #[allow(dead_code)]
    real_token_reserves: u64,
    #[allow(dead_code)]
    token_total_supply: u64,
    token_program: [u8; 32],
    is_mayhem_mode: bool,
    is_cashback_enabled: bool,
    #[allow(dead_code)]
    quote_mint: [u8; 32],
    #[allow(dead_code)]
    virtual_quote_reserves: u64,
}

struct DecodedCreateEvent {
    name: String,
    symbol: String,
    mint: String,
    bonding_curve: Option<String>,
    user: String,
    creator: String,
    token_program: Option<String>,
    is_mayhem_mode: bool,
    is_cashback_enabled: bool,
}

fn decode_create_events_from_logs(logs: &[&str]) -> Vec<DecodedCreateEvent> {
    let mut events = Vec::new();

    for log in logs {
        let Some(encoded) = log.strip_prefix("Program data: ") else {
            continue;
        };

        let bytes = match STANDARD.decode(encoded) {
            Ok(b) => b,
            Err(_) => continue,
        };

        if bytes.len() < 8 || bytes[..8] != CREATE_EVENT_DISCRIMINATOR {
            continue;
        }

        let mut buf: &[u8] = &bytes[8..];
        let raw = match RawCreateEvent::deserialize(&mut buf) {
            Ok(r) => r,
            Err(e) => {
                warn!("Failed to Borsh-decode CreateEvent: {e}");
                continue;
            }
        };

        events.push(DecodedCreateEvent {
            name: raw.name,
            symbol: raw.symbol,
            mint: bs58::encode(raw.mint).into_string(),
            bonding_curve: Some(bs58::encode(raw.bonding_curve).into_string()),
            user: bs58::encode(raw.user).into_string(),
            creator: bs58::encode(raw.creator).into_string(),
            token_program: Some(bs58::encode(raw.token_program).into_string()),
            is_mayhem_mode: raw.is_mayhem_mode,
            is_cashback_enabled: raw.is_cashback_enabled,
        });
    }

    events
}

// ---------------------------------------------------------------------------
// Step 1b — identify instruction kinds from log lines
// ---------------------------------------------------------------------------

/// Simplified instruction classifier.
/// `create` (SPL Token) and `create_v2` (Token-2022) are distinct instructions
/// with different account layouts; both map to `InstructionKind::Create`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstructionKind {
    Create,
    Buy,
    Sell,
    Migrate,
    Unknown,
}

/// Scan "Program log: Instruction: <Name>" entries and map each to a big-class
/// InstructionKind. All buy/sell variants (BuyExactSolIn, BuyExactQuoteInV2, etc.)
/// are collapsed into Buy / Sell.
fn collect_instruction_kinds(
    message: &Value,
    meta: &Value,
    account_keys: &[String],
) -> Vec<InstructionKind> {
    let mut kinds = Vec::new();

    let mut push_kind = |ix: &Value| {
        let program_id = resolve_instruction_program_id(ix, account_keys);
        let data_bytes = instruction_data_bytes(ix);

        if program_id != PUMP_FUN_PROGRAM_ID {
            return;
        }

        if let Some(bytes) = data_bytes.as_deref().filter(|b| b.len() >= 8) {
            let kind = if bytes.starts_with(&BUY_DISCRIMINATOR)
                || bytes.starts_with(&BUY_EXACT_SOL_IN_DISCRIMINATOR)
                || bytes.starts_with(&BUY_EXACT_QUOTE_IN_DISCRIMINATOR)
                || bytes.starts_with(&BUY_V2_DISCRIMINATOR)
                || bytes.starts_with(&BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR)
            {
                InstructionKind::Buy
            } else if bytes.starts_with(&SELL_DISCRIMINATOR) {
                InstructionKind::Sell
            } else if bytes.starts_with(&CREATE_INSTRUCTION_DISCRIMINATOR)
                || bytes.starts_with(&CREATE_V2_INSTRUCTION_DISCRIMINATOR)
            {
                InstructionKind::Create
            } else if bytes.starts_with(&MIGRATE_INSTRUCTION_DISCRIMINATOR)
                || bytes.starts_with(&MIGRATE_V2_INSTRUCTION_DISCRIMINATOR)
            {
                InstructionKind::Migrate
            } else {
                InstructionKind::Unknown
            };
            kinds.push(kind);
        }
    };

    if let Some(instructions) = message["instructions"].as_array() {
        for ix in instructions {
            push_kind(ix);
        }
    }

    if let Some(groups) = meta["innerInstructions"].as_array() {
        for group in groups {
            if let Some(instructions) = group["instructions"].as_array() {
                for ix in instructions {
                    push_kind(ix);
                }
            }
        }
    }

    kinds
}

fn determine_instruction_type(
    kinds: &[InstructionKind],
    decoded_events: &[DecodedTradeEvent],
) -> String {
    let has_buy = kinds.iter().any(|k| *k == InstructionKind::Buy);
    let has_sell = kinds.iter().any(|k| *k == InstructionKind::Sell);

    if has_buy && has_sell {
        let (buy_sol, sell_sol) = decoded_events.iter().fold((0.0, 0.0), |(buy, sell), ev| {
            if ev.is_buy {
                (buy + ev.sol_amount, sell)
            } else {
                (buy, sell + ev.sol_amount)
            }
        });
        if buy_sol > sell_sol {
            return "Buy".to_string();
        }
        if sell_sol > buy_sol {
            return "Sell".to_string();
        }
        let buy_count = kinds.iter().filter(|k| **k == InstructionKind::Buy).count();
        let sell_count = kinds
            .iter()
            .filter(|k| **k == InstructionKind::Sell)
            .count();
        return if buy_count >= sell_count {
            "Buy".to_string()
        } else {
            "Sell".to_string()
        };
    }

    if has_buy {
        return "Buy".to_string();
    }
    if has_sell {
        return "Sell".to_string();
    }
    if kinds.iter().any(|k| *k == InstructionKind::Create) {
        return "Create".to_string();
    }
    if kinds.iter().any(|k| *k == InstructionKind::Migrate) {
        return "Migrate".to_string();
    }

    "Unknown".to_string()
}

fn resolve_instruction_program_id(ix: &Value, account_keys: &[String]) -> String {
    ix["programId"]
        .as_str()
        .map(|s| s.to_owned())
        .or_else(|| {
            ix["programIdIndex"]
                .as_u64()
                .and_then(|i| account_keys.get(i as usize))
                .cloned()
        })
        .unwrap_or_default()
}

fn instruction_data_bytes(ix: &Value) -> Option<Vec<u8>> {
    ix["data"]
        .as_str()
        .and_then(|s| bs58::decode(s).into_vec().ok())
}

// ---------------------------------------------------------------------------
// Step 2 — build instruction-order labels from message.instructions
// ---------------------------------------------------------------------------

/// Borsh-deserialised Compute Budget instruction used to extract cu_limit /
/// cu_price from raw instruction bytes.
#[derive(BorshDeserialize)]
#[allow(dead_code)]
enum ComputeBudgetIx {
    Unused,                              // 0 - deprecated
    RequestHeapFrame(u32),               // 1
    SetComputeUnitLimit(u32),            // 2
    SetComputeUnitPrice(u64),            // 3 - micro-lamports per CU
    SetLoadedAccountsDataSizeLimit(u32), // 4
}

/// Iterate `message.instructions` (top-level only) and build a human-readable
/// label for each entry.  Also extracts `cu_limit` and `cu_price` in the same
/// pass.

/// Returns `(labels, cu_limit_opt, cu_price_opt)`.
fn build_instruction_labels(
    message: &Value,
    account_keys: &[String],
) -> (Vec<String>, Option<u64>, Option<u64>) {
    let instructions = match message["instructions"].as_array() {
        Some(arr) => arr,
        None => return (vec![], None, None),
    };

    let mut cu_limit: Option<u64> = None;
    let mut cu_price: Option<u64> = None;

    let labels = instructions
        .iter()
        .map(|ix| {
            let program_id: String = resolve_instruction_program_id(ix, account_keys);

            // Decode raw instruction bytes (base58 "data" field).
            // jsonParsed instructions use "parsed" instead; no "data" field.
            let data_bytes = instruction_data_bytes(ix);

            // Extract compute-budget values while labelling.
            if program_id == COMPUTE_BUDGET_PROGRAM_ID {
                if let Some(ref b) = data_bytes {
                    let mut buf: &[u8] = b;
                    match ComputeBudgetIx::deserialize(&mut buf) {
                        Ok(ComputeBudgetIx::SetComputeUnitLimit(units)) => {
                            cu_limit = Some(units as u64);
                        }
                        Ok(ComputeBudgetIx::SetComputeUnitPrice(price)) => {
                            cu_price = Some(price);
                        }
                        _ => {}
                    }
                }
            }

            label_instruction(&program_id, ix, data_bytes.as_deref())
        })
        .collect();

    (labels, cu_limit, cu_price)
}

/// Produce a single human-readable label for one instruction.
fn label_instruction(program_id: &str, ix: &Value, data_bytes: Option<&[u8]>) -> String {
    // ── Compute Budget ────────────────────────────────────────────────────────
    if program_id == COMPUTE_BUDGET_PROGRAM_ID {
        if let Some(bytes) = data_bytes {
            match bytes.first() {
                Some(&1) => return "Compute Budget: RequestHeapFrame".to_owned(),
                Some(&2) => return "Compute Budget: SetComputeUnitLimit".to_owned(),
                Some(&3) => return "Compute Budget: SetComputeUnitPrice".to_owned(),
                Some(&4) => return "Compute Budget: SetLoadedAccountsDataSizeLimit".to_owned(),
                _ => {}
            }
        }
        if let Some(t) = ix.pointer("/parsed/type").and_then(Value::as_str) {
            return format!("Compute Budget: {}", capitalize_first(t));
        }
        return "Compute Budget: Unknown".to_owned();
    }

    // ── Pump.fun — match 8-byte Anchor discriminator ──────────────────────────
    if program_id == PUMP_FUN_PROGRAM_ID {
        if let Some(b) = data_bytes.filter(|b| b.len() >= 8) {
            let d = &b[..8];

            // Trading instructions
            if d == BUY_DISCRIMINATOR {
                return "Pump.Fun: Buy".to_owned();
            }
            if d == BUY_EXACT_SOL_IN_DISCRIMINATOR {
                return "Pump.Fun: BuyExactSolIn".to_owned();
            }
            if d == BUY_EXACT_QUOTE_IN_DISCRIMINATOR {
                return "Pump.Fun: BuyExactQuoteIn".to_owned();
            }
            if d == BUY_V2_DISCRIMINATOR {
                return "Pump.Fun: BuyV2".to_owned();
            }
            if d == BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR {
                return "Pump.Fun: BuyExactQuoteInV2".to_owned();
            }
            if d == SELL_DISCRIMINATOR {
                return "Pump.Fun: Sell".to_owned();
            }
            if d == CREATE_INSTRUCTION_DISCRIMINATOR {
                return "Pump.Fun: Create".to_owned();
            }
            if d == CREATE_V2_INSTRUCTION_DISCRIMINATOR {
                return "Pump.Fun: Create_v2".to_owned();
            }
            if d == MIGRATE_INSTRUCTION_DISCRIMINATOR {
                return "Pump.Fun: Migrate".to_owned();
            }
        }
        return "Pump.Fun: Unknown".to_owned();
    }

    // ── System / Token programs — use Helius-parsed "type" ───────────────────
    let use_parsed = [
        SYSTEM_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
        TOKEN_2022_PROGRAM_ID,
        ASSOCIATED_TOKEN_PROGRAM_ID,
    ];
    if use_parsed.contains(&program_id) {
        let friendly = program_friendly_name(program_id).unwrap_or("Unknown Program");
        if let Some(t) = ix.pointer("/parsed/type").and_then(Value::as_str) {
            return format!("{}: {}", friendly, capitalize_first(t));
        }
        return format!("{}: Unknown", friendly);
    }

    // ── All other known programs ──────────────────────────────────────────────
    if let Some(name) = program_friendly_name(program_id) {
        return format!("{}: Unknown", name);
    }

    // ── Completely unknown — show last 8 chars of address ────────────────────
    let suffix = if program_id.len() >= 8 {
        &program_id[program_id.len() - 8..]
    } else {
        program_id
    };
    format!("Unknown (...{})", suffix)
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

#[derive(Debug)]
enum PumpBuyInstructionData {
    Buy {
        token_amount: u64,
        max_sol_cost: u64,
    },
    BuyExactSolIn {
        spendable_sol_in: u64,
        min_tokens_out: u64,
    },
    BuyExactQuoteIn {
        spendable_sol_in: u64,
        min_tokens_out: u64,
    },
    BuyV2 {
        token_amount: u64,
        max_sol_cost: u64,
    },
    BuyExactQuoteInV2 {
        spendable_sol_in: u64,
        min_tokens_out: u64,
    },
}

#[derive(BorshDeserialize)]
struct BuyArgs {
    token_amount: u64,
    max_sol_cost: u64,
}

#[derive(BorshDeserialize)]
struct BuyExactArgs {
    spendable_sol_in: u64,
    min_tokens_out: u64,
}

fn parse_pump_buy_instruction_data(data: &[u8]) -> Option<PumpBuyInstructionData> {
    if data.len() < 8 {
        return None;
    }

    let (discriminator, rest) = data.split_at(8);
    let mut buf = rest;

    if discriminator == BUY_DISCRIMINATOR {
        let args = BuyArgs::deserialize(&mut buf).ok()?;
        return Some(PumpBuyInstructionData::Buy {
            token_amount: args.token_amount,
            max_sol_cost: args.max_sol_cost,
        });
    }
    if discriminator == BUY_V2_DISCRIMINATOR {
        let args = BuyArgs::deserialize(&mut buf).ok()?;
        return Some(PumpBuyInstructionData::BuyV2 {
            token_amount: args.token_amount,
            max_sol_cost: args.max_sol_cost,
        });
    }
    if discriminator == BUY_EXACT_SOL_IN_DISCRIMINATOR {
        let args = BuyExactArgs::deserialize(&mut buf).ok()?;
        return Some(PumpBuyInstructionData::BuyExactSolIn {
            spendable_sol_in: args.spendable_sol_in,
            min_tokens_out: args.min_tokens_out,
        });
    }
    if discriminator == BUY_EXACT_QUOTE_IN_DISCRIMINATOR {
        let args = BuyExactArgs::deserialize(&mut buf).ok()?;
        return Some(PumpBuyInstructionData::BuyExactQuoteIn {
            spendable_sol_in: args.spendable_sol_in,
            min_tokens_out: args.min_tokens_out,
        });
    }
    if discriminator == BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR {
        let args = BuyExactArgs::deserialize(&mut buf).ok()?;
        return Some(PumpBuyInstructionData::BuyExactQuoteInV2 {
            spendable_sol_in: args.spendable_sol_in,
            min_tokens_out: args.min_tokens_out,
        });
    }

    None
}

fn buy_instruction_data_to_json(data: PumpBuyInstructionData) -> Value {
    match data {
        PumpBuyInstructionData::Buy {
            token_amount,
            max_sol_cost,
        } => json!({
            "kind": "buy",
            "token_amount": token_amount,
            "max_sol_cost": max_sol_cost,
        }),
        PumpBuyInstructionData::BuyV2 {
            token_amount,
            max_sol_cost,
        } => json!({
            "kind": "buy_v2",
            "token_amount": token_amount,
            "max_sol_cost": max_sol_cost,
        }),
        PumpBuyInstructionData::BuyExactSolIn {
            spendable_sol_in,
            min_tokens_out,
        } => json!({
            "kind": "buy_exact_sol_in",
            "spendable_sol_in": spendable_sol_in,
            "min_tokens_out": min_tokens_out,
        }),
        PumpBuyInstructionData::BuyExactQuoteIn {
            spendable_sol_in,
            min_tokens_out,
        } => json!({
            "kind": "buy_exact_quote_in",
            "spendable_sol_in": spendable_sol_in,
            "min_tokens_out": min_tokens_out,
        }),
        PumpBuyInstructionData::BuyExactQuoteInV2 {
            spendable_sol_in,
            min_tokens_out,
        } => json!({
            "kind": "buy_exact_quote_in_v2",
            "spendable_sol_in": spendable_sol_in,
            "min_tokens_out": min_tokens_out,
        }),
    }
}

fn extract_pump_buy_instruction_data(
    message: &Value,
    meta: &Value,
    account_keys: &[String],
) -> Option<PumpBuyInstructionData> {
    let ixs = find_pump_ixs_anywhere(message, meta, account_keys, PUMP_FUN_PROGRAM_ID);
    for ix in ixs {
        if let Some(bytes) = instruction_data_bytes(ix).as_deref() {
            if let Some(data) = parse_pump_buy_instruction_data(bytes) {
                return Some(data);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// JSON extraction helpers
// ---------------------------------------------------------------------------

fn extract_logs<'a>(meta: &'a Value) -> Vec<&'a str> {
    meta["logMessages"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default()
}

/// With `jsonParsed` encoding, accountKeys is:
///   [{pubkey: "...", signer: bool, writable: bool}, ...]
/// Fall back to plain string array for other encodings.
fn extract_account_keys(message: &Value) -> Vec<String> {
    message["accountKeys"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    v["pubkey"]
                        .as_str()
                        .or_else(|| v.as_str())
                        .map(|s| s.to_string())
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Find ALL instructions that belong to `program_id`, searching both the
/// top-level `message.instructions` list and every entry in
/// `meta.innerInstructions[*].instructions`.

/// This is needed because trading bots (Terminal, Axiom, …) wrap pump.fun
/// calls as CPI — the pump.fun instruction appears only as an inner call.
fn find_pump_ixs_anywhere<'a>(
    message: &'a Value,
    meta: &'a Value,
    account_keys: &[String],
    program_id: &str,
) -> Vec<&'a Value> {
    let mut result = Vec::new();

    if let Some(outer) = message["instructions"].as_array() {
        for ix in outer {
            if resolve_instruction_program_id(ix, account_keys) == program_id {
                result.push(ix);
            }
        }
    }

    if let Some(groups) = meta["innerInstructions"].as_array() {
        for group in groups {
            if let Some(inner) = group["instructions"].as_array() {
                for ix in inner {
                    if resolve_instruction_program_id(ix, account_keys) == program_id {
                        result.push(ix);
                    }
                }
            }
        }
    }

    result
}

/// Return true if the instruction's data starts with a known pump.fun Create
/// discriminator (v1 or v2).
fn is_pump_create_ix(ix: &Value) -> bool {
    let data_b58 = match ix["data"].as_str() {
        Some(s) => s,
        None => return false,
    };
    let data = match bs58::decode(data_b58).into_vec() {
        Ok(d) => d,
        Err(_) => return false,
    };
    data.len() >= 8
        && (data[..8] == CREATE_INSTRUCTION_DISCRIMINATOR
            || data[..8] == CREATE_V2_INSTRUCTION_DISCRIMINATOR)
}

/// Resolve account pubkeys from a pump instruction.

/// Helius with `jsonParsed` encoding may return accounts as either:
///   - Resolved pubkey strings: `["pk1", "pk2", ...]`
///   - Integer indices into `message.accountKeys`: `[0, 1, 2, ...]`
/// Both are handled; unresolvable entries are silently skipped.
fn resolve_pump_accounts(ix: &Value, account_keys: &[String]) -> Vec<String> {
    ix["accounts"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    v.as_str().map(|s| s.to_string()).or_else(|| {
                        v.as_u64()
                            .and_then(|i| account_keys.get(i as usize))
                            .cloned()
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn extract_balances(balances: &Value) -> Vec<u64> {
    balances
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Amount computation
// ---------------------------------------------------------------------------

/// Compute the absolute SOL change (in SOL, not lamports) for `wallet`
/// using the flat pre/post balance arrays (indexed by accountKeys order).
fn compute_sol_change(wallet: &str, account_keys: &[String], pre: &[u64], post: &[u64]) -> f64 {
    account_keys
        .iter()
        .position(|k| k == wallet)
        .map(|idx| {
            let pre_bal = pre.get(idx).copied().unwrap_or(0);
            let post_bal = post.get(idx).copied().unwrap_or(0);
            (pre_bal as f64 - post_bal as f64).abs() / 1_000_000_000.0
        })
        .unwrap_or(0.0)
}

/// Compute the absolute token amount change for `user_ata` (the trader's
/// Associated Token Account) using pre/post token balance entries.
/// Token balance entries carry an `accountIndex` that maps into `account_keys`.
fn compute_token_change(user_ata: &str, mint: &str, account_keys: &[String], meta: &Value) -> f64 {
    let ata_idx = match account_keys.iter().position(|k| k == user_ata) {
        Some(i) => i as u64,
        None => return 0.0,
    };

    let find_amount = |balances: &Value| -> f64 {
        balances
            .as_array()
            .and_then(|arr| {
                arr.iter().find(|entry| {
                    entry["accountIndex"].as_u64() == Some(ata_idx)
                        && entry["mint"].as_str() == Some(mint)
                })
            })
            .and_then(|entry| entry["uiTokenAmount"]["uiAmount"].as_f64())
            .unwrap_or(0.0)
    };

    let pre = find_amount(&meta["preTokenBalances"]);
    let post = find_amount(&meta["postTokenBalances"]);
    (post - pre).abs()
}

// ---------------------------------------------------------------------------
// Anchor string decoding (for Create instruction name/symbol/uri)
// ---------------------------------------------------------------------------

struct CreateInstructionInfo {
    name: String,
    symbol: String,
    creator: String,
    is_v2: bool,
    is_mayhem_mode: bool,
    is_cashback_enabled: bool,
}

/// User signer account index differs between create (v1) and create_v2.
fn pump_user_account(pump_accounts: &[String], is_v2: bool) -> Option<String> {
    let idx = if is_v2 { 5 } else { 7 };
    pump_accounts.get(idx).filter(|s| !s.is_empty()).cloned()
}

/// Resolve creator wallet: CreateEvent > ix arg > user signer > fee payer.
fn resolve_creator_wallet(
    ix_info: Option<&CreateInstructionInfo>,
    create_log: Option<&DecodedCreateEvent>,
    pump_accounts: &[String],
    is_v2: bool,
    account_keys: &[String],
) -> String {
    if let Some(ev) = create_log {
        return ev.creator.clone();
    }
    if let Some(ix) = ix_info {
        if !ix.creator.is_empty() {
            return ix.creator.clone();
        }
    }
    if let Some(user) = pump_user_account(pump_accounts, is_v2) {
        return user;
    }
    account_keys
        .first()
        .filter(|s| !s.is_empty())
        .cloned()
        .unwrap_or_default()
}

/// Attempt to decode create instruction args from base58-encoded data.

/// Anchor / Borsh layout (after the 8-byte discriminator):
///   name:                  u32 LE length + UTF-8 bytes
///   symbol:                u32 LE length + UTF-8 bytes
///   uri:                   u32 LE length + UTF-8 bytes
///   creator:               32-byte pubkey (both create and create_v2)
///   is_mayhem_mode:        bool (create_v2 only)
///   is_cashback_enabled:   OptionBool = 1 byte bool (create_v2 only)
fn decode_create_info(pump_ix: &Value) -> Option<CreateInstructionInfo> {
    let data_b58 = pump_ix["data"].as_str()?;
    let data = bs58::decode(data_b58).into_vec().ok()?;

    // Must have at least discriminator (8) + two minimal strings (4 bytes each)
    if data.len() < 16 {
        return None;
    }

    let discriminator = &data[..8];
    let is_create_v2 = discriminator == CREATE_V2_INSTRUCTION_DISCRIMINATOR;
    if discriminator != CREATE_INSTRUCTION_DISCRIMINATOR && !is_create_v2 {
        return None;
    }

    let mut offset = 8; // skip discriminator
    let name = read_anchor_string(&data, &mut offset)?;
    let symbol = read_anchor_string(&data, &mut offset)?;
    let _ = read_anchor_string(&data, &mut offset);

    let creator = read_pubkey(&data, &mut offset)
        .map(|pk| bs58::encode(pk).into_string())?;

    let (is_mayhem_mode, is_cashback_enabled) = if is_create_v2 {
        let mayhem = data.get(offset).copied().unwrap_or(0) == 1;
        offset += 1;
        let cashback = data.get(offset).copied().unwrap_or(0) == 1;
        (mayhem, cashback)
    } else {
        (false, false)
    };

    Some(CreateInstructionInfo {
        name,
        symbol,
        creator,
        is_v2: is_create_v2,
        is_mayhem_mode,
        is_cashback_enabled,
    })
}

fn read_pubkey(data: &[u8], offset: &mut usize) -> Option<[u8; 32]> {
    let pk: [u8; 32] = data.get(*offset..*offset + 32)?.try_into().ok()?;
    *offset += 32;
    Some(pk)
}

/// Read a Borsh-encoded `String` (u32 LE length prefix + UTF-8 bytes).
fn read_anchor_string(data: &[u8], offset: &mut usize) -> Option<String> {
    let len_bytes: [u8; 4] = data.get(*offset..*offset + 4)?.try_into().ok()?;
    let len = u32::from_le_bytes(len_bytes) as usize;
    *offset += 4;

    let s = std::str::from_utf8(data.get(*offset..*offset + len)?)
        .ok()?
        .to_string();
    *offset += len;

    Some(s)
}
