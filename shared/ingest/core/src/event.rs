//! Crate-owned output event types. The host maps these to its own domain
//! models via `From` impls — the crate stays decoupled from any storage model.

use chrono::{DateTime, Utc};

// ── Top-level event enum ──────────────────────────────────────────────────────

#[derive(Debug)]
pub enum IngestEvent {
    TokenCreated(TokenCreated),
    Trade(Trade),
    TokenMigrated(TokenMigrated),
    Liquidity(LiquidityEvent),
    CreatorActivity(CreatorActivityEvent),
    #[cfg(feature = "raw-tx")]
    RawTx(RawTx),
}

// ── Trade ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Trade {
    pub mint: String,
    pub wallet: String,
    pub side: Side,
    pub sol: f64,
    /// Exact quote lamports for this trade — the raw on-chain `u64`, never routed
    /// through `f64`. Hosts whose quote is native SOL (9 decimals) persist this
    /// directly instead of re-multiplying `sol` back to base units.
    pub sol_lamports: u64,
    /// Raw token units — exact on-chain integer count (`u64`).
    pub tokens: u64,
    pub price: f64,
    pub signature: String,
    /// Position of this trade's transaction within its block (`info.index` from
    /// the LaserStream update). 0 on the RPC backfill path, which has no block
    /// position (see `backfill::rpc_to_protobuf`).
    pub tx_index: u32,
    pub leg_index: u32,
    pub slot: u64,
    pub block_time: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    pub reserves: Reserves,
    pub venue: Venue,
    pub instruction_type: String,
    pub instruction_labels: Vec<String>,
    /// AMM trades only: the fully resolved (ALT-included) account list of the
    /// **top-level** venue swap instruction this trade decoded from, in
    /// instruction order. Lets a host passively warm its swap-builder caches
    /// (pool/vault/fee accounts) with zero RPC. `None` for curve trades,
    /// inner-CPI-routed swaps, and every non-first leg of a multi-swap tx
    /// (attached once per pool per transaction). `Box` keeps the common
    /// (`None`) event size flat.
    pub amm_swap_accounts: Option<Box<Vec<String>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Venue {
    Curve,
    Amm,
}

#[derive(Debug, Clone, Default)]
pub struct Reserves {
    pub virtual_sol: Option<f64>,
    pub virtual_token: Option<u64>,
    pub real_sol: Option<f64>,
    pub real_token: Option<u64>,
    /// Exact raw-`u64` lamport mirrors of the SOL-side reserves above (the token
    /// sides are already exact `u64`). Carried so a native-SOL host persists the
    /// reserve without an `f64` round-trip; `None` when reserves are unknown.
    pub virtual_sol_lamports: Option<u64>,
    pub real_sol_lamports: Option<u64>,
}

// ── TokenCreated ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct TokenCreated {
    pub mint: String,
    pub creator: String,
    pub name: String,
    pub symbol: String,
    pub token_program_id: Option<String>,
    pub bonding_curve: Option<String>,
    pub initial_supply: Option<u64>,
    pub initial_buy_sol: Option<f64>,
    pub initial_buy_instruction: Option<BuyInstructionArgs>,
    pub cu_limit: Option<u64>,
    pub cu_price: Option<u64>,
    pub is_mayhem_mode: bool,
    pub is_cashback_enabled: bool,
    pub instruction_labels: Vec<String>,
    pub signature: String,
    pub slot: u64,
    pub block_time: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
}

/// Typed representation of the buy instruction args from a create transaction.
/// The host serializes this to JSON for DB persistence.
#[derive(Debug, Clone)]
pub enum BuyInstructionArgs {
    Buy { token_amount: u64, max_sol_cost: u64 },
    BuyV2 { token_amount: u64, max_sol_cost: u64 },
    BuyExactSolIn { spendable_sol_in: u64, min_tokens_out: u64 },
    BuyExactQuoteIn { spendable_sol_in: u64, min_tokens_out: u64 },
    BuyExactQuoteInV2 { spendable_sol_in: u64, min_tokens_out: u64 },
}

// ── TokenMigrated ─────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct TokenMigrated {
    pub mint: String,
    pub signature: String,
    pub slot: u64,
    pub block_time: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
}

// ── LiquidityEvent ────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct LiquidityEvent {
    pub mint: String,
    pub wallet: String,
    pub amount_sol: f64,
    pub token_amount: f64,
    pub added: bool,
    pub signature: String,
    pub slot: u64,
    pub block_time: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
}

// ── CreatorActivityEvent ──────────────────────────────────────────────────────

#[derive(Debug)]
pub struct CreatorActivityEvent {
    pub creator: String,
    pub mint: String,
    pub kind: CreatorActivityKind,
    pub signature: String,
    pub slot: u64,
    pub block_time: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreatorActivityKind {
    Create,
    Buy,
    Sell,
}

// ── RawTx (raw-tx feature) ──────────────────────────────────────────────────

/// The verbatim raw transaction, lowered for the `raw_txs` hypertable. Carries
/// the prost-encoded wire bytes (`payload`) rather than a decoded/JSON shape —
/// the source-of-truth feed parses on read, never in SQL.
#[cfg(feature = "raw-tx")]
#[derive(Debug)]
pub struct RawTx {
    /// Raw 64-byte transaction signature (`BYTEA` in the table).
    pub signature: Vec<u8>,
    pub slot: u64,
    /// Position of the transaction within its block (`info.index`).
    pub tx_index: u32,
    /// Partition/dedup axis. The gRPC stream carries no block time, so this is
    /// the `received_at` approximation.
    pub block_time: DateTime<Utc>,
    /// prost-encoded `SubscribeUpdateTransaction` wire bytes (parse on read).
    pub payload: Vec<u8>,
}
