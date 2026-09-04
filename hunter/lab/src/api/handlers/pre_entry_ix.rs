//! **Pre-entry ix probe** — the Trader Analysis flow lens asked across every
//! token at once: did a structure from this set land on the tape BEFORE the
//! trader entered, and how often does that happen anyway?
//!
//! The overlay reads one token at a time, so it can confirm a story and never
//! test one. This read answers the same question per row, with a control window
//! beside it, so the answer carries its own denominator.
//!
//! Matching runs on the ENGINE's classifiers — [`BuildPatterns`] for an exact
//! set (fee pins and all), [`template_grain`] for a templates set. A second
//! spelling here would let the filter fire on a set the charts classify
//! differently, which is the one thing that would make the two surfaces lie
//! about each other.
//!
//! Nothing here writes: the set is supplied by the caller (the NARROWED lens
//! keys, so chips move the filter and the overlay together) and never re-read
//! from `ix_pattern_sets`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use hunter_engine::metrics::fee::{BuildPatterns, FeeKeys};
use hunter_engine::metrics::flow_ix::ix_hash_from_labels_value;
use hunter_engine::metrics::template_grain::{program_owned, grain};
use hunter_engine::grouping::normalize_labels;
use trading_core::config::constants::lamports_to_sol;
use trading_core::state::core_state::CoreState;
use trading_core::storage::repositories::trade_repo::{SlotWindow, TapePrint};

use crate::storage::repositories::ix_pattern_set_repo::{IxPattern, IxPatternSetKind};

/// Mints probed per SQL round-trip. Each window is one index range on
/// `idx_trades_mint_order`, but the mint list is unbounded when the page's Max
/// tokens is 0 — and an unbounded heavy scan is what OOMs the 6 GB VM. Chunks run
/// one at a time, never concurrently, for the same reason.
const MINTS_PER_QUERY: usize = 300;

/// Hard ceiling on one probe. Beyond it the response reports how many rows it
/// could not reach rather than silently answering for a prefix.
const MAX_ANCHORS: usize = 4_000;

/// Widest window the probe accepts, in slots. ~400ms a slot, so 2,000 slots is
/// ~13 minutes of tape before the entry — past the point where "before he
/// entered" describes a trigger at all.
const MAX_WINDOW_SLOTS: i64 = 2_000;

/// Slack on the `block_time` bounds, which only prune chunks (slot is the real
/// filter). Generous on purpose: slots stall, and a bound that is too tight drops
/// prints the slot range asks for.
const SLOT_TIME_MS: i64 = 600;
const TIME_SLACK_SECS: i64 = 60;

// ── Request ─────────────────────────────────────────────────────────────────

/// One row's anchor: the trader's FIRST buy on the mint, as the token table
/// already carries it.
///
/// The client sends what is on screen rather than the server re-deriving it, so a
/// window or threshold change re-probes without re-reading the per-mint rollups.
/// `entry_slot` / `entry_tx_index` absent ⇒ the window caught no buy leg
/// (exit-only row): there is no anchor, and the verdict is `unknown`.
#[derive(Debug, Clone, Deserialize)]
pub struct EntryAnchor {
    pub mint: String,
    #[serde(default)]
    pub entry_slot: Option<i64>,
    #[serde(default)]
    pub entry_tx_index: Option<i32>,
    /// The entry leg's `block_time`. Only used to bound which chunks the read
    /// touches; ordering is `(slot, tx_index)` throughout.
    #[serde(default)]
    pub entry_at: Option<DateTime<Utc>>,
}

