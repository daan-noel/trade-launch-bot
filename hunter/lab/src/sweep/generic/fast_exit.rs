//! The sweep's fast exit paths — for a **flat** held side, the first exit row without
//! walking every row.
//!
//! A flat held side is one stage with no deadline whose every line sells the whole bag
//! on ONE condition on our position (the TP/SL shortcuts, `pnl_pct`, `held_sec`,
//! `retrace_pct`, `bounce_pct`). Then the exit is the earliest row any line holds (ties
//! go to the earlier line, the order [`CompiledRule::held_line`] checks them in), and
//! each line's first row has a direct query: a prefix-extrema hull for a `pnl_pct`
//! bound (O(log n)), a binary search for a `held_sec` bound, a vectorized running-peak
//! scan for a trailing stop.
//!
//! Everything else walks ([`scan::resolve_exit_walk`]), and so does any shape a query
//! cannot prove itself sound on (a non-monotone clock, a `pnl` overflow): the walk is
//! the reference every query here is checked equal to (`super::guard`).

use chrono::DateTime;

use hunter_engine::arm::{CompiledRule, CondReq, MetricReq, POSITION_READ};
use hunter_engine::metrics::evaluator::{eval, Condition, Operator};
use hunter_engine::metrics::position::PositionCtx;
use hunter_engine::metrics::series::MetricSeries;
use hunter_engine::metrics::{Metric, Ts};

use crate::sweep::projection::CorpusTrade;
use crate::sweep::strategy::TokenOutcome;

use super::scan::{self, BoundCombo, EntryResolution, ExitTag, Fill};
use super::strategy::Pricing;

/// How one line's first firing row is found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ExitClass {
    /// `pnl_pct` under one ordering bound: `pnl` rises with price, so the hull answers
    /// it. The TP/SL shortcuts land here.
    PnlBound { up: bool },
    /// `held_sec` under a `>`/`>=` bound: `held` rises with row time.
    HeldBound,
    /// `retrace_pct`: needs the running peak — O(n), vectorized.
    Trailing,
    /// `bounce_pct`: needs the running trough — O(n).
    Bounce,
}

/// One line of a flat held side.
#[derive(Clone, Debug)]
pub(crate) struct FastLine {
    /// [`hunter_engine::arm::CompiledLine::index`].
    pub index: u16,
    pub class: ExitClass,
    pub req: MetricReq,
}

/// A flat held side, line by line in check order.
#[derive(Clone, Debug)]
pub(crate) struct FastPlan {
    pub lines: Vec<FastLine>,
}

impl FastPlan {
    /// The rule's held side as a flat plan, or `None` when it is not flat.
    pub(crate) fn of(rule: &CompiledRule) -> Option<Self> {
        let stage = match rule.stages.as_slice() {
            [s] if s.ends.is_none() && s.at_end.is_empty() => s,
            _ => return None,
        };
        let _ = stage;
        let mut lines = Vec::new();
        for l in rule.held_lines() {
            let sell = l.sell?;
            if sell.bps.is_some() || l.go.is_some() {
                return None;
            }
            let [CondReq::Metric(m)] = l.conds.as_slice() else { return None };
            lines.push(FastLine { index: l.index, class: classify(m)?, req: m.clone() });
        }
        Some(Self { lines })
    }
}

/// The single condition of a single-arm condition, if that is what it is.
fn lone_cond(req: &MetricReq) -> Option<Condition> {
    match req.conds.as_slice() {
        [arm] => match arm.as_slice() {
            [c] => Some(*c),
            _ => None,
        },
        _ => None,
    }
}

/// The query that finds `req`'s first firing row, or `None` when only the walk can.
fn classify(req: &MetricReq) -> Option<ExitClass> {
    if req.read_id != POSITION_READ {
        // A coin read is an arbitrary function of the series columns.
        return None;
    }
    match req.r.metric {
        Metric::RetracePct => Some(ExitClass::Trailing),
        Metric::BouncePct => Some(ExitClass::Bounce),
        Metric::PnlPct => match lone_cond(req).map(|c| c.operator) {
            Some(Operator::Gt | Operator::Gte) => Some(ExitClass::PnlBound { up: true }),
            Some(Operator::Lt | Operator::Lte) => Some(ExitClass::PnlBound { up: false }),
            // `=`/`!=` are tolerance bands and a multi-arm DNF can be an interval.
            _ => None,
        },
        Metric::HeldSec => match lone_cond(req).map(|c| c.operator) {
            // `held` only rises, so a lower bound is one crossing.
            Some(Operator::Gt | Operator::Gte) => Some(ExitClass::HeldBound),
            _ => None,
        },
        _ => None,
    }
}

/// Whether this (combo, entry) wants the exit index built — the one predicate
/// `Strategy::build_exit_ctx` branches on.
pub(crate) fn wants_exit_index(bound: &BoundCombo, entry: &EntryResolution) -> bool {
    bound.fast.is_some() && matches!(entry, EntryResolution::Entered { .. })
}

