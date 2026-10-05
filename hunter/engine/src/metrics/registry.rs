//! **The vocabulary** — every family, metric, span and tag rule the engine knows, each
//! with its one definition and one example.
//!
//! A metric is read as `m_family.metric @tag [span]`:
//!
//! * the **family** is one subject (`m_flow` = money moving, `m_price` = the chart);
//! * the **metric** is one number in it, its unit the last word of its name
//!   (`buy_sol`, `held_sec`, `trail_pct`, `wallet_count`; no suffix = a 0/1 flag);
//! * the **tag** picks whose trades: `@volume` = trades carrying the fingerprint's
//!   `volume` tag, `@!volume` = the rest (see [`super::tags`]);
//! * the **span** picks over what stretch: absent = the coin's whole life, `10s` / `20sl`
//!   / `5p` = the last 10 seconds / 20 slots / 5 prints, `age60s` = since the coin was
//!   60 s old ([`super::span`]).
//!
//! Every metric declares which tags and spans it accepts; the rule parser rejects any
//! other. Text here is rendered as-is by the UI, the rule readout and the Guide page —
//! a definition lives in exactly one place.

use serde_json::{json, Value};

// ── Units ────────────────────────────────────────────────────────────────────

/// The unit a metric's values (and its condition thresholds) are in. Always the last
/// word of the metric's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Seconds,
    Sol,
    Percent,
    /// A tally of things (prints, wallets, slots).
    Count,
    /// 0 or 1.
    Flag,
    Lamports,
}

impl Unit {
    /// Stable JSON token (frontend contract).
    pub fn as_str(self) -> &'static str {
        match self {
            Unit::Seconds => "seconds",
            Unit::Sol => "sol",
            Unit::Percent => "percent",
            Unit::Count => "count",
            Unit::Flag => "flag",
            Unit::Lamports => "lamports",
        }
    }
}

// ── Families ─────────────────────────────────────────────────────────────────

/// One subject. The JSON prefix of every metric in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Family {
    State,
    Price,
    Flow,
    Holdings,
    Crowd,
    Print,
    Slot,
    Wave,
    Position,
}

#[derive(Debug, Clone, Copy)]
pub struct FamilySpec {
    pub id: Family,
    /// JSON prefix: `m_flow`.
    pub name: &'static str,
    /// Two or three words for a heading: "Money moving".
    pub title: &'static str,
    /// One line: what the family measures.
    pub summary: &'static str,
    /// One line with a number.
    pub example: &'static str,
}

pub const FAMILIES: &[FamilySpec] = &[
    FamilySpec {
        id: Family::State,
        name: "m_state",
        title: "Pool state",
        summary: "The pool right now: how old the coin is, how much SOL is in the pool, curve or AMM.",
        example: "m_state.liquidity_sol >= 14 : at least 14 real SOL sits in the pool.",
    },
    FamilySpec {
        id: Family::Price,
        name: "m_price",
        title: "Price chart",
        summary: "Where the price sits against its high and its low, over the coin's life or a recent stretch.",
        example: "m_price.trail_pct [10s] >= 20 : the price is 20 % or more below its high of the last 10 s.",
    },
    FamilySpec {
        id: Family::Flow,
        name: "m_flow",
        title: "Money moving",
        summary: "SOL bought and sold and how many trades, for all trades or one tag's trades, over the life or a recent stretch.",
        example: "m_flow.buy_sol @!volume [3s] >= 1 : trades WITHOUT the volume tag bought 1 SOL or more in the last 3 s.",
    },
    FamilySpec {
        id: Family::Holdings,
        name: "m_holdings",
        title: "What a tag holds",
        summary: "The tokens a tag's trades still hold, and the profit they would make selling them now.",
        example: "m_holdings.profit_sol @volume >= 0.83 : the volume trades would net 0.83 SOL selling their whole bag now.",
    },
    FamilySpec {
        id: Family::Crowd,
        name: "m_crowd",
        title: "Who shows up",
        summary: "How many different wallets, ix shapes or ix templates are trading.",
        example: "m_crowd.unique_wallets [30s] >= 10 : 10 or more different wallets traded in the last 30 s.",
    },
    FamilySpec {
        id: Family::Print,
        name: "m_print",
        title: "This print",
        summary: "Facts about the print being read right now. Empty (NaN) on a clock tick.",
        example: "m_print.since_buy_sec <= 5 : the wallet behind this print last bought this coin 5 s ago or less.",
    },
    FamilySpec {
        id: Family::Slot,
        name: "m_slot",
        title: "This slot's buys",
        summary: "The buys landed so far in the current slot: bundles and packs of the same ix template.",
        example: "m_slot.buy_count @working >= 3 : 3 or more buys in this slot came from a working template.",
    },
    FamilySpec {
        id: Family::Wave,
        name: "m_wave",
        title: "This buy wave",
        summary: "The run of back-to-back slots with buys that this print belongs to.",
        example: "m_wave.wallet_count >= 2 : at least 2 different wallets bought in this wave.",
    },
    FamilySpec {
        id: Family::Position,
        name: "m_position",
        title: "Our position",
        summary: "Our own trade: profit, time held, fall from our best price. Exists only after we buy, so it is for sell lines.",
        example: "m_position.pnl_pct >= 50 : we are up 50 % or more.",
    },
];

pub fn family_spec(id: Family) -> &'static FamilySpec {
    &FAMILIES[id as usize]
}

impl Family {
    /// The JSON prefix, `m_flow`.
    pub fn as_str(self) -> &'static str {
        family_spec(self).name
    }
}

pub fn family_by_name(name: &str) -> Option<&'static FamilySpec> {
    FAMILIES.iter().find(|f| f.name == name)
}

// ── Metrics ──────────────────────────────────────────────────────────────────