/// `POST /api/wallets/{wallet}/pre-entry-ix` body.
#[derive(Debug, Deserialize)]
pub struct PreEntryProbeBody {
    pub anchors: Vec<EntryAnchor>,
    /// Window width `W`, in SLOTS. The probed range is `[entry - 2W, entry]`:
    /// `[entry - W, entry]` is the window, `[entry - 2W, entry - W)` the control.
    pub window_slots: i64,
    /// Matching prints needed before a token counts as matched (default 1).
    #[serde(default = "one")]
    pub min_hits: u32,
    /// Σ SOL of matching prints needed (default 0 — presence only).
    #[serde(default)]
    pub min_sol: f64,
    /// `"buy"` / `"sell"`, or absent for both legs. Labels carry no direction, so
    /// this filters TRADES, never patterns — the lens' own knob.
    #[serde(default)]
    pub side: Option<String>,
    /// Which vocabulary the keys below are in.
    #[serde(default)]
    pub kind: IxPatternSetKind,
    /// The NARROWED exact rows (`kind = exact`).
    #[serde(default)]
    pub patterns: Vec<IxPattern>,
    /// The NARROWED grain ids / program names (`kind = templates`).
    #[serde(default)]
    pub templates: Vec<String>,
}

fn one() -> u32 {
    1
}

// ── Response ────────────────────────────────────────────────────────────────

/// Why a row could not be answered. Never folded into `no-match`: retention and
/// a missing fee column would then shape the hit rate silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnknownReason {
    /// The look-back window caught no buy leg — there is nothing to sit before.
    NoEntry,
    /// The probe window reaches past the oldest tape `trades` still holds, so an
    /// absent match cannot be told from a dropped chunk.
    TapeTruncated,
    /// The set pins fee fields and no print in the window carries a fee reading
    /// at all — every pinned row fails by absence, not by budget.
    NoFeeReadings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerdictState {
    Matched,
    NoMatch,
    Unknown,
}

/// One row's verdict. `hits`/`sol` describe the window, `control_*` the window
/// before it — the same shape one `W` earlier, which is what turns a presence
/// count into a readable one.
#[derive(Debug, Clone, Serialize)]
pub struct PreEntryVerdict {
    pub mint_address: String,
    pub state: VerdictState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unknown_reason: Option<UnknownReason>,
    /// Matching TRANSACTIONS (legs collapsed on `(slot, tx_index)`), not legs.
    pub hits: u32,
    pub sol: f64,
    /// Nearest match's distance back from the anchor. `0` = the trader's own
    /// slot, where `nearest_lag_tx` is the entire distance — and where a "trigger"
    /// is co-arrival, not something a seat at p50 +1 slot could have read.
    pub nearest_lag_slots: Option<i64>,
    pub nearest_lag_tx: Option<i64>,
    /// Which unit matched nearest: the grain id, or the exact row's group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_unit: Option<String>,
    pub control_hits: u32,
    pub control_sol: f64,
    pub control_matched: bool,
}

/// The probe's answer: one verdict per anchor, plus what the read could not cover.
#[derive(Debug, Serialize)]
pub struct PreEntryProbeResponse {
    pub verdicts: Vec<PreEntryVerdict>,
    /// Anchors dropped by [`MAX_ANCHORS`]. Non-zero means the page is showing an
    /// answer for a prefix of its rows and must say so.
    pub skipped: usize,
    /// The oldest instant `trades` can still answer for, echoed so the page can
    /// explain a `tape-truncated` row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tape_floor: Option<DateTime<Utc>>,
}

// ── Matcher ─────────────────────────────────────────────────────────────────

/// The set compiled once per request, in the vocabulary it was stored in.
enum Matcher {
    /// Exact ordered `ix_labels` + fee pins, plus `ix_hash → group` for the
    /// matched-unit readout. Two rows of one shape under different groups keep the
    /// first group seen — the shape is what matched, and the label is a readout.
    Exact { builds: BuildPatterns, groups: HashMap<u64, String> },
    /// Grain ids and bare program names — the one list, two spellings that
    /// `m_burst_slot` reads (`Axiom Trade|CU|ATA|F` vs `Axiom Trade`).
    Templates { grains: HashSet<String>, programs: HashSet<String> },
}

