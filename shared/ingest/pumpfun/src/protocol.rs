//! Static protocol descriptor: pump.fun / PumpSwap program IDs and on-chain
//! discriminators, decoded to bytes once at [`Ingest::builder`] construction
//! so the hot decode path compares `[u8;32]` / `[u8;8]`, never base58.

/// Pre-decoded bytes of a Solana program address (32 bytes).
#[derive(Debug, Clone)]
pub struct ProgramId {
    /// Raw 32-byte key.
    pub bytes: [u8; 32],
    /// Cached base58 string (decoded once at construction).
    pub base58: String,
}

impl ProgramId {
    pub fn from_base58(s: &str) -> Result<Self, crate::error::IngestError> {
        let vec = bs58::decode(s).into_vec().map_err(|e| {
            crate::error::IngestError::InvalidProgramId { id: s.to_string(), reason: e.to_string() }
        })?;
        let bytes: [u8; 32] = vec.try_into().map_err(|_| {
            crate::error::IngestError::InvalidProgramId {
                id: s.to_string(),
                reason: "expected 32 bytes for a Solana public key".into(),
            }
        })?;
        Ok(Self {
            bytes,
            base58: s.to_string(),
        })
    }
}

/// All program addresses relevant to ingest decoding.
#[derive(Debug, Clone)]
pub struct Programs {
    pub pump_fun: ProgramId,
    pub pump_swap: ProgramId,
    pub token: ProgramId,
    pub token_2022: ProgramId,
    pub associated_token: ProgramId,
    pub system: ProgramId,
    pub compute_budget: ProgramId,
    pub wsol: ProgramId,
}

/// Instruction + event discriminators (first 8 bytes of Anchor-encoded data).
#[derive(Debug, Clone)]
pub struct Discriminators {
    // ── bonding-curve instructions ───────────────────────────────────────────
    pub buy: [u8; 8],
    pub sell: [u8; 8],
    pub buy_exact_sol_in: [u8; 8],
    pub buy_exact_quote_in: [u8; 8],
    pub buy_v2: [u8; 8],
    pub buy_exact_quote_in_v2: [u8; 8],
    pub create_ix: [u8; 8],
    pub create_v2_ix: [u8; 8],
    pub migrate_ix: [u8; 8],
    pub migrate_v2_ix: [u8; 8],
    // ── admin / lifecycle ────────────────────────────────────────────────────
    pub initialize: [u8; 8],
    pub set_params: [u8; 8],
    pub withdraw: [u8; 8],
    pub extend_account: [u8; 8],
    pub collect_creator_fee: [u8; 8],
    // ── PumpSwap instructions ────────────────────────────────────────────────
    pub pump_swap_create_pool: [u8; 8],
    pub pump_swap_deposit: [u8; 8],
    pub pump_swap_disable: [u8; 8],
    pub pump_swap_update_admin: [u8; 8],
    pub pump_swap_update_fee_config: [u8; 8],
    // ── on-chain events ──────────────────────────────────────────────────────
    pub trade_event: [u8; 8],
    pub anchor_event_cpi: [u8; 8],
    pub create_event: [u8; 8],
    pub pump_swap_buy_event: [u8; 8],
    pub pump_swap_sell_event: [u8; 8],
}

/// Full static descriptor for one instance of the pump.fun protocol.
///
/// Decoded to bytes once at construction; the hot decode path operates on
/// `[u8; 32]` / `[u8; 8]` slices, never on base58 strings.
#[derive(Debug, Clone)]
pub struct Protocol {
    pub programs: Programs,
    pub discriminators: Discriminators,
    /// Lamports per SOL (unit scale used by trade decode).
    pub lamports_per_sol: f64,
    /// Dust filter: trades below this SOL value are dropped.
    pub min_trade_sol: f64,
}

// ── Literal constants (pump.fun mainnet, June 2025) ──────────────────────────

// Program-ID literals — `pub(crate)` so the labeler's `program_registry` names
// them without re-declaring the base58 strings (single source of truth).
pub(crate) const PUMP_FUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
pub(crate) const PUMP_SWAP_PROGRAM_ID: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
pub(crate) const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
pub(crate) const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
pub(crate) const ASSOCIATED_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
pub(crate) const SYSTEM_PROGRAM_ID: &str = "11111111111111111111111111111111";
pub(crate) const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";
const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";

const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;
const MIN_TRADE_LAMPORTS: u64 = 10_000;

