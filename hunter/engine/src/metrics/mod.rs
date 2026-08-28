//! Metrics framework — the self-describing vocabulary of the generic strategy
//! engine. Self-contained: no strategy/DB/tokio imports (parity backbone — see
//! `hunter/docs/arch/strategies.md`).
//!
//! A **metric** is a named per-token quantity a rule can put `{operator, value}`
//! conditions on. Metrics live in **groups** (one file per group):
//! * `m_state` (static) — `time`, `liquidity`,
//!   `first_slot_buy`
//! * `m_price_lifetime` (static) — `stall`, `trail`, `rise` (lifetime peak/trough)
//! * `m_price_window` (dynamic, strict param `window_size_sec`) — `trail`, `rise`
//!   (rolling-window extrema; the dip trigger)
//! * `m_flow_lifetime` (static) — `gross_flow`, `net_flow`, `buy`, `sell`
//!   (lifetime SOL totals; no classifier)
//! * `m_flow_window` (dynamic, strict param `window_size_sec`) — same metrics
//!   over a trailing window, plus `buy_count` / `sell_count` / `buy_share`, plus
//!   the two-window `trade_share` / `sol_share` (a `slice_size_sec` nested inside
//!   the window; see [`is_two_window`])
//! * `m_crowd_window` (dynamic, strict param `window_size_sec`) — `unique_wallets`,
//!   `trades_per_wallet` — the metrics that need the WALLET column
//! * `m_flow_ix` (static, fingerprint-scoped) — vol/organic lifetime totals
//! * `m_flow_ix_window` (dynamic, fingerprint-scoped) — same metrics over a window
//! * `m_position` (static, **position-scoped**, exit-only) — `retrace`, `bounce`,
//!   `pnl`, `held` (anchored on your entry fill; TP/SL desugar into `pnl` — see `arm.rs`)
//!
//! The **registry** below is the single source of truth for group/metric names,
//! units, `=`-tolerances, static/dynamic kind, monotonicity, and required strict
//! params. Params validation, the evaluator, the engine, replay, and sweep axes
//! all read it — adding a metric here (plus its compute logic in the group file)
//! makes it immediately usable everywhere, with no schema change.

pub mod evaluator;
pub mod flow_slice;
pub mod flow_lifetime;
pub mod flow_ix;
pub mod flow_window;
pub mod grid;
pub mod position;
pub mod price_lifetime;
pub mod price_window;
pub mod series;
pub mod state;
pub mod track;

use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// (De)serializes an `f64` that may carry a non-finite `NaN` sentinel ("no value
/// yet") through formats — like JSON — that have no `NaN`/`Infinity` literal.
/// A derived `Serialize` on a bare `f64` already degrades a non-finite value to
/// `null` (e.g. `serde_json`'s writer does this silently), but the matching
/// derived `Deserialize` for a bare (non-`Option`) `f64` then rejects that same
/// `null` — a write-only asymmetry. [`TradeLite::reserve_sol`] uses `NaN` as its
/// "no real reserve decoded yet" sentinel (see [`crate::metrics::state`]), so
/// any event-log `Trade` line logged with that sentinel became permanently
/// unparseable (`invalid type: null, expected f64`) on replay/recovery. This
/// module makes the round trip explicit instead of widening the field to
/// `Option<f64>`, which would ripple the sentinel-vs-`None` distinction through
/// `track`/`snapshot`/`series`/sweep for no behavioral gain.
mod finite_f64 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
        if v.is_finite() {
            s.serialize_f64(*v)
        } else {
            s.serialize_none()
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
        Ok(Option::<f64>::deserialize(d)?.unwrap_or(f64::NAN))
    }

    /// `#[serde(default)]` for a field whose "unknown" is `NaN` — an event-log line
    /// written before the field existed deserializes to "depth unknown", not to `0.0`
    /// (which the cost model would read as a real, infinitely thin pool).
    pub fn nan() -> f64 {
        f64::NAN
    }
}

/// Every timestamp in the engine arrives on an event — the engine never reads a
/// clock (purity). `Ts` is that carried instant.
pub type Ts = DateTime<Utc>;

/// Whole seconds (as `f64`, sub-second precision preserved to the millisecond)
/// elapsed from `from` to `to`. The one place metric compute turns two instants
/// into a duration, so every group measures time identically.
pub fn secs_between(from: Ts, to: Ts) -> f64 {
    to.signed_duration_since(from).num_milliseconds() as f64 / 1000.0
}

/// A trade's direction. Buys add SOL to the curve; sells remove it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Buy,
    Sell,
}

/// The minimal per-trade fact the metrics need — the engine's `Trade` event
/// carries one of these. `sol` is the trade's absolute SOL notional (`>= 0`);
/// direction lives in `side`. `price` is the canonical curve-spot price and
/// `reserve_sol` the SOL reserves after the trade (liquidity).
///
/// `ix_hash` / `wallet_hash` feed the volume-flow classifier (V1+); adapters hash
/// via [`flow_ix`]. Missing fields on old event-log lines default via serde
/// (`ix_hash: None`, `wallet_hash: 0`) ⇒ organic unless tagged/creator.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TradeLite {
    pub side: Side,
    pub sol: f64,
    pub price: f64,
    /// May be `NaN` ("no real reserve decoded yet") — round-trips through JSON
    /// via [`finite_f64`], which maps `NaN <-> null` explicitly so an event-log
    /// line carrying the sentinel stays parseable (plain-`f64` derive isn't
    /// symmetric: it serializes `NaN` as `null` but fails to deserialize `null`).
    #[serde(with = "finite_f64")]
    pub reserve_sol: f64,
    /// SOL-side depth to charge **price impact** against — the *priced* reserve
    /// (`vsol`), not the real one.
    ///
    /// On a constant-product curve, spending `B` pays an average price of
    /// `(vsol + B) / vtok`, exactly `1 + B/vsol` times the pre-trade spot, so impact
    /// is `B / vsol`. [`reserve_sol`](Self::reserve_sol) is the **real** reserve
    /// (`vsol - PUMP_INITIAL_VIRTUAL_SOL` on the curve) because the `liquidity` metric
    /// and the deadness verdict both mean real deposited SOL. Charging impact against
    /// that overcharges by `vsol / (vsol - 30)` — 1.6x at `liquidity 50`, 11x at
    /// `liquidity 3`.
    ///
    /// This is carried rather than re-derived because the real reserve is **clamped at
    /// zero**, so `real -> priced` is not invertible exactly where the pool is thinnest
    /// and the error is largest. On the AMM the two are equal.
    ///
    /// `NaN` ⇒ depth unknown, and the cost model then charges no impact — never a guess.
    #[serde(with = "finite_f64", default = "finite_f64::nan")]
    pub priced_reserve_sol: f64,
    pub at: Ts,
    /// FNV-1a of the trade's ordered `ix_labels`; `None` when labels are absent.
    #[serde(default)]
    pub ix_hash: Option<u64>,
    /// FNV-1a of the trade's wallet address.
    #[serde(default)]
    pub wallet_hash: u64,
    /// The slot this trade landed in — the cursor every [`WindowUnit::Slot`] window
    /// counts in.
    ///
    /// `0` means "not supplied" (pre-slot event-log lines, a lake load without the
    /// column). A slot window cannot advance on those, which
    /// [`crate::arm::CompiledRule::needs_slot`] exists to make loud rather than
    /// silent — the same class of trap as the wallet-keyed metrics.
    #[serde(default)]
    pub slot: u64,
    /// Structural markers present in the trade's `ix_labels`, one bit each
    /// ([`crate::metrics::flow_ix::MARKERS`]). Set by the producer, which is the
    /// only layer holding the label strings; the engine compares bits.
    #[serde(default)]
    pub marker_bits: u16,
}

impl Default for TradeLite {
    fn default() -> Self {
        Self {
            side: Side::Buy,
            sol: 0.0,
            price: 0.0,
            reserve_sol: 0.0,
            priced_reserve_sol: f64::NAN,
            at: DateTime::from_timestamp(0, 0).expect("unix epoch"),
            ix_hash: None,
            wallet_hash: 0,
            slot: 0,
            marker_bits: 0,
        }
    }
}

// ── Window spans: a size, a lag, and the unit both are counted in ────────────

/// What a dynamic group's window counts in.
///
/// **Time is continuous, slots and prints are discrete**, and the three spans are
/// deliberately not the same shape:
///
/// * [`Sec`](Self::Sec) — `size` seconds of wall clock, a closed interval.
/// * [`Slot`](Self::Slot) — exactly `size` slots, discrete buckets.
/// * [`Print`](Self::Print) — exactly `size` prints of THIS token's tape.
///
/// A slot is what the chain actually batches in, so a bundle is a slot fact and
/// never a time fact: at ~400 ms a one-second window straddles two or three slots
/// and merges bursts that landed separately.
///
/// A print is what the tape itself batches in. Both clocks answer "how much SOL
/// moved" with a number that a busy tape and a quiet one reach differently: `10`
/// over one second is ten one-SOL prints or one ten-SOL print, and no wall-clock or
/// slot span can tell them apart. `size: 1, lag: 0` on a print window is **one
/// transaction**, which is the only span in which "10 SOL in one trade" is a
/// statement about a trade rather than about an interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowUnit {
    Sec,
    Slot,
    Print,
}

impl WindowUnit {
    /// Every unit, in resolution order. The one place a basis is enumerated: a
    /// window axis lists its size params from this, `validate_group` counts the ones
    /// a bag sets from this, and the frontend mirrors the same order.
    pub const ALL: [WindowUnit; 3] = [Self::Sec, Self::Slot, Self::Print];

    /// The JSON key a rule spells this unit's size on the group's OWN axis. The
    /// second axis of a two-window group has its own names — see [`WindowAxis`].
    pub const fn size_param(self) -> &'static str {
        WINDOW_AXIS.size_param(self)
    }


    /// The short suffix a window in this unit labels itself with: `30s`, `30sl`,
    /// `30p`. The ONE spelling — persisted exit reasons, live chips, chart legends
    /// and the search's ablation rows all render through it, so a label parsed back
    /// by `event::split_window_qualifier` means what it printed. `sl` must stay
    /// distinguishable from `s` by its suffix alone; the parser strips the longer
    /// one first.
    pub const fn suffix(self) -> &'static str {
        match self {
            Self::Sec => "s",
            Self::Slot => "sl",
            Self::Print => "p",
        }
    }
}

