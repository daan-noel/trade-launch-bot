//! Instruction classification and labeling.

use borsh::BorshDeserialize;

use crate::protocol::Protocol;

use super::program_registry::{program_friendly_name, program_instruction_label, MEMO_PROGRAM_ID};

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
    } else if bytes.starts_with(&d.sell) || bytes.starts_with(&d.sell_v2) {
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

/// The three fee facts one transaction carries, accumulated while its instruction
/// list is walked once.
///
/// Kept together because they are one decision read off three places: an operator
/// sets a *priority spend*, and the chain lets them pay it on the compute rail
/// (`cu_limit x cu_price`), on a tip rail (a transfer), or on both. Reading one
/// without the others reads a fraction of the number the sender chose.
///
/// Every field is `Option` and every `None` means **not present in this
/// transaction**, never zero — the same convention `Trade::fee_lamports` uses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct FeeBudget {
    /// `TransactionStatusMeta.fee` — base signature fee plus the compute rail's
    /// priority fee, as the chain charged it. Read from meta rather than recomputed
    /// from the two `cu_*` fields: those are what the sender ASKED for, this is what
    /// was taken.
    pub fee_lamports: Option<u64>,
    /// `SetComputeUnitLimit` argument, in compute units.
    pub cu_limit: Option<u64>,
    /// `SetComputeUnitPrice` argument, in MICRO-lamports per compute unit. Not a
    /// lamport count: the compute-rail spend is `cu_limit * cu_price / 1e6`.
    pub cu_price: Option<u64>,
    /// Lamports transferred to a known tip account.
    ///
    /// `None` = the transaction carries no system transfer at all. `Some(0)` = it
    /// carries transfers but none reached an account in
    /// [`Protocol::is_tip_account`](crate::protocol::Protocol::is_tip_account) —
    /// either pure router rake, or a tip rail that list does not know yet. The two
    /// are deliberately distinguishable so the second can be counted and the list
    /// grown from evidence.
    pub tip_lamports: Option<u64>,
}

impl FeeBudget {
    /// Read a compute-budget instruction, if this is one.
    pub(super) fn note_compute_budget(&mut self, is_compute_budget: bool, data: Option<&[u8]>) {
        if !is_compute_budget {
            return;
        }
        if let Some(b) = data {
            let mut buf: &[u8] = b;
            match ComputeBudgetIx::deserialize(&mut buf) {
                Ok(ComputeBudgetIx::SetComputeUnitLimit(u)) => self.cu_limit = Some(u as u64),
                Ok(ComputeBudgetIx::SetComputeUnitPrice(p)) => self.cu_price = Some(p),
                _ => {}
            }
        }
    }

    /// Record a system transfer of `lamports`, `is_tip` iff its destination is a
    /// known tip account. Seeing the transfer at all is what lifts `tip_lamports`
    /// out of `None`; a non-tip transfer lifts it to `Some(0)` and adds nothing.
    pub(super) fn note_transfer(&mut self, lamports: u64, is_tip: bool) {
        let acc = self.tip_lamports.get_or_insert(0);
        if is_tip {
            *acc = acc.saturating_add(lamports);
        }
    }
}

/// Lamports moved by a `System Program: Transfer`, or `None` for any other
/// instruction.
///
/// Layout is bincode: a 4-byte little-endian variant tag (`2` = `Transfer`) then
/// the `u64` lamports. The tag is checked in full rather than by its low byte —
/// `system_ix_name` can afford the short read because it only names the thing,
/// while a wrong hit here would book some other instruction's bytes as money.
pub(super) fn system_transfer_lamports(is_system: bool, data: Option<&[u8]>) -> Option<u64> {
    if !is_system {
        return None;
    }
    let b = data?;
    if b.len() < 12 || b[..4] != [2, 0, 0, 0] {
        return None;
    }
    Some(u64::from_le_bytes(b[4..12].try_into().ok()?))
}

