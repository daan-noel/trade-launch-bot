//! Token-creation decoding: Create / Create_v2 instruction + CreateEvent log.

use base64::{engine::general_purpose::STANDARD, Engine};
use borsh::BorshDeserialize;
use chrono::{DateTime, Utc};
use tracing::{debug, warn};

use crate::event::{BuyInstructionArgs, CreatorActivityEvent, CreatorActivityKind, IngestEvent, TokenCreated};
use crate::protocol::Protocol;

use super::Decoder;
use super::trade::DecodedTradeEvent;

impl Decoder {
    /// Decode a Create / Create_v2 instruction into TokenCreated +
    /// CreatorActivityDetected events.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn decode_create(
        &self,
        signature: &str,
        slot: u64,
        block_time: DateTime<Utc>,
        received_at: DateTime<Utc>,
        create_data: &[u8],
        pump_accounts: &[String],
        account_keys: &[&str],
        decoded_events: &[DecodedTradeEvent],
        decoded_create_events: &[DecodedCreateEvent],
        instruction_type: &str,
        instruction_labels: Vec<String>,
        cu_limit: Option<u64>,
        cu_price: Option<u64>,
        pump_ix_datas: &[&[u8]],
    ) -> Vec<IngestEvent> {
        let p = &self.protocol;
        let mint = match pump_accounts.first().filter(|s| !s.is_empty()) {
            Some(m) => m.clone(),
            None => {
                warn!("Create: missing mint at accounts[0] for tx {signature}");
                return vec![];
            }
        };

        let ix_info = decode_create_info(create_data, p);
        let create_log = decoded_create_events.iter().find(|e| e.mint == mint);
        let tok22 = &p.programs.token_2022.base58;
        let is_v2 = ix_info.as_ref().map(|i| i.is_v2).unwrap_or(false)
            || create_log
                .and_then(|e| e.token_program.as_deref())
                .is_some_and(|p| p == tok22.as_str());

        let creator = resolve_creator_wallet(ix_info.as_ref(), create_log, pump_accounts, is_v2, account_keys);
        if creator.is_empty() {
            warn!("Create: cannot determine creator for tx {signature}");
            return vec![];
        }

        let buy_user = create_log
            .map(|e| e.user.clone())
            .or_else(|| pump_user_account(pump_accounts, is_v2))
            .or_else(|| account_keys.first().copied().map(|s| s.to_string()))
            .unwrap_or_else(|| creator.clone());

        let initial_create_event = decoded_events
            .iter()
            .find(|ev| ev.is_buy && ev.user == buy_user && ev.mint == mint);

        let initial_supply = initial_create_event.map(|ev| ev.token_amount as u64);
        let initial_buy_sol = initial_create_event.map(|ev| ev.sol_amount);

        let name = create_log.map(|e| e.name.clone())
            .or_else(|| ix_info.as_ref().map(|i| i.name.clone()))
            .unwrap_or_else(|| "Unknown".to_string());
        let symbol = create_log.map(|e| e.symbol.clone())
            .or_else(|| ix_info.as_ref().map(|i| i.symbol.clone()))
            .unwrap_or_else(|| "UNKNOWN".to_string());
        let is_mayhem_mode = create_log.map(|e| e.is_mayhem_mode)
            .or_else(|| ix_info.as_ref().map(|i| i.is_mayhem_mode))
            .unwrap_or(false);
        let is_cashback_enabled = create_log.map(|e| e.is_cashback_enabled)
            .or_else(|| ix_info.as_ref().map(|i| i.is_cashback_enabled))
            .unwrap_or(false);
        let bonding_curve = create_log.and_then(|e| e.bonding_curve.clone())
            .or_else(|| pump_accounts.get(2).cloned());
        let initial_buy_instruction = extract_pump_buy_instruction_data(pump_ix_datas, p);

        let token_program_id = create_log
            .and_then(|e| e.token_program.clone())
            .or_else(|| {
                if is_v2 { Some(p.programs.token_2022.base58.clone()) }
                else { Some(p.programs.token.base58.clone()) }
            });

        debug!(
            sig = %signature, mint = %mint, creator = %creator,
            name = %name, symbol = %symbol, is_v2 = is_v2,
            instruction_type = %instruction_type,
            "Token created"
        );

        let creator_event = CreatorActivityEvent {
            creator: creator.clone(),
            mint: mint.clone(),
            kind: CreatorActivityKind::Create,
            signature: signature.to_string(),
            slot,
            block_time,
            received_at,
        };

        vec![
            IngestEvent::TokenCreated(TokenCreated {
                mint,
                creator,
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
                instruction_labels,
                signature: signature.to_string(),
                slot,
                block_time,
                received_at,
            }),
            IngestEvent::CreatorActivity(creator_event),
        ]
    }
}

// ── CreateEvent Borsh decode ──────────────────────────────────────────────────

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
    pub(super) name: String,
    pub(super) symbol: String,
    pub(super) mint: String,
    pub(super) bonding_curve: Option<String>,
    pub(super) user: String,
    pub(super) creator: String,
    pub(super) token_program: Option<String>,
    pub(super) is_mayhem_mode: bool,
    pub(super) is_cashback_enabled: bool,
}

