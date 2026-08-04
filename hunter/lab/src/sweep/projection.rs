//! The lake corpus's slim per-token trade projection — the single row type both
//! the grouped sweep and single-rule simulate walk.
//!
//! The hot loop walks one of these per trade instead of the full [`Trade`]
//! (5 `String`s + `Uuid` + a JSON `Value` ≈ 250 B with heap indirection). It
//! carries **only** the fields the shared entry/exit fns read — see [`TradeRow`].
//! The projection is built **once per token** at corpus-load time and reused
//! across every (combo) evaluation; `Trade` never enters the loop.
//!
//! No wallet identity is carried: the analysis path's cohort logic was removed, so
//! nothing here reads a trade's wallet (`type Wallet = ()`). The live `Trade` /
//! `CachedTrade` rows still key on wallet — that's why the shared [`TradeRow`] trait
//! keeps the associated type.

use chrono::{DateTime, Utc};

use hunter_engine::metrics::flow_split::{ix_hash_opt, wallet_hash};
use hunter_engine::metrics::{Side, TradeLite};

use trading_core::config::constants::approx_real_sol_reserves;

use crate::models::trade::{Trade, TradeRow};

/// One trade, projected to the scalar fields the entry/exit fns read — the single
/// row type both the grouped sweep and single-rule simulate walk.
///
/// `tx_signature` is an **`Option`** so that one row type serves both readers: the
/// sweep loads it `None` (the trigger/fill is resolved by index via
/// [`find_worst_case_paper_entry_at`] — so the ~88 B base58 string is dead weight in
/// the hot loop, and `None` is a bare 16 B, no heap), while single-rule **simulate**
/// loads it `Some` because its result tables render `entry_tx`/`exit_tx` as Solscan
/// links. The loader picks per read via [`Selection::with_signatures`]; nothing else
/// about the row differs, so the two paths price identically by construction. Every
/// other `Trade` `String`/`Uuid`/JSON field is dropped.
///
/// [`find_worst_case_paper_entry_at`]:
///   trading_core::strategies::paper_fill::find_worst_case_paper_entry_at
/// [`Selection::with_signatures`]: crate::sweep::corpus::Selection::with_signatures
#[derive(Clone, Debug)]
pub struct CorpusTrade {
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
    pub leg_index: u32,
    pub is_buy: bool,
    /// Base58 signature — `None` on the sweep read (slim), `Some` on the simulate read
    /// (Solscan links). See the struct doc. `Box<str>` (16 B) not `String` (24 B) since
    /// it's write-once.
    pub tx_signature: Option<Box<str>>,
    /// Normalized ix-label JSON string — loaded only when [`Selection::with_flow`].
    /// `None` on slim loads / pre-V0 sealed days.
    pub ix_labels: Option<Box<str>>,
    /// Wallet address — loaded only when [`Selection::with_flow`].
    pub wallet: Option<Box<str>>,
}

impl TradeRow for CorpusTrade {
    /// Unit: the analysis path never reads wallet identity (cohort logic was
    /// removed). The trait keeps `Wallet` for the live rows, which do key on it.
    type Wallet = ();

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
    fn wallet(&self) -> &() {
        &()
    }
    /// The stored base58 signature, or `""` when the row was loaded signature-free
    /// (the sweep path — the trigger is resolved by index, not signature). The
    /// `EntryFill`/`ExitFill` strings the shared fns build from this are discarded by
    /// the sweep (its `TokenOutcome` is `Copy`, signature-free) and rendered as
    /// Solscan links by simulate.
    fn tx_signature(&self) -> &str {
        self.tx_signature.as_deref().unwrap_or("")
    }
}

/// Build an engine [`TradeLite`] from a corpus row — hashes `ix_labels`/`wallet`
/// via the flow-split SSOT when those columns were loaded (`with_flow`).
pub fn to_trade_lite(ct: &CorpusTrade) -> TradeLite {
    let labels: Vec<String> = ct
        .ix_labels
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    TradeLite {
        side: if ct.is_buy { Side::Buy } else { Side::Sell },
        sol: ct.amount_sol,
        price: ct.price_per_token,
        reserve_sol: ct.real_reserve_sol.unwrap_or(f64::NAN),
        at: ct.block_time,
        ix_hash: ix_hash_opt(&labels),
        wallet_hash: ct.wallet.as_deref().map(wallet_hash).unwrap_or(0),
    }
}

