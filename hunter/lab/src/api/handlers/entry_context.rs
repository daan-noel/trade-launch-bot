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
//! The same fold breaks the window down by structure (exact ix shape, template grain
//! or program), classifying each print with the verdict the tag state returned — one
//! copy of the matchers, so a breakdown row can never disagree with the headline.
//!
//! The studied wallet is excluded from the tape in SQL: his own tool's structure
//! would otherwise sit in every window he is measured against.
//!
//! Nothing here writes.

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
    grain, grain_hash_from_labels_value, program_hash_from_labels_value, program_owned,
};
use hunter_engine::metrics::trade_keys::{
    ix_hash_from_labels_value, marker_bits_from_labels_value, wallet_hash,
};
use hunter_engine::metrics::{Cursor, Side, TradeLite, WindowSpec, WindowUnit};
use trading_core::config::constants::lamports_to_sol;
use trading_core::state::core_state::CoreState;
use trading_core::storage::repositories::trade_repo::{SlotWindow, TapePrint, WalletBuyTx};

/// Most anchors one request reads. Past it the response says so (`truncated`)
/// rather than answering for a silent prefix.
const MAX_ENTRIES: i64 = 3_000;

/// Widest window accepted, in seconds. The read spans `2W` (window + control).
const MAX_WINDOW_SECS: f64 = 600.0;

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

/// What a breakdown row groups the window's prints by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupBy {
    /// The full ordered `ix_labels` sequence.
    #[default]
    Exact,
    /// The coarse template grain `program|CU|ATA|N|S|F`.
    Template,
    /// The transaction's main program.
    Program,
}

/// `POST /api/wallets/{wallet}/entry-context` body.
#[derive(Debug, Deserialize)]
pub struct EntryContextBody {
    /// Anchors are the wallet's buys with `from <= block_time <= to`.
    pub from: DateTime<Utc>,
    #[serde(default)]
    pub to: Option<DateTime<Utc>>,
    /// Window `W`, seconds. Read as the engine's `[Ws]` span.
    #[serde(default = "default_window_secs")]
    pub window_secs: f64,
    /// ONE tag definition, in the fingerprint `tags` document's shape
    /// (`{"match": {...}, "side": "buy"}`), validated by the engine's own parser.
    pub tag: serde_json::Value,
    #[serde(default)]
    pub group_by: GroupBy,
}

fn default_window_secs() -> f64 {
    30.0
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
    /// The group's identity in the requested vocabulary (labels joined by ` > `
    /// for `exact`, the grain id, or the program name).
    pub key: String,
    /// The ordered labels, `exact` only — what a pattern set stores.
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
}

#[derive(Debug, Serialize)]
pub struct EntryContextResponse {
    pub entries: Vec<EntryRow>,
    /// `true` when the wallet made more buys in the range than [`MAX_ENTRIES`]:
    /// `entries` is then the most recent ones only.
    pub truncated: bool,
    pub max_entries: i64,
    pub window_secs: f64,
    pub group_by: GroupBy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tape_floor: Option<DateTime<Utc>>,
}

// ── Tag ─────────────────────────────────────────────────────────────────────

