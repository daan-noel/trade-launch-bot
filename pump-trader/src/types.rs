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
