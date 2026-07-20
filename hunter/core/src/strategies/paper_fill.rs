//! Worst-case paper/sim fill model — shared by live `exec_paper`, lab replay /
//! simulate, and the grouped-sweep scan.
//!
//! After a trigger (entry) or ladder fire (exit), the fill is **not** the deciding
//! trade's spot. Candidates are trades **after** that index in a short slot window:
//!
//! * window = trigger/fire slot `S` (always) + the next observed slot after `S` when
//!   that slot is within [`MAX_FILL_WAIT_SLOTS`] of `S`
//! * **entry** = highest qualifying **buy** price in the window (adverse for us)
//! * **exit** = lowest price of any trade in the window (adverse for us)
//!
//! Analysis paths pass `market_fill_on_empty_window = true` on exit so a fire with
//! an empty window still books a market exit at the firing trade (sparse / gappy
//! tails). Live paper keeps it `false` and waits / fails closed.

use chrono::{DateTime, Utc};

use crate::config::constants::MAX_FILL_WAIT_SLOTS;
use crate::models::trade::{Trade, TradeRow};

/// One resolved paper/sim fill (price + the corpus trade that priced it).
#[derive(Debug, Clone, PartialEq)]
pub struct PaperFill {
    /// Index into the chronological `trades` slice the helper was given.
    pub trade_idx: usize,
    pub price: f64,
    pub token_amount: f64,
    pub slot: u64,
    pub block_time: DateTime<Utc>,
    /// Base58 signature when the row carries one; `""` for slim cache/sweep rows.
    pub tx_signature: String,
}

fn paper_fill_from<T: TradeRow>(trades: &[T], idx: usize) -> PaperFill {
    let t = &trades[idx];
    PaperFill {
        trade_idx: idx,
        price: t.price_per_token(),
        token_amount: t.token_amount(),
        slot: t.slot(),
        block_time: t.block_time(),
        tx_signature: t.tx_signature().to_string(),
    }
}

/// Paper worst-case entry keyed by the trigger trade's index.
///
/// Fill model: window = trigger slot `S` (always) + the next observed slot after `S`
/// if it's within [`MAX_FILL_WAIT_SLOTS`]. Only trades at indices `> target_idx` are
/// considered (same-slot legs after the trigger are eligible). Fill = highest
/// qualifying buy price in the window. Returns `None` when the window has no
/// qualifying buy.
///
/// `target_idx` must index a real trade in `trades`.
pub fn find_worst_case_paper_entry_at<T: TradeRow>(
    trades: &[T],
    target_idx: usize,
) -> Option<PaperFill> {
    let trigger_slot = trades.get(target_idx)?.slot();
    let post = trades.get(target_idx + 1..).unwrap_or(&[]);
    let is_entry_buy = |t: &T| {
        t.is_buy() && t.price_per_token() > 0.0 && !Trade::is_dust(t.amount_sol())
    };

    // First slot > trigger_slot that has a qualifying buy — proximity check only.
    let next_slot = post
        .iter()
        .filter(|t| t.slot() > trigger_slot && is_entry_buy(t))
        .map(|t| t.slot())
        .next();

    let (best_rel, _) = post
        .iter()
        .enumerate()
        .filter(|(_, t)| {
            let s = t.slot();
            let in_s = s == trigger_slot;
            let in_next =
                next_slot.is_some_and(|ns| s == ns && ns <= trigger_slot + MAX_FILL_WAIT_SLOTS);
            (in_s || in_next) && is_entry_buy(t)
        })
        .max_by(|(_, a), (_, b)| a.price_per_token().total_cmp(&b.price_per_token()))?;

    Some(paper_fill_from(trades, target_idx + 1 + best_rel))
}

/// Paper worst-case exit keyed by the firing trade's index.
///
/// Window = fire slot `S` + the next observed slot after `S` when within
/// [`MAX_FILL_WAIT_SLOTS`]. Only trades after `fire_idx` are candidates. Fill =
/// lowest `price_per_token` in the window (any side).
///
/// When the window is empty: `market_fill_on_empty_window = true` (analysis) fills
/// at the firing trade itself; `false` (live paper poll) returns `None` so the
/// caller can wait or fail closed.
pub fn find_worst_case_paper_exit_at<T: TradeRow>(
    trades: &[T],
    fire_idx: usize,
    market_fill_on_empty_window: bool,
) -> Option<PaperFill> {
    let fire = trades.get(fire_idx)?;
    let fire_slot = fire.slot();
    let post = trades.get(fire_idx + 1..).unwrap_or(&[]);

    let next_slot = post.iter().map(|t| t.slot()).find(|&s| s > fire_slot);

    let in_window = |s: u64| match next_slot {
        Some(ns) if ns <= fire_slot + MAX_FILL_WAIT_SLOTS => s == fire_slot || s == ns,
        _ => s == fire_slot,
    };

    let best = post
        .iter()
        .enumerate()
        .filter(|(_, t)| in_window(t.slot()) && t.price_per_token() > 0.0)
        .min_by(|(_, a), (_, b)| a.price_per_token().total_cmp(&b.price_per_token()));

    match best {
        Some((rel, _)) => Some(paper_fill_from(trades, fire_idx + 1 + rel)),
        None if market_fill_on_empty_window && fire.price_per_token() > 0.0 => {
            Some(paper_fill_from(trades, fire_idx))
        }
        None => None,
    }
}

