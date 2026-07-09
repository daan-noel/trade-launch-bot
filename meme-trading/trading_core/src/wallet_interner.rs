//! Wallet-address interning, shared by the live token cache (core) and the sweep
//! projection (local). Lives in core so `state::token_cache` can hold one without
//! depending on the local `sweep` crate; `sweep::projection` re-exports it.

use std::collections::HashMap;

/// Interns wallet addresses to dense token-local `u32` ids in first-seen order.
/// The `Vec` (`u32 → address`) maps an id back to its address.
///
/// The sweep discards the map once a token is projected (keeping only the table for
/// the Parquet write). The **live token cache** keeps a `WalletInterner` resident on
/// each `TokenState` so it can intern every appended trade's wallet to a `u32` (Phase
/// B step 2) — hence `Clone` (the state derives it).
#[derive(Default, Clone)]
pub struct WalletInterner {
    by_addr: HashMap<String, u32>,
    table: Vec<Box<str>>,
}

impl WalletInterner {
    pub fn intern(&mut self, addr: &str) -> u32 {
        if let Some(&id) = self.by_addr.get(addr) {
            return id;
        }
        let id = self.table.len() as u32;
        self.table.push(Box::from(addr));
        self.by_addr.insert(addr.to_string(), id);
        id
    }

    /// Look up the interned id for `addr` without inserting. Returns `None` when
    /// the address has never been seen on this token's trade history.
    pub fn id_of(&self, addr: &str) -> Option<u32> {
        self.by_addr.get(addr).copied()
    }

    /// The finished `u32 → address` table (consuming form, for the sweep projection).
    pub fn into_table(self) -> Vec<Box<str>> {
        self.table
    }
}
