//! Instruction classification and labeling.

use borsh::BorshDeserialize;

use crate::protocol::Protocol;

use super::program_registry::{program_friendly_name, program_instruction_name};

use super::trade::DecodedTradeEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InstructionKind {
    Create,
    Buy,
    Sell,
    Migrate,
    Unknown,
}

/// Classify one pump.fun instruction by its 8-byte Anchor discriminator.
pub(super) fn classify_pump_ix(
    is_pump_fun: bool,
    data: Option<&[u8]>,
    p: &Protocol,
) -> Option<InstructionKind> {
    if !is_pump_fun {
        return None;
    }
    let d = &p.discriminators;
    let bytes = data.filter(|b| b.len() >= 8)?;

    let kind = if bytes.starts_with(&d.buy)
        || bytes.starts_with(&d.buy_exact_sol_in)
        || bytes.starts_with(&d.buy_exact_quote_in)
        || bytes.starts_with(&d.buy_v2)
        || bytes.starts_with(&d.buy_exact_quote_in_v2)
    {
        InstructionKind::Buy
    } else if bytes.starts_with(&d.sell) {
        InstructionKind::Sell
    } else if bytes.starts_with(&d.create_ix) || bytes.starts_with(&d.create_v2_ix) {
        InstructionKind::Create
    } else if bytes.starts_with(&d.migrate_ix) || bytes.starts_with(&d.migrate_v2_ix) {
        InstructionKind::Migrate
    } else {
        InstructionKind::Unknown
    };
    Some(kind)
}

pub(super) fn determine_instruction_type(
    kinds: &[InstructionKind],
    decoded_events: &[DecodedTradeEvent],
) -> String {
    let has_buy = kinds.contains(&InstructionKind::Buy);
    let has_sell = kinds.contains(&InstructionKind::Sell);

    if has_buy && has_sell {
        let (buy_sol, sell_sol) = decoded_events.iter().fold((0.0, 0.0), |(b, s), ev| {
            if ev.is_buy { (b + ev.sol_amount, s) } else { (b, s + ev.sol_amount) }
        });
        if buy_sol > sell_sol { return "Buy".to_string(); }
        if sell_sol > buy_sol { return "Sell".to_string(); }
        let bc = kinds.iter().filter(|k| **k == InstructionKind::Buy).count();
        let sc = kinds.iter().filter(|k| **k == InstructionKind::Sell).count();
        return if bc >= sc { "Buy".to_string() } else { "Sell".to_string() };
    }
    if has_buy { return "Buy".to_string(); }
    if has_sell { return "Sell".to_string(); }
    if kinds.contains(&InstructionKind::Create) { return "Create".to_string(); }
    if kinds.contains(&InstructionKind::Migrate) { return "Migrate".to_string(); }
    "Unknown".to_string()
}

// ── Borsh-deserialised Compute Budget instruction ─────────────────────────────

#[derive(BorshDeserialize)]
#[allow(dead_code)]
enum ComputeBudgetIx {
    Unused,
    RequestHeapFrame(u32),
    SetComputeUnitLimit(u32),
    SetComputeUnitPrice(u64),
    SetLoadedAccountsDataSizeLimit(u32),
}

pub(super) fn extract_compute_budget(
    is_compute_budget: bool,
    data: Option<&[u8]>,
    cu_limit: &mut Option<u64>,
    cu_price: &mut Option<u64>,
) {
    if !is_compute_budget {
        return;
    }
    if let Some(b) = data {
        let mut buf: &[u8] = b;
        match ComputeBudgetIx::deserialize(&mut buf) {
            Ok(ComputeBudgetIx::SetComputeUnitLimit(u)) => *cu_limit = Some(u as u64),
            Ok(ComputeBudgetIx::SetComputeUnitPrice(p)) => *cu_price = Some(p),
            _ => {}
        }
    }
}

