//! Analysis-only **death-close**: when the exit ladder never fires but the token
//! is provably dead (liquidity gone + gone silent), close the held position at the
//! **death point** — the last meaningful trade before the token went quiet — rather
//! than leaving it mislabeled `Open`/`Holding` and marked-to-a-stale last price.
//!
//! This is the analysis fallback behind the backtest / grouped-sweep / detect paths.
//! Live consumes the **same** [`find_death_point`] two ways: its 1s wall-clock exit
//! sweep fires Stall/TimeStop on silent tokens for rules that carry a time exit, and
//! `live`'s `sweep_dead_paper_exits` closes **paper** positions the ladder/time sweep
//! left `Holding` (rules with no time exit) at the death point with `ExitReason::Dead`
//! — the direct analogue of this fallback, so paper and sim agree on dead tokens. It
//! is deliberately **not** used by the live paper *poll* (which stays strict, waiting
//! for a real fill) nor by **real** positions: force-closing a real bag on a deadness
//! verdict is out of scope — a dead pool has no liquidity to sell into. The verdict
//! itself is single-sourced with live via [`is_dead_verdict`], so sim and live can
//! never disagree on *which* tokens are dead.

use chrono::{DateTime, Utc};

use crate::config::constants::DEAD_MEANINGFUL_TRADE_SOL;
use crate::models::trade::TradeRow;
use crate::state::token_cache::is_dead_verdict;

/// The resolved death point: the fill fields a strategy wraps into its own
/// `ExitFill` (with an `ExitReason::Dead`). Strategy-agnostic so all three exit
/// ladders share one death-detection pass.
#[derive(Debug, Clone, PartialEq)]
pub struct DeathPoint {
    pub price: f64,
    pub tx_signature: String,
    pub slot: u64,
    pub block_time: DateTime<Utc>,
}

