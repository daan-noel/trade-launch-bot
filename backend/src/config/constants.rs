#![allow(dead_code)]

// Shared program ID constants used across trading modules
pub const PUMP_FUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
pub const SYSTEM_PROGRAM_ID: &str = "11111111111111111111111111111111";
pub const ASSOCIATED_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
pub const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";
/// Classic SPL Token program.
pub const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
/// Token-2022 / Token Extensions program.
pub const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
/// Address Lookup Table program.
pub const ADDRESS_LOOKUP_TABLE_PROGRAM_ID: &str = "AddressLookupTab1e1111111111111111111111111";
/// Pump.fun graduation AMM (PumpSwap).
pub const PUMP_SWAP_PROGRAM_ID: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
pub const ARBITRAGE_BOT_FADO9_ID: &str = "FAdo9NCw1ssek6Z6yeWzWjhLVsr8uiCwcWNUnKgzTnHe";
pub const ARBITRAGE_BOT_9ZZF9_ID: &str = "9Zzf9QqTy3TkyXysvJBsXyuRjda5aXCEJ9vXfL2HKSYv";
pub const AXIOM_TRADE_PROGRAM_ID: &str = "FLASHX8DrLbgeR8FcfNV1F5krxYcYMUdBkrP1EPBtxB9";
pub const PHOTON_PROGRAM_ID: &str = "BSfD6SHZigAfDWSjzD5Q41jw8LmKwtmjskPH9XW1mrRW";
pub const GMGN_BOT_PROGRAM_ID: &str = "GMgnVFR8Jb39LoXsEVzb3DvBy3ywCmdmJquHUy1Lrkqb";
pub const DFLOW_AGGREGATOR_V4_PROGRAM_ID: &str = "DF1ow4tspfHX9JwWJsAb9epbkA8hmpSEAtxXy1V27QBH";
pub const TERMINAL_FORMERLY_PADRE_PROGRAM_ID: &str = "term9YPb9mzAsABaqN71A4xdbxHmpBNZavpBiQKZzN3";
pub const TROJAN_TRADE_PROGRAM_ID: &str = "troyXT7Ty3s2rjJe4bqWaroUrS4Fjd8rbHHNHxcACF4";
pub const JUPITER_AGGREGATOR_V6_PROGRAM_ID: &str = "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4";
pub const BLOOM_ROUTER_PROGRAM_ID: &str = "b1oomGGqPKGD6errbyfbVMBuzSC8WtAAYo8MwNafWW1";
pub const METEORA_DAMM_V2_PROGRAM_ID: &str = "cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG";

// ---------------------------------------------------------------------------
// Instruction discriminators (first 8 bytes of Anchor-encoded instruction data)
// ---------------------------------------------------------------------------

/// Pump.fun bonding-curve `buy` instruction.
pub const BUY_DISCRIMINATOR: [u8; 8] = [0x66, 0x06, 0x3d, 0x12, 0x01, 0xda, 0xeb, 0xea];
/// Pump.fun `buyExactSolIn` instruction variant.
pub const BUY_EXACT_SOL_IN_DISCRIMINATOR: [u8; 8] =
    [0x38, 0xfc, 0x74, 0x08, 0x9e, 0xdf, 0xcd, 0x5f];
/// Pump.fun bonding-curve `sell` instruction.
pub const SELL_DISCRIMINATOR: [u8; 8] = [0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad];
/// Pump.fun AMM V2 `buyExactQuoteIn` instruction.
pub const BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR: [u8; 8] =
    [0x83, 0x54, 0x8d, 0x53, 0x58, 0x35, 0xe5, 0x2d];
/// Pump.fun Token-2022 `create_v2` instruction.
pub const CREATE_V2_INSTRUCTION_DISCRIMINATOR: [u8; 8] =
    [0xd6, 0x90, 0x4c, 0xec, 0x5f, 0x8b, 0x31, 0xb4];
/// Classic SPL Token `create` instruction.
pub const CREATE_INSTRUCTION_DISCRIMINATOR: [u8; 8] =
    [0x18, 0x1e, 0xc8, 0x28, 0x05, 0x1c, 0x07, 0x77];
