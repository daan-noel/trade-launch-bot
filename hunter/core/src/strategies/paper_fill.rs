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
//! That pick is what [`FillModel`] varies — and only that; the window, the
//! eligibility rules and so the taken-position set are the same under every model.
//!
//! Analysis paths and live paper entry pass `market_fill_on_empty_window = true`
//! so a trigger/fire with an empty window still books a market fill at that trade
//! (sparse / gappy prints) — same taken-position set across sim and live paper.
//! Live paper exit keeps it `false` and waits / fails closed.

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
/// `next_slot_first` / `next_slot_median` / `signal_price`); the short aliases
/// (`worst` / `first` / `next_first` / `next_median` / `signal`) match the
/// fill-sensitivity analysis doc's column labels so a request can use either.
///
/// The two `NextSlot*` models restrict candidates to the **reachable** half of the
/// window. A print in the signal's own slot `S` prices a block our transaction had
/// to already be inside — i.e. built before the signal existed — and a bundle leg
/// there is unreachable outright, since nothing sequences between atomic bundle
/// txs. Dropping slot `S` leaves the slot we actually land in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FillModel {
    /// Entry = highest qualifying buy, exit = lowest price in the window. The
    /// pessimistic bound; what live paper and the sweep book.
    #[default]
    WorstCase,
    /// Entry/exit = the FIRST qualifying trade after the signal in the window — a
    /// neutral "take the next print" reaction (no worst-case cherry-pick). Biased
    /// optimistic whenever that print is in the signal's own slot; the `NextSlot*`
    /// pair drops exactly those.
    FirstInWindow,
    /// Entry/exit = the first qualifying trade at the **next** slot, skipping the
    /// signal's own slot entirely. The earliest price a +1-slot landing can hit.
    NextSlotFirst,
    /// Entry/exit = the **adverse median** of the next slot's qualifying trades.
    /// Ordering inside a block is the leader's call, so we are effectively random
    /// within that slot rather than first — this reads the middle of the
    /// dispersion instead of either tail. Always a real print (see
    /// [`adverse_median_in`]), never a synthetic average.
    NextSlotMedian,
    /// Fill at the trigger/fire trade's own spot — zero feed-reaction slippage, the
    /// optimistic bound approximating a same-slot landing.
    SignalPrice,
    /// Entry/exit = the **last** qualifying trade whose `block_time` is at or before
    /// `ms` milliseconds after the signal's own — the only model keyed to a **measured**
    /// reaction time rather than to slot structure.
    ///
    /// Last, not first, and that is the whole correctness of the model: a row's price is
    /// the pool state AFTER that trade, so the first print at or after the deadline is a
    /// trade we could not have landed behind, and pricing from it reaches forward past
    /// our own fill. When nothing lands inside the lag the state is still the trigger's
    /// own, which is what a fill arriving before the next print actually executes
    /// against.
    ///
    /// The slot-shaped models bracket reality but cannot express it: `FirstInWindow`
    /// assumes we are always the very next print (an ordering nobody outside the
    /// leader can buy), while the `NextSlot*` pair assumes we never make the signal's
    /// own slot — yet the live book lands in it. A wall-clock lag states the one
    /// number that is actually measurable end to end, so a rule can be graded at the
    /// bot's own decide-to-fill latency instead of at a bound.
    ///
    /// `block_time` is the **ingest** clock (a Yellowstone transaction frame carries
    /// no chain time, so the decoder stamps `received_at`), which is the correct
    /// clock here: it measures when a print could first have been reacted to.
    ///
    /// Falls back to [`WorstCase`](FillModel::WorstCase) when the window holds no
    /// qualifying trade that late, exactly as the `NextSlot*` pair does — eligibility
    /// stays identical across every model.
    ///
    /// Serde: `"lag_115"` — a bare string like every other variant, so the grouped
    /// sweep's `TEXT` column, the request DTOs and the frontend all round-trip it
    /// with no payload-variant special case. The legacy `{"lag_ms": 115}` object form
    /// still parses, so anything already stored keeps its meaning.
    LagMs(u32),
}

/// The canonical wire name of each unit variant, paired with the short alias the
/// fill-sensitivity doc's column labels use. ONE table, read by both directions of
/// the codec, so a name can never serialize as one string and parse from another.
const FILL_MODEL_NAMES: &[(FillModel, &str, &str)] = &[
    (FillModel::WorstCase, "worst_case", "worst"),
    (FillModel::FirstInWindow, "first_in_window", "first"),
    (FillModel::NextSlotFirst, "next_slot_first", "next_first"),
    (FillModel::NextSlotMedian, "next_slot_median", "next_median"),
    (FillModel::SignalPrice, "signal_price", "signal"),
];

