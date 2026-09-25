//! The per-combo scan over one token's precomputed [`MetricSeries`] — the sweep's
//! stand-in for folding the engine once per combo.
//!
//! **One decision path.** Every decision is the engine's own: the entry is
//! [`CompiledRule::try_enter`] and the held side is [`CompiledRule::held_line`], both
//! run over a series row through [`RowReads`] (the engine's [`CoinReads`] for a row).
//! The scan adds only what a series walk needs around them: the worst-case fill, the
//! running peak and trough, stage moves, partial legs and the money math. The guard
//! (`super::guard`) locks the result to a full `run_replay`.
//!
//! **Deliberate differences from the fold**, each documented where it lives:
//! caps and exclusivity are not applied (every token is judged alone, see
//! `GenericSweepStrategy::compile_combo`), and a partial sell fills at once rather
//! than after an `ExitPending` wait.

use hunter_engine::arm::{CoinReads, CompiledLine, CompiledRule, EntryVerdict, HeldStep, MetricReq, POSITION_READ};
use hunter_engine::event::ExitReason;
use hunter_engine::metrics::evaluator::eval;
use hunter_engine::metrics::position::PositionCtx;
use hunter_engine::metrics::series::{MetricSeries, SeriesColumn};
use hunter_engine::metrics::state::StateMetrics;
use hunter_engine::metrics::Ts;
use hunter_engine::rule_params::EntryLock;

use trading_core::models::trade::TradeRow;
use trading_core::strategies::kernel::{round_trip_multi_leg, round_trip_with_costs, ExitCode, ExitLeg};
use trading_core::strategies::paper_fill::{find_paper_entry_at, find_paper_exit_at, FillModel, PaperFill};

use crate::sweep::projection::CorpusTrade;
use crate::sweep::strategy::{TokenOutcome, N_EXIT_METRIC_SLOTS};

use super::fast_exit::FastPlan;
use super::strategy::Pricing;

// ───────────────────────────── reads ──────────────────────────────────────

/// A column this run did not record. Reads `NaN`, which satisfies no condition.
pub(crate) const MISSING_COL: usize = usize::MAX;

/// Value at a pre-resolved column index (`NaN` for an unrecorded column).
#[inline]
pub(crate) fn value_at_col(series: &MetricSeries, col: usize, row: usize) -> f64 {
    if col == MISSING_COL {
        f64::NAN
    } else {
        series.value_at(row, col)
    }
}

/// One series row as the engine's [`CoinReads`]: a coin read is the recorded column
/// for that read, which the series computed with the same `TokenTrack::value` the fold
/// reads at the same event.
pub(crate) struct RowReads<'a> {
    pub series: &'a MetricSeries,
    /// Column per [`CompiledRule::coin_reads`] entry.
    pub cols: &'a [usize],
    pub row: usize,
    /// The slot of the latest print at or before `row` (`0` before any).
    pub slot: u64,
}

impl CoinReads for RowReads<'_> {
    #[inline]
    fn coin_value(&self, req: &MetricReq, _now: Ts) -> f64 {
        match self.cols.get(usize::from(req.read_id)) {
            Some(&col) => value_at_col(self.series, col, self.row),
            None => f64::NAN,
        }
    }

    #[inline]
    fn price(&self) -> f64 {
        self.series.price[self.row]
    }

    #[inline]
    fn age_sec(&self, now: Ts) -> f64 {
        StateMetrics::time(self.series.created_at(), now)
    }

    #[inline]
    fn cur_slot(&self) -> u64 {
        self.slot
    }
}

// ───────────────────────────── bound combo ───────────────────────────────

/// A compiled combo plus everything a scan resolves about it once: the column of each
/// coin read, the exit slot of each held line, and whether the fast exit paths apply.
///
/// Every token's series is built over the run's one fixed column set (see
/// `GenericSweepStrategy::prepare_token`), so the column indices are the same on every
/// token and resolving them here costs once per combo, not once per (token, combo).
pub struct BoundCombo {
    pub(crate) rule: CompiledRule,
    /// Series column per [`CompiledRule::coin_reads`] entry ([`MISSING_COL`] when the
    /// run did not record it).
    pub(crate) cols: Vec<usize>,
    /// Per held line ([`CompiledLine::index`]): how its sell is recorded. The slot is
    /// the line's bucket in the aggregate's `n_exit_metrics_by_slot`, numbered over the
    /// lines that sell under their own label (the TP/SL shortcuts and move-only lines
    /// have none), capped at `N_EXIT_METRIC_SLOTS - 1`.
    line_tags: Vec<ExitTag>,
    /// The flat held side the fast exit paths resolve, when the rule has one.
    pub(crate) fast: Option<FastPlan>,
    /// Whether a sell line could hold before entry and so veto it. `false` ⇒ this
    /// combo's entry is a pure function of the shared [`EntryCandidates`] walk (the
    /// pure TP/SL shape), which must not pay for the veto.
    entry_veto_possible: bool,
}