/// Every metric. The variant order is the order of [`METRICS`] (a guard test pins it),
/// so [`metric_spec`] is an index, never a search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Metric {
    // m_state
    AgeSec,
    LiquiditySol,
    OnCurve,
    // m_price
    RisePct,
    TrailPct,
    StallSec,
    // m_flow
    BuySol,
    SellSol,
    NetSol,
    GrossSol,
    BuyCount,
    SellCount,
    TradeCount,
    BuyTxCount,
    SellTxCount,
    BuySharePct,
    TagSharePct,
    SliceTradeSharePct,
    SliceSolSharePct,
    // m_holdings
    ProfitSol,
    BagSharePct,
    // m_crowd
    UniqueWallets,
    TradesPerWallet,
    UniqueIxShapes,
    UniqueIxTemplates,
    BuyerCount,
    BuyerIsNew,
    // m_print
    SinceBuySec,
    SoldBagPct,
    FlatBefore,
    // m_slot
    SlotThisJoined,
    SlotThisHasTag,
    SlotSameTemplateBuyCount,
    SlotSameTemplateBuySol,
    SlotSameTemplateWalletCount,
    SlotTemplateCount,
    SlotBuyCount,
    SlotBuySol,
    SlotWalletCount,
    SlotBuySharePct,
    SlotHasNewWallet,
    SlotHasUnknownWallet,
    SlotPacked,
    SlotLiquidityBeforeSol,
    SlotTrailBeforePct,
    // m_wave
    WaveThisJoined,
    WaveThisHasTag,
    WaveWalletCount,
    WaveBuySol,
    WaveBuyCount,
    WaveGapSlots,
    WaveAllNewWallets,
    WaveHasUnknownWallet,
    WaveThisTipLamports,
    WaveHasTxGap,
    WaveTipBandSeen,
    // m_position
    PnlPct,
    HeldSec,
    RetracePct,
    BouncePct,
    RoomTakenPct,
    StageSec,
}

/// Which tags a metric accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagUse {
    /// Never takes a tag.
    None,
    /// With a tag it reads that tag's trades; without one, every trade.
    Optional,
    /// Means nothing without a tag.
    Required,
}

/// What kind of tag a metric can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagLevel {
    /// A fingerprint tag judged trade by trade (any matcher).
    Trade,
    /// A fingerprint tag judged by ix template: only `ix_template` and `program`
    /// matchers mean anything to it (the slot and wave families, which keep one
    /// buffer per coin and apply the tag when they are read).
    Template,
    /// A built-in wallet class (`bundled`, `public_app`), judged at the wallet's first buy.
    WalletClass,
}

/// Which spans a metric accepts. `life` = no span written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpanUse {
    pub life: bool,
    pub window: bool,
    pub since_age: bool,
    /// Requires a `slice` beside its window (the two `slice_*` metrics).
    pub slice: bool,
}

impl SpanUse {
    /// Only the coin's life (or: the metric has no span at all).
    pub const LIFE: Self = Self { life: true, window: false, since_age: false, slice: false };
    pub const LIFE_OR_WINDOW: Self = Self { life: true, window: true, since_age: false, slice: false };
    pub const WINDOW: Self = Self { life: false, window: true, since_age: false, slice: false };
    pub const SINCE_AGE: Self = Self { life: false, window: false, since_age: true, slice: false };
    pub const SLICED_WINDOW: Self = Self { life: false, window: true, since_age: false, slice: true };
}

#[derive(Debug, Clone, Copy)]
pub struct MetricSpec {
    pub id: Metric,
    pub family: Family,
    /// Name inside its family: `buy_sol`.
    pub name: &'static str,
    /// A short noun phrase for sentences, without tag or span: `SOL bought`. A flag's
    /// phrase is the statement its 1 means: `the coin is still on the curve`.
    pub phrase: &'static str,
    pub unit: Unit,
    /// One line: what it measures, in plain words.
    pub summary: &'static str,
    /// One line with a number.
    pub example: &'static str,
    /// The rules a reader has to know (when it reads NaN, what it excludes). Empty when
    /// there is nothing.
    pub note: &'static str,
    pub tags: TagUse,
    pub tag_level: TagLevel,
    pub spans: SpanUse,
    /// Never decreases over the coin's life when read without a trailing window. An
    /// upper bound on it that is crossed can never hold again, so an entry waiting on
    /// it gives up (derived-unsatisfiable disarm).
    pub monotonic: bool,
    /// Width of the `=` / `!=` band.
    pub eq_tolerance: f64,
    /// UI hue, HSL degrees. A family sits in one band a few degrees wide, so
    /// neighbors share a color and still read apart. `m_flow.buy_sol` (170) and
    /// `m_flow.sell_sol` (355) are the chart candle hues, not the flow band.
    pub hue: u16,
}

impl MetricSpec {
    /// `m_flow.buy_sol`.
    pub fn path(&self) -> String {
        format!("{}.{}", family_spec(self.family).name, self.name)
    }
}

/// One table row (a positional constructor keeps the table one row per metric).
#[allow(clippy::too_many_arguments)]
const fn m(
    id: Metric,
    family: Family,
    name: &'static str,
    phrase: &'static str,
    unit: Unit,
    summary: &'static str,
    example: &'static str,
    note: &'static str,
    tags: TagUse,
    tag_level: TagLevel,
    spans: SpanUse,
    monotonic: bool,
    eq_tolerance: f64,
    hue: u16,
) -> MetricSpec {
    MetricSpec { id, family, name, phrase, unit, summary, example, note, tags, tag_level, spans, monotonic, eq_tolerance, hue }
}

use Family as F;
use Metric as M;
use SpanUse as S;
use TagLevel::{Template as TT, Trade as TR, WalletClass as WC};
use TagUse::{None as NT, Optional as OT, Required as RT};
use Unit as U;

/// Price rule every price metric shares.
const SPOT: &str = "Prices are the pool price each print left (the chart's price), not what that trader paid.";