/// Prefix of the parameterized wall-clock-lag variant: `lag_115` = 115 ms.
const LAG_PREFIX: &str = "lag_";

impl FillModel {
    /// The canonical wire/display name. Always a plain string — including for
    /// [`LagMs`](FillModel::LagMs), which is what lets a `TEXT` column, a query
    /// param and a TypeScript union all carry the full set.
    pub fn as_str(self) -> std::borrow::Cow<'static, str> {
        match self {
            FillModel::LagMs(ms) => std::borrow::Cow::Owned(format!("{LAG_PREFIX}{ms}")),
            other => FILL_MODEL_NAMES
                .iter()
                .find(|(m, _, _)| *m == other)
                .map(|(_, name, _)| std::borrow::Cow::Borrowed(*name))
                .unwrap_or(std::borrow::Cow::Borrowed("worst_case")),
        }
    }

    /// Parse a wire name: canonical, short alias, or `lag_<ms>`.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if let Some(ms) = s.strip_prefix(LAG_PREFIX) {
            return ms.parse::<u32>().ok().map(FillModel::LagMs);
        }
        FILL_MODEL_NAMES
            .iter()
            .find(|(_, name, alias)| *name == s || *alias == s)
            .map(|(m, _, _)| *m)
    }

    /// The measured decide-to-fill lag in ms, for the models keyed to one.
    pub fn lag_ms(self) -> Option<u32> {
        match self {
            FillModel::LagMs(ms) => Some(ms),
            _ => None,
        }
    }
}

impl std::fmt::Display for FillModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.as_str())
    }
}

impl std::str::FromStr for FillModel {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or_else(|| format!("unknown fill model `{s}`"))
    }
}

impl Serialize for FillModel {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&self.as_str())
    }
}

impl<'de> Deserialize<'de> for FillModel {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = FillModel;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a fill-model name such as `worst_case` or `lag_115`")
            }
            fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<FillModel, E> {
                FillModel::parse(s).ok_or_else(|| E::custom(format!("unknown fill model `{s}`")))
            }
            /// The pre-string encoding of the lag variant (`{"lag_ms": 115}`), kept so
            /// a payload written before the codec landed still parses.
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut m: A) -> Result<FillModel, A::Error> {
                let mut out = None;
                while let Some(k) = m.next_key::<String>()? {
                    let v: serde_json::Value = m.next_value()?;
                    if k == "lag_ms" || k == "lag" || k == "LagMs" {
                        out = v.as_u64().map(|n| FillModel::LagMs(n as u32));
                    }
                }
                out.ok_or_else(|| serde::de::Error::custom("unknown fill model object"))
            }
        }
        de.deserialize_any(V)
    }
}

/// The contiguous run of trades at the window's **next** slot, plus its offset into
/// `post`. Empty when the window admits no later slot (nothing after `signal_slot`,
/// or the next one is past [`MAX_FILL_WAIT_SLOTS`]) — the `NextSlot*` models then
/// fall back to [`FillModel::WorstCase`], which keeps eligibility identical.
///
/// `post` is chronological and slots are non-decreasing along it, so one slot's
/// trades are contiguous — the same ordering `next_slot` itself relies on. Bounding
/// to this run is what keeps the median's pairwise scan off the whole tape.
fn next_slot_run<T: TradeRow>(
    post: &[T],
    signal_slot: u64,
    next_slot: Option<u64>,
) -> (usize, &[T]) {
    let Some(ns) = next_slot.filter(|&s| s <= signal_slot + MAX_FILL_WAIT_SLOTS) else {
        return (0, &[]);
    };
    let Some(start) = post.iter().position(|t| t.slot() == ns) else {
        return (0, &[]);
    };
    let end = post[start..]
        .iter()
        .position(|t| t.slot() != ns)
        .map_or(post.len(), |rel| start + rel);
    (start, &post[start..end])
}