impl BoundCombo {
    /// Bind `rule` against the run's fixed `columns`.
    pub(crate) fn new(columns: &[SeriesColumn], rule: CompiledRule) -> Self {
        let cols = rule
            .coin_reads
            .iter()
            .map(|c| columns.iter().position(|x| x == c).unwrap_or(MISSING_COL))
            .collect();
        let line_tags = line_tags(&rule);
        // A veto line is an `always` or first-stage sell line. It can hold before entry
        // only when every metric condition in it can: a coin read might, a position
        // read is `NaN` there and holds only if its condition accepts `NaN`. A signal
        // condition is taken as possible.
        let entry_veto_possible = rule
            .always
            .iter()
            .chain(rule.stages[0].on.iter())
            .filter(|l| l.sell.is_some())
            .any(|l| {
                l.conds.iter().all(|c| match c {
                    hunter_engine::arm::CondReq::Metric(m) => {
                        m.read_id != POSITION_READ || eval(&m.conds, f64::NAN, m.tolerance)
                    }
                    hunter_engine::arm::CondReq::Signal { .. } => true,
                })
            });
        let fast = FastPlan::of(&rule);
        Self { rule, cols, line_tags, fast, entry_veto_possible }
    }

    fn reads<'a>(&'a self, series: &'a MetricSeries, row: usize, slot: u64) -> RowReads<'a> {
        RowReads { series, cols: &self.cols, row, slot }
    }

    /// How the held line numbered `index` records its sell.
    pub(crate) fn line_tag(&self, index: u16) -> ExitTag {
        self.line_tags[usize::from(index)]
    }

    /// How line `l` records its sell.
    pub(crate) fn line_exit(&self, l: &CompiledLine) -> ExitTag {
        self.line_tag(l.index)
    }
}

/// How each held line of `rule` records its sell, by [`CompiledLine::index`]: the code,
/// the label, and the aggregate slot — the labelled sell lines numbered in rule order
/// (the TP/SL shortcuts and move-only lines have none), capped at
/// `N_EXIT_METRIC_SLOTS - 1`. The ONE numbering: the sweep counts by it, and a replay
/// outcome (which carries only the label) is slotted by it too.
pub(crate) fn line_tags(rule: &CompiledRule) -> Vec<ExitTag> {
    let mut tags = vec![ExitTag::DEAD; rule.held_lines().count()];
    let mut next: u8 = 0;
    for l in rule.held_lines() {
        let reason = l.sell.map_or(ExitReason::Dead, |s| s.reason);
        let (label, slot) = match reason {
            ExitReason::Line(label) => {
                let s = next.min(N_EXIT_METRIC_SLOTS as u8 - 1);
                next = next.saturating_add(1);
                (Some(label), Some(s))
            }
            _ => (None, None),
        };
        tags[usize::from(l.index)] = ExitTag { code: exit_code_of(reason), label, slot };
    }
    tags
}

/// The aggregate slot of the line labelled `label` in `rule` — how a replay outcome's
/// `ExitReason::Line` is bucketed exactly as the sweep buckets it.
pub(crate) fn line_slot_of(rule: &CompiledRule, label: &str) -> Option<u8> {
    line_tags(rule).into_iter().find(|t| t.label == Some(label)).and_then(|t| t.slot)
}

/// How an exit is recorded on the outcome.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ExitTag {
    pub code: ExitCode,
    /// The selling line's label, for a line exit.
    pub label: Option<&'static str>,
    pub slot: Option<u8>,
}

impl ExitTag {
    pub(crate) const DEAD: Self = Self { code: ExitCode::Dead, label: None, slot: None };
}

/// The outcome code of an engine exit reason.
pub(crate) fn exit_code_of(reason: ExitReason) -> ExitCode {
    match reason {
        ExitReason::TakeProfit => ExitCode::TakeProfit,
        ExitReason::StopLoss => ExitCode::StopLoss,
        ExitReason::Dead => ExitCode::Dead,
        ExitReason::Line(_) => ExitCode::Metrics,
        ExitReason::Manual => ExitCode::Manual,
        ExitReason::Migrated => ExitCode::Migrated,
    }
}

/// The slot of the latest print at or before `row` (`0` before any) — what the fold's
/// `TokenTrack::cur_slot` reads at that event.
fn slot_at(series: &MetricSeries, row: usize) -> u64 {
    series.slot[..=row.min(series.n_rows().saturating_sub(1))].iter().rev().find_map(|s| *s).unwrap_or(0)
}

