use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::constants::MIN_TRADE_SOL;

/// A single buy or sell on a tracked token's bonding curve.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub id: Uuid,
    pub mint_address: String,
    pub wallet_address: String,
    pub trade_type: TradeType,
    /// SOL amount (human-readable, already divided by lamports).
    pub amount_sol: f64,
    /// Raw token units — an exact on-chain integer count (never fractional).
    /// Stored as `u64` so large legs (>2^53 raw units) keep every digit on the
    /// way to the `BIGINT` column; ratio consumers cast to `f64` at the divide.
    pub token_amount: u64,
    /// Average execution price for this swap (`amount_sol / token_amount`).
    pub price_per_token: f64,
    pub tx_signature: String,
    /// Position of this trade's transaction within its block. Real on the live
    /// LaserStream feed and on LaserStream-replay backfill. On the RPC backfill path
    /// the true block position is unavailable, so token_sync stamps a per-slot
    /// monotonic proxy in gTFA chain order (`stamp_proxy_tx_index`): it orders trades
    /// within a slot but is NOT the absolute block index — don't compare it across
    /// sources.
    pub tx_index: u32,
    /// Index of this trade within the transaction (0 = first pump leg).
    pub leg_index: u32,
    pub slot: u64,
    /// On-chain block time from Solana (second precision).
    pub block_time: DateTime<Utc>,
    /// When this trade was ingested (sub-second precision).
    pub received_at: DateTime<Utc>,

    // ── On-chain state snapshot (from TradeEvent "Program data:" log) ─────────
    /// SOL side of the reserve pair this row prices from (venue-neutral): the
    /// bonding curve's *virtual* SOL reserves on curve rows, the PumpSwap pool's
    /// *real* SOL reserves on amm rows (the decoder copies pool reserves here for
    /// migrated tokens). Spot price = `reserve_sol / reserve_token`. SOL stays
    /// `f64` (small magnitude); the exactness lives in the lamports column.
    pub reserve_sol: Option<f64>,
    /// Token side of the reserve pair — raw on-chain integer units (`u64`, near 2^53).
    pub reserve_token: Option<u64>,
    /// Real (non-virtual) SOL reserves — used for graduation progress. Distinct
    /// from `reserve_sol`: on curve rows this is the *real* deposited SOL, not the
    /// virtual reserve the price derives from. Not persisted (DB keeps only the
    /// price pair); set by the live decoder.
    pub real_reserve_sol: Option<f64>,
    /// Real token reserves — raw on-chain integer units (`u64`). Not persisted.
    pub real_token_reserves: Option<u64>,

    // ── Instruction context ───────────────────────────────────────────────────
    /// Trade-side label for executed trades: "Buy" or "Sell".
    pub instruction_type: String,
    /// Ordered list of human-readable labels for every top-level instruction in
    /// the transaction, e.g. `["Compute Budget: SetComputeUnitLimit", "Pump.Fun: Buy"]`.
    /// Stored as a JSON array.
    pub instruction_labels: serde_json::Value,

    /// Trading venue: `"curve"` (pump.fun bonding curve) or `"amm"` (post-migration
    /// PumpSwap pool). Drives the per-venue incremental fetch boundary.
    pub venue: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradeType {
    Buy,
    Sell,
}

impl Trade {
    /// True when `amount_sol` is below the ingest dust threshold (10k lamports).
    pub fn is_dust(amount_sol: f64) -> bool {
        amount_sol < MIN_TRADE_SOL
    }

    pub fn new(
        mint_address: String,
        wallet_address: String,
        trade_type: TradeType,
        amount_sol: f64,
        token_amount: u64,
        tx_signature: String,
        slot: u64,
        block_time: DateTime<Utc>,
    ) -> Self {
        let price_per_token = if token_amount > 0 {
            amount_sol / token_amount as f64
        } else {
            0.0
        };

        Self {
            id: Uuid::new_v4(),
            mint_address,
            wallet_address,
            trade_type,
            amount_sol,
            token_amount,
            price_per_token,
            tx_signature,
            tx_index: 0,
            leg_index: 0,
            slot,
            block_time,
            // `received_at` is the wall-clock time this bot observed the trade,
            // distinct from the on-chain `block_time`. The live decoder overrides
            // it with the raw-tx receipt time; default to now (not block_time) so
            // any path that doesn't override isn't mislabeled with chain time.
            received_at: Utc::now(),
            reserve_sol: None,
            reserve_token: None,
            real_reserve_sol: None,
            real_token_reserves: None,
            instruction_type: "Unknown".to_string(),
            instruction_labels: serde_json::Value::Array(vec![]),
            venue: "curve".to_string(),
        }
    }
}

