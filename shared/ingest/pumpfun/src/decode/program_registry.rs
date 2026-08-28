//! Program-ID → friendly-name registry and instruction namer for the labeler.
//!
//! This is the single source of truth for turning a raw program address plus its
//! instruction `data` into a human label (`"Jupiter Aggregator V6: Route"`,
//! `"Terminal: V2BuyExactInPumpFun"`). It is consulted only by the
//! display/analytics label path ([`super::instructions::label_instruction`]) —
//! never the per-trade decode hot loop. Lookups are O(1) against maps built once.
//!
//! # Three naming tiers, in descending order of proof
//!
//! 1. **Anchor instructions** ([`ANCHOR_IX`]). We store the *name*; the 8-byte
//!    discriminator is COMPUTED as `sha256("global:<snake_name>")[..8]`. A wrong
//!    name therefore matches nothing on chain and the label degrades to a key —
//!    it can never produce a wrong *label*. `method_reproduces_pump_discriminators`
//!    pins the mechanism against pump.fun's known-correct discriminators.
//! 2. **Explicit keys** ([`EXPLICIT_IX`]). Programs that log an instruction name
//!    but do not hash it the Anchor way. The key bytes are transcribed, so these
//!    carry transcription risk the computed tier does not; keep the list short.
//! 3. **Key only.** Everything else renders `ix#<key>` — a *stable identity*, not
//!    a name. `ix#af051981a0d8389d` separates a router's buy from its sell without
//!    claiming to know what either is called, which is the whole point: a wrong
//!    label is worse than no name, but no identity at all is worse than both.
//!
//! # Growing the tables
//!
//! `cargo run -p hunter-live -- unknown-programs` ranks the program IDs still
//! rendering as `Unknown (<id>)` in the persisted `trades.ix_labels`.
//! `cargo run -p hunter-live -- decode-harvest` then reads those programs off
//! chain: it pairs each `Program log: Instruction: <Name>` line with the
//! discriminator of the instruction that produced it, **verifies** the pair by
//! recomputing `sha256("global:<snake>")`, and prints paste-ready rows for the
//! tables below. A pair that fails to verify is reported, never emitted.
//!
//! Prefer *no* entry over a guessed one. A vanity prefix (`MaestroAAe…`) is a
//! hint, not evidence; the instruction set it runs is evidence.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::protocol::{
    ASSOCIATED_TOKEN_PROGRAM_ID, COMPUTE_BUDGET_PROGRAM_ID, PUMP_FUN_PROGRAM_ID,
    PUMP_SWAP_PROGRAM_ID, SYSTEM_PROGRAM_ID, TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID,
};

/// SPL Memo. Named here rather than in `Protocol` because nothing decodes it —
/// the labeler only needs to stop calling it `Unknown`.
pub(super) const MEMO_PROGRAM_ID: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";

/// `(program_id_base58, friendly_name)`. Ordered by how the name is established:
/// native/SPL and DEX venues are documented addresses; the router block is named
/// from the instruction set each program actually runs (see [`ANCHOR_IX`]).
const REGISTRY: &[(&str, &str)] = &[
    // ── Native / SPL / pump.fun (also decoded structurally; listed so the
    //    harvest never reports them as "unknown") ─────────────────────────────
    (PUMP_FUN_PROGRAM_ID, "Pump.Fun"),
    (PUMP_SWAP_PROGRAM_ID, "PumpSwap"),
    (COMPUTE_BUDGET_PROGRAM_ID, "Compute Budget"),
    (SYSTEM_PROGRAM_ID, "System Program"),
    (TOKEN_PROGRAM_ID, "Token Program"),
    (TOKEN_2022_PROGRAM_ID, "Token 2022"),
    (ASSOCIATED_TOKEN_PROGRAM_ID, "Associated Token"),
    ("AddressLookupTab1e1111111111111111111111111", "Address Lookup Table"),
    ("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s", "Token Metadata"),
    (MEMO_PROGRAM_ID, "Memo Program"),

    // ── DEX venues & launchpads (stable, verified addresses) ─────────────────
    ("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8", "Raydium AMM v4"),
    ("CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK", "Raydium CLMM"),
    ("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C", "Raydium CPMM"),
    ("LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj", "Raydium LaunchLab"),
    ("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc", "Orca Whirlpool"),
    ("LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo", "Meteora DLMM"),
    ("Eo7WjKq67rjJQSZxS6z3YkapzY3eMj6Xy8X5EQVn5UaB", "Meteora Dynamic AMM"),
    ("cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG", "Meteora DAMM V2"),
    ("dbcij3LWUppWqq96dh6gJWwBifmcGfLSB5D4DuSMaqN", "Meteora DBC"),
    ("PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY", "Phoenix"),
    ("2wT8Yq49kHgDzXuPxZSaeLaH1qbmGXWEYUdrsBBWvv3F", "Lifinity V2"),
    ("MoonCVVNZFSYkqNXP6bxHLPL6QQJiMagDL3qcqUQTrG", "Moonshot"),

    // ── Aggregators / routers ────────────────────────────────────────────────
    ("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4", "Jupiter Aggregator V6"),
    ("6m2CDdhRgxpH4WjvdzxAYbGxwdGUz5MziiL5jek2kBma", "OKX DEX Router"),
    // Second OKX deployment. Its own on-chain IDL self-names it "OKX: DEX Router".
    ("proVF4pMXVaYqmy4NjniPh4pqKNfMmsihgd4wdkCX3u", "OKX DEX Router"),
    ("DF1ow4tspfHX9JwWJsAb9epbkA8hmpSEAtxXy1V27QBH", "DFlow Aggregator"),
    // Self-named in its on-chain IDL (`debot_router`).
    ("G7MVcM9YzGxrmLtmobUgyt8A6WhQ2dgQX3aSJcPejdEp", "deBot Router"),

    // ── Retail front-ends. Each name is corroborated by the instruction set the
    //    program runs (see ANCHOR_IX): a vanity prefix alone does not qualify. ─
    ("FLASHX8DrLbgeR8FcfNV1F5krxYcYMUdBkrP1EPBtxB9", "Axiom Trade"),
    ("BSfD6SHZigAfDWSjzD5Q41jw8LmKwtmjskPH9XW1mrRW", "Photon"),
    ("GMgnVFR8Jb39LoXsEVzb3DvBy3ywCmdmJquHUy1Lrkqb", "GMGN Bot"),
    ("GMGNreQcJFufBiCTLDBgKhYEfEe9B454UjpDr5CaSLA1", "GMGN Bot"),
    ("term9YPb9mzAsABaqN71A4xdbxHmpBNZavpBiQKZzN3", "Terminal"),
    ("troyXT7Ty3s2rjJe4bqWaroUrS4Fjd8rbHHNHxcACF4", "Trojan Trade"),
    ("TroYL71c8P2XNtDxHs98VtVLuiASJ7Ao5FvUoKyp3Bk", "Trojan Trade"),
    ("MaestroAAe9ge5HTc64VbBQZ6fP77pwvrhM8i1XWSAx", "Maestro"),
    ("b1oomGGqPKGD6errbyfbVMBuzSC8WtAAYo8MwNafWW1", "Bloom Router"),

    // ── Transaction guards. Not venues: they assert state and abort the tx.
    //    Lighthouse is inserted by the WALLET (Phantom et al.), so its presence
    //    is a property of the signer's client, not of the trade. ──────────────
    ("L2TExMFKdjpN9kozasaurPirfHy9P8sbXoAN1qA3S95", "Lighthouse"),

    // ── Arbitrage/MEV programs, named by behaviour only ──────────────────────
    ("FAdo9NCw1ssek6Z6yeWzWjhLVsr8uiCwcWNUnKgzTnHe", "Arbitrage Bot"),
    ("9Zzf9QqTy3TkyXysvJBsXyuRjda5aXCEJ9vXfL2HKSYv", "Arbitrage Bot"),
];