// ───────────────────────────── entry ─────────────────────────────────────

/// The resolved entry for one combo on one token.
#[derive(Clone, Copy, Debug)]
pub enum EntryResolution {
    /// Never entered: dead or unsatisfiable before entry, exhausted, or no fill.
    NoEntry,
    /// Entered — the fill landed at series row `fill_row`.
    Entered { fill_row: usize, price: f64, at: Ts },
}

/// What one row decides for a whole entry class, before the combo's own veto: the
/// engine's [`CompiledRule::try_enter`] with the veto left open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Candidate {
    /// Enter, unless a sell line vetoes; a vetoed row then does `then`.
    Enter { vetoed: AfterVeto },
    /// End the coin (a final filter failed on a print that would buy), unless a sell
    /// line vetoes; a vetoed row then does `then`.
    Exhaust { vetoed: AfterVeto },
}

/// What a vetoed candidate row does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AfterVeto {
    /// Keep walking (no lock, or the slot lock spent this slot).
    Continue,
    /// The coin is done (`token` lock: the first qualifying print decides).
    End,
}

/// One row's entry-class step.
enum RowStep {
    None,
    Candidate(Candidate),
    /// The class ends here whatever the veto says.
    End,
}

/// The veto-free half of [`CompiledRule::try_enter`] at one row. `locked` is the slot
/// the `slot` lock already spent; `spend` is set when this row spends its slot.
fn entry_step(rule: &CompiledRule, reads: &RowReads<'_>, now: Ts, on_print: bool, locked: Option<u64>, spend: &mut bool) -> RowStep {
    let event = || rule.event_satisfied(reads, now);
    let final_ok = || rule.final_filters_satisfied(reads, now);
    let rest_ok = || rule.filters_satisfied(reads, now);
    match rule.entry_lock {
        None => {
            if !event() {
                return RowStep::None;
            }
            match (final_ok(), rest_ok()) {
                (true, true) => RowStep::Candidate(Candidate::Enter { vetoed: AfterVeto::Continue }),
                (false, true) => RowStep::Candidate(Candidate::Exhaust { vetoed: AfterVeto::Continue }),
                _ => RowStep::None,
            }
        }
        Some(EntryLock::Token) => {
            if !on_print || !event() {
                return RowStep::None;
            }
            if final_ok() && rest_ok() {
                RowStep::Candidate(Candidate::Enter { vetoed: AfterVeto::End })
            } else {
                RowStep::End
            }
        }
        Some(EntryLock::Slot) => {
            let slot = reads.slot;
            if locked == Some(slot) && slot != 0 {
                return RowStep::None;
            }
            if !event() {
                return RowStep::None;
            }
            // Every outcome here but Enter and Exhaust spends the slot, and those two
            // end the walk, so the class can spend it for every combo.
            *spend = true;
            match (final_ok(), rest_ok()) {
                (true, true) => RowStep::Candidate(Candidate::Enter { vetoed: AfterVeto::Continue }),
                (false, true) => RowStep::Candidate(Candidate::Exhaust { vetoed: AfterVeto::Continue }),
                _ => RowStep::None,
            }
        }
    }
}

/// The reference entry walk: the fold's armed side, row by row — `Dead >
/// Unsatisfiable > try_enter`. [`resolve_entry_from`] is the shared-walk form the sweep
/// runs; this one is what it is checked against.
///
/// The fill is the buy the run's [`FillModel`] picks after the trigger trade (the
/// same model `run_replay` threads through `ReplayConfig.fill_model`). A tick-timed
/// decision uses the first later trade as the trigger. An empty fill window
/// market-fills at the trigger; a zero-price trigger yields `NoEntry`.
pub(crate) fn resolve_entry(trades: &[CorpusTrade], series: &MetricSeries, b: &BoundCombo, pricing: &Pricing) -> EntryResolution {
    let c = &b.rule;
    let mut locked: Option<u64> = None;
    let mut slot = 0u64;
    for i in 0..series.n_rows() {
        if let Some(s) = series.slot[i] {
            slot = s;
        }
        if series.dead[i] {
            return EntryResolution::NoEntry;
        }
        let reads = b.reads(series, i, slot);
        let now = series.at[i];
        if c.entry_unsatisfiable(&reads, now).is_some() {
            return EntryResolution::NoEntry;
        }
        match c.try_enter(&reads, now, locked, series.slot[i].is_some()) {
            EntryVerdict::Enter => return entry_fill_at(trades, series, i, pricing),
            EntryVerdict::Exhaust => return EntryResolution::NoEntry,
            EntryVerdict::SpendSlot => {
                if slot != 0 {
                    locked = Some(slot);
                }
            }
            EntryVerdict::No => {}
        }
    }
    EntryResolution::NoEntry
}