/// The size params ONE window axis spells itself with — one name per
/// [`WindowUnit`]. A dynamic group's own span is [`WINDOW_AXIS`]; the nested slice
/// the two-window metrics read is [`flow_slice::SLICE_AXIS`].
///
/// Exists so "which param carries this axis" is asked once instead of branching per
/// pair of units at every resolve, validate and label site — that branching is what
/// made a third basis a rewrite instead of an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowAxis {
    pub sec: &'static str,
    pub slot: &'static str,
    pub print: &'static str,
}

impl WindowAxis {
    /// This axis's size param for one unit.
    pub const fn size_param(&self, unit: WindowUnit) -> &'static str {
        match unit {
            WindowUnit::Sec => self.sec,
            WindowUnit::Slot => self.slot,
            WindowUnit::Print => self.print,
        }
    }

    /// Every size param of this axis, in [`WindowUnit::ALL`] order. The list a
    /// registry declares and a validator counts set keys against.
    pub const fn params(&self) -> [&'static str; 3] {
        [self.sec, self.slot, self.print]
    }
}

/// The reference axis every dynamic group carries.
pub const WINDOW_AXIS: WindowAxis =
    WindowAxis { sec: WINDOW_SEC_PARAM, slot: WINDOW_SLOT_PARAM, print: WINDOW_PRINT_PARAM };

/// Nominal seconds per slot. Used in exactly one place - sizing the tick-grid
/// horizon for a slot window, where the grid is a wall clock and the span is not.
/// It never enters a metric reading: a slot window's cursor is the slot number the
/// feed reports, never a time estimate.
pub const NOMINAL_SLOT_SECS: f64 = 0.4;

/// The size param for a wall-clock window. Unchanged from before slots existed, so
/// every stored rule round-trips byte-identically.
pub const WINDOW_SEC_PARAM: &str = "window_size_sec";
/// The size param for a slot window. Mutually exclusive with [`WINDOW_SEC_PARAM`].
pub const WINDOW_SLOT_PARAM: &str = "window_size_slots";
/// The size param for a print window — `size` prints of the token's own tape.
/// Mutually exclusive with the other two.
pub const WINDOW_PRINT_PARAM: &str = "window_size_prints";
/// How many units back from *now* the window ends. `0` (the default, and the only
/// value before this param existed) means it ends at now.
pub const WINDOW_LAG_PARAM: &str = "window_lag";

/// One dynamic group's window: how wide, how far back it ends, and in what unit.
///
/// `lag` is what makes a window **causal in its own terms**. A gate on "the state
/// entering this slot" must not be able to see the slot it is firing in, and
/// `lag: 1` on a slot window is exactly that: the burst is
/// `slots: 1, lag: 0` and the quiet tape before it is `slots: 30, lag: 1`, with no
/// arithmetic between windows and no way for one to leak into the other.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowSpec {
    pub size: f64,
    pub lag: f64,
    pub unit: WindowUnit,
}

impl WindowSpec {
    /// A wall-clock window ending at now — the shape every pre-slot rule has.
    pub fn secs(size: f64) -> Self {
        Self { size, lag: 0.0, unit: WindowUnit::Sec }
    }

    /// A slot window of `size` slots ending `lag` slots before now.
    pub fn slots(size: f64, lag: f64) -> Self {
        Self { size, lag, unit: WindowUnit::Slot }
    }

    /// A print window of `size` prints ending `lag` prints before now.
    /// `prints(1.0, 0.0)` is the current transaction alone.
    pub fn prints(size: f64, lag: f64) -> Self {
        Self { size, lag, unit: WindowUnit::Print }
    }

    /// Dedup identity. Two rules asking for the same span share one buffer; a
    /// 30-second and a 30-slot window are different buffers, as they must be.
    pub fn key(&self) -> WindowKey {
        WindowKey {
            unit: self.unit,
            size: quantize(self.size),
            lag: quantize(self.lag),
        }
    }

    /// The span, named: `30s`, `30sl`, `20p`, `30sl@1`.
    ///
    /// **The one spelling of a window**, and the inverse of [`parse`](Self::parse).
    /// A persisted exit reason, a live chip, a chart legend and a `?windows=` query
    /// all carry this string, so a span that round-trips here reads the same
    /// everywhere. The `@lag` half appears only when there IS a lag: a lagged window
    /// reads a DIFFERENT span from an unlagged one of the same size, and the two must
    /// never print identically.
    pub fn label(&self) -> String {
        let lag = if self.lag > 0.0 {
            format!("@{}", crate::event::format_metric_threshold(self.lag))
        } else {
            String::new()
        };
        format!(
            "{}{}{lag}",
            crate::event::format_metric_threshold(self.size),
            self.unit.suffix()
        )
    }

    /// Parse a span written by [`label`](Self::label). `None` on anything malformed —
    /// a caller must not silently read an unrecognised qualifier as a bare number,
    /// which is how `30sl` would become a 30-SECOND window.
    ///
    /// A bare number is seconds, so every span written before the other bases existed
    /// still parses to exactly what it always meant.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let (head, lag) = match s.split_once('@') {
            Some((head, l)) => (head, l.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0)?),
            None => (s, 0.0),
        };
        // Longest suffix first, or `sl` parses as a seconds span with a stray `l`.
        // A bare number falls through to `Sec`, which is the pre-basis spelling.
        let mut unit = WindowUnit::Sec;
        let mut size_str = head;
        for u in [WindowUnit::Slot, WindowUnit::Print, WindowUnit::Sec] {
            if let Some(rest) = head.strip_suffix(u.suffix()) {
                (unit, size_str) = (u, rest);
                break;
            }
        }
        let size = size_str.trim().parse::<f64>().ok()?;
        (size.is_finite() && size > 0.0).then_some(Self { size, lag, unit })
    }

    /// Where a point on the tape sits on this window's axis. `cur` is the cursor
    /// read for that point: a trade's own cursor when folding it, the token's
    /// current one when reading.
    pub fn pos(&self, at: Ts, cur: Cursor) -> i64 {
        match self.unit {
            WindowUnit::Sec => at.timestamp_millis(),
            WindowUnit::Slot => cur.slot as i64,
            WindowUnit::Print => cur.print as i64,
        }
    }

    /// Where *now* sits on this window's axis. `cur` is the token's current cursor —
    /// a discrete axis has no clock of its own, so it holds its last reading until a
    /// trade moves it.
    pub fn now_pos(&self, now: Ts, cur: Cursor) -> i64 {
        self.pos(now, cur)
    }

    /// Inclusive `[lo, hi]` bounds at `now_pos`.
    ///
    /// * `Sec` — `[now - lag - size, now - lag]` in milliseconds, so `lag: 0` is
    ///   byte-for-byte the old `[now - w, now]`.
    /// * `Slot` / `Print` — `[now - lag - (size-1), now - lag]`, exactly `size`
    ///   buckets, so `size: 1, lag: 0` is the current slot / the current print
    ///   alone. One arithmetic for both because a discrete cursor is a discrete
    ///   cursor; what differs between them is what advances it, not how it is
    ///   sliced.
    pub fn bounds(&self, now_pos: i64) -> (i64, i64) {
        match self.unit {
            WindowUnit::Sec => {
                let hi = now_pos - quantize(self.lag) as i64;
                (hi - quantize(self.size) as i64, hi)
            }
            WindowUnit::Slot | WindowUnit::Print => {
                let hi = now_pos - self.lag.max(0.0).round() as i64;
                (hi - (self.size.max(1.0).round() as i64 - 1), hi)
            }
        }
    }
}

/// Where a token stands on every DISCRETE window axis at once — the counters a
/// clock cannot supply.
///
/// One value rather than two arguments, so a call site cannot silently pass a slot
/// where a print ordinal belongs, and so a fourth discrete basis is a field rather
/// than a signature change at every fold and read site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cursor {
    /// Highest slot the token has observed. `0` before the first trade, or when no
    /// adapter supplies slots.
    pub slot: u64,
    /// How many prints the token has taken, counting the one being folded. `0`
    /// before the first trade, so `size: 1, lag: 0` reads an empty window rather
    /// than a phantom one.
    pub print: u64,
}

impl Cursor {
    /// The cursor of one trade being folded into the token whose current cursor is
    /// `self`: the trade's OWN slot (which may lag the token's on a regressed feed
    /// row) at the token's current print ordinal — the trade being folded IS that
    /// print, so on the print axis a trade always sits at `now`.
    pub fn at_trade(self, t: &TradeLite) -> Self {
        Self { slot: t.slot, print: self.print }
    }
}

/// Millisecond-resolution integer identity for a window size or lag. Sizes come
/// from rule params (finite, `>= 0`), so rounding gives a stable key two rules
/// requesting the same span collapse onto.
pub fn quantize(v: f64) -> u64 {
    (v * 1000.0).round().max(0.0) as u64
}

/// Dedup key for a [`WindowSpec`] — the map key on `TokenTrack`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowKey {
    pub unit: WindowUnit,
    pub size: u64,
    pub lag: u64,
}

/// A metric group — one compute module, one JSON key under `entry`/`exit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MetricGroupId {
    /// `m_state` — instantaneous token state.
    State,
    /// `m_price_lifetime` — incremental price-path state (lifetime peak).
    PriceLifetime,
    /// `m_price_window` — trailing-window price extrema (rolling high/low).
    PriceWindow,
    /// `m_flow_lifetime` — lifetime flow aggregates (no classifier).
    FlowLifetime,
    /// `m_flow_window` — trailing-window flow aggregates.
    FlowWindow,
    /// `m_crowd_window` — trailing-window wallet counts.
    CrowdWindow,
    /// `m_flow_ix` — volume/organic lifetime totals (fingerprint-scoped).
    FlowIx,
    /// `m_flow_ix_window` — volume/organic trailing-window totals (fingerprint-scoped).
    FlowIxWindow,
    /// `m_position` — metrics anchored on YOUR entry fill (position-scoped, exit-only).
    Position,
}

impl MetricGroupId {
    pub fn name(self) -> &'static str {
        group_spec(self).name
    }
}