fn registry() -> &'static HashMap<&'static str, &'static str> {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| REGISTRY.iter().copied().collect())
}

/// Short human-readable label for a known Solana program ID, or `None` when the
/// program is not in the registry (the caller then falls back to
/// `Unknown (<program id>)`).
pub fn program_friendly_name(id: &str) -> Option<&'static str> {
    registry().get(id).copied()
}

// ── Instruction naming ───────────────────────────────────────────────────────

/// `(program_id, &[(on_chain_snake_name, display_name)])` for programs whose
/// 8-byte discriminator is `sha256("global:<snake_name>")[..8]`.
///
/// Instruction names are the identifiers Anchor hashes (snake_case), not the
/// camelCase IDL spelling. The discriminator is never written down here — it is
/// computed from the name, so a typo yields a label that simply never fires.
///
/// Two sources feed this table, both machine-checked by `decode-harvest`:
/// the program's on-chain Anchor IDL, and its own
/// `Program log: Instruction: <Name>` lines paired with the discriminator that
/// produced them.
const ANCHOR_IX: &[(&str, &[(&str, &str)])] = &[
    // pump.fun bonding curve - our own venue. Full IDL set.
    (
        PUMP_FUN_PROGRAM_ID,
        &[
            ("add_quote_mint", "AddQuoteMint"),
            ("admin_set_creator", "AdminSetCreator"),
            ("admin_set_idl_authority", "AdminSetIdlAuthority"),
            ("admin_update_token_incentives", "AdminUpdateTokenIncentives"),
            ("buy", "Buy"),
            ("buy_exact_quote_in_v2", "BuyExactQuoteInV2"),
            ("buy_exact_sol_in", "BuyExactSolIn"),
            ("buy_v2", "BuyV2"),
            ("claim_cashback", "ClaimCashback"),
            ("claim_cashback_v2", "ClaimCashbackV2"),
            ("claim_token_incentives", "ClaimTokenIncentives"),
            ("close_user_volume_accumulator", "CloseUserVolumeAccumulator"),
            ("collect_creator_fee", "CollectCreatorFee"),
            ("collect_creator_fee_v2", "CollectCreatorFeeV2"),
            ("create", "Create"),
            ("create_v2", "Create_v2"),
            ("distribute_creator_fees", "DistributeCreatorFees"),
            ("distribute_creator_fees_v2", "DistributeCreatorFeesV2"),
            ("extend_account", "ExtendAccount"),
            ("get_minimum_distributable_fee", "GetMinimumDistributableFee"),
            ("init_user_volume_accumulator", "InitUserVolumeAccumulator"),
            ("initialize", "Initialize"),
            ("migrate", "Migrate"),
            ("migrate_bonding_curve_creator", "MigrateBondingCurveCreator"),
            ("migrate_v2", "Migrate_v2"),
            ("remove_quote_mint", "RemoveQuoteMint"),
            ("sell", "Sell"),
            ("sell_v2", "SellV2"),
            ("set_creator", "SetCreator"),
            ("set_mayhem_virtual_params", "SetMayhemVirtualParams"),
            ("set_metaplex_creator", "SetMetaplexCreator"),
            ("set_params", "SetParams"),
            ("set_reserved_fee_recipients", "SetReservedFeeRecipients"),
            ("set_virtual_quote_reserves", "SetVirtualQuoteReserves"),
            ("sync_user_volume_accumulator", "SyncUserVolumeAccumulator"),
            ("toggle_cashback_enabled", "ToggleCashbackEnabled"),
            ("toggle_create_v2", "ToggleCreateV2"),
            ("toggle_mayhem_mode", "ToggleMayhemMode"),
            ("update_buyback_config", "UpdateBuybackConfig"),
            ("update_global_authority", "UpdateGlobalAuthority"),
        ],
    ),
    // PumpSwap (pump_amm) - the post-graduation venue.
    (
        PUMP_SWAP_PROGRAM_ID,
        &[
            ("admin_set_coin_creator", "AdminSetCoinCreator"),
            ("admin_update_token_incentives", "AdminUpdateTokenIncentives"),
            ("buy", "Buy"),
            ("buy_exact_quote_in", "BuyExactQuoteIn"),
            ("claim_cashback", "ClaimCashback"),
            ("claim_token_incentives", "ClaimTokenIncentives"),
            ("close_user_volume_accumulator", "CloseUserVolumeAccumulator"),
            ("collect_coin_creator_fee", "CollectCoinCreatorFee"),
            ("create_config", "CreateConfig"),
            ("create_pool", "CreatePool"),
            ("deposit", "Deposit"),
            ("disable", "Disable"),
            ("extend_account", "ExtendAccount"),
            ("init_user_volume_accumulator", "InitUserVolumeAccumulator"),
            ("migrate_pool_coin_creator", "MigratePoolCoinCreator"),
            ("sell", "Sell"),
            ("set_coin_creator", "SetCoinCreator"),
            ("set_reserved_fee_recipients", "SetReservedFeeRecipients"),
            ("sync_user_volume_accumulator", "SyncUserVolumeAccumulator"),
            ("toggle_cashback_enabled", "ToggleCashbackEnabled"),
            ("toggle_mayhem_mode", "ToggleMayhemMode"),
            ("transfer_creator_fees_to_pump", "TransferCreatorFeesToPump"),
            ("transfer_creator_fees_to_pump_v2", "TransferCreatorFeesToPumpV2"),
            ("update_admin", "UpdateAdmin"),
            ("update_buyback_config", "UpdateBuybackConfig"),
            ("update_fee_config", "UpdateFeeConfig"),
            ("withdraw", "Withdraw"),
        ],
    ),
    // Jupiter Aggregator V6.
    (
        "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4",
        &[
            ("claim", "Claim"),
            ("claim_token", "ClaimToken"),
            ("close_token", "CloseToken"),
            ("create_token_ledger", "CreateTokenLedger"),
            ("create_token_account", "CreateTokenAccount"),
            ("close_wsol_token_account", "CloseWsolTokenAccount"),
            ("exact_out_route", "ExactOutRoute"),
            ("route", "Route"),
            ("route_with_token_ledger", "RouteWithTokenLedger"),
            ("set_token_ledger", "SetTokenLedger"),
            ("shared_accounts_exact_out_route", "SharedAccountsExactOutRoute"),
            ("shared_accounts_route", "SharedAccountsRoute"),
            ("shared_accounts_route_with_token_ledger", "SharedAccountsRouteWithTokenLedger"),
            ("exact_out_route_v2", "ExactOutRouteV2"),
            ("route_v2", "RouteV2"),
            ("shared_accounts_exact_out_route_v2", "SharedAccountsExactOutRouteV2"),
            ("shared_accounts_route_v2", "SharedAccountsRouteV2"),
        ],
    ),
    // DFlow Aggregator (swap_orchestrator).
    (
        "DF1ow4tspfHX9JwWJsAb9epbkA8hmpSEAtxXy1V27QBH",
        &[
            ("close_empty_token_account", "CloseEmptyTokenAccount"),
            ("close_order", "CloseOrder"),
            ("create_referral_token_account_idempotent", "CreateReferralTokenAccountIdempotent"),
            ("fill_order", "FillOrder"),
            ("init_market_ledger_idempotent", "InitMarketLedgerIdempotent"),
            ("open_order", "OpenOrder"),
            ("swap", "Swap"),
            ("swap2", "Swap2"),
            ("swap2_with_destination", "Swap2WithDestination"),
            ("swap2_with_destination_native", "Swap2WithDestinationNative"),
            ("swap_with_destination", "SwapWithDestination"),
            ("swap_with_destination_native", "SwapWithDestinationNative"),
            ("transfer_fee", "TransferFee"),
            ("transfer_sol", "TransferSol"),
            ("transfer_to_sponsor", "TransferToSponsor"),
            ("unwrap_sol", "UnwrapSol"),
            ("withdraw_fees", "WithdrawFees"),
            ("wrap_sol", "WrapSol"),
        ],
    ),
    // OKX DEX Router (dex_solana).
    (
        "6m2CDdhRgxpH4WjvdzxAYbGxwdGUz5MziiL5jek2kBma",
        &[
            ("claim", "Claim"),
            ("claim_cashback_pumpfun", "ClaimCashbackPumpfun"),
            ("claim_cashback_pumpswap", "ClaimCashbackPumpswap"),
            ("create_token_account", "CreateTokenAccount"),
            ("create_token_account_with_seed", "CreateTokenAccountWithSeed"),
            ("proxy_swap", "ProxySwap"),
            ("swap", "Swap"),
            ("swap_tob_v3", "SwapTobV3"),
            ("swap_tob_v3_enhanced", "SwapTobV3Enhanced"),
            ("swap_tob_v3_with_receiver", "SwapTobV3WithReceiver"),
            ("swap_v3", "SwapV3"),
            ("swap_v3_with_cpi_event", "SwapV3WithCpiEvent"),
            ("wrap_unwrap_v3", "WrapUnwrapV3"),
            ("wrap_unwrap_v3_with_receiver", "WrapUnwrapV3WithReceiver"),
        ],
    ),
    // OKX DEX Router, second deployment.
    (
        "proVF4pMXVaYqmy4NjniPh4pqKNfMmsihgd4wdkCX3u",
        &[
            ("claim", "Claim"),
            ("claim_cashback_pumpfun", "ClaimCashbackPumpfun"),
            ("claim_cashback_pumpswap", "ClaimCashbackPumpswap"),
            ("create_ata_with_close_authority", "CreateAtaWithCloseAuthority"),
            ("create_token_account", "CreateTokenAccount"),
            ("create_token_account_with_seed", "CreateTokenAccountWithSeed"),
            ("init_token_ledger", "InitTokenLedger"),
            ("proxy_swap", "ProxySwap"),
            ("set_token_ledger", "SetTokenLedger"),
            ("swap", "Swap"),
            ("swap_tob", "SwapTob"),
            ("swap_tob_enhanced", "SwapTobEnhanced"),
            ("swap_tob_v2", "SwapTobV2"),
            ("swap_tob_v3", "SwapTobV3"),
            ("swap_tob_with_receiver", "SwapTobWithReceiver"),
            ("swap_tob_with_receiver_token_ledger", "SwapTobWithReceiverTokenLedger"),
            ("swap_tob_with_receiver_token_ledger_v3", "SwapTobWithReceiverTokenLedgerV3"),
            ("swap_tob_with_receiver_v3", "SwapTobWithReceiverV3"),
            ("swap_tob_with_token_ledger", "SwapTobWithTokenLedger"),
            ("swap_tob_with_token_ledger_v3", "SwapTobWithTokenLedgerV3"),
            ("swap_toc", "SwapToc"),
            ("swap_toc_v2", "SwapTocV2"),
            ("swap_toc_v3", "SwapTocV3"),
            ("wrap_unwrap", "WrapUnwrap"),
            ("wrap_unwrap_with_receiver", "WrapUnwrapWithReceiver"),
        ],
    ),
    // debot_router - self-named in its on-chain IDL.
    (
        "G7MVcM9YzGxrmLtmobUgyt8A6WhQ2dgQX3aSJcPejdEp",
        &[
            ("create_token_account", "CreateTokenAccount"),
            ("create_token_account_with_seed", "CreateTokenAccountWithSeed"),
            ("initialize_router_fee_config", "InitializeRouterFeeConfig"),
            ("recover_stuck_sol", "RecoverStuckSol"),
            ("recover_stuck_token", "RecoverStuckToken"),
            ("recover_stuck_wsol", "RecoverStuckWsol"),
            ("set_fee_rate", "SetFeeRate"),
            ("set_fee_recipient", "SetFeeRecipient"),
            ("swap", "Swap"),
            ("swap_compact", "SwapCompact"),
        ],
    ),

    // ── DEX venues: swap-family instructions only; admin ixs fall through
    //    to a key. These barely appear on a curve tape but are on the path
    //    once a token graduates. ─────────────────────────────────────────
    // Raydium CLMM (concentrated liquidity).
    (
        "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK",
        &[
            ("swap", "Swap"),
            ("swap_v2", "SwapV2"),
            ("swap_router_base_in", "SwapRouterBaseIn"),
        ],
    ),
    // Raydium CPMM (constant-product v2).
    (
        "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C",
        &[
            ("swap_base_input", "SwapBaseInput"),
            ("swap_base_output", "SwapBaseOutput"),
        ],
    ),
    // Meteora DLMM.
    (
        "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo",
        &[
            ("swap", "Swap"),
            ("swap2", "Swap2"),
            ("swap_exact_out", "SwapExactOut"),
            ("swap_with_price_impact", "SwapWithPriceImpact"),
        ],
    ),
    // Meteora DAMM V2.
    ("cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG", &[("swap", "Swap")]),
    // Meteora DBC (dynamic bonding curve).
    ("dbcij3LWUppWqq96dh6gJWwBifmcGfLSB5D4DuSMaqN", &[("swap", "Swap")]),
    // Moonshot.
    (
        "MoonCVVNZFSYkqNXP6bxHLPL6QQJiMagDL3qcqUQTrG",
        &[
            ("buy", "Buy"),
            ("sell", "Sell"),
            ("token_mint", "TokenMint"),
            ("migrate_funds", "MigrateFunds"),
        ],
    ),

    // ── Retail front-ends. Names below are read from each program's own
    //    `Program log: Instruction:` lines and re-derive the discriminator
    //    that produced them; `router_names_are_reachable…` pins that. ────
    // Terminal.
    (
        "term9YPb9mzAsABaqN71A4xdbxHmpBNZavpBiQKZzN3",
        &[
            ("validate_nonce", "ValidateNonce"),
            ("route_open", "RouteOpen"),
            ("route_close", "RouteClose"),
            ("v2_buy_exact_in_pump_fun", "V2BuyExactInPumpFun"),
            ("v2_sell_exact_in_pump_fun", "V2SellExactInPumpFun"),
            ("v2_sell_exact_out_pump_fun", "V2SellExactOutPumpFun"),
        ],
    ),
    // GMGN Bot.
    (
        "GMgnVFR8Jb39LoXsEVzb3DvBy3ywCmdmJquHUy1Lrkqb",
        &[
            ("buy", "Buy"),
            ("sell", "Sell"),
        ],
    ),
    // GMGN Bot, second deployment.
    (
        "GMGNreQcJFufBiCTLDBgKhYEfEe9B454UjpDr5CaSLA1",
        &[
            ("swap", "Swap"),
        ],
    ),
    // Photon.
    (
        "BSfD6SHZigAfDWSjzD5Q41jw8LmKwtmjskPH9XW1mrRW",
        &[
            ("collect_fee", "CollectFee"),
            ("two_hop_swap", "TwoHopSwap"),
            ("pump_buy_v2", "PumpBuyV2"),
            ("pump_sell_v2", "PumpSellV2"),
        ],
    ),
    // Trojan.
    (
        "troyXT7Ty3s2rjJe4bqWaroUrS4Fjd8rbHHNHxcACF4",
        &[
            ("fee_transfer_with_tip", "FeeTransferWithTip"),
            ("fee_transfer", "FeeTransfer"),
            ("token_fee_transfer_with_tip", "TokenFeeTransferWithTip"),
            ("sell", "Sell"),
        ],
    ),
    // Trojan, second deployment (same instruction set).
    (
        "TroYL71c8P2XNtDxHs98VtVLuiASJ7Ao5FvUoKyp3Bk",
        &[
            ("fee_transfer_with_tip", "FeeTransferWithTip"),
            ("fee_transfer", "FeeTransfer"),
            ("token_fee_transfer_with_tip", "TokenFeeTransferWithTip"),
            ("sell", "Sell"),
        ],
    ),
    // Maestro.
    (
        "MaestroAAe9ge5HTc64VbBQZ6fP77pwvrhM8i1XWSAx",
        &[
            ("multi_swap2", "MultiSwap2"),
            ("save_balance", "SaveBalance"),
            ("check_balance_and_transfer", "CheckBalanceAndTransfer"),
            ("save_tax_state", "SaveTaxState"),
        ],
    ),
    // Unnamed router - the busiest unnamed program on the tape.
    (
        "6Vo3245eszAb5wuqEMw8mGdbfRUdKbHhDHP5LcaGuTAB",
        &[
            ("pump_swap_v3", "PumpSwapV3"),
            ("bonding_curve_v3", "BondingCurveV3"),
            ("sell_pump_swap_percentage", "SellPumpSwapPercentage"),
            ("sell_bonding_curve_percentage", "SellBondingCurvePercentage"),
            ("sell_pump_swap_exact_quote_out", "SellPumpSwapExactQuoteOut"),
            ("create_coin_and_buy_bonding_curve_v3", "CreateCoinAndBuyBondingCurveV3"),
        ],
    ),
    // Unnamed router.
    (
        "m9obQHAPyZeZ88w7XUY8Te6a8rBmhXsfKco7u8G8tnB",
        &[
            ("pump_bonding_curve_buy_with_volume_v2", "PumpBondingCurveBuyWithVolumeV2"),
            ("pump_swap_sell", "PumpSwapSell"),
            ("pump_swap_buy_with_volume", "PumpSwapBuyWithVolume"),
            ("pump_bonding_curve_sell", "PumpBondingCurveSell"),
            ("usdc_sell_pump_swap", "UsdcSellPumpSwap"),
            ("usdc_buy_pump_bonding_curve", "UsdcBuyPumpBondingCurve"),
            ("usdc_sell_pump_bonding_curve", "UsdcSellPumpBondingCurve"),
        ],
    ),
    // Unnamed pre/post-swap balance guard.
    (
        "CxvksNjwhdHDLr3qbCXNKVdeYACW8cs93vFqLqtgyFE5",
        &[
            ("close_ata_if_empty", "CloseAtaIfEmpty"),
            ("pre_token_swap_v2", "PreTokenSwapV2"),
            ("post_token_swap_v2", "PostTokenSwapV2"),
            ("pre_token_swap", "PreTokenSwap"),
            ("post_token_swap", "PostTokenSwap"),
            ("pre_sol_swap", "PreSolSwap"),
            ("post_sol_swap", "PostSolSwap"),
            ("pre_token_swap_v2_native_sol", "PreTokenSwapV2NativeSol"),
            ("post_token_swap_v2_native_sol", "PostTokenSwapV2NativeSol"),
            ("pre_token_swap_v3", "PreTokenSwapV3"),
            ("post_token_swap_v3", "PostTokenSwapV3"),
        ],
    ),
    // Unnamed limit-order router.
    (
        "AveaiuA1emN71q9mS2QQ9BEWNAAHmp8sHSvwLFHQjufM",
        &[
            ("route_swap", "RouteSwap"),
            ("close_order", "CloseOrder"),
            ("cancel_order", "CancelOrder"),
            ("create_market_token_vault", "CreateMarketTokenVault"),
            ("create_order", "CreateOrder"),
            ("fulfill_order_via_route_swap", "FulfillOrderViaRouteSwap"),
        ],
    ),
    // Unnamed router (bonding-curve / AMM split).
    (
        "4KRSet9YXoCKDamKQfoTc4MsnPU4w847KhPoYBcrRooY",
        &[
            ("sell_bc", "SellBc"),
            ("sell_amm", "SellAmm"),
            ("buy_bc", "BuyBc"),
        ],
    ),
    // Unnamed sell-side router.
    (
        "H5k1PVjAiZxvca3Fix3LKtpS7GJB6h3JinqZoiDx2N54",
        &[
            ("sell_pump_tokens", "SellPumpTokens"),
            ("sell_pumpswap_tokens", "SellPumpswapTokens"),
        ],
    ),
    // Unnamed sell-side router.
    (
        "7bopoA2tnvDYEfp54FhgNDiLR9wSPcEs9J7FaUGURsXj",
        &[
            ("sell_pump_pumpswap", "SellPumpPumpswap"),
        ],
    ),
    // Unnamed balance guard.
    (
        "tRunsb6NB127ES14E5Y6pUf3dfJC8DJMPTcbEaWZSLe",
        &[
            ("balance_below", "BalanceBelow"),
        ],
    ),
    // Unnamed forwarder.
    (
        "pumpapii17v9uhRiokHwhG3yWYtB6hgJRmsSC5p5bb2",
        &[
            ("forward", "Forward"),
        ],
    ),
];