/// Produce a human-readable label for one instruction.
pub(super) fn label_instruction(
    program_id: &str,
    parsed_type: Option<&str>,
    data_bytes: Option<&[u8]>,
    p: &Protocol,
) -> String {
    let d = &p.discriminators;
    let pump_id = &p.programs.pump_fun.base58;
    let swap_id = &p.programs.pump_swap.base58;
    let cb_id = &p.programs.compute_budget.base58;
    let sys_id = &p.programs.system.base58;
    let tok_id = &p.programs.token.base58;
    let tok22_id = &p.programs.token_2022.base58;
    let ata_id = &p.programs.associated_token.base58;

    if program_id == cb_id.as_str() {
        if let Some(bytes) = data_bytes {
            match bytes.first() {
                Some(&1) => return "Compute Budget: RequestHeapFrame".to_owned(),
                Some(&2) => return "Compute Budget: SetComputeUnitLimit".to_owned(),
                Some(&3) => return "Compute Budget: SetComputeUnitPrice".to_owned(),
                Some(&4) => return "Compute Budget: SetLoadedAccountsDataSizeLimit".to_owned(),
                _ => {}
            }
        }
        if let Some(t) = parsed_type {
            return format!("Compute Budget: {}", capitalize_first(t));
        }
        return "Compute Budget: Unknown".to_owned();
    }

    if program_id == pump_id.as_str() {
        if let Some(b) = data_bytes.filter(|b| b.len() >= 8) {
            let disc = &b[..8];
            if disc == d.buy { return "Pump.Fun: Buy".to_owned(); }
            if disc == d.buy_exact_sol_in { return "Pump.Fun: BuyExactSolIn".to_owned(); }
            if disc == d.buy_exact_quote_in { return "Pump.Fun: BuyExactQuoteIn".to_owned(); }
            if disc == d.buy_v2 { return "Pump.Fun: BuyV2".to_owned(); }
            if disc == d.buy_exact_quote_in_v2 { return "Pump.Fun: BuyExactQuoteInV2".to_owned(); }
            if disc == d.sell { return "Pump.Fun: Sell".to_owned(); }
            if disc == d.create_ix { return "Pump.Fun: Create".to_owned(); }
            if disc == d.create_v2_ix { return "Pump.Fun: Create_v2".to_owned(); }
            if disc == d.migrate_ix { return "Pump.Fun: Migrate".to_owned(); }
            if disc == d.migrate_v2_ix { return "Pump.Fun: Migrate_v2".to_owned(); }
            if disc == d.initialize { return "Pump.Fun: Initialize".to_owned(); }
            if disc == d.set_params { return "Pump.Fun: SetParams".to_owned(); }
            if disc == d.withdraw { return "Pump.Fun: Withdraw".to_owned(); }
            if disc == d.extend_account { return "Pump.Fun: ExtendAccount".to_owned(); }
            if disc == d.collect_creator_fee { return "Pump.Fun: CollectCreatorFee".to_owned(); }
        }
        return "Pump.Fun: Unknown".to_owned();
    }

    if program_id == swap_id.as_str() {
        if let Some(b) = data_bytes.filter(|b| b.len() >= 8) {
            let disc = &b[..8];
            if disc == d.buy { return "PumpSwap: Buy".to_owned(); }
            if disc == d.sell { return "PumpSwap: Sell".to_owned(); }
            if disc == d.pump_swap_create_pool { return "PumpSwap: CreatePool".to_owned(); }
            if disc == d.pump_swap_deposit { return "PumpSwap: Deposit".to_owned(); }
            if disc == d.withdraw { return "PumpSwap: Withdraw".to_owned(); }
            if disc == d.initialize { return "PumpSwap: Initialize".to_owned(); }
            if disc == d.pump_swap_disable { return "PumpSwap: Disable".to_owned(); }
            if disc == d.pump_swap_update_admin { return "PumpSwap: UpdateAdmin".to_owned(); }
            if disc == d.pump_swap_update_fee_config { return "PumpSwap: UpdateFeeConfig".to_owned(); }
        }
        return "PumpSwap: Unknown".to_owned();
    }

    if program_id == sys_id.as_str() {
        if let Some(name) = data_bytes.and_then(system_ix_name) {
            return format!("System Program: {name}");
        }
        return label_from_parsed_or_unknown("System Program", parsed_type);
    }

    if program_id == tok_id.as_str() || program_id == tok22_id.as_str() {
        let friendly = if program_id == tok_id.as_str() { "Token Program" } else { "Token 2022" };
        if let Some(name) = data_bytes.and_then(token_ix_name) {
            return format!("{friendly}: {name}");
        }
        return label_from_parsed_or_unknown(friendly, parsed_type);
    }

    if program_id == ata_id.as_str() {
        if let Some(name) = ata_ix_name(data_bytes) {
            return format!("Associated Token: {name}");
        }
        return label_from_parsed_or_unknown("Associated Token", parsed_type);
    }

    if let Some(name) = program_friendly_name(program_id) {
        // Phase 4: name the instruction within known open (Anchor) programs
        // (`Jupiter Aggregator V6: Route`); unknown/closed ones stay `: Unknown`.
        let ix = program_instruction_name(program_id, data_bytes).unwrap_or("Unknown");
        return format!("{name}: {ix}");
    }

    // Carry the FULL program id (not a truncated suffix) so unknown programs are
    // self-identifying in the persisted `trades.ix_labels` — that column is the
    // only durable record of what ran (this deployment does not persist raw_txs),
    // and the `unknown-programs` harvest ranks these to grow `program_registry`.
    // Label arrays stay low-cardinality (ids repeat), so columnar compression is
    // unaffected.
    format!("Unknown ({program_id})")
}

