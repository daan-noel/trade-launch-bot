//! The sweep's slim, wallet-interned per-token trade projection.
//!
//! The hot loop walks one of these per trade instead of the full [`Trade`]
//! (5 `String`s + `Uuid` + a JSON `Value` ≈ 250 B with heap indirection). It
//! carries **only** the fields the shared entry/exit fns read — see
//! [`TradeRow`] — and interns each token's wallets to a `u32` so wallet-set
//! membership in the inner walk is integer-keyed (no base58-String hashing or
//! clones). The projection is built **once per token** at corpus-load time and
//! reused across every (combo) evaluation; `Trade` never enters the sweep loop.
//!
//! Wallet ids are token-local: each token gets its own dense `u32` namespace
//! plus a `wallets` table (`u32 → address`) kept only for the Parquet cache
//! write.

use chrono::{DateTime, Utc};

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
    pub amount_sol: f64,
    pub token_amount: f64,
    pub price_per_token: f64,
    pub reserve_sol: Option<f64>,
    /// Token side of the priced reserve pair — carried (vs the sweep row's historic
    /// `None`) so the backtest computes the **same** GMGN spot (`reserve_sol /
    /// reserve_token`) as live + chart instead of silently falling back to execution
    /// price. Costs ~+8 B/row; accepted as the price of price parity (swing1 Step 0).
    pub reserve_token: Option<f64>,
    pub real_reserve_sol: Option<f64>,
    /// Real TOKEN reserves — feeds the pool-spot fallback of the shared
    /// [`chart_spot_price`](TradeRow::chart_spot_price). ~+8 B/row.
    pub real_token_reserves: Option<f64>,
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
    fn amount_sol(&self) -> f64 {
        self.amount_sol
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
    fn reserve_sol(&self) -> Option<f64> {
        self.reserve_sol
    }
    fn reserve_token(&self) -> Option<f64> {
        self.reserve_token
    }
    fn real_reserve_sol(&self) -> Option<f64> {
        self.real_reserve_sol
    }
    fn real_token_reserves(&self) -> Option<f64> {
        self.real_token_reserves
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

// `WalletInterner` moved to `trading_core` (shared with the live token cache, which
// can't depend on this local sweep crate); re-exported so `crate::sweep::projection::
// WalletInterner` paths keep resolving.
pub use trading_core::wallet_interner::WalletInterner;

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
            amount_sol: t.amount_sol(),
            token_amount: t.token_amount(),
            price_per_token: t.price_per_token(),
            reserve_sol: t.reserve_sol(),
            reserve_token: t.reserve_token(),
            real_reserve_sol: t.real_reserve_sol(),
            real_token_reserves: t.real_token_reserves(),
            slot: t.slot(),
            wallet: interner.intern(t.wallet()),
            leg_index: t.leg_index(),
            is_buy: t.is_buy(),
        })
        .collect();
    (rows, interner.into_table())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use trading_core::models::trade::{Trade, TradeType};
    use trading_core::state::token_cache::CachedTrade;

    /// A `Trade` carrying virtual curve reserves (curve-spot) — the live-curve shape.
    fn curve_trade(sol: f64, tokens: u64, vsol: f64, vtok: u64) -> Trade {
        let mut t = Trade::new(
            "mint".into(),
            "wallet".into(),
            TradeType::Buy,
            sol,
            tokens,
            "sig".into(),
            1,
            Utc::now(),
        );
        t.reserve_sol = Some(vsol);
        t.reserve_token = Some(vtok);
        t
    }

    /// Step-0 parity guard: the **same** trades produce an identical GMGN
    /// `chart_spot_price()` series across the live `Trade`, the live cache row
    /// `CachedTrade`, and the sweep's `SweepTrade` — so a swing leg detected offline
    /// is the leg detected live. Covers curve-spot rows and the execution fallback.
    #[test]
    fn chart_spot_price_identical_across_trade_cached_and_sweep() {
        let trades = vec![
            curve_trade(1.0, 1_000_000, 30.0, 900_000),
            curve_trade(2.0, 2_000_000, 31.0, 880_000),
            // Bare row: no reserves → execution-price fallback on all three.
            Trade::new(
                "mint".into(),
                "wallet".into(),
                TradeType::Sell,
                0.5,
                250_000,
                "sig2".into(),
                2,
                Utc::now(),
            ),
        ];

        let (sweep_rows, _) = project_trades(&trades);
        let cached: Vec<CachedTrade> =
            trades.iter().map(|t| CachedTrade::from_trade(t, 0)).collect();

        for (i, t) in trades.iter().enumerate() {
            let want = t.chart_spot_price();
            assert_eq!(want, cached[i].chart_spot_price(), "CachedTrade row {i}");
            assert_eq!(want, sweep_rows[i].chart_spot_price(), "SweepTrade row {i}");
        }
    }
}