/// The fill half of an entry decision at series `row`: trigger trade → the run's
/// [`FillModel`] fill → the series row it lands on. `NoEntry` when any of the three is
/// unresolvable — a terminal answer for the token.
fn entry_fill_at(trades: &[CorpusTrade], series: &MetricSeries, row: usize, pricing: &Pricing) -> EntryResolution {
    let Some(trigger_idx) = entry_trigger_trade_idx(series, row) else {
        return EntryResolution::NoEntry;
    };
    let Some(fill) = find_paper_entry_at(trades, trigger_idx, true, pricing.fill_model) else {
        return EntryResolution::NoEntry;
    };
    let Some(fill_row) = series_row_for_trade_idx(series, fill.trade_idx) else {
        return EntryResolution::NoEntry;
    };
    EntryResolution::Entered { fill_row, price: fill.price, at: fill.block_time }
}

// ── The shared entry walk (Stage A candidates, Stage B veto) ─────────────────
//
// Combos that share a `Strategy::entry_key` share their whole entry side; only the
// veto reads the held side. So the walk is split: Stage A walks the rows once per
// (token, entry class) and records the candidate rows with what each does unvetoed and
// vetoed; Stage B applies one combo's veto to them. The walk is resumable, not eager —
// Stage B pulls candidates one at a time — so a class that is never vetoed walks only
// to its first candidate.

/// **Stage A** state for one (token, entry class). Buffers are reused in place across
/// combos, tokens and classes, so steady state allocates nothing.
#[derive(Default)]
pub struct EntryCandidates {
    /// Every examined row is an `Enter` candidate (no entry condition and no lock), so
    /// rows are not recorded — the index arithmetic already describes them.
    all_rows: bool,
    next_row: usize,
    /// Candidate rows found so far, ascending.
    rows: Vec<(u32, Candidate)>,
    /// Where the walk ended (`None` while it can resume): the first dead, killed or
    /// terminal row, or `n_rows`.
    stopped_at: Option<usize>,
    locked_slot: Option<u64>,
    slot: u64,
    /// [`entry_fill_at`] memo by candidate row.
    fills: Vec<(u32, EntryResolution)>,
}

/// Distinct candidate rows one entry class memoizes fills for.
const FILL_MEMO_CAP: usize = 32;

impl EntryCandidates {
    fn open(&mut self, all_rows: bool) {
        self.all_rows = all_rows;
        self.next_row = 0;
        self.rows.clear();
        self.stopped_at = None;
        self.locked_slot = None;
        self.slot = 0;
        self.fills.clear();
    }

    /// Examine one more row. Entry-side reads only.
    fn step(&mut self, series: &MetricSeries, b: &BoundCombo) {
        let i = self.next_row;
        if i >= series.n_rows() {
            self.stopped_at = Some(i);
            return;
        }
        if let Some(s) = series.slot[i] {
            self.slot = s;
        }
        let reads = b.reads(series, i, self.slot);
        let now = series.at[i];
        if series.dead[i] || b.rule.entry_unsatisfiable(&reads, now).is_some() {
            self.stopped_at = Some(i);
            return;
        }
        if !self.all_rows {
            let mut spend = false;
            match entry_step(&b.rule, &reads, now, series.slot[i].is_some(), self.locked_slot, &mut spend) {
                RowStep::None => {}
                RowStep::Candidate(c) => self.rows.push((i as u32, c)),
                RowStep::End => {
                    self.stopped_at = Some(i);
                    return;
                }
            }
            if spend && self.slot != 0 {
                self.locked_slot = Some(self.slot);
            }
        }
        self.next_row = i + 1;
    }

    /// The `k`-th candidate, resuming the walk as far as needed; `None` once the walk
    /// ended before it.
    fn nth(&mut self, k: usize, series: &MetricSeries, b: &BoundCombo) -> Option<(usize, Candidate)> {
        if self.all_rows {
            while self.stopped_at.is_none() && self.next_row <= k {
                self.step(series, b);
            }
            match self.stopped_at {
                Some(stop) if k >= stop => None,
                _ => Some((k, Candidate::Enter { vetoed: AfterVeto::Continue })),
            }
        } else {
            while self.stopped_at.is_none() && self.rows.len() <= k {
                self.step(series, b);
            }
            self.rows.get(k).map(|&(r, c)| (r as usize, c))
        }
    }

    fn fill_at(&mut self, row: usize, compute: impl FnOnce() -> EntryResolution) -> EntryResolution {
        let key = row as u32;
        if let Some((_, hit)) = self.fills.iter().find(|(k, _)| *k == key) {
            return *hit;
        }
        let resolved = compute();
        if self.fills.len() < FILL_MEMO_CAP {
            self.fills.push((key, resolved));
        }
        resolved
    }
}