/// The held side over a prebuilt [`super::exit_index::ExitIndex`]: each line's first
/// firing row by its class's query, then the earliest of those rows — and `Dead` —
/// decides. Falls back to the walk for a non-flat rule, an unready index, or a query
/// that cannot prove itself sound on this token.
pub(crate) fn resolve_exit_indexed(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    b: &BoundCombo,
    entry: &EntryResolution,
    pricing: &Pricing,
    index: &super::exit_index::ExitIndex,
    tail_horizon: Option<Ts>,
) -> TokenOutcome {
    let Some(fill) = Fill::of(series, entry) else { return TokenOutcome::no_entry() };
    let Some(plan) = b.fast.as_ref().filter(|_| index.is_ready()) else {
        return scan::resolve_exit_walk(trades, series, b, entry, pricing, tail_horizon);
    };
    // (row, line position) of the earliest line that fires; a tie keeps the earlier
    // line, the order the walk checks lines in.
    let mut best: Option<(usize, usize)> = None;
    for (i, line) in plan.lines.iter().enumerate() {
        let row = match line.class {
            ExitClass::PnlBound { up } => match first_pnl_row(index, &line.req, fill.price, up) {
                Ok(row) => row,
                Err(()) => return scan::resolve_exit_walk(trades, series, b, entry, pricing, tail_horizon),
            },
            ExitClass::HeldBound => match first_held_row(series, index, fill.row, fill.at, &line.req) {
                Ok(row) => row,
                Err(()) => return scan::resolve_exit_walk(trades, series, b, entry, pricing, tail_horizon),
            },
            ExitClass::Trailing => first_trailing_row(series, fill.row, fill.price, &line.req),
            ExitClass::Bounce => first_bounce_row(series, fill.row, fill.price, &line.req),
        };
        if let Some(row) = row {
            if best.is_none_or(|(br, _)| row < br) {
                best = Some((row, i));
            }
        }
    }
    let pricing = &pricing.at_entry(trades, series, fill.row);
    // Dead outranks every line at any row (the walk checks it first).
    let winner = match (index.dead_row(), best) {
        (Some(dead), Some((br, _))) if dead <= br => Some((dead, ExitTag::DEAD)),
        (Some(dead), None) => Some((dead, ExitTag::DEAD)),
        (_, Some((br, i))) => Some((br, b.line_tag(plan.lines[i].index))),
        (None, None) => None,
    };
    match winner {
        Some((row, tag)) => scan::close_flat(trades, series, &fill, tag, row, pricing),
        None => open_tail(trades, series, b, &fill, pricing, tail_horizon, index.last_finite_row()),
    }
}

/// A flat position still open at the series' end: the frozen-tail resolve, else the
/// mark at the last finite price.
fn open_tail(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    b: &BoundCombo,
    fill: &Fill,
    pricing: &Pricing,
    tail_horizon: Option<Ts>,
    last_finite_row: Option<usize>,
) -> TokenOutcome {
    let mut held = scan::Held { pos: fill.position(), stage: 0, sold_bps: 0, legs: Vec::new() };
    // The tail reads the running peak and trough the walk would have folded.
    for j in (fill.row + 1)..series.n_rows() {
        held.pos.fold_price(series.price[j]);
    }
    if let Some(closed) = super::frozen_tail::resolve(trades, series, b, fill, &mut held, pricing, tail_horizon) {
        return closed;
    }
    let mark = last_finite_row
        .filter(|&k| series.price[k].is_finite())
        .map_or((fill.price, None), |k| (series.price[k], scan::depth_at(series, k)));
    scan::open(trades, series, fill, &held, mark, pricing)
}

// ───────────────────── per-class first-firing-row resolvers ────────────────────
//
// Each of these must return **exactly** the row the scalar walk would stop at for
// that one req — same `eval`, same value, same NaN handling. They differ only in
// how they get there.

/// First row satisfying a monotone `m_position.pnl` bound, via the prefix-extrema
/// hull — O(log n).
///
/// `pnl` is strictly increasing in price (for `entry_price > 0`), so an upward-closed
/// condition holds at some row `≤ i` iff it holds at the running **max** price up to
/// `i`; symmetrically for the running min. The predicate is the same `eval` the
/// scalar walk applies, so the two cannot disagree about inclusivity or NaN.
///
/// `Err(())` ⇒ the monotonicity premise doesn't hold on this token (a price extreme
/// whose `pnl` overflows to ±inf, which `eval` then rejects out of order); the caller
/// must fall back to scalar.
fn first_pnl_row(
    index: &super::exit_index::ExitIndex,
    req: &MetricReq,
    entry_price: f64,
    up: bool,
) -> Result<Option<usize>, ()> {
    if !usable_entry_price(entry_price) {
        // `pnl` is NaN throughout — no row can fire, exactly as scalar finds.
        return Ok(None);
    }
    let ctx = PositionCtx::at_fill(entry_price, DateTime::UNIX_EPOCH);
    let extreme = if up { index.hull_max_last() } else { index.hull_min_last() };
    if let Some(x) = extreme {
        // The extreme bounds every value the hull can carry; if `pnl` stays finite
        // there it is finite (and monotone) everywhere in range.
        if x.is_finite() && !ctx.pnl(x).is_finite() {
            return Err(());
        }
    }
    let pred = |price: f64| eval(&req.conds, ctx.pnl(price), req.tolerance);
    Ok(if up { index.first_max_row(pred) } else { index.first_min_row(pred) })
}

/// First row satisfying a `>=`/`>` bound on `m_position.held`, by binary search on
/// the series' `at` column — O(log n).
///
/// `held` is `now − entered_at` floored at zero, so it rises with `at`; the search is
/// only sound while `at` is non-decreasing over the scan range, which
/// [`ExitIndex`](super::exit_index::ExitIndex) checks during its rebuild. `Err(())`
/// ⇒ it isn't; fall back to scalar.
fn first_held_row(
    series: &MetricSeries,
    index: &super::exit_index::ExitIndex,
    fill_row: usize,
    entry_at: Ts,
    req: &MetricReq,
) -> Result<Option<usize>, ()> {
    if !index.at_nondecreasing() {
        return Err(());
    }
    let n = series.n_rows();
    let start = fill_row.saturating_add(1);
    if start >= n {
        return Ok(None);
    }
    let ctx = PositionCtx::at_fill(1.0, entry_at);
    let at = &series.at[start..n];
    // `partition_point` wants the "not yet fired" prefix — true then false.
    let i = at.partition_point(|&now| !eval(&req.conds, ctx.held(now), req.tolerance));
    Ok((i < at.len()).then_some(start + i))
}

