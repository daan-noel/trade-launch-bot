//! All constants used by the trader. Protocol addresses and unit conversions
//! plus the trader's behaviour/tuning values — hardcoded so the crate is fully
//! self-contained (no external config needed for these).

// ---------------------------------------------------------------------------
// Protocol program IDs / unit conversions (fixed on-chain values)
// ---------------------------------------------------------------------------

pub const ASSOCIATED_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
pub const EVENT_AUTHORITY: &str = "Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1";
pub const FEE_PROGRAM_ID: &str = "pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ";
pub const LAMPORTS_PER_SOL: u64 = 1_000_000_000;
pub const PUMP_FUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

/// Classic SPL Token program.
pub const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
/// Token-2022 / Token Extensions program.
pub const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";

/// PumpSwap (pump_amm) program — handles trading once a token has migrated off
/// the bonding curve. Used for buy/sell of migrated tokens.
pub const PUMP_SWAP_PROGRAM_ID: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
/// Wrapped SOL mint — the quote mint for PumpSwap pools.
pub const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";

/// Fixed pfee account that cashback-enabled PumpSwap pools append in the
/// trailing fee block, just before the buyback recipient pair. Constant across
/// on-chain cashback swaps; has no account data of its own (a program marker).
/// Only present when the pool's `is_cashback_coin` flag is set.
pub const PUMP_AMM_CASHBACK_GLOBAL: &str = "5817UmPM7KLKu2mSQVsXMJ7X2rr3PtEb3j4EioyoJgd1";

/// Buyback fee recipient appended to every PumpSwap swap (the deployed program
/// is newer than its published IDL). This is `buyback_fee_recipients[0]` from
/// the on-chain global config; the program accepts any whitelist member.
pub const PUMP_PROGRAM_UPGRADE_FEE_RECIPIENT: &str = "5YxQFdt3Tr9zJLvkFccqXVUwhdTWJQc1fFg2YPbxvxeD";

/// Jito tip accounts (mainnet); one is chosen at random per trader instance.
pub const JITO_TIP_ACCOUNTS: &[&str] = &[
    "9bnz4RShgq1hAnLnZbP8kbgBg1kEmcJBYQq3gQbmnSta",
    "4TQLFNWK8AovT1gFvda5jfw2oJeRMKEmw7aH6MGBJ3or",
    "2nyhqdwKcJZR2vcqCyrYsaPVdAnFoJjiksCXJ7hfEYgD",
    "wyvPkWjVZz1M8fHQnMMCDTQDbkManefNNhweYk5WkcF",
    "D2L6yPZ2FmmmTKPgzaMKdhu6EWZcTpLy1Vhx8uvZe7NZ",
    "3KCKozbAaF75qEU33jtzozcJ29yJuaLJTy2jFdzUY8bT",
    "2q5pghRs6arqVjRvT5gfgWfWcHWmw1ZuCzphgd5KfWGJ",
    "5VY91ws6B2hMmBFRsXkoAAdsPHBJwRfBht4DXox3xkwn",
    "4vieeGHPYPG2MmyPRcYjdiDmmhN3ww7hsFNap8pVN3Ey",
    "D1Mc6j9xQWgR1o1Z7yU5nVVXFQiAYx7FG9AW1aVfwrUM",
    "4ACfpUFoaSD9bfPdeu6DBt89gB6ENTeHBXCAi87NhDEE",
];

// ---------------------------------------------------------------------------
// Trading behaviour / tuning (hardcoded)
// ---------------------------------------------------------------------------


/// How many buy templates to keep pre-built per token-program pool.
pub const BUY_SEED_POOL_SIZE: usize = 16;

/// Minimum Jito tip per transaction, in SOL.
pub const MIN_JITO_TIP_SOL: f64 = 0.0002;

/// Compute-unit price in micro-lamports (the priority-fee rate; passed to `set_compute_unit_price` on every trade tx).
pub const COMPUTE_UNIT_PRICE_MICRO_LAMPORTS: u64 = 1_000_000;
/// Compute-unit limit set on every trade transaction.
pub const COMPUTE_UNIT_LIMIT: u32 = 200_000;


/// How many times `sell_token` retries before giving up.
pub const MAX_SELL_ATTEMPTS: usize = 5;
/// How many times we poll for transaction confirmation.
pub const CONFIRM_MAX_RETRIES: usize = 5;
/// Milliseconds between confirmation polls.
pub const CONFIRM_POLL_MS: u64 = 1_000;


/// Maximum spin-wait iterations when all nonce slots are in use.
pub const NONCE_MAX_WAIT_ITERS: usize = 200;
/// Sleep between nonce spin-wait iterations, in milliseconds.
pub const NONCE_WAIT_SLEEP_MS: u64 = 20;

/// Max age of a WS-fed reserve snapshot still trusted on the trade path before
/// falling back to an on-chain read. Within this window the cached reserve is as
/// fresh as an RPC would be (and only goes "stale" when the token is quiet, in
/// which case the reserve hasn't changed). Beyond it — e.g. after a WS gap — the
/// trade path re-reads on-chain. 3 s.
pub const RESERVE_CACHE_MAX_AGE_MS: u64 = 3_000;


// ---------------------------------------------------------------------------
// PumpSwap AMM (migrated tokens)
// ---------------------------------------------------------------------------

/// Default AMM slippage tolerance, in basis points, applied to the computed
/// `min_out` when the caller doesn't specify one. 500 bps = 5%.
pub const AMM_DEFAULT_SLIPPAGE_BPS: u64 = 500;

/// Conservative fee allowance (basis points) subtracted from the bonding-curve
/// quote before applying slippage, when computing a curve buy/sell `min_out`.
/// The deployed curve charges protocol + creator (+ dynamic) fees we don't read
/// off-chain here; over-estimating them keeps `min_out` a safe lower bound so a
/// fee misestimate never causes a false slippage failure (only looser
/// protection). 200 bps comfortably covers the ~1% standard curve fee.
pub const CURVE_FEE_BUFFER_BPS: u128 = 200;

/// Byte offsets into a PumpSwap `Pool` account (after the 8-byte Anchor
/// discriminator). Layout: pool_bump(u8) index(u16) creator(32) base_mint(32)
/// quote_mint(32) lp_mint(32) pool_base_token_account(32)
/// pool_quote_token_account(32) lp_supply(u64) coin_creator(32)
/// is_mayhem_mode(bool) is_cashback_coin(bool).
pub const AMM_POOL_BASE_VAULT_OFFSET: usize = 139;
pub const AMM_POOL_QUOTE_VAULT_OFFSET: usize = 171;
pub const AMM_POOL_COIN_CREATOR_OFFSET: usize = 211;
pub const AMM_POOL_IS_CASHBACK_OFFSET: usize = 244;
pub const AMM_POOL_MIN_LEN: usize = 245;

/// Byte offsets into a PumpSwap `GlobalConfig` account (after the 8-byte
/// discriminator). Layout: admin(32) lp_fee_basis_points(u64)
/// protocol_fee_basis_points(u64) disable_flags(u8)
/// protocol_fee_recipients([pubkey;8]) coin_creator_fee_basis_points(u64) ...
pub const AMM_CONFIG_LP_FEE_BPS_OFFSET: usize = 40;
pub const AMM_CONFIG_PROTOCOL_FEE_BPS_OFFSET: usize = 48;
pub const AMM_CONFIG_FEE_RECIPIENTS_OFFSET: usize = 57;
pub const AMM_CONFIG_COIN_CREATOR_FEE_BPS_OFFSET: usize = 313;
pub const AMM_CONFIG_MIN_LEN: usize = 417;
