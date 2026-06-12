//! Token-creation decoding: the `Create`/`Create_v2` instruction, the on-chain
//! `CreateEvent` log, creator-wallet resolution, Anchor string/pubkey reading,
//! and parsing of the initial-buy instruction args attached to a new token.

use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD, Engine};
use borsh::BorshDeserialize;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use tracing::{debug, warn};

use crate::config::constants::{
    BUY_DISCRIMINATOR, BUY_EXACT_QUOTE_IN_DISCRIMINATOR, BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR,
    BUY_EXACT_SOL_IN_DISCRIMINATOR, BUY_V2_DISCRIMINATOR, CREATE_EVENT_DISCRIMINATOR,
    CREATE_INSTRUCTION_DISCRIMINATOR, CREATE_V2_INSTRUCTION_DISCRIMINATOR, PUMP_FUN_PROGRAM_ID,
    TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID,
};
use crate::models::{
    events::{CreatorActivityEvent, CreatorActivityKind, InternalEvent, TokenCreatedEvent},
    token::Token,
    transaction::RawTransaction,
};

use super::instructions::instruction_data_bytes;
use super::parse::find_pump_ixs_anywhere;
use super::trade::DecodedTradeEvent;
use super::HeliusDecoder;

impl HeliusDecoder {
    /// Decode a `Create` or `Create_v2` instruction into `TokenCreated` +
    /// `CreatorActivityDetected` events.
    ///
    /// Creator wallet prefers on-chain `CreateEvent.creator`, then the instruction
    /// arg pubkey, then the user signer account (`create`: idx 7, `create_v2`: idx 5).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn decode_create(
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
        raw_tx: &Arc<RawTransaction>,
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

pub(super) struct DecodedCreateEvent {
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

pub(super) fn decode_create_events_from_logs(logs: &[&str]) -> Vec<DecodedCreateEvent> {
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
// Initial-buy instruction args (attached to a newly created token)
// ---------------------------------------------------------------------------

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
