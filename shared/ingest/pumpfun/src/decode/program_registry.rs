//! Program-ID → friendly-name registry for the instruction labeler.
//!
//! This is the single source of truth for turning a raw program address into a
//! human name (`"Axiom Trade"`, `"Raydium AMM v4"`, …). It is consulted only by
//! the display/analytics label path ([`super::instructions::label_instruction`])
//! — never the per-trade decode hot loop. Lookup is O(1) against a `HashMap`
//! built once.
//!
//! **Growing the table:** run `cargo run -p hunter-live -- unknown-programs`,
//! which ranks the program IDs still rendering as `Unknown (<program id>)` in the
//! persisted `trades.ix_labels` (the labeler now embeds the full id there — see
//! [`super::instructions::label_instruction`]). Look the top offenders up on
//! Solscan once, then add a `(program_id, name)` row below. Prefer *no* entry
//! over a guessed one — a wrong label is worse than `Unknown`.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::protocol::{
    ASSOCIATED_TOKEN_PROGRAM_ID, COMPUTE_BUDGET_PROGRAM_ID, PUMP_FUN_PROGRAM_ID,
    PUMP_SWAP_PROGRAM_ID, SYSTEM_PROGRAM_ID, TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID,
};

/// `(program_id_base58, friendly_name)`. Keep the blocks ordered by confidence:
/// native/SPL and well-known DEX venues are stable; the aggregator/bot block is
/// volatile — verify each against Solscan before trusting it.
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
    ("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr", "Memo Program"),

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
    ("DF1ow4tspfHX9JwWJsAb9epbkA8hmpSEAtxXy1V27QBH", "DFlow Aggregator"),

    // ── Pre-existing entries — UNVERIFIED. Confirm on Solscan or replace via the
    //    `unknown-programs` harvest; kept non-destructively, not trusted. ──────
    ("FLASHX8DrLbgeR8FcfNV1F5krxYcYMUdBkrP1EPBtxB9", "Axiom Trade"),
    ("BSfD6SHZigAfDWSjzD5Q41jw8LmKwtmjskPH9XW1mrRW", "Photon"),
    ("GMgnVFR8Jb39LoXsEVzb3DvBy3ywCmdmJquHUy1Lrkqb", "GMGN Bot"),
    ("term9YPb9mzAsABaqN71A4xdbxHmpBNZavpBiQKZzN3", "Terminal"),
    ("troyXT7Ty3s2rjJe4bqWaroUrS4Fjd8rbHHNHxcACF4", "Trojan Trade"),
    ("b1oomGGqPKGD6errbyfbVMBuzSC8WtAAYo8MwNafWW1", "Bloom Router"),
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

// ── Instruction-level decode for open (Anchor) programs (Phase 4) ─────────────
//
// The registry names the *program*; this names the *instruction* within it, so a
// labelled ix goes from `"Jupiter Aggregator V6: Unknown"` to
// `"Jupiter Aggregator V6: Route"`. Only Anchor programs are covered — their
// 8-byte instruction discriminator is `sha256("global:<snake_case_name>")[..8]`,
// which we COMPUTE from the name rather than hard-code. That is fail-safe: if a
// name here is wrong (or the program isn't the Anchor variant we assumed), the
// computed discriminator simply won't match any on-chain ix and the label stays
// `"…: Unknown"` — a wrong *name* never yields a wrong *label*. Non-Anchor
// programs (e.g. Raydium AMM v4's 1-byte tag) are intentionally omitted and stay
// `Unknown`. `program_registry_tests::method_reproduces_pump_discriminators`
// pins the SHA-256 mechanism against pump.fun's known-correct discriminators.

/// `(program_id, &[(on_chain_snake_name, display_name)])`. Instruction names are
/// the identifiers Anchor hashes (snake_case), not the camelCase IDL spelling.
/// Only swap/route-family ixs are listed — the ones that actually appear on the
/// trade path; admin ixs are left to fall through to `Unknown`.
const ANCHOR_IX: &[(&str, &[(&str, &str)])] = &[
    // Jupiter Aggregator V6 — the dominant aggregator on the trade path.
    (
        "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4",
        &[
            ("route", "Route"),
            ("route_with_token_ledger", "RouteWithTokenLedger"),
            ("shared_accounts_route", "SharedAccountsRoute"),
            ("shared_accounts_route_with_token_ledger", "SharedAccountsRouteWithTokenLedger"),
            ("exact_out_route", "ExactOutRoute"),
            ("shared_accounts_exact_out_route", "SharedAccountsExactOutRoute"),
        ],
    ),
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
];

/// The 8-byte Anchor instruction discriminator for `name`
/// (`sha256("global:<name>")[..8]`). `solana_sdk::hash::hash` is SHA-256.
fn anchor_discriminator(name: &str) -> [u8; 8] {
    let digest = solana_sdk::hash::hash(format!("global:{name}").as_bytes());
    let mut disc = [0u8; 8];
    disc.copy_from_slice(&digest.to_bytes()[..8]);
    disc
}

#[allow(clippy::type_complexity)]
fn anchor_ix_table() -> &'static HashMap<&'static str, Vec<([u8; 8], &'static str)>> {
    static MAP: OnceLock<HashMap<&'static str, Vec<([u8; 8], &'static str)>>> = OnceLock::new();
    MAP.get_or_init(|| {
        ANCHOR_IX
            .iter()
            .map(|(program_id, ixs)| {
                let decoded = ixs
                    .iter()
                    .map(|(name, display)| (anchor_discriminator(name), *display))
                    .collect();
                (*program_id, decoded)
            })
            .collect()
    })
}

/// Display name for the instruction `data` invokes on program `program_id`, or
/// `None` when the program has no ix table or its discriminator doesn't match one
/// (caller then labels it `"<program>: Unknown"`).
pub fn program_instruction_name(program_id: &str, data: Option<&[u8]>) -> Option<&'static str> {
    let table = anchor_ix_table().get(program_id)?;
    let disc = data?.get(..8)?;
    table
        .iter()
        .find(|(d, _)| d.as_slice() == disc)
        .map(|(_, name)| *name)
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
    fn method_reproduces_pump_discriminators() {
        // pump.fun is an Anchor program with KNOWN-correct discriminators in
        // `protocol.rs`. Reproducing them from the instruction name proves
        // `anchor_discriminator` really is `sha256("global:<name>")[..8]` — so the
        // computed Jupiter/Raydium/Meteora discriminators are correct by
        // construction (given the right instruction name).
        let d = &Protocol::pump_fun().discriminators;
        assert_eq!(anchor_discriminator("buy"), d.buy);
        assert_eq!(anchor_discriminator("sell"), d.sell);
        assert_eq!(anchor_discriminator("create"), d.create_ix);
        assert_eq!(anchor_discriminator("migrate"), d.migrate_ix);
    }

    #[test]
    fn known_program_ix_resolves_and_unknown_falls_through() {
        let jup = "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4";
        // A real `route` discriminator resolves; a bogus one falls through.
        let route = anchor_discriminator("route");
        assert_eq!(program_instruction_name(jup, Some(&route)), Some("Route"));
        assert_eq!(program_instruction_name(jup, Some(&[0u8; 8])), None);
        // Program with no ix table (Address Lookup Table) → None regardless.
        assert_eq!(
            program_instruction_name("AddressLookupTab1e1111111111111111111111111", Some(&route)),
            None,
        );
    }
}
