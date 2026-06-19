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
/// Hard floor applied to any client-supplied slippage. A `0` (or near-zero)
/// tolerance means the trade reverts on any price movement at all — on a
/// volatile meme coin that is a guaranteed revert + wasted base/priority fee,
/// so an explicit sub-floor value is raised to this minimum. 10 bps = 0.1%.
pub const SLIPPAGE_MIN_BPS: u64 = 10;

/// Per-trade SOL ceiling enforced on the manual buy API (`POST /api/solana/wallet/buy`).
/// The request body carries `sol_amount` as a raw `f64` that becomes a real spend,
/// so a fat-finger ("buy 1000 SOL") or hostile value is rejected with a 400 before
/// any on-chain work. This is the operator-facing business limit — tune it to the
/// trading wallet's size; the `pump_trader` crate enforces its own (more generous)
/// `MAX_BUY_SOL` backstop one layer down regardless.
pub const MAX_MANUAL_BUY_SOL: f64 = 5.0;

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

/// The bonding-curve **genesis spot price** in the same units as a trade's
/// `price_per_token` — whole SOL per raw token unit (token amounts stay raw in the
/// decoder, SOL is scaled to whole). Constant for every Pump.fun mint:
/// `virtual_sol / virtual_token` at curve genesis. Used as the dead-token launch
/// baseline when a token has no recorded dev buy, so a no-dev-buy mint is still
/// evaluated against a real floor instead of being silently immune to deadness.
pub const PUMPFUN_GENESIS_PRICE_PER_RAW_TOKEN: f64 =
    INITIAL_VIRTUAL_SOL_RESERVES / LAMPORTS_PER_SOL as f64 / INITIAL_VIRTUAL_TOKEN_RESERVES;

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

/// Early-buyer cohort window. Wallets that bought within this many slots of a
/// token's first trade are treated as the launch sniper / bundler cluster (Solana
/// slots are ≈400 ms, so ~150 ≈ the first minute). One window defines the cluster
/// across the tpsl2 entry gates and the cohort-dump exit — see `cohort.rs`.
pub const EARLY_COHORT_SLOT_WINDOW: i64 = 150;

// ── Dead-token detection ─────────────────────────────────────────────────────
// A token is "dead" when BOTH conditions hold simultaneously:
//   1. Real SOL reserves are below `DEAD_MAX_LIQUIDITY_SOL` — liquidity is gone.
//   2. No meaningful trade (≥ `DEAD_MEANINGFUL_TRADE_SOL`) has arrived for at least
//      `DEAD_QUIET_SECS` — activity has permanently ceased.
// This two-condition design is durable: the quiet requirement means a token that
// temporarily dips in reserves during a trough but then recovers will NOT be flagged
// dead (a new meaningful trade resets the quiet clock). The verdict flips to true
// exactly once and stays there. All reads are from in-memory `TokenState` — no DB.

/// Signal 1 — liquidity is gone. The latest `real_sol_reserves` is below this many
/// SOL. Spoof-proof anchor: a buy adds SOL, the matching wash-sell removes it, net≈0.
pub const DEAD_MAX_LIQUIDITY_SOL: f64 = 1.0;

/// Signal 2 — no meaningful trade for this many seconds. A "meaningful trade" is one
/// with `sol_amount >= DEAD_MEANINGFUL_TRADE_SOL`; tiny probe/dust transactions are
/// ignored so they cannot keep a dead token artificially alive. Falls back to
/// `token.created_at` when no meaningful trade has ever arrived.
pub const DEAD_QUIET_SECS: i64 = 120; // 2 minutes

/// SOL amount threshold below which a trade is considered dust and does NOT reset the
/// quiet timer used by `DEAD_QUIET_SECS`. Prevents bot noise from keeping dead tokens
/// alive indefinitely.
pub const DEAD_MEANINGFUL_TRADE_SOL: f64 = 0.1;

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