/// The read-only fields the shared entry/exit fns consume, abstracted over
/// the concrete row type so those fns can run against either the live `Trade`
/// (decision parity with real trading) **or** the sweep's slim, wallet-interned
/// [`CorpusTrade`](crate::sweep::projection::CorpusTrade) projection without
/// duplicating any decision logic.
///
/// `Trade` impls this trivially (below); the sweep impls it over a ~3× smaller
/// row whose `Wallet` is an interned `u32` (integer-keyed hashing/membership,
/// not 44-char-base58-String-keyed). Because the fns monomorphize, the live path
/// (`T = Trade`) compiles to byte-identical code — there is no runtime cost or
/// behavior change for live trading.
pub trait TradeRow {
    /// Cheap, hashable wallet identity. `String` for the live `Trade` (unchanged
    /// hashing); an interned `u32` for the sweep row.
    type Wallet: Eq + std::hash::Hash + Clone;

    fn is_buy(&self) -> bool;
    fn amount_sol(&self) -> f64;
    fn token_amount(&self) -> f64;
    fn price_per_token(&self) -> f64;
    fn slot(&self) -> u64;
    /// Position of this trade's transaction within its block — the real intra-slot
    /// ordering key. Together with `slot` and `leg_index` it gives the one canonical
    /// trade order (`slot → tx_index → leg_index`) the DB, lake, and every in-memory
    /// consumer sort on. Defaults to `0` for slim rows that don't carry it
    /// (`CorpusTrade`/`CachedTrade`): they're built from an already DB-ordered slice
    /// and never re-sorted, so they need no per-row copy (RAM budget). Only the live
    /// `Trade` — which IS re-sorted in memory — overrides it with the real column.
    fn tx_index(&self) -> u32 {
        0
    }
    fn leg_index(&self) -> u32;
    fn block_time(&self) -> DateTime<Utc>;
    /// SOL side of the venue-neutral reserve pair this row prices from — curve
    /// virtual SOL reserves on curve rows, pool real SOL reserves on amm rows
    /// (tpsl1's E4 liquidity exit reads this).
    fn reserve_sol(&self) -> Option<f64>;
    /// Token side of the reserve pair. Only the live `Trade`/`CachedTrade` carry it
    /// (used to maintain `TokenState::current_reserve_token`); the sweep row that
    /// drops reserves defaults to `None`.
    fn reserve_token(&self) -> Option<f64> {
        None
    }
    /// Real (non-virtual) SOL reserves (tpsl2's E4 + scalp liquidity gates read this).
    fn real_reserve_sol(&self) -> Option<f64>;
    /// Real (non-virtual) TOKEN reserves — used for the PumpSwap pool spot fallback
    /// in [`chart_spot_price`](TradeRow::chart_spot_price). Only rows that carry the
    /// real-reserve pair price via the pool branch; the default is `None`.
    fn real_token_reserves(&self) -> Option<f64> {
        None
    }
    /// Borrowed wallet identity — borrowed so callers never clone unnecessarily.
    fn wallet(&self) -> &Self::Wallet;
    fn tx_signature(&self) -> &str;

    // ── Shared GMGN price (single definition: chart == analyzer == live == sweep) ──

    /// Average execution price (`amount_sol / token_amount`), `price_per_token` when
    /// `token_amount` is zero.
    fn execution_price(&self) -> f64 {
        let tok = self.token_amount();
        if tok > 0.0 {
            self.amount_sol() / tok
        } else {
            self.price_per_token()
        }
    }

    /// Venue-neutral spot from the reserve pair (`reserve_sol / reserve_token`) —
    /// curve virtual reserves on curve rows, pool real reserves on amm rows. `None`
    /// if either reserve is absent or the token reserve is non-positive.
    fn spot_price(&self) -> Option<f64> {
        match (self.reserve_sol(), self.reserve_token()) {
            (Some(sol), Some(tok)) if tok > 0.0 => Some(sol / tok),
            _ => None,
        }
    }