/// First row satisfying a `m_position.retrace` condition — O(n) with the running
/// since-entry peak, vectorized on AVX-512 hosts for a single ordering condition.
///
/// **Not** O(log n), and there is no cheap index for it: the peak is a running
/// quantity, so this is a genuine prefix-dependent scan, not a static prefix query.
fn first_trailing_row(
    series: &MetricSeries,
    fill_row: usize,
    entry_price: f64,
    req: &MetricReq,
) -> Option<usize> {
    let n = series.n_rows();
    let start = fill_row.saturating_add(1);
    if start >= n {
        return None;
    }
    if let Some(c) = lone_cond(req).filter(|c| is_ordering_op(c.operator)) {
        return first_trailing_row_cmp(&series.price, start, n, entry_price, c.operator, c.value);
    }
    first_trailing_row_scalar(series, start, n, entry_price, req)
}

/// Scalar reference for [`first_trailing_row`] — the exact per-row work the scalar
/// walk does for a `retrace` req, extracted so the vector kernel has one definition
/// to be proven against.
fn first_trailing_row_scalar(
    series: &MetricSeries,
    start: usize,
    n: usize,
    entry_price: f64,
    req: &MetricReq,
) -> Option<usize> {
    let mut ctx = PositionCtx::at_fill(entry_price, DateTime::UNIX_EPOCH);
    for j in start..n {
        let p = series.price[j];
        ctx.fold_price(p);
        if eval(&req.conds, ctx.retrace(p), req.tolerance) {
            return Some(j);
        }
    }
    None
}

/// First row satisfying a `m_position.bounce` condition — O(n) with the running
/// since-entry trough (mirror of [`first_trailing_row`]).
fn first_bounce_row(
    series: &MetricSeries,
    fill_row: usize,
    entry_price: f64,
    req: &MetricReq,
) -> Option<usize> {
    let n = series.n_rows();
    let start = fill_row.saturating_add(1);
    if start >= n {
        return None;
    }
    let mut ctx = PositionCtx::at_fill(entry_price, DateTime::UNIX_EPOCH);
    for j in start..n {
        let p = series.price[j];
        ctx.fold_price(p);
        if eval(&req.conds, ctx.bounce(p), req.tolerance) {
            return Some(j);
        }
    }
    None
}

/// Whether `entry_price` is a usable reference for the position metrics.
/// [`PositionCtx::pnl`] / [`PositionCtx::retrace`] / [`PositionCtx::bounce`] yield
/// `NaN` for anything else, so no row can fire and the fast paths have nothing to
/// search for.
#[inline]
fn usable_entry_price(p: f64) -> bool {
    p.is_finite() && p > 0.0
}

/// Ordering operators the vector kernels can replicate exactly (`=`/`!=` are
/// tolerance bands and stay on the scalar path).
#[inline]
fn is_ordering_op(op: Operator) -> bool {
    matches!(op, Operator::Gt | Operator::Gte | Operator::Lt | Operator::Lte)
}

/// `eval_one` for an ordering operator, spelled out so the vector kernels and the
/// scalar remainder share one definition of the compare.
#[inline]
fn cmp_ordering(op: Operator, value: f64, threshold: f64) -> bool {
    if !value.is_finite() {
        return false;
    }
    match op {
        Operator::Gt => value > threshold,
        Operator::Gte => value >= threshold,
        Operator::Lt => value < threshold,
        Operator::Lte => value <= threshold,
        // Never reached: `is_ordering_op` gates every caller.
        _ => false,
    }
}

// ───────────────────────────── SIMD exit scan ──────────────────────────────
//
// AVX-512 counterpart of the all-`pnl_pct` flat shape (the TP/SL rule). It vectorizes
// ONLY the search for the first exit row; the line that fires there and the money
// math stay the shared scalar copies, so the question it answers is just "did the
// vector scan find the row the walk finds?" — which the guard proves.
//
// The price column is `f64`, so the vector is 8 lanes (`__m512d`).

/// The all-`pnl_pct` flat shape, vectorized; any other shape (or a host without
/// AVX-512) takes [`resolve_exit_indexed`].
pub(crate) fn resolve_exit_simd(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    b: &BoundCombo,
    entry: &EntryResolution,
    pricing: &Pricing,
    index: &super::exit_index::ExitIndex,
    tail_horizon: Option<Ts>,
) -> TokenOutcome {
    let Some(fill) = Fill::of(series, entry) else { return TokenOutcome::no_entry() };
    let (Some(plan), Some(bounds)) = (b.fast.as_ref(), pnl_bounds_for_vector_scan(b, fill.price)) else {
        return resolve_exit_indexed(trades, series, b, entry, pricing, index, tail_horizon);
    };
    if !simd_available() {
        return resolve_exit_indexed(trades, series, b, entry, pricing, index, tail_horizon);
    }
    let pricing = &pricing.at_entry(trades, series, fill.row);
    let n = series.n_rows();
    if let Some(j) = first_pnl_exit_row(&series.price, &series.dead, fill.row + 1, n, fill.price, &bounds) {
        // The walk's order at this one row: Dead first, else the earliest line holding.
        let tag = if series.dead[j] {
            ExitTag::DEAD
        } else {
            let ctx = PositionCtx::at_fill(fill.price, fill.at);
            let pnl = ctx.pnl(series.price[j]);
            plan.lines
                .iter()
                .find(|l| eval(&l.req.conds, pnl, l.req.tolerance))
                .map_or(ExitTag::DEAD, |l| b.line_tag(l.index))
        };
        return scan::close_flat(trades, series, &fill, tag, j, pricing);
    }
    open_tail(trades, series, b, &fill, pricing, tail_horizon, index.last_finite_row())
}

