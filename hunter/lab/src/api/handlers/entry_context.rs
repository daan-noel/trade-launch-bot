//! **Entry Context** — what the tape looked like in the seconds before each of a
//! trader's buys, read as the engine would read it.
//!
//! Every buy transaction the wallet made in the range is an anchor. For each, the
//! prints in `[entry - W, entry)` (tape order, so a print in the trader's own slot
//! counts only when its tx landed ahead of his) are folded through the engine's own
//! [`TagState`] under the target tag, and the window is read with the registry's
//! `m_flow` metrics on both halves: `buy_tx_count @tag [Ws]` and `@!tag`, and the
//! same for `buy_sol`. The control window is the same read one `W` earlier
//! (`[Ws@W]`), so every number carries the baseline it is judged against.
//!
//! The same fold breaks the window down by exact ix structure, classifying each print with the verdict the tag state returned — one
//! copy of the matchers, so a breakdown row can never disagree with the headline.
//!
//! The studied wallet is excluded from the tape in SQL: his own tool's structure
//! would otherwise sit in every window he is measured against.
//!
//! Each seat also carries its top-holder read ([`entry_holders`]): the price drop if
//! the coin's biggest holders sold their whole bags at once.
//!
//! Nothing here writes to the database. A scan's result is kept on disk
//! ([`entry_scan_cache`]).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use hunter_engine::grouping::normalize_labels;
use hunter_engine::metrics::fee::FeeKeys;
use hunter_engine::metrics::registry::Metric;
use hunter_engine::metrics::tags::config::{compile_tags, validate_tags, TagPatterns};
use hunter_engine::metrics::tags::state::TagState;
use hunter_engine::metrics::template_grain::{
    grain, grain_hash_from_labels_value, program_hash_from_labels_value,
};
use hunter_engine::metrics::trade_keys::{
    ix_hash_from_labels_value, marker_bits_from_labels_value, wallet_hash,
};
use hunter_engine::metrics::{Cursor, Side, TradeLite, WindowSpec, WindowUnit, NOMINAL_SLOT_SECS};
use super::entry_holders::{holders_at, HolderAsk, HolderRead};
use crate::lake;
use crate::state::entry_scan_cache;
use trading_core::config::constants::lamports_to_sol;
use trading_core::state::core_state::CoreState;
use trading_core::storage::repositories::trade_repo::{MintSpan, SlotWindow, TapePrint, TradeRepo, WalletBuyTx};

/// Most anchors one request reads. Past it the response says so (`truncated`)
/// rather than answering for a silent prefix.
const MAX_ENTRIES: i64 = 3_000;

/// Widest analysis window accepted, in seconds. The read spans `2W` (window + control).
const MAX_WINDOW_SECS: f64 = 600.0;

/// Widest probe window accepted, in slots — the Trader Analysis probe's own ceiling
/// (`pre_entry_ix::MAX_WINDOW_SLOTS`).
const MAX_PROBE_SLOTS: i64 = 2_000;

/// Slowest slot assumed when bounding the `block_time` side of a slot read: only
/// prunes chunks, so generous (the pre-entry probe's `SLOT_TIME_MS`).
const MAX_SLOT_MS: i64 = 600;

/// Mint windows per SQL round-trip, run one at a time — the same bound the
/// pre-entry probe keeps, for the same reason (heavy concurrent scans OOM the VM).
const WINDOWS_PER_QUERY: usize = 300;

/// Seconds per slot assumed when turning a seconds window into the slot range the
/// index read takes. Faster than the chain ever runs, so the slot range always
/// over-covers; the exact cut is `block_time`, applied in the fold.
const MIN_SLOT_SECS: f64 = 0.25;

/// Chunk-pruning slack on the `block_time` bounds (slot is the index filter).
const TIME_SLACK_SECS: i64 = 60;

/// Breakdown rows kept per entry, largest first. The rest are counted, not listed.
const MAX_GROUPS: usize = 30;

// ── Request ─────────────────────────────────────────────────────────────────

/// `POST /api/wallets/{wallet}/entry-context` body.
#[derive(Debug, Deserialize)]
pub struct EntryContextBody {
    /// Anchors are the wallet's buys with `from <= block_time <= to`.
    pub from: DateTime<Utc>,
    #[serde(default)]
    pub to: Option<DateTime<Utc>>,
    /// Analysis window `W`, seconds. Read as the engine's `[Ws]` span: the shares,
    /// counts and the structure breakdown.
    #[serde(default = "default_window_secs")]
    pub window_secs: f64,
    /// Probe window `P`, slots — the Trader Analysis pre-entry probe's window: did a
    /// tagged transaction land in `[entry - P, entry)`, with `[entry - 2P, entry - P)`
    /// as its control.
    #[serde(default = "default_probe_slots")]
    pub probe_slots: i64,
    /// ONE tag definition, in the fingerprint `tags` document's shape
    /// (`{"match": {...}, "side": "buy"}`), validated by the engine's own parser.
    /// Absent: no target — the windows and their breakdown are still read, and every
    /// share reads `None`.
    #[serde(default)]
    pub tag: Option<serde_json::Value>,
}

/// `POST /api/wallets/{wallet}/entry-context/range` body: one token, one picked
/// stretch of its tape, read exactly as an entry's window is. The range `[from, to]`
/// is the window; the same length just before it is the control.
#[derive(Debug, Deserialize)]
pub struct EntryRangeBody {
    pub mint: String,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    /// The last slot the range holds (the chart knows it from its trades): the read
    /// takes every print through this slot whose `block_time` is in the range.
    pub end_slot: i64,
    #[serde(default = "default_probe_slots")]
    pub probe_slots: i64,
    #[serde(default)]
    pub tag: Option<serde_json::Value>,
}

fn default_window_secs() -> f64 {
    30.0
}

fn default_probe_slots() -> i64 {
    25
}

// ── Response ────────────────────────────────────────────────────────────────

/// Why an entry could not be read. Never folded into a zero share.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnknownReason {
    /// The read reaches past the oldest tape `trades` still holds.
    TapeTruncated,
    /// The tag pins fee fields and no print in the window carries a fee reading.
    NoFeeReadings,
}

/// One window read on both halves of the tag. Counts are TRANSACTIONS the way the
/// engine counts them (a print with `leg_index = 0`); SOL sums every leg.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct WindowRead {
    /// `m_flow.buy_tx_count @tag` + `@!tag`: every buy transaction in the window.
    pub buy_tx: u32,
    /// `m_flow.buy_tx_count @tag`.
    pub tag_buy_tx: u32,
    pub buy_sol: f64,
    pub tag_buy_sol: f64,
    pub sell_tx: u32,
    pub tag_sell_tx: u32,
    pub sell_sol: f64,
    pub tag_sell_sol: f64,
    /// `tag_buy_tx / buy_tx`, percent. `None` when the window holds no buy.
    pub tx_share_pct: Option<f64>,
    /// `tag_buy_sol / buy_sol`, percent. `None` when the window holds no buy SOL.
    pub sol_share_pct: Option<f64>,
}

/// One breakdown row: the window's prints of one structure.
#[derive(Debug, Clone, Serialize)]
pub struct GroupRow {
    /// The group's identity: its exact ordered ix labels joined by ` > `.
    pub key: String,
    /// The ordered labels — what a pattern set stores. Absent on `(no labels)`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    pub buy_tx: u32,
    pub sell_tx: u32,
    pub buy_sol: f64,
    pub sell_sol: f64,
    /// Buy transactions of this group that carried the target tag.
    pub tag_buy_tx: u32,
    /// Distinct wallets among the group's prints.
    pub wallets: u32,
    /// Distinct one-second buckets holding a buy of this group.
    pub buy_secs: u32,
    /// This group's buy transactions over every buy transaction in the window.
    pub buy_tx_share_pct: Option<f64>,
    /// This group's buy SOL over every buy SOL in the window.
    pub buy_sol_share_pct: Option<f64>,
    /// The signal's group: the one the probe's nearest tagged print belongs to.
    /// Kept in the breakdown even past `MAX_GROUPS`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub signal: bool,
}

/// The tagged print nearest ahead of his buy inside the probe window — how close
/// the target came before he acted, and which structure it was.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NearestTag {
    /// Slots back from his. `0` = his own slot, where `lag_tx` is the whole distance
    /// and the print is co-arrival rather than something a later seat could read.
    pub lag_slots: i64,
    /// Transactions ahead of his in the same slot; `None` a slot or more away, where
    /// the difference of two block positions is not a distance.
    pub lag_tx: Option<i64>,
    /// Seconds back by block time (second precision, shared by a whole slot).
    pub lag_secs: f64,
    /// Its own tape position: where [`seat_behind`] stands to read what it showed.
    pub slot: i64,
    pub tx_index: i32,
    /// Its template grain (`program|CU|ATA|N|S|F`): the sort and search key.
    pub key: String,
    /// Its exact ordered ix labels, for the Matched cell's abbreviation line.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
}

