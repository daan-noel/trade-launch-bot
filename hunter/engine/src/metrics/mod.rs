//! Metrics — the self-describing vocabulary of the strategy engine and the per-coin
//! state it is computed from. Self-contained: no strategy/DB/tokio imports (the parity
//! backbone — see `hunter/docs/arch/strategies.md`).
//!
//! A metric is read as `m_family.metric @tag [span]` ([`registry`]):
//!
//! | family       | subject                                  | compute module |
//! | ------------ | ---------------------------------------- | -------------- |
//! | `m_state`    | the pool now                             | [`state`] |
//! | `m_price`    | the chart                                | [`price_lifetime`], [`price_window`] |
//! | `m_flow`     | money moving (all trades, or one tag's)  | [`flow_lifetime`], [`flow_window`], [`flow_slice`], [`tags`] |
//! | `m_holdings` | what a tag holds and made                | [`tags`], [`holder_book`] |
//! | `m_crowd`    | who shows up                             | [`crowd_window`], [`build_window`], [`crowd_after_age`], [`burst_slot`] |
//! | `m_print`    | the print being read                     | [`print_wallet`] |
//! | `m_slot`     | this slot's buys                         | [`burst_slot`] |
//! | `m_wave`     | this buy wave                            | [`burst_wave`] |
//! | `m_position` | our trade                                | [`position`] |
//!
//! [`registry`] is the single source of truth for every name, unit, definition,
//! example and accepted tag/span; [`track::TokenTrack`] routes a read to the module
//! that computes it.

pub mod buffers;
pub mod build_window;
pub mod burst_slot;
pub mod burst_wave;
pub mod crowd_after_age;
pub mod crowd_window;
pub mod distinct_window;
pub mod evaluator;
pub mod fee;
pub mod flow_lifetime;
pub mod flow_slice;
pub mod flow_window;
pub mod grid;
pub mod holder_book;
pub mod metric_ref;
pub mod position;
pub mod price_lifetime;
pub mod price_window;
pub mod print_wallet;
pub mod registry;
pub mod series;
pub mod span;
pub mod state;
pub mod tags;
pub mod template_grain;
pub mod track;
pub mod trade_keys;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use self::fee::FeeKeys;

pub use registry::{family_spec, metric_by_path, metric_spec, registry_json, Family, Metric, MetricSpec, TagUse, Unit};
pub use metric_ref::{chart_reads, MetricRef};
pub use span::Span;
pub use tags::TagRef;

/// Hue of the frontend's candle **up** color (`#089981` ⇒ HSL hue 170): the hue of
/// `m_flow.buy_sol`. Duplicated across the language boundary — the hex lives in the
/// frontend's `CHART_COLORS.up` and `--color-green`; `registry` tests pin the pair.
pub const CANDLE_UP_HUE: u16 = 170;

