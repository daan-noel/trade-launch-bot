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

/// Pump.fun program-upgrade fee recipient (an account on every trade tx).
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