/// Produce a human-readable label for one instruction.
///
/// `parsed_type` is the jsonParsed `parsed.type`, used only when `data_bytes`
/// carries no usable discriminator. **It is `None` on every production path** and
/// cannot be otherwise: labeling runs on the protobuf message
/// (`decode::protobuf`), and the jsonParsed→protobuf conversion keeps `data` and
/// drops `parsed`. So a rebuild `convert::data_from_parsed` cannot cover degrades
/// to `Unknown` here — naming it means adding the arm THERE, not passing a type
/// in here. The parameter stays for the gRPC-free tests that exercise the
/// fallback, and for a future feed that labels before protobuf conversion.
pub(super) fn label_instruction(
    program_id: &str,
    parsed_type: Option<&str>,
    data_bytes: Option<&[u8]>,
    p: &Protocol,
) -> String {
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

    if program_id == MEMO_PROGRAM_ID {
        // A memo's instruction data IS its text, and that text is deliberately
        // NOT put in the label: memo payloads are per-transaction unique, so a
        // text label would make `ix_hash` unique per trade and dissolve every
        // fingerprint grouping built on it. The memo's PRESENCE is the signal
        // (it is a `m_flow_ix` marker); its content belongs to `decode-harvest`,
        // which reports the frequent payloads without persisting them per row.
        return match data_bytes {
            Some(b) if !b.is_empty() => "Memo Program: Memo".to_owned(),
            _ => "Memo Program: Unknown".to_owned(),
        };
    }

    // Name the instruction where the registry can prove a name, else carry its
    // stable key (`ix#…`) so two different instructions of one program stay two
    // different labels. `Unknown` survives only when the feed delivered no data.
    let ix = program_instruction_label(program_id, data_bytes);

    match program_friendly_name(program_id) {
        Some(name) => format!("{name}: {ix}"),
        // Carry the FULL program id (not a truncated suffix) so unknown programs
        // are self-identifying in the persisted `trades.ix_labels` — that column
        // is the only durable record of what ran (this deployment does not
        // persist raw_txs), and `unknown-programs` ranks these to grow the
        // registry. The instruction half is still named where we can name it:
        // knowing a program did `SellBondingCurvePercentage` does not require
        // knowing who owns it. Label arrays stay low-cardinality (ids repeat),
        // so columnar compression is unaffected.
        None => format!("Unknown ({program_id}): {ix}"),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::program_registry::MEMO_PROGRAM_ID;

    fn disc(name: &str) -> [u8; 8] {
        let d = solana_sdk::hash::hash(format!("global:{name}").as_bytes());
        let mut out = [0u8; 8];
        out.copy_from_slice(&d.to_bytes()[..8]);
        out
    }

    #[test]
    fn a_label_names_the_program_and_the_instruction_independently() {
        let p = Protocol::pump_fun();
        // Our own venue, through the same computed table as everyone else.
        assert_eq!(
            label_instruction(&p.programs.pump_fun.base58, None, Some(&disc("sell_v2")), &p),
            "Pump.Fun: SellV2",
        );
        // A named router.
        assert_eq!(
            label_instruction(
                "term9YPb9mzAsABaqN71A4xdbxHmpBNZavpBiQKZzN3",
                None,
                Some(&disc("route_open")),
                &p,
            ),
            "Terminal: RouteOpen",
        );
        // An UNNAMED program whose instruction we can still name — the whole
        // point of resolving the two halves separately.
        assert_eq!(
            label_instruction(
                "6Vo3245eszAb5wuqEMw8mGdbfRUdKbHhDHP5LcaGuTAB",
                None,
                Some(&disc("pump_swap_v3")),
                &p,
            ),
            "Unknown (6Vo3245eszAb5wuqEMw8mGdbfRUdKbHhDHP5LcaGuTAB): PumpSwapV3",
        );
    }

    /// `forge/launcher/src/service.rs` writes `"Pump.Fun: Create_v2"` and
    /// `"Pump.Fun: Create"` as literals when it stamps a launch it made itself.
    /// The two products must agree on the spelling or a forge-launched token
    /// stops matching hunter's creation fingerprints — this is the guard on the
    /// copy that cannot import this table.
    #[test]
    fn create_labels_match_the_strings_forge_writes() {
        let p = Protocol::pump_fun();
        let pump = &p.programs.pump_fun.base58;
        assert_eq!(
            label_instruction(pump, None, Some(&disc("create_v2")), &p),
            "Pump.Fun: Create_v2",
        );
        assert_eq!(
            label_instruction(pump, None, Some(&disc("create")), &p),
            "Pump.Fun: Create",
        );
    }

    #[test]
    fn an_unnameable_instruction_keeps_a_stable_identity() {
        let p = Protocol::pump_fun();
        // Axiom logs no instruction names; its two dispatch tags must still be
        // two different labels, or its buys and sells collapse into one string.
        let buyish = [0x00, 0xc0, 0x19, 0x81, 0x1d, 0, 0, 0, 0, 0];
        let sellish = [0x01, 0xc0, 0x19, 0x81, 0x1d, 0, 0, 0, 0, 0];
        assert_eq!(
            label_instruction("FLASHX8DrLbgeR8FcfNV1F5krxYcYMUdBkrP1EPBtxB9", None, Some(&buyish), &p),
            "Axiom Trade: ix#00",
        );
        assert_ne!(
            label_instruction("FLASHX8DrLbgeR8FcfNV1F5krxYcYMUdBkrP1EPBtxB9", None, Some(&sellish), &p),
            label_instruction("FLASHX8DrLbgeR8FcfNV1F5krxYcYMUdBkrP1EPBtxB9", None, Some(&buyish), &p),
        );
        // The same payload read eight bytes wide would fork on the amount.
        let other_amount = [0x00, 0x2d, 0x31, 0x01, 0, 0, 0, 0, 0, 0];
        assert_eq!(
            label_instruction("FLASHX8DrLbgeR8FcfNV1F5krxYcYMUdBkrP1EPBtxB9", None, Some(&other_amount), &p),
            "Axiom Trade: ix#00",
        );
    }

    #[test]
    fn unknown_means_the_feed_lost_the_data_and_nothing_else() {
        let p = Protocol::pump_fun();
        // This is the signal the 2026-08-25 blackout produced for four hours; it
        // must stay distinguishable from "we cannot name this instruction".
        assert_eq!(
            label_instruction(&p.programs.system.base58, None, None, &p),
            "System Program: Unknown",
        );
        assert_eq!(
            label_instruction("term9YPb9mzAsABaqN71A4xdbxHmpBNZavpBiQKZzN3", None, None, &p),
            "Terminal: Unknown",
        );
    }

    #[test]
    fn a_memo_is_named_but_its_text_never_reaches_the_label() {
        let p = Protocol::pump_fun();
        // Two different memos must produce the SAME label: memo payloads are
        // per-transaction unique, and `ix_hash` is built from these strings.
        let a = label_instruction(MEMO_PROGRAM_ID, None, Some(b"ref:abc123"), &p);
        let b = label_instruction(MEMO_PROGRAM_ID, None, Some(b"totally different"), &p);
        assert_eq!(a, "Memo Program: Memo");
        assert_eq!(a, b);
        assert_eq!(label_instruction(MEMO_PROGRAM_ID, None, None, &p), "Memo Program: Unknown");
    }

    #[test]
    fn sell_v2_classifies_as_a_sell() {
        let p = Protocol::pump_fun();
        assert_eq!(
            classify_pump_ix(true, Some(&disc("sell_v2")), &p),
            Some(InstructionKind::Sell),
        );
        assert_eq!(
            determine_instruction_type(&[InstructionKind::Sell], &[]),
            "Sell",
        );
    }
}