/// Whether the exit fill window starting at `fire_slot` is fully closed given the
/// highest slot observed so far — used by the live paper poll to know when to stop
/// waiting for more trades to index.
pub fn exit_fill_window_closed(fire_slot: u64, max_slot_seen: u64) -> bool {
    max_slot_seen > fire_slot + MAX_FILL_WAIT_SLOTS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::trade::TradeType;

    fn base_time() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).unwrap()
    }

    fn leg(sol: f64, tokens: f64, slot: u64, leg: u32, secs: i64) -> Trade {
        let mut tr = Trade::new(
            "mint".into(),
            "w".into(),
            TradeType::Buy,
            sol,
            tokens as u64,
            format!("sig-{slot}-{leg}"),
            slot,
            base_time() + chrono::Duration::seconds(secs),
        );
        tr.leg_index = leg;
        tr
    }

    fn sell(sol: f64, tokens: f64, slot: u64, leg_i: u32, secs: i64) -> Trade {
        let mut tr = leg(sol, tokens, slot, leg_i, secs);
        tr.trade_type = TradeType::Sell;
        tr
    }

    // ── entry ──────────────────────────────────────────────────────────────

    #[test]
    fn worst_case_entry_fills_in_window_of_trigger_and_next_slot() {
        let trigger = leg(1.0, 1.0, 100, 0, 0);
        let trades = vec![
            trigger.clone(),
            leg(1.2, 1.0, 100, 1, 0),
            leg(1.5, 1.0, 101, 0, 1),
            leg(1.8, 1.0, 101, 1, 1),
            leg(2.0, 1.0, 102, 0, 2),
        ];
        let entry = find_worst_case_paper_entry_at(&trades, 0).expect("qualifying buy");
        assert_eq!(entry.price, 1.8);
        assert_eq!(entry.trade_idx, 3);
    }

    #[test]
    fn worst_case_entry_fills_from_trigger_slot_when_no_next_slot() {
        let trades = vec![leg(1.0, 1.0, 100, 0, 0), leg(1.5, 1.0, 100, 1, 0)];
        let entry = find_worst_case_paper_entry_at(&trades, 0).expect("fill");
        assert!((entry.price - 1.5).abs() < 1e-9);
    }

    #[test]
    fn worst_case_entry_filters_dust_and_zero_price() {
        let dust_sol = crate::config::constants::MIN_TRADE_SOL / 2.0;
        let mut dust = leg(dust_sol, 1.0, 101, 0, 1);
        dust.amount_sol = dust_sol;
        let mut zero = leg(1.0, 0.0, 101, 1, 1);
        zero.price_per_token = 0.0;
        let trades = vec![leg(1.0, 1.0, 100, 0, 0), dust, zero, leg(1.1, 1.0, 101, 2, 1)];
        let entry = find_worst_case_paper_entry_at(&trades, 0).expect("valid buy");
        assert_eq!(entry.price, 1.1);
    }

    #[test]
    fn worst_case_entry_none_when_only_sells_after_trigger() {
        let trades = vec![leg(1.0, 1.0, 100, 0, 0), sell(0.9, 1.0, 101, 0, 1)];
        assert!(find_worst_case_paper_entry_at(&trades, 0).is_none());
    }

    #[test]
    fn worst_case_entry_none_when_window_empty() {
        let trades = vec![leg(1.0, 1.0, 100, 0, 0)];
        assert!(find_worst_case_paper_entry_at(&trades, 0).is_none());
    }

    #[test]
    fn worst_case_entry_none_past_max_wait() {
        let trades = vec![
            leg(1.0, 1.0, 100, 0, 0),
            leg(1.5, 1.0, 100 + MAX_FILL_WAIT_SLOTS + 1, 0, 5),
        ];
        assert!(find_worst_case_paper_entry_at(&trades, 0).is_none());
    }

    #[test]
    fn worst_case_entry_fills_at_max_wait_boundary() {
        let trades = vec![
            leg(1.0, 1.0, 100, 0, 0),
            leg(1.5, 1.0, 100 + MAX_FILL_WAIT_SLOTS, 0, 5),
        ];
        let entry = find_worst_case_paper_entry_at(&trades, 0).expect("boundary");
        assert!((entry.price - 1.5).abs() < 1e-9);
    }

    // ── exit ───────────────────────────────────────────────────────────────

    #[test]
    fn worst_case_exit_takes_lowest_in_window() {
        let trades = vec![
            leg(1.0, 1.0, 100, 0, 0), // fire
            leg(1.4, 1.0, 100, 1, 0),
            sell(1.1, 1.0, 101, 0, 1),
            leg(1.3, 1.0, 101, 1, 1),
            leg(0.5, 1.0, 102, 0, 2), // beyond next_slot
        ];
        let exit = find_worst_case_paper_exit_at(&trades, 0, false).expect("fill");
        assert_eq!(exit.price, 1.1);
        assert_eq!(exit.trade_idx, 2);
    }

    #[test]
    fn worst_case_exit_market_fill_on_empty_window() {
        let trades = vec![leg(1.0, 1.0, 100, 0, 0)];
        let exit = find_worst_case_paper_exit_at(&trades, 0, true).expect("market");
        assert_eq!(exit.trade_idx, 0);
        assert_eq!(exit.price, 1.0);
        assert!(find_worst_case_paper_exit_at(&trades, 0, false).is_none());
    }

    #[test]
    fn worst_case_exit_ignores_far_next_slot() {
        // next slot too far → window is fire_slot only.
        let trades = vec![
            leg(1.0, 1.0, 100, 0, 0),
            leg(0.8, 1.0, 100, 1, 0),
            leg(0.1, 1.0, 100 + MAX_FILL_WAIT_SLOTS + 1, 0, 5),
        ];
        let exit = find_worst_case_paper_exit_at(&trades, 0, false).expect("fill");
        assert_eq!(exit.price, 0.8);
        assert_eq!(exit.trade_idx, 1);
    }
}