impl fmt::Display for MetricGroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One metric within a group — a JSON key holding `{operator, value}` lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MetricId {
    /// Seconds since token creation (`m_state`).
    Time,
    /// SOL reserves (`m_state`).
    Liquidity,
    /// Total buy SOL that landed in the token's **creation slot** (`m_state`).
    ///
    /// "Was the launch real?" — it separates a funded launch from a dust one, and it
    /// is the same quantity the fingerprint axis `first_slot_buy_lamports` ranges
    /// over. Prefer the axis for a launch-shape THRESHOLD — it selects the token set
    /// before any evaluation. This metric is for a condition that has to be read
    /// alongside live tape state in the same rule.
    ///
    /// Static once seeded, so an entry gate on it is a token filter that can never
    /// re-trigger. `NaN` until the creation slot settles — which is later than
    /// `TokenCreated`, because the number is summed from that slot's trades. A rule
    /// using it therefore cannot fire at birth; that is the fact, not a limitation.
    FirstSlotBuy,
    /// Seconds since the price last set a **new all-time high** (`m_price_lifetime`)
    /// — NOT "since the last trade". Only a strictly higher price resets the clock,
    /// so on a token trading actively below its peak `stall` keeps climbing. Read
    /// [`price_lifetime`] before using it: as an **exit** it caps position lifetime
    /// (and, through `can_enter`, doubles as an entry filter), which is a trap for a
    /// dip-entry rule that is below its high by construction.
    Stall,
    /// Percent off the peak price (`m_price_lifetime`).
    Trail,
    /// Percent above the lifetime trough (`m_price_lifetime`).
    LifeRise,
    /// Percent below the rolling-window high (`m_price_window`) — the dip trigger.
    WinTrail,
    /// Percent above the rolling-window low (`m_price_window`).
    WinRise,
    // ── m_flow_lifetime (lifetime; JSON names shared with m_flow_window) ─
    /// Buy + sell SOL since token birth (`m_flow_lifetime`).
    LifeGrossFlow,
    /// Buy − sell SOL since token birth (`m_flow_lifetime`).
    LifeNetFlow,
    /// Buy SOL since token birth (`m_flow_lifetime`).
    LifeBuy,
    /// Sell SOL since token birth (`m_flow_lifetime`).
    LifeSell,
    // ── m_flow_window (trailing) ─
    /// Buy + sell SOL over the trailing window (`m_flow_window`).
    GrossFlow,
    /// Buy − sell SOL over the trailing window (`m_flow_window`).
    NetFlow,
    /// Buy SOL over the trailing window (`m_flow_window`).
    Buy,
    /// Sell SOL over the trailing window (`m_flow_window`).
    Sell,
    /// Distinct trading wallets over the trailing window (`m_flow_window`) — how many
    /// people are in the token, as against `gross_flow`'s how much SOL. One wallet
    /// churning and a crowd arriving look identical in SOL and different here.
    UniqueWallets,
    /// Trades since the token's first (`m_flow_lifetime`) — the lifetime twin of
    /// [`TradeCount`], and the maturity counter to `LifeGrossFlow`'s volume.
    ///
    /// **Monotonic**, so an entry UPPER bound on it (`<= 140`) is a one-way door: once a
    /// token crosses it the requirement can never come back, and the arm is disarmed as
    /// unsatisfiable rather than re-checked for the rest of the token's life.
    ///
    /// A trade dropped by the non-finite/negative SOL guard is not counted, matching the
    /// window sibling on the same tape.
    LifeTradeCount,
    /// Trades over the trailing window (`m_flow_window`) — how BUSY the tape is, as
    /// against `unique_wallets`' how many people are on it. One wallet re-entering ten
    /// times reads 10 here and 1 there.
    ///
    /// Unlike `unique_wallets` this needs no wallet column, so it survives an offline
    /// load that did not request wallet identity (see [`needs_wallet_identity`]).
    TradeCount,
    /// Buys (not trades) in the window - the slice's transaction count. Distinct
    /// from `trade_count`, which sells inflate.
    BuyCount,
    /// Sells (not trades) in the window (`m_flow_window`) — `buy_count`'s twin.
    ///
    /// Registered rather than left to arithmetic because a condition cannot do any:
    /// `trade_count - buy_count` is not expressible, so without this metric "at most
    /// two sells" has no spelling at all.
    SellCount,
    /// Share of the trailing window's SOL that is buys — `buy / (buy + sell)`, in
    /// percent (`m_flow_window`).
    ///
    /// The DIRECTION of the tape, independent of its size. `net_flow` conflates the
    /// two: +5 SOL net is a different situation on 6 SOL of turnover than on 200.
    /// Reads high when one side is being absorbed rather than matched.
    ///
    /// `NaN` on an empty window — no flow, no direction to report, and a `NaN`
    /// satisfies no condition (evaluator contract).
    BuyShare,
    /// Trades per distinct wallet over the trailing window —
    /// `trade_count / unique_wallets` (`m_flow_window`).
    ///
    /// How hard each wallet is working the tape. `<= 2` is a crowd arriving; a large
    /// value is one wallet re-entering, which `trade_count` and `gross_flow` cannot
    /// tell apart. It is a **count ratio, never an identity**, so it survives the
    /// wallet rotation that makes identity useless on this chain.
    ///
    /// `NaN` on an empty window rather than `0.0` — a `0.0` would let
    /// `trades_per_wallet <= 2` pass on a dead tape, the exact reading it excludes.
    TradesPerWallet,
    // ── m_flow_window, TWO-window reads (a slice nested in the window) ─
    /// Percent of the reference window's trades that landed in the slice window
    /// nested inside it — `trade_count(slice) / trade_count(window) * 100`
    /// (`m_flow_window`).
    ///
    /// How CONCENTRATED the tape is in time, independent of how busy it is. Ten
    /// trades arriving in the last three seconds and ten spread evenly over a minute
    /// are the same `trade_count` and the same `gross_flow`, and 50 vs 10 here.
    ///
    /// `NaN` on an empty reference window. Reads `100` on a token younger than the
    /// burst window, which is a true reading and not a maturity signal — see
    /// [`flow_slice::trade_share`].
    SliceTradeShare,
    /// Percent of the reference window's SOL that moved in the slice nested inside
    /// it — `gross_flow(slice) / gross_flow(window) * 100` (`m_flow_window`).
    ///
    /// The SOL twin of [`SliceTradeShare`], and not a restatement of it: ten prints
    /// carrying a tenth of a SOL each and one print carrying ten are the same
    /// `trade_share` and far apart here.
    ///
    /// It is also the reading that survives a PRINT basis. On a print window the
    /// slice is a fixed count of transactions, so `trade_share` is `slice / window`
    /// on every token and carries nothing; `sol_share` is the only one of the pair
    /// that still varies.
    ///
    /// `NaN` on an empty reference window, same contract as [`SliceTradeShare`].
    SliceSolShare,
    // ── m_flow_ix (lifetime; JSON names shared with m_flow_ix_window) ─
    TaggedBuy,
    TaggedSell,
    TaggedNet,
    TaggedGross,
    UntaggedBuy,
    UntaggedSell,
    UntaggedNet,
    UntaggedGross,
    TaggedShare,
    // ── m_flow_ix_window (trailing; distinct ids so monotonic flags can differ) ─
    WinTaggedBuy,
    WinTaggedSell,
    WinTaggedNet,
    WinTaggedGross,
    WinUntaggedBuy,
    WinUntaggedSell,
    WinUntaggedNet,
    WinUntaggedGross,
    WinTaggedShare,
    // ── m_position (position-scoped; anchored on the entry fill; exit-only) ──
    /// Percent below the since-entry peak — the trailing stop.
    Retrace,
    /// Percent above the since-entry trough — the bounce twin of `retrace`.
    Bounce,
    /// Signed percent vs the entry price.
    Pnl,
    /// Seconds since the entry fill.
    Held,
}

/// True for metrics whose state is keyed by fingerprint (flow split / window).
pub fn is_flow_metric(id: MetricId) -> bool {
    matches!(
        group_of(id).id,
        MetricGroupId::FlowIx | MetricGroupId::FlowIxWindow
    )
}

/// True for the metrics whose basis is TWO nested windows — a slice inside the
/// group's own window — rather than one.
///
/// Per-METRIC, not per-group, and that is the whole point of it. The slice axis
/// lives on `m_flow_window` beside the single-window metrics, so an instance that
/// gates only on `gross_flow` must not be made to carry a span nothing reads, and an
/// instance that gates on `trade_share` must not be allowed to omit it. `validate_group`
/// reads this to decide which of those two errors to raise, and `arm::build_reqs`
/// reads it to attach [`Windows::secondary`] to these metrics alone — so a
/// requirement's identity carries the second span exactly when the second span is
/// what it reads.
pub fn is_two_window(id: MetricId) -> bool {
    matches!(id, MetricId::SliceTradeShare | MetricId::SliceSolShare)
}

/// True for metrics whose **state** is keyed by fingerprint, and which therefore
/// need a `FingerprintId` to read a value.
///
/// Identical to [`is_flow_metric`] while flow split is the only fingerprint-scoped
/// family, and kept separate because the two mean different things: this one asks
/// "does the read need a fingerprint", while [`is_flow_metric`] additionally selects
/// a *flow* series column offline. A future fingerprint-scoped group that is not a
/// flow group belongs here and NOT there — conflating them routes its reads at a flow
/// column.
pub fn is_fingerprint_scoped(id: MetricId) -> bool {
    is_flow_metric(id)
}


impl MetricId {
    pub fn name(self) -> &'static str {
        metric_spec(self).name
    }

    /// Whether this metric's value depends on **who** traded, not just how much.
    ///
    /// Offline that is a load-time question, not a fold-time one: the lake leaves the
    /// `wallet` / `ix_labels` columns out unless a run asks for them, and a fold over
    /// rows without them sees every trade as one anonymous wallet. The failure is
    /// silent and reads like a strict gate — `unique_wallets >= 10` simply never fires
    /// — so the answer lives here, next to the registry, rather than as a group list
    /// copied into each loader. A new wallet-keyed metric must be added here too.
    pub fn needs_wallet_identity(self) -> bool {
        is_flow_metric(self)
            || matches!(self, MetricId::UniqueWallets | MetricId::TradesPerWallet)
    }

}

impl fmt::Display for MetricId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Unit a metric's values (and its condition values) are expressed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Seconds,
    Sol,
    Percent,
    /// A dimensionless tally (wallets, trades). Renders bare — no suffix — because a
    /// count with a unit glyph reads as a quantity of something else.
    Count,
}

impl Unit {
    /// Stable JSON/label token (frontend registry contract).
    pub fn as_str(self) -> &'static str {
        match self {
            Unit::Seconds => "seconds",
            Unit::Sol => "sol",
            Unit::Percent => "percent",
            Unit::Count => "count",
        }
    }
}

/// Whether a group's metrics are rule-independent (one value per token) or need
/// per-rule strict params (deduped by those params across rules).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricKind {
    Static,
    Dynamic,
}

impl MetricKind {
    /// Stable JSON/label token (frontend registry contract).
    pub fn as_str(self) -> &'static str {
        match self {
            MetricKind::Static => "static",
            MetricKind::Dynamic => "dynamic",
        }
    }
}