impl Matcher {
    fn build(body: &PreEntryProbeBody) -> Result<Self, String> {
        match body.kind {
            IxPatternSetKind::Exact => {
                let mut rows = Vec::with_capacity(body.patterns.len());
                let mut groups = HashMap::new();
                for p in &body.patterns {
                    if p.ix_labels.is_empty() {
                        continue;
                    }
                    let mut obj = serde_json::Map::new();
                    obj.insert("labels".into(), serde_json::json!(p.ix_labels));
                    for (k, v) in [
                        ("cu_limit", p.cu_limit),
                        ("cu_price", p.cu_price),
                        ("tip_lamports", p.tip_lamports),
                    ] {
                        if let Some(n) = v {
                            obj.insert(k.into(), serde_json::json!(n));
                        }
                    }
                    let labels: Vec<&str> = p.ix_labels.iter().map(String::as_str).collect();
                    groups
                        .entry(hunter_engine::metrics::flow_ix::ix_hash(&labels))
                        .or_insert_with(|| p.group.clone().unwrap_or_else(|| "ungrouped".into()));
                    rows.push(serde_json::Value::Object(obj));
                }
                let builds = BuildPatterns::parse(&rows)
                    .ok_or_else(|| "patterns are not a valid ix_patterns list".to_string())?;
                if builds.is_empty() {
                    return Err("the set classifies with nothing — no patterns".into());
                }
                Ok(Self::Exact { builds, groups })
            }
            IxPatternSetKind::Templates => {
                let mut grains = HashSet::new();
                let mut programs = HashSet::new();
                for id in &body.templates {
                    let id = id.trim();
                    if id.is_empty() {
                        continue;
                    }
                    // Same split `BurstPatterns` makes: a `|` id is a grain, a
                    // bare name is a program and matches every grain of it.
                    if id.contains('|') {
                        grains.insert(id.to_string());
                    } else {
                        programs.insert(id.to_string());
                    }
                }
                if grains.is_empty() && programs.is_empty() {
                    return Err("the set classifies with nothing — no grain ids".into());
                }
                Ok(Self::Templates { grains, programs })
            }
        }
    }

    /// Whether any row pins a fee field — i.e. whether this set needs the fee
    /// columns to classify the way it is written.
    fn pins_fee(&self) -> bool {
        match self {
            Self::Exact { builds, .. } => builds.pins_fee(),
            Self::Templates { .. } => false,
        }
    }

    /// The unit that matched this print, or `None` for no match.
    fn matched_unit(&self, labels: Option<&serde_json::Value>, fee: FeeKeys) -> Option<String> {
        let labels = labels?;
        match self {
            Self::Exact { builds, groups } => {
                let h = ix_hash_from_labels_value(labels)?;
                builds
                    .matches(Some(h), fee)
                    .then(|| groups.get(&h).cloned().unwrap_or_else(|| "ungrouped".into()))
            }
            Self::Templates { grains, programs } => {
                let normalized = normalize_labels(labels);
                if normalized.is_empty() {
                    return None;
                }
                let g = grain(&normalized);
                if grains.contains(&g) {
                    return Some(g);
                }
                let p = program_owned(&normalized);
                programs.contains(&p).then_some(p)
            }
        }
    }
}

// ── Fold ────────────────────────────────────────────────────────────────────

/// One transaction inside a window: its legs on the asked side, collapsed.
struct PrintTx {
    slot: i64,
    tx_index: i32,
    lamports: i64,
    labels: Option<serde_json::Value>,
    fee: FeeKeys,
}