    /// PumpSwap pool spot from real reserves (`real_sol / real_token`).
    fn pool_spot_price(&self) -> Option<f64> {
        match (self.real_reserve_sol(), self.real_token_reserves()) {
            (Some(sol), Some(tok)) if tok > 0.0 => Some(sol / tok),
            _ => None,
        }
    }

    /// **The** canonical GMGN chart spot: the venue-neutral reserve-pair spot →
    /// pool real-reserve fallback → execution price. This one definition is what the
    /// chart, the swing analyzer, the live strategies, and the backtest sweep all
    /// price from, so a leg detected offline is the leg detected live. The
    /// `reserve_*` pair already holds pool reserves on amm rows, so `spot_price()`
    /// covers both venues; the `pool_spot_price` rung only fires for a row that
    /// carries `real_*` but not the `reserve_*` pair. Always returns a finite,
    /// positive number (the execution fallback) unless the row carries no usable
    /// price at all.
    fn chart_spot_price(&self) -> Option<f64> {
        self.spot_price()
            .or_else(|| self.pool_spot_price())
            .filter(|p| p.is_finite() && *p > 0.0)
            .or_else(|| {
                let exec = self.execution_price();
                (exec.is_finite() && exec > 0.0).then_some(exec)
            })
    }
}

impl TradeRow for Trade {
    type Wallet = String;

    fn is_buy(&self) -> bool {
        matches!(self.trade_type, TradeType::Buy)
    }
    fn amount_sol(&self) -> f64 {
        self.amount_sol
    }
    fn token_amount(&self) -> f64 {
        self.token_amount as f64
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
        self.reserve_token.map(|v| v as f64)
    }
    fn real_reserve_sol(&self) -> Option<f64> {
        self.real_reserve_sol
    }
    fn real_token_reserves(&self) -> Option<f64> {
        self.real_token_reserves.map(|v| v as f64)
    }
    fn wallet(&self) -> &String {
        &self.wallet_address
    }
    fn tx_signature(&self) -> &str {
        &self.tx_signature
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn buy_with_reserves(sol: f64, tokens: u64, vsol_post: f64, vtok_post: u64) -> Trade {
        let mut t = Trade::new(
            "mint".into(),
            "user".into(),
            TradeType::Buy,
            sol,
            tokens,
            "sig".into(),
            1,
            Utc::now(),
        );
        t.reserve_sol = Some(vsol_post);
        t.reserve_token = Some(vtok_post);
        t
    }

    #[test]
    fn price_per_token_is_execution_not_curve_spot() {
        let t = buy_with_reserves(10.0, 1_000_000, 110.0, 900_000);
        assert!((t.price_per_token - 10.0 / 1_000_000.0).abs() < 1e-12);
        assert!((t.execution_price() - t.price_per_token).abs() < 1e-15);
        let spot = t.spot_price().unwrap();
        assert!((t.price_per_token - spot).abs() > 1e-15);
    }

    #[test]
    fn is_dust_matches_min_trade_sol() {
        assert!(Trade::is_dust(MIN_TRADE_SOL - 1e-12));
        assert!(!Trade::is_dust(MIN_TRADE_SOL));
        assert!(!Trade::is_dust(1.0));
    }

    #[test]
    fn chart_spot_price_prefers_curve_then_pool_then_execution() {
        let curve = buy_with_reserves(1.0, 100, 50.0, 500);
        assert_eq!(curve.chart_spot_price(), curve.spot_price());

        let mut amm = Trade::new(
            "mint".into(),
            "user".into(),
            TradeType::Buy,
            1.0,
            100,
            "sig".into(),
            1,
            Utc::now(),
        );
        amm.venue = "amm".into();
        amm.real_reserve_sol = Some(25.0);
        amm.real_token_reserves = Some(250);
        assert!((amm.chart_spot_price().unwrap() - 0.1).abs() < 1e-12);

        let bare = Trade::new(
            "mint".into(),
            "user".into(),
            TradeType::Buy,
            2.0,
            200,
            "sig".into(),
            1,
            Utc::now(),
        );
        assert!((bare.chart_spot_price().unwrap() - 0.01).abs() < 1e-12);
    }
}