/// **Stage A** — open the shared walk for one (token, entry class).
pub(crate) fn entry_candidates(_series: &MetricSeries, b: &BoundCombo, out: &mut EntryCandidates) {
    out.open(b.rule.enter_on_arm() && b.rule.entry_lock.is_none());
}

/// **Stage B** — this combo's entry out of the shared walk, applying its own veto.
/// Equal to [`resolve_entry`] by construction; every test fold checks it.
pub(crate) fn resolve_entry_from(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    b: &BoundCombo,
    cands: &mut EntryCandidates,
    pricing: &Pricing,
) -> EntryResolution {
    let mut k = 0usize;
    let resolved = loop {
        let Some((row, cand)) = cands.nth(k, series, b) else { break EntryResolution::NoEntry };
        let vetoed = b.entry_veto_possible
            && b.rule.sell_line_holding_before_entry(&b.reads(series, row, slot_at(series, row)), series.at[row]).is_some();
        match (cand, vetoed) {
            (Candidate::Enter { .. }, false) => break cands.fill_at(row, || entry_fill_at(trades, series, row, pricing)),
            (Candidate::Exhaust { .. }, false) => break EntryResolution::NoEntry,
            (Candidate::Enter { vetoed: after } | Candidate::Exhaust { vetoed: after }, true) => match after {
                AfterVeto::Continue => k += 1,
                AfterVeto::End => break EntryResolution::NoEntry,
            },
        }
    };
    #[cfg(test)]
    {
        // The equivalence the split rests on, against the fused reference, on every
        // combo a test folds. `cfg(test)`, not `debug_assert`: the reference is the O(n)
        // walk the split exists to avoid.
        let reference = resolve_entry(trades, series, b, pricing);
        assert!(same_entry(&resolved, &reference), "shared entry walk disagreed with the reference: {resolved:?} vs {reference:?}");
    }
    resolved
}

#[cfg(test)]
fn same_entry(a: &EntryResolution, b: &EntryResolution) -> bool {
    match (a, b) {
        (EntryResolution::NoEntry, EntryResolution::NoEntry) => true,
        (
            EntryResolution::Entered { fill_row: r1, price: p1, at: t1 },
            EntryResolution::Entered { fill_row: r2, price: p2, at: t2 },
        ) => r1 == r2 && p1.to_bits() == p2.to_bits() && t1 == t2,
        _ => false,
    }
}

// ───────────────────────────── held side ─────────────────────────────────

/// The entry fill every exit prices against.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Fill {
    pub row: usize,
    pub price: f64,
    pub at: Ts,
    /// Slot of the trade the entry fills against.
    pub slot: Option<u64>,
    /// Pool depth at the fill.
    pub reserve: Option<f64>,
}

impl Fill {
    pub(crate) fn of(series: &MetricSeries, entry: &EntryResolution) -> Option<Self> {
        match *entry {
            EntryResolution::NoEntry => None,
            EntryResolution::Entered { fill_row, price, at } => Some(Self {
                row: fill_row,
                price,
                at,
                slot: fill_trade_slot(series, fill_row),
                reserve: depth_at(series, fill_row),
            }),
        }
    }

    /// The position context at the fill, as the fold seeds it.
    pub(crate) fn position(&self) -> PositionCtx {
        PositionCtx::at_fill(self.price, self.at).with_entry_priced_reserve(self.reserve.unwrap_or(f64::NAN))
    }
}

/// A held position mid-walk: the running context, the stage and the legs banked by
/// partial sells.
pub(crate) struct Held {
    pub pos: PositionCtx,
    pub stage: u8,
    pub sold_bps: u16,
    pub legs: Vec<ExitLeg>,
}

/// What one held step did.
pub(crate) enum Stepped {
    /// Nothing, a move, or a partial sell: keep walking.
    Continue,
    /// The whole remaining bag sells, recorded as this exit.
    Close(ExitTag),
}

/// Apply one [`CompiledRule::held_line`] result at instant `now`. A partial leg is
/// priced by `leg` (the caller's fill for this row) and moves to the line's stage at
/// that fill; a move takes effect from the next evaluation.
pub(crate) fn apply_step(
    b: &BoundCombo,
    held: &mut Held,
    step: HeldStep<'_>,
    now: Ts,
    leg: impl FnOnce() -> (ExitLeg, Ts),
) -> Stepped {
    match step {
        HeldStep::None => Stepped::Continue,
        HeldStep::Move(s) => {
            held.stage = s;
            held.pos.stage_since = now;
            Stepped::Continue
        }
        HeldStep::Line(l) => match l.sell {
            None => {
                if let Some(s) = l.go {
                    held.stage = s;
                    held.pos.stage_since = now;
                }
                Stepped::Continue
            }
            // A partial that would take the bag past what is left sells the rest,
            // exactly as `decide_arm` does.
            Some(sell) => match sell.bps {
                Some(bps) if u32::from(held.sold_bps) + u32::from(bps) < 10_000 => {
                    let (mut leg, filled_at) = leg();
                    leg.sell_bps = bps;
                    held.legs.push(leg);
                    held.sold_bps += bps;
                    if let Some(s) = l.go {
                        held.stage = s;
                        held.pos.stage_since = filled_at;
                    }
                    Stepped::Continue
                }
                _ => Stepped::Close(b.line_exit(l)),
            },
        },
    }
}

