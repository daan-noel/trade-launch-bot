// On-chain program addresses for the backend.
//
// These protocol IDs / unit conversions are intentionally duplicated with
// `pump_trader::constants` (see the note there). `trading_core` keeps its own
// copy so it — and `lab` — can read them without depending on `pump-trader`
// (which pulls the full trading/RPC executor stack). Program IDs effectively
// never change; keep the two copies in sync if a value ever does.

pub const ASSOCIATED_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
pub const EVENT_AUTHORITY: &str = "Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1";
pub const FEE_PROGRAM_ID: &str = "pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ";
pub const LAMPORTS_PER_SOL: u64 = 1_000_000_000;
pub const PUMP_FUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
/// Classic SPL Token program.
pub const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
/// Token-2022 / Token Extensions program.
pub const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
/// PumpSwap (pump_amm) program — trading once a token has migrated off the curve.
pub const PUMP_SWAP_PROGRAM_ID: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
/// Wrapped SOL mint — the quote mint for PumpSwap pools.
pub const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";

// CU cost-model constants (duplicated with `pump_trader::constants`) — read by
// `lab`'s sweep cost model via `trading_core::config::constants`.
/// Compute-unit price in micro-lamports (priority-fee rate per CU).
pub const COMPUTE_UNIT_PRICE_MICRO_LAMPORTS: u64 = 200_000;
/// Curve buy CU limit (measured p95 × 1.2).
pub const COMPUTE_UNIT_LIMIT_CURVE_BUY: u32 = 150_000;
/// Curve sell CU limit (measured p95 × ~1.15).
pub const COMPUTE_UNIT_LIMIT_CURVE_SELL: u32 = 100_000;

// Backend-only program IDs — not needed by the trader crate.
pub const SYSTEM_PROGRAM_ID: &str = "11111111111111111111111111111111";
pub const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";

/// Address Lookup Table program.
pub const ADDRESS_LOOKUP_TABLE_PROGRAM_ID: &str = "AddressLookupTab1e1111111111111111111111111";

/// Known aggregator / bot program IDs — labelled by `program_friendly_name`.
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
