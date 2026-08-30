//! The lake corpus's slim per-token trade projection — the single row type both
//! the grouped sweep and single-rule simulate walk.
//!
//! The hot loop walks one of these per trade instead of the full [`Trade`]
//! (5 `String`s + `Uuid` + a JSON `Value` ≈ 250 B with heap indirection). It
//! carries **only** the fields the shared entry/exit fns read — see [`TradeRow`].
//! The projection is built **once per token** at corpus-load time and reused
//! across every (combo) evaluation; `Trade` never enters the loop.
//!
//! No wallet identity is carried: the analysis path has no cohort logic, so
//! nothing here reads a trade's wallet (`type Wallet = ()`). The live `Trade` /
//! `CachedTrade` rows still key on wallet — that's why the shared [`TradeRow`] trait
//! keeps the associated type.

use chrono::{DateTime, Utc};
use serde_json::Value;

use hunter_engine::metrics::flow_ix::{
    ix_hash_from_labels_json, ix_hash_from_labels_value, marker_bits_from_labels_value,
    wallet_hash,
};
use hunter_engine::metrics::template_grain::{
    grain_hash_from_labels_json, grain_hash_from_labels_value, is_launch_from_labels_json,
    is_launch_from_labels_value,
};
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
    /// Intra-slot index. `0` is a valid first-in-block trade.
    pub tx_index: u32,
    pub leg_index: u32,
    pub is_buy: bool,
    /// Base58 signature — `None` on the sweep read (slim), `Some` on the simulate read
    /// (Solscan links). See the struct doc. `Box<str>` (16 B) not `String` (24 B) since
    /// it's write-once.
    pub tx_signature: Option<Box<str>>,
    /// Volume-flow classifier inputs, **already hashed** through the flow-split
    /// SSOT at load time — see [`FlowKeys`].
    pub flow: FlowKeys,
    /// Normalized ix-label JSON string — kept only when [`Selection::with_flow_text`]
    /// (flow *discovery*, which reports label text). `None` on every other load,
    /// including flow sweeps/simulates, which read [`flow`](Self::flow) instead.
    pub ix_labels: Option<Box<str>>,
    /// Wallet address — kept only when [`Selection::with_flow_text`]; see
    /// [`ix_labels`](Self::ix_labels).
    pub wallet: Option<Box<str>>,
}

/// A trade's volume-flow classifier keys, resolved **once at load** rather than
/// per fold.
///
/// The engine only ever wants two integers ([`TradeLite::ix_hash`] /
/// [`TradeLite::wallet_hash`]). Carrying the raw JSON label array and the base58
/// wallet on the corpus and re-deriving them in [`to_trade_lite`] would cost a
/// `serde_json` parse plus a heap allocation per label on **every trade of every
/// run**. Hashing at the row decode keeps the fold allocation-free and shrinks a
/// flow row: 24 B of scalars replace two pointers into ~85 B of heap.
///
/// Both fields keep the "absent ⇒ organic" contract: `ix_hash: None` (missing or
/// unparseable labels) and `wallet_hash: 0` (no wallet column) classify volume-side
/// only via contagion or the creator seed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FlowKeys {
    pub ix_hash: Option<u64>,
    pub wallet_hash: u64,
    /// Structural markers of the row's labels - the offline twin of the live
    /// producer's, resolved at load beside `ix_hash` so the fold does no string work.
    pub marker_bits: u16,
    /// FNV-1a of the build-template grain; `None` when labels are missing.
    pub template_hash: Option<u64>,
    /// `Pump.Fun: Create*` present on the labels.
    pub is_launch: bool,
}

impl FlowKeys {
    /// Hash a row's stored label JSON + wallet address through the flow-split SSOT.
    /// `None` inputs stay the missing sentinel. The lake read path: `export.rs`
    /// already funnels the column through `normalize_labels`, so a lake row's text
    /// is always the bare-array form.
    pub fn from_stored(ix_labels: Option<&str>, wallet: Option<&str>) -> Self {
        Self {
            ix_hash: ix_labels.and_then(ix_hash_from_labels_json),
            wallet_hash: wallet.map(wallet_hash).unwrap_or(0),
            marker_bits: ix_labels
                .and_then(|j| serde_json::from_str::<Value>(j).ok())
                .map(|v| marker_bits_from_labels_value(&v))
                .unwrap_or(0),
            template_hash: ix_labels.and_then(grain_hash_from_labels_json),
            is_launch: ix_labels.is_some_and(is_launch_from_labels_json),
        }
    }