/// Hues are a family band (a few degrees per metric), except `buy_sol` / `sell_sol`,
/// which stay the chart's up / down hues. See `MetricSpec::hue`.
pub const METRICS: &[MetricSpec] = &[
    // ── m_state ──
    m(M::AgeSec, F::State, "age_sec", "coin age", U::Seconds,
      "Seconds since the coin was created.",
      "m_state.age_sec >= 20 : the coin is 20 s old or older.",
      "", NT, TR, S::LIFE, true, 0.5, 222),
    m(M::LiquiditySol, F::State, "liquidity_sol", "real SOL in the pool", U::Sol,
      "Real SOL in the pool after the last print (on the curve: pool SOL minus the 30 virtual).",
      "m_state.liquidity_sol >= 80 : the curve holds 80+ real SOL, near graduation (85).",
      "NaN before the first print.", NT, TR, S::LIFE, false, 0.1, 226),
    m(M::OnCurve, F::State, "on_curve", "the coin is still on the curve", U::Flag,
      "1 while the last print was on the pump.fun curve, 0 once it was on the AMM.",
      "m_state.on_curve = 1 : the coin has not graduated.",
      "NaN before the first print.", NT, TR, S::LIFE, false, 0.5, 230),
    // ── m_price ──
    m(M::RisePct, F::Price, "rise_pct", "price above its low", U::Percent,
      "How far the price is above its low, in percent.",
      "m_price.rise_pct [30s] >= 50 : the price is 50 % above its low of the last 30 s.",
      SPOT, NT, TR, S::LIFE_OR_WINDOW, false, 1.0, 42),
    m(M::TrailPct, F::Price, "trail_pct", "price below its high", U::Percent,
      "How far the price is below its high, in percent.",
      "m_price.trail_pct >= 30 : the price is 30 % below the coin's all-time high.",
      SPOT, NT, TR, S::LIFE_OR_WINDOW, false, 1.0, 46),
    m(M::StallSec, F::Price, "stall_sec", "time since the last all-time high", U::Seconds,
      "Seconds since the price last set a new all-time high.",
      "m_price.stall_sec >= 60 : no new high for a minute.",
      "Only a strictly higher price resets it; a quiet tape keeps counting.", NT, TR, S::LIFE, false, 0.5, 50),
    // ── m_flow ──
    m(M::BuySol, F::Flow, "buy_sol", "SOL bought", U::Sol,
      "SOL spent buying.",
      "m_flow.buy_sol [10s] >= 5 : 5+ SOL bought in the last 10 s.",
      "Every leg of a transaction counts.", OT, TR, S::LIFE_OR_WINDOW, true, 0.1, 170),
    m(M::SellSol, F::Flow, "sell_sol", "SOL sold", U::Sol,
      "SOL received selling.",
      "m_flow.sell_sol @dump [1p] > 0 : the print being read is a dump sell.",
      "Every leg of a transaction counts.", OT, TR, S::LIFE_OR_WINDOW, true, 0.1, 355),
    m(M::NetSol, F::Flow, "net_sol", "SOL bought minus SOL sold", U::Sol,
      "SOL bought minus SOL sold: which way the money is going.",
      "m_flow.net_sol [30s] < 0 : more SOL left than came in over the last 30 s.",
      "", OT, TR, S::LIFE_OR_WINDOW, false, 0.1, 166),
    m(M::GrossSol, F::Flow, "gross_sol", "SOL traded", U::Sol,
      "SOL bought plus SOL sold: how much is changing hands.",
      "m_flow.gross_sol [30s] >= 10 : 10+ SOL traded in the last 30 s.",
      "", OT, TR, S::LIFE_OR_WINDOW, true, 0.1, 174),
    m(M::BuyCount, F::Flow, "buy_count", "buy prints", U::Count,
      "Number of buy prints.",
      "m_flow.buy_count [1sl] >= 3 : 3+ buys landed in this slot.",
      "A print is one leg: a bundle of 4 buys in one transaction counts 4.", OT, TR, S::LIFE_OR_WINDOW, true, 0.5, 158),
    m(M::SellCount, F::Flow, "sell_count", "sell prints", U::Count,
      "Number of sell prints.",
      "m_flow.sell_count [10s] = 0 : nobody sold in the last 10 s.",
      "A print is one leg: a bundle of 4 sells in one transaction counts 4.", OT, TR, S::LIFE_OR_WINDOW, true, 0.5, 349),
    m(M::TradeCount, F::Flow, "trade_count", "prints", U::Count,
      "Number of prints, buys and sells.",
      "m_flow.trade_count <= 7 : the coin has had 7 prints or fewer.",
      "", OT, TR, S::LIFE_OR_WINDOW, true, 0.5, 178),
    m(M::BuyTxCount, F::Flow, "buy_tx_count", "buy transactions", U::Count,
      "Number of buy transactions (a bundle of 4 buy legs counts 1).",
      "m_flow.buy_tx_count @targets [1p] >= 1 : a target wallet just bought.",
      "", RT, TR, S::LIFE_OR_WINDOW, true, 0.5, 154),
    m(M::SellTxCount, F::Flow, "sell_tx_count", "sell transactions", U::Count,
      "Number of sell transactions (a bundle of 4 sell legs counts 1).",
      "m_flow.sell_tx_count @dump [1p] >= 1 : the print being read is part of a dump transaction.",
      "", RT, TR, S::LIFE_OR_WINDOW, true, 0.5, 343),
    m(M::BuySharePct, F::Flow, "buy_share_pct", "buying share of the SOL traded", U::Percent,
      "Share of the traded SOL that was buying: buy_sol / gross_sol.",
      "m_flow.buy_share_pct [30s] >= 70 : 70 % of the SOL traded in 30 s was buying.",
      "NaN when nothing traded.", NT, TR, S::WINDOW, false, 0.5, 150),
    m(M::TagSharePct, F::Flow, "tag_share_pct", "share of the SOL traded", U::Percent,
      "Share of all traded SOL (buys and sells) that this tag's trades carried.",
      "m_flow.tag_share_pct @volume <= 50 : the volume trades carried half the SOL or less.",
      "NaN when nothing traded. Trades the tag excludes count on neither side.", RT, TR, S::LIFE_OR_WINDOW, false, 1.0, 182),
    m(M::SliceTradeSharePct, F::Flow, "slice_trade_share_pct", "share of the prints in the last slice", U::Percent,
      "Share of the span's prints that landed in the most recent slice of it.",
      "m_flow.slice_trade_share_pct [30s, slice 2s] >= 50 : half of the last 30 s of prints came in the last 2 s.",
      "Reads 100 on a coin younger than the slice; NaN on an empty span.", NT, TR, S::SLICED_WINDOW, false, 0.5, 186),
    m(M::SliceSolSharePct, F::Flow, "slice_sol_share_pct", "share of the SOL in the last slice", U::Percent,
      "Share of the span's SOL that moved in the most recent slice of it.",
      "m_flow.slice_sol_share_pct [30s, slice 2s] >= 50 : half of the last 30 s of SOL moved in the last 2 s.",
      "NaN on an empty span.", NT, TR, S::SLICED_WINDOW, false, 0.5, 190),
    // ── m_holdings ──
    m(M::ProfitSol, F::Holdings, "profit_sol", "profit if the whole bag were sold now", U::Sol,
      "What the tag's trades would net if their whole remaining bag were sold into the pool now: sale value + SOL taken out - SOL put in.",
      "m_holdings.profit_sol @volume >= 0.83 : the volume trades would make 0.83 SOL dumping everything now.",
      "The sale value includes the price drop the dump itself causes. NaN once a tagged print had no token amount.",
      RT, TR, S::LIFE, false, 0.1, 204),
    m(M::BagSharePct, F::Holdings, "bag_share_pct", "share of the held supply", U::Percent,
      "Share of the coin's held supply that sits with this wallet class.",
      "m_holdings.bag_share_pct @bundled >= 30 : bundle-bought wallets hold 30 % of the supply.",
      "Classes: bundled (first buy in a slot where 3+ wallets first bought with one ix shape), public_app (first buy through an app with 100+ buyers the day before). NaN while nobody holds.",
      RT, WC, S::LIFE, false, 0.01, 210),
    // ── m_crowd ──
    m(M::UniqueWallets, F::Crowd, "unique_wallets", "different wallets", U::Count,
      "Different wallets that traded.",
      "m_crowd.unique_wallets [30s] >= 10 : 10+ different wallets traded in the last 30 s.",
      "", NT, TR, S::WINDOW, false, 0.5, 264),
    m(M::TradesPerWallet, F::Crowd, "trades_per_wallet", "prints per wallet", U::Count,
      "Prints per wallet: trade_count / unique_wallets. Low = a crowd arriving, high = a few wallets working the tape.",
      "m_crowd.trades_per_wallet [30s] <= 1.5 : most wallets traded once.",
      "NaN on an empty span.", NT, TR, S::WINDOW, false, 0.05, 268),
    m(M::UniqueIxShapes, F::Crowd, "unique_ix_shapes", "different ix shapes", U::Count,
      "Different ix shapes that traded (setup, teardown and memo instructions ignored).",
      "m_crowd.unique_ix_shapes [30s] >= 4 : prints came from 4+ different tools.",
      "", NT, TR, S::WINDOW, false, 0.5, 272),
    m(M::UniqueIxTemplates, F::Crowd, "unique_ix_templates", "different ix templates seen", U::Count,
      "Different ix templates of this tag seen among the coin's curve buys, this print included.",
      "m_crowd.unique_ix_templates @working >= 4 : 4+ working templates have bought this coin.",
      "", RT, TT, S::LIFE, true, 0.5, 276),
    m(M::BuyerCount, F::Crowd, "buyer_count", "new buyers", U::Count,
      "Different wallets, the creator excluded, that bought since the span's start age.",
      "m_crowd.buyer_count [age60s] >= 5 : 5+ new wallets bought after the coin was a minute old.",
      "Buys before the start age never count.", NT, TR, S::SINCE_AGE, true, 0.5, 280),
    m(M::BuyerIsNew, F::Crowd, "buyer_is_new", "this print adds a new buyer", U::Flag,
      "1 when the print being read is a buy that just added a wallet to buyer_count.",
      "m_crowd.buyer_is_new [age60s] = 1 and m_crowd.buyer_count [age60s] = 5 : the 5th arrival, once.",
      "0 on a tick, a sell, a repeat buyer and the creator.", NT, TR, S::SINCE_AGE, false, 0.5, 284),
    // ── m_print ──
    m(M::SinceBuySec, F::Print, "since_buy_sec", "time since this wallet last bought", U::Seconds,
      "Seconds since the wallet behind this print last bought this coin.",
      "m_print.since_buy_sec <= 5 : this wallet bought 5 s ago or less (on a sell: a 5 s flip).",
      "NaN when that wallet never bought, and on a tick.", NT, TR, S::LIFE, false, 0.5, 248),
    m(M::SoldBagPct, F::Print, "sold_bag_pct", "share of his bag this sell took", U::Percent,
      "Percent of this wallet's token bag, measured before this print, that this sell removes.",
      "m_print.sold_bag_pct @targets >= 30 : this sell took 30 % or more of a target wallet's bag.",
      "0 when this print is not a sell by a wallet carrying the tag. 100 when the sell empties the bag. NaN on a tick, and when that sell has no token amount. A negated tag reads 0.",
      RT, TR, S::LIFE, false, 0.5, 252),
    m(M::FlatBefore, F::Print, "flat_before", "this wallet held nothing before this print", U::Flag,
      "1 when this print's wallet carries the tag and held no tokens of this coin before it.",
      "m_print.flat_before @targets = 1 : a target wallet held nothing before this print.",
      "0 when the wallet already held, or the print's wallet does not carry the tag. NaN on a tick. A negated tag reads 0.",
      RT, TR, S::LIFE, false, 0.5, 256),
    // ── m_slot ──
    m(M::SlotThisJoined, F::Slot, "this_joined", "this print joined the slot's buys", U::Flag,
      "1 when this print just joined the slot's buy group: a curve buy with an ix template, not the create.",
      "m_slot.this_joined = 1 : decide on a buy print, not on a sell or a tick.",
      "", NT, TT, S::LIFE, false, 0.5, 68),
    m(M::SlotThisHasTag, F::Slot, "this_has_tag", "this print's template carries the tag", U::Flag,
      "1 when this print's ix template carries the tag.",
      "m_slot.this_has_tag @working = 1 : this buy came from a working template.",
      "0 when the print has no ix labels.", RT, TT, S::LIFE, false, 0.5, 70),
    m(M::SlotSameTemplateBuyCount, F::Slot, "same_template_buy_count", "buys in this slot with this print's template", U::Count,
      "Buys so far in this slot with the same ix template as this print.",
      "m_slot.same_template_buy_count >= 3 : 3 buys of one template landed together.",
      "", NT, TT, S::LIFE, false, 0.5, 72),
    m(M::SlotSameTemplateBuySol, F::Slot, "same_template_buy_sol", "SOL bought in this slot with this print's template", U::Sol,
      "SOL bought so far in this slot by buys with the same ix template as this print.",
      "m_slot.same_template_buy_sol >= 2 : one template put 2 SOL in this slot.",
      "", NT, TT, S::LIFE, false, 0.1, 74),
    m(M::SlotSameTemplateWalletCount, F::Slot, "same_template_wallet_count", "wallets in this slot with this print's template", U::Count,
      "Different wallets among this slot's buys with the same ix template as this print.",
      "m_slot.same_template_wallet_count >= 3 : 3 wallets bought with one template in this slot.",
      "", NT, TT, S::LIFE, false, 0.5, 76),
    m(M::SlotTemplateCount, F::Slot, "template_count", "templates in this slot", U::Count,
      "Different ix templates among this slot's buys (with a tag: only the tag's templates).",
      "m_slot.template_count @working >= 2 : two different working templates bought in this slot.",
      "", OT, TT, S::LIFE, false, 0.5, 78),
    m(M::SlotBuyCount, F::Slot, "buy_count", "buys in this slot", U::Count,
      "Buys so far in this slot whose ix template carries the tag.",
      "m_slot.buy_count @working >= 3 : 3 working-template buys in this slot.",
      "", RT, TT, S::LIFE, false, 0.5, 80),
    m(M::SlotBuySol, F::Slot, "buy_sol", "SOL bought in this slot", U::Sol,
      "SOL bought so far in this slot by buys whose ix template carries the tag.",
      "m_slot.buy_sol @working >= 1 : working templates put 1 SOL in this slot.",
      "", RT, TT, S::LIFE, false, 0.1, 82),
    m(M::SlotWalletCount, F::Slot, "wallet_count", "wallets in this slot", U::Count,
      "Different wallets among this slot's buys whose ix template carries the tag.",
      "m_slot.wallet_count @working >= 2 : two wallets bought with working templates in this slot.",
      "", RT, TT, S::LIFE, false, 0.5, 84),
    m(M::SlotBuySharePct, F::Slot, "buy_share_pct", "share of this slot's buys", U::Percent,
      "Share of this slot's buys whose ix template carries the tag.",
      "m_slot.buy_share_pct @working = 100 : every buy in this slot is a working template.",
      "NaN before the slot's first buy.", RT, TT, S::LIFE, false, 0.5, 86),
    m(M::SlotHasNewWallet, F::Slot, "has_new_wallet", "a first-time buyer is in this slot", U::Flag,
      "1 when a wallet among this slot's tagged buys is buying this coin for the first time.",
      "m_slot.has_new_wallet @working = 1 : at least one fresh wallet in the pack.",
      "", RT, TT, S::LIFE, false, 0.5, 88),
    m(M::SlotHasUnknownWallet, F::Slot, "has_unknown_wallet", "a buy in this slot has no wallet on record", U::Flag,
      "1 when a buy in this slot has no wallet on record.",
      "m_slot.has_unknown_wallet = 0 : every buyer in the slot is known.",
      "", NT, TT, S::LIFE, false, 0.5, 90),
    m(M::SlotPacked, F::Slot, "packed", "this slot's buys sit back to back", U::Flag,
      "1 when this slot's buys sit in back-to-back positions in the block, 0 when another transaction sits between them.",
      "m_slot.packed = 1 : the buys were bundled.",
      "NaN when a buy has no block position.", NT, TT, S::LIFE, false, 0.5, 92),
    m(M::SlotLiquidityBeforeSol, F::Slot, "liquidity_before_sol", "real SOL in the pool before this slot", U::Sol,
      "Real SOL in the pool at the last print of an earlier slot: the depth before this slot's buys.",
      "m_slot.liquidity_before_sol <= 10 : the pool was shallow when this slot started.",
      "NaN until an earlier slot has a print.", NT, TT, S::LIFE, false, 0.1, 94),
    m(M::SlotTrailBeforePct, F::Slot, "trail_before_pct", "price below its high before this print", U::Percent,
      "How far below its all-time high the price was before this print.",
      "m_slot.trail_before_pct >= 30 : this print lands in a 30 % dip.",
      SPOT, NT, TT, S::LIFE, false, 1.0, 96),
    // ── m_wave ──
    m(M::WaveThisJoined, F::Wave, "this_joined", "this print joined the wave", U::Flag,
      "1 when this print just joined the current wave: a curve buy with an ix template, not the create.",
      "m_wave.this_joined = 1 : decide on a wave buy.",
      "", NT, TT, S::LIFE, false, 0.5, 300),
    m(M::WaveThisHasTag, F::Wave, "this_has_tag", "this print joined the wave with a tagged template", U::Flag,
      "1 when this print just joined the wave and its ix template carries the tag.",
      "m_wave.this_has_tag @working = 1 : a working template joined the wave.",
      "NaN when the fingerprint has no such tag.", RT, TT, S::LIFE, false, 0.5, 302),
    m(M::WaveWalletCount, F::Wave, "wallet_count", "wallets in this wave", U::Count,
      "Different wallets among this wave's buys.",
      "m_wave.wallet_count >= 2 : the second wallet just joined.",
      "", NT, TT, S::LIFE, false, 0.5, 304),
    m(M::WaveBuySol, F::Wave, "buy_sol", "SOL bought in this wave", U::Sol,
      "SOL bought in this wave so far.",
      "m_wave.buy_sol >= 1 : the wave has put 1 SOL in.",
      "", NT, TT, S::LIFE, false, 0.1, 306),
    m(M::WaveBuyCount, F::Wave, "buy_count", "buys in this wave", U::Count,
      "Buys in this wave whose ix template carries the tag.",
      "m_wave.buy_count @working >= 2 : two working-template buys in this wave.",
      "NaN when the wave cannot fire (the create slot, or no gap before it).", RT, TT, S::LIFE, false, 0.5, 308),
    m(M::WaveGapSlots, F::Wave, "gap_slots", "empty slots before this wave", U::Count,
      "Empty slots (no buys) before this wave started.",
      "m_wave.gap_slots >= 10 : the wave broke 10 quiet slots.",
      "NaN on the create slot, with no proven gap, or after a slot regression.", NT, TT, S::LIFE, false, 0.5, 310),
    m(M::WaveAllNewWallets, F::Wave, "all_new_wallets", "every wallet in this wave is a first-time buyer", U::Flag,
      "1 when every wallet in this wave is buying this coin for the first time.",
      "m_wave.all_new_wallets = 1 : a wave of fresh wallets.",
      "NaN on an empty wave.", NT, TT, S::LIFE, false, 0.5, 312),
    m(M::WaveHasUnknownWallet, F::Wave, "has_unknown_wallet", "a buy in this wave has no wallet on record", U::Flag,
      "1 when a buy in this wave has no wallet on record.",
      "m_wave.has_unknown_wallet = 0 : every buyer in the wave is known.",
      "", NT, TT, S::LIFE, false, 0.5, 314),
    m(M::WaveThisTipLamports, F::Wave, "this_tip_lamports", "tip this print paid", U::Lamports,
      "The tip this print's transaction paid.",
      "m_wave.this_tip_lamports >= 1000000 : this buyer tipped 0.001 SOL or more.",
      "NaN when the fee was not captured; a real 0 reads 0.", NT, TT, S::LIFE, false, 0.5, 316),
    m(M::WaveHasTxGap, F::Wave, "has_tx_gap", "another transaction sits before this buy", U::Flag,
      "1 when this buy's block position is more than one after the wave's previous buy: another transaction sits between them.",
      "m_wave.has_tx_gap = 0 : this buy sits right behind the last one.",
      "0 for the wave's first buy.", NT, TT, S::LIFE, false, 0.5, 318),
    m(M::WaveTipBandSeen, F::Wave, "tip_band_seen", "an earlier buy in this wave tipped in this print's band", U::Flag,
      "1 when an earlier buy in this wave paid a tip in the same band as this print (none, 0, <0.0001, <0.001, more).",
      "m_wave.tip_band_seen = 0 : this buyer tips differently from the rest of the wave.",
      "", NT, TT, S::LIFE, false, 0.5, 320),
    // ── m_position ──
    m(M::PnlPct, F::Position, "pnl_pct", "our profit", U::Percent,
      "Our profit against our buy price, in percent.",
      "m_position.pnl_pct <= -25 : we are down 25 %.",
      SPOT, NT, TR, S::LIFE, false, 1.0, 12),
    m(M::HeldSec, F::Position, "held_sec", "time held", U::Seconds,
      "Seconds since our buy filled.",
      "m_position.held_sec >= 1000 : we have held for 1000 s.",
      "", NT, TR, S::LIFE, false, 0.5, 16),
    m(M::RetracePct, F::Position, "retrace_pct", "fall from our best price", U::Percent,
      "How far the price fell from its best since our buy, in percent: the trailing stop.",
      "m_position.retrace_pct >= 20 : the price is 20 % below its peak since we bought.",
      SPOT, NT, TR, S::LIFE, false, 1.0, 20),
    m(M::BouncePct, F::Position, "bounce_pct", "rise from our worst price", U::Percent,
      "How far the price rose from its worst since our buy, in percent.",
      "m_position.bounce_pct >= 30 : the price climbed 30 % off its low since we bought.",
      SPOT, NT, TR, S::LIFE, false, 1.0, 24),
    m(M::RoomTakenPct, F::Position, "room_taken_pct", "share of the way to graduation covered", U::Percent,
      "How much of the way from our buy price to the graduation price the price has covered.",
      "m_position.room_taken_pct >= 40 : 40 % of the room to graduation is used (at a buy near pool 100 that is +13 %).",
      "NaN when the pool depth at our buy is unknown.", NT, TR, S::LIFE, false, 1.0, 28),
    m(M::StageSec, F::Position, "stage_sec", "time in this stage", U::Seconds,
      "Seconds since the rule entered its current stage.",
      "m_position.stage_sec <= 30 : still in the first 30 s of this stage.",
      "", NT, TR, S::LIFE, false, 0.5, 32),
];