/// One lane-comparable exit bound: `pnl <op> value`, `op` an ordering operator.
#[derive(Clone, Copy, Debug)]
struct PnlBound {
    op: Operator,
    value: f64,
}

/// The flat plan as vector-comparable `pnl` bounds, or `None` when one line is not a
/// single ordering bound on `pnl_pct`, or the entry price leaves `pnl` `NaN`.
fn pnl_bounds_for_vector_scan(b: &BoundCombo, entry_price: f64) -> Option<Vec<PnlBound>> {
    let plan = b.fast.as_ref()?;
    if !usable_entry_price(entry_price) || plan.lines.is_empty() {
        return None;
    }
    plan.lines
        .iter()
        .map(|l| match l.class {
            ExitClass::PnlBound { .. } => {
                lone_cond(&l.req).filter(|c| is_ordering_op(c.operator)).map(|c| PnlBound { op: c.operator, value: c.value })
            }
            _ => None,
        })
        .collect()
}

/// True only when the host has the AVX-512 features the kernels here use. They need
/// just `avx512f` (the dead lanes are built scalar), matching
/// [`crate::sweep::registry::avx512_available`].
#[cfg(target_arch = "x86_64")]
#[inline]
fn simd_available() -> bool {
    std::is_x86_feature_detected!("avx512f")
}
#[cfg(not(target_arch = "x86_64"))]
#[inline]
fn simd_available() -> bool {
    false
}

/// First series row in `start..n` at which the scalar exit predicate holds:
/// `dead[j] || any bound holds on pnl(price[j])`. Returned in the same scan order
/// the scalar loop uses, so the caller classifies exactly one row identically to
/// scalar. Vectorized 8×`f64` on AVX-512; scalar remainder + scalar fallback.
///
/// `pnl` is computed per lane with the same `(p − entry) / entry · 100` op sequence
/// [`PositionCtx::pnl`] uses. IEEE-754 basic ops are exactly rounded, so the vector
/// and scalar values are **bit-identical** — which is why this can compare in pnl
/// space rather than inverting each bound back into a price threshold (an inversion
/// that would only be correct to within a rounding step).
#[cfg(target_arch = "x86_64")]
fn first_pnl_exit_row(
    price: &[f64],
    dead: &[bool],
    start: usize,
    n: usize,
    entry_price: f64,
    bounds: &[PnlBound],
) -> Option<usize> {
    if start >= n {
        return None;
    }
    if simd_available() {
        // SAFETY: `avx512f` confirmed present just above. The kernel reads `price`
        // and `dead` only within `[start, n)`, and `n == series.n_rows()` bounds both
        // parallel columns (see `MetricSeries`), so every access is in range.
        unsafe { first_pnl_exit_row_avx512(price, dead, start, n, entry_price, bounds) }
    } else {
        first_pnl_exit_row_scalar(price, dead, start, n, entry_price, bounds)
    }
}
#[cfg(not(target_arch = "x86_64"))]
fn first_pnl_exit_row(
    price: &[f64],
    dead: &[bool],
    start: usize,
    n: usize,
    entry_price: f64,
    bounds: &[PnlBound],
) -> Option<usize> {
    first_pnl_exit_row_scalar(price, dead, start, n, entry_price, bounds)
}

/// Scalar reference for [`first_pnl_exit_row`] — the exact predicate the scalar
/// [`resolve_exit`] loop applies for a set of `pnl` bounds, extracted so the vector
/// path's remainder tail, the non-AVX-512 fallback, and the parity guard all share
/// one definition of it.
#[inline]
fn first_pnl_exit_row_scalar(
    price: &[f64],
    dead: &[bool],
    start: usize,
    n: usize,
    entry_price: f64,
    bounds: &[PnlBound],
) -> Option<usize> {
    let ctx = PositionCtx::at_fill(entry_price, DateTime::UNIX_EPOCH);
    for j in start..n {
        if dead[j] {
            return Some(j);
        }
        let v = ctx.pnl(price[j]);
        if bounds.iter().any(|b| cmp_ordering(b.op, v, b.value)) {
            return Some(j);
        }
    }
    None
}