/// `extendAccount` — storage growth instruction, not a trade.
pub const EXTEND_ACCOUNT_DISCRIMINATOR: [u8; 8] = [0xea, 0x66, 0xc2, 0xcb, 0x96, 0x48, 0x3e, 0xe5];
/// `migrate_bonding_curve_creator` instruction.
pub const MIGRATE_BONDING_CURVE_CREATOR_INSTRUCTION_DISCRIMINATOR: [u8; 8] =
    [4, 52, 191, 52, 38, 214, 232, 0];
/// `admin_set_creator` instruction.
pub const ADMIN_SET_CREATOR_INSTRUCTION_DISCRIMINATOR: [u8; 8] = [69, 25, 171, 142, 57, 239, 13, 4];

// ---------------------------------------------------------------------------
// On-chain event discriminators (emitted via `emit!` in "Program data:" logs)
// ---------------------------------------------------------------------------

/// TradeEvent discriminator — matches the CPI event data for every buy/sell.
/// "Program data:" log entries that start with these 8 bytes carry a full
/// RawTradeEvent (Borsh-encoded), including virtual/real reserves.
pub const TRADE_EVENT_DISCRIMINATOR: [u8; 8] = [0xbd, 0xdb, 0x7f, 0xd3, 0x4e, 0xe6, 0x61, 0xee];

// ---------------------------------------------------------------------------
// Human-readable program name lookup
// ---------------------------------------------------------------------------

/// Map a Solana program address to a short human-readable label.
/// Returns `None` for unrecognised programs.
pub fn program_friendly_name(program_id: &str) -> Option<&'static str> {
    match program_id {
        PUMP_FUN_PROGRAM_ID => Some("Pump.Fun"),
        COMPUTE_BUDGET_PROGRAM_ID => Some("Compute Budget"),
        SYSTEM_PROGRAM_ID => Some("System Program"),
        TOKEN_PROGRAM_ID => Some("Token Program"),
        ASSOCIATED_TOKEN_PROGRAM_ID => Some("Associated Token"),
        TOKEN_2022_PROGRAM_ID => Some("Token 2022"),
        ADDRESS_LOOKUP_TABLE_PROGRAM_ID => Some("Address Lookup Table"),
        PUMP_SWAP_PROGRAM_ID => Some("PumpSwap"),
        AXIOM_TRADE_PROGRAM_ID => Some("Axiom Trade"),
        PHOTON_PROGRAM_ID => Some("Photon"),
        GMGN_BOT_PROGRAM_ID => Some("GMGN Bot"),
        DFLOW_AGGREGATOR_V4_PROGRAM_ID => Some("DFlow Aggregator V4"),
        TERMINAL_FORMERLY_PADRE_PROGRAM_ID => Some("Terminal"),
        TROJAN_TRADE_PROGRAM_ID => Some("Trojan Trade"),
        JUPITER_AGGREGATOR_V6_PROGRAM_ID => Some("Jupiter Aggregator V6"),
        BLOOM_ROUTER_PROGRAM_ID => Some("Bloom Router"),
        METEORA_DAMM_V2_PROGRAM_ID => Some("Meteora DAMM V2"),
        ARBITRAGE_BOT_FADO9_ID | ARBITRAGE_BOT_9ZZF9_ID => Some("Arbitrage Bot"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Static initial reserve values for Pump.fun tokens
// These are the on-chain defaults observed for newly-created Pump.fun tokens
// and are used to compute circulating supply = initial_virtual_token_reserves - current_virtual_token_reserves
// Values are raw token units (no decimal scaling applied here).
pub const INITIAL_VIRTUAL_TOKEN_RESERVES: f64 = 1073000000000000.0;
pub const INITIAL_VIRTUAL_SOL_RESERVES: f64 = 30000000000.0;
pub const INITIAL_REAL_TOKEN_RESERVES: f64 = 793100000000000.0;
pub const TOKEN_TOTAL_SUPPLY: f64 = 1000000000000000.0;

/// How long a token can go without a price change before being considered rugged.
pub const RUGGED_STALE_SECONDS: i64 = 3600; // 1 hour
