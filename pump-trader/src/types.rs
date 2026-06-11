use serde::Serialize;

/// One token account entry in the wallet — pure on-chain data.
#[derive(Debug, Clone, Serialize)]
pub struct WalletHolding {
    pub mint: String,
    pub amount: u64,
    pub ui_amount: f64,
    pub decimals: u8,
    pub token_account: String,
    pub token_program_id: String,
}

/// Routing facts for a manual buy, resolved on-chain from the bonding-curve
/// PDA (creator + `complete` flag) and the mint account (token program).
/// `is_migrated` picks the bonding-curve vs PumpSwap AMM path.
#[derive(Debug, Clone, Serialize)]
pub struct BuyRouting {
    pub creator: String,
    pub token_program_id: String,
    pub is_migrated: bool,
}

/// Bonding-curve facts the wallet view needs for a mint the local cache has
/// never tracked: migration status (`complete` @48) and the create_v2 cashback
/// flag (@82). Both are read from the same bonding-curve account, so a single
/// batched read yields both. Returned by `PumpFunTrader::resolve_curve_facts_batch`.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct CurveFacts {
    pub is_migrated: bool,
    pub cashback_enabled: bool,
}

/// On-chain token balance for a wallet + mint pair.
#[derive(Debug, Clone, Serialize)]
pub struct TokenBalance {
    pub mint: String,
    pub wallet: String,
    pub amount: u64,
    pub ui_amount: f64,
    pub decimals: u8,
    pub token_account: Option<String>,
    pub token_program_id: String,
}