/// How the stable instruction key is cut out of `data` for one program.
///
/// The key is what an unnamed instruction is identified BY, so its width decides
/// label cardinality: too wide and a `u64` argument forks one instruction into
/// thousands of labels (which would make `ix_hash` unique per trade and destroy
/// the fingerprint grouping); too narrow and two instructions merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IxKey {
    /// 8-byte leading dispatch value — Anchor's discriminator, or a custom one
    /// of the same width. Safe to take whole because it carries no arguments.
    Disc8,
    /// The first byte dispatches and the rest are arguments. The conservative
    /// default: bounded at 256 labels for a program we know nothing about, and
    /// what every measured non-Anchor router on this tape actually does.
    Tag1,
}

/// Programs whose key width is not the default (`Disc8` when the program has an
/// instruction table, `Tag1` otherwise). Every entry is a measured shape, not a
/// guess — `decode-harvest` prints the observed key distribution it came from.
const IX_KEY_OVERRIDE: &[(&str, IxKey)] = &[
    // Anchor-shaped dispatch, instruction logs suppressed: keys but no names.
    ("b1oomGGqPKGD6errbyfbVMBuzSC8WtAAYo8MwNafWW1", IxKey::Disc8),
    ("haqqqMGN35ehCftXca3KWxJFXTBcWTWeNHNtUHLGQdh", IxKey::Disc8),
    ("9ddjzqYhSTMHaBrrKukRXRfy4WzHUPjdX88uPXZ7MXyn", IxKey::Disc8),
];

