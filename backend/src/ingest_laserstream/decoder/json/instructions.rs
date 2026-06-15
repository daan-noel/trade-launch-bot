//! `Value`-path instruction helpers: resolve + base58-decode the Helius
//! `jsonParsed` instructions, map them to [`InstructionKind`], and build the
//! per-instruction labels. The classification/labeling leaves themselves
//! (`classify_pump_ix`, `extract_compute_budget`, `label_instruction`) are shared
//! from [`super::super::instructions`]; this module only adapts the `Value`
//! shape into them. The grpc path has its own protobuf analogues in
//! [`super::super::grpc`].

use serde_json::Value;

use crate::config::constants::PUMP_FUN_PROGRAM_ID;

use super::super::instructions::{
    classify_pump_ix, extract_compute_budget, label_instruction, InstructionKind,
};

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
    account_keys: &[&str],
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

/// Map every pump.fun instruction (outer + inner) to a big-class
/// [`InstructionKind`]. Outer ixs come pre-decoded; inner ixs resolve their
/// program id first and skip the base58 decode unless they belong to pump.fun.
pub(super) fn collect_instruction_kinds(
    outer: &[PreparedIx],
    meta: &Value,
    account_keys: &[&str],
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

pub(super) fn resolve_instruction_program_id(ix: &Value, account_keys: &[&str]) -> String {
    ix["programId"]
        .as_str()
        .map(|s| s.to_owned())
        .or_else(|| {
            ix["programIdIndex"]
                .as_u64()
                .and_then(|i| account_keys.get(i as usize).copied())
                .map(|s| s.to_owned())
        })
        .unwrap_or_default()
}

pub(super) fn instruction_data_bytes(ix: &Value) -> Option<Vec<u8>> {
    ix["data"]
        .as_str()
        .and_then(|s| bs58::decode(s).into_vec().ok())
}

/// Iterate the pre-decoded top-level instructions and build a human-readable
/// label for each entry. Also extracts `cu_limit` and `cu_price` in the same
/// pass, reusing the already-decoded `data` bytes (no re-decode).
///
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
            extract_compute_budget(program_id, data_bytes, &mut cu_limit, &mut cu_price);

            let parsed_type = p.ix.pointer("/parsed/type").and_then(Value::as_str);
            label_instruction(program_id, parsed_type, data_bytes)
        })
        .collect();

    (labels, cu_limit, cu_price)
}
