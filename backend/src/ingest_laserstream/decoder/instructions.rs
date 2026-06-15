//! Shared instruction classification and labeling leaves (used by both paths).
//!
//! Classifies pump.fun instructions into [`InstructionKind`] by Anchor
//! discriminator, derives the primary instruction type, extracts compute-budget
//! values, and renders one human-readable label per instruction — all from plain
//! byte slices, so the grpc and json paths share them. The `Value`-shaped
//! adapters live in [`super::json::instructions`]; the protobuf ones in
//! [`super::grpc`].

use borsh::BorshDeserialize;

use crate::config::constants::{
    program_friendly_name, ASSOCIATED_TOKEN_PROGRAM_ID, BUY_DISCRIMINATOR,
    BUY_EXACT_QUOTE_IN_DISCRIMINATOR, BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR,
    BUY_EXACT_SOL_IN_DISCRIMINATOR, BUY_V2_DISCRIMINATOR, COMPUTE_BUDGET_PROGRAM_ID,
    CREATE_INSTRUCTION_DISCRIMINATOR, CREATE_V2_INSTRUCTION_DISCRIMINATOR,
    MIGRATE_INSTRUCTION_DISCRIMINATOR, MIGRATE_V2_INSTRUCTION_DISCRIMINATOR, PUMP_FUN_PROGRAM_ID,
    SELL_DISCRIMINATOR, SYSTEM_PROGRAM_ID, TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID,
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
/// `parsed_type` is the Helius `jsonParsed` instruction type (`parsed.type`)
/// when available — present only on the RPC/`Value` path. The protobuf-native
/// live path has no such field and passes `None`, so System/Token/ATA
/// instructions there fall through to `"<friendly>: Unknown"` (matching the
/// live adapter's output before Tier B, where `parsed` was never synthesized).
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
        if let Some(t) = parsed_type {
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