/// The token's **creation slot**, as the offline corpus can know it: the slot of
/// its first chronological trade.
///
/// `tokens.creation_slot` is the on-chain truth (live's `TokenState` first-slot
/// accumulators key on it), but the lake `tokens` dimension does not carry it —
/// only the two derived `fp_first_slot_*` sums. So every offline first-slot
/// derivation stands in the first trade's slot, which agrees whenever the
/// creation tx itself produced a trade (a dev buy / launch bundle — the shapes
/// anyone asks this question about). A token whose creation slot traded not at
/// all reports its first *later* slot instead; that is the one known divergence,
/// and it is why this is ONE fn and not a re-derived `trades.first().slot` at
/// each call site (replay's `FirstSlotSettled`, discovery's first-slot split).
pub fn creation_slot(trades: &[CorpusTrade]) -> Option<u64> {
    trades.first().map(|t| t.slot)
}

/// Project a token's chronological trade slice into the slim rows. Generic over any
/// [`TradeRow`] whose `Wallet` is a `String`, so it projects the full [`Trade`]
/// field-for-field; no decision data is lost. Signature-free (the sweep resolves the
/// trigger by index); the lake's simulate read populates `tx_signature` directly (see
/// `duck.rs`).
pub fn project_trades<T: TradeRow<Wallet = String>>(trades: &[T]) -> Vec<CorpusTrade> {
    trades
        .iter()
        .map(|t| CorpusTrade {
            block_time: t.block_time(),
            amount_sol: t.amount_sol(),
            token_amount: t.token_amount(),
            price_per_token: t.price_per_token(),
            reserve_sol: t.reserve_sol(),
            reserve_token: t.reserve_token(),
            real_reserve_sol: t.real_reserve_sol(),
            real_token_reserves: t.real_token_reserves(),
            slot: t.slot(),
            leg_index: t.leg_index(),
            is_buy: t.is_buy(),
            tx_signature: None,
            ix_labels: None,
            wallet: None,
        })
        .collect()
}

/// Real (non-virtual) SOL reserves for a Postgres-read [`Trade`] row.
///
/// The program-emitted `real_sol_reserves` is **not persisted** — only the live
/// decoder sets it (see [`Trade::real_reserve_sol`]) — so a row read back from
/// Postgres always has `real_reserve_sol == None`. Reconstruct it from the persisted
/// virtual reserve + `venue` via the SSOT [`approx_real_sol_reserves`], the **same**
/// derivation the sealed lake applies at load (`lab::lake::duck`). That keeps a PG
/// fresh-tail row and its eventual lake copy computing identical liquidity/deadness,
/// so a token that lives only in the PG tail (created after the last lake export)
/// isn't blind to liquidity offline. A live value, if somehow present, wins; a row
/// with no reserve pair stays `None` (⇒ NaN liquidity ⇒ never a false fire).
fn pg_real_reserve_sol(t: &Trade) -> Option<f64> {
    t.real_reserve_sol()
        .or_else(|| t.reserve_sol().map(|s| approx_real_sol_reserves(s, &t.venue)))
}

