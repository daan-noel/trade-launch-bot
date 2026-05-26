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
// Source: Official Pump.fun IDL (https://github.com/PumpFunOfficial)
// ---------------------------------------------------------------------------

// ── Core Trading Instructions ─────────────────────────────────────────────
/// Pump.fun bonding-curve `buy` instruction — buy tokens from a bonding curve.
pub const BUY_DISCRIMINATOR: [u8; 8] = [0x66, 0x06, 0x3d, 0x12, 0x01, 0xda, 0xeb, 0xea];
/// Pump.fun bonding-curve `sell` instruction — sell tokens into a bonding curve.
pub const SELL_DISCRIMINATOR: [u8; 8] = [0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad];

// ── Token Creation & Lifecycle ───────────────────────────────────────────
/// Pump.fun `create` instruction — creates a new coin and bonding curve.
pub const CREATE_INSTRUCTION_DISCRIMINATOR: [u8; 8] =
    [0x18, 0x1e, 0xc8, 0x28, 0x05, 0x1c, 0x07, 0x77];
/// Pump.fun `create_v2` instruction — creates a new coin and bonding curve (v2 variant).
pub const CREATE_V2_INSTRUCTION_DISCRIMINATOR: [u8; 8] = [0xd6, 0x90, 0x4c, 0xec, 0x5f, 0x8b, 0x31, 0xb4];
/// Pump.fun `migrate` instruction — migrates liquidity to pump_amm when bonding curve completes.
pub const MIGRATE_INSTRUCTION_DISCRIMINATOR: [u8; 8] =
    [0x9b, 0xea, 0xe7, 0x92, 0xec, 0x9e, 0xa2, 0x1e];

// ── Buy Variants (all collapse to InstructionKind::Buy in the decoder) ───
/// `buy_exact_sol_in` — specify exact SOL in, receive tokens out.
/// sha256("global:buy_exact_sol_in")[..8]
pub const BUY_EXACT_SOL_IN_DISCRIMINATOR: [u8; 8] =
    [0x38, 0xfc, 0x74, 0x08, 0x9e, 0xdf, 0xcd, 0x5f];
/// `buy_exact_quote_in` — original second variant, still active.
/// sha256("global:buy_exact_quote_in")[..8]
pub const BUY_EXACT_QUOTE_IN_DISCRIMINATOR: [u8; 8] =
    [0xc6, 0x2e, 0x15, 0x52, 0xb4, 0xd9, 0xe8, 0x70];
/// `buy_v2` — new unified buy interface.
/// sha256("global:buy_v2")[..8]
pub const BUY_V2_DISCRIMINATOR: [u8; 8] =
    [0xb8, 0x17, 0xee, 0x61, 0x67, 0xc5, 0xd3, 0x3d];
/// `buy_exact_quote_in_v2` — new unified buy interface, exact quote in.
/// sha256("global:buy_exact_quote_in_v2")[..8]
pub const BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR: [u8; 8] =
    [0xc2, 0xab, 0x1c, 0x46, 0x68, 0x4d, 0x5b, 0x2f];

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
