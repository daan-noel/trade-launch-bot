// Protocol constants shared with the standalone trader are owned by the
// `pump_trader` crate; re-export them so the backend keeps a single source.
pub use pump_trader::constants::{
    ASSOCIATED_TOKEN_PROGRAM_ID, EVENT_AUTHORITY, FEE_PROGRAM_ID, LAMPORTS_PER_SOL,
    PUMP_FUN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID,
};

// Backend-only program IDs.
pub const SYSTEM_PROGRAM_ID: &str = "11111111111111111111111111111111";
pub const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";

/// Address Lookup Table program.
pub const ADDRESS_LOOKUP_TABLE_PROGRAM_ID: &str = "AddressLookupTab1e1111111111111111111111111";

/// Pump.fun graduation AMM (PumpSwap).
pub const PUMP_SWAP_PROGRAM_ID: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
/// Wrapped SOL — the quote mint for every canonical (migrated) PumpSwap pool.
pub const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
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
pub const CREATE_V2_INSTRUCTION_DISCRIMINATOR: [u8; 8] =
    [0xd6, 0x90, 0x4c, 0xec, 0x5f, 0x8b, 0x31, 0xb4];
/// Pump.fun `migrate` instruction — migrates liquidity to pump_amm when bonding curve completes.
pub const MIGRATE_INSTRUCTION_DISCRIMINATOR: [u8; 8] =
    [0x9b, 0xea, 0xe7, 0x92, 0xec, 0x9e, 0xa2, 0x1e];
/// Pump.fun `migrate_v2` instruction — current migration path (replaced `migrate`).
/// sha256("global:migrate_v2")[..8]
pub const MIGRATE_V2_INSTRUCTION_DISCRIMINATOR: [u8; 8] =
    [0xbb, 0xcb, 0x12, 0x1f, 0xce, 0xed, 0xfe, 0x29];

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
pub const BUY_V2_DISCRIMINATOR: [u8; 8] = [0xb8, 0x17, 0xee, 0x61, 0x67, 0xc5, 0xd3, 0x3d];
/// `buy_exact_quote_in_v2` — new unified buy interface, exact quote in.
/// sha256("global:buy_exact_quote_in_v2")[..8]
pub const BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR: [u8; 8] =
    [0xc2, 0xab, 0x1c, 0x46, 0x68, 0x4d, 0x5b, 0x2f];

// ── Trade slippage ───────────────────────────────────────────────────────────
/// Default trade slippage tolerance in basis points (100 = 1%) when neither the
/// request nor the persisted `AppSettings.slippage_bps` specifies one. 500 = 5%.
pub const DEFAULT_SLIPPAGE_BPS: u64 = 500;
/// Hard ceiling applied to any client-supplied slippage, to guard against a
/// fat-finger or hostile value. 5000 bps = 50%.
pub const SLIPPAGE_MAX_BPS: u64 = 5_000;

// ---------------------------------------------------------------------------
// On-chain event discriminators (emitted via `emit!` in "Program data:" logs)
// ---------------------------------------------------------------------------

/// TradeEvent discriminator — matches the CPI event data for every buy/sell.
/// "Program data:" log entries that start with these 8 bytes carry a full
/// RawTradeEvent (Borsh-encoded), including virtual/real reserves.
pub const TRADE_EVENT_DISCRIMINATOR: [u8; 8] = [0xbd, 0xdb, 0x7f, 0xd3, 0x4e, 0xe6, 0x61, 0xee];
/// Anchor `emit_cpi!` self-CPI tag. Pump.fun emits each event BOTH as an
/// `emit!` "Program data:" log AND as an `emit_cpi!` inner instruction to the
/// event authority. The inner-instruction data is this 8-byte tag followed by
/// the event's own discriminator + Borsh payload. Unlike logs, inner
/// instructions are never truncated by Solana, so they are the reliable source
/// when a transaction's logs are truncated.
pub const ANCHOR_EVENT_CPI_DISCRIMINATOR: [u8; 8] =
    [0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];
/// CreateEvent discriminator — emitted on every token creation via `emit!`.
pub const CREATE_EVENT_DISCRIMINATOR: [u8; 8] =
    [0x1b, 0x72, 0xa9, 0x4d, 0xde, 0xeb, 0x63, 0x76];

// ── PumpSwap (pump_amm) post-migration swap events ───────────────────────────
// Emitted via `emit!` in "Program data:" logs for every AMM buy/sell. Only the
// leading fields (amounts, pool, user) are read; trailing fields added in later
// program versions (e.g. coin_creator) are tolerated by Borsh deserialization.
/// PumpSwap `BuyEvent` discriminator.
pub const PUMP_SWAP_BUY_EVENT_DISCRIMINATOR: [u8; 8] =
    [103, 244, 82, 31, 44, 245, 119, 119];
