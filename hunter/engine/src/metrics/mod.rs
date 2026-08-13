//! Metrics framework — the self-describing vocabulary of the generic strategy
//! engine. Self-contained: no strategy/DB/tokio imports (parity backbone — see
//! `hunter/docs/arch/strategies.md`).
//!
//! A **metric** is a named per-token quantity a rule can put `{operator, value}`
//! conditions on. Metrics live in **groups** (one file per group):
//! * `m_snapshot` (static) — `time`, `liquidity`
//! * `m_price_lifetime` (static) — `stall`, `trail`, `rise` (lifetime peak/trough)
//! * `m_price_window` (dynamic, strict param `window_size_sec`) — `trail`, `rise`
//!   (rolling-window extrema; the dip trigger)
//! * `m_flow_lifetime` (static) — `gross_flow`, `net_flow`, `buy`, `sell`
//!   (lifetime SOL totals; no classifier)
//! * `m_flow_window` (dynamic, strict param `window_size_sec`) — same metrics
//!   over a trailing window
//! * `m_flow_split` (static, fingerprint-scoped) — vol/organic lifetime totals
//! * `m_flow_split_window` (dynamic, fingerprint-scoped) — same metrics over a window
//! * `m_position` (static, **position-scoped**, exit-only) — `retrace`, `bounce`,
//!   `pnl`, `held` (anchored on your entry fill; TP/SL desugar into `pnl` — see `arm.rs`)
//!
//! The **registry** below is the single source of truth for group/metric names,
//! units, `=`-tolerances, static/dynamic kind, monotonicity, and required strict
//! params. Params validation, the evaluator, the engine, replay, and sweep axes
//! all read it — adding a metric here (plus its compute logic in the group file)
//! makes it immediately usable everywhere, with no schema change.

pub mod evaluator;
pub mod flow_lifetime;
pub mod flow_split;
pub mod flow_window;
pub mod grid;
pub mod position;
pub mod price_lifetime;
pub mod price_window;
pub mod series;
pub mod snapshot;
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
/// "no real reserve decoded yet" sentinel (see [`crate::metrics::snapshot`]), so
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
/// via [`flow_split`]. Missing fields on old event-log lines default via serde
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
    pub at: Ts,
    /// FNV-1a of the trade's ordered `ix_labels`; `None` when labels are absent.
    #[serde(default)]
    pub ix_hash: Option<u64>,
    /// FNV-1a of the trade's wallet address.
    #[serde(default)]
    pub wallet_hash: u64,
}

impl Default for TradeLite {
    fn default() -> Self {
        Self {
            side: Side::Buy,
            sol: 0.0,
            price: 0.0,
            reserve_sol: 0.0,
            at: DateTime::from_timestamp(0, 0).expect("unix epoch"),
            ix_hash: None,
            wallet_hash: 0,
        }
    }
}

/// A metric group — one compute module, one JSON key under `entry`/`exit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MetricGroupId {
    /// `m_snapshot` — instantaneous token state.
    Snapshot,
    /// `m_price_lifetime` — incremental price-path state (lifetime peak).
    PriceLifetime,
    /// `m_price_window` — trailing-window price extrema (rolling high/low).
    PriceWindow,
    /// `m_flow_lifetime` — lifetime flow aggregates (no classifier).
    FlowLifetime,
    /// `m_flow_window` — trailing-window flow aggregates.
    FlowWindow,
    /// `m_flow_split` — volume/organic lifetime totals (fingerprint-scoped).
    FlowSplit,
    /// `m_flow_split_window` — volume/organic trailing-window totals (fingerprint-scoped).
    FlowSplitWindow,
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
    /// Seconds since token creation (`m_snapshot`).
    Time,
    /// SOL reserves (`m_snapshot`).
    Liquidity,
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
    // ── m_flow_split (lifetime; JSON names shared with m_flow_split_window) ─
    VolBuy,
    VolSell,
    VolNet,
    VolGross,
    NonvolBuy,
    NonvolSell,
    NonvolNet,
    NonvolGross,
    VolShare,
    // ── m_flow_split_window (trailing; distinct ids so monotonic flags can differ) ─
    WinVolBuy,
    WinVolSell,
    WinVolNet,
    WinVolGross,
    WinNonvolBuy,
    WinNonvolSell,
    WinNonvolNet,
    WinNonvolGross,
    WinVolShare,
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
        MetricGroupId::FlowSplit | MetricGroupId::FlowSplitWindow
    )
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
        is_flow_metric(self) || matches!(self, MetricId::UniqueWallets)
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
/// (deliberately independent of any fingerprint's `bucket_size_amount`).
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
    /// Classifier-split flow (volume vs organic), lifetime and windowed.
    FlowSplit,
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
            MetricFamily::FlowSplit => "flow_split",
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

