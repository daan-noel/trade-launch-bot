//! Address Lookup Table (ALT) content — the SSOT list of **immutable** pump.fun
//! accounts a launch tx references.
//!
//! A create_v2 + dev-buy names ~27 accounts; ~15 of them never vary across
//! launches (program IDs, constant-seed PDAs, the Jito tip accounts). Pre-loading
//! those into one persistent on-chain ALT lets the launch tx reference them by a
//! 1-byte index instead of a 32-byte key, dropping it from ~1267 B (over the
//! 1232 B legacy-message limit) to ~840 B.
//!
//! Deliberately EXCLUDED (kept inline in the tx):
//!   - per-launch/per-wallet PDAs (mint, bonding curve, ATA, creator vault,
//!     user-volume accumulator, mayhem state/vault, …) — they change every launch,
//!     so they can't live in a persistent table;
//!   - the on-chain `Global.fee_recipient` — its *value* is governance-rotatable,
//!     so caching its current value in the ALT could silently stale the table. Its
//!     32 bytes inline are cheap against the ~400 B the immutable set already saves.
//!
//! Every entry here is either a compile-time program ID / recipient constant or a
//! PDA derived from **constant seeds** (its address is fixed even if the account's
//! contents change), so the provisioned ALT never needs re-issuing.

use crate::protocol;
use solana_sdk::{compute_budget, pubkey::Pubkey, system_program, sysvar};

/// The immutable accounts the launch (create + dev-buy) path references, for
/// pre-loading into the persistent launch ALT. Covers both `create_v1`
/// (Metaplex) and `create_v2` (Mayhem) plus the curve dev-buy and Jito tip — so
/// ONE table serves every launch variant. Order is stable but irrelevant (the ALT
/// is content-addressed by membership, and v0 compilation matches by pubkey).
pub fn launch_alt_addresses() -> Vec<Pubkey> {
    let mut addrs = vec![
        // --- programs / sysvars ---
        system_program::id(),
        compute_budget::id(),
        sysvar::rent::id(),
        protocol::PUMP_FUN,
        protocol::EVENT_AUTHORITY,
        protocol::FEE_PROGRAM,
        protocol::TOKEN,
        protocol::TOKEN_2022,
        protocol::ASSOCIATED_TOKEN_PROGRAM,
        protocol::MPL_TOKEN_METADATA,
        protocol::MAYHEM_PROGRAM,
        // --- fixed recipient ---
        protocol::PUMP_CURVE_FEE_RECIPIENT,
        // --- constant-seed PDAs (fixed address; contents may change) ---
        pda(&[b"mint-authority"], &protocol::PUMP_FUN),
        pda(&[b"global"], &protocol::PUMP_FUN),
        pda(&[b"global_volume_accumulator"], &protocol::PUMP_FUN),
        pda(&[b"fee_config", protocol::PUMP_FUN.as_ref()], &protocol::FEE_PROGRAM),
        pda(&[b"global-params"], &protocol::MAYHEM_PROGRAM),
        pda(&[b"sol-vault"], &protocol::MAYHEM_PROGRAM),
    ];
    // All Jito tip accounts — the engine picks one at random per instance, so the
    // table must cover every candidate for the ALT to compress that leg.
    addrs.extend_from_slice(&protocol::JITO_TIP_ACCOUNTS);
    addrs
}

fn pda(seeds: &[&[u8]], program: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(seeds, program).0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn address_set_is_deduped_and_covers_tip_accounts() {
        let addrs = launch_alt_addresses();
        let unique: HashSet<_> = addrs.iter().collect();
        assert_eq!(unique.len(), addrs.len(), "ALT address list has duplicates");
        for tip in protocol::JITO_TIP_ACCOUNTS {
            assert!(addrs.contains(&tip), "tip account {tip} missing from ALT set");
        }
        // Well under the 256-address ALT cap, with headroom for future accounts.
        assert!(addrs.len() < 256);
    }
}