/// One [`EXPLICIT_IX`] row: program id, key width, and its `(key_hex, display)` pairs.
type ExplicitIxRow = (&'static str, IxKey, &'static [(&'static str, &'static str)]);

/// `(program_id, key_width, &[(key_hex, display_name)])` — programs that log an
/// instruction name but do NOT derive their dispatch value from it.
///
/// Tier 2: the key bytes are transcribed rather than computed, so a typo here
/// yields a label attached to the wrong instruction instead of no label at all.
/// `decode-harvest` re-derives every row from chain; keep the list short.
const EXPLICIT_IX: &[ExplicitIxRow] = &[
    // 8-byte dispatch values that are not `sha256("global:<name>")`.
    (
        "J7pourVwqP1VtRNBiFNqbdGxwkx7ta2LHAURURRRJqmd",
        IxKey::Disc8,
        &[
            ("4be09fdde54f2eb3", "SellPumpfun"),
            ("a1476cd877c2b059", "BuyPumpfun"),
            // sha256("global:init_user_wsol_ata")[..8] — this one IS anchor-hashed,
            // but a program lives in exactly one table, so it is written out here.
            ("1fa3ccdce0393068", "InitUserWsolAta"),
        ],
    ),
    // One-byte dispatch tag followed by arguments.
    (
        "B3111yJCeHBcA1bizdJjUFPALfhAfSRnAbJzGUtnt56A",
        IxKey::Tag1,
        &[
            ("01", "Swap"),
            ("03", "SwapWithFeeLog"),
            ("04", "SwapWithSolFeeLog"),
            ("05", "TakeFeeLog"),
            ("06", "TakeSolFeeLog"),
            ("07", "BatchCreateTokenAccounts"),
            ("08", "SyncNative"),
            ("09", "BatchCloseTokenAccounts"),
        ],
    ),
];

