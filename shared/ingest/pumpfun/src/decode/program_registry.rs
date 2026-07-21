//! Program-ID → friendly-name registry for the instruction labeler.
//!
//! This is the single source of truth for turning a raw program address into a
//! human name (`"Axiom Trade"`, `"Raydium AMM v4"`, …). It is consulted only by
//! the display/analytics label path ([`super::instructions::label_instruction`])
//! and the unknown-program harvest ([`super::analyze`]) — never the per-trade
//! decode hot loop. Lookup is O(1) against a `HashMap` built once.
//!
//! **Growing the table:** run `cargo run -p hunter-live -- unknown-programs`,
//! which ranks the program IDs in your own ingested `raw_txs` that still fall
//! through to `Unknown (...)`. Look the top offenders up on Solscan once, then
//! add a `(program_id, name)` row below. Prefer *no* entry over a guessed one —
//! a wrong label is worse than `Unknown`.

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
/// `Unknown (...suffix)`).
pub fn program_friendly_name(id: &str) -> Option<&'static str> {
    registry().get(id).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
