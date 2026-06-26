//! Shared instruction classification and labeling leaves.
//!
//! Classifies pump.fun instructions into [`InstructionKind`] by Anchor
//! discriminator, derives the primary instruction type, extracts compute-budget
//! values, and renders one human-readable label per instruction — all from plain
//! byte slices. The protobuf-shaped instruction adapters that feed these live in
//! [`super::grpc`].

use borsh::BorshDeserialize;

use backend_core::config::constants::{
    program_friendly_name, ASSOCIATED_TOKEN_PROGRAM_ID, BUY_DISCRIMINATOR,
    BUY_EXACT_QUOTE_IN_DISCRIMINATOR, BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR,
    BUY_EXACT_SOL_IN_DISCRIMINATOR, BUY_V2_DISCRIMINATOR, COMPUTE_BUDGET_PROGRAM_ID,
    CREATE_INSTRUCTION_DISCRIMINATOR, CREATE_V2_INSTRUCTION_DISCRIMINATOR,
    MIGRATE_INSTRUCTION_DISCRIMINATOR, MIGRATE_V2_INSTRUCTION_DISCRIMINATOR,
    PUMP_COLLECT_CREATOR_FEE_DISCRIMINATOR, PUMP_EXTEND_ACCOUNT_DISCRIMINATOR,
    PUMP_FUN_PROGRAM_ID, PUMP_INITIALIZE_DISCRIMINATOR, PUMP_SET_PARAMS_DISCRIMINATOR,
    PUMP_SWAP_CREATE_POOL_DISCRIMINATOR, PUMP_SWAP_DEPOSIT_DISCRIMINATOR,
    PUMP_SWAP_DISABLE_DISCRIMINATOR, PUMP_SWAP_PROGRAM_ID,
    PUMP_SWAP_UPDATE_ADMIN_DISCRIMINATOR, PUMP_SWAP_UPDATE_FEE_CONFIG_DISCRIMINATOR,
    PUMP_WITHDRAW_DISCRIMINATOR, SELL_DISCRIMINATOR, SYSTEM_PROGRAM_ID, TOKEN_2022_PROGRAM_ID,
    TOKEN_PROGRAM_ID,
};

use super::trade::DecodedTradeEvent;

/// Simplified instruction classifier.
/// `create` (SPL Token) and `create_v2` (Token-2022) are distinct instructions
/// with different account layouts; both map to `InstructionKind::Create`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InstructionKind {
    Create,
    Buy,
    Sell,
    Migrate,
    Unknown,
}

/// Classify one already-decoded pump.fun instruction by its 8-byte Anchor
/// discriminator. Returns `None` for non-pump ixs or data shorter than 8 bytes.
/// All buy/sell variants (BuyExactSolIn, BuyExactQuoteInV2, …) collapse into
/// Buy / Sell.
pub(super) fn classify_pump_ix(program_id: &str, data: Option<&[u8]>) -> Option<InstructionKind> {
    if program_id != PUMP_FUN_PROGRAM_ID {
        return None;
    }
    let bytes = data.filter(|b| b.len() >= 8)?;

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
    Some(kind)
}

pub(super) fn determine_instruction_type(
    kinds: &[InstructionKind],
    decoded_events: &[DecodedTradeEvent],
) -> String {
    let has_buy = kinds.contains(&InstructionKind::Buy);
    let has_sell = kinds.contains(&InstructionKind::Sell);

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
    if kinds.contains(&InstructionKind::Create) {
        return "Create".to_string();
    }
    if kinds.contains(&InstructionKind::Migrate) {
        return "Migrate".to_string();
    }

    "Unknown".to_string()
}

// ---------------------------------------------------------------------------
// Step 2 — per-instruction labels + compute-budget extraction (shared leaves)
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