pub fn metric_spec(id: Metric) -> &'static MetricSpec {
    &METRICS[id as usize]
}

impl Metric {
    pub fn spec(self) -> &'static MetricSpec {
        metric_spec(self)
    }

    pub fn family(self) -> Family {
        self.spec().family
    }

    /// Name inside its family.
    pub fn name(self) -> &'static str {
        self.spec().name
    }

    /// Reads our position rather than the coin: exit side only.
    pub fn is_position(self) -> bool {
        self.family() == Family::Position
    }

    /// Whether the value depends on WHO traded. Offline this is a load-time question:
    /// the lake leaves the wallet column out unless a run asks for it, and a fold over
    /// rows without it sees one anonymous wallet — `unique_wallets >= 10` would simply
    /// never fire. `tagged` = the read carries a tag (a tag may use `wallet`, `creator`
    /// or `sticky`).
    pub fn needs_wallet_identity(self, tagged: bool) -> bool {
        tagged
            || matches!(self.family(), Family::Wave | Family::Holdings | Family::Print)
            || matches!(
                self,
                M::UniqueWallets
                    | M::TradesPerWallet
                    | M::BuyerCount
                    | M::BuyerIsNew
                    | M::SlotSameTemplateWalletCount
                    | M::SlotWalletCount
                    | M::SlotHasNewWallet
                    | M::SlotHasUnknownWallet
            )
    }

    /// Whether the value depends on the trade's instruction labels (full hash, markers
    /// or template). Offline the lake leaves `ix_labels` out unless asked.
    pub fn needs_ix_labels(self, tagged: bool) -> bool {
        tagged
            || matches!(self.family(), Family::Slot | Family::Wave | Family::Holdings)
            || matches!(self, M::UniqueIxShapes | M::UniqueIxTemplates)
    }
}