/// What a group's metric state **anchors on**. Everything token-scoped is one value
/// per token, shared by every rule armed on it. A position-scoped metric anchors on
/// *your* entry fill, so it only has a value while a position is held — it is
/// **exit-only** (validation rejects it under `entry`, and with no position context
/// it reads `NaN`, satisfying nothing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricScope {
    Token,
    Position,
}

impl MetricScope {
    /// Stable JSON/label token (frontend registry contract).
    pub fn as_str(self) -> &'static str {
        match self {
            MetricScope::Token => "token",
            MetricScope::Position => "position",
        }
    }
}

/// Registry entry for one metric: name, unit, `=`-tolerance, monotonicity, UI hue.
///
/// `eq_tolerance` is the metric's own bucket-equality width for `=`/`!=`
/// (deliberately independent of any fingerprint criterion's range).
/// `monotonic` (non-decreasing over a token's life) powers derived
/// unsatisfiability disarm — an entry upper bound on a monotonic metric that is
/// permanently crossed can never re-satisfy.
/// `hue` is the SSOT UI color (HSL degrees 0..359) for badges / axis tints —
/// metrics in the same group share a nearby hue family; the frontend applies a
/// fixed per-operator shade offset on top.
#[derive(Debug, Clone, Copy)]
pub struct MetricSpec {
    pub id: MetricId,
    pub name: &'static str,
    /// **The** definition of this metric: what it measures, plus any NaN or basis rule a
    /// rule author has to know. One or two sentences, written HERE and rendered into the
    /// UI from this text — a metric carries one definition and it lives where the metric
    /// is defined, so a tooltip can never say something the code does not. Unit, `=`
    /// tolerance and monotonicity are their own registry fields and the UI appends them,
    /// so this must not restate them.
    ///
    /// Longer prose — worked examples, reading guides, refutations — may still live in the
    /// frontend's `strategyHelp.ts`, but only BELOW this and never as a second definition.
    pub description: &'static str,
    pub unit: Unit,
    pub eq_tolerance: f64,
    pub monotonic: bool,
    /// HSL hue in degrees `[0, 359]`. Group siblings stay in the same family.
    pub hue: u16,
}

/// A strict (non-condition) group parameter, e.g. `m_flow_window`'s
/// `window_size_sec`. Values must be finite and `> 0` unless
/// [`allows_zero`](Self::allows_zero).
#[derive(Debug, Clone, Copy)]
pub struct StrictParamSpec {
    pub name: &'static str,
    pub required: bool,
    /// Whether `0` is a legal **value** of this param's domain (`>= 0` instead of
    /// `> 0`). This is NOT the "zero-as-unbound" sentinel — an *absent* optional
    /// param is what means "off". `m_position.arm_above_pct = 0` is a real setting
    /// (arm the trailing stop the moment the position is green) and must not be
    /// confused with the param being unset, so the two stay distinguishable:
    /// `None` ⇒ feature off, `Some(0.0)` ⇒ arm at break-even.
    pub allows_zero: bool,
}

/// A fingerprint-side config field declared by a metric group (stored under
/// `fingerprints.metric_config[group_name]`). Mirrored into `registry_json` so
/// the FE can render editors without hardcoding keys.
#[derive(Debug, Clone, Copy)]
pub struct FpConfigFieldSpec {
    pub name: &'static str,
    /// JSON type hint for the FE (`"string[][]"` for ordered label sequences).
    pub value_type: &'static str,
    pub required: bool,
}

/// The **interaction family** a group belongs to.
///
/// Metrics interact strongly *within* a family (they are different views of the same
/// underlying quantity) and largely compose *across* families. The discovery
/// pipeline's Layer 2 grids per family and then measures whether two families
/// actually interact, instead of paying for one blind cross-product
/// (see `hunter/docs/arch/sweep.md` "Metric-combo discovery pipeline"). The grouping mirrors the hue
/// families the registry already keeps — promoted from a color convention to a real
/// field so the pipeline reads it as data.
///
/// A newly registered group that fits none of these declares [`Standalone`]: it is
/// gridded alone and interaction-checked against everything else, which costs compute
/// but is never wrong.
///
/// [`Standalone`]: MetricFamily::Standalone
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MetricFamily {
    /// The price path — lifetime extrema, rolling-window extrema, since-entry
    /// extrema. One path, three views.
    Price,
    /// Unclassified SOL flow — lifetime and trailing-window aggregates.
    Flow,
    /// Flow split by a **wallet classifier** — volume vs organic, lifetime and windowed.
    FlowIx,
    /// Token state that is neither price nor flow: age and liquidity.
    LiquidityAge,
    /// Default for a group that belongs to no established family.
    Standalone,
}

impl MetricFamily {
    /// Stable JSON/label token (frontend registry contract).
    pub fn as_str(self) -> &'static str {
        match self {
            MetricFamily::Price => "price",
            MetricFamily::Flow => "flow",
            MetricFamily::FlowIx => "flow_ix",
            MetricFamily::LiquidityAge => "liquidity_age",
            MetricFamily::Standalone => "standalone",
        }
    }
}

/// Registry entry for one metric group.
#[derive(Debug, Clone, Copy)]
pub struct GroupSpec {
    pub id: MetricGroupId,
    pub name: &'static str,
    pub kind: MetricKind,
    /// Token-scoped (default) or position-scoped (`m_position`, exit-only).
    pub scope: MetricScope,
    /// Which metrics this group is expected to *interact* with (discovery Layer 2).
    pub family: MetricFamily,
    pub strict_params: &'static [StrictParamSpec],
    /// Fingerprint-side config fields (empty for most groups).
    pub fingerprint_config: &'static [FpConfigFieldSpec],
    pub metrics: &'static [MetricSpec],
}

impl GroupSpec {
    /// Resolve a metric of this group by its JSON name.
    pub fn metric_by_name(&self, name: &str) -> Option<&'static MetricSpec> {
        self.metrics.iter().find(|m| m.name == name)
    }

    /// Resolve a strict param of this group by its JSON name.
    pub fn strict_param_by_name(&self, name: &str) -> Option<&'static StrictParamSpec> {
        self.strict_params.iter().find(|p| p.name == name)
    }
}

/// Hue of the frontend's candle **up** color (`#089981` ⇒ HSL hue 170).
///
/// Duplicated across the language boundary — the hex itself lives in
/// `hunter/frontend/src/shared/components/token-price-chart/constants.ts`
/// (`CHART_COLORS.up`) and `--color-green` in `index.css`. Kept in sync by the
/// `direction_metrics_match_candle_hues` guard test below; if you change the hex
/// there, change the hue here.
pub const CANDLE_UP_HUE: u16 = 170;

/// Hue of the frontend's candle **down** color (`#f23645` ⇒ HSL hue 355).
/// See [`CANDLE_UP_HUE`] for the sync contract.
pub const CANDLE_DOWN_HUE: u16 = 355;

/// Which trailing window(s) a metric read is scoped to.
///
/// One value rather than a loose `Option<f64>` argument, for two reasons. It keeps
/// the hot-path read signature stable as bases grow, and — the load-bearing one — it
/// makes the window part of a requirement's **identity** automatic: blockers and
/// monotonic kills compare reqs by `(metric, windows, fingerprint)`, so a group whose
/// basis is two nested windows cannot collide with a sibling instance that differs
/// only in the second one.
///
/// `primary` is the group's `window_size_sec`. `secondary` is `None` for every group
/// whose basis is a single window — every read but the two-window metrics
/// [`is_two_window`] selects.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Windows {
    pub primary: Option<WindowSpec>,
    pub secondary: Option<WindowSpec>,
}

impl Windows {
    /// A static group's read — no window on either axis.
    pub const NONE: Self = Self { primary: None, secondary: None };

    /// A single-window read (the group's own span).
    pub fn one(spec: WindowSpec) -> Self {
        Self { primary: Some(spec), secondary: None }
    }

    /// A single wall-clock window - the shape every pre-slot caller means.
    pub fn secs(size: f64) -> Self {
        Self::one(WindowSpec::secs(size))
    }

    /// A two-window read: the group's own window plus its second axis.
    pub fn two(primary: WindowSpec, secondary: WindowSpec) -> Self {
        Self { primary: Some(primary), secondary: Some(secondary) }
    }

    /// True when either axis counts in slots - the flag a loader must respect,
    /// since a lake read without the slot column cannot advance such a window.
    pub fn needs_slot(self) -> bool {
        [self.primary, self.secondary]
            .into_iter()
            .flatten()
            .any(|w| w.unit == WindowUnit::Slot)
    }

    /// Whether this read is scoped to a trailing window at all — i.e. it is a dynamic
    /// group's read, not a static one. Tests BOTH axes: a two-window group is windowed
    /// even where only its second axis is set, and a caller asking "is this a rate-1
    /// clock?" must not answer yes to one.
    pub fn is_windowed(self) -> bool {
        self.primary.is_some() || self.secondary.is_some()
    }
}

impl From<Option<WindowSpec>> for Windows {
    fn from(primary: Option<WindowSpec>) -> Self {
        Self { primary, secondary: None }
    }
}