/// PumpSwap `SellEvent` discriminator.
pub const PUMP_SWAP_SELL_EVENT_DISCRIMINATOR: [u8; 8] =
    [62, 47, 55, 10, 165, 3, 220, 42];

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

/// Total token supply (raw units) for a token, accounting for Mayhem-mode
/// tokens which are minted via `create_v2` with 2× the standard supply (2B vs
/// 1B). Use this anywhere FDV / market cap is computed as `supply × price`.
pub fn total_supply_for(is_mayhem_mode: bool) -> f64 {
    if is_mayhem_mode {
        TOKEN_TOTAL_SUPPLY * 2.0
    } else {
        TOKEN_TOTAL_SUPPLY
    }
}

/// How long a token can go without a price change before being considered rugged.
pub const RUGGED_STALE_SECONDS: i64 = 3600; // 1 hour

/// Minimum spacing between per-mint rugged recomputes on the ingest hot path. The
/// rugged verdict only changes on the `RUGGED_STALE_SECONDS` (1h) staleness scale,
/// so re-running its (up to 3) whole-history aggregate scans on every trade for a
/// stale-but-still-trading mint is pure waste — the ingest flow flags the
/// recompute at most once per this interval per mint instead.
pub const RUGGED_RECHECK_INTERVAL_SECONDS: i64 = 300; // 5 minutes

// ── Rug detection signals ───────────────────────────────────────────────────
// All signals below are gated behind `RUGGED_STALE_SECONDS`: an actively trading
// token is never flagged. They are evaluated in order and any one is sufficient.

/// Signal 1 — liquidity collapse. A stale token is rugged when its most recent
/// `real_sol_reserves` has fallen to this fraction of its all-time peak. Real
/// SOL reserves cannot be inflated by wash trading across many wallets (a buy
/// adds SOL, the matching wash-sell removes it, net ≈ 0), so this is the single
/// most spoof-proof signal and covers both curve and post-migration AMM rugs.
pub const RUGGED_RESERVE_DRAWDOWN_RATIO: f64 = 0.10;

/// Minimum peak `real_sol_reserves` (SOL) a token must have reached before the
/// liquidity-collapse signal applies. Tokens that never attracted real SOL are
/// "dead", not "rugged", and are left to the other signals.
pub const RUGGED_MIN_PEAK_SOL: f64 = 2.0;

/// Signal 2 — early-buyer cohort exit. Wallets that bought within this many slots
/// of a token's first trade are treated as the launch sniper / bundler cohort
/// (Solana slots are ≈400 ms, so ~150 ≈ the first minute).
pub const RUGGED_EARLY_SLOT_WINDOW: i64 = 150;

/// The early-buyer cohort signal only fires when that cohort controlled the
/// launch, i.e. its share of total buy volume is at least this fraction. Stops a
/// handful of tiny early buyers exiting from flagging an otherwise healthy token.
pub const RUGGED_COHORT_MIN_SHARE: f64 = 0.30;

/// The early-buyer cohort counts as having exited when its net holdings fall to
/// this fraction of everything it ever bought. Generalises the single-creator
/// dump check to the whole insider cluster, defeating multi-wallet spoofing.
pub const RUGGED_COHORT_EXIT_RATIO: f64 = 0.05;

/// A silence longer than this between consecutive trades marks the token going
/// quiet. Trailing trades after such a gap are stripped when computing a token's
/// active lifetime, so a lone late trade hours after death doesn't inflate it.
pub const LIFETIME_GAP_SECONDS: i64 = 600; // 10 minutes

/// A migrated token's PumpSwap pool is included in the live subscription set
/// only if it has traded within this window. Bounds the subscription to
/// recently-active pools instead of every token that ever graduated; quiet pools
/// are re-added when fresh activity appears (e.g. a manual sync refreshes
/// `last_trade_at`). Tune up to keep slower pools live, down to subscribe to
/// fewer accounts.
pub const POOL_SUBSCRIBE_ACTIVITY_WINDOW_SECONDS: i64 = 6 * 3600; // 6 hours

/// How often the pool-subscription refresh re-evaluates token liveness and
/// subscribes the pools of migrated tokens that have become active since.
pub const POOL_REFRESH_INTERVAL_SECONDS: u64 = 120; // 2 minutes





/// Trades below this size are dust (bot noise / probe txs) and are not ingested.
pub const MIN_TRADE_LAMPORTS: u64 = 10_000;
pub const MIN_TRADE_SOL: f64 = MIN_TRADE_LAMPORTS as f64 / LAMPORTS_PER_SOL as f64;