/// Extract `cu_limit` / `cu_price` from a Compute Budget instruction's bytes,
/// updating the caller's accumulators. No-op for any other program. Shared by
/// the `Value` label pass and the protobuf-native decode path.
pub(super) fn extract_compute_budget(
    program_id: &str,
    data: Option<&[u8]>,
    cu_limit: &mut Option<u64>,
    cu_price: &mut Option<u64>,
) {
    if program_id != COMPUTE_BUDGET_PROGRAM_ID {
        return;
    }
    if let Some(b) = data {
        let mut buf: &[u8] = b;
        match ComputeBudgetIx::deserialize(&mut buf) {
            Ok(ComputeBudgetIx::SetComputeUnitLimit(units)) => *cu_limit = Some(units as u64),
            Ok(ComputeBudgetIx::SetComputeUnitPrice(price)) => *cu_price = Some(price),
            _ => {}
        }
    }
}

/// Produce a single human-readable label for one instruction.
///
/// System Program, SPL Token / Token-2022, and Associated Token instructions are
/// decoded directly from their instruction-data discriminator (4-byte u32 LE for
/// System, 1-byte index for Token, empty-data/1-byte for ATA), exactly like
/// Compute Budget — so the protobuf-native live path keeps them labelled instead of
/// rendering `"<friendly>: Unknown"`. `parsed_type` is the Helius `jsonParsed`
/// instruction type (`parsed.type`), present only on the legacy RPC/`Value` path; it
/// is used as a fallback for anything the byte decode doesn't recognise. The
/// protobuf path passes `None`.
pub(super) fn label_instruction(
    program_id: &str,
    parsed_type: Option<&str>,
    data_bytes: Option<&[u8]>,
) -> String {
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
        if let Some(t) = parsed_type {
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
            // Admin / lifecycle
            if d == PUMP_INITIALIZE_DISCRIMINATOR {
                return "Pump.Fun: Initialize".to_owned();
            }
            if d == PUMP_SET_PARAMS_DISCRIMINATOR {
                return "Pump.Fun: SetParams".to_owned();
            }
            if d == PUMP_WITHDRAW_DISCRIMINATOR {
                return "Pump.Fun: Withdraw".to_owned();
            }
            if d == PUMP_EXTEND_ACCOUNT_DISCRIMINATOR {
                return "Pump.Fun: ExtendAccount".to_owned();
            }
            if d == PUMP_COLLECT_CREATOR_FEE_DISCRIMINATOR {
                return "Pump.Fun: CollectCreatorFee".to_owned();
            }
        }
        return "Pump.Fun: Unknown".to_owned();
    }

    // ── PumpSwap (pump_amm) — 8-byte Anchor discriminator ────────────────────
    if program_id == PUMP_SWAP_PROGRAM_ID {
        if let Some(b) = data_bytes.filter(|b| b.len() >= 8) {
            let d = &b[..8];
            if d == BUY_DISCRIMINATOR {
                return "PumpSwap: Buy".to_owned();
            }
            if d == SELL_DISCRIMINATOR {
                return "PumpSwap: Sell".to_owned();
            }
            if d == PUMP_SWAP_CREATE_POOL_DISCRIMINATOR {
                return "PumpSwap: CreatePool".to_owned();
            }
            if d == PUMP_SWAP_DEPOSIT_DISCRIMINATOR {
                return "PumpSwap: Deposit".to_owned();
            }
            if d == PUMP_WITHDRAW_DISCRIMINATOR {
                return "PumpSwap: Withdraw".to_owned();
            }
            if d == PUMP_INITIALIZE_DISCRIMINATOR {
                return "PumpSwap: Initialize".to_owned();
            }
            if d == PUMP_SWAP_DISABLE_DISCRIMINATOR {
                return "PumpSwap: Disable".to_owned();
            }
            if d == PUMP_SWAP_UPDATE_ADMIN_DISCRIMINATOR {
                return "PumpSwap: UpdateAdmin".to_owned();
            }
            if d == PUMP_SWAP_UPDATE_FEE_CONFIG_DISCRIMINATOR {
                return "PumpSwap: UpdateFeeConfig".to_owned();
            }
        }
        return "PumpSwap: Unknown".to_owned();
    }

    // ── System Program — 4-byte u32 LE instruction discriminator ─────────────
    if program_id == SYSTEM_PROGRAM_ID {
        if let Some(name) = data_bytes.and_then(system_ix_name) {
            return format!("System Program: {name}");
        }
        return label_from_parsed_or_unknown("System Program", parsed_type);
    }

    // ── SPL Token / Token-2022 — 1-byte instruction index ────────────────────
    if program_id == TOKEN_PROGRAM_ID || program_id == TOKEN_2022_PROGRAM_ID {
        let friendly = if program_id == TOKEN_PROGRAM_ID {
            "Token Program"
        } else {
            "Token 2022"
        };
        if let Some(name) = data_bytes.and_then(token_ix_name) {
            return format!("{friendly}: {name}");
        }
        return label_from_parsed_or_unknown(friendly, parsed_type);
    }

    // ── Associated Token — empty data = legacy Create; else 1-byte index ─────
    if program_id == ASSOCIATED_TOKEN_PROGRAM_ID {
        if let Some(name) = ata_ix_name(data_bytes) {
            return format!("Associated Token: {name}");
        }
        return label_from_parsed_or_unknown("Associated Token", parsed_type);
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

/// Fallback label for a native program whose byte-level discriminator wasn't
/// recognised: use the Helius `parsed.type` if present (legacy RPC path), else
/// `"<friendly>: Unknown"`.
fn label_from_parsed_or_unknown(friendly: &str, parsed_type: Option<&str>) -> String {
    match parsed_type {
        Some(t) => format!("{}: {}", friendly, capitalize_first(t)),
        None => format!("{friendly}: Unknown"),
    }
}

/// System Program instruction name by its 4-byte little-endian `u32`
/// discriminator. Returns `None` for data shorter than 4 bytes or an
/// unrecognised variant.
fn system_ix_name(data: &[u8]) -> Option<&'static str> {
    let disc = u32::from_le_bytes(data.get(0..4)?.try_into().ok()?);
    let name = match disc {
        0 => "CreateAccount",
        1 => "Assign",
        2 => "Transfer",
        3 => "CreateAccountWithSeed",
        4 => "AdvanceNonceAccount",
        5 => "WithdrawNonceAccount",
        6 => "InitializeNonceAccount",
        7 => "AuthorizeNonceAccount",
        8 => "Allocate",
        9 => "AllocateWithSeed",
        10 => "AssignWithSeed",
        11 => "TransferWithSeed",
        12 => "UpgradeNonceAccount",
        _ => return None,
    };
    Some(name)
}