/// **The metric registry** — every group and metric the engine knows.
/// Compile-time data; every other layer derives its vocabulary from here.
pub const REGISTRY: &[GroupSpec] = &[
    GroupSpec {
        id: MetricGroupId::State,
        name: "m_state",
        kind: MetricKind::Static,
        scope: MetricScope::Token,
        family: MetricFamily::LiquidityAge,
        strict_params: &[],
        fingerprint_config: &[],
        // Blue/indigo family (~212–236). Deliberately clear of the green at 170:
        // the old sky hues (185/200) sat within ~15° of `buy` and the two groups
        // were near-indistinguishable at chip size.
        metrics: &[
            MetricSpec {
                id: MetricId::Time,
                name: "time",
                description: "Seconds since token creation. Monotonic, so an upper bound is a one-way door.",
                unit: Unit::Seconds,
                eq_tolerance: 0.5,
                monotonic: true,
                hue: 212,
            },
            MetricSpec {
                id: MetricId::Liquidity,
                name: "liquidity",
                description: "REAL SOL reserves at the most recent trade (`vsol - 30` on the curve, so 0..85 up to the graduation wall). NaN before the first trade.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 236,
            },
            MetricSpec {
                id: MetricId::FirstSlotBuy,
                name: "first_slot_buy",
                description: "Total buy SOL that landed in the token's CREATION slot - was the launch funded. Seeded when that slot settles, so it is NaN at birth and a rule using it cannot fire at launch.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                // Static after the creation slot settles - it never moves, so no
                // monotonic derivation.
                monotonic: false,
                // Between `time` (212) and `liquidity` (236), inside the snapshot family.
                hue: 218,
            },
        ],
    },
    GroupSpec {
        id: MetricGroupId::PriceLifetime,
        name: "m_price_lifetime",
        kind: MetricKind::Static,
        scope: MetricScope::Token,
        family: MetricFamily::Price,
        strict_params: &[],
        fingerprint_config: &[],
        // Amber/gold family (~40–62). Peak metrics (`stall`/`trail`) and the trough
        // twin (`rise`) share the band with m_price_window / m_position. Nudged up
        // off 35 to widen the gap to the red at 355.
        metrics: &[
            MetricSpec {
                id: MetricId::Stall,
                name: "stall",
                description: "Seconds since the price last set a new ALL-TIME high - not since the last trade. Only a strictly higher price resets it.",
                unit: Unit::Seconds,
                eq_tolerance: 0.5,
                monotonic: false,
                hue: 40,
            },
            MetricSpec {
                id: MetricId::Trail,
                name: "trail",
                description: "Percent below the lifetime peak price.",
                unit: Unit::Percent,
                eq_tolerance: 1.0,
                monotonic: false,
                hue: 42,
            },
            MetricSpec {
                id: MetricId::LifeRise,
                name: "rise",
                description: "Percent above the lifetime trough price.",
                unit: Unit::Percent,
                eq_tolerance: 1.0,
                monotonic: false,
                hue: 50,
            },
        ],
    },
    GroupSpec {
        id: MetricGroupId::PriceWindow,
        name: "m_price_window",
        kind: MetricKind::Dynamic,
        scope: MetricScope::Token,
        family: MetricFamily::Price,
        strict_params: &[
            // EXACTLY ONE of the three size params. None is `required` on its own;
            // `validate_group` enforces the choice, because "one of these" is a
            // cross-param rule a `StrictParamSpec` cannot spell.
            StrictParamSpec { name: WINDOW_SEC_PARAM, required: false, allows_zero: false },
            StrictParamSpec { name: WINDOW_SLOT_PARAM, required: false, allows_zero: false },
            StrictParamSpec { name: WINDOW_PRINT_PARAM, required: false, allows_zero: false },
            // How many units back from now the window ENDS. `0` is a real value (end
            // at now) and the only behaviour that existed before this param, so it is
            // the default and it allows zero.
            StrictParamSpec { name: WINDOW_LAG_PARAM, required: false, allows_zero: true },
        ],
        fingerprint_config: &[],
        // Amber family (44–48), sharing the 40–62 price band with m_price_lifetime —
        // three views of one price path (lifetime extrema vs rolling extrema vs
        // since-entry), the same "one classifier, sibling groups" rationale as
        // m_flow_ix / m_flow_ix_window. The cross-group hue guard exempts the
        // price family; do NOT widen the gap constants (the wheel is full — see
        // the guard test).
        metrics: &[
            MetricSpec {
                id: MetricId::WinTrail,
                name: "trail",
                description: "Percent below the rolling-window high - the dip trigger.",
                unit: Unit::Percent,
                eq_tolerance: 1.0,
                monotonic: false,
                hue: 44,
            },
            MetricSpec {
                id: MetricId::WinRise,
                name: "rise",
                description: "Percent above the rolling-window low.",
                unit: Unit::Percent,
                eq_tolerance: 1.0,
                monotonic: false,
                hue: 48,
            },
        ],
    },
    GroupSpec {
        id: MetricGroupId::FlowLifetime,
        name: "m_flow_lifetime",
        kind: MetricKind::Static,
        scope: MetricScope::Token,
        family: MetricFamily::Flow,
        strict_params: &[],
        fingerprint_config: &[],
        // Violet/magenta family (~278–300), shared with m_flow_window (one aggregate
        // flow, two views). `buy`/`sell` take the candle hues — same direction override
        // as the window sibling; the family-width guard exempts them.
        // Lifetime buy/sell/gross are monotonic (only grow) so entry upper bounds can
        // disarm; net is not (sells can reverse it).
        metrics: &[
            MetricSpec {
                id: MetricId::LifeGrossFlow,
                name: "gross_flow",
                description: "Buy + sell SOL since the token was born - total churn.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: true,
                hue: 278,
            },
            MetricSpec {
                id: MetricId::LifeNetFlow,
                name: "net_flow",
                description: "Buy - sell SOL since the token was born. Equals the depth added, so it moves with `liquidity`.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 300,
            },
            MetricSpec {
                id: MetricId::LifeBuy,
                name: "buy",
                description: "Buy SOL since the token was born.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: true,
                hue: CANDLE_UP_HUE,
            },
            MetricSpec {
                id: MetricId::LifeTradeCount,
                name: "trade_count",
                description: "Trades since the token's first - maturity, against `gross_flow`'s volume.",
                // A tally, so half a trade — same reasoning as the window sibling.
                unit: Unit::Count,
                eq_tolerance: 0.5,
                // Totals only grow, which is what makes an upper bound a one-way door.
                monotonic: true,
                // The window sibling's hue: one quantity, two views — same pairing as
                // `gross_flow` at 278 in both groups.
                hue: 294,
            },
            MetricSpec {
                id: MetricId::LifeSell,
                name: "sell",
                description: "Sell SOL since the token was born.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: true,
                hue: CANDLE_DOWN_HUE,
            },
        ],
    },
    GroupSpec {
        id: MetricGroupId::FlowWindow,
        name: "m_flow_window",
        kind: MetricKind::Dynamic,
        scope: MetricScope::Token,
        family: MetricFamily::Flow,
        strict_params: &[
            // EXACTLY ONE of the three size params. None is `required` on its own;
            // `validate_group` enforces the choice, because "one of these" is a
            // cross-param rule a `StrictParamSpec` cannot spell.
            StrictParamSpec { name: WINDOW_SEC_PARAM, required: false, allows_zero: false },
            StrictParamSpec { name: WINDOW_SLOT_PARAM, required: false, allows_zero: false },
            StrictParamSpec { name: WINDOW_PRINT_PARAM, required: false, allows_zero: false },
            // How many units back from now the window ENDS. `0` is a real value (end
            // at now) and the only behaviour that existed before this param, so it is
            // the default and it allows zero.
            StrictParamSpec { name: WINDOW_LAG_PARAM, required: false, allows_zero: true },
            // The SECOND axis - a slice NESTED in the window above, and the basis of
            // the `*_share` metrics alone. Optional at the group level and required
            // per-metric ([`is_two_window`]): a group instance that reads no share
            // must not be made to carry a span nothing reads, which is the silent
            // no-op `arm_above_pct` is guarded against. `validate_group` enforces
            // exactly one size, agreement with the reference unit, and the nesting
            // bound - none of which a `StrictParamSpec` can spell.
            StrictParamSpec { name: flow_slice::SLICE_PARAM, required: false, allows_zero: false },
            StrictParamSpec { name: flow_slice::SLICE_SLOT_PARAM, required: false, allows_zero: false },
            StrictParamSpec { name: flow_slice::SLICE_PRINT_PARAM, required: false, allows_zero: false },
        ],
        fingerprint_config: &[],
        // Same violet/magenta family as m_flow_lifetime — the cross-group hue guard
        // exempts the sibling pair. `buy`/`sell` take the candle up/down hues
        // (direction outranks group identity); the family-width guard exempts them.
        metrics: &[
            MetricSpec {
                id: MetricId::GrossFlow,
                name: "gross_flow",
                description: "Buy + sell SOL over the trailing window - how much is changing hands right now.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 278,
            },
            MetricSpec {
                id: MetricId::NetFlow,
                name: "net_flow",
                description: "Buy - sell SOL over the trailing window - the instantaneous direction of money.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 300,
            },
            MetricSpec {
                id: MetricId::Buy,
                name: "buy",
                description: "Buy SOL over the trailing window. Satisfied BY a buy landing, so a gate on it fills behind that buy's own price impact.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: CANDLE_UP_HUE,
            },
            MetricSpec {
                id: MetricId::BuyCount,
                name: "buy_count",
                description: "Number of BUYS over the trailing window. `trade_count` counts sells too, so on a one-slot window only this one answers `how many people bought into this burst`.",
                unit: Unit::Count,
                // A tally, so half a trade: smaller would make `== 2` depend on float
                // noise, larger would let it match 3.
                eq_tolerance: 0.5,
                monotonic: false,
                hue: 288,
            },
            MetricSpec {
                id: MetricId::BuyShare,
                name: "buy_share",
                description: "Share of the window's SOL that is buys, `buy / (buy + sell)` in percent - the tape's DIRECTION independent of its size. NaN on an empty window.",
                // A ratio in percent: 0.5pp is below any threshold worth authoring and
                // above float noise on a sum of f64 SOL amounts.
                unit: Unit::Percent,
                eq_tolerance: 0.5,
                monotonic: false,
                // Inside the group's violet family, NOT the candle up-hue: this is a
                // ratio, and the candle hues stay reserved for `buy`/`sell` so a
                // direction chip is recognizable at a glance.
                hue: 284,
            },
            MetricSpec {
                id: MetricId::TradeCount,
                name: "trade_count",
                description: "Trades over the trailing window - how BUSY the tape is, against `unique_wallets`' how many people. Needs no wallet column.",
                // A tally, so half a trade — same reasoning as `unique_wallets`.
                unit: Unit::Count,
                eq_tolerance: 0.5,
                monotonic: false,
                hue: 294,
            },
            MetricSpec {
                id: MetricId::Sell,
                name: "sell",
                description: "Sell SOL over the trailing window.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: CANDLE_DOWN_HUE,
            },
            MetricSpec {
                id: MetricId::SellCount,
                name: "sell_count",
                description: "Number of SELLS over the trailing window - `buy_count`'s twin. A condition cannot subtract, so `trade_count - buy_count` has no spelling and this is the only way to bound sells.",
                unit: Unit::Count,
                // A tally, so half a trade - same reasoning as `buy_count`.
                eq_tolerance: 0.5,
                monotonic: false,
                hue: 292,
            },
            MetricSpec {
                id: MetricId::SliceTradeShare,
                name: "trade_share",
                description: "Percent of the window's trades that landed in the `slice_size_*` span nested inside it - how CONCENTRATED the tape is in time, independent of how busy it is. Reads 100 on a token younger than the slice; NaN on an empty window.",
                // A ratio in percent, same reasoning as `buy_share`: 0.5pp is below any
                // threshold worth authoring and above float noise on a count/count.
                unit: Unit::Percent,
                eq_tolerance: 0.5,
                monotonic: false,
                // Above `net_flow` (300) at the top of the family, still >= 35 off the
                // candle red at 355.
                hue: 306,
            },
            MetricSpec {
                id: MetricId::SliceSolShare,
                name: "sol_share",
                description: "Percent of the window's SOL that moved in the `slice_size_*` span nested inside it. The SOL twin of `trade_share`, and the only one of the pair that still varies on a PRINT window. NaN on an empty window.",
                unit: Unit::Percent,
                eq_tolerance: 0.5,
                monotonic: false,
                hue: 308,
            },
        ],
    },
    GroupSpec {
        id: MetricGroupId::CrowdWindow,
        name: "m_crowd_window",
        kind: MetricKind::Dynamic,
        scope: MetricScope::Token,
        family: MetricFamily::Flow,
        strict_params: &[
            StrictParamSpec { name: WINDOW_SEC_PARAM, required: false, allows_zero: false },
            StrictParamSpec { name: WINDOW_SLOT_PARAM, required: false, allows_zero: false },
            StrictParamSpec { name: WINDOW_PRINT_PARAM, required: false, allows_zero: false },
            StrictParamSpec { name: WINDOW_LAG_PARAM, required: false, allows_zero: true },
        ],
        fingerprint_config: &[],
        // Its own group because its subject is WHO traded, not how much - and that
        // difference is a load obligation, not a taste: these two are the only metrics
        // that need the wallet column ([`MetricId::needs_wallet_identity`]), and an
        // offline read without it sees one anonymous wallet and makes every condition
        // here silently false. One group, one obligation, so a loader answers the
        // question by group instead of by metric list.
        //
        // Same violet family as the flow groups, whose tape it reads; the cross-group
        // hue guard exempts the set.
        metrics: &[
            MetricSpec {
                id: MetricId::UniqueWallets,
                name: "unique_wallets",
                description: "Distinct trading wallets over the trailing window - how many PEOPLE are in the token, against `m_flow_window.gross_flow`'s how much SOL.",
                // A tally, so the `=` tolerance is half a wallet: anything smaller
                // would make `== 5` depend on float noise, anything larger would let
                // it match 6.
                unit: Unit::Count,
                eq_tolerance: 0.5,
                monotonic: false,
                hue: 290,
            },
            MetricSpec {
                id: MetricId::TradesPerWallet,
                name: "trades_per_wallet",
                description: "`m_flow_window.trade_count / unique_wallets` over the same window. Low is a crowd arriving, high is one wallet working the tape - a COUNT ratio, never an identity, so wallet rotation does not defeat it. NaN on an empty window.",
                // A ratio of two tallies, not a tally: real thresholds sit at 1.5-3, so
                // half a unit would swallow them. 0.05 is below anything worth
                // authoring and far above float noise on a count/count.
                unit: Unit::Count,
                eq_tolerance: 0.05,
                monotonic: false,
                hue: 296,
            },
        ],
    },
    GroupSpec {
        id: MetricGroupId::FlowIx,
        name: "m_flow_ix",
        kind: MetricKind::Static,
        scope: MetricScope::Token,
        family: MetricFamily::FlowIx,
        strict_params: &[],
        fingerprint_config: &[FpConfigFieldSpec {
            name: "ix_patterns",
            value_type: "string[][]",
            required: true,
        }],
        // Teal family (~93–109) — clear of price-path (≤62) and candle green (170).
        metrics: &[
            MetricSpec {
                id: MetricId::TaggedBuy,
                name: "tagged_buy",
                description: "Buy SOL from VOLUME-side wallets (creator tooling, contagion, the creator) since birth.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: true,
                hue: 93,
            },
            MetricSpec {
                id: MetricId::TaggedSell,
                name: "tagged_sell",
                description: "Sell SOL from volume-side wallets since birth.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: true,
                hue: 95,
            },
            MetricSpec {
                id: MetricId::TaggedNet,
                name: "tagged_net",
                description: "Volume-side buy - sell SOL since birth.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 97,
            },
            MetricSpec {
                id: MetricId::TaggedGross,
                name: "tagged_gross",
                description: "Volume-side buy + sell SOL since birth.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: true,
                hue: 99,
            },
            MetricSpec {
                id: MetricId::UntaggedBuy,
                name: "untagged_buy",
                description: "Buy SOL from ORGANIC wallets (everyone the classifier does not tag) since birth.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: true,
                hue: 101,
            },
            MetricSpec {
                id: MetricId::UntaggedSell,
                name: "untagged_sell",
                description: "Sell SOL from organic wallets since birth.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: true,
                hue: 103,
            },
            MetricSpec {
                id: MetricId::UntaggedNet,
                name: "untagged_net",
                description: "Organic buy - sell SOL since birth.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 105,
            },
            MetricSpec {
                id: MetricId::UntaggedGross,
                name: "untagged_gross",
                description: "Organic buy + sell SOL since birth.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: true,
                hue: 107,
            },
            MetricSpec {
                id: MetricId::TaggedShare,
                name: "tagged_share",
                description: "Share of lifetime SOL that is volume-side, in percent. Needs the fingerprint's `ix_patterns`; NaN when unconfigured.",
                unit: Unit::Percent,
                eq_tolerance: 1.0,
                monotonic: false,
                hue: 109,
            },
        ],
    },
    GroupSpec {
        id: MetricGroupId::FlowIxWindow,
        name: "m_flow_ix_window",
        kind: MetricKind::Dynamic,
        scope: MetricScope::Token,
        family: MetricFamily::FlowIx,
        strict_params: &[
            // EXACTLY ONE of the three size params. None is `required` on its own;
            // `validate_group` enforces the choice, because "one of these" is a
            // cross-param rule a `StrictParamSpec` cannot spell.
            StrictParamSpec { name: WINDOW_SEC_PARAM, required: false, allows_zero: false },
            StrictParamSpec { name: WINDOW_SLOT_PARAM, required: false, allows_zero: false },
            StrictParamSpec { name: WINDOW_PRINT_PARAM, required: false, allows_zero: false },
            // How many units back from now the window ENDS. `0` is a real value (end
            // at now) and the only behaviour that existed before this param, so it is
            // the default and it allows zero.
            StrictParamSpec { name: WINDOW_LAG_PARAM, required: false, allows_zero: true },
        ],
        // Reads the same fingerprint key as m_flow_ix (one classifier, two views).
        fingerprint_config: &[],
        // Same teal family as m_flow_ix (one classifier, two views) — the
        // cross-group hue guard exempts this sibling pair.
        metrics: &[
            MetricSpec {
                id: MetricId::WinTaggedBuy,
                name: "tagged_buy",
                description: "Buy SOL from VOLUME-side wallets over the trailing window.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 93,
            },
            MetricSpec {
                id: MetricId::WinTaggedSell,
                name: "tagged_sell",
                description: "Sell SOL from volume-side wallets over the trailing window.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 95,
            },
            MetricSpec {
                id: MetricId::WinTaggedNet,
                name: "tagged_net",
                description: "Volume-side buy - sell SOL over the trailing window.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 97,
            },
            MetricSpec {
                id: MetricId::WinTaggedGross,
                name: "tagged_gross",
                description: "Volume-side buy + sell SOL over the trailing window.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 99,
            },
            MetricSpec {
                id: MetricId::WinUntaggedBuy,
                name: "untagged_buy",
                description: "Buy SOL from ORGANIC wallets over the trailing window.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 101,
            },
            MetricSpec {
                id: MetricId::WinUntaggedSell,
                name: "untagged_sell",
                description: "Sell SOL from organic wallets over the trailing window.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 103,
            },
            MetricSpec {
                id: MetricId::WinUntaggedNet,
                name: "untagged_net",
                description: "Organic buy - sell SOL over the trailing window.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 105,
            },
            MetricSpec {
                id: MetricId::WinUntaggedGross,
                name: "untagged_gross",
                description: "Organic buy + sell SOL over the trailing window.",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 107,
            },
            MetricSpec {
                id: MetricId::WinTaggedShare,
                name: "tagged_share",
                description: "Share of the window's SOL that is volume-side, in percent. Needs the fingerprint's `ix_patterns`; NaN when unconfigured.",
                unit: Unit::Percent,
                eq_tolerance: 1.0,
                monotonic: false,
                hue: 109,
            },
        ],
    },
    GroupSpec {
        id: MetricGroupId::Position,
        name: "m_position",
        kind: MetricKind::Static,
        scope: MetricScope::Position,
        family: MetricFamily::Price,
        // `arm_above_pct` gates the TRAILING metrics (`retrace`/`bounce`) on the
        // position being at least this far in profit — see
        // [`position::is_trailing`]. It exists because the exit combinator ORs
        // across metrics, so "trail out, but only once the trade has cleared the
        // fee" (`retrace >= 3 AND pnl >= 2`) is otherwise unauthorable. Absent ⇒
        // today's behaviour (the peak seeds at the entry fill, so an unarmed
        // `retrace` doubles as a hard stop from entry).
        strict_params: &[StrictParamSpec {
            name: "arm_above_pct",
            required: false,
            allows_zero: true,
        }],
        fingerprint_config: &[],
        // Amber family (52–60), the third view in the price family
        // {m_price_lifetime, m_price_window, m_position} sharing the 40–62 band — the
        // cross-group hue guard exempts the whole family. `monotonic: false` for all:
        // the flag powers ENTRY-side derived disarm, and these are exit-only.
        metrics: &[
            MetricSpec {
                id: MetricId::Retrace,
                name: "retrace",
                description: "Percent below the since-entry peak - the trailing stop. With no `arm_above_pct` the peak seeds at your fill, so it doubles as a hard stop from entry.",
                unit: Unit::Percent,
                eq_tolerance: 1.0,
                monotonic: false,
                hue: 52,
            },
            MetricSpec {
                id: MetricId::Bounce,
                name: "bounce",
                description: "Percent above the since-entry trough - the bounce twin of `retrace`.",
                unit: Unit::Percent,
                eq_tolerance: 1.0,
                monotonic: false,
                hue: 54,
            },
            MetricSpec {
                id: MetricId::Pnl,
                name: "pnl",
                description: "Signed percent against your entry price. Take-profit and stop-loss desugar into this.",
                unit: Unit::Percent,
                eq_tolerance: 1.0,
                monotonic: false,
                hue: 56,
            },
            MetricSpec {
                id: MetricId::Held,
                name: "held",
                description: "Seconds since the entry fill - the clock exit. A clock is not adversely selected at its own fill; a barrier is.",
                unit: Unit::Seconds,
                eq_tolerance: 0.5,
                monotonic: false,
                hue: 58,
            },
        ],
    },
];