pub(super) fn decode_create_events_from_logs(logs: &[&str], disc: &[u8; 8]) -> Vec<DecodedCreateEvent> {
    let mut events = Vec::new();
    for log in logs {
        let Some(encoded) = log.strip_prefix("Program data: ") else { continue; };
        let bytes = match STANDARD.decode(encoded) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if bytes.len() < 8 || &bytes[..8] != disc {
            continue;
        }
        let mut buf: &[u8] = &bytes[8..];
        match RawCreateEvent::deserialize(&mut buf) {
            Ok(raw) => events.push(DecodedCreateEvent {
                name: raw.name,
                symbol: raw.symbol,
                mint: bs58::encode(raw.mint).into_string(),
                bonding_curve: Some(bs58::encode(raw.bonding_curve).into_string()),
                user: bs58::encode(raw.user).into_string(),
                creator: bs58::encode(raw.creator).into_string(),
                token_program: Some(bs58::encode(raw.token_program).into_string()),
                is_mayhem_mode: raw.is_mayhem_mode,
                is_cashback_enabled: raw.is_cashback_enabled,
            }),
            Err(e) => warn!("Failed to Borsh-decode CreateEvent: {e}"),
        }
    }
    events
}

// ── Buy instruction args ──────────────────────────────────────────────────────

#[derive(BorshDeserialize)]
struct BuyArgs { token_amount: u64, max_sol_cost: u64 }
#[derive(BorshDeserialize)]
struct BuyExactArgs { spendable_sol_in: u64, min_tokens_out: u64 }

fn parse_buy_ix(data: &[u8], p: &Protocol) -> Option<BuyInstructionArgs> {
    if data.len() < 8 { return None; }
    let d = &p.discriminators;
    let (disc, rest) = data.split_at(8);
    let mut buf = rest;
    if disc == d.buy {
        let a = BuyArgs::deserialize(&mut buf).ok()?;
        return Some(BuyInstructionArgs::Buy { token_amount: a.token_amount, max_sol_cost: a.max_sol_cost });
    }
    if disc == d.buy_v2 {
        let a = BuyArgs::deserialize(&mut buf).ok()?;
        return Some(BuyInstructionArgs::BuyV2 { token_amount: a.token_amount, max_sol_cost: a.max_sol_cost });
    }
    if disc == d.buy_exact_sol_in {
        let a = BuyExactArgs::deserialize(&mut buf).ok()?;
        return Some(BuyInstructionArgs::BuyExactSolIn { spendable_sol_in: a.spendable_sol_in, min_tokens_out: a.min_tokens_out });
    }
    if disc == d.buy_exact_quote_in {
        let a = BuyExactArgs::deserialize(&mut buf).ok()?;
        return Some(BuyInstructionArgs::BuyExactQuoteIn { spendable_sol_in: a.spendable_sol_in, min_tokens_out: a.min_tokens_out });
    }
    if disc == d.buy_exact_quote_in_v2 {
        let a = BuyExactArgs::deserialize(&mut buf).ok()?;
        return Some(BuyInstructionArgs::BuyExactQuoteInV2 { spendable_sol_in: a.spendable_sol_in, min_tokens_out: a.min_tokens_out });
    }
    None
}

fn extract_pump_buy_instruction_data(datas: &[&[u8]], p: &Protocol) -> Option<BuyInstructionArgs> {
    datas.iter().find_map(|&d| parse_buy_ix(d, p))
}

// ── Create instruction args decode ────────────────────────────────────────────

struct CreateInstructionInfo {
    name: String,
    symbol: String,
    creator: String,
    is_v2: bool,
    is_mayhem_mode: bool,
    is_cashback_enabled: bool,
}

fn decode_create_info(data: &[u8], p: &Protocol) -> Option<CreateInstructionInfo> {
    if data.len() < 16 { return None; }
    let d = &p.discriminators;
    let disc = &data[..8];
    let is_v2 = disc == d.create_v2_ix;
    if disc != d.create_ix && !is_v2 { return None; }

    let mut offset = 8;
    let name = read_anchor_string(data, &mut offset)?;
    let symbol = read_anchor_string(data, &mut offset)?;
    read_anchor_string(data, &mut offset)?; // uri — consumed but not used
    let creator = read_pubkey(data, &mut offset)
        .map(|pk| bs58::encode(pk).into_string())?;

    let (is_mayhem_mode, is_cashback_enabled) = if is_v2 {
        let m = data.get(offset).copied().unwrap_or(0) == 1;
        offset += 1;
        let c = data.get(offset).copied().unwrap_or(0) == 1;
        (m, c)
    } else {
        (false, false)
    };

    Some(CreateInstructionInfo { name, symbol, creator, is_v2, is_mayhem_mode, is_cashback_enabled })
}

fn pump_user_account(pump_accounts: &[String], is_v2: bool) -> Option<String> {
    let idx = if is_v2 { 5 } else { 7 };
    pump_accounts.get(idx).filter(|s| !s.is_empty()).cloned()
}

fn resolve_creator_wallet(
    ix_info: Option<&CreateInstructionInfo>,
    create_log: Option<&DecodedCreateEvent>,
    pump_accounts: &[String],
    is_v2: bool,
    account_keys: &[&str],
) -> String {
    if let Some(ev) = create_log { return ev.creator.clone(); }
    if let Some(ix) = ix_info { if !ix.creator.is_empty() { return ix.creator.clone(); } }
    if let Some(user) = pump_user_account(pump_accounts, is_v2) { return user; }
    account_keys.first().copied().filter(|s| !s.is_empty())
        .map(|s| s.to_string()).unwrap_or_default()
}

fn read_pubkey(data: &[u8], offset: &mut usize) -> Option<[u8; 32]> {
    let pk: [u8; 32] = data.get(*offset..*offset + 32)?.try_into().ok()?;
    *offset += 32;
    Some(pk)
}

fn read_anchor_string(data: &[u8], offset: &mut usize) -> Option<String> {
    let len_bytes: [u8; 4] = data.get(*offset..*offset + 4)?.try_into().ok()?;
    let len = u32::from_le_bytes(len_bytes) as usize;
    *offset += 4;
    let s = std::str::from_utf8(data.get(*offset..*offset + len)?).ok()?.to_string();
    *offset += len;
    Some(s)
}
