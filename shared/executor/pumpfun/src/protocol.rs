//! Tier 1 — protocol invariants (compile-time `const`).
//!
//! These are the values that, if changed, would describe a *different* protocol:
//! program IDs, the WSOL mint, fixed fee recipients, Anchor discriminators,
//! account-layout byte offsets, account spaces, and unit conversions. They are
//! never configuration — a consumer that needs different values is targeting a
//! different chain/program, not tuning this one.
//!
//! Addresses are `const Pubkey` (via the `solana_sdk::pubkey!` macro), so the
//! trader references them directly with **zero** init-time base58 parsing and no
//! `.unwrap()` panics. A small set of the program IDs are also exposed as `&str`
//! for the JSON-RPC `programId` params and the `TokenProgram` string classifier,
//! which work in string form.
//!
//! NOTE: a subset of these (protocol IDs / unit conversions / the CU tuning) is
//! intentionally duplicated in `trading_core::config::constants` so
//! `trading_core`/`lab` can read them without depending on this crate (which
//! pulls the full trading/RPC executor stack). Keep the copies in sync if a
//! shared value ever changes (program IDs effectively never do).

use solana_sdk::{pubkey, pubkey::Pubkey};

// ---------------------------------------------------------------------------
// Program IDs / mints (const Pubkey — no parse, no unwrap)
// ---------------------------------------------------------------------------

/// pump.fun bonding-curve program.
pub const PUMP_FUN: Pubkey = pubkey!("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P");
/// PumpSwap (pump_amm) program — trading once a token has migrated off the curve.
pub const PUMP_SWAP: Pubkey = pubkey!("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA");
/// Wrapped SOL mint — the quote mint for PumpSwap pools.
pub const WSOL_MINT: Pubkey = pubkey!("So11111111111111111111111111111111111111112");
/// pump.fun `__event_authority` PDA marker account.
pub const EVENT_AUTHORITY: Pubkey = pubkey!("Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1");
/// pfee fee program (`fee_config` PDA base).
pub const FEE_PROGRAM: Pubkey = pubkey!("pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ");
/// Classic SPL Token program.
pub const TOKEN: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
/// Token-2022 / Token Extensions program.
pub const TOKEN_2022: Pubkey = pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
/// Associated-token-account program.
pub const ASSOCIATED_TOKEN_PROGRAM: Pubkey =
    pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
/// Metaplex Token Metadata program (legacy `create` CPI).
pub const MPL_TOKEN_METADATA: Pubkey =
    pubkey!("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s");
/// Pump.mayhem program (`create_v2` mayhem-mode accounts).
pub const MAYHEM_PROGRAM: Pubkey = pubkey!("MAyhSmzXzV1pTf7LsNkrNwkWKTo4ougAJ1PPg47MD4e");

// ---------------------------------------------------------------------------
// Create instruction discriminators (Anchor sha256("global:<name>")[..8])
// ---------------------------------------------------------------------------

/// Legacy SPL-Token `create` instruction.
pub const CREATE_DISC: [u8; 8] = [24, 30, 200, 40, 5, 28, 7, 119];
/// Token-2022 `create_v2` instruction (current pump.fun default).
pub const CREATE_V2_DISC: [u8; 8] = [214, 144, 76, 236, 95, 139, 49, 180];

// ---------------------------------------------------------------------------
// Buy / sell instruction discriminators — the SSOT the ix builders AND the
// `catalog` reference (Anchor sha256("global:<name>")[..8]). The plain `buy` /
// `sell` names hash the same on the pump.fun **curve** and the PumpSwap **AMM**
// programs, so those two discriminators are shared across both venues and
// disambiguated only by `program_id` — which is exactly why `VenueId` is a
// load-bearing catalog axis. Kept here (not inline in each builder) so a program
// upgrade re-points every builder + the catalog in one edit; `catalog::tests`
// guards them equal.
// ---------------------------------------------------------------------------

/// `buy` — exact BASE (tokens) out, ≤ max quote. Curve **and** AMM (`global:buy`).
pub const BUY_DISC: [u8; 8] = [102, 6, 61, 18, 1, 218, 235, 234];
/// `buy_exact_sol_in` — spend exact QUOTE (lamports), ≥ min base. Curve only.
pub const BUY_EXACT_SOL_IN_DISC: [u8; 8] = [56, 252, 116, 8, 158, 223, 205, 95];
/// `buy_v2` — v2 curve account layout, exact base out. Curve only.
pub const BUY_V2_DISC: [u8; 8] = [184, 23, 238, 97, 103, 197, 211, 61];
/// `buy_exact_quote_in` (v2) — spend exact QUOTE, v2 layout (cashback). Curve only.
pub const BUY_EXACT_QUOTE_IN_V2_DISC: [u8; 8] = [194, 171, 28, 70, 104, 77, 91, 47];
/// `sell` — exact BASE (tokens) in, ≥ min quote. Curve **and** AMM (`global:sell`).
pub const SELL_DISC: [u8; 8] = [51, 230, 133, 164, 1, 127, 131, 173];