/// SPL Token / Token-2022 instruction name by its leading discriminator byte.
/// Covers the standard SPL Token instruction set (shared by Token-2022 for
/// indices 0–25); Token-2022 extension instructions (26+) and any unrecognised
/// byte return `None`.
fn token_ix_name(data: &[u8]) -> Option<&'static str> {
    let name = match data.first()? {
        0 => "InitializeMint",
        1 => "InitializeAccount",
        2 => "InitializeMultisig",
        3 => "Transfer",
        4 => "Approve",
        5 => "Revoke",
        6 => "SetAuthority",
        7 => "MintTo",
        8 => "Burn",
        9 => "CloseAccount",
        10 => "FreezeAccount",
        11 => "ThawAccount",
        12 => "TransferChecked",
        13 => "ApproveChecked",
        14 => "MintToChecked",
        15 => "BurnChecked",
        16 => "InitializeAccount2",
        17 => "SyncNative",
        18 => "InitializeAccount3",
        19 => "InitializeMultisig2",
        20 => "InitializeMint2",
        21 => "GetAccountDataSize",
        22 => "InitializeImmutableOwner",
        23 => "AmountToUiAmount",
        24 => "UiAmountToAmount",
        25 => "InitializeMintCloseAuthority",
        _ => return None,
    };
    Some(name)
}