/// **The metric registry** — every group and metric the engine knows.
/// Compile-time data; every other layer derives its vocabulary from here.
pub const REGISTRY: &[GroupSpec] = &[
    GroupSpec {
        id: MetricGroupId::Snapshot,
        name: "m_snapshot",
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
                unit: Unit::Seconds,
                eq_tolerance: 0.5,
                monotonic: true,
                hue: 212,
            },
            MetricSpec {
                id: MetricId::Liquidity,
                name: "liquidity",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 236,
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
                unit: Unit::Seconds,
                eq_tolerance: 0.5,
                monotonic: false,
                hue: 40,
            },
            MetricSpec {
                id: MetricId::Trail,
                name: "trail",
                unit: Unit::Percent,
                eq_tolerance: 1.0,
                monotonic: false,
                hue: 42,
            },
            MetricSpec {
                id: MetricId::LifeRise,
                name: "rise",
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
        strict_params: &[StrictParamSpec { name: "window_size_sec", required: true, allows_zero: false }],
        fingerprint_config: &[],
        // Amber family (44–48), sharing the 40–62 price band with m_price_lifetime —
        // three views of one price path (lifetime extrema vs rolling extrema vs
        // since-entry), the same "one classifier, sibling groups" rationale as
        // m_flow_split / m_flow_split_window. The cross-group hue guard exempts the
        // price family; do NOT widen the gap constants (the wheel is full — see
        // the guard test).
        metrics: &[
            MetricSpec {
                id: MetricId::WinTrail,
                name: "trail",
                unit: Unit::Percent,
                eq_tolerance: 1.0,
                monotonic: false,
                hue: 44,
            },
            MetricSpec {
                id: MetricId::WinRise,
                name: "rise",
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
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: true,
                hue: 278,
            },
            MetricSpec {
                id: MetricId::LifeNetFlow,
                name: "net_flow",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 300,
            },
            MetricSpec {
                id: MetricId::LifeBuy,
                name: "buy",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: true,
                hue: CANDLE_UP_HUE,
            },
            MetricSpec {
                id: MetricId::LifeSell,
                name: "sell",
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
        strict_params: &[StrictParamSpec { name: "window_size_sec", required: true, allows_zero: false }],
        fingerprint_config: &[],
        // Same violet/magenta family as m_flow_lifetime — the cross-group hue guard
        // exempts the sibling pair. `buy`/`sell` take the candle up/down hues
        // (direction outranks group identity); the family-width guard exempts them.
        metrics: &[
            MetricSpec {
                id: MetricId::GrossFlow,
                name: "gross_flow",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 278,
            },
            MetricSpec {
                id: MetricId::NetFlow,
                name: "net_flow",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 300,
            },
            MetricSpec {
                id: MetricId::Buy,
                name: "buy",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: CANDLE_UP_HUE,
            },
            MetricSpec {
                id: MetricId::UniqueWallets,
                name: "unique_wallets",
                // A tally, so the `=` tolerance is half a wallet: anything smaller
                // would make `== 5` depend on float noise, anything larger would let
                // it match 6.
                unit: Unit::Count,
                eq_tolerance: 0.5,
                monotonic: false,
                hue: 290,
            },
            MetricSpec {
                id: MetricId::Sell,
                name: "sell",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: CANDLE_DOWN_HUE,
            },
        ],
    },
    GroupSpec {
        id: MetricGroupId::FlowSplit,
        name: "m_flow_split",
        kind: MetricKind::Static,
        scope: MetricScope::Token,
        family: MetricFamily::FlowSplit,
        strict_params: &[],
        fingerprint_config: &[FpConfigFieldSpec {
            name: "volume_ix_patterns",
            value_type: "string[][]",
            required: true,
        }],
        // Teal family (~93–109) — clear of price-path (≤62) and candle green (170).
        metrics: &[
            MetricSpec {
                id: MetricId::VolBuy,
                name: "vol_buy",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: true,
                hue: 93,
            },
            MetricSpec {
                id: MetricId::VolSell,
                name: "vol_sell",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: true,
                hue: 95,
            },
            MetricSpec {
                id: MetricId::VolNet,
                name: "vol_net",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 97,
            },
            MetricSpec {
                id: MetricId::VolGross,
                name: "vol_gross",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: true,
                hue: 99,
            },
            MetricSpec {
                id: MetricId::NonvolBuy,
                name: "nonvol_buy",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: true,
                hue: 101,
            },
            MetricSpec {
                id: MetricId::NonvolSell,
                name: "nonvol_sell",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: true,
                hue: 103,
            },
            MetricSpec {
                id: MetricId::NonvolNet,
                name: "nonvol_net",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 105,
            },
            MetricSpec {
                id: MetricId::NonvolGross,
                name: "nonvol_gross",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: true,
                hue: 107,
            },
            MetricSpec {
                id: MetricId::VolShare,
                name: "vol_share",
                unit: Unit::Percent,
                eq_tolerance: 1.0,
                monotonic: false,
                hue: 109,
            },
        ],
    },
    GroupSpec {
        id: MetricGroupId::FlowSplitWindow,
        name: "m_flow_split_window",
        kind: MetricKind::Dynamic,
        scope: MetricScope::Token,
        family: MetricFamily::FlowSplit,
        strict_params: &[StrictParamSpec { name: "window_size_sec", required: true, allows_zero: false }],
        // Reads the same fingerprint key as m_flow_split (one classifier, two views).
        fingerprint_config: &[],
        // Same teal family as m_flow_split (one classifier, two views) — the
        // cross-group hue guard exempts this sibling pair.
        metrics: &[
            MetricSpec {
                id: MetricId::WinVolBuy,
                name: "vol_buy",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 93,
            },
            MetricSpec {
                id: MetricId::WinVolSell,
                name: "vol_sell",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 95,
            },
            MetricSpec {
                id: MetricId::WinVolNet,
                name: "vol_net",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 97,
            },
            MetricSpec {
                id: MetricId::WinVolGross,
                name: "vol_gross",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 99,
            },
            MetricSpec {
                id: MetricId::WinNonvolBuy,
                name: "nonvol_buy",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 101,
            },
            MetricSpec {
                id: MetricId::WinNonvolSell,
                name: "nonvol_sell",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 103,
            },
            MetricSpec {
                id: MetricId::WinNonvolNet,
                name: "nonvol_net",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 105,
            },
            MetricSpec {
                id: MetricId::WinNonvolGross,
                name: "nonvol_gross",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
                hue: 107,
            },
            MetricSpec {
                id: MetricId::WinVolShare,
                name: "vol_share",
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
                unit: Unit::Percent,
                eq_tolerance: 1.0,
                monotonic: false,
                hue: 52,
            },
            MetricSpec {
                id: MetricId::Bounce,
                name: "bounce",
                unit: Unit::Percent,
                eq_tolerance: 1.0,
                monotonic: false,
                hue: 54,
            },
            MetricSpec {
                id: MetricId::Pnl,
                name: "pnl",
                unit: Unit::Percent,
                eq_tolerance: 1.0,
                monotonic: false,
                hue: 56,
            },
            MetricSpec {
                id: MetricId::Held,
                name: "held",
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

/// Resolve a group by its JSON name (`m_snapshot`, …). `None` = unknown group.
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

/// Resolve a metric by its JSON name (`time`, `stall`, `vol_buy`, …). When the
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
/// (`m_flow_split.nonvol_buy` / `m_flow_split_window.nonvol_buy`), so
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
                        "unit": m.unit.as_str(),
                        "eq_tolerance": m.eq_tolerance,
                        "monotonic": m.monotonic,
                        "hue": m.hue,
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
        // m_flow_split / m_flow_split_window share vol_*).
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
    /// share a hue band (`m_price_*`/`m_position`, `m_flow_*`, `m_flow_split*`) must
    /// therefore also share a family, and a group in `Standalone` must be alone.
    #[test]
    fn families_group_the_registry_the_way_hues_do() {
        use std::collections::BTreeMap;
        let mut by_family: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for g in REGISTRY {
            by_family.entry(g.family.as_str()).or_default().push(g.name);
        }
        assert_eq!(by_family["price"], vec!["m_price_lifetime", "m_price_window", "m_position"]);
        assert_eq!(by_family["flow"], vec!["m_flow_lifetime", "m_flow_window"]);
        assert_eq!(by_family["flow_split"], vec!["m_flow_split", "m_flow_split_window"]);
        assert_eq!(by_family["liquidity_age"], vec!["m_snapshot"]);
        // Nothing is unclassified today; a new group that lands in `Standalone` is
        // gridded alone (correct, just more compute) and this assert is the prompt
        // to decide whether it really belongs to an existing family.
        assert!(!by_family.contains_key("standalone"), "unclassified group: {by_family:?}");
    }

    #[test]
    fn tolerances_are_positive_and_finite() {
        for g in REGISTRY {
            for m in g.metrics {
                assert!(m.eq_tolerance.is_finite() && m.eq_tolerance > 0.0, "{}", m.name);
            }
        }
    }

    #[test]
    fn monotonic_flags_match_contract() {
        // Lifetime accumulators that only grow: time + m_flow_lifetime buy/sell/gross
        // + m_flow_split vol/nonvol buy/sell/gross. Windowed / net / share /
        // everything else: not monotonic.
        let lifetime_flow_mono = [
            MetricId::LifeBuy,
            MetricId::LifeSell,
            MetricId::LifeGrossFlow,
            MetricId::VolBuy,
            MetricId::VolSell,
            MetricId::VolGross,
            MetricId::NonvolBuy,
            MetricId::NonvolSell,
            MetricId::NonvolGross,
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
                assert_eq!(m_json["unit"], m.unit.as_str());
                assert_eq!(m_json["eq_tolerance"], m.eq_tolerance);
                assert_eq!(m_json["monotonic"], m.monotonic);
                assert_eq!(m_json["hue"], m.hue);
            }
        }
        // m_flow_window advertises its required strict param; lifetime has none.
        let tw = groups.iter().find(|g| g["name"] == "m_flow_window").unwrap();
        assert_eq!(tw["strict_params"][0]["name"], "window_size_sec");
        assert_eq!(tw["strict_params"][0]["required"], true);
        let life = groups.iter().find(|g| g["name"] == "m_flow_lifetime").unwrap();
        assert!(life["strict_params"].as_array().unwrap().is_empty());
        assert_eq!(life["kind"], "static");
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
    /// `m_snapshot` chip and a `m_flow_window` chip read as the same thing.
    /// **Sibling families** share a hue band on purpose and are exempt:
    /// * split flow — `m_flow_split` / `m_flow_split_window` (one classifier, two views);
    /// * aggregate flow — `m_flow_lifetime` / `m_flow_window` (lifetime vs trailing);
    /// * price — `m_price_lifetime` / `m_price_window` / `m_position` (lifetime
    ///   extrema, rolling extrema, and since-entry — three views of the one price path).
    #[test]
    fn distinct_groups_use_distinct_hues() {
        use MetricGroupId::*;
        // Which family a group belongs to (`None` = its own group, never exempt).
        let family = |g: MetricGroupId| -> Option<u8> {
            match g {
                FlowSplit | FlowSplitWindow => Some(0),
                PriceLifetime | PriceWindow | Position => Some(1),
                FlowLifetime | FlowWindow => Some(2),
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