/// Collapse legs onto `(slot, tx_index)` — the transaction identity within a
/// mint. Counting legs would score a two-leg buy twice, and `leg_index = 0` would
/// drop the later-leg buys entirely.
///
/// `side` filters legs, so a tx with nothing on the asked side never appears;
/// under no narrowing, a tx's size is every leg it emitted here.
fn collapse(prints: &[TapePrint], side: Option<bool>) -> Vec<PrintTx> {
    let mut out: Vec<PrintTx> = Vec::new();
    for p in prints {
        if let Some(want_buy) = side {
            if p.is_buy != want_buy {
                continue;
            }
        }
        match out.last_mut() {
            // The read arrives ordered by (mint, slot, tx_index, leg_index), so a
            // tx's legs are contiguous and the tail is the only candidate.
            Some(last) if last.slot == p.slot && last.tx_index == p.tx_index => {
                last.lamports += p.amount_lamports;
                if last.labels.is_none() {
                    last.labels = p.ix_labels.clone();
                }
            }
            _ => out.push(PrintTx {
                slot: p.slot,
                tx_index: p.tx_index,
                lamports: p.amount_lamports,
                labels: p.ix_labels.clone(),
                fee: FeeKeys::new(
                    p.cu_limit.map(|v| v as u32),
                    p.cu_price.map(|v| v as u64),
                    p.tip_lamports.map(|v| v as u64),
                ),
            }),
        }
    }
    out
}

/// Running totals for one window.
#[derive(Default)]
struct WindowTally {
    hits: u32,
    lamports: i64,
    /// Nearest match to the anchor (largest tape position below it).
    nearest: Option<(i64, i64, String)>,
}

impl WindowTally {
    fn take(&mut self, tx: &PrintTx, unit: String, anchor_slot: i64, anchor_tx: i32) {
        self.hits += 1;
        self.lamports += tx.lamports;
        let lag_slots = anchor_slot - tx.slot;
        let lag_tx = i64::from(anchor_tx) - i64::from(tx.tx_index);
        // Nearest = smallest lag, ties broken by the intra-slot distance.
        let better = self
            .nearest
            .as_ref()
            .is_none_or(|(s, t, _)| (lag_slots, lag_tx) < (*s, *t));
        if better {
            self.nearest = Some((lag_slots, lag_tx, unit));
        }
    }

    fn clears(&self, min_hits: u32, min_sol: f64) -> bool {
        self.hits >= min_hits.max(1) && lamports_to_sol(self.lamports) >= min_sol
    }
}

// ── Handler ─────────────────────────────────────────────────────────────────