    /// Same contract for a row holding `ix_labels` as a decoded `Value` — the
    /// Postgres `Trade` shape, which (unlike the lake's export) still carries
    /// **either** persisted shape, so it must go through the shape-complete reader.
    pub fn from_value(ix_labels: &Value, wallet: Option<&str>) -> Self {
        Self {
            ix_hash: ix_hash_from_labels_value(ix_labels),
            wallet_hash: wallet.map(wallet_hash).unwrap_or(0),
            marker_bits: marker_bits_from_labels_value(ix_labels),
            template_hash: grain_hash_from_labels_value(ix_labels),
            is_launch: is_launch_from_labels_value(ix_labels),
        }
    }
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
    fn tx_index(&self) -> u32 {
        self.tx_index
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

/// Build an engine [`TradeLite`] from a corpus row. Pure field moves — the
/// flow-split hashes were resolved at load ([`FlowKeys`]), so this allocates
/// nothing even on a flow run.
pub fn to_trade_lite(ct: &CorpusTrade) -> TradeLite {
    TradeLite {
        side: if ct.is_buy { Side::Buy } else { Side::Sell },
        sol: ct.amount_sol,
        price: ct.price_per_token,
        reserve_sol: ct.real_reserve_sol.unwrap_or(f64::NAN),
        // `reserve_sol` on the corpus row IS the priced reserve (`vsol`); the lake
        // derives `real_reserve_sol` from it per venue. Impact is charged on the
        // priced one — see `TradeLite::priced_reserve_sol`.
        priced_reserve_sol: ct.reserve_sol.unwrap_or(f64::NAN),
        at: ct.block_time,
        ix_hash: ct.flow.ix_hash,
        wallet_hash: ct.flow.wallet_hash,
        slot: ct.slot,
        marker_bits: ct.flow.marker_bits,
        // Which instruction of its transaction this is. A bundle selling several
        // wallets' bags emits one trade per leg, all carrying the same `ix_labels`,
        // so `m_dump_ix`'s transaction count reads leg 0 only. Saturates: the byte is
        // only ever compared against 0, and a tx never carries 255 legs.
        leg_index: ct.leg_index.min(u8::MAX as u32) as u8,
        tx_index: Some(ct.tx_index),
        template_hash: ct.flow.template_hash,
        is_launch: ct.flow.is_launch,
        on_curve: true,
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

/// `peak_after[i]` = the maximum [`TradeRow::chart_spot_price`] over `trades[i..]` —
/// the best price still printed at or after row `i`.
///
/// The **oracle** denominator (family search D3): what an exit could have got, as a
/// property of `(token, entry moment)` alone, so it is computed once per corpus and
/// reused by every candidate of every stage. One backward pass at load, O(1) at any
/// entry index afterwards; `f32` because 4 B against `CorpusTrade`'s ~100 B keeps the
/// opt-in under 5% and a spot price carries nowhere near f32's 7 significant digits
/// of meaning.
///
/// Rows with no usable price contribute nothing; a suffix holding no priced row at all
/// reads `f32::NEG_INFINITY` — the "no exit available" sentinel every reader filters
/// with `is_finite()`, never a fabricated 0.
pub fn suffix_peak(trades: &[CorpusTrade]) -> Vec<f32> {
    let mut out = vec![f32::NEG_INFINITY; trades.len()];
    let mut best = f32::NEG_INFINITY;
    for (i, t) in trades.iter().enumerate().rev() {
        if let Some(p) = t.chart_spot_price() {
            let p = p as f32;
            if p.is_finite() && p > best {
                best = p;
            }
        }
        out[i] = best;
    }
    out
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
            tx_index: t.tx_index(),
            leg_index: t.leg_index(),
            is_buy: t.is_buy(),
            tx_signature: None,
            flow: FlowKeys::default(),
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
/// produce. `with_flow` resolves each row's [`FlowKeys`] for the volume-flow metrics;
/// when false they stay the missing sentinel (slim), exactly as each caller requests.
/// The raw label/wallet text is never carried here — only flow *discovery* reads it,
/// and that runs off the lake.
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
            tx_index: t.tx_index(),
            leg_index: t.leg_index(),
            is_buy: t.is_buy(),
            tx_signature: None,
            flow: if with_flow {
                FlowKeys::from_value(&t.instruction_labels, Some(t.wallet_address.as_str()))
            } else {
                FlowKeys::default()
            },
            ix_labels: None,
            wallet: None,
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

    /// Regression guard: the two reserve channels on a `TradeLite` mean different
    /// things and must not re-converge.
    ///
    /// `reserve_sol` is the **real** deposited SOL — what the `liquidity` metric and the
    /// deadness verdict read. `priced_reserve_sol` is the **priced** reserve (`vsol`) —
    /// the only correct basis for price impact, because spending `B` on a
    /// constant-product curve pays `1 + B/vsol` times spot. They differ by exactly
    /// `PUMP_INITIAL_VIRTUAL_SOL` on the curve and are equal on the AMM.
    ///
    /// Collapsing them (which the sweep did, charging impact on the real reserve)
    /// overcharges by `vsol / (vsol - 30)`: 1.6x at `liquidity 50`, 11x at
    /// `liquidity 3`, and unbounded as the pool thins. The real reserve is clamped at
    /// zero, so the priced value cannot be recovered from it — hence a carried field
    /// rather than a derivation.
    #[test]
    fn impact_depth_is_the_priced_reserve_not_the_real_one() {
        use trading_core::config::constants::{approx_real_sol_reserves, PUMP_INITIAL_VIRTUAL_SOL};

        let vsol = 44.89;
        let curve = project_pg_tail(&[curve_trade(1.0, 1_000_000, vsol, 900_000)], false);
        let lite = to_trade_lite(&curve[0]);
        assert_eq!(lite.priced_reserve_sol, vsol, "impact is charged on the priced reserve");
        assert_eq!(lite.reserve_sol, approx_real_sol_reserves(vsol, "curve"));
        assert!(
            (lite.priced_reserve_sol - lite.reserve_sol - PUMP_INITIAL_VIRTUAL_SOL).abs() < 1e-9,
            "curve: the two channels differ by exactly the initial virtual SOL"
        );

        // AMM: no virtual offset, so the two channels agree.
        let mut amm_t = curve_trade(1.0, 1_000_000, 25.0, 900_000);
        amm_t.venue = "amm".into();
        let amm = project_pg_tail(&[amm_t], false);
        let amm_lite = to_trade_lite(&amm[0]);
        assert_eq!(amm_lite.priced_reserve_sol, amm_lite.reserve_sol, "amm: real == priced");

        // The bug this guards: charging impact on the real reserve at liquidity 3
        // costs 11x what the curve actually takes.
        let thin = 33.0_f64;
        let real = approx_real_sol_reserves(thin, "curve");
        assert!((thin / real - 11.0).abs() < 1e-9, "11x overcharge at liquidity 3");
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

        // `with_flow=false` keeps the slim shape (no classifier keys resolved).
        assert_eq!(rows[0].flow, FlowKeys::default());
        assert!(rows[0].ix_labels.is_none() && rows[0].wallet.is_none());
        // `with_flow=true` resolves the hashes without disturbing the reconstruction.
        let flow_rows = project_pg_tail(&[curve], true);
        assert_eq!(flow_rows[0].flow.wallet_hash, wallet_hash("wallet"));
        assert_eq!(flow_rows[0].real_reserve_sol, Some(curve_real));
    }

    /// `trades.ix_labels` holds **either** persisted shape (see
    /// `trading_core::storage::ix_labels_sql`), and the PG tail reads the column
    /// decoded rather than through the lake export's `normalize_labels`. Both shapes
    /// must therefore resolve to the same ix-pattern hash here.
    ///
    /// The failure this guards is silent, not loud: an object-shaped row that hashes
    /// to `None` is indistinguishable from a row that genuinely has no labels, so it
    /// is simply booked untagged — deflating `tagged_*`, inflating `untagged_*`, and making
    /// the metric pane disagree with a chart that classified the same trade correctly.
    #[test]
    fn pg_tail_hashes_both_ix_label_shapes_alike() {
        let labels = ["Pump.Fun: Create", "Pump.Fun: Buy"];
        let want = hunter_engine::metrics::flow_ix::ix_hash(&labels);

        let mut bare = curve_trade(1.0, 1_000_000, 44.89, 900_000);
        bare.instruction_labels = serde_json::json!(labels);

        let mut wrapped = curve_trade(1.0, 1_000_000, 44.89, 900_000);
        wrapped.instruction_labels = serde_json::json!({ "instructions": labels });

        let rows = project_pg_tail(&[bare, wrapped], true);
        assert_eq!(rows[0].flow.ix_hash, Some(want), "bare array");
        assert_eq!(rows[1].flow.ix_hash, Some(want), "object wrapper");

        // A row with genuinely no labels still reports the missing sentinel.
        let mut none = curve_trade(1.0, 1_000_000, 44.89, 900_000);
        none.instruction_labels = serde_json::Value::Null;
        assert_eq!(project_pg_tail(&[none], true)[0].flow.ix_hash, None);
    }
}