/// The registry entry for a group id (total — every id has an entry).
pub fn group_spec(id: MetricGroupId) -> &'static GroupSpec {
    REGISTRY
        .iter()
        .find(|g| g.id == id)
        .expect("every MetricGroupId has a REGISTRY entry")
}

/// Resolve a group by its JSON name (`m_state`, …). `None` = unknown group.
pub fn group_by_name(name: &str) -> Option<&'static GroupSpec> {
    REGISTRY.iter().find(|g| g.name == name)
}

/// The registry entry for a metric id (total — every id has an entry).
pub fn metric_spec(id: MetricId) -> &'static MetricSpec {
    REGISTRY
        .iter()
        .flat_map(|g| g.metrics.iter())
        .find(|m| m.id == id)
        .expect("every MetricId has a REGISTRY entry")
}

/// Resolve a metric by its JSON name (`time`, `stall`, `tagged_buy`, …). When the
/// same name appears in more than one group (flow split vs window), the first
/// registry hit wins — enough for exit-reason label parse/display.
pub fn metric_id_by_name(name: &str) -> Option<MetricId> {
    REGISTRY
        .iter()
        .flat_map(|g| g.metrics.iter())
        .find(|m| m.name == name)
        .map(|m| m.id)
}

/// Resolve a metric name **within one group kind**.
///
/// A dynamic group and its lifetime twin deliberately share every metric name
/// (`m_flow_ix.untagged_buy` / `m_flow_ix_window.untagged_buy`), so
/// [`metric_id_by_name`] alone cannot answer which one a persisted exit label meant.
/// The window qualifier on the label is the discriminator, and this is how it is
/// applied. See `event::parse_metric_exit_label`.
pub fn metric_id_by_name_kind(name: &str, kind: MetricKind) -> Option<MetricId> {
    REGISTRY
        .iter()
        .filter(|g| g.kind == kind)
        .flat_map(|g| g.metrics.iter())
        .find(|m| m.name == name)
        .map(|m| m.id)
}