/// The held-side walk from the entry fill: each row, `Dead` first, then the peak and
/// trough fold, then one [`CompiledRule::held_line`] step. A position still open at
/// the series' end goes to the frozen-tail resolve, then marks to the last price.
///
/// **The reference exit.** The fast paths in [`super::fast_exit`] resolve the flat
/// shapes without walking every row and are checked equal to this.
pub(crate) fn resolve_exit_walk(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    b: &BoundCombo,
    entry: &EntryResolution,
    pricing: &Pricing,
    tail_horizon: Option<Ts>,
) -> TokenOutcome {
    let Some(fill) = Fill::of(series, entry) else { return TokenOutcome::no_entry() };
    let pricing = &pricing.at_entry(trades, series, fill.row);
    let mut held = Held { pos: fill.position(), stage: 0, sold_bps: 0, legs: Vec::new() };
    let mut slot = slot_at(series, fill.row);
    for j in (fill.row + 1)..series.n_rows() {
        if let Some(s) = series.slot[j] {
            slot = s;
        }
        if series.dead[j] {
            return close(trades, series, ExitTag::DEAD, &fill, &held, j, series.at[j], pricing);
        }
        held.pos.fold_price(series.price[j]);
        let now = series.at[j];
        let step = b.rule.held_line(&b.reads(series, j, slot), &held.pos, held.stage, now);
        let leg = || partial_leg(trades, series, j, pricing, fill.reserve);
        if let Stepped::Close(tag) = apply_step(b, &mut held, step, now, leg) {
            return close(trades, series, tag, &fill, &held, j, now, pricing);
        }
    }
    if let Some(closed) = super::frozen_tail::resolve(trades, series, b, &fill, &mut held, pricing, tail_horizon) {
        return closed;
    }
    open(trades, series, &fill, &held, last_mark(series, fill.price), pricing)
}

/// The exit a flat rule's fast path resolved: a row, and the line (or `Dead`) there.
pub(crate) fn close_flat(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    fill: &Fill,
    tag: ExitTag,
    row: usize,
    pricing: &Pricing,
) -> TokenOutcome {
    let held = Held { pos: fill.position(), stage: 0, sold_bps: 0, legs: Vec::new() };
    close(trades, series, tag, fill, &held, row, series.at[row], pricing)
}

// ───────────────────────────── fills and money ───────────────────────────

/// One partial leg filled after `fire_row`, and when it filled: the run's fill model,
/// falling back to the row's spot when no trade maps. `sell_bps` is set by the caller.
pub(crate) fn partial_leg(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    fire_row: usize,
    pricing: &Pricing,
    entry_reserve: Option<f64>,
) -> (ExitLeg, Ts) {
    let (fill, exit_row) = exit_fill_or_spot(trades, series, fire_row, pricing.fill_model);
    let reserve = depth_at(series, exit_row).or(entry_reserve);
    let leg = ExitLeg { sell_bps: 0, price: fill.price, reserve_sol: reserve, venue_fee_bps: venue_fee_of(trades, fill.trade_idx) };
    (leg, fill.block_time.max(series.at[fire_row]))
}

/// The exit fill after `fire_row` and the series row it lands on.
fn exit_fill_or_spot(trades: &[CorpusTrade], series: &MetricSeries, fire_row: usize, fill_model: FillModel) -> (PaperFill, usize) {
    let fill = exit_fill(trades, series, fire_row, fill_model).unwrap_or_else(|| PaperFill {
        trade_idx: 0,
        price: series.price[fire_row],
        token_amount: 0.0,
        slot: series.slot[fire_row].unwrap_or(0),
        block_time: series.at[fire_row],
        tx_signature: String::new(),
        reserve_sol: depth_at(series, fire_row),
    });
    let exit_row = series_row_for_trade_idx(series, fill.trade_idx).unwrap_or(fire_row);
    (fill, exit_row)
}