impl std::fmt::Display for Metric {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.spec().path())
    }
}

/// Resolve `m_flow.buy_sol`.
pub fn metric_by_path(path: &str) -> Option<Metric> {
    let (fam, name) = path.split_once('.')?;
    let fam = family_by_name(fam)?.id;
    METRICS.iter().find(|s| s.family == fam && s.name == name).map(|s| s.id)
}

// ── Rule parts (text only; the grammar lives in `rule`) ──────────────────────

/// One part of a rule, for the editor's headings and the Guide page.
#[derive(Debug, Clone, Copy)]
pub struct PartSpec {
    /// JSON key path: `enter.filters`.
    pub key: &'static str,
    pub title: &'static str,
    pub summary: &'static str,
    pub example: &'static str,
}

pub const RULE_PARTS: &[PartSpec] = &[
    PartSpec { key: "enter.event", title: "Buy On",
        summary: "The print that triggers the buy. Every condition must hold on that print.",
        example: "m_state.age_sec >= 1 : buy on a print once the coin is 1 s old." },
    PartSpec { key: "enter.filters", title: "Only If",
        summary: "Must also hold on that try. A miss fails this try. Once Per Coin has no next try, so that miss stops the coin.",
        example: "m_state.liquidity_sol >= 14 : only when the pool holds 14+ SOL." },
    PartSpec { key: "enter.final_filters", title: "Give Up",
        summary: "Checked on a try that would otherwise buy. A miss stops this coin.",
        example: "m_crowd.unique_ix_templates @working >= 4 : give up on coins with fewer than 4 working templates." },
    PartSpec { key: "enter.lock", title: "Tries",
        summary: "How many tries this rule gets on one coin. Any Print: a try on every trade and every clock tick where On is true. Once Per Coin: one try, the first trade where On is true; a clock tick is never a try; a failed Only If stops the coin. Once Per Slot: one try per block, the first moment On is true.",
        example: "Once Per Coin, On is age >= 1 s: the first trade at 1 s is the only try. If Only If fails, this coin is finished." },
    PartSpec { key: "enter.size_pct_of_pool", title: "Size as % of pool",
        summary: "Buy this percent of the pool's SOL instead of the fixed amount, so our own price impact stays the same.",
        example: "1.5 : at pool 100 SOL, buy 1.5 SOL." },
    PartSpec { key: "signals", title: "Signals",
        summary: "Named conditions, written once and used by name in any condition: a buy gate or a line. A signal holds when any of its groups holds (every condition of that group). The gate or line adds that case's own metrics beside the name.",
        example: "cashout = profit_sol @volume >= 1.18, or (profit_sol @volume >= 0.71 and buy_sol @!volume [3s] >= 1)." },
    PartSpec { key: "always", title: "Always",
        summary: "Lines checked first, in every stage. One signal splits on age here: both bands are read in every stage.",
        example: "cashout and age_sec < 20 -> sell \"spike\". cashout and age_sec >= 20 -> go ride." },
    PartSpec { key: "stages", title: "Stages",
        summary: "The steps after the buy. The position starts in the first stage; each stage has its own lines and may have a deadline. A guard that starts later lives on the stage a line moves to.",
        example: "open -> ride." },
    PartSpec { key: "stage.on", title: "While in this stage",
        summary: "Lines checked on every print and every 200 ms tick while in this stage, top to bottom; the first that holds acts.",
        example: "burst and stage_sec <= 30 -> sell \"burst\"." },
    PartSpec { key: "stage.ends", title: "Deadline",
        summary: "When the stage ends: at a coin age, after time held, or after time in this stage. At the deadline the 'at end' lines run once, then the rule moves to 'then' (default: the next stage). A loop of deadlines needs a time-in-stage wait above 0 s in one of its stages.",
        example: "ends 30 s after this stage began, then hold." },
    PartSpec { key: "stage.at_end", title: "At the deadline",
        summary: "Lines checked once, when the deadline is reached. A checkpoint.",
        example: "m_flow.buy_sol @!volume < 1 -> sell \"dead at 20 s\"." },
    PartSpec { key: "line", title: "Line",
        summary: "If every condition holds: sell (all, or a percent of the first buy's bag) and/or move to another stage. A line that moves to the stage the position is already in does nothing there, unless it sells everything.",
        example: "pnl_pct >= 15 -> sell 50% of the first bag, go to stub." },
    PartSpec { key: "take_profit", title: "Take profit %",
        summary: "Shortcut for an always line: sell when m_position.pnl_pct reaches this.",
        example: "100 : sell at +100 %." },
    PartSpec { key: "stop_loss", title: "Stop loss %",
        summary: "Shortcut for an always line, checked before everything: sell when m_position.pnl_pct falls to minus this.",
        example: "30 : sell at -30 %." },
    PartSpec { key: "reentry", title: "Buy again",
        summary: "After a normal sell, wait this long and watch the coin again, up to a number of buys per coin.",
        example: "cooldown 30 s, at most 3 buys per coin." },
    PartSpec { key: "exclusive", title: "Exclusive",
        summary: "Do not buy while another rule holds this coin. With two exclusive rules, the higher priority wins.",
        example: "exclusive + priority 2 beats exclusive + priority 1." },
];