/// Fresh bonding-curve virtual reserves at creation (protocol constants).
pub const INITIAL_VIRTUAL_TOKEN_RESERVES: u128 = 1_073_000_000_000_000;
pub const INITIAL_VIRTUAL_SOL_RESERVES: u128 = 30_000_000_000;

/// Classic SPL Token program — base58 string form (for JSON-RPC `programId`
/// params and the `TokenProgram` string classifier, which compare in string
/// form rather than parsing).
pub const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
/// Token-2022 program — base58 string form (see [`TOKEN_PROGRAM_ID`]).
pub const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";

/// Slot-[17] fee recipient the **curve** (pump.fun program) buy/sell paths must
/// send. The deployed program is newer than its published IDL and checks this
/// account *exactly* — `fee_recipient.rs:19` reverts `Custom(6000) =
/// NotAuthorized` on any mismatch. pump.fun rotated it in a program upgrade; the
/// pre-upgrade value (`5YxQFdt3…`) now reverts. Post-upgrade value verified
/// 2026-06-22 against a live 18-account curve buy (`4roGuTpp…`) and a passing
/// zero-SOL `simulate-buy`. NOT interchangeable with the AMM recipient below —
/// the curve wants this exact account, the AMM accepts any whitelist member.
pub const PUMP_CURVE_FEE_RECIPIENT: Pubkey =
    pubkey!("A7hAgCzFw14fejgCp387JUJRMNyz4j89JKnhtKU8piqW");

/// Trailing buyback-fee recipient the **AMM** (pump_amm `pAMMBay…`) swap paths
/// append (the deployed program is newer than its published IDL). This is a
/// *different* program from the curve with *different* semantics: the recipient
/// rotates across a whitelist and the program accepts any member, so we send
/// `buyback_fee_recipients[0]`. Verified 2026-06-22 still a live whitelist
/// member (live pump_amm swap `6779XKXc…` used this exact account; sibling swaps
/// rotated through `5eHhjP8J…`/`5cjcW9wE…`). Do NOT replace with the curve's
/// `A7hAgCz…`, which is not an AMM whitelist member.
pub const PUMP_AMM_BUYBACK_FEE_RECIPIENT: Pubkey =
    pubkey!("5YxQFdt3Tr9zJLvkFccqXVUwhdTWJQc1fFg2YPbxvxeD");

/// Fixed pfee account that cashback-enabled PumpSwap pools append in the
/// trailing fee block, just before the buyback recipient pair. Constant across
/// on-chain cashback swaps; has no account data of its own (a program marker).
/// Only present when the pool's `is_cashback_coin` flag is set.
pub const PUMP_AMM_CASHBACK_GLOBAL: Pubkey =
    pubkey!("5817UmPM7KLKu2mSQVsXMJ7X2rr3PtEb3j4EioyoJgd1");

/// Jito tip accounts (mainnet); one is chosen at random per trader instance.
/// These are Jito's canonical 8 tip accounts (`getTipAccounts` RPC), verified
/// 2026-07-10 against Jito's docs. A tip that isn't sent to one of THESE exact
/// accounts is invisible to the block engine's auction-eligibility check, which
/// rejects the bundle with "must write lock at least one tip account". They are
/// documented to remain constant.
pub const JITO_TIP_ACCOUNTS: [Pubkey; 8] = [
    pubkey!("96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5"),
    pubkey!("HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe"),
    pubkey!("Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY"),
    pubkey!("ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49"),
    pubkey!("DfXygSm4jCyNCybVYYK6DwvWqjKee8pbDmJGcLWNDXjh"),
    pubkey!("ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt"),
    pubkey!("DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL"),
    pubkey!("3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnizKZ6jT"),
];

// ---------------------------------------------------------------------------
// Unit conversions
// ---------------------------------------------------------------------------

pub const LAMPORTS_PER_SOL: u64 = 1_000_000_000;

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

// ---------------------------------------------------------------------------
// Token-account spaces / rent placeholder
// ---------------------------------------------------------------------------

/// SPL token account data size (bytes) — the fixed `spl_token::state::Account`
/// length, used to size its rent-exemption lookup in `initialize`.
pub const TOKEN_ACCOUNT_SPACE: u64 = 165;
/// Token-2022 token account data size (bytes): the 165-byte base account plus
/// the account-type tag + immutable-owner extension a pump.fun ATA carries.
pub const TOKEN_2022_ACCOUNT_SPACE: u64 = 182;
/// Placeholder rent (lamports, ~0.002 SOL) for the token-account fields before
/// `initialize` overwrites them with the on-chain
/// `getMinimumBalanceForRentExemption`. Only a value, never used to fund a real
/// buy: every buy path bails at the `global_account` "Not initialized" guard
/// before it reads rent, so a pre-`initialize` buy can't under-fund an account.
pub const TOKEN_ACCOUNT_RENT_PLACEHOLDER: u64 = 2_000_000;

// ---------------------------------------------------------------------------
// PumpSwap AMM account-layout byte offsets
// ---------------------------------------------------------------------------

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