/// Index into `run` of the adverse-median qualifying trade, or `None` when `run`
/// holds none.
///
/// An even count has two middle prints straddling the median: `adverse_high` takes
/// the upper (entry — a higher buy price is the adverse one), otherwise the lower
/// (exit). Both are REAL prints, so the fill keeps a genuine corpus row — a true
/// average of the two would be a price no trade ever printed, and `PaperFill`
/// carries `trade_idx` / `slot` / `tx_signature` pointing at one.
///
/// Equal prices break by tape order, so the pick is deterministic. `run` is one
/// slot's worth of trades, so the pairwise rank scan is bounded and allocation-free.
fn adverse_median_in<T: TradeRow>(
    run: &[T],
    qualifies: impl Fn(&T) -> bool,
    adverse_high: bool,
) -> Option<usize> {
    let n = run.iter().filter(|t| qualifies(t)).count();
    if n == 0 {
        return None;
    }
    let k = if adverse_high { n / 2 } else { (n - 1) / 2 };
    let before = |j: usize, i: usize| {
        match run[j].price_per_token().total_cmp(&run[i].price_per_token()) {
            std::cmp::Ordering::Less => true,
            std::cmp::Ordering::Equal => j < i,
            std::cmp::Ordering::Greater => false,
        }
    };
    (0..run.len()).find(|&i| {
        qualifies(&run[i])
            && (0..run.len()).filter(|&j| qualifies(&run[j]) && before(j, i)).count() == k
    })
}