// ── The document ─────────────────────────────────────────────────────────────

/// Copy readings. The rule catalog omits them; the Copy page lists them.
fn is_copy_metric(id: Metric) -> bool {
    matches!(id, Metric::SoldBagPct | Metric::FlatBefore)
}

fn metric_json(m: &MetricSpec) -> Value {
    json!({
        "name": m.name,
        "path": m.path(),
        "phrase": m.phrase,
        "unit": m.unit.as_str(),
        "summary": m.summary,
        "example": m.example,
        "note": m.note,
        "tags": tag_use_str(m.tags),
        "tag_level": tag_level_str(m.tag_level),
        "spans": {
            "life": m.spans.life,
            "window": m.spans.window,
            "since_age": m.spans.since_age,
            "slice": m.spans.slice,
        },
        "monotonic": m.monotonic,
        "eq_tolerance": m.eq_tolerance,
        "hue": m.hue,
        "position": m.family == Family::Position,
    })
}

fn tag_use_str(t: TagUse) -> &'static str {
    match t {
        TagUse::None => "none",
        TagUse::Optional => "optional",
        TagUse::Required => "required",
    }
}

fn tag_level_str(t: TagLevel) -> &'static str {
    match t {
        TagLevel::Trade => "trade",
        TagLevel::Template => "template",
        TagLevel::WalletClass => "wallet_class",
    }
}

