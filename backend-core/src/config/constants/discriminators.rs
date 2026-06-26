// ---------------------------------------------------------------------------
// Instruction discriminators (first 8 bytes of Anchor-encoded instruction data)
// Source: Official Pump.fun IDL (https://github.com/PumpFunOfficial)
// ---------------------------------------------------------------------------

// ── Core Trading Instructions ─────────────────────────────────────────────
/// Pump.fun bonding-curve `buy` instruction — buy tokens from a bonding curve.
pub const BUY_DISCRIMINATOR: [u8; 8] = [0x66, 0x06, 0x3d, 0x12, 0x01, 0xda, 0xeb, 0xea];
/// Pump.fun bonding-curve `sell` instruction — sell tokens into a bonding curve.
pub const SELL_DISCRIMINATOR: [u8; 8] = [0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad];

// ── Token Creation & Lifecycle ───────────────────────────────────────────
/// Pump.fun `create` instruction — creates a new coin and bonding curve.
pub const CREATE_INSTRUCTION_DISCRIMINATOR: [u8; 8] =
    [0x18, 0x1e, 0xc8, 0x28, 0x05, 0x1c, 0x07, 0x77];
/// Pump.fun `create_v2` instruction — creates a new coin and bonding curve (v2 variant).
pub const CREATE_V2_INSTRUCTION_DISCRIMINATOR: [u8; 8] =
    [0xd6, 0x90, 0x4c, 0xec, 0x5f, 0x8b, 0x31, 0xb4];
/// Pump.fun `migrate` instruction — migrates liquidity to pump_amm when bonding curve completes.
pub const MIGRATE_INSTRUCTION_DISCRIMINATOR: [u8; 8] =
    [0x9b, 0xea, 0xe7, 0x92, 0xec, 0x9e, 0xa2, 0x1e];
/// Pump.fun `migrate_v2` instruction — current migration path (replaced `migrate`).
/// sha256("global:migrate_v2")[..8]
pub const MIGRATE_V2_INSTRUCTION_DISCRIMINATOR: [u8; 8] =
    [0xbb, 0xcb, 0x12, 0x1f, 0xce, 0xed, 0xfe, 0x29];

// ── Buy Variants (all collapse to InstructionKind::Buy in the decoder) ───
/// `buy_exact_sol_in` — specify exact SOL in, receive tokens out.
/// sha256("global:buy_exact_sol_in")[..8]
pub const BUY_EXACT_SOL_IN_DISCRIMINATOR: [u8; 8] =
    [0x38, 0xfc, 0x74, 0x08, 0x9e, 0xdf, 0xcd, 0x5f];
/// `buy_exact_quote_in` — original second variant, still active.
/// sha256("global:buy_exact_quote_in")[..8]
pub const BUY_EXACT_QUOTE_IN_DISCRIMINATOR: [u8; 8] =
    [0xc6, 0x2e, 0x15, 0x52, 0xb4, 0xd9, 0xe8, 0x70];
/// `buy_v2` — new unified buy interface.
/// sha256("global:buy_v2")[..8]
pub const BUY_V2_DISCRIMINATOR: [u8; 8] = [0xb8, 0x17, 0xee, 0x61, 0x67, 0xc5, 0xd3, 0x3d];
/// `buy_exact_quote_in_v2` — new unified buy interface, exact quote in.
/// sha256("global:buy_exact_quote_in_v2")[..8]
pub const BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR: [u8; 8] =
    [0xc2, 0xab, 0x1c, 0x46, 0x68, 0x4d, 0x5b, 0x2f];

// ── Pump.fun admin / lifecycle (bonding-curve program) ───────────────────────
/// sha256("global:initialize")[..8]
pub const PUMP_INITIALIZE_DISCRIMINATOR: [u8; 8] =
    [0xaf, 0xaf, 0x6d, 0x1f, 0x0d, 0x98, 0x9b, 0xed];
/// sha256("global:set_params")[..8]
pub const PUMP_SET_PARAMS_DISCRIMINATOR: [u8; 8] =
    [0x1b, 0xea, 0xb2, 0x34, 0x93, 0x02, 0xbb, 0x8d];