/// Associated Token Account instruction name. The legacy `Create` carries **no**
/// instruction data (the most common case in a pump create+buy); newer variants
/// carry a 1-byte discriminator. Returns `None` for an unrecognised byte.
fn ata_ix_name(data: Option<&[u8]>) -> Option<&'static str> {
    match data {
        Some(b) if b.is_empty() => Some("Create"),
        Some(b) => match b[0] {
            0 => Some("Create"),
            1 => Some("CreateIdempotent"),
            2 => Some("RecoverNested"),
            _ => None,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The protobuf path passes `parsed_type=None`; native programs must still
    /// resolve from their instruction-data discriminator instead of "Unknown".
    #[test]
    fn native_programs_decode_from_bytes_without_parsed_type() {
        // System Program: 4-byte u32 LE discriminator → 2 = Transfer.
        assert_eq!(
            label_instruction(SYSTEM_PROGRAM_ID, None, Some(&2u32.to_le_bytes())),
            "System Program: Transfer"
        );
        // SPL Token: 1-byte index → 7 = MintTo.
        assert_eq!(
            label_instruction(TOKEN_PROGRAM_ID, None, Some(&[7])),
            "Token Program: MintTo"
        );
        // Token-2022 shares the SPL index space.
        assert_eq!(
            label_instruction(TOKEN_2022_PROGRAM_ID, None, Some(&[3])),
            "Token 2022: Transfer"
        );
        // ATA legacy Create carries no instruction data.
        assert_eq!(
            label_instruction(ASSOCIATED_TOKEN_PROGRAM_ID, None, Some(&[])),
            "Associated Token: Create"
        );
        assert_eq!(
            label_instruction(ASSOCIATED_TOKEN_PROGRAM_ID, None, Some(&[1])),
            "Associated Token: CreateIdempotent"
        );
    }

    /// Unrecognised discriminators fall back to "<friendly>: Unknown" (or the
    /// Helius parsed type when the legacy RPC path supplies one).
    #[test]
    fn unrecognised_discriminator_falls_back() {
        assert_eq!(
            label_instruction(SYSTEM_PROGRAM_ID, None, Some(&99u32.to_le_bytes())),
            "System Program: Unknown"
        );
        assert_eq!(
            label_instruction(TOKEN_PROGRAM_ID, Some("syncNative"), Some(&[250])),
            "Token Program: SyncNative"
        );
    }

    /// Pump.fun admin / lifecycle instructions are now labelled instead of "Unknown".
    #[test]
    fn pump_admin_instructions_labeled() {
        assert_eq!(
            label_instruction(PUMP_FUN_PROGRAM_ID, None, Some(&PUMP_EXTEND_ACCOUNT_DISCRIMINATOR)),
            "Pump.Fun: ExtendAccount"
        );
        assert_eq!(
            label_instruction(PUMP_FUN_PROGRAM_ID, None, Some(&PUMP_WITHDRAW_DISCRIMINATOR)),
            "Pump.Fun: Withdraw"
        );
        assert_eq!(
            label_instruction(PUMP_FUN_PROGRAM_ID, None, Some(&PUMP_SET_PARAMS_DISCRIMINATOR)),
            "Pump.Fun: SetParams"
        );
        assert_eq!(
            label_instruction(PUMP_FUN_PROGRAM_ID, None, Some(&PUMP_INITIALIZE_DISCRIMINATOR)),
            "Pump.Fun: Initialize"
        );
        assert_eq!(
            label_instruction(
                PUMP_FUN_PROGRAM_ID,
                None,
                Some(&PUMP_COLLECT_CREATOR_FEE_DISCRIMINATOR)
            ),
            "Pump.Fun: CollectCreatorFee"
        );
    }

    /// PumpSwap instructions are decoded rather than falling through to "Unknown".
    #[test]
    fn pump_swap_instructions_labeled() {
        assert_eq!(
            label_instruction(PUMP_SWAP_PROGRAM_ID, None, Some(&BUY_DISCRIMINATOR)),
            "PumpSwap: Buy"
        );
        assert_eq!(
            label_instruction(PUMP_SWAP_PROGRAM_ID, None, Some(&SELL_DISCRIMINATOR)),
            "PumpSwap: Sell"
        );
        assert_eq!(
            label_instruction(
                PUMP_SWAP_PROGRAM_ID,
                None,
                Some(&PUMP_SWAP_CREATE_POOL_DISCRIMINATOR)
            ),
            "PumpSwap: CreatePool"
        );
        assert_eq!(
            label_instruction(
                PUMP_SWAP_PROGRAM_ID,
                None,
                Some(&PUMP_SWAP_DEPOSIT_DISCRIMINATOR)
            ),
            "PumpSwap: Deposit"
        );
        assert_eq!(
            label_instruction(PUMP_SWAP_PROGRAM_ID, None, Some(&[0xde, 0xad, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])),
            "PumpSwap: Unknown"
        );
    }
}