/// Hue of the frontend's candle **down** color (`#f23645` ⇒ HSL hue 355): the hue of
/// `m_flow.sell_sol`. See [`CANDLE_UP_HUE`].
pub const CANDLE_DOWN_HUE: u16 = 355;

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
/// direction lives in `side`. `reserve_sol` is the SOL reserves after the trade
/// (liquidity).
///
/// **`price` is the pool state this print LEFT** — `TradeRow::fill_basis`, the
/// reserve-pair spot (`chart_spot_price`), falling back to the execution price
/// only when a row carries no reserve pair. Every adapter feeds it this way (live
/// `producers`, the lab's `to_trade_lite`, the readout), so every price metric —
/// `m_price_lifetime`, `m_price_window`, `m_position` — reads the spot series, which
/// is what the next order transacts against; the cost model charges OUR impact on
/// top. A print's own `price_per_token` is what that trader paid along the curve
/// and is not this series.
///
/// `ix_hash` / `wallet_hash` feed the volume-flow classifier (V1+); adapters hash
/// via [`flow_ix`]. Missing fields on old event-log lines default via serde
/// (`ix_hash: None`, `wallet_hash: 0`) ⇒ untagged unless contagious/creator.
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
    /// column). A slot window cannot advance on those: its cursor never moves, so it
    /// holds its opening content for the whole run and reads exactly like a strict
    /// gate that never fires — the same class of silent trap as the wallet-keyed
    /// metrics. [`CompiledRule::needs_slot`](crate::arm::CompiledRule::needs_slot) is
    /// the precomputed answer a loader checks its source against so that stays loud.
    #[serde(default)]
    pub slot: u64,
    /// Structural markers present in the trade's `ix_labels`, one bit each
    /// ([`crate::metrics::trade_keys::MARKERS`]). Set by the producer, which is the
    /// only layer holding the label strings; the engine compares bits.
    #[serde(default)]
    pub marker_bits: u16,
    /// Which instruction of its TRANSACTION this trade is, `0` for the first.
    ///
    /// One Solana transaction can carry several `Pump.Fun: Sell` instructions — a
    /// bundle selling four different wallets' bags at once is a real and common
    /// shape — and each becomes its own trade here. Every leg of a transaction
    /// carries the same `ix_labels`, so a metric that counts events counts the
    /// bundle four times; counting `leg_index == 0` counts TRANSACTIONS.
    ///
    /// `0` when a source does not supply it, which reads as "every trade is its own
    /// transaction" — the behaviour that existed before the field.
    #[serde(default)]
    pub leg_index: u8,
    /// Position of this trade's transaction within its block. `0` is the first
    /// transaction in the block — a real and common index — so missing is
    /// [`None`], never `0`. Packed (`m_burst_slot.packed`) is `NaN` when any buy
    /// in the current-slot prefix arrives without one.
    #[serde(default)]
    pub tx_index: Option<u32>,
    /// FNV-1a of this trade's build-template grain (`program|CU|ATA|N|S|F`).
    /// `None` when labels are absent. Distinct from [`ix_hash`](Self::ix_hash)
    /// (full ordered sequence) and from [`marker_bits`](Self::marker_bits).
    #[serde(default)]
    pub template_hash: Option<u64>,
    /// FNV-1a of this trade's program name (`Axiom Trade`, `Pump.Fun`, …).
    /// `None` when labels are absent. A bare name on `working_templates`
    /// matches this, not the grain.
    #[serde(default)]
    pub program_hash: Option<u64>,
    /// FNV-1a of this trade's build RECIPE ([`trade_keys::build_hash`]): the ordered
    /// labels without account setup, teardown and memos. `None` when labels are
    /// absent. What `m_build_window` counts distinct values of.
    #[serde(default)]
    pub build_hash: Option<u64>,
    /// Curve vs AMM. Default `true` so a pre-field event-log line still joins
    /// the burst prefix (the harvest universe is the curve). AMM prints do not.
    #[serde(default = "default_true")]
    pub on_curve: bool,
    /// `Pump.Fun: Create*` on the labels. Launch prints update first-on-mint
    /// but do not join the member prefix.
    #[serde(default)]
    pub is_launch: bool,
    /// The compute budget and tip this trade's TRANSACTION declared — the second
    /// half of a build's identity, read beside [`ix_hash`](Self::ix_hash) by the
    /// two pattern classifiers. Empty ([`FeeKeys::is_empty`]) on every trade
    /// predating core migration `0013` and on any source without the columns, which
    /// is why a pinned fee criterion must fail rather than pass against it.
    #[serde(default)]
    pub fee: FeeKeys,
    /// Tokens this print moved, in the token's raw units: what a buy received, what a
    /// sell gave up. The quantity `m_holder_book` keeps each wallet's bag in. `NaN`
    /// when a source does not carry it.
    #[serde(with = "finite_f64", default = "finite_f64::nan")]
    pub token_amount: f64,
    /// Whether this print's build recipe went through a public app on the UTC day
    /// before this print ([`holder_book::is_public_app`] over the daily build-breadth
    /// table). Stamped by `reduce` on a buy while a loaded rule reads `m_holder_book`
    /// (`Some(false)` for a recipe the table does not hold); `None` when no table is
    /// loaded, and on every row an adapter builds.
    #[serde(default)]
    pub build_day_public: Option<bool>,
}

fn default_true() -> bool {
    true
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
            leg_index: 0,
            tx_index: None,
            template_hash: None,
            program_hash: None,
            build_hash: None,
            on_curve: true,
            is_launch: false,
            fee: FeeKeys::default(),
            token_amount: f64::NAN,
            build_day_public: None,
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
    /// Every unit, in resolution order. The one place a basis is enumerated.
    pub const ALL: [WindowUnit; 3] = [Self::Sec, Self::Slot, Self::Print];

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

/// Nominal seconds per slot. Used in exactly one place - sizing the tick-grid
/// horizon for a slot window, where the grid is a wall clock and the span is not.
/// It never enters a metric reading: a slot window's cursor is the slot number the
/// feed reports, never a time estimate.
pub const NOMINAL_SLOT_SECS: f64 = 0.4;

/// A trailing window: how wide, how far back it ends, and in what unit.
///
/// `lag` is what makes a window **causal in its own terms**. A gate on "the state
/// entering this slot" must not be able to see the slot it is firing in, and
/// `lag: 1` on a slot window is exactly that: the slice is
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

#[cfg(test)]
mod tests {
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
}
