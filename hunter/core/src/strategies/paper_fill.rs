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
use serde::{Deserialize, Serialize};

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

/// Which trade in the fill window prices a paper/sim fill.
///
/// [`WorstCase`](FillModel::WorstCase) is the **only** model live paper and the
/// grouped-sweep use (adverse on both legs); the others exist for the sim's
/// fill-sensitivity analysis — how much of the modeled ~4%/round cost is fill
/// *pessimism* vs a genuine absence of edge (flow-scalper lever #2). Every model
/// shares the same fill **eligibility** (the window + a qualifying trade must
/// exist), so the taken-position SET is identical across models and only the fill
/// PRICE varies — a controlled reprice, not a different entry population.
/// Serde: the canonical name is `snake_case` (`worst_case` / `first_in_window` /
/// `signal_price`); the short aliases (`worst` / `first` / `signal`) match the
/// fill-sensitivity analysis doc's column labels so a request can use either.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FillModel {
    /// Entry = highest qualifying buy, exit = lowest price in the window. The
    /// pessimistic bound; what live paper and the sweep book.
    #[default]
    #[serde(alias = "worst")]
    WorstCase,
    /// Entry/exit = the FIRST qualifying trade after the signal in the window — a
    /// neutral "take the next print" reaction (no worst-case cherry-pick).
    #[serde(alias = "first")]
    FirstInWindow,
    /// Fill at the trigger/fire trade's own spot — zero feed-reaction slippage, the
    /// optimistic bound approximating a same-slot landing.
    #[serde(alias = "signal")]
    SignalPrice,
}

/// Paper entry keyed by the trigger trade's index, priced per [`FillModel`].
///
/// Window = trigger slot `S` (always) + the next observed slot after `S` if it's
/// within [`MAX_FILL_WAIT_SLOTS`]. Only trades at indices `> target_idx` are
/// considered (same-slot legs after the trigger are eligible). Returns `None` when
/// the window has no qualifying buy — identical eligibility for every model; only
/// the chosen fill price differs. `target_idx` must index a real trade in `trades`.
pub fn find_paper_entry_at<T: TradeRow>(
    trades: &[T],
    target_idx: usize,
    model: FillModel,
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
    let in_window = |s: u64| {
        s == trigger_slot
            || next_slot.is_some_and(|ns| s == ns && ns <= trigger_slot + MAX_FILL_WAIT_SLOTS)
    };
    let qualifies = |t: &T| in_window(t.slot()) && is_entry_buy(t);

    // Eligibility is fixed across models: a qualifying buy must exist in the window.
    if !post.iter().any(qualifies) {
        return None;
    }
    match model {
        // Zero-slippage: the trigger's own spot (fill row = the trigger trade).
        FillModel::SignalPrice => Some(paper_fill_from(trades, target_idx)),
        // The first qualifying buy after the trigger.
        FillModel::FirstInWindow => post
            .iter()
            .position(qualifies)
            .map(|rel| paper_fill_from(trades, target_idx + 1 + rel)),
        // The highest qualifying buy price in the window (adverse — the original).
        FillModel::WorstCase => post
            .iter()
            .enumerate()
            .filter(|(_, t)| qualifies(t))
            .max_by(|(_, a), (_, b)| a.price_per_token().total_cmp(&b.price_per_token()))
            .map(|(rel, _)| paper_fill_from(trades, target_idx + 1 + rel)),
    }
}

/// Paper exit keyed by the firing trade's index, priced per [`FillModel`].
///
/// Window = fire slot `S` + the next observed slot after `S` when within
/// [`MAX_FILL_WAIT_SLOTS`]. Only trades after `fire_idx` are candidates. When the
/// window is empty: `market_fill_on_empty_window = true` (analysis) fills at the
/// firing trade itself; `false` (live paper poll) returns `None` so the caller can
/// wait or fail closed. Eligibility is identical across models; only the fill price
/// differs.
pub fn find_paper_exit_at<T: TradeRow>(
    trades: &[T],
    fire_idx: usize,
    market_fill_on_empty_window: bool,
    model: FillModel,
) -> Option<PaperFill> {
    let fire = trades.get(fire_idx)?;
    let fire_slot = fire.slot();
    let post = trades.get(fire_idx + 1..).unwrap_or(&[]);

    let next_slot = post.iter().map(|t| t.slot()).find(|&s| s > fire_slot);
    let in_window = |s: u64| match next_slot {
        Some(ns) if ns <= fire_slot + MAX_FILL_WAIT_SLOTS => s == fire_slot || s == ns,
        _ => s == fire_slot,
    };
    let priced = |t: &T| in_window(t.slot()) && t.price_per_token() > 0.0;

    let fill_idx = if post.iter().any(priced) {
        match model {
            // Zero-slippage: sell at the fire trade's own spot.
            FillModel::SignalPrice => (fire.price_per_token() > 0.0).then_some(fire_idx),
            // The first priced trade in the window.
            FillModel::FirstInWindow => post.iter().position(priced).map(|rel| fire_idx + 1 + rel),
            // The lowest price in the window (adverse — the original).
            FillModel::WorstCase => post
                .iter()
                .enumerate()
                .filter(|(_, t)| priced(t))
                .min_by(|(_, a), (_, b)| a.price_per_token().total_cmp(&b.price_per_token()))
                .map(|(rel, _)| fire_idx + 1 + rel),
        }
    } else {
        None
    };

    match fill_idx {
        Some(idx) => Some(paper_fill_from(trades, idx)),
        None if market_fill_on_empty_window && fire.price_per_token() > 0.0 => {
            Some(paper_fill_from(trades, fire_idx))
        }
        None => None,
    }
}

