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

// ---------------------------------------------------------------------------
// Cashback claim discriminators (Anchor sha256("global:<name>")[..8]) — read
// from the pump-fun/pump-public-docs IDLs. Used by the off-hot-path cashback
// sweep (trader/claim.rs), NEVER on a buy/sell.
// ---------------------------------------------------------------------------

/// `sync_user_volume_accumulator` — recompute the 30-day rolling window so
/// `cashback_earned` is current. Prepended before each claim.
pub const SYNC_UVA_DISC: [u8; 8] = [86, 31, 192, 87, 163, 87, 79, 238];
/// `claim_cashback` — WSOL variant used by the PumpSwap (AMM) program: 9
/// accounts, no associated-token-program slot.
pub const CLAIM_CASHBACK_DISC: [u8; 8] = [37, 58, 35, 126, 190, 53, 228, 197];
/// `claim_cashback_v2` — WSOL variant on the pump (curve) program: adds the
/// associated-token-program account vs the AMM layout.
pub const CLAIM_CASHBACK_V2_DISC: [u8; 8] = [122, 243, 204, 65, 94, 116, 29, 55];

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

/// Hard per-trade sanity ceiling on a buy, in SOL. The public buy entry points
/// take an `f64` straight from a caller (e.g. an API request), so this is the
/// crate's own last-line guard that an absurd value — a fat-finger, a hostile
/// request, or a garbage read — can never reach the on-chain spend, no matter
/// what the caller does (a huge `f64` otherwise saturates the lamports cast
/// toward `u64::MAX`). The *business* per-trade limit is enforced separately by
/// the API layer; this is deliberately generous, a backstop and not policy.
pub const MAX_BUY_SOL: f64 = 5.0;

// --- Dynamic Jito tip --------------------------------------------------------
// The tip is sized per-trade from Jito's live tip-floor feed (see jito_tip.rs),
// clamped to [MIN_JITO_TIP_SOL, MAX_JITO_TIP_SOL]. A static tip silently loses
// the auction when the floor spikes during hot launches.

/// Floor for the Jito tip, in SOL — also the fallback when the tip-floor feed is
/// cold or stale. (Jito's own auction minimum is 0.0001 SOL.)
pub const MIN_JITO_TIP_SOL: f64 = 0.0002;

/// Ceiling for the Jito tip, in SOL — a cost guardrail so a spiking feed (or a
/// bad read) can never tip more than this on a single trade. Tune to taste.
pub const MAX_JITO_TIP_SOL: f64 = 0.005;

/// Which landed-tip percentile from the feed to target: one of 25 | 50 | 75 | 95
/// | 99. Higher lands in hotter auctions but costs more; 75 is a sane sniping
/// default. (Anything else falls back to the 75th percentile.) This is the tip
/// for the FIRST attempt; successive sell retries climb the ladder (see below).
pub const JITO_TIP_PERCENTILE: u8 = 75;

/// Per-retry Jito tip escalation. A sell that lost the auction simply didn't
/// land — and a non-landing tx costs nothing — so each retry bids up to win the
/// next block instead of re-sending the same losing tip. The ladder climbs the
/// live auction: level 0 = `JITO_TIP_PERCENTILE`, 1 = p95, 2 = p99; beyond p99
/// (and, when the tip-floor feed is cold, from the floor) the tip multiplies by
/// this factor per extra level. Always clamped to [MIN_JITO_TIP_SOL,
/// MAX_JITO_TIP_SOL], so the ceiling stays the hard per-trade cost guardrail.
pub const JITO_TIP_ESCALATION_TAIL_MULT: f64 = 1.5;

/// Jito tip-floor REST feed (landed-tip percentiles; values in SOL).
pub const JITO_TIP_FLOOR_URL: &str = "https://bundles.jito.wtf/api/v1/bundles/tip_floor";

/// How often to re-fetch the tip-floor feed, in milliseconds.
pub const JITO_TIP_FLOOR_REFRESH_MS: u64 = 3_000;

/// Max age of a cached tip-floor read before the trade path falls back to the
/// floor, in milliseconds.
pub const JITO_TIP_FLOOR_MAX_AGE_MS: u64 = 30_000;