/// The pre-entry probe's read over its slot window: tagged TRANSACTIONS (either
/// side, on the tag's side; `leg_index = 0`, as the analysis window counts) and the
/// SOL their legs moved, the same one window earlier, and the nearest one. The
/// thresholds (min hits / min SOL) are the page's to apply.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ProbeRead {
    pub hits: u32,
    pub sol: f64,
    pub control_hits: u32,
    pub control_sol: f64,
    pub nearest: Option<NearestTag>,
}

/// One anchor: the trader's buy transaction and the window before it.
#[derive(Debug, Clone, Serialize)]
pub struct EntryRow {
    pub mint_address: String,
    pub slot: i64,
    pub tx_index: i32,
    pub at: DateTime<Utc>,
    /// SOL his buy legs spent in this transaction.
    pub sol: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unknown_reason: Option<UnknownReason>,
    pub window: WindowRead,
    pub control: WindowRead,
    pub groups: Vec<GroupRow>,
    /// Breakdown rows past [`MAX_GROUPS`], left out.
    pub groups_omitted: u32,
    pub probe: ProbeRead,
    /// The same read from the seat right behind the signal (the probe's nearest
    /// target print): what a bot firing on that print reads, and what the scan
    /// reads for that print. Absent with no signal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at_signal: Option<SeatRead>,
    /// The top-holder read at the seat; absent when the coin's history is not all
    /// on the tape.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holders: Option<HolderRead>,
}

/// The window, its control and its breakdown, read from one seat.
#[derive(Debug, Clone, Serialize)]
pub struct SeatRead {
    pub window: WindowRead,
    pub control: WindowRead,
    pub groups: Vec<GroupRow>,
    pub groups_omitted: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holders: Option<HolderRead>,
}

#[derive(Debug, Serialize)]
pub struct EntryContextResponse {
    pub entries: Vec<EntryRow>,
    /// `true` when the wallet made more buys in the range than [`MAX_ENTRIES`]:
    /// `entries` is then the most recent ones only.
    pub truncated: bool,
    pub max_entries: i64,
    pub window_secs: f64,
    pub probe_slots: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tape_floor: Option<DateTime<Utc>>,
}

// ── Tag ─────────────────────────────────────────────────────────────────────

/// Compile the target tag with the engine's parser, refusing matchers that need the
/// coin's history before the window: this read folds the window alone, so the
/// creator would classify from nothing. `sticky` is accepted
/// and scoped to each read's own span (see [`fold_entry`]).
fn compile_target(def: &serde_json::Value) -> Result<TagPatterns, String> {
    let doc = serde_json::json!({ "target": def });
    validate_tags(&doc)?;
    if def.get("match").and_then(serde_json::Value::as_object).is_some_and(|m| m.contains_key("creator")) {
        return Err("`creator` needs the coin's history before the window; this read folds the window only".into());
    }
    compile_tags(&doc)
        .into_iter()
        .next()
        .map(|t| t.patterns)
        .ok_or_else(|| "the tag did not compile".to_string())
}

// ── Fold ────────────────────────────────────────────────────────────────────

/// One stored leg as the engine's `TradeLite`, with the fields a tag reads. The
/// hashes come from the same label readers `rule_readout::trade_lite` uses.
fn trade_lite(p: &TapePrint) -> TradeLite {
    let labels = p.ix_labels.as_ref().unwrap_or(&serde_json::Value::Null);
    TradeLite {
        side: if p.is_buy { Side::Buy } else { Side::Sell },
        sol: lamports_to_sol(p.amount_lamports),
        at: p.block_time,
        ix_hash: ix_hash_from_labels_value(labels),
        wallet_hash: wallet_hash(&p.wallet_address),
        slot: u64::try_from(p.slot).unwrap_or(0),
        marker_bits: marker_bits_from_labels_value(labels),
        leg_index: u8::try_from(p.leg_index.max(0)).unwrap_or(u8::MAX),
        tx_index: u32::try_from(p.tx_index).ok(),
        template_hash: grain_hash_from_labels_value(labels),
        program_hash: program_hash_from_labels_value(labels),
        fee: fee_keys(p),
        ..TradeLite::default()
    }
}

fn fee_keys(p: &TapePrint) -> FeeKeys {
    FeeKeys::new(
        p.cu_limit.and_then(|v| u32::try_from(v).ok()),
        p.cu_price.and_then(|v| u64::try_from(v).ok()),
        p.tip_lamports.and_then(|v| u64::try_from(v).ok()),
    )
}

/// The group a print belongs to (its exact ordered ix labels, joined by ` > `), and
/// those labels.
fn group_key(p: &TapePrint) -> (String, Option<Vec<String>>) {
    let labels = p.ix_labels.as_ref().map(normalize_labels).unwrap_or_default();
    if labels.is_empty() {
        return ("(no labels)".into(), None);
    }
    (labels.join(" > "), Some(labels))
}

/// `part / whole` in percent, `None` on an empty whole.
fn pct(part: f64, whole: f64) -> Option<f64> {
    (whole > 0.0).then(|| 100.0 * part / whole)
}

#[derive(Default)]
struct GroupAcc {
    labels: Option<Vec<String>>,
    buy_tx: u32,
    sell_tx: u32,
    buy_sol: f64,
    sell_sol: f64,
    tag_buy_tx: u32,
    wallets: HashSet<u64>,
    buy_secs: HashSet<i64>,
}

/// Read one window of the tag state on both halves.
fn read_window(st: &TagState, spec: WindowSpec, now: DateTime<Utc>) -> WindowRead {
    let cur = Cursor::default();
    let get = |m: Metric, negated: bool| {
        let v = st.value(m, negated, Some(spec), now, cur);
        if v.is_finite() { v } else { 0.0 }
    };
    // Counts are whole numbers held in f64 by the tag state.
    let count = |m: Metric, negated: bool| get(m, negated).round() as u32;
    let tag_buy_tx = count(Metric::BuyTxCount, false);
    let buy_tx = tag_buy_tx + count(Metric::BuyTxCount, true);
    let tag_buy_sol = get(Metric::BuySol, false);
    let buy_sol = tag_buy_sol + get(Metric::BuySol, true);
    let tag_sell_tx = count(Metric::SellTxCount, false);
    let tag_sell_sol = get(Metric::SellSol, false);
    WindowRead {
        buy_tx,
        tag_buy_tx,
        buy_sol,
        tag_buy_sol,
        sell_tx: tag_sell_tx + count(Metric::SellTxCount, true),
        tag_sell_tx,
        sell_sol: tag_sell_sol + get(Metric::SellSol, true),
        tag_sell_sol,
        tx_share_pct: pct(f64::from(tag_buy_tx), f64::from(buy_tx)),
        sol_share_pct: pct(tag_buy_sol, buy_sol),
    }
}

/// Where one read's span starts: the analysis window and its control by block
/// time, the probe window and its control by slot.
#[derive(Clone, Copy)]
enum ScopeStart {
    Ms(i64),
    Slot(i64),
}

impl ScopeStart {
    fn holds(self, slot: i64, at_ms: i64) -> bool {
        match self {
            Self::Ms(lo) => at_ms >= lo,
            Self::Slot(lo) => slot >= lo,
        }
    }
}

/// The four reads, in [`fold_entry`]'s scope order.
const SCOPE_WINDOW: usize = 0;
const SCOPE_CONTROL: usize = 1;
const SCOPE_PROBE: usize = 2;
const SCOPE_PROBE_CONTROL: usize = 3;

