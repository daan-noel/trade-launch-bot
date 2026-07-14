use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Normalized representation of a Pump.fun token.
/// Decoupled from any Helius response structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    pub id: Uuid,
    /// The SPL mint address (base58).
    pub mint_address: String,
    /// Wallet that created the token.
    pub creator_wallet: String,
    pub name: String,
    pub symbol: String,
    /// Program id used for this token (SPL legacy or Token-2022), as base58 string.
    pub token_program_id: Option<String>,
    /// Bonding curve program address for this token.
    pub bonding_curve_address: Option<String>,
    /// First creator buy amount, expressed in raw token units. **NOT** the token's
    /// total supply — do not use it for market cap / FDV (H1). Total supply is the
    /// fixed pump.fun constant (`config::constants::token_math::total_supply_for`),
    /// persisted as `tokens.total_supply_token`.
    pub initial_supply_token: Option<u64>,
    /// First creator buy amount in SOL.
    pub initial_buy_sol: Option<f64>,
    /// Buy instruction arguments from the creation transaction, if present.
    pub initial_buy_instruction: Option<Value>,
    /// Compute-unit limit requested in the creation transaction (if any).
    pub cu_limit: Option<u64>,
    /// Compute-unit price requested in the creation transaction (if any), in micro-lamports per CU.
    pub cu_price: Option<u64>,
    /// Whether this token was created in Pump.mayhem mode.
    pub is_mayhem_mode: bool,
    /// Whether cashback was enabled at token creation (create_v2 only).
    pub is_cashback_enabled: bool,
    /// Instruction labels from the create transaction.
    pub instruction_labels: Value,
    /// Transaction signature of the creation instruction.
    pub creation_tx_signature: String,
    /// Solana slot of the creation transaction. Creation fact captured at
    /// `TokenCreated`; enables the same-slot buy/sell activity sums on
    /// `tokens_info` (`first_slot_buy_sol`/`first_slot_sell_sol`).
    pub creation_slot: Option<u64>,
    /// Same-creation-slot buy SOL total. Populated at first-slot gate resolution
    /// (live) or from a `tokens_info` join (backtest/simulate scan) — not a
    /// `tokens` column.
    #[serde(default)]
    pub first_slot_buy_sol: Option<f64>,
    /// Same-creation-slot sell SOL total — same population rules as
    /// [`first_slot_buy_sol`](Self::first_slot_buy_sol).
    #[serde(default)]
    pub first_slot_sell_sol: Option<f64>,
    pub created_at: DateTime<Utc>,
}

impl Token {
    pub fn new(
        mint_address: String,
        creator_wallet: String,
        name: String,
        symbol: String,
        token_program_id: Option<String>,
        bonding_curve_address: Option<String>,
        initial_supply_token: Option<u64>,
        initial_buy_sol: Option<f64>,
        initial_buy_instruction: Option<Value>,
        cu_limit: Option<u64>,
        cu_price: Option<u64>,
        is_mayhem_mode: bool,
        is_cashback_enabled: bool,
        instruction_labels: Value,
        creation_tx_signature: String,
        creation_slot: Option<u64>,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            mint_address,
            creator_wallet,
            name,
            symbol,
            token_program_id,
            bonding_curve_address,
            initial_supply_token,
            initial_buy_sol,
            initial_buy_instruction,
            cu_limit,
            cu_price,
            is_mayhem_mode,
            is_cashback_enabled,
            instruction_labels,
            creation_tx_signature,
            creation_slot,
            first_slot_buy_sol: None,
            first_slot_sell_sol: None,
            created_at,
        }
    }
}