/// `POST /api/wallets/{wallet}/pre-entry-ix`.
///
/// The studied wallet is excluded from the tape it is measured against: his own
/// entry carries his own tool's structure, and a set built from it would match
/// him on every token.
pub async fn probe_pre_entry_ix(
    state: web::Data<Arc<CoreState>>,
    path: web::Path<String>,
    body: web::Json<PreEntryProbeBody>,
) -> impl Responder {
    let wallet = path.into_inner();
    let body = body.into_inner();

    let window = body.window_slots;
    if window <= 0 || window > MAX_WINDOW_SLOTS {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": format!("window_slots must be 1..={MAX_WINDOW_SLOTS}")
        }));
    }
    let matcher = match Matcher::build(&body) {
        Ok(m) => m,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({ "error": e })),
    };
    let side = match body.side.as_deref() {
        None | Some("") => None,
        Some("buy") => Some(true),
        Some("sell") => Some(false),
        Some(other) => {
            return HttpResponse::BadRequest()
                .json(serde_json::json!({ "error": format!("unknown side '{other}'") }))
        }
    };

    let skipped = body.anchors.len().saturating_sub(MAX_ANCHORS);
    let anchors: Vec<EntryAnchor> = body.anchors.into_iter().take(MAX_ANCHORS).collect();

    // A window reaching below the oldest chunk cannot tell an absent match from a
    // dropped one. Read once per request (chunk catalog, not `MIN(block_time)`).
    let tape_floor = match state.trade_repo().tape_floor().await {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("pre-entry probe: tape floor unreadable: {e}");
            None
        }
    };

    let mut verdicts: Vec<PreEntryVerdict> = Vec::with_capacity(anchors.len());
    let mut windows: Vec<SlotWindow> = Vec::new();
    let mut probed: Vec<EntryAnchor> = Vec::new();

    for a in anchors {
        let (Some(entry_slot), Some(entry_tx), Some(entry_at)) =
            (a.entry_slot, a.entry_tx_index, a.entry_at)
        else {
            verdicts.push(unknown(&a.mint, UnknownReason::NoEntry));
            continue;
        };
        let lo_slot = entry_slot - 2 * window;
        let span = Duration::milliseconds(2 * window * SLOT_TIME_MS)
            + Duration::seconds(TIME_SLACK_SECS);
        let lo_time = entry_at - span;
        if tape_floor.is_some_and(|f| lo_time < f) {
            verdicts.push(unknown(&a.mint, UnknownReason::TapeTruncated));
            continue;
        }
        windows.push(SlotWindow {
            mint_address: a.mint.clone(),
            lo_slot,
            hi_slot: entry_slot,
            lo_time,
            hi_time: entry_at + Duration::seconds(TIME_SLACK_SECS),
        });
        probed.push(EntryAnchor {
            mint: a.mint,
            entry_slot: Some(entry_slot),
            entry_tx_index: Some(entry_tx),
            entry_at: Some(entry_at),
        });
    }

    // One chunk at a time — never a join_all. Heavy concurrent scans are what OOM
    // the VM this DB runs in.
    let mut by_mint: HashMap<String, Vec<TapePrint>> = HashMap::new();
    for chunk in windows.chunks(MINTS_PER_QUERY) {
        match state.trade_repo().prints_in_slot_windows(chunk, Some(&wallet)).await {
            Ok(prints) => {
                for p in prints {
                    by_mint.entry(p.mint_address.clone()).or_default().push(p);
                }
            }
            Err(e) => {
                tracing::error!("pre-entry probe: window read failed for {wallet}: {e}");
                return HttpResponse::InternalServerError()
                    .json(serde_json::json!({ "error": "database error" }));
            }
        }
    }

    let pins_fee = matcher.pins_fee();
    for a in probed {
        // Written by the loop above — every probed anchor carries all three.
        let (entry_slot, entry_tx) = (a.entry_slot.unwrap_or(0), a.entry_tx_index.unwrap_or(0));
        let prints = by_mint.remove(&a.mint).unwrap_or_default();
        let txs = collapse(&prints, side);

        // A pinned row can only fail by absence when nothing in the window carries
        // a fee reading at all — the state every print written before the fee
        // cutover is in. Reported as unknown, never as "this structure was absent".
        if pins_fee && !txs.is_empty() && txs.iter().all(|t| t.fee.is_empty()) {
            verdicts.push(unknown(&a.mint, UnknownReason::NoFeeReadings));
            continue;
        }

        let mut win = WindowTally::default();
        let mut ctl = WindowTally::default();
        for tx in &txs {
            // Strictly earlier in the tape: a print in the trader's own slot counts
            // only when its tx landed ahead of his.
            if (tx.slot, tx.tx_index) >= (entry_slot, entry_tx) {
                continue;
            }
            let Some(unit) = matcher.matched_unit(tx.labels.as_ref(), tx.fee) else {
                continue;
            };
            if tx.slot >= entry_slot - window {
                win.take(tx, unit, entry_slot, entry_tx);
            } else {
                ctl.take(tx, unit, entry_slot, entry_tx);
            }
        }

        let matched = win.clears(body.min_hits, body.min_sol);
        // The tx delta is reported only at lag 0, where it IS the whole distance.
        // A slot away it is the difference between two unrelated block positions,
        // and shipping it invites a reader to treat it as a finer lag.
        let (nearest_lag_slots, nearest_lag_tx, matched_unit) = match win.nearest {
            Some((s, t, u)) => (Some(s), (s == 0).then_some(t), Some(u)),
            None => (None, None, None),
        };
        verdicts.push(PreEntryVerdict {
            mint_address: a.mint,
            state: if matched { VerdictState::Matched } else { VerdictState::NoMatch },
            unknown_reason: None,
            hits: win.hits,
            sol: lamports_to_sol(win.lamports),
            nearest_lag_slots,
            nearest_lag_tx,
            matched_unit,
            control_hits: ctl.hits,
            control_sol: lamports_to_sol(ctl.lamports),
            control_matched: ctl.clears(body.min_hits, body.min_sol),
        });
    }

    HttpResponse::Ok().json(PreEntryProbeResponse { verdicts, skipped, tape_floor })
}

