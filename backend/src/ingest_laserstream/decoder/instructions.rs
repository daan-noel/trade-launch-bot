//! Instruction classification and human-readable labeling.
//!
//! Resolves program IDs from raw instructions, classifies pump.fun
//! instructions into [`InstructionKind`] by Anchor discriminator, and builds
//! the per-instruction labels (plus compute-budget extraction) attached to
//! every decoded trade/token.

use borsh::BorshDeserialize;
use serde_json::Value;

use crate::config::constants::{
    program_friendly_name, ASSOCIATED_TOKEN_PROGRAM_ID, BUY_DISCRIMINATOR,
    BUY_EXACT_QUOTE_IN_DISCRIMINATOR, BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR,
    BUY_EXACT_SOL_IN_DISCRIMINATOR, BUY_V2_DISCRIMINATOR, COMPUTE_BUDGET_PROGRAM_ID,
    CREATE_INSTRUCTION_DISCRIMINATOR, CREATE_V2_INSTRUCTION_DISCRIMINATOR,
    MIGRATE_INSTRUCTION_DISCRIMINATOR, MIGRATE_V2_INSTRUCTION_DISCRIMINATOR, PUMP_FUN_PROGRAM_ID,
    SELL_DISCRIMINATOR, SYSTEM_PROGRAM_ID, TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID,
};

use super::trade::DecodedTradeEvent;

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

/// One outer instruction with its program id resolved and its base58 `data`
/// decoded **exactly once**. The per-tx hot path classifies instruction kinds and
/// builds labels from the same prepared slice, so the same bytes are no longer
/// base58-decoded twice (once per pass) for every outer instruction.
pub(super) struct PreparedIx<'a> {
    pub(super) program_id: String,
    pub(super) data: Option<Vec<u8>>,
    pub(super) ix: &'a Value,
}

/// Resolve + base58-decode every outer (`message.instructions`) instruction once.
pub(super) fn prepare_instructions<'a>(
    message: &'a Value,
    account_keys: &[String],
) -> Vec<PreparedIx<'a>> {
    message["instructions"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|ix| PreparedIx {
                    program_id: resolve_instruction_program_id(ix, account_keys),
                    data: instruction_data_bytes(ix),
                    ix,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Classify one already-decoded pump.fun instruction by its 8-byte Anchor
/// discriminator. Returns `None` for non-pump ixs or data shorter than 8 bytes.
/// All buy/sell variants (BuyExactSolIn, BuyExactQuoteInV2, …) collapse into
/// Buy / Sell.
fn classify_pump_ix(program_id: &str, data: Option<&[u8]>) -> Option<InstructionKind> {
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

/// Map every pump.fun instruction (outer + inner) to a big-class
/// [`InstructionKind`]. Outer ixs come pre-decoded; inner ixs resolve their
/// program id first and skip the base58 decode unless they belong to pump.fun.
pub(super) fn collect_instruction_kinds(
    outer: &[PreparedIx],
    meta: &Value,
    account_keys: &[String],
) -> Vec<InstructionKind> {
    let mut kinds = Vec::new();

    for p in outer {
        if let Some(kind) = classify_pump_ix(&p.program_id, p.data.as_deref()) {
            kinds.push(kind);
        }
    }

    if let Some(groups) = meta["innerInstructions"].as_array() {
        for group in groups {
            if let Some(instructions) = group["instructions"].as_array() {
                for ix in instructions {
                    // Skip non-pump inner ixs before paying for the data decode.
                    let program_id = resolve_instruction_program_id(ix, account_keys);
                    if program_id != PUMP_FUN_PROGRAM_ID {
                        continue;
                    }
                    let data = instruction_data_bytes(ix);
                    if let Some(kind) = classify_pump_ix(&program_id, data.as_deref()) {
                        kinds.push(kind);
                    }
                }
            }
        }
    }

    kinds
}

pub(super) fn determine_instruction_type(
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

pub(super) fn resolve_instruction_program_id(ix: &Value, account_keys: &[String]) -> String {
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

pub(super) fn instruction_data_bytes(ix: &Value) -> Option<Vec<u8>> {
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

/// Iterate the pre-decoded top-level instructions and build a human-readable
/// label for each entry. Also extracts `cu_limit` and `cu_price` in the same
/// pass, reusing the already-decoded `data` bytes (no re-decode).

/// Returns `(labels, cu_limit_opt, cu_price_opt)`.
pub(super) fn build_instruction_labels(
    instructions: &[PreparedIx],
) -> (Vec<String>, Option<u64>, Option<u64>) {
    let mut cu_limit: Option<u64> = None;
    let mut cu_price: Option<u64> = None;

    let labels = instructions
        .iter()
        .map(|p| {
            let program_id = p.program_id.as_str();
            let data_bytes = p.data.as_deref();

            // Extract compute-budget values while labelling.
            if program_id == COMPUTE_BUDGET_PROGRAM_ID {
                if let Some(b) = data_bytes {
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

            label_instruction(program_id, p.ix, data_bytes)
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