/// Project the Postgres **fresh tail** ([`Trade`] rows) into the slim corpus rows for
/// per-token analysis (metric-series + single-rule simulate). `Trade`-concrete (unlike
/// the generic [`project_trades`]) so it can **reconstruct `real_reserve_sol`** from
/// the persisted virtual reserve + venue (see [`pg_real_reserve_sol`]) — the lake load
/// does the same, so a PG-only token's liquidity/deadness match what the lake would
/// produce. `with_flow` fills `ix_labels`/`wallet` for the volume-flow metrics; when
/// false they stay `None` (slim), exactly as each caller requests.
pub fn project_pg_tail(trades: &[Trade], with_flow: bool) -> Vec<CorpusTrade> {
    trades
        .iter()
        .map(|t| CorpusTrade {
            block_time: t.block_time(),
            amount_sol: t.amount_sol(),
            token_amount: t.token_amount(),
            price_per_token: t.price_per_token(),
            reserve_sol: t.reserve_sol(),
            reserve_token: t.reserve_token(),
            real_reserve_sol: pg_real_reserve_sol(t),
            real_token_reserves: t.real_token_reserves(),
            slot: t.slot(),
            leg_index: t.leg_index(),
            is_buy: t.is_buy(),
            tx_signature: None,
            ix_labels: with_flow.then(|| Box::from(t.instruction_labels.to_string())),
            wallet: with_flow.then(|| Box::from(t.wallet_address.as_str())),
        })
        .collect()
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
    /// `CachedTrade`, and the sweep's `CorpusTrade` — so a swing leg detected offline
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

        let sweep_rows = project_trades(&trades);
        let cached: Vec<CachedTrade> =
            trades.iter().map(|t| CachedTrade::from_trade(t, 0)).collect();

        for (i, t) in trades.iter().enumerate() {
            let want = t.chart_spot_price();
            assert_eq!(want, cached[i].chart_spot_price(), "CachedTrade row {i}");
            assert_eq!(want, sweep_rows[i].chart_spot_price(), "CorpusTrade row {i}");
        }
    }

    /// Regression guard: a Postgres-read curve `Trade` never carries the program's
    /// `real_sol_reserves` (it isn't persisted), so the PG-tail projection MUST
    /// reconstruct it from the virtual reserve via the SSOT `approx_real_sol_reserves`
    /// — the exact derivation the sealed lake applies. Without this, `real_reserve_sol`
    /// is `None` → the engine's `liquidity` metric is `NaN` for every event, blanking
    /// the chart pane and making any `liquidity >= X` entry gate unsatisfiable, so a
    /// profitable token created after the last lake export is silently never entered
    /// in simulate/paper.
    #[test]
    fn pg_tail_reconstructs_real_reserves_like_the_lake() {
        use trading_core::config::constants::approx_real_sol_reserves;

        // Curve row with ~44.89 virtual SOL and NO persisted real reserve (the PG shape).
        let vsol = 44.89;
        let curve = curve_trade(1.0, 1_000_000, vsol, 900_000);
        assert!(curve.real_reserve_sol().is_none(), "PG rows carry no real reserve");

        // AMM row: real reserve == pool reserve (no virtual offset).
        let mut amm = curve_trade(1.0, 1_000_000, 25.0, 900_000);
        amm.venue = "amm".into();

        let rows = project_pg_tail(&[curve.clone(), amm.clone()], false);

        // Curve: reconstructed real = virtual − 30 (≈ 14.89), clears a `liquidity >= 10` gate.
        let curve_real = rows[0].real_reserve_sol.expect("curve real reconstructed");
        assert_eq!(curve_real, approx_real_sol_reserves(vsol, "curve"));
        assert!((curve_real - 14.89).abs() < 1e-9);
        assert!(curve_real >= 10.0, "reconstructed liquidity must satisfy the entry gate");
        assert!(to_trade_lite(&rows[0]).reserve_sol.is_finite(), "liquidity is finite, not NaN");

        // AMM: reconstructed real == the pool reserve itself.
        assert_eq!(rows[1].real_reserve_sol, Some(approx_real_sol_reserves(25.0, "amm")));

        // `with_flow=false` keeps the slim shape (no ix-label/wallet copy).
        assert!(rows[0].ix_labels.is_none() && rows[0].wallet.is_none());
        // `with_flow=true` fills them without disturbing the reconstruction.
        let flow_rows = project_pg_tail(&[curve], true);
        assert!(flow_rows[0].ix_labels.is_some() && flow_rows[0].wallet.is_some());
        assert_eq!(flow_rows[0].real_reserve_sol, Some(curve_real));
    }
}