/// Upper bound on how many tokens the startup cache seed pulls from the
/// (continuously growing) `tokens` table. The seed loads the most-recent
/// `SEED_TOKEN_LIMIT` tokens (newest first) instead of the whole table, so
/// startup time + memory stay bounded as `tokens` grows without limit. Old,
/// long-dead memecoins beyond this window simply aren't tracked live until they
/// trade again (the live path ignores trades for untracked mints, exactly as it
/// does for any cache miss); raise it to keep more history warm at startup.
pub const SEED_TOKEN_LIMIT: i64 = 100_000;

/// Keyset page size for the analysis scans (tpsl matched / simulate) that stream
/// the **whole** `tokens` table one page at a time instead of resident-cache-only.
/// Memory per page = page rows × `Token` (transient — dropped once the page is
/// filtered to its sparse matches), so this trades round-trips against peak
/// per-page heap. See `TokenRepo::find_page_before`.
pub const ANALYSIS_SCAN_PAGE: i64 = 5_000;

/// Only tokens created within this window are pulled into the startup cache seed
/// (on top of the `SEED_TOKEN_LIMIT` cap). The seed's dominant cost is scanning
/// those mints' trade history, so excluding the long tail of long-dead memecoins
/// — which won't trade live again — keeps cold start proportional to *recent*
/// activity instead of total history. Tokens older than this simply aren't tracked
/// live until they trade again (the live path ignores untracked mints), EXCEPT any
/// mint with an unsettled position, which is always seeded regardless of age so an
/// open exit never strands. Raise it to keep more history warm at higher cold-start
/// cost.
pub const SEED_ACTIVITY_WINDOW_DAYS: i64 = 7;

/// Per-mint cap on trade history pulled into the cache at seed time. Matches the
/// live retained-history cap (`MAX_TRADES_RETAINED`), so a high-volume token reads
/// only its newest window at startup instead of its full (unbounded) history.
pub const SEED_TRADES_PER_MINT: i64 = crate::state::token_cache::MAX_TRADES_RETAINED as i64;

/// How often the runtime token-cache eviction sweep runs. The cache grows one
/// entry per created mint, so without a periodic prune it would climb without
/// bound for the life of the process. Growth is gradual (driven by token
/// creations), so a coarse interval keeps the per-pass `DashMap` walk well clear
/// of the hot path while still bounding resident size.
pub const TOKEN_CACHE_EVICT_INTERVAL_SECONDS: u64 = 300; // 5 minutes

/// A tracked token that has been inactive for at least this long — no trade,
/// falling back to its creation time for a mint that never traded — and has no
/// open position is evicted from the in-memory cache. This bounds the live
/// process's heap: Pump.fun mints thousands of tokens/day, the vast majority of
/// which die within minutes, and every one would otherwise stay resident forever.
/// An evicted mint behaves exactly like any untracked mint — the live path
/// ignores its trades (cache miss) until a manual re-sync re-seeds it, the same
/// contract as `SEED_ACTIVITY_WINDOW_DAYS`. A mint with an open paper/real
/// position is always exempt regardless of idleness, so an open exit (whose
/// confirm loop reads this cache) never strands.
pub const TOKEN_CACHE_EVICT_IDLE_SECONDS: i64 = 2 * 3600; // 2 hours

/// How often the background task refreshes the DB-backed base of the token-list
/// snapshot (`tokens LEFT JOIN tokens_info`, windowed by `SEED_ACTIVITY_WINDOW_DAYS`
/// + capped at `SEED_TOKEN_LIMIT`). This base lets `GET /api/tokens` reflect the
/// whole seeded universe — including mints already evicted from the live cache by
/// `TOKEN_CACHE_EVICT_IDLE_SECONDS` — overlaid with live cache stats for tracked
/// mints. Coarse on purpose: the base only matters for idle/evicted mints (whose
/// stats are static), since tracked mints are always superseded by the live
/// overlay, so one bounded query per minute is plenty and keeps DB load trivial.
pub const TOKEN_LIST_DB_REFRESH_SECS: u64 = 60;




/// Trades below this size are dust (bot noise / probe txs) and are not ingested.
pub const MIN_TRADE_LAMPORTS: u64 = 10_000;
pub const MIN_TRADE_SOL: f64 = MIN_TRADE_LAMPORTS as f64 / LAMPORTS_PER_SOL as f64;