/// The 8-byte Anchor instruction discriminator for `name`
/// (`sha256("global:<name>")[..8]`). `solana_sdk::hash::hash` is SHA-256.
fn anchor_discriminator(name: &str) -> [u8; 8] {
    let digest = solana_sdk::hash::hash(format!("global:{name}").as_bytes());
    let mut disc = [0u8; 8];
    disc.copy_from_slice(&digest.to_bytes()[..8]);
    disc
}

/// One program's instruction table, already reduced to `key_hex → display`.
struct IxTable {
    key: IxKey,
    names: HashMap<String, &'static str>,
}

fn ix_tables() -> &'static HashMap<&'static str, IxTable> {
    static MAP: OnceLock<HashMap<&'static str, IxTable>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut out: HashMap<&'static str, IxTable> = HashMap::new();
        for (program_id, ixs) in ANCHOR_IX {
            let names = ixs
                .iter()
                .map(|(name, display)| (hex(&anchor_discriminator(name)), *display))
                .collect();
            out.insert(program_id, IxTable { key: IxKey::Disc8, names });
        }
        for (program_id, key, ixs) in EXPLICIT_IX {
            let names = ixs.iter().map(|(k, display)| ((*k).to_string(), *display)).collect();
            out.insert(program_id, IxTable { key: *key, names });
        }
        out
    })
}