/// Paper entry keyed by the trigger trade's index, priced per [`FillModel`].
///
/// Window = trigger slot `S` (always) + the next observed slot after `S` if it's
/// within [`MAX_FILL_WAIT_SLOTS`]. Only trades at indices `> target_idx` are
/// considered (same-slot legs after the trigger are eligible). When the window
/// has no qualifying buy: `market_fill_on_empty_window = true` (analysis + live
/// paper entry) fills at the trigger trade itself; `false` returns `None` so the
/// caller can wait or fail closed. Eligibility is identical across models; only
/// the fill price differs. `target_idx` must index a real trade in `trades`.
pub fn find_paper_entry_at<T: TradeRow>(
    trades: &[T],
    target_idx: usize,
    market_fill_on_empty_window: bool,
    model: FillModel,
) -> Option<PaperFill> {
    let trigger = trades.get(target_idx)?;
    let trigger_slot = trigger.slot();
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

    // Eligibility is fixed across models: a qualifying buy must exist in the
    // window (or the empty-window market-fill fallback below).
    if !post.iter().any(qualifies) {
        return if market_fill_on_empty_window && trigger.price_per_token() > 0.0 {
            Some(paper_fill_from(trades, target_idx))
        } else {
            None
        };
    }
    // The highest qualifying buy price in the window (adverse — the original), and
    // the `NextSlot*` fallback when the window admits no later slot.
    let worst = || {
        post.iter()
            .enumerate()
            .filter(|(_, t)| qualifies(t))
            .max_by(|(_, a), (_, b)| a.price_per_token().total_cmp(&b.price_per_token()))
            .map(|(rel, _)| rel)
    };
    let (run_base, run) = next_slot_run(post, trigger_slot, next_slot);
    let rel = match model {
        // Zero-slippage: the trigger's own spot (fill row = the trigger trade).
        FillModel::SignalPrice => return Some(paper_fill_from(trades, target_idx)),
        // The first qualifying buy after the trigger.
        FillModel::FirstInWindow => post.iter().position(qualifies),
        // The first qualifying buy at the next slot — the trigger's own slot dropped.
        FillModel::NextSlotFirst => {
            run.iter().position(qualifies).map(|rel| run_base + rel).or_else(worst)
        }
        // The adverse median of the next slot's qualifying buys.
        FillModel::NextSlotMedian => adverse_median_in(run, qualifies, true)
            .map(|rel| run_base + rel)
            .or_else(worst),
        // The pool state a buy landing `ms` after the trigger executes against: the
        // LAST qualifying buy at or before that instant. A row's price is the state
        // AFTER that trade, so the FIRST print at or after the deadline is a trade we
        // could not have landed behind — pricing from it reaches forward past our own
        // fill. When nothing lands inside the lag the state is still the trigger's own.
        FillModel::LagMs(ms) => {
            let deadline = trigger.block_time() + chrono::Duration::milliseconds(i64::from(ms));
            match post
                .iter()
                .enumerate()
                .filter(|(_, t)| qualifies(t) && t.block_time() <= deadline)
                .map(|(rel, _)| rel)
                .next_back()
            {
                Some(rel) => Some(rel),
                None => return Some(paper_fill_from(trades, target_idx)),
            }
        }
        FillModel::WorstCase => worst(),
    };
    rel.map(|rel| paper_fill_from(trades, target_idx + 1 + rel))
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

    // The lowest price in the window (adverse — the original), and the `NextSlot*`
    // fallback when the window admits no later slot.
    let worst = || {
        post.iter()
            .enumerate()
            .filter(|(_, t)| priced(t))
            .min_by(|(_, a), (_, b)| a.price_per_token().total_cmp(&b.price_per_token()))
            .map(|(rel, _)| rel)
    };
    let (run_base, run) = next_slot_run(post, fire_slot, next_slot);
    let fill_idx = if post.iter().any(priced) {
        match model {
            // Zero-slippage: sell at the fire trade's own spot.
            FillModel::SignalPrice => (fire.price_per_token() > 0.0).then_some(fire_idx),
            // The first priced trade in the window.
            FillModel::FirstInWindow => post.iter().position(priced).map(|rel| fire_idx + 1 + rel),
            // The first priced trade at the next slot — the fire's own slot dropped.
            FillModel::NextSlotFirst => run
                .iter()
                .position(priced)
                .map(|rel| run_base + rel)
                .or_else(worst)
                .map(|rel| fire_idx + 1 + rel),
            // The adverse median of the next slot's priced trades.
            FillModel::NextSlotMedian => adverse_median_in(run, priced, false)
                .map(|rel| run_base + rel)
                .or_else(worst)
                .map(|rel| fire_idx + 1 + rel),
            // Same rule as the entry leg: the LAST priced trade at or before
            // `fire + ms`, never the first one after it. Pricing a sell from the next
            // print credits us with flow that arrived after we sold — the error is
            // largest exactly where it hurts, on a take-profit firing into a rise.
            FillModel::LagMs(ms) => {
                let deadline = fire.block_time() + chrono::Duration::milliseconds(i64::from(ms));
                post.iter()
                    .enumerate()
                    .filter(|(_, t)| priced(t) && t.block_time() <= deadline)
                    .map(|(rel, _)| rel)
                    .next_back()
                    .map(|rel| fire_idx + 1 + rel)
                    .or(Some(fire_idx))
            }
            FillModel::WorstCase => worst().map(|rel| fire_idx + 1 + rel),
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
    market_fill_on_empty_window: bool,
) -> Option<PaperFill> {
    find_paper_entry_at(
        trades,
        target_idx,
        market_fill_on_empty_window,
        FillModel::WorstCase,
    )
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

    /// Like [`leg`] with millisecond resolution — the granularity a wall-clock
    /// lag model has to be tested at, since a whole second is already two slots.
    fn leg_ms(sol: f64, tokens: f64, slot: u64, leg_i: u32, ms: i64) -> Trade {
        let mut tr = leg(sol, tokens, slot, leg_i, 0);
        tr.block_time = base_time() + chrono::Duration::milliseconds(ms);
        tr
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
        let entry = find_worst_case_paper_entry_at(&trades, 0, false).expect("qualifying buy");
        assert_eq!(entry.price, 1.8);
        assert_eq!(entry.trade_idx, 3);
    }

    #[test]
    fn worst_case_entry_fills_from_trigger_slot_when_no_next_slot() {
        let trades = vec![leg(1.0, 1.0, 100, 0, 0), leg(1.5, 1.0, 100, 1, 0)];
        let entry = find_worst_case_paper_entry_at(&trades, 0, false).expect("fill");
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
        let entry = find_worst_case_paper_entry_at(&trades, 0, false).expect("valid buy");
        assert_eq!(entry.price, 1.1);
    }

    #[test]
    fn worst_case_entry_none_when_only_sells_after_trigger() {
        let trades = vec![leg(1.0, 1.0, 100, 0, 0), sell(0.9, 1.0, 101, 0, 1)];
        assert!(find_worst_case_paper_entry_at(&trades, 0, false).is_none());
    }

    #[test]
    fn worst_case_entry_none_when_window_empty() {
        let trades = vec![leg(1.0, 1.0, 100, 0, 0)];
        assert!(find_worst_case_paper_entry_at(&trades, 0, false).is_none());
    }

    #[test]
    fn worst_case_entry_none_past_max_wait() {
        let trades = vec![
            leg(1.0, 1.0, 100, 0, 0),
            leg(1.5, 1.0, 100 + MAX_FILL_WAIT_SLOTS + 1, 0, 5),
        ];
        assert!(find_worst_case_paper_entry_at(&trades, 0, false).is_none());
    }

    #[test]
    fn worst_case_entry_fills_at_max_wait_boundary() {
        let trades = vec![
            leg(1.0, 1.0, 100, 0, 0),
            leg(1.5, 1.0, 100 + MAX_FILL_WAIT_SLOTS, 0, 5),
        ];
        let entry = find_worst_case_paper_entry_at(&trades, 0, false).expect("boundary");
        assert!((entry.price - 1.5).abs() < 1e-9);
    }

    #[test]
    fn worst_case_entry_market_fill_on_empty_window() {
        // Isolated trigger (no post trades) → market-fill at trigger when flag true.
        let alone = vec![leg(1.0, 1.0, 100, 0, 0)];
        let fill = find_worst_case_paper_entry_at(&alone, 0, true).expect("market");
        assert_eq!(fill.trade_idx, 0);
        assert_eq!(fill.price, 1.0);
        assert!(find_worst_case_paper_entry_at(&alone, 0, false).is_none());

        // Only a sell after the trigger → still empty for entry eligibility.
        let sell_only = vec![leg(1.0, 1.0, 100, 0, 0), sell(0.9, 1.0, 101, 0, 1)];
        let fill = find_worst_case_paper_entry_at(&sell_only, 0, true).expect("market");
        assert_eq!(fill.trade_idx, 0);
        assert!(find_worst_case_paper_entry_at(&sell_only, 0, false).is_none());

        // Next buy exists but beyond MAX_FILL_WAIT_SLOTS → empty window.
        let far = vec![
            leg(1.0, 1.0, 100, 0, 0),
            leg(1.5, 1.0, 100 + MAX_FILL_WAIT_SLOTS + 1, 0, 5),
        ];
        let fill = find_worst_case_paper_entry_at(&far, 0, true).expect("market");
        assert_eq!(fill.trade_idx, 0);
        assert_eq!(fill.price, 1.0);
        assert!(find_worst_case_paper_entry_at(&far, 0, false).is_none());
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

    /// Every selectable model — a parity/eligibility assertion must cover each,
    /// never one hardcoded model.
    const ALL_MODELS: [FillModel; 6] = [
        FillModel::WorstCase,
        FillModel::FirstInWindow,
        FillModel::NextSlotFirst,
        FillModel::NextSlotMedian,
        FillModel::SignalPrice,
        FillModel::LagMs(115),
    ];

    #[test]
    fn fill_models_reprice_a_fixed_entry_set() {
        // trigger @100, then buys 1.2 (idx1,s100), then 1.5/1.8/1.6 at s101,
        // then 2.0 at s102 — a SECOND later slot, so out of the window entirely.
        let trades = vec![
            leg(1.0, 1.0, 100, 0, 0),
            leg(1.2, 1.0, 100, 1, 0),
            leg(1.5, 1.0, 101, 0, 1),
            leg(1.8, 1.0, 101, 1, 1),
            leg(1.6, 1.0, 101, 2, 1),
            leg(2.0, 1.0, 102, 0, 2),
        ];
        let at = |m| find_paper_entry_at(&trades, 0, false, m).unwrap();
        // Worst = highest buy in the window (1.8); First = earliest qualifying buy,
        // which is in the trigger's OWN slot (1.2); NextSlotFirst = earliest buy at
        // s101 (1.5); NextSlotMedian = middle of s101's three (1.6); Signal = the
        // trigger's own spot (1.0). All fill the SAME trigger (idx 0).
        assert_eq!(at(FillModel::WorstCase).price, 1.8);
        assert_eq!(at(FillModel::FirstInWindow).price, 1.2);
        assert_eq!(at(FillModel::NextSlotFirst).price, 1.5);
        assert_eq!(at(FillModel::NextSlotMedian).price, 1.6);
        assert_eq!(at(FillModel::SignalPrice).price, 1.0);
        assert_eq!(at(FillModel::SignalPrice).trade_idx, 0, "signal-price fill rows at the trigger");
        // The whole point of the NextSlot pair: never a print in the trigger's slot.
        for m in [FillModel::NextSlotFirst, FillModel::NextSlotMedian] {
            assert!(at(m).slot > 100, "{m:?} must skip the trigger's own slot");
        }
        // The median is a REAL print, not an average — its row prices it.
        let median = at(FillModel::NextSlotMedian);
        assert_eq!(trades[median.trade_idx].price_per_token(), median.price);
        // Worst-case wrapper stays byte-identical to the parameterized WorstCase.
        assert_eq!(
            find_worst_case_paper_entry_at(&trades, 0, false).unwrap(),
            at(FillModel::WorstCase)
        );
    }

    #[test]
    fn next_slot_models_fall_back_to_worst_when_the_window_has_no_next_slot() {
        // Next buy is past MAX_FILL_WAIT_SLOTS, so the window is the trigger slot
        // alone and the NextSlot pair has no reachable candidate.
        let trades = vec![
            leg(1.0, 1.0, 100, 0, 0),
            leg(1.5, 1.0, 100, 1, 0),
            leg(2.0, 1.0, 100 + MAX_FILL_WAIT_SLOTS + 1, 0, 5),
        ];
        let worst = find_paper_entry_at(&trades, 0, false, FillModel::WorstCase).unwrap();
        assert_eq!(worst.price, 1.5);
        for m in [FillModel::NextSlotFirst, FillModel::NextSlotMedian] {
            assert_eq!(find_paper_entry_at(&trades, 0, false, m).unwrap(), worst, "{m:?}");
        }
        // Same on the exit leg.
        let worst_exit = find_paper_exit_at(&trades, 0, false, FillModel::WorstCase).unwrap();
        for m in [FillModel::NextSlotFirst, FillModel::NextSlotMedian] {
            assert_eq!(find_paper_exit_at(&trades, 0, false, m).unwrap(), worst_exit, "{m:?}");
        }
    }

    #[test]
    fn the_adverse_median_leans_the_adverse_way_on_an_even_count() {
        // Two prints at s101 straddle the median: entry takes the higher (paying
        // more is adverse), exit the lower (selling for less is adverse).
        let trades =
            vec![leg(1.0, 1.0, 100, 0, 0), leg(1.2, 1.0, 101, 0, 1), leg(1.6, 1.0, 101, 1, 1)];
        let entry = find_paper_entry_at(&trades, 0, false, FillModel::NextSlotMedian).unwrap();
        let exit = find_paper_exit_at(&trades, 0, false, FillModel::NextSlotMedian).unwrap();
        assert_eq!(entry.price, 1.6, "entry median leans high");
        assert_eq!(exit.price, 1.2, "exit median leans low");
    }

    #[test]
    fn the_adverse_median_breaks_equal_prices_by_tape_order() {
        // Three prints at s101, two of them equal: the rank is still total, so the
        // pick is one determinate row rather than whichever the scan reached first.
        let trades = vec![
            leg(1.0, 1.0, 100, 0, 0),
            leg(1.4, 1.0, 101, 0, 1),
            leg(1.4, 1.0, 101, 1, 1),
            leg(1.9, 1.0, 101, 2, 1),
        ];
        let entry = find_paper_entry_at(&trades, 0, false, FillModel::NextSlotMedian).unwrap();
        assert_eq!((entry.price, entry.trade_idx), (1.4, 2), "the LATER of the equal pair");
    }


    /// The lag model is the only one keyed to wall-clock reaction time, and the
    /// only one that can fill inside the signal's OWN slot while still charging a
    /// delay. That combination is the whole point: the live book lands in the
    /// trigger's slot about half the time, so a model that always skips the slot
    /// (the `NextSlot*` pair) overcharges, while `FirstInWindow` charges nothing.
    #[test]
    fn the_lag_model_charges_wall_clock_not_slot_structure() {
        // Trigger at t=0 (slot 100), then two more buys in the SAME slot at
        // +50ms / +200ms, then the next slot a full second later.
        let trades = vec![
            leg_ms(1.0, 1.0, 100, 0, 0),
            leg_ms(1.2, 1.0, 100, 1, 50),
            leg_ms(1.4, 1.0, 100, 2, 200),
            leg_ms(1.5, 1.0, 101, 0, 1_000),
        ];
        let at = |m| find_paper_entry_at(&trades, 0, false, m).unwrap();

        // Zero lag: nothing has landed yet, so the state is the trigger's own spot.
        // NOT "the next print" — that print happens after us.
        assert_eq!(at(FillModel::LagMs(0)).price, 1.0);
        assert_eq!(at(FillModel::LagMs(0)).price, at(FillModel::SignalPrice).price);
        // 115ms (the measured decide->fill p50): the +50ms print has landed, the
        // +200ms one has not, so we execute against the +50ms state — still slot 100,
        // so a delay is charged WITHOUT pretending we missed the block.
        let lagged = at(FillModel::LagMs(115));
        assert_eq!(lagged.price, 1.2);
        assert_eq!(lagged.slot, 100, "a lag inside the slot must not skip the slot");
        // Waiting longer can only cost more on a rising tape — monotone in the lag,
        // which is the property that makes a fill LADDER meaningful.
        assert!(at(FillModel::LagMs(0)).price <= lagged.price);
        assert!(lagged.price <= at(FillModel::LagMs(300)).price);
        assert_eq!(at(FillModel::LagMs(300)).price, 1.4);
        // A lag past everything in the window fills at the LAST print inside it, not
        // the adverse one: by then every trade in the window has landed ahead of us.
        assert_eq!(
            at(FillModel::LagMs(60_000)).price,
            1.5,
            "a lag past the window prices at the last print, never no-fill"
        );
    }

    /// The regression this file exists to prevent: `LagMs` must never price a fill
    /// from a trade that lands AFTER the fill. A row's price is the pool state after
    /// that trade, so taking the first print at-or-after the deadline books flow we
    /// were not behind — measured at +8 to +12pp per trade in our favour.
    #[test]
    fn the_lag_model_never_prices_from_a_trade_that_lands_after_the_fill() {
        // Trigger at t=0, then a lone print at +900ms — well past a 115ms fill.
        let trades = vec![
            leg_ms(1.0, 1.0, 100, 0, 0),
            leg_ms(9.0, 1.0, 100, 1, 900),
        ];
        let fill = find_paper_entry_at(&trades, 0, false, FillModel::LagMs(115)).unwrap();
        assert_eq!(
            fill.price, 1.0,
            "nothing landed inside 115ms, so the fill is the trigger's own state"
        );
        assert_ne!(fill.price, 9.0, "the +900ms print is in our future at fill time");
    }

    /// The exit leg charges the same delay from the firing trade.
    #[test]
    fn the_lag_model_charges_the_exit_leg_too() {
        let trades = vec![
            leg_ms(1.0, 1.0, 100, 0, 0),
            leg_ms(0.9, 1.0, 100, 1, 50),
            leg_ms(0.7, 1.0, 100, 2, 200),
        ];
        let at = |m| find_paper_exit_at(&trades, 0, false, m).unwrap();
        // Zero lag: nothing has landed, so we sell into the fire trade's own state.
        assert_eq!(at(FillModel::LagMs(0)).price, 1.0, "zero lag = the fire's own spot");
        // 115ms: the +50ms print has landed and the +200ms one has not.
        assert_eq!(at(FillModel::LagMs(115)).price, 0.9, "the +50ms print is the state we hit");
        // Waiting longer costs more on a falling tape — monotone in the lag.
        assert_eq!(at(FillModel::LagMs(300)).price, 0.7);
    }

    /// The lag model is a BARE STRING on the wire like every other variant. That is
    /// the whole contract: a payload-shaped variant cannot live in the sweep's `TEXT`
    /// column, cannot be a TypeScript string union, and renders as `[object Object]`
    /// wherever the UI prints the model name — which is how it first showed up.
    #[test]
    fn the_lag_model_is_a_bare_string_on_the_wire() {
        use serde_json::json;
        assert_eq!(serde_json::to_value(FillModel::LagMs(115)).unwrap(), json!("lag_115"));
        let got: FillModel = serde_json::from_value(json!("lag_115")).unwrap();
        assert_eq!(got, FillModel::LagMs(115));
        // Every variant round-trips through the string form, so no caller needs a
        // special case for one of them.
        for m in ALL_MODELS {
            let wire = serde_json::to_value(m).unwrap();
            assert!(wire.is_string(), "{m:?} did not serialize as a string: {wire}");
            let back: FillModel = serde_json::from_value(wire).unwrap();
            assert_eq!(back, m);
            assert_eq!(FillModel::parse(&m.as_str()), Some(m));
        }
        // The pre-codec object form still parses, so anything already stored keeps
        // its meaning rather than failing the whole request it rides in.
        let legacy: FillModel = serde_json::from_value(json!({"lag_ms": 115})).unwrap();
        assert_eq!(legacy, FillModel::LagMs(115));
        let alias: FillModel = serde_json::from_value(json!({"lag": 250})).unwrap();
        assert_eq!(alias, FillModel::LagMs(250));
        // A garbage name is an error, never a silent fall back to the default - which
        // would book a different fill model than the one that was asked for.
        assert!(serde_json::from_value::<FillModel>(json!("lag_")).is_err());
        assert!(serde_json::from_value::<FillModel>(json!("nope")).is_err());
    }

    #[test]
    fn fill_model_serde_names_and_aliases() {
        use serde_json::json;
        // Canonical snake_case names.
        for (json_name, want) in [
            ("worst_case", FillModel::WorstCase),
            ("first_in_window", FillModel::FirstInWindow),
            ("next_slot_first", FillModel::NextSlotFirst),
            ("next_slot_median", FillModel::NextSlotMedian),
            ("signal_price", FillModel::SignalPrice),
        ] {
            let got: FillModel = serde_json::from_value(json!(json_name)).unwrap();
            assert_eq!(got, want, "canonical name '{json_name}'");
        }
        // Short aliases the analysis doc uses (the API contract the sim request relies on).
        for (alias, want) in [
            ("worst", FillModel::WorstCase),
            ("first", FillModel::FirstInWindow),
            ("next_first", FillModel::NextSlotFirst),
            ("next_median", FillModel::NextSlotMedian),
            ("signal", FillModel::SignalPrice),
        ] {
            let got: FillModel = serde_json::from_value(json!(alias)).unwrap();
            assert_eq!(got, want, "alias '{alias}'");
        }
        // Absent ⇒ WorstCase via #[derive(Default)] (what `#[serde(default)]` yields).
        assert_eq!(FillModel::default(), FillModel::WorstCase);
        // Serialize emits the canonical snake_case name.
        assert_eq!(serde_json::to_value(FillModel::FirstInWindow).unwrap(), json!("first_in_window"));
        assert_eq!(serde_json::to_value(FillModel::NextSlotMedian).unwrap(), json!("next_slot_median"));
    }

    #[test]
    fn fill_models_share_entry_eligibility() {
        // No qualifying buy after the trigger (only a sell) ⇒ None for EVERY model
        // when the empty-window fallback is off, so the taken-position set is
        // identical; models differ only in price.
        let trades = vec![leg(1.0, 1.0, 100, 0, 0), sell(0.9, 1.0, 101, 0, 1)];
        for m in ALL_MODELS {
            assert!(find_paper_entry_at(&trades, 0, false, m).is_none(), "{m:?}");
        }
        // With the analysis fallback on, every model fills at the SAME trigger
        // (taken-position set stays identical; price is the trigger spot for all).
        for m in ALL_MODELS {
            let fill = find_paper_entry_at(&trades, 0, true, m).expect("market");
            assert_eq!(fill.trade_idx, 0, "{m:?}");
            assert_eq!(fill.price, 1.0, "{m:?}");
        }
        // The NextSlot pair narrows the CANDIDATES, never eligibility: wherever a
        // fill exists at all, it exists under every model — otherwise a model
        // change would move the taken-position set and stop being a reprice.
        let mixed = vec![
            leg(1.0, 1.0, 100, 0, 0),
            leg(1.3, 1.0, 100, 1, 0),
            sell(0.9, 1.0, 101, 0, 1),
            leg(1.7, 1.0, 101, 1, 1),
        ];
        for idx in 0..mixed.len() {
            let want = find_paper_entry_at(&mixed, idx, false, FillModel::WorstCase).is_some();
            for m in ALL_MODELS {
                let got = find_paper_entry_at(&mixed, idx, false, m).is_some();
                assert_eq!(got, want, "entry eligibility at {idx} under {m:?}");
                let want_x = find_paper_exit_at(&mixed, idx, false, FillModel::WorstCase).is_some();
                let got_x = find_paper_exit_at(&mixed, idx, false, m).is_some();
                assert_eq!(got_x, want_x, "exit eligibility at {idx} under {m:?}");
            }
        }
    }

    #[test]
    fn fill_models_reprice_a_fixed_exit_set() {
        // fire @100 (price 1.0); s101 holds sell 1.1 (idx2), buy 1.3 (idx3), buy 1.2 (idx4).
        let trades = vec![
            leg(1.0, 1.0, 100, 0, 0),
            leg(1.4, 1.0, 100, 1, 0),
            sell(1.1, 1.0, 101, 0, 1),
            leg(1.3, 1.0, 101, 1, 1),
            leg(1.2, 1.0, 101, 2, 1),
        ];
        let at = |m| find_paper_exit_at(&trades, 0, false, m).unwrap();
        // Worst = lowest in window (1.1); First = first priced after the fire, which
        // is in the fire's OWN slot (1.4, idx1); NextSlotFirst = first print at s101
        // — here the low one, so it AGREES with worst by coincidence, not by design;
        // NextSlotMedian = middle of s101's three (1.2); Signal = the fire's spot.
        assert_eq!(at(FillModel::WorstCase).price, 1.1);
        assert_eq!((at(FillModel::FirstInWindow).price, at(FillModel::FirstInWindow).trade_idx), (1.4, 1));
        assert_eq!((at(FillModel::NextSlotFirst).price, at(FillModel::NextSlotFirst).trade_idx), (1.1, 2));
        assert_eq!((at(FillModel::NextSlotMedian).price, at(FillModel::NextSlotMedian).trade_idx), (1.2, 4));
        assert_eq!((at(FillModel::SignalPrice).price, at(FillModel::SignalPrice).trade_idx), (1.0, 0));
        for m in [FillModel::NextSlotFirst, FillModel::NextSlotMedian] {
            assert!(at(m).slot > 100, "{m:?} must skip the fire's own slot");
        }
        assert_eq!(
            find_worst_case_paper_exit_at(&trades, 0, false).unwrap(),
            at(FillModel::WorstCase)
        );
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