/// Worst-case entry (adverse). The model live paper + sweep use — a thin wrapper so
/// they never carry a [`FillModel`]; see [`find_paper_entry_at`].
pub fn find_worst_case_paper_entry_at<T: TradeRow>(
    trades: &[T],
    target_idx: usize,
) -> Option<PaperFill> {
    find_paper_entry_at(trades, target_idx, FillModel::WorstCase)
}

/// Worst-case exit (adverse). Live paper + sweep wrapper; see [`find_paper_exit_at`].
pub fn find_worst_case_paper_exit_at<T: TradeRow>(
    trades: &[T],
    fire_idx: usize,
    market_fill_on_empty_window: bool,
) -> Option<PaperFill> {
    find_paper_exit_at(trades, fire_idx, market_fill_on_empty_window, FillModel::WorstCase)
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

    // ── fill models (lever #2 sim knob) ─────────────────────────────────────

    #[test]
    fn fill_models_reprice_a_fixed_entry_set() {
        // trigger @100, then buys 1.2 (idx1,s100), 1.5 (idx2,s101), 1.8 (idx3,s101).
        let trades = vec![
            leg(1.0, 1.0, 100, 0, 0),
            leg(1.2, 1.0, 100, 1, 0),
            leg(1.5, 1.0, 101, 0, 1),
            leg(1.8, 1.0, 101, 1, 1),
        ];
        // Worst = highest buy (1.8); First = earliest qualifying buy (1.2);
        // Signal = the trigger's own spot (1.0). All fill the SAME trigger (idx 0).
        let worst = find_paper_entry_at(&trades, 0, FillModel::WorstCase).unwrap();
        let first = find_paper_entry_at(&trades, 0, FillModel::FirstInWindow).unwrap();
        let signal = find_paper_entry_at(&trades, 0, FillModel::SignalPrice).unwrap();
        assert_eq!(worst.price, 1.8);
        assert_eq!(first.price, 1.2);
        assert_eq!(signal.price, 1.0);
        assert_eq!(signal.trade_idx, 0, "signal-price fill rows at the trigger");
        // Worst-case wrapper stays byte-identical to the parameterized WorstCase.
        assert_eq!(find_worst_case_paper_entry_at(&trades, 0).unwrap(), worst);
    }

    #[test]
    fn fill_model_serde_names_and_aliases() {
        use serde_json::json;
        // Canonical snake_case names.
        for (json_name, want) in [
            ("worst_case", FillModel::WorstCase),
            ("first_in_window", FillModel::FirstInWindow),
            ("signal_price", FillModel::SignalPrice),
        ] {
            let got: FillModel = serde_json::from_value(json!(json_name)).unwrap();
            assert_eq!(got, want, "canonical name '{json_name}'");
        }
        // Short aliases the analysis doc uses (the API contract the sim request relies on).
        for (alias, want) in [
            ("worst", FillModel::WorstCase),
            ("first", FillModel::FirstInWindow),
            ("signal", FillModel::SignalPrice),
        ] {
            let got: FillModel = serde_json::from_value(json!(alias)).unwrap();
            assert_eq!(got, want, "alias '{alias}'");
        }
        // Absent ⇒ WorstCase via #[derive(Default)] (what `#[serde(default)]` yields).
        assert_eq!(FillModel::default(), FillModel::WorstCase);
        // Serialize emits the canonical snake_case name.
        assert_eq!(serde_json::to_value(FillModel::FirstInWindow).unwrap(), json!("first_in_window"));
    }

    #[test]
    fn fill_models_share_entry_eligibility() {
        // No qualifying buy after the trigger (only a sell) ⇒ None for EVERY model,
        // so the taken-position set is identical; models differ only in price.
        let trades = vec![leg(1.0, 1.0, 100, 0, 0), sell(0.9, 1.0, 101, 0, 1)];
        for m in [FillModel::WorstCase, FillModel::FirstInWindow, FillModel::SignalPrice] {
            assert!(find_paper_entry_at(&trades, 0, m).is_none(), "{m:?}");
        }
    }

    #[test]
    fn fill_models_reprice_a_fixed_exit_set() {
        // fire @100 (price 1.0); window has sell 1.1 (idx2,s101) and buy 1.3 (idx3,s101).
        let trades = vec![
            leg(1.0, 1.0, 100, 0, 0),
            leg(1.4, 1.0, 100, 1, 0),
            sell(1.1, 1.0, 101, 0, 1),
            leg(1.3, 1.0, 101, 1, 1),
        ];
        // Worst = lowest in window (1.1); First = first priced after fire (1.4, idx1);
        // Signal = the fire's own spot (1.0, idx0).
        let worst = find_paper_exit_at(&trades, 0, false, FillModel::WorstCase).unwrap();
        let first = find_paper_exit_at(&trades, 0, false, FillModel::FirstInWindow).unwrap();
        let signal = find_paper_exit_at(&trades, 0, false, FillModel::SignalPrice).unwrap();
        assert_eq!(worst.price, 1.1);
        assert_eq!((first.price, first.trade_idx), (1.4, 1));
        assert_eq!((signal.price, signal.trade_idx), (1.0, 0));
        assert_eq!(find_worst_case_paper_exit_at(&trades, 0, false).unwrap(), worst);
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
