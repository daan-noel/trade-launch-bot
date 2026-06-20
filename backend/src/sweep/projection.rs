//! The sweep's slim, wallet-interned per-token trade projection.
//!
//! The hot loop walks one of these per trade instead of the full [`Trade`]
//! (5 `String`s + `Uuid` + a JSON `Value` ≈ 250 B with heap indirection). It
//! carries **only** the fields the shared entry/exit/cohort fns read — see
//! [`TradeRow`] — and interns each token's wallets to a `u32` so cohort-set
//! membership in the inner walk is integer-keyed (no base58-String hashing or
//! clones). The projection is built **once per token** at corpus-load time and
//! reused across every (combo) evaluation; `Trade` never enters the sweep loop.
//!
//! Wallet ids are token-local: cohort membership is always within a single
//! token, so each token gets its own dense `u32` namespace plus a `wallets`
//! table (`u32 → address`) kept only for the Parquet cache write.

use chrono::{DateTime, Utc};
use std::collections::HashMap;

use crate::models::trade::TradeRow;

/// One trade, projected to the scalar fields the sweep reads, with the wallet
/// interned to a token-local `u32`. **No** `tx_signature` is retained (Phase 1.2):
/// the only sweep consumer was the worst-case-entry trigger match, now resolved by
/// **index** ([`find_worst_case_paper_entry_at`]) — so the ~88 B base58 string per
/// trade is gone, halving the resident row. Every other `Trade`
/// `String`/`Uuid`/JSON field is likewise dropped.
///
/// [`find_worst_case_paper_entry_at`]:
///   crate::strategies::tpsl_sniper_2::entry::find_worst_case_paper_entry_at
#[derive(Clone, Debug)]
pub struct SweepTrade {
    pub block_time: DateTime<Utc>,
    pub sol_amount: f64,
    pub token_amount: f64,
    pub price_per_token: f64,
    pub virtual_sol_reserves: Option<f64>,
    pub real_sol_reserves: Option<f64>,
    pub slot: u64,
    /// Token-local interned wallet id (index into the token's `wallets` table).
    pub wallet: u32,
    pub leg_index: u32,
    pub is_buy: bool,
}

impl TradeRow for SweepTrade {
    type Wallet = u32;

    fn is_buy(&self) -> bool {
        self.is_buy
    }
    fn sol_amount(&self) -> f64 {
        self.sol_amount
    }
    fn token_amount(&self) -> f64 {
        self.token_amount
    }
    fn price_per_token(&self) -> f64 {
        self.price_per_token
    }
    fn slot(&self) -> u64 {
        self.slot
    }
    fn leg_index(&self) -> u32 {
        self.leg_index
    }
    fn block_time(&self) -> DateTime<Utc> {
        self.block_time
    }
    fn virtual_sol_reserves(&self) -> Option<f64> {
        self.virtual_sol_reserves
    }
    fn real_sol_reserves(&self) -> Option<f64> {
        self.real_sol_reserves
    }
    fn wallet(&self) -> &u32 {
        &self.wallet
    }
    /// The sweep never resolves the trigger by signature (it uses
    /// [`find_worst_case_paper_entry_at`]), and no other shared `TradeRow` fn reads
    /// a meaningful signature on the sweep row — so `SweepTrade` carries none and
    /// returns the empty string. The `EntryFill`/`ExitFill` strings the shared fns
    /// build from this are discarded in the sweep (its `TokenOutcome` is `Copy`,
    /// signature-free).
    ///
    /// [`find_worst_case_paper_entry_at`]:
    ///   crate::strategies::tpsl_sniper_2::entry::find_worst_case_paper_entry_at
    fn tx_signature(&self) -> &str {
        ""
    }
}

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

    /// The finished `u32 → address` table (consuming form, for the sweep projection).
    pub fn into_table(self) -> Vec<Box<str>> {
        self.table
    }
}

/// Project a token's chronological trade slice into the slim sweep rows plus the
/// interned `u32 → wallet` table. Generic over any [`TradeRow`] whose `Wallet` is a
/// `String`, so it projects the DB-loaded full [`Trade`] (DB corpus source)
/// field-for-field; no decision data is lost.
pub fn project_trades<T: TradeRow<Wallet = String>>(
    trades: &[T],
) -> (Vec<SweepTrade>, Vec<Box<str>>) {
    let mut interner = WalletInterner::default();
    let rows: Vec<SweepTrade> = trades
        .iter()
        .map(|t| SweepTrade {
            block_time: t.block_time(),
            sol_amount: t.sol_amount(),
            token_amount: t.token_amount(),
            price_per_token: t.price_per_token(),
            virtual_sol_reserves: t.virtual_sol_reserves(),
            real_sol_reserves: t.real_sol_reserves(),
            slot: t.slot(),
            wallet: interner.intern(t.wallet()),
            leg_index: t.leg_index(),
            is_buy: t.is_buy(),
        })
        .collect();
    (rows, interner.into_table())
}