/// Fold one anchor's tape: `prints` is the mint's read in tape order, the trader
/// already excluded. Returns the entry with its window, control and breakdown.
///
/// Under `sticky` each read (window, control, probe, probe control) folds its own
/// tag state from its own start, so a wallet carries the tag from its first target
/// trade INSIDE that span: the sticky memory is the span, not the coin's history.
/// A wallet tagged just before a span starts is untagged in it, so a sticky share
/// here can read below the same tag's live read. Without `sticky` a tag is a pure
/// function of the transaction and one state serves every read.
fn fold_entry(
    anchor: &WalletBuyTx,
    prints: &[TapePrint],
    patterns: &TagPatterns,
    window_secs: f64,
    probe_slots: i64,
) -> EntryRow {
    let win = WindowSpec::secs(window_secs);
    let ctl = WindowSpec { size: window_secs, lag: window_secs, unit: WindowUnit::Sec };
    let entry_ms = anchor.block_time.timestamp_millis();
    let w_ms = (window_secs * 1000.0).round() as i64;
    // The spans' own closed bounds: `[now - W, now]` and `[now - 2W, now - W]`.
    let (win_lo, ctl_lo) = (entry_ms - w_ms, entry_ms - 2 * w_ms);

    // The probe's slot windows, as `pre_entry_ix` cuts them.
    let (probe_lo, probe_ctl_lo) = (anchor.slot - probe_slots, anchor.slot - 2 * probe_slots);
    let starts = [
        ScopeStart::Ms(win_lo),
        ScopeStart::Ms(ctl_lo),
        ScopeStart::Slot(probe_lo),
        ScopeStart::Slot(probe_ctl_lo),
    ];
    let sticky = patterns.sticky;
    let state_of = |scope: usize| if sticky { scope } else { 0 };
    let mut states: Vec<TagState> =
        (0..if sticky { starts.len() } else { 1 }).map(|_| TagState::new(patterns.clone())).collect();
    states[state_of(SCOPE_WINDOW)].ensure_window(win);
    states[state_of(SCOPE_CONTROL)].ensure_window(ctl);
    let mut groups: HashMap<String, GroupAcc> = HashMap::new();
    let mut window_has_fee = false;
    let mut window_prints = 0usize;
    let mut probe = ProbeRead::default();
    // Tape order: the last tagged print seen is the nearest to his buy.
    let mut nearest: Option<(i64, i32, i64, &TapePrint)> = None;

    for p in prints {
        // Tape order: everything from his transaction on is after the decision.
        if (p.slot, p.tx_index) >= (anchor.slot, anchor.tx_index) {
            break;
        }
        let at_ms = p.block_time.timestamp_millis();
        let held = starts.map(|s| s.holds(p.slot, at_ms));
        if !held.contains(&true) {
            continue;
        }
        let t = trade_lite(p);
        let cur = Cursor { slot: t.slot, print: 0 };
        // Under sticky each state folds its own span's prints; the one shared state
        // folds every print some span holds (a print older than a time-bounded span
        // changes no read of it).
        let mut folded = [false; 4];
        for (i, st) in states.iter_mut().enumerate() {
            if !sticky || held[i] {
                folded[i] = st.on_trade(&t, cur);
            }
        }
        let tagged_in = |scope: usize| held[scope] && folded[state_of(scope)];
        let first_leg = t.leg_index == 0;
        if held[SCOPE_PROBE] {
            if tagged_in(SCOPE_PROBE) {
                probe.hits += u32::from(first_leg);
                probe.sol += t.sol;
                nearest = Some((p.slot, p.tx_index, at_ms, p));
            }
        } else if tagged_in(SCOPE_PROBE_CONTROL) {
            probe.control_hits += u32::from(first_leg);
            probe.control_sol += t.sol;
        }
        if !held[SCOPE_WINDOW] {
            continue;
        }
        let tagged = tagged_in(SCOPE_WINDOW);
        window_prints += 1;
        window_has_fee |= !t.fee.is_empty();
        let (key, labels) = group_key(p);
        let g = groups.entry(key).or_default();
        if g.labels.is_none() {
            g.labels = labels;
        }
        g.wallets.insert(t.wallet_hash);
        match t.side {
            Side::Buy => {
                g.buy_sol += t.sol;
                g.buy_secs.insert(at_ms.div_euclid(1000));
                if first_leg {
                    g.buy_tx += 1;
                    g.tag_buy_tx += u32::from(tagged);
                }
            }
            Side::Sell => {
                g.sell_sol += t.sol;
                g.sell_tx += u32::from(first_leg);
            }
        }
    }

    let window = read_window(&states[state_of(SCOPE_WINDOW)], win, anchor.block_time);
    let control = read_window(&states[state_of(SCOPE_CONTROL)], ctl, anchor.block_time);
    let unknown_reason = (patterns.builds.pins_fee() && window_prints > 0 && !window_has_fee)
        .then_some(UnknownReason::NoFeeReadings);
    let signal_key = nearest.map(|(.., p)| group_key(p).0);

    let mut rows: Vec<GroupRow> = groups
        .into_iter()
        .map(|(key, g)| GroupRow {
            labels: g.labels,
            buy_tx: g.buy_tx,
            sell_tx: g.sell_tx,
            buy_sol: g.buy_sol,
            sell_sol: g.sell_sol,
            tag_buy_tx: g.tag_buy_tx,
            wallets: g.wallets.len() as u32,
            buy_secs: g.buy_secs.len() as u32,
            buy_tx_share_pct: pct(f64::from(g.buy_tx), f64::from(window.buy_tx)),
            buy_sol_share_pct: pct(g.buy_sol, window.buy_sol),
            signal: signal_key.as_ref() == Some(&key),
            key,
        })
        .collect();
    rows.sort_by(|a, b| {
        b.buy_tx
            .cmp(&a.buy_tx)
            .then(b.buy_sol.total_cmp(&a.buy_sol))
            .then_with(|| a.key.cmp(&b.key))
    });
    let groups_omitted = rows.len().saturating_sub(MAX_GROUPS) as u32;
    // The signal's row stays, in the last kept place, when the cap would cut it.
    if let Some(i) = rows.iter().position(|r| r.signal).filter(|&i| i >= MAX_GROUPS) {
        rows.swap(i, MAX_GROUPS - 1);
    }
    rows.truncate(MAX_GROUPS);
    probe.nearest = nearest.map(|(slot, tx, at_ms, p)| {
        let lag_slots = anchor.slot - slot;
        let labels = p.ix_labels.as_ref().map(normalize_labels).unwrap_or_default();
        NearestTag {
            lag_slots,
            lag_tx: (lag_slots == 0).then(|| i64::from(anchor.tx_index) - i64::from(tx)),
            lag_secs: (entry_ms - at_ms) as f64 / 1000.0,
            slot,
            tx_index: tx,
            key: grain(&labels),
            labels,
        }
    });

    EntryRow {
        mint_address: anchor.mint_address.clone(),
        slot: anchor.slot,
        tx_index: anchor.tx_index,
        at: anchor.block_time,
        sol: lamports_to_sol(anchor.amount_lamports),
        unknown_reason,
        window,
        control,
        groups: rows,
        groups_omitted,
        probe,
        at_signal: None,
        holders: None,
    }
}

/// The top-holder read at each entry's own seat and at its signal's.
async fn fill_entry_holders(
    repo: &TradeRepo,
    wallet: &str,
    entries: &mut [EntryRow],
    tape_floor: Option<DateTime<Utc>>,
) -> Result<(), EntryContextError> {
    let mut asks = Vec::new();
    for e in entries.iter() {
        asks.push(HolderAsk { mint: e.mint_address.clone(), seat: (e.slot, e.tx_index), at: e.at });
        if let (Some(_), Some(n)) = (&e.at_signal, &e.probe.nearest) {
            asks.push(HolderAsk { mint: e.mint_address.clone(), seat: (n.slot, n.tx_index + 1), at: e.at });
        }
    }
    let reads = holders_at(repo, wallet, &asks, tape_floor).await.map_err(EntryContextError::Db)?;
    for e in entries.iter_mut() {
        e.holders = reads.get(&(e.mint_address.clone(), (e.slot, e.tx_index))).copied();
        if let (Some(s), Some(n)) = (e.at_signal.as_mut(), &e.probe.nearest) {
            s.holders = reads.get(&(e.mint_address.clone(), (n.slot, n.tx_index + 1))).copied();
        }
    }
    Ok(())
}

/// The read from the seat right behind the transaction at `(slot, tx_index)`: the
/// window and the probe hold that transaction, as they do for a buy following it.
/// One read for both sides: the scan stands behind each target buy, and his entry
/// stands behind the target print he followed.
fn seat_behind(
    mint: &str,
    (slot, tx_index, at): (i64, i32, DateTime<Utc>),
    prints: &[TapePrint],
    patterns: &TagPatterns,
    w: f64,
    pw: i64,
) -> EntryRow {
    let anchor = WalletBuyTx {
        mint_address: mint.to_string(),
        slot,
        tx_index: tx_index + 1,
        block_time: at,
        amount_lamports: 0,
    };
    fold_entry(&anchor, prints, patterns, w, pw)
}

/// His entry read again from behind its signal; `None` with no signal.
fn at_signal(e: &EntryRow, prints: &[TapePrint], patterns: &TagPatterns, w: f64, pw: i64) -> Option<SeatRead> {
    let n = e.probe.nearest.as_ref()?;
    let at = e.at - Duration::milliseconds((n.lag_secs * 1000.0).round() as i64);
    let r = seat_behind(&e.mint_address, (n.slot, n.tx_index, at), prints, patterns, w, pw);
    Some(SeatRead {
        window: r.window,
        control: r.control,
        groups: r.groups,
        groups_omitted: r.groups_omitted,
        holders: None,
    })
}