/// If the token is dead at `now`, return the death point to close a position held
/// from `entry_time` at. `None` when the token is still alive (e.g. a recent token
/// whose data window merely ends) or has no post-entry trade to price against — the
/// caller then keeps the position `Open`, which is correct for a live token.
///
/// Deadness uses the newest post-entry real-reserve reading + the newest meaningful
/// post-entry trade time, fed through the shared [`is_dead_verdict`]. The death
/// point is the last meaningful post-entry trade (falls back to the last post-entry
/// trade of any size when none crossed the meaningful threshold).
pub fn find_death_point<T: TradeRow>(
    trades: &[T],
    entry_time: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Option<DeathPoint> {
    let mut newest_reserves: Option<(DateTime<Utc>, f64)> = None;
    let mut last_meaningful: Option<&T> = None;
    let mut last_any: Option<&T> = None;

    for t in trades.iter().filter(|t| t.block_time() > entry_time) {
        let bt = t.block_time();
        // Newest-by-block_time reserve snapshot (mirrors live `current_real_sol_reserves`).
        if let Some(r) = t.real_reserve_sol() {
            if newest_reserves.is_none_or(|(seen, _)| bt >= seen) {
                newest_reserves = Some((bt, r));
            }
        }
        // Newest meaningful trade drives both the quiet clock and the death price.
        if t.amount_sol() >= DEAD_MEANINGFUL_TRADE_SOL
            && last_meaningful.is_none_or(|m| bt >= m.block_time())
        {
            last_meaningful = Some(t);
        }
        if last_any.is_none_or(|m| bt >= m.block_time()) {
            last_any = Some(t);
        }
    }

    // No meaningful post-entry trade → the quiet clock runs from entry.
    let last_meaningful_time = last_meaningful.map(|t| t.block_time()).unwrap_or(entry_time);
    if !is_dead_verdict(newest_reserves.map(|(_, r)| r), last_meaningful_time, now) {
        return None;
    }
    // Prefer the last meaningful trade as the death price; fall back to the last
    // post-entry trade of any size (dust-only tail). No post-entry trade → nothing
    // to price against.
    let et = last_meaningful.or(last_any)?;
    Some(DeathPoint {
        price: et.price_per_token(),
        tx_signature: et.tx_signature().to_string(),
        slot: et.slot(),
        block_time: et.block_time(),
    })
}

/// Deterministic "as of" instant for the analysis-only death-close fallback: the
/// block_time of the token's last (chronologically newest) trade in `trades`, or
/// `entry_time` when there are none. `trades` is assumed slot/time-sorted
/// upstream — the same assumption [`find_death_point`]'s callers and the ladder's
/// market-fill fallback already rely on.
///
/// Feeding [`find_death_point`] the corpus's own last trade instead of
/// `Utc::now()` makes a death verdict a pure function of the (fixed) trade
/// history: the same token/corpus resolves the same Dead/Open call whether swept
/// today or replayed next week, and whether by the grouped sweep or a
/// single-rule simulate reading the same lake slice — see
/// `docs/plans/sweep/sweep-sim-parity.md` (C1).
pub fn analysis_as_of<T: TradeRow>(trades: &[T], entry_time: DateTime<Utc>) -> DateTime<Utc> {
    trades.last().map(|t| t.block_time()).unwrap_or(entry_time)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::trade::{Trade, TradeType};
    use chrono::Duration;

    fn base() -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap()
    }

    /// A buy at `price` (token_amount 1 → price_per_token = price), `sol` notional,
    /// optional real reserves, `secs` after `base()`.
    fn trade(price: f64, sol: f64, reserves: Option<f64>, slot: u64, secs: i64) -> Trade {
        let mut t = Trade::new(
            "mint".into(),
            "wallet".into(),
            TradeType::Buy,
            sol,
            1,
            format!("sig-{slot}"),
            slot,
            base() + Duration::seconds(secs),
        );
        t.real_reserve_sol = reserves;
        // `Trade::new` derives price_per_token = amount_sol / token_amount; override
        // it so the death-point price the test asserts on is the explicit `price`.
        t.price_per_token = price;
        t
    }

    #[test]
    fn dead_when_liquidity_gone_and_silent() {
        // Last meaningful trade at +10s, reserves crashed below 30, and `now` is well
        // past the quiet window → dead, priced at the last meaningful trade.
        let trades = vec![trade(0.5, 1.0, Some(5.0), 2, 10)];
        let now = base() + Duration::seconds(10 + 400); // > DEAD_QUIET_SECS (300)
        let dp = find_death_point(&trades, base(), now).expect("should be dead");
        assert!((dp.price - 0.5).abs() < 1e-9);
        assert_eq!(dp.block_time, base() + Duration::seconds(10));
    }

    #[test]
    fn alive_when_reserves_healthy() {
        // Reserves above the liquidity floor → never dead, no matter how quiet.
        let trades = vec![trade(0.5, 1.0, Some(50.0), 2, 10)];
        let now = base() + Duration::seconds(10_000);
        assert!(find_death_point(&trades, base(), now).is_none());
    }

    #[test]
    fn alive_when_recent_meaningful_trade() {
        // Liquidity is gone but a meaningful trade printed inside the quiet window.
        let trades = vec![trade(0.5, 1.0, Some(5.0), 2, 10)];
        let now = base() + Duration::seconds(10 + 100); // < DEAD_QUIET_SECS
        assert!(find_death_point(&trades, base(), now).is_none());
    }

    #[test]
    fn dust_does_not_reset_quiet_clock() {
        // Meaningful trade at +10s, then only dust (< DEAD_MEANINGFUL_TRADE_SOL) at
        // +320s. The quiet clock runs from the meaningful trade, so the token is dead
        // and prices at the meaningful trade, not the dust tail.
        let trades = vec![
            trade(0.5, 1.0, Some(5.0), 2, 10),
            trade(0.4, 0.001, Some(5.0), 3, 320),
        ];
        let now = base() + Duration::seconds(400);
        let dp = find_death_point(&trades, base(), now).expect("dead despite dust");
        assert!((dp.price - 0.5).abs() < 1e-9, "priced at meaningful trade");
    }

    #[test]
    fn no_post_entry_trades_is_none() {
        let trades = vec![trade(0.5, 1.0, Some(5.0), 2, -10)]; // before entry
        let now = base() + Duration::seconds(10_000);
        assert!(find_death_point(&trades, base(), now).is_none());
    }

    #[test]
    fn analysis_as_of_is_the_corpus_last_trade_not_the_wall_clock() {
        // The whole point of C1 (parity plan): a fixed trade history must resolve
        // the same death verdict no matter when the analysis runs. `analysis_as_of`
        // is the corpus's own last trade time — never `Utc::now()`.
        let trades = vec![trade(0.5, 1.0, Some(5.0), 2, 10), trade(0.4, 1.0, Some(4.0), 3, 250)];
        assert_eq!(analysis_as_of(&trades, base()), base() + Duration::seconds(250));
    }

    #[test]
    fn analysis_as_of_falls_back_to_entry_time_when_no_trades() {
        let trades: Vec<Trade> = vec![];
        assert_eq!(analysis_as_of(&trades, base()), base());
    }

    #[test]
    fn death_verdict_over_a_fixed_corpus_is_stable_regardless_of_when_it_is_asked() {
        // Same trade history, same entry — the last meaningful trade is only 250s
        // before the corpus's own last trade (< DEAD_QUIET_SECS from
        // `analysis_as_of`), so the token reads alive no matter what the real
        // wall-clock happens to be when this runs (it would read Dead under the
        // old `Utc::now()` call site, since real time is always > +250s from a
        // fixed `base()` in the past).
        let trades = vec![trade(0.5, 1.0, Some(5.0), 2, 10), trade(0.4, 0.5, Some(5.0), 3, 250)];
        let as_of = analysis_as_of(&trades, base());
        assert!(find_death_point(&trades, base(), as_of).is_none(), "alive: still within quiet window of its own last trade");
    }
}