/// AVX-512 kernel for [`first_pnl_exit_row`]: scans 8 `f64` prices per instruction
/// for the first exit row, handling the `< 8` tail via
/// [`first_pnl_exit_row_scalar`].
///
/// # Safety
/// The caller must have verified `avx512f` is available (see [`first_pnl_exit_row`]).
/// `start ≤ n` and `n` must not exceed `price.len()` or `dead.len()`.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn first_pnl_exit_row_avx512(
    price: &[f64],
    dead: &[bool],
    start: usize,
    n: usize,
    entry_price: f64,
    bounds: &[PnlBound],
) -> Option<usize> {
    use std::arch::x86_64::*;
    const LANES: usize = 8;

    let max_v = _mm512_set1_pd(f64::MAX);
    let entry_v = _mm512_set1_pd(entry_price);
    let hundred_v = _mm512_set1_pd(100.0);

    let mut j = start;
    while j + LANES <= n {
        // 8 prices (unaligned — the flat series buffer isn't 64-B aligned).
        let pv = _mm512_loadu_pd(price.as_ptr().add(j));
        // finite lanes: |p| ≤ f64::MAX — false for NaN and ±inf, exactly `is_finite`.
        let finite = _mm512_cmp_pd_mask::<_CMP_LE_OQ>(_mm512_abs_pd(pv), max_v);
        // pnl per lane, same op order as `PositionCtx::pnl` ⇒ bit-identical.
        let pnl = _mm512_mul_pd(
            _mm512_div_pd(_mm512_sub_pd(pv, entry_v), entry_v),
            hundred_v,
        );
        let mut hit: __mmask8 = 0;
        for b in bounds {
            let t = _mm512_set1_pd(b.value);
            // Ordered compares so a NaN lane never matches, matching `eval_one`'s
            // non-finite rejection; the `finite` AND below covers ±inf prices, whose
            // pnl would otherwise compare as a genuine ±inf.
            hit |= match b.op {
                Operator::Gt => _mm512_cmp_pd_mask::<_CMP_GT_OQ>(pnl, t),
                Operator::Gte => _mm512_cmp_pd_mask::<_CMP_GE_OQ>(pnl, t),
                Operator::Lt => _mm512_cmp_pd_mask::<_CMP_LT_OQ>(pnl, t),
                _ => _mm512_cmp_pd_mask::<_CMP_LE_OQ>(pnl, t),
            };
        }
        // A finite price can still overflow `pnl` to ±inf on a tiny entry price;
        // `is_finite` on the pnl VALUE is what `eval_one` checks, so mask on both.
        let pnl_finite = _mm512_cmp_pd_mask::<_CMP_LE_OQ>(_mm512_abs_pd(pnl), max_v);
        hit &= finite & pnl_finite;
        // dead lanes: built scalar (dead is rarely set → cheap predictable branches),
        // which keeps the kernel to plain AVX-512F (no BW/VL byte-mask intrinsics).
        let mut dead_hit: __mmask8 = 0;
        for lane in 0..LANES {
            if dead[j + lane] {
                dead_hit |= 1u8 << lane;
            }
        }
        let hit = hit | dead_hit;
        if hit != 0 {
            return Some(j + hit.trailing_zeros() as usize);
        }
        j += LANES;
    }
    // Tail (< 8 rows left): the shared scalar predicate.
    first_pnl_exit_row_scalar(price, dead, j, n, entry_price, bounds)
}

// ─────────────────── AVX-512 trailing stop (running-peak scan) ─────────────────
//
// `retrace` compares against the since-entry PEAK, so its first crossing is
// `first j where price[j] <= k · max(price[fill..j])` — a prefix-dependent scan, not
// a static prefix query. There is no cheap index for it and none is claimed: the
// honest target is a **vectorized O(n)**, which is what this is. The prefix max is
// built with a 3-step Hillis-Steele shift-and-max inside each 8-lane block, seeded
// with the max carried out of the previous block.

/// First row in `start..n` where `retrace <op> value` holds against the running
/// since-entry peak (seeded at `entry_price`). Vectorized on AVX-512; the scalar
/// reference below is the SSOT both the tail and non-AVX-512 hosts use.
#[cfg(target_arch = "x86_64")]
fn first_trailing_row_cmp(
    price: &[f64],
    start: usize,
    n: usize,
    entry_price: f64,
    op: Operator,
    value: f64,
) -> Option<usize> {
    if start >= n {
        return None;
    }
    if simd_available() {
        // SAFETY: `avx512f` confirmed present just above; the kernel reads `price`
        // only within `[start, n)` and `n ≤ price.len()` by construction.
        unsafe { first_trailing_row_avx512(price, start, n, entry_price, op, value) }
    } else {
        first_trailing_row_cmp_scalar(price, start, n, entry_price, op, value)
    }
}
#[cfg(not(target_arch = "x86_64"))]
fn first_trailing_row_cmp(
    price: &[f64],
    start: usize,
    n: usize,
    entry_price: f64,
    op: Operator,
    value: f64,
) -> Option<usize> {
    first_trailing_row_cmp_scalar(price, start, n, entry_price, op, value)
}

/// Scalar reference for [`first_trailing_row_cmp`] — the same running-peak +
/// [`PositionCtx::retrace`] the scalar walk computes per row.
#[inline]
fn first_trailing_row_cmp_scalar(
    price: &[f64],
    start: usize,
    n: usize,
    entry_price: f64,
    op: Operator,
    value: f64,
) -> Option<usize> {
    let mut ctx = PositionCtx::at_fill(entry_price, DateTime::UNIX_EPOCH);
    for (j, &p) in price.iter().enumerate().take(n).skip(start) {
        ctx.fold_price(p);
        if cmp_ordering(op, ctx.retrace(p), value) {
            return Some(j);
        }
    }
    None
}