fn key_overrides() -> &'static HashMap<&'static str, IxKey> {
    static MAP: OnceLock<HashMap<&'static str, IxKey>> = OnceLock::new();
    MAP.get_or_init(|| IX_KEY_OVERRIDE.iter().map(|(id, k)| (*id, *k)).collect())
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// The key width this program's instructions are identified by.
///
/// An override wins; otherwise a program with an instruction table is keyed the
/// way that table is, and everything else falls back to the one-byte tag — the
/// only width that cannot fork a `u64` argument into thousands of labels.
pub(super) fn program_ix_key(program_id: &str) -> IxKey {
    if let Some(k) = key_overrides().get(program_id) {
        return *k;
    }
    ix_tables().get(program_id).map(|t| t.key).unwrap_or(IxKey::Tag1)
}

/// The stable instruction key for `data` under `key`, or `None` when `data` is
/// too short to carry one.
pub(super) fn instruction_key(key: IxKey, data: &[u8]) -> Option<String> {
    match key {
        IxKey::Disc8 => data.get(..8).map(hex),
        IxKey::Tag1 => data.get(..1).map(hex),
    }
}

/// Display name for the instruction `data` invokes on `program_id`, or `None`
/// when the program has no table or its key is not in it.
pub fn program_instruction_name(program_id: &str, data: Option<&[u8]>) -> Option<&'static str> {
    let table = ix_tables().get(program_id)?;
    let key = instruction_key(table.key, data?)?;
    table.names.get(&key).copied()
}

