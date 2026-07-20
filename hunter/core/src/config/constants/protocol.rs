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
/// Circle USDC (Solana mainnet) — wallet **cash** / dry powder, not a meme position.
/// Face value is 1 USD per UI unit (`USDC_DECIMALS`).
pub const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
/// USDC mint decimals (raw units → UI).
pub const USDC_DECIMALS: u8 = 6;

// CU cost-model constants (duplicated with `pump_trader::constants`) — read by
// `lab`'s sweep cost model via `trading_core::config::constants`.
/// Compute-unit price in micro-lamports (priority-fee rate per CU).
pub const COMPUTE_UNIT_PRICE_MICRO_LAMPORTS: u64 = 200_000;
/// Curve buy CU limit (measured p95 × 1.2).
pub const COMPUTE_UNIT_LIMIT_CURVE_BUY: u32 = 150_000;
/// Curve sell CU limit (measured p95 × ~1.15).
pub const COMPUTE_UNIT_LIMIT_CURVE_SELL: u32 = 100_000;

