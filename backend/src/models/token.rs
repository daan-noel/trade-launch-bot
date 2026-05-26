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
    /// Bonding curve program address for this token.
    pub bonding_curve_address: Option<String>,
    /// First creator buy amount, expressed in raw token units.
    pub initial_supply_token: Option<u64>,
    /// First creator buy amount in SOL.
    pub initial_buy_sol: Option<f64>,
    /// Compute-unit limit requested in the creation transaction (if any).
    pub cu_limit: Option<u64>,
    /// Compute-unit price requested in the creation transaction (if any), in micro-lamports per CU.
    pub cu_price: Option<u64>,
    /// Whether this token was created in Pump.mayhem mode.
    pub is_mayhem_mode: bool,
    /// Instruction labels from the create transaction.
    pub instruction_labels: Value,
    /// Transaction signature of the creation instruction.
    pub creation_tx_signature: String,
    pub created_at: DateTime<Utc>,
}

impl Token {
    pub fn new(
        mint_address: String,
        creator_wallet: String,
        name: String,
        symbol: String,
        bonding_curve_address: Option<String>,
        initial_supply_token: Option<u64>,
        initial_buy_sol: Option<f64>,
        cu_limit: Option<u64>,
        cu_price: Option<u64>,
        is_mayhem_mode: bool,
        instruction_labels: Value,
        creation_tx_signature: String,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            mint_address,
            creator_wallet,
            name,
            symbol,
            bonding_curve_address,
            initial_supply_token,
            initial_buy_sol,
            cu_limit,
            cu_price,
            is_mayhem_mode,
            instruction_labels,
            creation_tx_signature,
            created_at,
        }
    }
}