/// The group a metric belongs to.
pub fn group_of(id: MetricId) -> &'static GroupSpec {
    REGISTRY
        .iter()
        .find(|g| g.metrics.iter().any(|m| m.id == id))
        .expect("every MetricId belongs to a REGISTRY group")
}

/// The metric registry as a stable JSON document — the payload behind
/// `GET /api/meta/strategy-registry`. The frontend renders its whole
/// rule-authoring UI (group pickers, metric rows, operator lists) from this, so
/// adding a metric to [`REGISTRY`] surfaces it in the UI with no frontend change
/// (extensibility contract, plan §8).
pub fn registry_json() -> serde_json::Value {
    use serde_json::{json, Value};
    let groups: Vec<Value> = REGISTRY
        .iter()
        .map(|g| {
            let strict: Vec<Value> = g
                .strict_params
                .iter()
                .map(|p| {
                    json!({
                        "name": p.name,
                        "required": p.required,
                        "allows_zero": p.allows_zero,
                    })
                })
                .collect();
            let metrics: Vec<Value> = g
                .metrics
                .iter()
                .map(|m| {
                    json!({
                        "name": m.name,
                        "description": m.description,
                        "unit": m.unit.as_str(),
                        "eq_tolerance": m.eq_tolerance,
                        "monotonic": m.monotonic,
                        "hue": m.hue,
                        // Whether this metric reads the group's SECOND window axis.
                        // Mirrored so the editor asks the registry rather than
                        // hardcoding a group name: `m_flow_window` declares the slice
                        // axis for every instance, and only these metrics may set it.
                        "two_window": is_two_window(m.id),
                    })
                })
                .collect();
            let fp_cfg: Vec<Value> = g
                .fingerprint_config
                .iter()
                .map(|p| {
                    json!({
                        "name": p.name,
                        "value_type": p.value_type,
                        "required": p.required,
                    })
                })
                .collect();
            json!({
                "name": g.name,
                "kind": g.kind.as_str(),
                "scope": g.scope.as_str(),
                "family": g.family.as_str(),
                "strict_params": strict,
                "fingerprint_config": fp_cfg,
                "metrics": metrics,
            })
        })
        .collect();
    json!({
        "operators": ["<", "<=", ">", ">=", "=", "!="],
        "groups": groups,
    })
}

#[cfg(test)]
mod tests {
    use crate::metrics::WindowSpec;
    use super::*;

    /// `TradeLite::reserve_sol` uses `NaN` as its "no real reserve decoded yet"
    /// sentinel; the event log writes it as JSON `null` (serde_json's default
    /// non-finite-float behavior). A plain-`f64` derive round-trips one way only
    /// — it fails to read `null` back — which was silently dropping event-log
    /// lines (`invalid type: null, expected f64`) on replay/boot-recovery.
    /// Locks the fix: `null <-> NaN` must round-trip through JSON.
    #[test]
    fn trade_lite_reserve_sol_nan_round_trips_through_json() {
        let t = TradeLite { reserve_sol: f64::NAN, ..Default::default() };
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("\"reserve_sol\":null"), "json: {json}");
        let back: TradeLite = serde_json::from_str(&json).unwrap();
        assert!(back.reserve_sol.is_nan());