fn unknown(mint: &str, reason: UnknownReason) -> PreEntryVerdict {
    PreEntryVerdict {
        mint_address: mint.to_string(),
        state: VerdictState::Unknown,
        unknown_reason: Some(reason),
        hits: 0,
        sol: 0.0,
        nearest_lag_slots: None,
        nearest_lag_tx: None,
        matched_unit: None,
        control_hits: 0,
        control_sol: 0.0,
        control_matched: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn print(slot: i64, tx_index: i32, labels: &[&str], is_buy: bool, lamports: i64) -> TapePrint {
        TapePrint {
            mint_address: "M".into(),
            slot,
            tx_index,
            wallet_address: "W".into(),
            is_buy,
            amount_lamports: lamports,
            ix_labels: Some(serde_json::json!(labels)),
            cu_limit: None,
            cu_price: None,
            tip_lamports: None,
        }
    }

    fn templates_body(ids: &[&str]) -> PreEntryProbeBody {
        PreEntryProbeBody {
            anchors: vec![],
            window_slots: 10,
            min_hits: 1,
            min_sol: 0.0,
            side: None,
            kind: IxPatternSetKind::Templates,
            patterns: vec![],
            templates: ids.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn legs_of_one_tx_collapse_into_one_print() {
        let prints = vec![
            print(10, 3, &["Pump.Fun: Buy"], true, 1_000),
            print(10, 3, &["Pump.Fun: Buy"], true, 2_000),
            print(10, 4, &["Pump.Fun: Buy"], true, 500),
        ];
        let txs = collapse(&prints, None);
        assert_eq!(txs.len(), 2);
        assert_eq!(txs[0].lamports, 3_000);
    }

    #[test]
    fn side_filters_legs_not_patterns() {
        let prints = vec![
            print(10, 3, &["Pump.Fun: Buy"], true, 1_000),
            print(10, 4, &["Pump.Fun: Sell"], false, 9_000),
        ];
        assert_eq!(collapse(&prints, Some(true)).len(), 1);
        assert_eq!(collapse(&prints, Some(false))[0].lamports, 9_000);
    }

    #[test]
    fn a_grain_id_and_its_bare_program_both_match() {
        let labels = serde_json::json!(["Compute Budget: SetComputeUnitLimit", "Axiom Trade: Buy"]);
        let by_grain = Matcher::build(&templates_body(&["Axiom Trade|CU"])).unwrap();
        let by_program = Matcher::build(&templates_body(&["Axiom Trade"])).unwrap();
        assert_eq!(
            by_grain.matched_unit(Some(&labels), FeeKeys::default()),
            Some("Axiom Trade|CU".into())
        );
        assert_eq!(
            by_program.matched_unit(Some(&labels), FeeKeys::default()),
            Some("Axiom Trade".into())
        );
        // A grain id that is not this print's stays off, program name or not.
        let other = Matcher::build(&templates_body(&["Axiom Trade|CU|ATA"])).unwrap();
        assert_eq!(other.matched_unit(Some(&labels), FeeKeys::default()), None);
    }

    #[test]
    fn an_exact_row_reports_its_group_and_pins_are_honored() {
        let mut body = templates_body(&[]);
        body.kind = IxPatternSetKind::Exact;
        body.templates = vec![];
        body.patterns = vec![IxPattern {
            group: Some("Axiom".into()),
            ix_labels: vec!["Compute Budget: SetComputeUnitLimit".into(), "Axiom Trade: Buy".into()],
            cu_limit: Some(300_000),
            cu_price: None,
            tip_lamports: None,
        }];
        let m = Matcher::build(&body).unwrap();
        assert!(m.pins_fee());
        let labels =
            serde_json::json!(["Compute Budget: SetComputeUnitLimit", "Axiom Trade: Buy"]);
        assert_eq!(
            m.matched_unit(Some(&labels), FeeKeys::new(Some(300_000), None, None)),
            Some("Axiom".into())
        );
        // Wrong budget, and a print with no fee reading at all: a pinned row
        // matches neither.
        assert_eq!(m.matched_unit(Some(&labels), FeeKeys::new(Some(200_000), None, None)), None);
        assert_eq!(m.matched_unit(Some(&labels), FeeKeys::default()), None);
    }

    /// The browser's types are a hand-written mirror of these, so the wire keys
    /// are the contract: a rename on either side has to fail here rather than
    /// silently read as an all-blank column.
    #[test]
    fn the_wire_shape_is_what_the_page_sends_and_reads() {
        let body: PreEntryProbeBody = serde_json::from_value(serde_json::json!({
            "anchors": [
                { "mint": "M1", "entry_slot": 100, "entry_tx_index": 7, "entry_at": "2026-09-04T10:00:00Z" },
                { "mint": "M2", "entry_slot": null, "entry_tx_index": null, "entry_at": null },
            ],
            "window_slots": 25,
            "min_hits": 2,
            "min_sol": 0.5,
            "side": "buy",
            "kind": "templates",
            "templates": ["Axiom Trade|CU|ATA|F"],
        }))
        .expect("body");
        assert_eq!(body.anchors.len(), 2);
        assert!(body.anchors[1].entry_slot.is_none());
        assert_eq!(body.kind, IxPatternSetKind::Templates);

        let json = serde_json::to_value(PreEntryProbeResponse {
            verdicts: vec![unknown("M2", UnknownReason::NoEntry)],
            skipped: 0,
            tape_floor: None,
        })
        .expect("response");
        let v = &json["verdicts"][0];
        assert_eq!(v["state"], "unknown");
        assert_eq!(v["unknown_reason"], "no-entry");
        // A verdict always carries its counts, so the columns never read `-` for a
        // real zero.
        assert_eq!(v["hits"], 0);
        assert_eq!(v["control_hits"], 0);
        assert!(json["tape_floor"].is_null() || json.get("tape_floor").is_none());
    }

    #[test]
    fn a_same_slot_print_is_nearer_than_one_a_slot_back() {
        let mut t = WindowTally::default();
        // 3 transactions ahead of him in his own slot beats 1 whole slot earlier.
        t.take(&PrintTx { slot: 99, tx_index: 2, lamports: 10, labels: None, fee: FeeKeys::default() }, "a".into(), 100, 5);
        t.take(&PrintTx { slot: 100, tx_index: 2, lamports: 10, labels: None, fee: FeeKeys::default() }, "b".into(), 100, 5);
        assert_eq!(t.nearest.as_ref().map(|n| (n.0, n.1)), Some((0, 3)));
    }

    #[test]
    fn nearest_keeps_the_smallest_lag() {
        let mut t = WindowTally::default();
        t.take(&PrintTx { slot: 90, tx_index: 1, lamports: 10, labels: None, fee: FeeKeys::default() }, "a".into(), 100, 5);
        t.take(&PrintTx { slot: 99, tx_index: 2, lamports: 10, labels: None, fee: FeeKeys::default() }, "b".into(), 100, 5);
        assert_eq!(t.hits, 2);
        assert_eq!(t.nearest.as_ref().map(|n| (n.0, n.2.as_str())), Some((1, "b")));
    }

    #[test]
    fn thresholds_read_hits_and_sol_together() {
        let mut t = WindowTally::default();
        t.take(&PrintTx { slot: 99, tx_index: 2, lamports: 500_000_000, labels: None, fee: FeeKeys::default() }, "a".into(), 100, 5);
        assert!(t.clears(1, 0.4));
        // One hit of 0.5 SOL clears neither a two-print floor nor a 1 SOL one.
        assert!(!t.clears(2, 0.0));
        assert!(!t.clears(1, 1.0));
    }
}