/// One slot range per mint covering every anchor's reads — the analysis `2W` and
/// the probe's `2P` — overlapping ranges merged so a print is fetched once however
/// many anchors share it.
fn mint_windows(anchors: &[&WalletBuyTx], window_secs: f64, probe_slots: i64) -> Vec<SlotWindow> {
    // The signal sits up to `P` slots back, and its own read ([`at_signal`]) spans `2W` before it.
    let back_slots = ((2.0 * window_secs / MIN_SLOT_SECS).ceil() as i64).max(2 * probe_slots) + probe_slots + 1;
    let back_ms =
        ((2.0 * window_secs * 1000.0).round() as i64).max(2 * probe_slots * MAX_SLOT_MS) + probe_slots * MAX_SLOT_MS;
    let back = Duration::milliseconds(back_ms) + Duration::seconds(TIME_SLACK_SECS);
    let mut sorted: Vec<&WalletBuyTx> = anchors.to_vec();
    sorted.sort_by(|a, b| (&a.mint_address, a.slot).cmp(&(&b.mint_address, b.slot)));
    let mut out: Vec<SlotWindow> = Vec::new();
    for a in sorted {
        let lo_slot = a.slot - back_slots;
        let lo_time = a.block_time - back;
        let hi_time = a.block_time + Duration::seconds(TIME_SLACK_SECS);
        match out.last_mut() {
            Some(w) if w.mint_address == a.mint_address && lo_slot <= w.hi_slot => {
                w.hi_slot = w.hi_slot.max(a.slot);
                w.lo_time = w.lo_time.min(lo_time);
                w.hi_time = w.hi_time.max(hi_time);
            }
            _ => out.push(SlotWindow {
                mint_address: a.mint_address.clone(),
                lo_slot,
                hi_slot: a.slot,
                lo_time,
                hi_time,
            }),
        }
    }
    out
}

/// A read with no target has no share to state: blank the target fields rather
/// than report every window as 0 % tagged.
fn untarget(e: &mut EntryRow) {
    for r in [&mut e.window, &mut e.control] {
        r.tx_share_pct = None;
        r.sol_share_pct = None;
    }
}

// ── Read ────────────────────────────────────────────────────────────────────

/// Widest picked range, in seconds. The read spans twice it (range + control) on
/// one mint.
const MAX_RANGE_SECS: f64 = 3_600.0;

/// `POST /api/wallets/{wallet}/entry-context/range` response.
#[derive(Debug, Serialize)]
pub struct EntryRangeResponse {
    /// The range read as one entry's window: `window` is the range, `control` the
    /// same length before it, `groups` its breakdown.
    pub read: EntryRow,
    pub window_secs: f64,
}

/// Why a read failed: the caller's request, or the database.
#[derive(Debug)]
pub enum EntryContextError {
    Bad(String),
    Db(anyhow::Error),
}

/// The target, or none: with no matcher every print folds on the rest half.
fn compile_optional(tag: Option<&serde_json::Value>) -> Result<(TagPatterns, bool), EntryContextError> {
    match tag {
        Some(def) => compile_target(def).map(|p| (p, true)).map_err(EntryContextError::Bad),
        None => Ok((TagPatterns::default(), false)),
    }
}

/// One picked range of one mint, read as an entry's window: a synthetic anchor at
/// the range's end (just past `end_slot`, at `to`) with `W = to - from`, folded by
/// [`fold_entry`], so a range and an entry are one read. His own trades stay out.
pub async fn read_entry_range(
    repo: &TradeRepo,
    wallet: &str,
    body: &EntryRangeBody,
) -> Result<EntryRangeResponse, EntryContextError> {
    if body.to < body.from {
        return Err(EntryContextError::Bad("`to` is before `from`".into()));
    }
    // A one-instant range (one slot) still holds its prints: the engine span is closed.
    let w = ((body.to - body.from).num_milliseconds() as f64 / 1000.0).max(0.001);
    if w > MAX_RANGE_SECS {
        return Err(EntryContextError::Bad(format!("the range must be at most {MAX_RANGE_SECS} s")));
    }
    let pw = body.probe_slots;
    if !(1..=MAX_PROBE_SLOTS).contains(&pw) {
        return Err(EntryContextError::Bad(format!("probe_slots must be 1..={MAX_PROBE_SLOTS}")));
    }
    let (patterns, targeted) = compile_optional(body.tag.as_ref())?;

    let anchor = WalletBuyTx {
        mint_address: body.mint.clone(),
        slot: body.end_slot + 1,
        tx_index: 0,
        block_time: body.to,
        amount_lamports: 0,
    };
    let windows = mint_windows(&[&anchor], w, pw);
    let mut prints = repo
        .prints_in_slot_windows(&windows, Some(wallet))
        .await
        .map_err(EntryContextError::Db)?;
    prints.sort_by_key(|p| (p.slot, p.tx_index, p.leg_index));

    let tape_floor = repo.tape_floor().await.unwrap_or_else(|e| {
        tracing::warn!("entry context range: tape floor unreadable: {e}");
        None
    });
    let mut read = fold_entry(&anchor, &prints, &patterns, w, pw);
    if tape_floor.is_some_and(|f| body.from - (body.to - body.from) < f) {
        read.unknown_reason = Some(UnknownReason::TapeTruncated);
    }
    if !targeted {
        untarget(&mut read);
    }
    Ok(EntryRangeResponse { read, window_secs: w })
}

/// The window, probe and range bounds every buys read shares.
fn check_body(body: &EntryContextBody) -> Result<(), EntryContextError> {
    let w = body.window_secs;
    if !w.is_finite() || w <= 0.0 || w > MAX_WINDOW_SECS {
        return Err(EntryContextError::Bad(format!("window_secs must be in (0, {MAX_WINDOW_SECS}]")));
    }
    if !(1..=MAX_PROBE_SLOTS).contains(&body.probe_slots) {
        return Err(EntryContextError::Bad(format!("probe_slots must be 1..={MAX_PROBE_SLOTS}")));
    }
    if body.to.is_some_and(|to| to < body.from) {
        return Err(EntryContextError::Bad("`to` is before `from`".into()));
    }
    Ok(())
}

/// Fetch every window's prints, grouped per mint and in tape order. Several merged
/// ranges of one mint come back from different chunks, so each mint is sorted.
async fn prints_by_mint(
    repo: &TradeRepo,
    wallet: &str,
    mut windows: Vec<SlotWindow>,
) -> Result<HashMap<String, Vec<TapePrint>>, EntryContextError> {
    // Time order before chunking: a query's `block_time` bounds are the span of its
    // windows, so a chunk of neighbours in time lets the planner skip every other
    // hypertable chunk instead of scanning the whole range's.
    windows.sort_by_key(|x| x.lo_time);
    let mut by_mint: HashMap<String, Vec<TapePrint>> = HashMap::new();
    for chunk in windows.chunks(WINDOWS_PER_QUERY) {
        let prints = repo
            .prints_in_slot_windows(chunk, Some(wallet))
            .await
            .map_err(EntryContextError::Db)?;
        for p in prints {
            by_mint.entry(p.mint_address.clone()).or_default().push(p);
        }
    }
    for prints in by_mint.values_mut() {
        prints.sort_by_key(|p| (p.slot, p.tx_index, p.leg_index));
    }
    Ok(by_mint)
}

/// The whole read: anchors, windows, fold. `repo` must sit on a pool without a
/// short statement ceiling (the lab's batch pool): a range of windows is a long
/// read.
pub async fn read_entry_context(
    repo: &TradeRepo,
    wallet: &str,
    body: &EntryContextBody,
) -> Result<EntryContextResponse, EntryContextError> {
    check_body(body)?;
    let (w, pw) = (body.window_secs, body.probe_slots);
    let (patterns, targeted) = compile_optional(body.tag.as_ref())?;

    let mut anchors = repo
        .wallet_buy_txs(wallet, body.from, body.to, MAX_ENTRIES + 1)
        .await
        .map_err(EntryContextError::Db)?;
    let truncated = anchors.len() as i64 > MAX_ENTRIES;
    anchors.truncate(MAX_ENTRIES as usize);

    let tape_floor = repo.tape_floor().await.unwrap_or_else(|e| {
        tracing::warn!("entry context: tape floor unreadable: {e}");
        None
    });
    // The farther of the two reads: the analysis `2W`, or the probe's `2P` slots at
    // a nominal slot time.
    let reach = Duration::milliseconds(
        ((2.0 * w * 1000.0).round() as i64).max(((2 * pw) as f64 * NOMINAL_SLOT_SECS * 1000.0) as i64),
    );
    let covered = |a: &WalletBuyTx| tape_floor.is_none_or(|f| a.block_time - reach >= f);

    let readable: Vec<&WalletBuyTx> = anchors.iter().filter(|a| covered(a)).collect();
    let by_mint = prints_by_mint(repo, wallet, mint_windows(&readable, w, pw)).await?;

    let empty: Vec<TapePrint> = Vec::new();
    let mut entries: Vec<EntryRow> = anchors
        .iter()
        .map(|a| {
            if !covered(a) {
                return EntryRow {
                    mint_address: a.mint_address.clone(),
                    slot: a.slot,
                    tx_index: a.tx_index,
                    at: a.block_time,
                    sol: lamports_to_sol(a.amount_lamports),
                    unknown_reason: Some(UnknownReason::TapeTruncated),
                    window: WindowRead::default(),
                    control: WindowRead::default(),
                    groups: Vec::new(),
                    groups_omitted: 0,
                    probe: ProbeRead::default(),
                    at_signal: None,
                    holders: None,
                };
            }
            let prints = by_mint.get(&a.mint_address).unwrap_or(&empty);
            let mut e = fold_entry(a, prints, &patterns, w, pw);
            if targeted {
                e.at_signal = at_signal(&e, prints, &patterns, w, pw);
            } else {
                untarget(&mut e);
            }
            e
        })
        .collect();
    fill_entry_holders(repo, wallet, &mut entries, tape_floor).await?;

    Ok(EntryContextResponse {
        entries,
        truncated,
        max_entries: MAX_ENTRIES,
        window_secs: w,
        probe_slots: pw,
        tape_floor,
    })
}