/// Close the rest of the bag at the fill after `fire_row`, decided at `fire_at` (which
/// a frozen-tail clock can put past the last row: the fill still prices at the last
/// trade, the exit is stamped at the decision).
#[allow(clippy::too_many_arguments)]
pub(crate) fn close(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    tag: ExitTag,
    entry: &Fill,
    held: &Held,
    fire_row: usize,
    fire_at: Ts,
    pricing: &Pricing,
) -> TokenOutcome {
    let (fill, exit_row) = exit_fill_or_spot(trades, series, fire_row, pricing.fill_model);
    let mut legs = held.legs.clone();
    let rem = 10_000u16.saturating_sub(held.sold_bps);
    if rem > 0 {
        legs.push(ExitLeg {
            sell_bps: rem,
            price: fill.price,
            reserve_sol: depth_at(series, exit_row).or(entry.reserve),
            venue_fee_bps: venue_fee_of(trades, fill.trade_idx),
        });
    }
    let exit_at = fill.block_time.max(fire_at);
    let (pnl_sol, pnl_pct) = if legs.is_empty() {
        round_trip_with_costs(entry.price, fill.price, pricing.buy_amount_sol, entry.reserve, entry.reserve, &pricing.cost)
    } else {
        round_trip_multi_leg(entry.price, pricing.buy_amount_sol, entry.reserve, &legs, &pricing.cost)
    };
    TokenOutcome {
        fired: true,
        holding_secs: (exit_at - entry.at).num_seconds(),
        pnl_percent: pnl_pct as f32,
        pnl_sol: pnl_sol as f32,
        exit: tag.code,
        exit_label: tag.label,
        exit_metric_slot: tag.slot,
        entry_time: Some(entry.at),
        entry_price: Some(entry.price),
        entry_slot: entry.slot,
        exit_time: Some(exit_at),
        exit_price: Some(fill.price),
        exit_slot: fill_trade_slot(series, exit_row),
    }
}

/// A still-`Open` position: banked legs plus the rest marked to `last_price` sold
/// into `last_depth`.
pub(crate) fn open(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    entry: &Fill,
    held: &Held,
    (last_price, last_depth): (f64, Option<f64>),
    pricing: &Pricing,
) -> TokenOutcome {
    let rem = 10_000u16.saturating_sub(held.sold_bps);
    let mut legs = held.legs.clone();
    // With nothing banked the mark carries only the depth at the mark; with legs
    // banked the rest falls back to the entry depth (the two shapes this had before).
    let reserve = if held.legs.is_empty() { last_depth } else { last_depth.or(entry.reserve) };
    if rem > 0 {
        legs.push(ExitLeg { sell_bps: rem, price: last_price, reserve_sol: reserve, venue_fee_bps: mark_venue_fee(trades) });
    }
    let (pnl_sol, pnl_pct) =
        round_trip_multi_leg(entry.price, pricing.buy_amount_sol, depth_at(series, entry.row), &legs, &pricing.cost);
    TokenOutcome {
        fired: true,
        holding_secs: 0,
        pnl_percent: pnl_pct as f32,
        pnl_sol: pnl_sol as f32,
        exit: ExitCode::Open,
        exit_label: None,
        exit_metric_slot: None,
        entry_time: Some(entry.at),
        entry_price: Some(entry.price),
        entry_slot: entry.slot,
        exit_time: None,
        exit_price: None,
        exit_slot: None,
    }
}

/// SOL-side pool depth at `row` for price impact — the **priced** depth (`vsol`), not
/// the real reserve. `None` when unknown (pre-first-trade rows are `NaN`), which the
/// cost model treats as "depth unknown" and charges no impact.
pub(crate) fn depth_at(series: &MetricSeries, row: usize) -> Option<f64> {
    series.priced_reserve_sol.get(row).copied().filter(|r| r.is_finite() && *r > 0.0)
}

/// The last finite price in the series and the depth there — what an `Open` position
/// is marked to. Falls back to the entry price (no depth).
pub(crate) fn last_mark(series: &MetricSeries, entry_price: f64) -> (f64, Option<f64>) {
    (0..series.n_rows())
        .rev()
        .find(|&k| series.price[k].is_finite())
        .map_or((entry_price, None), |k| (series.price[k], depth_at(series, k)))
}

/// The pool fee an open bag is marked to sell at: its token's newest trade's.
fn mark_venue_fee(trades: &[CorpusTrade]) -> Option<f64> {
    trades.last().and_then(TradeRow::venue_fee_bps)
}

/// The PumpSwap fee of the trade a fill at series `row` lands on — the first trade
/// row at or after it.
pub(crate) fn venue_fee_at_row(trades: &[CorpusTrade], series: &MetricSeries, row: usize) -> Option<f64> {
    trades.last().and_then(TradeRow::venue_fee_bps)?;
    let trade_row = (row..series.n_rows()).find(|&r| series.slot[r].is_some())?;
    let idx = series.slot[..trade_row].iter().filter(|s| s.is_some()).count();
    trades.get(idx).and_then(TradeRow::venue_fee_bps)
}