/// AVX-512 kernel for [`first_trailing_row_cmp`].
///
/// Per 8-lane block: replace non-finite prices with `−inf` (the scalar peak update
/// only admits finite prices, and `−inf` never wins a max), inclusive-prefix-max the
/// block, fold in the carried peak, compute `retrace = (peak − p) / peak · 100` with
/// the same op sequence [`PositionCtx::retrace`] uses (⇒ bit-identical), then compare
/// with an ordered predicate ANDed with the finite-price mask.
///
/// # Safety
/// The caller must have verified `avx512f` (see [`first_trailing_row_cmp`]).
/// `start ≤ n ≤ price.len()`.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn first_trailing_row_avx512(
    price: &[f64],
    start: usize,
    n: usize,
    entry_price: f64,
    op: Operator,
    value: f64,
) -> Option<usize> {
    use std::arch::x86_64::*;
    const LANES: usize = 8;

    let max_v = _mm512_set1_pd(f64::MAX);
    let neg_inf = _mm512_set1_pd(f64::NEG_INFINITY);
    let hundred_v = _mm512_set1_pd(100.0);
    let thresh_v = _mm512_set1_pd(value);
    let zero_v = _mm512_setzero_pd();
    // Lane-shift permutations for the Hillis-Steele prefix max: lane i reads lane
    // i−d (lanes < d are masked to −inf, the max identity).
    let sh1 = _mm512_set_epi64(6, 5, 4, 3, 2, 1, 0, 0);
    let sh2 = _mm512_set_epi64(5, 4, 3, 2, 1, 0, 0, 0);
    let sh4 = _mm512_set_epi64(3, 2, 1, 0, 0, 0, 0, 0);

    let mut carry = entry_price;
    let mut j = start;
    while j + LANES <= n {
        let pv = _mm512_loadu_pd(price.as_ptr().add(j));
        let finite = _mm512_cmp_pd_mask::<_CMP_LE_OQ>(_mm512_abs_pd(pv), max_v);
        // Only finite prices may raise the peak — everything else becomes −inf.
        let clean = _mm512_mask_blend_pd(finite, neg_inf, pv);
        // Inclusive prefix max, 3 shift-and-max steps (no NaN survives `clean`, so
        // plain `max` is well-defined here).
        let mut pm = clean;
        pm = _mm512_max_pd(pm, _mm512_mask_permutexvar_pd(neg_inf, 0xFE, sh1, pm));
        pm = _mm512_max_pd(pm, _mm512_mask_permutexvar_pd(neg_inf, 0xFC, sh2, pm));
        pm = _mm512_max_pd(pm, _mm512_mask_permutexvar_pd(neg_inf, 0xF0, sh4, pm));
        // Fold in the peak carried out of every earlier row.
        let peak = _mm512_max_pd(pm, _mm512_set1_pd(carry));
        // retrace = (peak − p) / peak · 100, with `peak > 0 && p finite` (else NaN,
        // which satisfies nothing).
        let retrace = _mm512_mul_pd(
            _mm512_div_pd(_mm512_sub_pd(peak, pv), peak),
            hundred_v,
        );
        let peak_pos = _mm512_cmp_pd_mask::<_CMP_GT_OQ>(peak, zero_v);
        let retrace_finite = _mm512_cmp_pd_mask::<_CMP_LE_OQ>(_mm512_abs_pd(retrace), max_v);
        let mut hit = match op {
            Operator::Gt => _mm512_cmp_pd_mask::<_CMP_GT_OQ>(retrace, thresh_v),
            Operator::Gte => _mm512_cmp_pd_mask::<_CMP_GE_OQ>(retrace, thresh_v),
            Operator::Lt => _mm512_cmp_pd_mask::<_CMP_LT_OQ>(retrace, thresh_v),
            _ => _mm512_cmp_pd_mask::<_CMP_LE_OQ>(retrace, thresh_v),
        };
        hit &= finite & peak_pos & retrace_finite;
        if hit != 0 {
            return Some(j + hit.trailing_zeros() as usize);
        }
        // Carry the block's last (== whole-block) prefix max forward.
        let mut lanes = [0f64; LANES];
        _mm512_storeu_pd(lanes.as_mut_ptr(), peak);
        carry = lanes[LANES - 1];
        j += LANES;
    }
    // Tail (< 8 rows left): the shared scalar predicate, resumed at the carried peak.
    // Trough is unused for `retrace` — seed it to the fill so the ctx is well-formed.
    let mut ctx = PositionCtx::at_fill(entry_price, DateTime::UNIX_EPOCH);
    ctx.peak_price = carry;
    for (k, &p) in price.iter().enumerate().take(n).skip(j) {
        ctx.fold_price(p);
        if cmp_ordering(op, ctx.retrace(p), value) {
            return Some(k);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use hunter_engine::event::{LoadedRule, RuleId, TradeMode};
    use hunter_engine::fingerprint::FingerprintId;
    use hunter_engine::rule_params::RuleParams;
    use uuid::Uuid;

    // ── pnl-bound exit scan ────────────────────────────────────────────────────

    fn bound(op: Operator, value: f64) -> PnlBound {
        PnlBound { op, value }
    }

    /// Assert the (possibly vectorized) `first_pnl_exit_row` agrees with the scalar
    /// reference for **every** start offset — so a block-vs-remainder split at any
    /// alignment is covered.
    fn assert_agrees(price: &[f64], dead: &[bool], entry: f64, bounds: &[PnlBound]) {
        let n = price.len();
        assert_eq!(dead.len(), n, "test arrays must be parallel");
        for start in 0..=n {
            let got = first_pnl_exit_row(price, dead, start, n, entry, bounds);
            let want = first_pnl_exit_row_scalar(price, dead, start, n, entry, bounds);
            assert_eq!(
                got, want,
                "first_pnl_exit_row != scalar (start={start}, entry={entry}, \
                 bounds={bounds:?}, price={price:?}, dead={dead:?})"
            );
        }
    }

    /// Lengths straddling the 8-lane boundary: exact blocks, block+remainder, and
    /// sub-block. A cross planted at each index exercises the tzcnt first-lane pick.
    const LENS: [usize; 10] = [1, 7, 8, 9, 15, 16, 17, 24, 25, 40];

    #[test]
    fn simd_sl_cross_every_position() {
        // entry 1.0, `pnl <= −50` ⇔ price ≤ 0.5.
        let sl = [bound(Operator::Lte, -50.0)];
        for &n in &LENS {
            for cross in 0..n {
                let mut price = vec![1.0f64; n];
                price[cross] = 0.4;
                assert_agrees(&price, &vec![false; n], 1.0, &sl);
            }
        }
    }

    #[test]
    fn simd_tp_cross_every_position() {
        // entry 1.0, `pnl >= 100` ⇔ price ≥ 2.0.
        let tp = [bound(Operator::Gte, 100.0)];
        for &n in &LENS {
            for cross in 0..n {
                let mut price = vec![1.0f64; n];
                price[cross] = 2.5;
                assert_agrees(&price, &vec![false; n], 1.0, &tp);
            }
        }
    }

    #[test]
    fn simd_strict_operators_match_scalar() {
        // `>` / `<` must not fire exactly at the threshold, in vector or scalar.
        for &n in &LENS {
            for cross in 0..n {
                let mut price = vec![1.0f64; n];
                price[cross] = 2.0; // pnl == 100 exactly
                assert_agrees(&price, &vec![false; n], 1.0, &[bound(Operator::Gt, 100.0)]);
                assert_agrees(&price, &vec![false; n], 1.0, &[bound(Operator::Gte, 100.0)]);
                price[cross] = 0.5; // pnl == −50 exactly
                assert_agrees(&price, &vec![false; n], 1.0, &[bound(Operator::Lt, -50.0)]);
                assert_agrees(&price, &vec![false; n], 1.0, &[bound(Operator::Lte, -50.0)]);
            }
        }
    }

    #[test]
    fn simd_dead_every_position_beats_price() {
        // Dead has priority over a same/later price cross, and fires with no bounds.
        for &n in &LENS {
            for k in 0..n {
                let mut dead = vec![false; n];
                dead[k] = true;
                let mut price = vec![1.0f64; n];
                if k + 1 < n {
                    price[k + 1] = 0.1;
                }
                assert_agrees(&price, &dead, 1.0, &[bound(Operator::Lte, -50.0)]);
                assert_agrees(&price, &dead, 1.0, &[]); // dead-only
            }
        }
    }

    #[test]
    fn simd_both_bounds_and_no_cross() {
        let both = [bound(Operator::Lte, -50.0), bound(Operator::Gte, 100.0)];
        // No cross anywhere → None.
        assert_agrees(&vec![1.0f64; 33], &[false; 33], 1.0, &both);
        // SL and TP crosses on the same run — the earlier row wins (scalar order).
        let mut price = vec![1.0f64; 33];
        price[20] = 2.9; // TP
        price[9] = 0.2; // SL earlier → this one
        assert_agrees(&price, &[false; 33], 1.0, &both);
    }

    #[test]
    fn simd_non_finite_prices_never_cross() {
        // NaN / ±inf must be ignored exactly as `eval_one`'s non-finite guard does,
        // even though an ordered `−inf ≤ t` / `+inf ≥ t` compare would "match".
        let both = [bound(Operator::Lte, -50.0), bound(Operator::Gte, 100.0)];
        let n = 20;
        let mut price = vec![1.0f64; n];
        price[3] = f64::NAN;
        price[5] = f64::INFINITY;
        price[9] = f64::NEG_INFINITY;
        assert_agrees(&price, &vec![false; n], 1.0, &both);
        assert_eq!(first_pnl_exit_row(&price, &vec![false; n], 0, n, 1.0, &both), None);
        // Add a real finite cross after the non-finite noise: it, not the ±inf, is found.
        price[12] = 0.3;
        assert_agrees(&price, &vec![false; n], 1.0, &both);
        assert_eq!(first_pnl_exit_row(&price, &vec![false; n], 0, n, 1.0, &both), Some(12));
    }

    #[test]
    fn simd_empty_range_is_none() {
        let price = vec![1.0f64; 8];
        let dead = vec![false; 8];
        let both = [bound(Operator::Lte, -50.0), bound(Operator::Gte, 100.0)];
        assert_eq!(first_pnl_exit_row(&price, &dead, 8, 8, 1.0, &both), None);
        assert_eq!(first_pnl_exit_row(&[], &[], 0, 0, 1.0, &both), None);
    }

    // ── trailing-stop (running-peak) scan ──────────────────────────────────────

    /// Assert the (possibly vectorized) trailing scan agrees with its scalar
    /// reference for every start offset. Start matters more here than for a static
    /// threshold: the peak is seeded at `entry` and carried, so a shifted start is a
    /// genuinely different scan, not just a shifted window.
    fn assert_trailing_agrees(price: &[f64], entry: f64, op: Operator, value: f64) {
        let n = price.len();
        for start in 0..=n {
            let got = first_trailing_row_cmp(price, start, n, entry, op, value);
            let want = first_trailing_row_cmp_scalar(price, start, n, entry, op, value);
            assert_eq!(
                got, want,
                "first_trailing_row_cmp != scalar (start={start}, entry={entry}, \
                 {op:?} {value}, price={price:?})"
            );
        }
    }

    #[test]
    fn trailing_cross_every_position_after_a_run_up() {
        // Rise to a peak at `peak_at`, then dump — the retrace crossing must be found
        // at the dump row for every peak position and every array length.
        for &n in &LENS {
            for peak_at in 0..n {
                let mut price = vec![1.0f64; n];
                price[peak_at] = 4.0;
                if peak_at + 1 < n {
                    price[peak_at + 1] = 1.0; // 75% off the 4.0 peak
                }
                assert_trailing_agrees(&price, 1.0, Operator::Gte, 50.0);
                assert_trailing_agrees(&price, 1.0, Operator::Gt, 0.0);
            }
        }
    }

    #[test]
    fn trailing_peak_carries_across_block_boundaries() {
        // Peak in block 0, crossing in block 2 — only a correct carried max finds it.
        let mut price = vec![1.0f64; 24];
        price[2] = 10.0;
        price[20] = 4.0; // 60% off the 10.0 peak, two blocks later
        assert_trailing_agrees(&price, 1.0, Operator::Gte, 50.0);
        assert_eq!(
            first_trailing_row_cmp(&price, 0, price.len(), 1.0, Operator::Gte, 50.0),
            Some(3),
            "the row right after the peak is already 90% off it"
        );
    }

    #[test]
    fn trailing_seeded_peak_is_the_entry_price() {
        // Before any run-up the peak IS the fill price, so `retrace` measures the drop
        // from entry — a soft stop. Nothing here ever exceeds entry.
        let price = vec![1.0, 0.9, 0.8, 0.7, 0.6, 0.5, 0.4, 0.3, 0.2];
        assert_trailing_agrees(&price, 1.0, Operator::Gte, 25.0);
        assert_eq!(
            first_trailing_row_cmp(&price, 0, price.len(), 1.0, Operator::Gte, 25.0),
            Some(3),
            "0.7 is 30% below the 1.0 seed peak"
        );
    }

    #[test]
    fn trailing_non_finite_prices_neither_raise_the_peak_nor_fire() {
        for &n in &LENS {
            if n < 8 {
                continue;
            }
            let mut price = vec![2.0f64; n];
            price[1] = f64::INFINITY; // must NOT become the peak
            price[2] = f64::NAN; // must not fire
            price[3] = f64::NEG_INFINITY;
            price[n - 1] = 1.0; // 50% off the real 2.0 peak
            assert_trailing_agrees(&price, 2.0, Operator::Gte, 40.0);
            assert_eq!(
                first_trailing_row_cmp(&price, 0, n, 2.0, Operator::Gte, 40.0),
                Some(n - 1),
                "an +inf print must not inflate the peak into an early trigger"
            );
        }
    }

    #[test]
    fn trailing_no_cross_and_empty_range() {
        let price = vec![1.0f64; 33];
        assert_trailing_agrees(&price, 1.0, Operator::Gte, 1.0);
        assert_eq!(first_trailing_row_cmp(&price, 0, 33, 1.0, Operator::Gte, 1.0), None);
        assert_eq!(first_trailing_row_cmp(&price, 33, 33, 1.0, Operator::Gte, 1.0), None);
    }

    // ── flat-plan classification ───────────────────────────────────────────────
    //
    // A fast path that is correct but never taken rots silently, so reachability is
    // asserted, not just equality.

    fn compiled(params: serde_json::Value) -> CompiledRule {
        CompiledRule::compile(&LoadedRule {
            id: RuleId(Uuid::from_u128(1)),
            fingerprint_id: FingerprintId(Uuid::from_u128(2)),
            trade_mode: TradeMode::Paper,
            buy_amount_lamports: 1_000_000_000,
            max_concurrent_tokens: 1,
            max_total_tokens: 0,
            params: RuleParams::parse(&params).expect("valid params"),
            entry_enabled: true,
        })
    }

    fn classes(params: serde_json::Value) -> Option<Vec<ExitClass>> {
        FastPlan::of(&compiled(params)).map(|p| p.lines.iter().map(|l| l.class).collect())
    }

    fn sell(metric: &str, op: &str, value: f64) -> serde_json::Value {
        serde_json::json!({ "if": [{ "metric": metric, "is": [[{ "operator": op, "value": value }]] }], "sell": true })
    }

    #[test]
    fn tp_sl_is_a_flat_plan_of_two_pnl_bounds() {
        let c = classes(serde_json::json!({ "take_profit": 50, "stop_loss": 30 }));
        assert_eq!(
            c,
            Some(vec![ExitClass::PnlBound { up: false }, ExitClass::PnlBound { up: true }]),
            "the stop loss is the downward bound and is checked before the take profit"
        );
    }

    #[test]
    fn position_lines_classify_by_metric() {
        let c = classes(serde_json::json!({ "always": [
            sell("m_position.retrace_pct", ">=", 3.0),
            sell("m_position.bounce_pct", ">=", 15.0),
            sell("m_position.held_sec", ">=", 60.0)
        ] }));
        assert_eq!(c, Some(vec![ExitClass::Trailing, ExitClass::Bounce, ExitClass::HeldBound]));
    }

    #[test]
    fn anything_else_walks() {
        // A coin read is an arbitrary function of the series.
        assert_eq!(classes(serde_json::json!({ "always": [sell("m_price.trail_pct", ">", 50.0)] })), None);
        // `=` is a band, and an upper bound on `held` needs the opposite search.
        assert_eq!(classes(serde_json::json!({ "always": [sell("m_position.pnl_pct", "=", 10.0)] })), None);
        assert_eq!(classes(serde_json::json!({ "always": [sell("m_position.held_sec", "<=", 60.0)] })), None);
        // A partial sell, a deadline, or a second stage is not flat.
        assert_eq!(
            classes(serde_json::json!({ "stages": [{ "name": "one", "on": [{
                "if": [{ "metric": "m_position.pnl_pct", "is": [[{ "operator": ">=", "value": 50 }]] }],
                "sell": true, "sell_pct": 50, "go": "one"
            }] }] })),
            None
        );
        assert_eq!(
            classes(serde_json::json!({ "stages": [{ "name": "early", "ends": { "held_sec": 20 } }, { "name": "late" }] })),
            None
        );
    }

    #[test]
    fn stage_lines_join_the_plan_after_the_always_lines() {
        let c = classes(serde_json::json!({
            "take_profit": 50,
            "stages": [{ "name": "ride", "on": [sell("m_position.retrace_pct", ">=", 20.0)] }]
        }));
        assert_eq!(c, Some(vec![ExitClass::PnlBound { up: true }, ExitClass::Trailing]));
    }
}