/// sha256("global:withdraw")[..8] — admin fee withdrawal from bonding curve.
pub const PUMP_WITHDRAW_DISCRIMINATOR: [u8; 8] =
    [0xb7, 0x12, 0x46, 0x9c, 0x94, 0x6d, 0xa1, 0x22];
/// sha256("global:extend_account")[..8] — extends bonding-curve account data.
pub const PUMP_EXTEND_ACCOUNT_DISCRIMINATOR: [u8; 8] =
    [0xea, 0x66, 0xc2, 0xcb, 0x96, 0x48, 0x3e, 0xe5];
/// sha256("global:collect_creator_fee")[..8] — collects accumulated creator fees.
pub const PUMP_COLLECT_CREATOR_FEE_DISCRIMINATOR: [u8; 8] =
    [0x14, 0x16, 0x56, 0x7b, 0xc6, 0x1c, 0xdb, 0x84];

// ── PumpSwap (pump_amm) instructions ──────────────────────────────────────────
// NOTE: buy/sell discriminators are identical to the bonding-curve program —
// program_id is what distinguishes them at the call site.
/// sha256("global:create_pool")[..8]
pub const PUMP_SWAP_CREATE_POOL_DISCRIMINATOR: [u8; 8] =
    [0xe9, 0x92, 0xd1, 0x8e, 0xcf, 0x68, 0x40, 0xbc];
/// sha256("global:deposit")[..8]
pub const PUMP_SWAP_DEPOSIT_DISCRIMINATOR: [u8; 8] =
    [0xf2, 0x23, 0xc6, 0x89, 0x52, 0xe1, 0xf2, 0xb6];
/// sha256("global:disable")[..8]
pub const PUMP_SWAP_DISABLE_DISCRIMINATOR: [u8; 8] =
    [0xb9, 0xad, 0xbb, 0x5a, 0xd8, 0x0f, 0xee, 0xe9];
/// sha256("global:update_admin")[..8]
pub const PUMP_SWAP_UPDATE_ADMIN_DISCRIMINATOR: [u8; 8] =
    [0xa1, 0xb0, 0x28, 0xd5, 0x3c, 0xb8, 0xb3, 0xe4];
/// sha256("global:update_fee_config")[..8]
pub const PUMP_SWAP_UPDATE_FEE_CONFIG_DISCRIMINATOR: [u8; 8] =
    [0x68, 0xb8, 0x67, 0xf2, 0x58, 0x97, 0x6b, 0x14];

// ---------------------------------------------------------------------------
// On-chain event discriminators (emitted via `emit!` in "Program data:" logs)
// ---------------------------------------------------------------------------

/// TradeEvent discriminator — matches the CPI event data for every buy/sell.
/// "Program data:" log entries starting with these 8 bytes carry a full
/// RawTradeEvent (Borsh-encoded), including virtual/real reserves.
pub const TRADE_EVENT_DISCRIMINATOR: [u8; 8] = [0xbd, 0xdb, 0x7f, 0xd3, 0x4e, 0xe6, 0x61, 0xee];
/// Anchor `emit_cpi!` self-CPI tag. Pump.fun emits each event both as an
/// `emit!` "Program data:" log AND as an `emit_cpi!` inner instruction to the
/// event authority. Inner instructions are never truncated by Solana, so they
/// are the reliable source when a transaction's logs are truncated.
pub const ANCHOR_EVENT_CPI_DISCRIMINATOR: [u8; 8] =
    [0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];
/// CreateEvent discriminator — emitted on every token creation via `emit!`.
pub const CREATE_EVENT_DISCRIMINATOR: [u8; 8] =
    [0x1b, 0x72, 0xa9, 0x4d, 0xde, 0xeb, 0x63, 0x76];

// ── PumpSwap (pump_amm) post-migration swap events ───────────────────────────
// Only the leading fields (amounts, pool, user) are read; trailing fields added
// in later program versions are tolerated by Borsh deserialization.
/// PumpSwap `BuyEvent` discriminator.
pub const PUMP_SWAP_BUY_EVENT_DISCRIMINATOR: [u8; 8] = [103, 244, 82, 31, 44, 245, 119, 119];
/// PumpSwap `SellEvent` discriminator.
pub const PUMP_SWAP_SELL_EVENT_DISCRIMINATOR: [u8; 8] = [62, 47, 55, 10, 165, 3, 220, 42];