fn label_from_parsed_or_unknown(friendly: &str, parsed_type: Option<&str>) -> String {
    match parsed_type {
        Some(t) => format!("{}: {}", friendly, capitalize_first(t)),
        None => format!("{friendly}: Unknown"),
    }
}

fn system_ix_name(data: &[u8]) -> Option<&'static str> {
    let disc = u32::from_le_bytes(data.get(0..4)?.try_into().ok()?);
    Some(match disc {
        0 => "CreateAccount", 1 => "Assign", 2 => "Transfer",
        3 => "CreateAccountWithSeed", 4 => "AdvanceNonceAccount",
        5 => "WithdrawNonceAccount", 6 => "InitializeNonceAccount",
        7 => "AuthorizeNonceAccount", 8 => "Allocate",
        9 => "AllocateWithSeed", 10 => "AssignWithSeed",
        11 => "TransferWithSeed", 12 => "UpgradeNonceAccount",
        _ => return None,
    })
}

fn token_ix_name(data: &[u8]) -> Option<&'static str> {
    Some(match data.first()? {
        0 => "InitializeMint", 1 => "InitializeAccount", 2 => "InitializeMultisig",
        3 => "Transfer", 4 => "Approve", 5 => "Revoke", 6 => "SetAuthority",
        7 => "MintTo", 8 => "Burn", 9 => "CloseAccount", 10 => "FreezeAccount",
        11 => "ThawAccount", 12 => "TransferChecked", 13 => "ApproveChecked",
        14 => "MintToChecked", 15 => "BurnChecked", 16 => "InitializeAccount2",
        17 => "SyncNative", 18 => "InitializeAccount3", 19 => "InitializeMultisig2",
        20 => "InitializeMint2", 21 => "GetAccountDataSize",
        22 => "InitializeImmutableOwner", 23 => "AmountToUiAmount",
        24 => "UiAmountToAmount", 25 => "InitializeMintCloseAuthority",
        _ => return None,
    })
}

fn ata_ix_name(data: Option<&[u8]>) -> Option<&'static str> {
    match data {
        Some(b) if b.is_empty() => Some("Create"),
        Some(b) => match b[0] {
            0 => Some("Create"), 1 => Some("CreateIdempotent"),
            2 => Some("RecoverNested"), _ => None,
        },
        None => None,
    }
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}