impl Protocol {
    /// Construct the default pump.fun mainnet descriptor.
    ///
    /// Panics if the hard-coded addresses cannot be base58-decoded — that would
    /// indicate a build-time logic error, not a runtime condition.
    pub fn pump_fun() -> Self {
        let p = |s: &str| {
            ProgramId::from_base58(s).unwrap_or_else(|e| panic!("bad program id '{s}': {e}"))
        };

        Self {
            programs: Programs {
                pump_fun: p(PUMP_FUN_PROGRAM_ID),
                pump_swap: p(PUMP_SWAP_PROGRAM_ID),
                token: p(TOKEN_PROGRAM_ID),
                token_2022: p(TOKEN_2022_PROGRAM_ID),
                associated_token: p(ASSOCIATED_TOKEN_PROGRAM_ID),
                system: p(SYSTEM_PROGRAM_ID),
                compute_budget: p(COMPUTE_BUDGET_PROGRAM_ID),
                wsol: p(WSOL_MINT),
            },
            discriminators: Discriminators {
                buy: [0x66, 0x06, 0x3d, 0x12, 0x01, 0xda, 0xeb, 0xea],
                sell: [0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad],
                buy_exact_sol_in: [0x38, 0xfc, 0x74, 0x08, 0x9e, 0xdf, 0xcd, 0x5f],
                buy_exact_quote_in: [0xc6, 0x2e, 0x15, 0x52, 0xb4, 0xd9, 0xe8, 0x70],
                buy_v2: [0xb8, 0x17, 0xee, 0x61, 0x67, 0xc5, 0xd3, 0x3d],
                buy_exact_quote_in_v2: [0xc2, 0xab, 0x1c, 0x46, 0x68, 0x4d, 0x5b, 0x2f],
                create_ix: [0x18, 0x1e, 0xc8, 0x28, 0x05, 0x1c, 0x07, 0x77],
                create_v2_ix: [0xd6, 0x90, 0x4c, 0xec, 0x5f, 0x8b, 0x31, 0xb4],
                migrate_ix: [0x9b, 0xea, 0xe7, 0x92, 0xec, 0x9e, 0xa2, 0x1e],
                migrate_v2_ix: [0xbb, 0xcb, 0x12, 0x1f, 0xce, 0xed, 0xfe, 0x29],
                initialize: [0xaf, 0xaf, 0x6d, 0x1f, 0x0d, 0x98, 0x9b, 0xed],
                set_params: [0x1b, 0xea, 0xb2, 0x34, 0x93, 0x02, 0xbb, 0x8d],
                withdraw: [0xb7, 0x12, 0x46, 0x9c, 0x94, 0x6d, 0xa1, 0x22],
                extend_account: [0xea, 0x66, 0xc2, 0xcb, 0x96, 0x48, 0x3e, 0xe5],
                collect_creator_fee: [0x14, 0x16, 0x56, 0x7b, 0xc6, 0x1c, 0xdb, 0x84],
                pump_swap_create_pool: [0xe9, 0x92, 0xd1, 0x8e, 0xcf, 0x68, 0x40, 0xbc],
                pump_swap_deposit: [0xf2, 0x23, 0xc6, 0x89, 0x52, 0xe1, 0xf2, 0xb6],
                pump_swap_disable: [0xb9, 0xad, 0xbb, 0x5a, 0xd8, 0x0f, 0xee, 0xe9],
                pump_swap_update_admin: [0xa1, 0xb0, 0x28, 0xd5, 0x3c, 0xb8, 0xb3, 0xe4],
                pump_swap_update_fee_config: [0x68, 0xb8, 0x67, 0xf2, 0x58, 0x97, 0x6b, 0x14],
                trade_event: [0xbd, 0xdb, 0x7f, 0xd3, 0x4e, 0xe6, 0x61, 0xee],
                anchor_event_cpi: [0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d],
                create_event: [0x1b, 0x72, 0xa9, 0x4d, 0xde, 0xeb, 0x63, 0x76],
                pump_swap_buy_event: [103, 244, 82, 31, 44, 245, 119, 119],
                pump_swap_sell_event: [62, 47, 55, 10, 165, 3, 220, 42],
            },
            lamports_per_sol: LAMPORTS_PER_SOL,
            min_trade_sol: MIN_TRADE_LAMPORTS as f64 / LAMPORTS_PER_SOL,
        }
    }
}