/// The instruction half of a label: the name when we can prove one, else the
/// stable key (`ix#<key>`), else `Unknown` when there is no data to key on.
///
/// Never returns a *guess*. `ix#…` says "this is a distinct instruction and here
/// is its identity", which is what the metric system needs; the name is a bonus.
pub fn program_instruction_label(program_id: &str, data: Option<&[u8]>) -> String {
    if let Some(name) = program_instruction_name(program_id, data) {
        return name.to_owned();
    }
    match data.and_then(|d| instruction_key(program_ix_key(program_id), d)) {
        Some(key) => format!("ix#{key}"),
        None => "Unknown".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Protocol;

    #[test]
    fn native_and_venue_ids_resolve() {
        assert_eq!(program_friendly_name(PUMP_FUN_PROGRAM_ID), Some("Pump.Fun"));
        assert_eq!(
            program_friendly_name("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8"),
            Some("Raydium AMM v4"),
        );
        assert_eq!(program_friendly_name("definitely-not-a-program"), None);
    }

    #[test]
    fn registry_has_no_duplicate_ids() {
        // A dup would mean the last row silently wins — catch it at build time.
        assert_eq!(registry().len(), REGISTRY.len(), "duplicate program id in REGISTRY");
    }

    #[test]
    fn ix_tables_have_no_duplicate_programs() {
        // ANCHOR_IX and EXPLICIT_IX share one map: a program in both would have
        // the explicit rows silently replace the computed ones.
        let anchor: Vec<&str> = ANCHOR_IX.iter().map(|(id, _)| *id).collect();
        let explicit: Vec<&str> = EXPLICIT_IX.iter().map(|(id, _, _)| *id).collect();
        let mut all = anchor.clone();
        all.extend(explicit.iter().copied());
        let unique: std::collections::BTreeSet<&str> = all.iter().copied().collect();
        assert_eq!(unique.len(), all.len(), "a program id appears in two ix tables");
        assert_eq!(ix_tables().len(), unique.len());
    }

    #[test]
    fn anchor_names_do_not_collide_within_a_program() {
        // Two names hashing to one discriminator would drop an instruction from
        // the table silently (the map keeps one).
        for (program_id, ixs) in ANCHOR_IX {
            let table = ix_tables().get(program_id).expect("table built");
            assert_eq!(
                table.names.len(),
                ixs.len(),
                "discriminator collision in {program_id}",
            );
        }
    }

    #[test]
    fn method_reproduces_pump_discriminators() {
        // pump.fun is an Anchor program with KNOWN-correct discriminators in
        // `protocol.rs`. Reproducing EVERY curve/admin one from its instruction
        // name proves `anchor_discriminator` really is `sha256("global:<name>")[..8]`
        // — so the computed router/venue discriminators are correct by
        // construction (given the right instruction name), and it holds the two
        // copies of each pump discriminator (the struct's bytes and this table's
        // name) equal, which is the only thing keeping them from drifting.
        let d = &Protocol::pump_fun().discriminators;
        for (name, bytes) in [
            ("buy", d.buy),
            ("sell", d.sell),
            ("sell_v2", d.sell_v2),
            ("buy_exact_sol_in", d.buy_exact_sol_in),
            ("buy_exact_quote_in", d.buy_exact_quote_in),
            ("buy_v2", d.buy_v2),
            ("buy_exact_quote_in_v2", d.buy_exact_quote_in_v2),
            ("create", d.create_ix),
            ("create_v2", d.create_v2_ix),
            ("migrate", d.migrate_ix),
            ("migrate_v2", d.migrate_v2_ix),
            ("initialize", d.initialize),
            ("set_params", d.set_params),
            ("withdraw", d.withdraw),
            ("extend_account", d.extend_account),
            ("collect_creator_fee", d.collect_creator_fee),
        ] {
            assert_eq!(anchor_discriminator(name), bytes, "discriminator drift on `{name}`");
        }
    }

    #[test]
    fn known_program_ix_resolves_and_unknown_falls_through_to_a_key() {
        let jup = "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4";
        let route = anchor_discriminator("route");
        assert_eq!(program_instruction_name(jup, Some(&route)), Some("Route"));
        assert_eq!(program_instruction_name(jup, Some(&[0u8; 8])), None);
        // An unrecognised discriminator still yields a stable identity.
        assert_eq!(program_instruction_label(jup, Some(&[0u8; 8])), "ix#0000000000000000");
        // Program with no ix table → keyed by its first byte, never named.
        assert_eq!(
            program_instruction_name("AddressLookupTab1e1111111111111111111111111", Some(&route)),
            None,
        );
        assert_eq!(
            program_instruction_label("AddressLookupTab1e1111111111111111111111111", Some(&route)),
            format!("ix#{:02x}", route[0]),
        );
        // No data at all stays `Unknown` — that is the feed telling us it lost
        // the bytes, and it must not be dressed up as an instruction identity.
        assert_eq!(program_instruction_label(jup, None), "Unknown");
        assert_eq!(program_instruction_label(jup, Some(&[])), "Unknown");
    }

    #[test]
    fn explicit_and_override_keys_apply() {
        // 8-byte custom dispatch (not anchor-hashed).
        let jpour = "J7pourVwqP1VtRNBiFNqbdGxwkx7ta2LHAURURRRJqmd";
        let disc = [0x4b, 0xe0, 0x9f, 0xdd, 0xe5, 0x4f, 0x2e, 0xb3];
        assert_eq!(program_instruction_label(jpour, Some(&disc)), "SellPumpfun");
        // One-byte dispatch tag: the trailing u64 argument must not fork it.
        let b3 = "B3111yJCeHBcA1bizdJjUFPALfhAfSRnAbJzGUtnt56A";
        assert_eq!(program_instruction_label(b3, Some(&[0x04, 0, 0xa3, 0xe1, 0x11, 0, 0, 0])), "SwapWithSolFeeLog");
        assert_eq!(program_instruction_label(b3, Some(&[0x04, 0, 0x2d, 0x31, 0x01, 0, 0, 0])), "SwapWithSolFeeLog");
        // Axiom logs no instruction names, so it gets identity only. `decode-harvest`
        // measures exactly two leading tags across its traffic (`00` and `01`) —
        // reading eight bytes instead would fold the u64 argument into the key and
        // fork one instruction into a hundred labels.
        let axiom = "FLASHX8DrLbgeR8FcfNV1F5krxYcYMUdBkrP1EPBtxB9";
        assert_eq!(program_ix_key(axiom), IxKey::Tag1);
        assert_eq!(program_instruction_label(axiom, Some(&[0x01, 0xc0, 0x19, 0x81, 0x1d, 0, 0, 0, 0, 0])), "ix#01");
        assert_eq!(program_instruction_label(axiom, Some(&[0x00, 0x2d, 0x31, 0x01, 0, 0, 0, 0, 0, 0])), "ix#00");
    }

    #[test]
    fn router_names_are_reachable_from_their_observed_discriminators() {
        // The discriminators below were read off chain next to the program's own
        // `Program log: Instruction:` line. They are the evidence the router
        // names rest on; if a name is ever edited, this stops resolving.
        for (program_id, disc_hex, expect) in [
            ("term9YPb9mzAsABaqN71A4xdbxHmpBNZavpBiQKZzN3", "a0bbe397d2052255", "V2BuyExactInPumpFun"),
            ("term9YPb9mzAsABaqN71A4xdbxHmpBNZavpBiQKZzN3", "def8e0a674678e2f", "V2SellExactOutPumpFun"),
            ("GMgnVFR8Jb39LoXsEVzb3DvBy3ywCmdmJquHUy1Lrkqb", "33e685a4017f83ad", "Sell"),
            ("BSfD6SHZigAfDWSjzD5Q41jw8LmKwtmjskPH9XW1mrRW", "1b4f0265289c23b3", "PumpBuyV2"),
            ("troyXT7Ty3s2rjJe4bqWaroUrS4Fjd8rbHHNHxcACF4", "4d4df51d1cf91bee", "FeeTransferWithTip"),
            ("MaestroAAe9ge5HTc64VbBQZ6fP77pwvrhM8i1XWSAx", "8409d42d2771d736", "MultiSwap2"),
            ("6Vo3245eszAb5wuqEMw8mGdbfRUdKbHhDHP5LcaGuTAB", "af051981a0d8389d", "PumpSwapV3"),
            ("6Vo3245eszAb5wuqEMw8mGdbfRUdKbHhDHP5LcaGuTAB", "f4f8e72d8fd1dc8f", "SellBondingCurvePercentage"),
            ("m9obQHAPyZeZ88w7XUY8Te6a8rBmhXsfKco7u8G8tnB", "096a17f82988f538", "PumpSwapSell"),
            ("tRunsb6NB127ES14E5Y6pUf3dfJC8DJMPTcbEaWZSLe", "fe4e6f93cfd683df", "BalanceBelow"),
        ] {
            let mut data = [0u8; 8];
            for (i, b) in data.iter_mut().enumerate() {
                *b = u8::from_str_radix(&disc_hex[i * 2..i * 2 + 2], 16).unwrap();
            }
            assert_eq!(
                program_instruction_name(program_id, Some(&data)),
                Some(expect),
                "{program_id} {disc_hex}",
            );
        }
    }
}