        // A finite value still round-trips exactly (no regression on the happy path).
        let t = TradeLite { reserve_sol: 42.5, ..Default::default() };
        let json = serde_json::to_string(&t).unwrap();
        let back: TradeLite = serde_json::from_str(&json).unwrap();
        assert_eq!(back.reserve_sol, 42.5);
    }

    #[test]
    fn registry_names_resolve_both_ways() {
        for g in REGISTRY {
            assert_eq!(group_by_name(g.name).unwrap().id, g.id);
            assert_eq!(g.id.name(), g.name);
            for m in g.metrics {
                assert_eq!(g.metric_by_name(m.name).unwrap().id, m.id);
                assert_eq!(m.id.name(), m.name);
                assert_eq!(group_of(m.id).id, g.id);
            }
        }
    }

    #[test]
    fn registry_names_are_unique() {
        let mut group_names: Vec<_> = REGISTRY.iter().map(|g| g.name).collect();
        group_names.sort_unstable();
        group_names.dedup();
        assert_eq!(group_names.len(), REGISTRY.len());

        // Metric names are unique within a group. The same name may appear in
        // sibling groups (m_flow_lifetime / m_flow_window share buy/sell/net/gross;
        // m_flow_ix / m_flow_ix_window share vol_*).
        for g in REGISTRY {
            let mut names: Vec<_> = g.metrics.iter().map(|m| m.name).collect();
            let total = names.len();
            names.sort_unstable();
            names.dedup();
            assert_eq!(names.len(), total, "{} has duplicate metric names", g.name);
        }

        // MetricIds themselves stay globally unique.
        let mut ids: Vec<_> = REGISTRY.iter().flat_map(|g| g.metrics.iter().map(|m| m.id)).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total);
    }

    /// Families mirror the hue families the registry already keeps — that is the
    /// intuition they were promoted from, and the discovery pipeline's Layer-2 grid
    /// is only meaningful if the two stay aligned. Sibling groups that deliberately
    /// share a hue band (`m_price_*`/`m_position`, `m_flow_*`, `m_flow_ix*`) must
    /// therefore also share a family, and a group in `Standalone` must be alone.
    #[test]
    fn families_group_the_registry_the_way_hues_do() {
        use std::collections::BTreeMap;
        let mut by_family: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for g in REGISTRY {
            by_family.entry(g.family.as_str()).or_default().push(g.name);
        }
        assert_eq!(by_family["price"], vec!["m_price_lifetime", "m_price_window", "m_position"]);
        assert_eq!(
            by_family["flow"],
            vec!["m_flow_lifetime", "m_flow_window", "m_crowd_window"]
        );
        assert_eq!(by_family["flow_ix"], vec!["m_flow_ix", "m_flow_ix_window"]);
        assert_eq!(by_family["liquidity_age"], vec!["m_state"]);
        // Nothing is unclassified today; a new group that lands in `Standalone` is
        // gridded alone (correct, just more compute) and this assert is the prompt
        // to decide whether it really belongs to an existing family.
        assert!(!by_family.contains_key("standalone"), "unclassified group: {by_family:?}");
    }

    /// A metric ships explained, and explained once. The definition lives here and the
    /// UI renders it from this text, so an empty one is a metric whose tooltip silently
    /// falls back to whatever the frontend happens to still say about it.
    #[test]
    fn every_metric_carries_its_own_definition() {
        for g in REGISTRY {
            for m in g.metrics {
                let d = m.description;
                assert!(!d.trim().is_empty(), "{}.{} has no description", g.name, m.name);
                // A definition, not a label: anything this short is a restatement of the
                // name and tells a rule author nothing.
                assert!(d.len() >= 30, "{}.{}: description too thin: {d:?}", g.name, m.name);
                assert!(d.ends_with('.'), "{}.{}: description is a sentence", g.name, m.name);
                // Unit / tolerance / monotonicity are their own fields and the UI appends
                // them; restating one here is the second copy the rule forbids.
                for banned in ["eq_tolerance", "monotonic"] {
                    assert!(
                        !d.contains(banned),
                        "{}.{}: {banned} is a registry FIELD, not prose",
                        g.name,
                        m.name,
                    );
                }
            }
        }
    }

    #[test]
    fn tolerances_are_positive_and_finite() {
        for g in REGISTRY {
            for m in g.metrics {
                assert!(m.eq_tolerance.is_finite() && m.eq_tolerance > 0.0, "{}", m.name);
            }
        }
    }

    /// The property the carrier exists for: a requirement's identity includes BOTH
    /// axes. Blockers and monotonic kills match reqs by `(metric, windows, ...)`, so
    /// without this two instances of a two-window group that differ only in the second
    /// axis would collide and one would silently mask the other.
    #[test]
    fn windows_identity_covers_both_axes() {
        assert_eq!(Windows::secs(60.0), Windows::from(Some(WindowSpec::secs(60.0))));
        assert_ne!(Windows::two(WindowSpec::secs(60.0), WindowSpec::secs(3.0)), Windows::two(WindowSpec::secs(60.0), WindowSpec::secs(5.0)), "second axis is identity");
        assert_ne!(Windows::two(WindowSpec::secs(60.0), WindowSpec::secs(3.0)), Windows::secs(60.0), "a second axis is not nothing");
        assert_eq!(Windows::NONE, Windows::from(None));
        // `is_windowed` answers "dynamic read?", so it must see either axis.
        assert!(!Windows::NONE.is_windowed());
        assert!(Windows::secs(5.0).is_windowed());
        assert!(Windows { primary: None, secondary: Some(WindowSpec::secs(3.0)) }.is_windowed());
    }

    #[test]
    fn monotonic_flags_match_contract() {
        // Lifetime accumulators that only grow: time + m_flow_lifetime
        // buy/sell/gross/trade_count + m_flow_ix vol/nonvol buy/sell/gross.
        // Windowed / net / share / everything else: not monotonic.
        let lifetime_flow_mono = [
            MetricId::LifeBuy,
            MetricId::LifeSell,
            MetricId::LifeGrossFlow,
            MetricId::LifeTradeCount,
            MetricId::TaggedBuy,
            MetricId::TaggedSell,
            MetricId::TaggedGross,
            MetricId::UntaggedBuy,
            MetricId::UntaggedSell,
            MetricId::UntaggedGross,
        ];
        for g in REGISTRY {
            for m in g.metrics {
                let expect = m.id == MetricId::Time || lifetime_flow_mono.contains(&m.id);
                assert_eq!(m.monotonic, expect, "{}.{}", g.name, m.name);
            }
        }
    }

    #[test]
    fn registry_json_mirrors_the_registry() {
        let j = registry_json();
        assert_eq!(j["operators"].as_array().unwrap().len(), 6);
        let groups = j["groups"].as_array().unwrap();
        assert_eq!(groups.len(), REGISTRY.len());
        for (jg, g) in groups.iter().zip(REGISTRY) {
            assert_eq!(jg["name"], g.name);
            assert_eq!(jg["kind"], g.kind.as_str());
            assert_eq!(jg["scope"], g.scope.as_str());
            assert_eq!(jg["family"], g.family.as_str());
            assert_eq!(jg["strict_params"].as_array().unwrap().len(), g.strict_params.len());
            let jm = jg["metrics"].as_array().unwrap();
            assert_eq!(jm.len(), g.metrics.len());
            for (m_json, m) in jm.iter().zip(g.metrics) {
                assert_eq!(m_json["name"], m.name);
                assert_eq!(m_json["description"], m.description);
                assert_eq!(m_json["unit"], m.unit.as_str());
                assert_eq!(m_json["eq_tolerance"], m.eq_tolerance);
                assert_eq!(m_json["monotonic"], m.monotonic);
                assert_eq!(m_json["hue"], m.hue);
            }
        }
        // m_flow_window advertises EVERY size param plus the lag; no size is
        // `required` alone because exactly one of them must be set, which is a
        // cross-param rule `validate_group` owns.
        let tw = groups.iter().find(|g| g["name"] == "m_flow_window").unwrap();
        let names: Vec<&str> = tw["strict_params"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            vec![
                WINDOW_SEC_PARAM,
                WINDOW_SLOT_PARAM,
                WINDOW_PRINT_PARAM,
                WINDOW_LAG_PARAM,
                // The nested slice, declared for every instance and required only of
                // the instances whose metrics read it (`is_two_window`).
                flow_slice::SLICE_PARAM,
                flow_slice::SLICE_SLOT_PARAM,
                flow_slice::SLICE_PRINT_PARAM,
            ]
        );
        assert!(tw["strict_params"].as_array().unwrap().iter().all(|p| p["required"] == false));
        let life = groups.iter().find(|g| g["name"] == "m_flow_lifetime").unwrap();
        assert!(life["strict_params"].as_array().unwrap().is_empty());
        assert_eq!(life["kind"], "static");
    }

    /// `label` and `parse` are one grammar, and every surface that names a span uses
    /// it: a persisted exit reason, a live chip, a chart legend, a `?windows=` query,
    /// a sweep axis. A span that survives this round trip means the same window
    /// wherever it is written.
    #[test]
    fn every_span_round_trips_through_its_label() {
        for w in [
            WindowSpec::secs(30.0),
            WindowSpec::secs(0.5),
            WindowSpec::secs(2.5),
            WindowSpec::slots(1.0, 0.0),
            WindowSpec::slots(30.0, 1.0),
            WindowSpec::prints(1.0, 0.0),
            WindowSpec::prints(20.0, 1.0),
        ] {
            let label = w.label();
            assert_eq!(WindowSpec::parse(&label), Some(w), "{label}");
        }
        // One size, three bases, three labels - the property that keeps a 30-slot read
        // from being served under a 30-second column.
        let labels: std::collections::BTreeSet<String> = WindowUnit::ALL
            .into_iter()
            .map(|unit| WindowSpec { size: 1.0, lag: 0.0, unit }.label())
            .collect();
        assert_eq!(labels.len(), 3, "{labels:?}");

        // A bare number is SECONDS: the spelling every span had before the other
        // bases existed, and what `?windows=10,30,60` still means.
        assert_eq!(WindowSpec::parse("60"), Some(WindowSpec::secs(60.0)));
        for bad in ["", "abc", "0p", "-5s", "30x", "30sl@-1", "s", "@1"] {
            assert_eq!(WindowSpec::parse(bad), None, "{bad}");
        }
    }

    /// Metrics that intentionally sit outside their group's hue family because
    /// they encode trade DIRECTION, which is colored globally (candle up/down)
    /// rather than per-group. See the `m_flow_window` comment in `REGISTRY`.
    const DIRECTION_METRICS: &[&str] = &["buy", "sell"];

    #[test]
    fn hues_in_range_and_grouped_nearby() {
        for g in REGISTRY {
            for m in g.metrics {
                assert!(m.hue < 360, "{}.{} hue {} out of range", g.name, m.name, m.hue);
            }
            // Direction metrics are exempt from the family-width rule — they take
            // the candle hues on purpose (asserted by the test below).
            let hues: Vec<u16> = g
                .metrics
                .iter()
                .filter(|m| !DIRECTION_METRICS.contains(&m.name))
                .map(|m| m.hue)
                .collect();
            if hues.is_empty() {
                continue;
            }
            let lo = *hues.iter().min().unwrap();
            let hi = *hues.iter().max().unwrap();
            assert!(
                hi - lo <= 60,
                "{} hue family too wide (span={}): {hues:?}",
                g.name,
                hi - lo,
            );
        }
    }

    /// Shortest distance between two hues on the 360° color wheel.
    fn hue_gap(a: u16, b: u16) -> u16 {
        let d = a.abs_diff(b);
        d.min(360 - d)
    }

    /// Every metric must stay this far from the two direction hues. Below roughly
    /// this, a chip's tint stops reading as "not a buy/sell" at a glance.
    const MIN_DIRECTION_GAP: u16 = 35;

    /// Two metrics in *different* groups must be at least this far apart, so the
    /// group a chip belongs to is legible from color alone.
    const MIN_CROSS_GROUP_GAP: u16 = 30;

    /// The whole point of pinning buy/sell to the candle colors is that they're
    /// instantly recognizable — which fails if an unrelated metric sits at a
    /// neighbouring hue. (It did: `liquidity` was 185, only 15° off the 170 green,
    /// so snapshot and direction chips looked alike.) Keep the two direction hues
    /// visually reserved.
    #[test]
    fn direction_hues_are_visually_isolated() {
        for g in REGISTRY {
            for m in g.metrics {
                if DIRECTION_METRICS.contains(&m.name) {
                    continue;
                }
                for (dir_name, dir_hue) in
                    [("buy", CANDLE_UP_HUE), ("sell", CANDLE_DOWN_HUE)]
                {
                    let gap = hue_gap(m.hue, dir_hue);
                    assert!(
                        gap >= MIN_DIRECTION_GAP,
                        "{}.{} (hue {}) is only {gap}° from {dir_name} (hue {dir_hue}); \
                         needs >= {MIN_DIRECTION_GAP}° to stay distinguishable",
                        g.name,
                        m.name,
                        m.hue,
                    );
                }
            }
        }
    }

    /// Metrics from different groups must not collide either — otherwise a
    /// `m_state` chip and a `m_flow_window` chip read as the same thing.
    /// **Sibling families** share a hue band on purpose and are exempt:
    /// * split flow — `m_flow_ix` / `m_flow_ix_window` (one classifier, two views);
    /// * aggregate flow — `m_flow_lifetime` / `m_flow_window` / `m_flow_window`
    ///   (lifetime, trailing, and the share of a trailing window that is recent);
    /// * price — `m_price_lifetime` / `m_price_window` / `m_position` (lifetime
    ///   extrema, rolling extrema, and since-entry — three views of the one price path).
    #[test]
    fn distinct_groups_use_distinct_hues() {
        use MetricGroupId::*;
        // Which family a group belongs to (`None` = its own group, never exempt).
        let family = |g: MetricGroupId| -> Option<u8> {
            match g {
                FlowIx | FlowIxWindow => Some(0),
                PriceLifetime | PriceWindow | Position => Some(1),
                FlowLifetime | FlowWindow | CrowdWindow => Some(2),
                _ => None,
            }
        };
        let siblings = |a: MetricGroupId, b: MetricGroupId| {
            matches!((family(a), family(b)), (Some(x), Some(y)) if x == y)
        };
        for (i, ga) in REGISTRY.iter().enumerate() {
            for gb in &REGISTRY[i + 1..] {
                if siblings(ga.id, gb.id) {
                    continue;
                }
                for ma in ga.metrics {
                    for mb in gb.metrics {
                        let gap = hue_gap(ma.hue, mb.hue);
                        assert!(
                            gap >= MIN_CROSS_GROUP_GAP,
                            "{}.{} (hue {}) and {}.{} (hue {}) are only {gap}° apart; \
                             needs >= {MIN_CROSS_GROUP_GAP}°",
                            ga.name,
                            ma.name,
                            ma.hue,
                            gb.name,
                            mb.name,
                            mb.hue,
                        );
                    }
                }
            }
        }
    }

    /// Guards the cross-language duplicate: the `buy`/`sell` chip hues must stay
    /// equal to the frontend's candle up/down colors, so a buy chip never drifts
    /// away from the green of an up-candle. If this fails, either the registry
    /// hue or the frontend hex moved — reconcile them, don't just update one.
    #[test]
    fn direction_metrics_match_candle_hues() {
        for group_name in ["m_flow_lifetime", "m_flow_window"] {
            let g = REGISTRY
                .iter()
                .find(|g| g.name == group_name)
                .unwrap_or_else(|| panic!("{group_name} group"));
            assert_eq!(
                g.metric_by_name("buy").expect("buy metric").hue,
                CANDLE_UP_HUE,
                "{group_name}.buy must render the candle up (#089981) hue",
            );
            assert_eq!(
                g.metric_by_name("sell").expect("sell metric").hue,
                CANDLE_DOWN_HUE,
                "{group_name}.sell must render the candle down (#f23645) hue",
            );
        }

        // Every direction metric named above must actually exist somewhere in the
        // registry, so the exemption list can't silently rot into a no-op.
        for name in DIRECTION_METRICS {
            assert!(
                REGISTRY.iter().any(|g| g.metric_by_name(name).is_some()),
                "DIRECTION_METRICS lists unknown metric {name}",
            );
        }
    }
}