/// Compile the target tag with the engine's parser, refusing matchers that need the
/// coin's history before the window: this read folds the window alone, so a sticky
/// wallet set, the creator or the creation slot would classify from nothing.
fn compile_target(def: &serde_json::Value) -> Result<TagPatterns, String> {
    let doc = serde_json::json!({ "target": def });
    validate_tags(&doc)?;
    let history = ["creator", "creation_slot"];
    if def.get("sticky").and_then(serde_json::Value::as_bool) == Some(true) {
        return Err("sticky needs the coin's history before the window; turn it off here".into());
    }
    if let Some(m) = def.get("match").and_then(serde_json::Value::as_object) {
        if let Some(k) = history.iter().find(|k| m.contains_key(**k)) {
            return Err(format!("`{k}` needs the coin's history before the window; this read folds the window only"));
        }
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

/// The group a print belongs to, and its labels when the vocabulary is `exact`.
fn group_key(p: &TapePrint, by: GroupBy) -> (String, Option<Vec<String>>) {
    let labels = p.ix_labels.as_ref().map(normalize_labels).unwrap_or_default();
    if labels.is_empty() {
        return ("(no labels)".into(), None);
    }
    match by {
        GroupBy::Exact => (labels.join(" > "), Some(labels)),
        GroupBy::Template => (grain(&labels), None),
        GroupBy::Program => (program_owned(&labels), None),
    }
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

/// Fold one anchor's tape: `prints` is the mint's read in tape order, the trader
/// already excluded. Returns the entry with its window, control and breakdown.
fn fold_entry(
    anchor: &WalletBuyTx,
    prints: &[TapePrint],
    patterns: &TagPatterns,
    window_secs: f64,
    by: GroupBy,
) -> EntryRow {
    let win = WindowSpec::secs(window_secs);
    let ctl = WindowSpec { size: window_secs, lag: window_secs, unit: WindowUnit::Sec };
    let entry_ms = anchor.block_time.timestamp_millis();
    let w_ms = (window_secs * 1000.0).round() as i64;
    // The spans' own closed bounds: `[now - W, now]` and `[now - 2W, now - W]`.
    let (win_lo, ctl_lo) = (entry_ms - w_ms, entry_ms - 2 * w_ms);

    let mut st = TagState::new(patterns.clone());
    st.ensure_window(win);
    st.ensure_window(ctl);
    let mut groups: HashMap<String, GroupAcc> = HashMap::new();
    let mut window_has_fee = false;
    let mut window_prints = 0usize;

    for p in prints {
        // Tape order: everything from his transaction on is after the decision.
        if (p.slot, p.tx_index) >= (anchor.slot, anchor.tx_index) {
            break;
        }
        let at_ms = p.block_time.timestamp_millis();
        if at_ms < ctl_lo {
            continue;
        }
        let t = trade_lite(p);
        let tagged = st.on_trade(&t, Cursor { slot: t.slot, print: 0 });
        if at_ms < win_lo {
            continue;
        }
        window_prints += 1;
        window_has_fee |= !t.fee.is_empty();
        let (key, labels) = group_key(p, by);
        let g = groups.entry(key).or_default();
        if g.labels.is_none() {
            g.labels = labels;
        }
        g.wallets.insert(t.wallet_hash);
        let first_leg = t.leg_index == 0;
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

    let window = read_window(&st, win, anchor.block_time);
    let control = read_window(&st, ctl, anchor.block_time);
    let unknown_reason = (patterns.builds.pins_fee() && window_prints > 0 && !window_has_fee)
        .then_some(UnknownReason::NoFeeReadings);

    let mut rows: Vec<GroupRow> = groups
        .into_iter()
        .map(|(key, g)| GroupRow {
            key,
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
        })
        .collect();
    rows.sort_by(|a, b| {
        b.buy_tx
            .cmp(&a.buy_tx)
            .then(b.buy_sol.total_cmp(&a.buy_sol))
            .then_with(|| a.key.cmp(&b.key))
    });
    let groups_omitted = rows.len().saturating_sub(MAX_GROUPS) as u32;
    rows.truncate(MAX_GROUPS);

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
    }
}

/// One slot range per mint covering every anchor's `2W` read, overlapping ranges
/// merged so a print is fetched once however many anchors share it.
fn mint_windows(anchors: &[&WalletBuyTx], window_secs: f64) -> Vec<SlotWindow> {
    let back_slots = (2.0 * window_secs / MIN_SLOT_SECS).ceil() as i64 + 1;
    let back = Duration::milliseconds((2.0 * window_secs * 1000.0).round() as i64)
        + Duration::seconds(TIME_SLACK_SECS);
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

// ── Handler ─────────────────────────────────────────────────────────────────

fn bad(msg: impl Into<String>) -> HttpResponse {
    HttpResponse::BadRequest().json(serde_json::json!({ "error": msg.into() }))
}

/// `POST /api/wallets/{wallet}/entry-context`.
pub async fn entry_context(
    state: web::Data<Arc<CoreState>>,
    path: web::Path<String>,
    body: web::Json<EntryContextBody>,
) -> impl Responder {
    let wallet = path.into_inner();
    let body = body.into_inner();
    let w = body.window_secs;
    if !w.is_finite() || w <= 0.0 || w > MAX_WINDOW_SECS {
        return bad(format!("window_secs must be in (0, {MAX_WINDOW_SECS}]"));
    }
    if body.to.is_some_and(|to| to < body.from) {
        return bad("`to` is before `from`");
    }
    let patterns = match compile_target(&body.tag) {
        Ok(p) => p,
        Err(e) => return bad(e),
    };

    let repo = state.trade_repo();
    let mut anchors = match repo.wallet_buy_txs(&wallet, body.from, body.to, MAX_ENTRIES + 1).await {
        Ok(a) => a,
        Err(e) => {
            tracing::error!("entry context: buy read failed for {wallet}: {e}");
            return HttpResponse::InternalServerError().json(serde_json::json!({ "error": "database error" }));
        }
    };
    let truncated = anchors.len() as i64 > MAX_ENTRIES;
    anchors.truncate(MAX_ENTRIES as usize);

    let tape_floor = match repo.tape_floor().await {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("entry context: tape floor unreadable: {e}");
            None
        }
    };
    let reach = Duration::milliseconds((2.0 * w * 1000.0).round() as i64);
    let covered = |a: &WalletBuyTx| tape_floor.is_none_or(|f| a.block_time - reach >= f);

    let readable: Vec<&WalletBuyTx> = anchors.iter().filter(|a| covered(a)).collect();
    let mut by_mint: HashMap<String, Vec<TapePrint>> = HashMap::new();
    for chunk in mint_windows(&readable, w).chunks(WINDOWS_PER_QUERY) {
        match repo.prints_in_slot_windows(chunk, Some(&wallet)).await {
            Ok(prints) => {
                for p in prints {
                    by_mint.entry(p.mint_address.clone()).or_default().push(p);
                }
            }
            Err(e) => {
                tracing::error!("entry context: window read failed for {wallet}: {e}");
                return HttpResponse::InternalServerError().json(serde_json::json!({ "error": "database error" }));
            }
        }
    }
    // Several merged ranges of one mint arrive in slot order already; a sort keeps
    // the fold's `break` correct whatever order the chunks came back in.
    for prints in by_mint.values_mut() {
        prints.sort_by_key(|p| (p.slot, p.tx_index, p.leg_index));
    }

    let empty: Vec<TapePrint> = Vec::new();
    let entries: Vec<EntryRow> = anchors
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
                };
            }
            let prints = by_mint.get(&a.mint_address).unwrap_or(&empty);
            fold_entry(a, prints, &patterns, w, body.group_by)
        })
        .collect();

    HttpResponse::Ok().json(EntryContextResponse {
        entries,
        truncated,
        max_entries: MAX_ENTRIES,
        window_secs: w,
        group_by: body.group_by,
        tape_floor,
    })
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
        let e = fold_entry(&anchor(110, 0, 100), &prints, &six_tag(), 30.0, GroupBy::Program);
        assert_eq!((e.window.tag_buy_tx, e.window.buy_tx), (3, 4));
        assert_eq!(e.window.tx_share_pct, Some(75.0));
        let sol = e.window.sol_share_pct.unwrap();
        assert!((sol - 100.0 * 1.2 / 1.5).abs() < 1e-9, "{sol}");
        assert_eq!(e.window.tag_sell_tx, 1);
        // Breakdown agrees with the headline: 6Vo is the top row with every tagged buy.
        assert_eq!(e.groups[0].key, "Unknown (6Vo3245e)");
        assert_eq!((e.groups[0].buy_tx, e.groups[0].tag_buy_tx), (3, 3));
        assert_eq!(e.groups[0].buy_tx_share_pct, Some(75.0));
        let total: u32 = e.groups.iter().map(|g| g.buy_tx).sum();
        assert_eq!(total, e.window.buy_tx);
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
        let e = fold_entry(&anchor(90, 5, 100), &prints, &six_tag(), 30.0, GroupBy::Exact);
        assert_eq!((e.window.tag_buy_tx, e.window.buy_tx), (1, 2));
        // Both engine spans are closed, so the print exactly W back sits in each.
        assert_eq!((e.control.tag_buy_tx, e.control.buy_tx), (2, 3));
    }

    #[test]
    fn an_empty_window_has_no_share() {
        let e = fold_entry(&anchor(90, 5, 100), &[], &six_tag(), 30.0, GroupBy::Exact);
        assert_eq!(e.window.buy_tx, 0);
        assert_eq!(e.window.tx_share_pct, None);
        assert!(e.groups.is_empty());
    }

    /// A second leg of one transaction adds SOL but not a transaction.
    #[test]
    fn legs_count_once_as_a_transaction() {
        let mut leg = print(50, 1, 90, SIX, true, 0.5, "b");
        leg.leg_index = 1;
        let prints = vec![print(50, 1, 90, SIX, true, 0.5, "a"), leg];
        let e = fold_entry(&anchor(90, 5, 100), &prints, &six_tag(), 30.0, GroupBy::Exact);
        assert_eq!(e.window.buy_tx, 1);
        assert!((e.window.buy_sol - 1.0).abs() < 1e-9);
        assert_eq!(e.groups[0].wallets, 2);
    }

    #[test]
    fn history_matchers_are_refused() {
        for def in [
            serde_json::json!({ "match": { "creator": true } }),
            serde_json::json!({ "match": { "program": ["X"] }, "sticky": true }),
        ] {
            assert!(compile_target(&def).is_err(), "{def}");
        }
        assert!(compile_target(&serde_json::json!({ "match": {} })).is_err());
    }

    #[test]
    fn overlapping_anchor_ranges_merge_per_mint() {
        let a = anchor(1_000, 0, 100);
        let b = anchor(1_050, 0, 120);
        let far = anchor(9_000, 0, 5_000);
        let ws = mint_windows(&[&a, &b, &far], 30.0);
        assert_eq!(ws.len(), 2);
        assert_eq!(ws[0].hi_slot, 1_050);
        assert!(ws[0].lo_slot <= 1_000 - 240);
    }

    /// The browser's types mirror these keys by hand, so the wire shape is a contract.
    #[test]
    fn the_wire_shape_is_what_the_page_sends_and_reads() {
        let body: EntryContextBody = serde_json::from_value(serde_json::json!({
            "from": "2026-09-20T00:00:00Z",
            "window_secs": 30,
            "tag": { "match": { "program": ["X"] }, "side": "buy" },
            "group_by": "template",
        }))
        .expect("body");
        assert_eq!(body.group_by, GroupBy::Template);
        assert!(body.to.is_none());
        let e = fold_entry(&anchor(90, 5, 100), &[], &six_tag(), 30.0, GroupBy::Exact);
        let v = serde_json::to_value(&e).unwrap();
        for k in ["mint_address", "slot", "tx_index", "at", "sol", "window", "control", "groups", "groups_omitted"] {
            assert!(v.get(k).is_some(), "{k}");
        }
        for k in ["buy_tx", "tag_buy_tx", "buy_sol", "tag_buy_sol", "tx_share_pct", "sol_share_pct"] {
            assert!(v["window"].get(k).is_some(), "window.{k}");
        }
    }
}