// ── Scan ────────────────────────────────────────────────────────────────────

/// Horizons the price change after a point is read at, seconds.
const AFTER_SECS: [i64; 2] = [30, 120];
/// The scan's read, as [`scan_key`] names it. Changes with what a [`ScanMoment`] holds.
const SCAN_READ: &str = "follower-seat-holders";
/// Points per scan. Past it the scan stops (most recent first) and says so.
const MAX_SCAN_MOMENTS: usize = 100_000;

/// `POST /api/wallets/{wallet}/entry-context/scan` body: the buys read's body.
/// The scan anchors on each buy of the target, so it has no step of its own.
#[derive(Debug, Deserialize)]
pub struct EntryScanBody {
    #[serde(flatten)]
    pub base: EntryContextBody,
    /// `true` reads the market again and replaces the stored result of this request.
    #[serde(default)]
    pub refresh: bool,
}

/// One target buy on one mint: read as a buy landing right behind it is (`at` is
/// that buy, `sol` 0, the window is the `W` seconds up to and with it, the breakdown
/// cut to its top row), plus what came after.
#[derive(Debug, Serialize)]
pub struct ScanMoment {
    #[serde(flatten)]
    pub read: EntryRow,
    /// The last trade's price at the moment, SOL per raw token ([`TapePrint::price`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
    /// Price change from `price` to the last trade price [`AFTER_SECS`] later, percent.
    /// No trade in between = no move = 0. `None` with no price at the moment.
    pub ret_pct: [Option<f64>; 2],
    /// Seconds from the moment to his next buy on the mint; `None` when he bought no more.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_buy_secs: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct EntryScanResponse {
    pub moments: Vec<ScanMoment>,
    /// `true` when [`MAX_SCAN_MOMENTS`] cut the scan short.
    pub truncated: bool,
    /// Mints checked.
    pub mints: u32,
    pub window_secs: f64,
    pub probe_slots: i64,
    pub after_secs: [i64; 2],
    /// When the market was read. A stored result keeps the time of its read.
    pub scanned_at: DateTime<Utc>,
}

/// The last trade price at or before `t`, his trades left out.
fn price_at(prints: &[TapePrint], t: DateTime<Utc>) -> Option<f64> {
    prints[..prints.partition_point(|p| p.block_time <= t)].iter().rev().find_map(TapePrint::price)
}

/// Buys in `from..=to` whose own transaction matches the target. Sticky is off:
/// a later buy by a wallet that already traded the target is not itself the print.
fn each_target_buy<'a>(
    prints: &'a [TapePrint],
    patterns: &TagPatterns,
    from: DateTime<Utc>,
    to: Option<DateTime<Utc>>,
) -> Vec<&'a TapePrint> {
    let mut gate = patterns.clone();
    gate.sticky = false;
    let mut st = TagState::new(gate);
    let mut out = Vec::new();
    for p in prints {
        if p.block_time < from || to.is_some_and(|t| p.block_time > t) || !p.is_buy || p.leg_index != 0 {
            continue;
        }
        let t = trade_lite(p);
        if st.on_trade(&t, Cursor { slot: t.slot, print: 0 }) {
            out.push(p);
        }
    }
    out
}

/// One target buy, read from the seat right behind it ([`seat_behind`]). The row
/// keeps the buy's own position. Priced at the buy, then after.
#[allow(clippy::too_many_arguments)]
fn point_of(
    mint: &str,
    trigger: &TapePrint,
    prints: &[TapePrint],
    buys: &[&WalletBuyTx],
    patterns: &TagPatterns,
    w: f64,
    pw: i64,
    reach: Duration,
) -> ScanMoment {
    let t = trigger.block_time;
    let end = prints.partition_point(|p| (p.slot, p.tx_index) <= (trigger.slot, trigger.tx_index));
    let start = prints.partition_point(|p| p.block_time < t - reach).min(end);
    let seat = (trigger.slot, trigger.tx_index, t);
    let mut read = seat_behind(mint, seat, &prints[start..end], patterns, w, pw);
    read.tx_index = trigger.tx_index;
    // Only what a logic reads: the top row (Top structure) and the signal's row, no
    // label lists.
    let mut i = 0;
    read.groups.retain(|g| {
        i += 1;
        i == 1 || g.signal
    });
    read.groups_omitted = 0;
    if let Some(n) = read.probe.nearest.as_mut() {
        n.labels.clear();
    }
    let price = price_at(prints, t);
    let ret_pct = AFTER_SECS.map(|h| {
        let (p0, p1) = (price?, price_at(prints, t + Duration::seconds(h))?);
        (p0 > 0.0).then(|| (p1 / p0 - 1.0) * 100.0)
    });
    let next_buy_secs = buys
        .iter()
        .find(|b| b.block_time >= t)
        .map(|b| (b.block_time - t).num_milliseconds() as f64 / 1000.0);
    ScanMoment { read, price, ret_pct, next_buy_secs }
}

/// One mint to check: moments over `lo..=hi`, his buys on it in time order (for
/// `next_buy_secs`; none on a mint he never bought), and two known tape positions
/// `(slot, time)` its print window is sized from.
struct ScanMint<'a> {
    mint: String,
    lo: DateTime<Utc>,
    hi: DateTime<Utc>,
    lo_at: (i64, DateTime<Utc>),
    hi_at: (i64, DateTime<Utc>),
    buys: Vec<&'a WalletBuyTx>,
}

impl<'a> ScanMint<'a> {
    /// From the mint's first trade in the range to its last.
    fn of(s: &MintSpan, buys: Vec<&'a WalletBuyTx>) -> Self {
        Self {
            mint: s.mint_address.clone(),
            lo: s.first_time,
            hi: s.last_time,
            lo_at: (s.first_slot, s.first_time),
            hi_at: (s.last_slot, s.last_time),
            buys,
        }
    }

    /// The tape range its moments read: the reads' `reach` before `lo`, the price
    /// horizon `after` past `hi`.
    fn window(&self, reach: Duration, after: Duration) -> SlotWindow {
        let slots = |d: Duration| (d.num_milliseconds().max(0) as f64 / 1000.0 / MIN_SLOT_SECS).ceil() as i64 + 1;
        SlotWindow {
            mint_address: self.mint.clone(),
            lo_slot: self.lo_at.0 - slots(self.lo_at.1 - (self.lo - reach)),
            hi_slot: self.hi_at.0 + slots(self.hi + after - self.hi_at.1),
            lo_time: self.lo - reach - Duration::seconds(TIME_SLACK_SECS),
            hi_time: self.hi + after + Duration::seconds(TIME_SLACK_SECS),
        }
    }
}

