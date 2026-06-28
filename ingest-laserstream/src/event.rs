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
    #[cfg(feature = "raw-json")]
    RawTx(RawTx),
}

// ── Trade ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Trade {
    pub mint: String,
    pub wallet: String,
    pub side: Side,
    pub sol: f64,
    pub tokens: f64,
    pub price: f64,
    pub signature: String,
    pub leg_index: u32,
    pub slot: u64,
    pub block_time: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    pub reserves: Reserves,
    pub venue: Venue,
    pub instruction_type: String,
    pub instruction_labels: Vec<String>,
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
    pub virtual_token: Option<f64>,
    pub real_sol: Option<f64>,
    pub real_token: Option<f64>,
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
    pub sol_amount: f64,
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

// ── RawTx (raw-json feature) ──────────────────────────────────────────────────

#[cfg(feature = "raw-json")]
#[derive(Debug)]
pub struct RawTx {
    pub signature: String,
    pub slot: u64,
    pub block_time: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    pub raw_data: serde_json::Value,
}
