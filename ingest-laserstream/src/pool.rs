//! Self-contained pool→mint index + PumpSwap pool PDA derivation.
//!
//! The decode task and the host both update the index via [`PoolIndex`]; the
//! transport task reads it to build per-pool gRPC subscriptions.

use std::str::FromStr;
use std::sync::Arc;

use dashmap::DashMap;
use solana_sdk::pubkey::Pubkey;
use tracing::warn;

use crate::protocol::Protocol;

/// Shared pool address → base mint address index.
///
/// - The decode task inserts on every `TokenMigrated` (auto-discovery).
/// - The host inserts known-active pools at warm-restart via
///   `IngestHandle::track_pools`.
/// - The transport task reads it to build gRPC account_include filters.
pub type PoolIndex = Arc<DashMap<String, String>>;

/// Derive the canonical PumpSwap pool PDA for a migrated pump.fun token.
///
/// Uses the fixed layout: pool-authority PDA seeded with [mint] on the pump
/// program, then pool PDA seeded with [0u16, authority, mint, WSOL] on the
/// pump-swap program.
pub fn derive_pool(mint: &str, protocol: &Protocol) -> Option<String> {
    let mint_pk = Pubkey::from_str(mint)
        .map_err(|e| warn!("derive_pool: invalid mint '{mint}': {e}"))
        .ok()?;
    let pump_pk = Pubkey::from_str(&protocol.programs.pump_fun.base58)
        .map_err(|e| warn!("derive_pool: invalid pump program: {e}"))
        .ok()?;
    let swap_pk = Pubkey::from_str(&protocol.programs.pump_swap.base58)
        .map_err(|e| warn!("derive_pool: invalid pump-swap program: {e}"))
        .ok()?;
    let wsol_pk = Pubkey::from_str(&protocol.programs.wsol.base58)
        .map_err(|e| warn!("derive_pool: invalid wsol: {e}"))
        .ok()?;

    let (authority, _) =
        Pubkey::find_program_address(&[b"pool-authority", mint_pk.as_ref()], &pump_pk);
    let index: u16 = 0;
    let (pool, _) = Pubkey::find_program_address(
        &[
            b"pool",
            &index.to_le_bytes(),
            authority.as_ref(),
            mint_pk.as_ref(),
            wsol_pk.as_ref(),
        ],
        &swap_pk,
    );
    Some(pool.to_string())
}

/// Register a migrated token's pool in the index. Returns `true` if the pool
/// was newly inserted (caller should notify the transport to resubscribe).
pub fn register_pool(index: &PoolIndex, mint: &str, protocol: &Protocol) -> bool {
    match derive_pool(mint, protocol) {
        Some(pool) => index.insert(pool, mint.to_string()).is_none(),
        None => false,
    }
}

/// Return the pool PDA for a given mint (without modifying the index).
pub fn pool_for_mint(mint: &str, protocol: &Protocol) -> Option<String> {
    derive_pool(mint, protocol)
}