/// Every mint traded in the range (the market). A point is a buy of the target;
/// the page keeps the ones whose window passes its filters. Nothing here knows
/// those filters.
pub async fn read_entry_scan(
    repo: &TradeRepo,
    wallet: &str,
    body: &EntryScanBody,
) -> Result<EntryScanResponse, EntryContextError> {
    let b = &body.base;
    check_body(b)?;
    let (w, pw) = (b.window_secs, b.probe_slots);
    let (patterns, targeted) = compile_optional(b.tag.as_ref())?;
    if !targeted {
        return Err(EntryContextError::Bad(
            "pick a pattern set: the scan anchors on its buys".into(),
        ));
    }

    let buys = repo.wallet_buy_txs(wallet, b.from, b.to, MAX_ENTRIES).await.map_err(EntryContextError::Db)?;
    // His buys per mint in time order.
    let mut per_mint: HashMap<&str, Vec<&WalletBuyTx>> = HashMap::new();
    for x in &buys {
        per_mint.entry(x.mint_address.as_str()).or_default().push(x);
    }
    for v in per_mint.values_mut() {
        v.sort_by_key(|x| (x.slot, x.tx_index));
    }
    let mut mints: Vec<ScanMint> = repo
        .traded_mint_spans(b.from, b.to)
        .await
        .map_err(EntryContextError::Db)?
        .iter()
        .map(|s| ScanMint::of(s, per_mint.remove(s.mint_address.as_str()).unwrap_or_default()))
        .collect();
    // Most recent first, so the moment cap keeps the recent ones.
    mints.sort_by_key(|m| std::cmp::Reverse(m.hi));

    let reach = Duration::milliseconds(((2.0 * w * 1000.0).round() as i64).max(2 * pw * MAX_SLOT_MS));
    let after = Duration::seconds(AFTER_SECS[1]);
    let windows: Vec<SlotWindow> = mints.iter().map(|m| m.window(reach, after)).collect();
    let by_mint = prints_by_mint(repo, wallet, windows).await?;
    let tape_floor = repo.tape_floor().await.unwrap_or_else(|e| {
        tracing::warn!("entry context scan: tape floor unreadable: {e}");
        None
    });

    let empty: Vec<TapePrint> = Vec::new();
    let mut moments: Vec<ScanMoment> = Vec::new();
    let mut truncated = false;
    'mints: for sm in &mints {
        let prints = by_mint.get(&sm.mint).unwrap_or(&empty);
        // Recent points first, so the cap keeps the latest buys.
        for trigger in each_target_buy(prints, &patterns, b.from, b.to).into_iter().rev() {
            if moments.len() >= MAX_SCAN_MOMENTS {
                truncated = true;
                break 'mints;
            }
            let mut m = point_of(&sm.mint, trigger, prints, &sm.buys, &patterns, w, pw, reach);
            if tape_floor.is_some_and(|f| trigger.block_time - reach < f) {
                m.read.unknown_reason = Some(UnknownReason::TapeTruncated);
            }
            moments.push(m);
        }
    }
    // The seat right behind each target buy, as its window reads.
    let asks: Vec<HolderAsk> = moments
        .iter()
        .map(|m| HolderAsk { mint: m.read.mint_address.clone(), seat: (m.read.slot, m.read.tx_index + 1), at: m.read.at })
        .collect();
    let reads = holders_at(repo, wallet, &asks, tape_floor).await.map_err(EntryContextError::Db)?;
    for m in &mut moments {
        m.read.holders = reads.get(&(m.read.mint_address.clone(), (m.read.slot, m.read.tx_index + 1))).copied();
    }

    Ok(EntryScanResponse {
        moments,
        truncated,
        mints: mints.len() as u32,
        window_secs: w,
        probe_slots: pw,
        after_secs: AFTER_SECS,
        scanned_at: Utc::now(),
    })
}

// ── Handler ─────────────────────────────────────────────────────────────────

/// `POST /api/wallets/{wallet}/entry-context`.
pub async fn entry_context(
    state: web::Data<Arc<CoreState>>,
    path: web::Path<String>,
    body: web::Json<EntryContextBody>,
) -> impl Responder {
    let wallet = path.into_inner();
    // The batch pool: the API pool's 8 s statement ceiling cancels this read on a
    // busy wallet.
    let repo = TradeRepo::new(state.batch_db.clone());
    match read_entry_context(&repo, &wallet, &body).await {
        Ok(r) => HttpResponse::Ok().json(r),
        Err(EntryContextError::Bad(msg)) => {
            HttpResponse::BadRequest().json(serde_json::json!({ "error": msg }))
        }
        Err(EntryContextError::Db(e)) => {
            tracing::error!("entry context: read failed for {wallet}: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": "database error" }))
        }
    }
}

/// `POST /api/wallets/{wallet}/entry-context/range`.
pub async fn entry_context_range(
    state: web::Data<Arc<CoreState>>,
    path: web::Path<String>,
    body: web::Json<EntryRangeBody>,
) -> impl Responder {
    let wallet = path.into_inner();
    let repo = TradeRepo::new(state.batch_db.clone());
    match read_entry_range(&repo, &wallet, &body).await {
        Ok(r) => HttpResponse::Ok().json(r),
        Err(EntryContextError::Bad(msg)) => {
            HttpResponse::BadRequest().json(serde_json::json!({ "error": msg }))
        }
        Err(EntryContextError::Db(e)) => {
            tracing::error!("entry context range: read failed for {wallet}: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": "database error" }))
        }
    }
}

/// What a scan result is stored under: the wallet and every field of the body
/// that changes the read. An open `to` stays open, so the stored result is the
/// range as it stood at the scan. [`SCAN_READ`] names the read itself: a result
/// stored under another read is never served.
fn scan_key(wallet: &str, b: &EntryContextBody) -> String {
    format!(
        "{SCAN_READ}|{wallet}|{}|{}|{}|{}|{}",
        b.from.to_rfc3339(),
        b.to.map(|t| t.to_rfc3339()).unwrap_or_default(),
        b.window_secs,
        b.probe_slots,
        b.tag.as_ref().map(|t| t.to_string()).unwrap_or_default(),
    )
}