/// The PumpSwap fee of `trades[idx]`.
fn venue_fee_of(trades: &[CorpusTrade], idx: usize) -> Option<f64> {
    trades.get(idx).and_then(TradeRow::venue_fee_bps)
}

/// The real trade a fill at series `row` executes against: the first trade row at or
/// after `row` — the trade the chart marker snaps to.
pub(crate) fn fill_trade_slot(series: &MetricSeries, row: usize) -> Option<u64> {
    (row..series.n_rows()).find_map(|j| series.slot[j])
}

/// Corpus trade index that triggers an entry decision at `decision_row`: the trade on
/// that row, else the first print after it.
fn entry_trigger_trade_idx(series: &MetricSeries, decision_row: usize) -> Option<usize> {
    if series.slot.get(decision_row).copied().flatten().is_some() {
        return trade_idx_at_row(series, decision_row);
    }
    first_trade_idx_after_row(series, decision_row)
}

/// Corpus index of the trade on series `row` (`None` on a tick).
fn trade_idx_at_row(series: &MetricSeries, row: usize) -> Option<usize> {
    series.slot.get(row).copied().flatten()?;
    Some(series.slot[..=row].iter().filter(|s| s.is_some()).count() - 1)
}

/// Corpus index of the first trade row strictly after `row`.
fn first_trade_idx_after_row(series: &MetricSeries, row: usize) -> Option<usize> {
    let n = series.n_rows();
    if n == 0 {
        return None;
    }
    let before = if row >= n {
        series.slot.iter().filter(|s| s.is_some()).count()
    } else {
        series.slot[..=row].iter().filter(|s| s.is_some()).count()
    };
    let from = row.saturating_add(1).min(n);
    let after = series.slot[from..].iter().filter(|s| s.is_some()).count();
    (after > 0).then_some(before)
}

/// Series row of the `trade_idx`-th trade print.
fn series_row_for_trade_idx(series: &MetricSeries, trade_idx: usize) -> Option<usize> {
    let mut seen = 0usize;
    for (row, slot) in series.slot.iter().enumerate() {
        if slot.is_some() {
            if seen == trade_idx {
                return Some(row);
            }
            seen += 1;
        }
    }
    None
}

/// The exit fill after `fire_row`, priced by the run's [`FillModel`]. A tick-timed fire
/// signals at the last trade at or before the row. Analysis path: an empty window
/// market-fills at the signal trade.
fn exit_fill(trades: &[CorpusTrade], series: &MetricSeries, fire_row: usize, fill_model: FillModel) -> Option<PaperFill> {
    let fire_idx = trade_idx_at_row(series, fire_row).or_else(|| (0..=fire_row).rev().find_map(|r| trade_idx_at_row(series, r)))?;
    find_paper_exit_at(trades, fire_idx, true, fill_model)
}

// ───────────────────────────── drivers ───────────────────────────────────

/// Entry then exit for one token, binding the rule against this series' own columns —
/// the guard's and the single-combo drill-in's driver. The sweep binds once per combo
/// instead (`Strategy::bind_param`).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn scan(trades: &[CorpusTrade], series: &MetricSeries, c: &CompiledRule, pricing: &Pricing) -> TokenOutcome {
    scan_with_horizon(trades, series, c, pricing, None)
}

/// [`scan`] with the corpus frozen-tail horizon (see [`super::frozen_tail`]).
pub(crate) fn scan_with_horizon(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    c: &CompiledRule,
    pricing: &Pricing,
    tail_horizon: Option<Ts>,
) -> TokenOutcome {
    let bound = BoundCombo::new(series.columns(), c.clone());
    let entry = resolve_entry(trades, series, &bound, pricing);
    resolve_exit_walk(trades, series, &bound, &entry, pricing, tail_horizon)
}

/// The precompute columns a compiled rule reads — its coin reads, in order.
pub(crate) fn columns_for(compiled: &CompiledRule) -> Vec<SeriesColumn> {
    compiled.coin_reads.clone()
}

/// The sparse grid one compiled rule needs: its own clock horizons.
pub(crate) fn sparse_grid_for(compiled: &CompiledRule) -> hunter_engine::metrics::grid::SparseGrid {
    let h = compiled.clock_horizons;
    hunter_engine::metrics::grid::SparseGrid {
        max_window_secs: h.max_window_secs,
        time_horizon_secs: h.time_secs,
        // `held` and `stage` climb from instants at or before the last trade, so they
        // ride the `stall` horizon, as `readout::replay_series` sizes it.
        stall_horizon_secs: h.stall_secs.max(h.held_secs).max(h.stage_secs),
    }
}