/// Compute-unit price in micro-lamports — the priority-fee rate, passed to
/// `set_compute_unit_price` on every trade tx. Priority fee = this × the CU
/// limit (below), charged on every included tx. On the Helius Sender path
/// inclusion is driven mainly by the Jito tip (the Jito leg), not this priority
/// fee (which only orders the staked-validator/SWQOS leg), so this can run well
/// below the old 1_000_000 (1 lamport/cu) and lean on the dynamic tip. Lowered
/// to 200_000 (0.2 lamport/cu) to cut the per-tx fee ~5×; A/B land-rate and
/// raise if inclusion suffers.
pub const COMPUTE_UNIT_PRICE_MICRO_LAMPORTS: u64 = 200_000;

// --- Compute-unit limits (priority-fee sizing), set per trade path ----------
// The priority fee you pay is `COMPUTE_UNIT_PRICE_MICRO_LAMPORTS × <limit>` on
// *every* included tx — success OR on-chain revert — and it's based on the
// limit *requested*, not the units actually consumed. So an over-large limit
// silently overpays on every single trade. These are split by path because a
// curve buy/sell is far lighter than a ~27-account AMM swap; a single shared
// limit had to size for the heaviest path and made the common curve trades
// overpay.
//
// Values below are sized from MEASURED on-chain consumption (getTransaction
// `computeUnitsConsumed` over ~120 recent landed wallet txs, 2026-06-11) at
// ≈ p95 × 1.2. Each path also has rare heavy outliers (one curve buy at 333k,
// one curve sell at 247k) that DO exhaust these limits and revert — deliberately
// not covered, since sizing every trade for a 1-in-15 outlier would inflate the
// priority fee on every normal trade. Keep real headroom: a CU-exhausted tx
// still pays the full fee and then reverts (pure waste), so erring high is
// cheaper than erring low.
/// Curve buy (may include create-with-seed + initialize_account3 + buy).
/// Measured landed: p50 112k, p95 124k, normal-max 124k → 124k×1.2 ≈ 150k.
pub const COMPUTE_UNIT_LIMIT_CURVE_BUY: u32 = 150_000;
/// Curve sell (sell + tip; no account creation).
/// Measured landed: p50 78k, p95 87k, normal-max 87k → ~1.15× headroom.
pub const COMPUTE_UNIT_LIMIT_CURVE_SELL: u32 = 100_000;
/// PumpSwap AMM swap (heaviest path — many accounts, CPIs, wSOL wrap/unwrap);
/// shared by AMM buy and sell. Measured landed max: buy 142k, sell 133k →
/// 142k×1.27 ≈ 180k. (Sample is thin and excludes the heaviest cashback swaps,
/// so kept a touch above max×1.2.) Tightened from 200k.
pub const COMPUTE_UNIT_LIMIT_AMM: u32 = 180_000;


/// How many times `sell_token` retries before giving up.
pub const MAX_SELL_ATTEMPTS: usize = 5;
/// How many times we poll for transaction confirmation.
pub const CONFIRM_MAX_RETRIES: usize = 5;
/// Fallback / steady-state gap between confirmation polls, in milliseconds —
/// used once the ramp below is exhausted.
pub const CONFIRM_POLL_MS: u64 = 1_000;
/// Ramped gaps between the first confirmation polls, in milliseconds. A
/// confirmed-commitment tx usually lands in ~1–2 slots (~0.8–1.6 s), so polling
/// fast early returns a manual buy/sell sooner in the common case; later polls
/// widen toward `CONFIRM_POLL_MS`. Polls beyond this list fall back to
/// `CONFIRM_POLL_MS`. Worst-case wait ≈ sum of the first `CONFIRM_MAX_RETRIES-1`
/// entries, below the old flat `(retries-1) × 1 s`.
pub const CONFIRM_POLL_SCHEDULE_MS: &[u64] = &[250, 400, 700, 1_000];


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

/// Background refresh interval for the recent-blockhash cache, in milliseconds.
pub const BLOCKHASH_REFRESH_MS: u64 = 2_000;
/// Max age of a cached recent blockhash still used on the AMM buy path before a
/// fresh fetch. Well inside a blockhash's ~60-90 s validity, so a built tx never
/// rides an expired hash even if the refresher briefly stalls.
pub const BLOCKHASH_CACHE_MAX_AGE_MS: u64 = 10_000;


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