/// `POST /api/wallets/{wallet}/entry-context/scan`. A request scanned before is
/// answered from its stored result ([`entry_scan_cache`]); `refresh` reads again.
pub async fn entry_context_scan(
    state: web::Data<Arc<CoreState>>,
    path: web::Path<String>,
    body: web::Json<EntryScanBody>,
) -> impl Responder {
    let wallet = path.into_inner();
    let key = scan_key(&wallet, &body.base);
    let dir = lake::entry_scan_dir(&lake::lake_root());
    if !body.refresh {
        if let Some(json) = entry_scan_cache::load(&dir, &key).await {
            return HttpResponse::Ok().content_type("application/json").body(json);
        }
    }
    let repo = TradeRepo::new(state.batch_db.clone());
    match read_entry_scan(&repo, &wallet, &body).await.map(|r| serde_json::to_vec(&r)) {
        Ok(Ok(json)) => {
            if let Err(e) = entry_scan_cache::store(&dir, &key, &json).await {
                tracing::warn!("entry context scan: result not stored for {wallet}: {e}");
            }
            HttpResponse::Ok().content_type("application/json").body(json)
        }
        Ok(Err(e)) => {
            tracing::error!("entry context scan: response not serialized for {wallet}: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": "serialize error" }))
        }
        Err(EntryContextError::Bad(msg)) => {
            HttpResponse::BadRequest().json(serde_json::json!({ "error": msg }))
        }
        Err(EntryContextError::Db(e)) => {
            tracing::error!("entry context scan: read failed for {wallet}: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": "database error" }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const SIX: &str = "Unknown (6Vo3245e): Buy";

    fn at(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_800_000_000 + secs, 0).unwrap()
    }

    fn print(slot: i64, tx: i32, secs: i64, label: &str, is_buy: bool, sol: f64, wallet: &str) -> TapePrint {
        TapePrint {
            mint_address: "M".into(),
            slot,
            tx_index: tx,
            leg_index: 0,
            block_time: at(secs),
            wallet_address: wallet.into(),
            is_buy,
            amount_lamports: (sol * 1e9) as i64,
            token_amount: (sol * 1e9) as i64,
            ix_labels: Some(serde_json::json!(["Compute Budget: SetComputeUnitLimit", label])),
            cu_limit: None,
            cu_price: None,
            tip_lamports: None,
        }
    }

    fn anchor(slot: i64, tx: i32, secs: i64) -> WalletBuyTx {
        WalletBuyTx { mint_address: "M".into(), slot, tx_index: tx, block_time: at(secs), amount_lamports: 1_000_000_000 }
    }

    fn six_tag() -> TagPatterns {
        compile_target(&serde_json::json!({ "match": { "program": ["Unknown (6Vo3245e)"] } })).unwrap()
    }

    /// The finding the page exists for: 6Vo buys dominate the window by count and SOL.
    #[test]
    fn share_is_tagged_buys_over_every_buy_in_the_window() {
        let prints = vec![
            print(100, 0, 75, SIX, true, 0.4, "a"),
            print(101, 0, 80, SIX, true, 0.4, "b"),
            print(102, 0, 85, SIX, true, 0.4, "c"),
            print(103, 0, 90, "Pump.Fun: Buy", true, 0.3, "d"),
            // A sell is on neither side of a BUY share.
            print(104, 0, 92, SIX, false, 5.0, "a"),
        ];
        let e = fold_entry(&anchor(110, 0, 100), &prints, &six_tag(), 30.0, 25);
        assert_eq!((e.window.tag_buy_tx, e.window.buy_tx), (3, 4));
        assert_eq!(e.window.tx_share_pct, Some(75.0));
        let sol = e.window.sol_share_pct.unwrap();
        assert!((sol - 100.0 * 1.2 / 1.5).abs() < 1e-9, "{sol}");
        assert_eq!(e.window.tag_sell_tx, 1);
        // Breakdown agrees with the headline: 6Vo is the top row with every tagged buy.
        assert_eq!(e.groups[0].key, format!("Compute Budget: SetComputeUnitLimit > {SIX}"));
        assert_eq!((e.groups[0].buy_tx, e.groups[0].tag_buy_tx), (3, 3));
        assert_eq!(e.groups[0].buy_tx_share_pct, Some(75.0));
        // The probe (25 slots) sees every tagged transaction, the sell included (the
        // tag has no side); the nearest is that sell, 6 slots and 8 s back.
        assert_eq!(e.probe.hits, 4);
        let n = e.probe.nearest.as_ref().unwrap();
        assert_eq!((n.lag_slots, n.lag_tx, n.lag_secs), (6, None, 8.0));
        // Matched reads the template grain, whatever the breakdown vocabulary.
        assert_eq!(n.key, "Unknown (6Vo3245e)|CU");
        assert_eq!(n.labels, vec!["Compute Budget: SetComputeUnitLimit".to_string(), SIX.to_string()]);
        let total: u32 = e.groups.iter().map(|g| g.buy_tx).sum();
        assert_eq!(total, e.window.buy_tx);
    }

    /// The signal row is the breakdown group of the probe's nearest tagged print, and
    /// it survives the group cap.
    #[test]
    fn the_signal_row_is_the_nearest_tagged_prints_group() {
        let six_b = "Unknown (6Vo3245e): BuyExactIn";
        let mut prints: Vec<TapePrint> = (0..MAX_GROUPS as i32 + 5)
            .map(|i| print(80, i, 80, &format!("Other{i}: Buy"), true, 1.0, "x"))
            .collect();
        prints.push(print(85, 0, 85, SIX, true, 0.1, "a"));
        prints.push(print(89, 0, 99, six_b, true, 0.1, "b"));
        let e = fold_entry(&anchor(90, 5, 100), &prints, &six_tag(), 30.0, 25);
        let signal: Vec<&GroupRow> = e.groups.iter().filter(|g| g.signal).collect();
        assert_eq!(signal.len(), 1);
        assert!(signal[0].key.ends_with(six_b), "{}", signal[0].key);
        assert_eq!(e.groups.len(), MAX_GROUPS);
    }

    /// `[entry - W, entry]` by block time, and in his own slot only the txs ahead of
    /// his; everything one W earlier is the control.
    #[test]
    fn window_bounds_and_the_control_window() {
        let prints = vec![
            print(10, 0, 50, SIX, true, 1.0, "a"),        // 50 s back: control
            print(40, 0, 69, SIX, true, 1.0, "b"),        // 31 s back: control
            print(50, 0, 70, "Pump.Fun: Buy", true, 1.0, "c"), // exactly W back: window
            print(90, 2, 100, SIX, true, 1.0, "d"),       // his slot, ahead of him
            print(90, 7, 100, SIX, true, 1.0, "e"),       // his slot, after him
        ];
        let e = fold_entry(&anchor(90, 5, 100), &prints, &six_tag(), 30.0, 25);
        assert_eq!((e.window.tag_buy_tx, e.window.buy_tx), (1, 2));
        // The nearest target is the one in his own slot, 3 transactions ahead of him.
        let n = e.probe.nearest.as_ref().expect("a tagged print in the probe window");
        assert_eq!((n.lag_slots, n.lag_tx, n.lag_secs), (0, Some(3), 0.0));
        // Both engine spans are closed, so the print exactly W back sits in each.
        assert_eq!((e.control.tag_buy_tx, e.control.buy_tx), (2, 3));
    }

    /// With no target the breakdown is still read and no share is claimed.
    #[test]
    fn no_target_reads_the_breakdown_and_no_share() {
        let prints = vec![print(50, 0, 90, SIX, true, 1.0, "a")];
        let mut e = fold_entry(&anchor(90, 5, 100), &prints, &TagPatterns::default(), 30.0, 25);
        untarget(&mut e);
        assert_eq!((e.window.tag_buy_tx, e.window.buy_tx), (0, 1));
        assert_eq!(e.window.tx_share_pct, None);
        assert_eq!(e.groups.len(), 1);
    }

    #[test]
    fn an_empty_window_has_no_share() {
        let e = fold_entry(&anchor(90, 5, 100), &[], &six_tag(), 30.0, 25);
        assert_eq!(e.window.buy_tx, 0);
        assert_eq!(e.window.tx_share_pct, None);
        assert!(e.groups.is_empty());
        assert!(e.probe.nearest.is_none());
    }

    /// The probe counts in SLOTS, as Trader Analysis' does: `[entry - P, entry)` is
    /// the window, `[entry - 2P, entry - P)` the control, whatever the seconds say.
    #[test]
    fn the_probe_window_is_slots_before_him() {
        let prints = vec![
            print(84, 0, 94, SIX, true, 1.0, "a"), // 6 slots back: outside both
            print(86, 0, 95, SIX, true, 2.0, "b"), // 4 back: control
            print(87, 0, 96, SIX, true, 1.0, "c"), // 3 back: control
            print(88, 0, 97, SIX, true, 0.5, "d"), // 2 back: window
            print(89, 0, 98, "Pump.Fun: Buy", true, 9.0, "e"), // untagged
            print(90, 7, 100, SIX, true, 1.0, "f"), // his slot, after him
        ];
        let e = fold_entry(&anchor(90, 5, 100), &prints, &six_tag(), 30.0, 2);
        assert_eq!((e.probe.hits, e.probe.control_hits), (1, 2));
        assert!((e.probe.sol - 0.5).abs() < 1e-9 && (e.probe.control_sol - 3.0).abs() < 1e-9);
        let n = e.probe.nearest.as_ref().unwrap();
        assert_eq!((n.lag_slots, n.lag_tx), (2, None));
        // The analysis window is unchanged by the probe's: all 5 buys ahead of him
        // sit in the 30 s before his buy.
        assert_eq!(e.window.buy_tx, 5);
    }

    /// A second leg of one transaction adds SOL but not a transaction.
    #[test]
    fn legs_count_once_as_a_transaction() {
        let mut leg = print(50, 1, 90, SIX, true, 0.5, "b");
        leg.leg_index = 1;
        let prints = vec![print(50, 1, 90, SIX, true, 0.5, "a"), leg];
        let e = fold_entry(&anchor(90, 5, 100), &prints, &six_tag(), 30.0, 25);
        assert_eq!(e.window.buy_tx, 1);
        assert!((e.window.buy_sol - 1.0).abs() < 1e-9);
        assert_eq!(e.groups[0].wallets, 2);
    }

    /// Sticky remembers a wallet from its first target trade inside each span only.
    /// Wallet x buys 6Vo 40 s back (control), then pump 35 s back (control) and 10 s
    /// back (window): the control carries both its buys, the window none of them,
    /// since the window's memory starts at the window. Wallet y buys 6Vo then pump
    /// inside the window: both count.
    #[test]
    fn sticky_is_scoped_to_each_window() {
        let tag = compile_target(&serde_json::json!({ "match": { "program": ["Unknown (6Vo3245e)"] }, "sticky": true })).unwrap();
        let prints = vec![
            print(10, 0, 60, SIX, true, 1.0, "x"),
            print(20, 0, 65, "Pump.Fun: Buy", true, 1.0, "x"),
            print(60, 0, 90, "Pump.Fun: Buy", true, 1.0, "x"),
            print(70, 0, 92, SIX, true, 1.0, "y"),
            print(80, 0, 95, "Pump.Fun: Buy", true, 1.0, "y"),
        ];
        let e = fold_entry(&anchor(90, 5, 100), &prints, &tag, 30.0, 25);
        assert_eq!((e.window.tag_buy_tx, e.window.buy_tx), (2, 3));
        assert_eq!((e.control.tag_buy_tx, e.control.buy_tx), (2, 2));
        let pump = e.groups.iter().find(|g| g.key.ends_with("Pump.Fun: Buy")).unwrap();
        assert_eq!((pump.buy_tx, pump.tag_buy_tx), (2, 1));
        // Probe [65, 90) holds y's two buys; x's slot-60 buy sits in the probe
        // control [40, 65), untagged there: its 6Vo buy is before that span.
        assert_eq!((e.probe.hits, e.probe.control_hits), (2, 0));
        // The same tag without sticky: only the 6Vo prints carry it.
        let plain = compile_target(&serde_json::json!({ "match": { "program": ["Unknown (6Vo3245e)"] } })).unwrap();
        let e = fold_entry(&anchor(90, 5, 100), &prints, &plain, 30.0, 25);
        assert_eq!((e.window.tag_buy_tx, e.control.tag_buy_tx), (1, 1));
    }

    /// A picked range reads as an entry window ending at the range's end: every
    /// print through `end_slot` in `[from, to]`, the control the same length before.
    #[test]
    fn a_range_is_an_entry_window_ending_at_its_end() {
        let prints = vec![
            print(40, 0, 60, SIX, true, 1.0, "a"), // control of a 20 s range ending at 100
            print(60, 0, 80, SIX, true, 1.0, "b"), // range start (closed)
            print(70, 3, 95, "Pump.Fun: Buy", true, 1.0, "c"),
            print(75, 0, 100, SIX, true, 1.0, "d"), // end_slot, at `to`
            print(76, 0, 100, SIX, true, 1.0, "e"), // past end_slot
        ];
        let anchor = WalletBuyTx {
            mint_address: "M".into(),
            slot: 75 + 1,
            tx_index: 0,
            block_time: at(100),
            amount_lamports: 0,
        };
        let e = fold_entry(&anchor, &prints, &six_tag(), 20.0, 25);
        assert_eq!((e.window.tag_buy_tx, e.window.buy_tx), (2, 3));
        assert_eq!((e.control.tag_buy_tx, e.control.buy_tx), (2, 2));
    }

    #[test]
    fn history_matchers_are_refused() {
        assert!(compile_target(&serde_json::json!({ "match": { "creator": true } })).is_err());
        let sticky = serde_json::json!({ "match": { "program": ["X"] }, "sticky": true });
        assert!(compile_target(&sticky).is_ok());
        assert!(compile_target(&serde_json::json!({ "match": {} })).is_err());
    }

    #[test]
    fn overlapping_anchor_ranges_merge_per_mint() {
        let a = anchor(1_000, 0, 100);
        let b = anchor(1_050, 0, 120);
        let far = anchor(9_000, 0, 5_000);
        let ws = mint_windows(&[&a, &b, &far], 30.0, 25);
        assert_eq!(ws.len(), 2);
        assert_eq!(ws[0].hi_slot, 1_050);
        assert!(ws[0].lo_slot <= 1_000 - 240);
    }

    /// The range body the Entry Context chart sends (`EntryRangeRequest`).
    #[test]
    fn the_range_body_is_what_the_chart_sends() {
        let body: EntryRangeBody = serde_json::from_value(serde_json::json!({
            "mint": "M",
            "from": "2026-09-20T00:00:00Z",
            "to": "2026-09-20T00:00:45.5Z",
            "end_slot": 123,
            "probe_slots": 5,
            "tag": { "match": { "program": ["X"] }, "sticky": true },
        }))
        .expect("range body");
        assert_eq!((body.end_slot, body.probe_slots), (123, 5));
        assert_eq!((body.to - body.from).num_milliseconds(), 45_500);
    }

    /// The browser's types mirror these keys by hand, so the wire shape is a contract.
    #[test]
    fn the_wire_shape_is_what_the_page_sends_and_reads() {
        let body: EntryContextBody = serde_json::from_value(serde_json::json!({
            "from": "2026-09-20T00:00:00Z",
            "window_secs": 30,
            "tag": { "match": { "program": ["X"] }, "side": "buy" },
        }))
        .expect("body");
        assert!(body.to.is_none());
        assert!(body.tag.is_some());
        let untagged: EntryContextBody =
            serde_json::from_value(serde_json::json!({ "from": "2026-09-20T00:00:00Z" })).expect("no tag");
        assert!(untagged.tag.is_none());
        assert_eq!(untagged.window_secs, 30.0);
        assert_eq!(untagged.probe_slots, 25);
        let e = fold_entry(&anchor(90, 5, 100), &[], &six_tag(), 30.0, 25);
        let v = serde_json::to_value(&e).unwrap();
        for k in ["mint_address", "slot", "tx_index", "at", "sol", "window", "control", "groups", "groups_omitted", "probe"] {
            assert!(v.get(k).is_some(), "{k}");
        }
        for k in ["buy_tx", "tag_buy_tx", "buy_sol", "tag_buy_sol", "tx_share_pct", "sol_share_pct"] {
            assert!(v["window"].get(k).is_some(), "window.{k}");
        }
        for k in ["hits", "sol", "control_hits", "control_sol", "nearest"] {
            assert!(v["probe"].get(k).is_some(), "probe.{k}");
        }
    }

    /// A point is a buy of the target. The window is the seconds up to that buy and
    /// holds it, as the window of a buy following it does; the probe's nearest is the
    /// buy itself. A sell, and a buy of another structure, are not points. Sticky
    /// does not carry the tag onto the next buy.
    #[test]
    fn a_market_point_is_a_target_buy_and_holds_itself() {
        let prints = vec![
            print(10, 0, 70, SIX, true, 1.0, "a"),
            print(20, 0, 90, "Pump.Fun: Buy", true, 1.0, "b"),
            print(25, 0, 95, SIX, false, 9.0, "c"),
            print(30, 0, 100, SIX, true, 10.0, "d"),
            print(31, 0, 100, "Pump.Fun: Buy", true, 3.0, "same"),
        ];
        let patterns = six_tag();
        let hits = each_target_buy(&prints, &patterns, at(0), None);
        assert_eq!(hits.len(), 2);
        let m = point_of("M", hits[1], &prints, &[], &patterns, 30.0, 25, Duration::seconds(60));
        assert_eq!(m.read.at, at(100));
        assert_eq!(m.read.sol, 0.0);
        assert_eq!((m.read.slot, m.read.tx_index), (30, 0));
        assert_eq!((m.read.window.tag_buy_tx, m.read.window.buy_tx), (2, 3));
        // 1 + 10 SOL of the target in 12 SOL of buys.
        let sol = m.read.window.sol_share_pct.unwrap();
        assert!((sol - 1100.0 / 12.0).abs() < 1e-9, "{sol}");
        assert_eq!(m.read.probe.nearest.as_ref().map(|n| (n.lag_slots, n.lag_tx)), Some((0, Some(1))));

        let sticky = compile_target(&serde_json::json!({
            "match": { "program": ["Unknown (6Vo3245e)"] },
            "sticky": true,
        }))
        .unwrap();
        let carried = vec![
            print(10, 0, 70, SIX, true, 1.0, "same"),
            print(20, 0, 80, "Pump.Fun: Buy", true, 1.0, "same"),
        ];
        assert_eq!(each_target_buy(&carried, &sticky, at(0), None).len(), 1);
    }

    /// His entry, read behind its signal, is the scan's read of that target buy: a
    /// print landing between the signal and his buy is in his window and in neither.
    #[test]
    fn his_entry_behind_its_signal_reads_as_the_scan_does() {
        let prints = vec![
            print(10, 0, 70, SIX, true, 1.0, "a"),
            print(30, 0, 100, SIX, true, 2.0, "d"),
            print(31, 0, 100, "Pump.Fun: Buy", true, 5.0, "late"),
        ];
        let patterns = six_tag();
        let his = fold_entry(&anchor(32, 0, 101), &prints, &patterns, 30.0, 25);
        assert_eq!(his.window.buy_tx, 2, "his own window: from 71 s on, with the late print");
        let seat = at_signal(&his, &prints, &patterns, 30.0, 25).unwrap();
        let hits = each_target_buy(&prints, &patterns, at(0), None);
        let m = point_of("M", hits[1], &prints, &[], &patterns, 30.0, 25, Duration::seconds(60));
        assert_eq!(seat.window, m.read.window);
        assert_eq!(seat.control, m.read.control);
        assert_eq!((seat.window.tag_buy_tx, seat.window.buy_tx), (2, 2));
    }

    /// The point is priced at that buy, and the price is read again 30 s and 120 s later.
    #[test]
    fn a_point_is_priced_at_the_buy_and_after() {
        let priced = |slot, secs, tokens| {
            let mut p = print(slot, 0, secs, SIX, true, 1.0, "a");
            p.token_amount = tokens;
            p
        };
        // 1 SOL for 500, 250, 125 tokens: price 0.002, then double, then double again.
        let prints = vec![priced(20, 90, 500), priced(40, 120, 250), priced(90, 210, 125)];
        let his = anchor(30, 0, 100);
        let m = point_of("M", &prints[0], &prints, &[&his], &six_tag(), 30.0, 25, Duration::seconds(60));
        assert!((m.price.unwrap() - 0.002).abs() < 1e-12);
        let [r30, r120] = m.ret_pct;
        assert!((r30.unwrap() - 100.0).abs() < 1e-9, "{r30:?}");
        assert!((r120.unwrap() - 300.0).abs() < 1e-9, "{r120:?}");
        assert_eq!(m.next_buy_secs, Some(10.0));
    }

    #[test]
    fn the_scan_body_is_the_buys_body() {
        let body: EntryScanBody = serde_json::from_value(serde_json::json!({
            "from": "2026-09-20T00:00:00Z",
            "window_secs": 30,
            "probe_slots": 25,
        }))
        .expect("scan body");
        assert_eq!((body.base.window_secs, body.base.probe_slots), (30.0, 25));
    }

    #[test]
    fn a_mint_is_scanned_from_its_first_trade_to_its_last() {
        let span = |last: i64| MintSpan {
            mint_address: "M".into(),
            first_slot: 100,
            first_time: at(1_000),
            last_slot: 100 + last * 2,
            last_time: at(1_000 + last),
        };
        let m = ScanMint::of(&span(600), Vec::new());
        assert_eq!((m.lo, m.hi, m.hi_at), (at(1_000), at(1_600), (1_300, at(1_600))));
        let long = ScanMint::of(&span(20_000), Vec::new());
        assert_eq!((long.hi, long.hi_at), (at(21_000), (40_100, at(21_000))));
        let win = long.window(Duration::seconds(60), Duration::seconds(120));
        assert!(win.lo_slot < 100);
        assert_eq!(win.hi_time, at(21_000 + 120 + TIME_SLACK_SECS));
    }
}
