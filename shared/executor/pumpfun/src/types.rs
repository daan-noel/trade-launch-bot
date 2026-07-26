use crate::protocol::{self, TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

/// A durable, transport-neutral snapshot of the PumpSwap pool facts the trader
/// caches for a migrated token (internally `AmmPoolInfo`). Pubkeys are base58
/// strings so a consumer can persist it to its own store and later hand it back
/// via [`crate::PumpFunTrader::seed_amm_pool_facts`] to warm the cache with **no
/// RPC**. The steady-state source of these facts is the zero-RPC feed harvest
/// (`observe_amm_swap_accounts`); persisting them lets that knowledge survive a
/// restart, so even a dead/illiquid token whose pool has no *recent* swap still
/// sells (the cold RPC path would otherwise have to reconstruct the swap tail).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmmPoolFacts {
    pub pool: String,
    pub base_mint: String,
    pub quote_mint: String,
    pub base_token_program: String,
    pub pool_base_token_account: String,
    pub pool_quote_token_account: String,
    pub coin_creator: String,
    pub coin_creator_vault_ata: String,
    pub coin_creator_vault_authority: String,
    pub is_cashback_coin: bool,
    pub fee_share_marker: Option<String>,
    pub needs_pool_v2: bool,
}

/// The SPL token program a mint uses. Resolved once at the trade boundary so the
/// hot path branches on a two-variant enum the compiler exhaustively checks,
/// instead of re-comparing or re-parsing the program-id string at each step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenProgram {
    /// Classic SPL Token program.
    Legacy,
    /// Token-2022 / Token Extensions program.
    Token2022,
}

impl TokenProgram {
    /// Classify a program-id string. Anything that isn't the classic SPL Token
    /// program id is treated as Token-2022 — the same `!= TOKEN_PROGRAM_ID ⇒
    /// 2022` convention the buy/pool paths already used.
    pub fn from_id(id: &str) -> Self {
        if id == TOKEN_PROGRAM_ID {
            Self::Legacy
        } else {
            Self::Token2022
        }
    }

    /// Classify a token program's `Pubkey` (a mint account's owner).
    pub fn from_pubkey(owner: &Pubkey) -> Self {
        if *owner == protocol::TOKEN {
            Self::Legacy
        } else {
            Self::Token2022
        }
    }

    /// The program id as the canonical base58 string constant.
    pub fn as_id(self) -> &'static str {
        match self {
            Self::Legacy => TOKEN_PROGRAM_ID,
            Self::Token2022 => TOKEN_2022_PROGRAM_ID,
        }
    }

    /// The program id as a `const Pubkey` (no parse — sourced from `protocol`).
    pub fn pubkey(self) -> Pubkey {
        match self {
            Self::Legacy => protocol::TOKEN,
            Self::Token2022 => protocol::TOKEN_2022,
        }
    }
}

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
    /// Cashback flag read live from the bonding curve (create_v2 byte @82). Gates
    /// the curve **sell**'s `user_volume_accumulator` account; resolved here so a
    /// manual sell never has to fall back to a stale `token_cache` value (the
    /// `Custom(6024)` missing-UVA bug). The AMM reads cashback from the pool, so
    /// this only matters pre-migration.
    pub cashback_enabled: bool,
    /// Parsed forms of the fields above, carried so the curve-buy path reuses the
    /// parse [`crate::PumpFunTrader::resolve_buy_routing`] already did instead of
    /// re-parsing the same strings. (Skipped in the serialized view, which only
    /// exposes the string forms.)
    #[serde(skip)]
    pub mint: Pubkey,
    #[serde(skip)]
    pub creator_pubkey: Pubkey,
    #[serde(skip)]
    pub token_program: TokenProgram,
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

/// Metadata for a legacy pump.fun `create` (SPL Token mint + Metaplex metadata).
#[derive(Debug, Clone)]
pub struct CreateTokenArgs {
    pub name: String,
    pub symbol: String,
    pub uri: String,
    /// On-chain creator — usually the dev wallet (`user` signer).
    pub creator: Pubkey,
}

/// Metadata for pump.fun `create_v2` (Token-2022 mint; current default).
#[derive(Debug, Clone)]
pub struct CreateTokenV2Args {
    pub name: String,
    pub symbol: String,
    pub uri: String,
    pub creator: Pubkey,
    pub is_mayhem_mode: bool,
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