/// The whole vocabulary as JSON — `GET /api/meta/strategy-registry`. The frontend
/// renders every picker, tooltip, sentence and the Guide page from this and nothing
/// else.
pub fn registry_json() -> Value {
    let families: Vec<Value> = FAMILIES
        .iter()
        .map(|f| {
            let metrics: Vec<Value> = METRICS
                .iter()
                .filter(|m| m.family == f.id && !is_copy_metric(m.id))
                .map(metric_json)
                .collect();
            json!({
                "name": f.name,
                "title": f.title,
                "summary": f.summary,
                "example": f.example,
                "metrics": metrics,
            })
        })
        .collect();
    json!({
        "operators": ["<", "<=", ">", ">=", "=", "!="],
        "families": families,
        "copy": METRICS.iter().filter(|m| is_copy_metric(m.id)).map(metric_json).collect::<Vec<_>>(),
        "spans": super::span::span_kinds_json(),
        "tags": super::tags::config::tags_json(),
        "rule_parts": RULE_PARTS.iter().map(|p| json!({
            "key": p.key, "title": p.title, "summary": p.summary, "example": p.example,
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_order_matches_the_enum() {
        for (i, s) in METRICS.iter().enumerate() {
            assert_eq!(s.id as usize, i, "METRICS[{i}] is {:?}", s.id);
        }
        assert_eq!(METRICS.len(), Metric::StageSec as usize + 1);
    }

    #[test]
    fn family_order_matches_the_enum() {
        for (i, f) in FAMILIES.iter().enumerate() {
            assert_eq!(f.id as usize, i, "FAMILIES[{i}] is {:?}", f.id);
        }
    }

    #[test]
    fn paths_are_unique_and_resolve() {
        let mut seen = std::collections::BTreeSet::new();
        for s in METRICS {
            let p = s.path();
            assert!(seen.insert(p.clone()), "duplicate path {p}");
            assert_eq!(metric_by_path(&p), Some(s.id));
        }
    }

    /// The unit is the last word of the name — the naming rule the whole vocabulary
    /// stands on.
    #[test]
    fn every_name_ends_in_its_unit() {
        for s in METRICS {
            let ok = match s.unit {
                Unit::Seconds => s.name.ends_with("_sec"),
                Unit::Sol => s.name.ends_with("_sol"),
                Unit::Percent => s.name.ends_with("_pct"),
                Unit::Lamports => s.name.ends_with("_lamports"),
                Unit::Count => {
                    s.name.ends_with("_count")
                        || s.name.ends_with("_slots")
                        || s.name.starts_with("unique_")
                        || s.name == "trades_per_wallet"
                }
                Unit::Flag => !s.name.contains("_sec") && !s.name.ends_with("_sol") && !s.name.ends_with("_pct"),
            };
            assert!(ok, "{} is a {:?} metric", s.path(), s.unit);
        }
    }

    #[test]
    fn every_metric_is_explained_with_an_example() {
        for s in METRICS {
            assert!(!s.summary.is_empty(), "{} has no summary", s.path());
            // A phrase slots into a sentence: short, no trailing period, not a copy of the name.
            assert!(!s.phrase.is_empty() && s.phrase.len() <= 60 && !s.phrase.ends_with('.'), "{} phrase: {:?}", s.path(), s.phrase);
            assert!(s.example.starts_with(&s.path()) || s.example.contains(&format!("{}.", family_spec(s.family).name)),
                "{} example must show the metric in use: {}", s.path(), s.example);
            assert!(s.example.chars().any(|c| c.is_ascii_digit()), "{} example has no number", s.path());
        }
        for f in FAMILIES {
            assert!(!f.summary.is_empty() && !f.example.is_empty(), "{} unexplained", f.name);
        }
    }

    #[test]
    fn a_metric_that_needs_a_tag_says_so_in_its_summary_or_example() {
        for s in METRICS.iter().filter(|s| s.tags == TagUse::Required) {
            assert!(s.example.contains('@'), "{} requires a tag but its example shows none", s.path());
        }
    }

    #[test]
    fn position_metrics_take_no_tag_or_window() {
        for s in METRICS.iter().filter(|s| s.family == Family::Position) {
            assert_eq!(s.tags, TagUse::None);
            assert_eq!(s.spans, SpanUse::LIFE);
        }
    }
}

/// The registry document the frontend tests read (`fixtures/registry.json`), so the
/// UI's sentences, checks and pickers are tested against the real vocabulary. Rewrite
/// it with `REGEN_FIXTURES=1 cargo test -p hunter-engine registry_fixture`.
#[cfg(test)]
mod fixture {
    #[test]
    fn registry_fixture_is_current() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/registry.json");
        let want = serde_json::to_string_pretty(&super::registry_json()).expect("serializes") + "\n";
        if std::env::var_os("REGEN_FIXTURES").is_some() {
            std::fs::write(path, &want).expect("write fixture");
        }
        let have = std::fs::read_to_string(path).unwrap_or_default().replace("\r\n", "\n");
        assert!(
            have == want,
            "fixtures/registry.json is stale: run REGEN_FIXTURES=1 cargo test -p hunter-engine registry_fixture"
        );
    }
}
