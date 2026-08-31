//! The engine's I/O vocabulary — the [`Event`]s the fold consumes and the
//! [`Effect`]s it emits. Live, replay, simulate, and sweep differ **only** in who
//! produces events and who consumes effects; the decision logic
//! ([`reduce`](crate::reduce::reduce)) is identical, so identical event streams
//! yield identical effect streams (plan §6).
//!
//! Determinism rules that keep that promise (a violation is a bug):
//! * [`IntentId`] is **derived**, never random: `(rule, mint, monotonic seq)`.
//! * Every timestamp arrives on an event (`at`/`now`) — the engine reads no clock.
//! * Effect order is reproducible: the fold iterates tokens/rules in sorted key
//!   order (see [`crate::state`]).

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate::cap::Cap;
use crate::fingerprint::{Fingerprint, FingerprintId};
use crate::grouping::TokenFingerprint;
use crate::metrics::evaluator::Operator;
use crate::metrics::{metric_id_by_name, MetricId, Ts, TradeLite};
use crate::rule_params::RuleParams;

/// A token mint address — the event stream's partition key. `Arc<str>` so cloning
/// it into events/effects/state keys is cheap, and its `Ord` gives the fold a
/// stable per-token iteration order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Mint(pub Arc<str>);

impl Mint {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Mint {
    fn from(s: &str) -> Self {
        Mint(Arc::from(s))
    }
}

/// Look a `Mint` up by its address without minting one.
///
/// `From<&str>` allocates and copies the 44-byte base58 address every time, so a
/// producer that built one per event paid a heap allocation per trade for a value
/// it already held. Keying a map by `Mint` and borrowing the key back is what turns
/// that into an `Arc` refcount bump. Sound because `Hash`/`Eq`/`Ord` are derived
/// over `Arc<str>`, which delegates to `str` — the borrowed and owned forms agree.
impl std::borrow::Borrow<str> for Mint {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl From<String> for Mint {
    fn from(s: String) -> Self {
        Mint(Arc::from(s.as_str()))
    }
}

impl std::fmt::Display for Mint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A rule's stable id (`strategy_rules.id`). Ids are minted in the DB, never here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RuleId(pub Uuid);

/// An engine-internal position id — derived, deterministic (a monotonic counter in
/// [`EngineState`](crate::state::EngineState)). The live adapter maps it to the
/// `strategy_positions.id` UUID; replay/sweep use it directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PositionId(pub u64);

/// A submit intent's id: the correlation token between a `SubmitBuy`/`SubmitSell`
/// effect and the `FillConfirmed`/`FillFailed` event that resolves it. **Derived**
/// from `(rule, mint, seq)` where `seq` is a monotonic counter — so a retry after a
/// failure is a *distinct* intent, never a collision.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct IntentId {
    pub rule: RuleId,
    pub mint: Mint,
    pub seq: u64,
}

/// Execution mode of a rule. The engine does **not** branch on it (parity); it
/// rides along so the effect consumer can route paper vs real.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TradeMode {
    Paper,
    Real,
}

/// Why a position closed. Persisted as a short string label ([`ExitReason::label`]):
/// `TakeProfit | StopLoss | Dead | Manual | Migrated | {name}[({w}s)] {op} {value}`
/// (e.g. `stall > 3`, `trail >= 20`, `untagged_buy(2s) >= 0.9`). Legacy rows may still
/// store bare `Metrics`, the brief `{name}{op}` form (`stall>`), or a windowed exit
/// with no `({w}s)` — see [`parse_metric_exit_label`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExitReason {
    /// Price reached `entry_price · (1 + take_profit/100)`.
    TakeProfit,
    /// Price fell to `entry_price · (1 − stop_loss/100)`.
    StopLoss,
    /// An exit metric condition held — name, operator, and authored threshold.
    Metrics {
        metric: MetricId,
        operator: Operator,
        /// Authored condition threshold (not the live metric reading).
        value: f64,
        /// Trailing-window size of the req that fired, `None` for a static metric.
        ///
        /// Load-bearing, not decoration: a dynamic group and its lifetime twin share
        /// every metric *name* (`m_flow_ix.untagged_buy` and
        /// `m_flow_ix_window.untagged_buy` are both `"untagged_buy"`), so without the
        /// window the label names a monotone lifetime total when what actually fired
        /// was a burst over `window` seconds — two readings that move in opposite
        /// ways. Multi-window rules make it worse: several reqs print identically.
        window: Option<crate::metrics::WindowSpec>,
    },
    /// The token was judged dead (liquidity gone + silent).
    Dead,
    /// A manual sell / stop-all closed it.
    Manual,
    /// The token migrated off the curve.
    Migrated,
}

impl ExitReason {
    /// Persisted / displayed label. Ladder reasons keep PascalCase; metric exits
    /// are spaced `name[({w}s)] op value` (`stall > 3`, `untagged_buy(2s) >= 0.9`).
    pub fn label(self) -> Cow<'static, str> {
        match self {
            Self::TakeProfit => Cow::Borrowed("TakeProfit"),
            Self::StopLoss => Cow::Borrowed("StopLoss"),
            Self::Dead => Cow::Borrowed("Dead"),
            Self::Manual => Cow::Borrowed("Manual"),
            Self::Migrated => Cow::Borrowed("Migrated"),
            Self::Metrics {
                metric,
                operator,
                value,
                window,
            } => Cow::Owned(format_metric_exit_label(metric, operator, value, window)),
        }
    }

    pub fn is_metrics(self) -> bool {
        matches!(self, Self::Metrics { .. })
    }
}

/// Compact threshold for labels: integers without `.0`, else trimmed decimals.
pub fn format_metric_threshold(v: f64) -> String {
    if !v.is_finite() {
        return v.to_string();
    }
    if v == 0.0 {
        return "0".to_string();
    }
    if v.fract() == 0.0 && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Spaced metric-exit label SSOT: `name[({w}s)] op value`.
///
/// The `({w}s)` qualifier rides on the **name**, not as a fourth token, so the
/// existing three-token split still parses every form and a reader scanning a
/// column of reasons sees the metric and its window as one word.
pub fn format_metric_exit_label(
    metric: MetricId,
    operator: Operator,
    value: f64,
    window: Option<crate::metrics::WindowSpec>,
) -> String {
    format!(
        "{} {} {}",
        format_metric_exit_name(metric, window),
        operator.symbol(),
        format_metric_threshold(value)
    )
}

/// The name half of a metric exit label: `untagged_buy` or `untagged_buy(2s)`.
/// Shared with every UI that renders a condition beside its reason, so a chart
/// legend and a persisted reason can never disagree about which req is meant.
pub fn format_metric_exit_name(
    metric: MetricId,
    window: Option<crate::metrics::WindowSpec>,
) -> String {
    match window {
        // The qualifier is `WindowSpec::label` in parentheses — the SAME grammar a
        // `?windows=` query and a chart legend use, so a reason written here parses
        // back through `WindowSpec::parse` by construction rather than by agreement.
        Some(w) if w.size.is_finite() && w.size > 0.0 => {
            format!("{}({})", metric.name(), w.label())
        }
        _ => metric.name().to_string(),
    }
}

impl fmt::Display for ExitReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label())
    }
}

impl Serialize for ExitReason {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.label())
    }
}

impl<'de> Deserialize<'de> for ExitReason {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        parse_exit_reason(&s).ok_or_else(|| {
            serde::de::Error::custom(format!("unknown exit reason: {s}"))
        })
    }
}

/// Parse a persisted exit-reason label. Accepts ladder names and spaced
/// `name op value` detail forms. Bare legacy `"Metrics"` / compact `stall>` are
/// recognized by [`is_metric_exit_label`] for rollups but not reconstructed here
/// without a threshold (compact → threshold `0` only when parsing for typed use
/// is impossible — prefer [`parse_metric_exit_label`]).
pub fn parse_exit_reason(s: &str) -> Option<ExitReason> {
    match s {
        "TakeProfit" => Some(ExitReason::TakeProfit),
        "StopLoss" => Some(ExitReason::StopLoss),
        "Dead" => Some(ExitReason::Dead),
        "Manual" => Some(ExitReason::Manual),
        "Migrated" => Some(ExitReason::Migrated),
        other => parse_metric_exit_label(other).map(|(metric, operator, value, window)| {
            ExitReason::Metrics {
                metric,
                operator,
                value,
                window,
            }
        }),
    }
}

/// True for bare legacy `"Metrics"`, spaced `name op value`, or brief `{name}{op}`.
pub fn is_metric_exit_label(s: &str) -> bool {
    s == "Metrics" || parse_metric_exit_label(s).is_some()
}

const METRIC_OPS: &[(&str, Operator)] = &[
    (">=", Operator::Gte),
    ("<=", Operator::Lte),
    ("!=", Operator::Ne),
    (">", Operator::Gt),
    ("<", Operator::Lt),
    ("=", Operator::Eq),
];

/// Parse `name[({w}s)] op value` (`stall > 3`, `untagged_buy(2s) >= 0.9`) or legacy
/// compact `name{op}` (`stall>`). Compact forms return `value = 0.0` as a
/// placeholder (rollup detection only).
///
/// The window is what disambiguates a dynamic group from its lifetime twin, so it
/// also **selects** the `MetricId`: qualified ⇒ the `Dynamic`-group metric of that
/// name, bare ⇒ the `Static` one. Rows written before the qualifier existed are bare
/// whichever group fired, so a bare name falls back to any group — the same answer
/// this returned before, never a parse failure on a real stored reason.
pub fn parse_metric_exit_label(
    s: &str,
) -> Option<(MetricId, Operator, f64, Option<crate::metrics::WindowSpec>)> {
    let s = s.trim();
    // Spaced form: split on whitespace into [name, op, value, ...].
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() >= 3 {
        let (name, window) = split_window_qualifier(parts[0]);
        let op_sym = parts[1];
        let val_str = parts[2];
        let metric = metric_id_by_name_windowed(name, window.is_some())?;
        let operator = METRIC_OPS
            .iter()
            .find(|(sym, _)| *sym == op_sym)
            .map(|(_, op)| *op)?;
        let value: f64 = val_str.parse().ok()?;
        return Some((metric, operator, value, window));
    }
    // Legacy compact `{name}{op}` (no threshold).
    for &(sym, op) in METRIC_OPS {
        if let Some(name) = s.strip_suffix(sym) {
            if name.is_empty() || name.chars().any(|c| c.is_whitespace()) {
                continue;
            }
            let (name, window) = split_window_qualifier(name);
            if let Some(metric) = metric_id_by_name_windowed(name, window.is_some()) {
                return Some((metric, op, 0.0, window));
            }
        }
    }
    None
}

/// Split `untagged_buy(2s)` into `("untagged_buy", Some(2s))`, `buy(30sl@1)` into the
/// 30-slot span lagged by one, `buy(1p)` into the single-print span. Anything that is
/// not a well-formed suffix is left on the name, so an unrecognised qualifier fails
/// the name lookup instead of silently parsing as a bare metric. A label written
/// before slots existed has no `sl`, no `p` and no `@`, so it still parses to exactly
/// the seconds window it always meant.
fn split_window_qualifier(token: &str) -> (&str, Option<crate::metrics::WindowSpec>) {
    let Some(open) = token.find('(') else {
        return (token, None);
    };
    let Some(inner) = token[open + 1..].strip_suffix(')') else {
        return (token, None);
    };
    // A bare number inside the parens would be a legacy seconds span, which
    // `WindowSpec::parse` accepts — but this position never held one: the qualifier
    // has carried its `s` since it was introduced, and `untagged_buy(2)` is exactly the
    // malformed shape that must fail the name lookup rather than resolve.
    match crate::metrics::WindowSpec::parse(inner) {
        Some(w) if inner.trim().parse::<f64>().is_err() => (&token[..open], Some(w)),
        _ => (token, None),
    }
}

/// Resolve a metric name to the dynamic (windowed) or static variant.
///
/// `windowed = false` prefers a static group but accepts a dynamic one, because a
/// pre-qualifier row stored a bare name whichever group fired and rejecting it would
/// turn old reasons into unparseable strings.
fn metric_id_by_name_windowed(name: &str, windowed: bool) -> Option<MetricId> {
    let want = if windowed {
        crate::metrics::MetricKind::Dynamic
    } else {
        crate::metrics::MetricKind::Static
    };
    crate::metrics::metric_id_by_name_kind(name, want).or_else(|| metric_id_by_name(name))
}

/// Why a submitted buy/sell did not confirm. Drives the fold's retry policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FillFailReason {
    /// The tx reverted on-chain (nothing happened) — safe to retry / book failed.
    Reverted,
    /// No fill observed within the watchdog window (may or may not have landed).
    Timeout,
    /// A sell whose clearing the feed never confirmed and that did not revert —
    /// re-selling risks a double-sell, so the position is alarmed, never re-sold.
    Unconfirmed,
    /// Structural / non-retryable failure (e.g. `StopFeeBurn` from
    /// `classify_swap_revert`) — give up immediately; a blind resend would only
    /// re-pay fees.
    Fatal,
}

/// A confirmed fill (entry or exit). `sol` is the SOL spent (entry) or received
/// (exit); `price` is what the fill actually paid — the leg's `price_per_token`,
/// i.e. the EXECUTION price, the same basis
/// [`TradeLite::price`](crate::metrics::TradeLite::price) carries, so `m_position`
/// marks an entry and the tape it is marked against in one series.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Fill {
    pub price: f64,
    pub sol: f64,
    pub token_amount: u64,
    pub at: Ts,
}

/// A rule as the engine consumes it — the DB row's columns plus **parsed**
/// [`RuleParams`] (parsed once at load, never per event; plan §5). Delivered on a
/// [`Event::RulesReloaded`].
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedRule {
    pub id: RuleId,
    pub fingerprint_id: FingerprintId,
    pub trade_mode: TradeMode,
    /// Buy size per fired token, exact lamports.
    pub buy_amount_lamports: u64,
    /// Cap on concurrently open+in-flight tokens (`0` ⇒ unlimited).
    pub max_concurrent_tokens: u32,
    /// Cap on total successful entries over the rule's life (`0` ⇒ unlimited).
    /// With re-entry configured this counts **episodes**, not distinct tokens —
    /// each re-entry is one more entry against the cap (the safer direction:
    /// the cap fires sooner). One-shot rules are unaffected (1 token = 1 entry).
    pub max_total_tokens: u32,
    pub params: RuleParams,
    /// When `false`, the rule stays loaded for exit/drain but must not arm new
    /// entries (paused / stopped-with-open-positions drain set).
    pub entry_enabled: bool,
}

impl LoadedRule {
    /// Effective concurrency cap (`0` in the DB means unlimited). The ONE decode
    /// of that sentinel — see [`crate::cap::Cap`].
    pub fn concurrent_cap(&self) -> Cap {
        Cap::zero_unlimited(self.max_concurrent_tokens)
    }

    /// Effective lifetime cap (`0` in the DB means unlimited). The ONE decode of
    /// that sentinel: every reader (the fold's cap check, the SSE/`strategy_runs`
    /// snapshot) goes through here rather than re-deriving `!= 0`.
    pub fn total_cap(&self) -> Cap {
        Cap::zero_unlimited(self.max_total_tokens)
    }
}

/// A manual position's optional exit config (`{tp_pct, sl_pct}`). Compiled into a
/// one-off per-position exit rule (the same TP/SL desugar bot rules use), so a
/// manual position with TP/SL gets the full engine exit stack including the
/// Dead-exit. Both `None` ⇒ tracked-only (no auto-exit of any kind).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ManualExit {
    pub tp_pct: Option<f64>,
    pub sl_pct: Option<f64>,
}

impl ManualExit {
    /// Whether any exit is configured (else the position is tracked-only).
    pub fn is_some(&self) -> bool {
        self.tp_pct.is_some() || self.sl_pct.is_some()
    }
}

/// The ordered input stream. One variant per thing that can change a decision.
///
/// `Clone` is cheap by construction — the only non-`Copy` payloads are an `Arc`
/// (rule/fingerprint slices) or a small `Box` — and it is what lets a differential
/// test replay one recorded stream through two engine configurations (see
/// `tests/settled_ticks.rs`).
#[derive(Clone)]
pub enum Event {
    /// A new token appeared. `fp` carries its instant creation axes (the
    /// first-slot axes are still unknown — resolved by [`Event::FirstSlotSettled`]).
    /// `creator_wallet_hash` feeds the volume-flow classifier (V1+); `None` when
    /// the creator address is unknown (old logs / lake rows without a creator).
    /// `identity` is the `(name, symbol)` key the duplicate-identity guard matches
    /// on ([`crate::identity::token_identity_hash`]); `None` when either half is
    /// blank or the producer has no metadata for the mint.
    /// `creation_slot` seeds `m_burst_wave` so the create slot is not a fireable
    /// wave; `None` on old logs (gap is then measured from the first mem buy).
    TokenCreated {
        mint: Mint,
        fp: Box<TokenFingerprint>,
        at: Ts,
        creator_wallet_hash: Option<u64>,
        identity: Option<crate::identity::IdentityHash>,
        creation_slot: Option<u64>,
    },
    /// The token's creation slot closed; the two first-slot SOL sums are now known.
    /// Resolves any fingerprint whose identity includes a first-slot axis (plan §2.2).
    FirstSlotSettled { mint: Mint, buy_lamports: u64, sell_lamports: u64, at: Ts },
    /// A trade printed for the token.
    Trade { mint: Mint, trade: TradeLite },
    /// The clock tick (or a replay's synthetic tick; cadence = `TICK_MS`). Advances every tracked
    /// token to `now` so quiet-token metrics (stall/time/decayed flows) fire.
    Tick { now: Ts },
    /// A submitted buy/sell confirmed with a fill.
    FillConfirmed { intent: IntentId, fill: Fill },
    /// A submitted buy/sell failed to confirm.
    ///
    /// `at` is when the failure was observed — the instant an entry retry is
    /// re-qualified against the rule's entry conditions (see the `FillFailed`
    /// arm of `reduce`). It is the engine's only "the clock moved" input on this
    /// event, so a live producer must always supply it. `None` skips the
    /// re-qualification and retries blind — reserved for replaying pre-`at`
    /// JSONL lines, which carry no timestamp to judge against.
    FillFailed { intent: IntentId, reason: FillFailReason, at: Option<Ts> },
    /// The token migrated off the curve.
    Migrated { mint: Mint, at: Ts },
    /// The active rule set (and the fingerprints they reference) changed. Parsed
    /// once at load; the engine recompiles per-rule metric requests + derived
    /// bounds here, never per event.
    RulesReloaded { rules: Arc<[LoadedRule]>, fps: Arc<[Fingerprint]> },
    /// A manual (operator) buy episode injected by the Console. Bypasses
    /// fingerprint arming, entry conditions, and rule caps (they're the user's
    /// call) — but from here on it IS a bot buy: the same `EntryPending` arm,
    /// retry policy, fill confirm, and reaper coverage. `rule` is a **fresh
    /// per-episode id** minted by the adapter (never a real `strategy_rules`
    /// row); it keys the arm, the intents, and the synthesized exit rule.
    ManualBuy {
        mint: Mint,
        rule: RuleId,
        lamports: u64,
        at: Ts,
        exit: Option<ManualExit>,
    },
    /// Set / replace / clear a manual position's TP/SL config post-entry
    /// (`[+TP/SL]` on a Holding row). `None` ⇒ back to tracked-only.
    SetManualExit { position: PositionId, exit: Option<ManualExit> },
    /// A manual sell / stop-all targeting one open position.
    /// `portion: All` = Sell ALL (legacy); `BpsOfInitial` = Console "Sell N%".
    ManualClose { position: PositionId, portion: Portion },
    /// One open position whose token bag was already cleared **off-chain** (an
    /// external / manual wallet sell) — book it closed at `fill` WITHOUT emitting a
    /// `SubmitSell` (the bag is gone; a sell would only revert into an empty wallet).
    /// The live adapter resolves `fill` from the wallet's last sell (or the entry as
    /// a fallback). Mirrors the retired `reconcile_externally_cleared_mint`.
    ExternallyCleared { position: PositionId, fill: Fill },
}

/// How much of a held bag a sell should close. `All` is today's full-bag path;
/// `BpsOfInitial` sells that many basis points of the **initial** entry bag
/// (partial exits / scale-out / manual Sell N%). See `docs/plans/strategies/partial-exits.md`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Portion {
    /// Close 100% of the remaining bag.
    #[default]
    All,
    /// Sell `bps / 10_000` of the initial token amount (clamped to remaining at exec).
    BpsOfInitial(u16),
}

impl Portion {
    /// Whether this portion leaves a stub behind (a Holding-preserving fill).
    pub fn is_partial(self) -> bool {
        matches!(self, Self::BpsOfInitial(_))
    }

    /// Exact integer token units to sell for this portion.
    ///
    /// `BpsOfInitial` sizes off the **initial** bag (`initial * bps / 10_000`) and
    /// clamps to the still-held remainder (`initial - sold`). `All` sells the
    /// entire remainder. Shared by live exec, paper, lab replay, and orphan sells
    /// — one formula so legs cannot drift (see `docs/plans/strategies/partial-exits.md`).
    pub fn token_amount(self, initial: u64, sold: u64) -> u64 {
        let remaining = initial.saturating_sub(sold);
        match self {
            Self::All => remaining,
            Self::BpsOfInitial(bps) => {
                let want = (u128::from(initial) * u128::from(bps) / 10_000) as u64;
                want.min(remaining)
            }
        }
    }
}

/// The ordered output stream. `SubmitBuy`/`SubmitSell` are the trade *decisions*
/// (the golden-log spec asserts on these); `PositionUpdate`/`ArmedChanged` are the
/// persistence + SSE side-effects the adapters consume.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Buy `lamports` of `mint` for `rule`. The consumer submits and, on fill,
    /// feeds a `FillConfirmed { intent, .. }` back into the engine.
    SubmitBuy { intent: IntentId, rule: RuleId, mint: Mint, lamports: u64 },
    /// Sell (a portion of) the position out, `reason` recording why.
    /// `portion: All` is the legacy full close; `BpsOfInitial` is a scale-out leg.
    SubmitSell {
        intent: IntentId,
        position: PositionId,
        reason: ExitReason,
        portion: Portion,
    },
    /// A position lifecycle transition — the PG writer + position SSE consume it.
    PositionUpdate(PositionDelta),
    /// A (token, rule) arming transition — the live-monitor SSE consumes it.
    ArmedChanged(ArmedDelta),
}

/// A position's current lifecycle status, mirroring the `strategy_positions`
/// vocabulary the PG writer persists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PositionStatus {
    /// A snipe buy is in flight (durable "buy submitted" marker).
    BuySubmitted,
    /// Entry filled; SOL deployed and held.
    Holding,
    /// A sell is in flight.
    ExitPending,
    /// Confirmed exit fill — terminal.
    End,
    /// The buy never filled (entry exhausted / fatal) — terminal, no SOL was
    /// deployed. Excluded from realized PnL (there was never a position).
    EntryFailed,
    /// The sell reverted / gave up and the bag is **still held** — engine-terminal
    /// (the arm is dropped) but the position stays OPEN: the reaper re-drives it,
    /// then parks it for a manual Retry / Dump / Write-off.
    ExitStuck,
    /// The sell may or may not have cleared and the feed never confirmed —
    /// engine-terminal, OPEN + alarmed for manual review, never auto-re-sold.
    ExitUnconfirmed,
}

/// One position lifecycle transition. `fill` is the entry fill on `Holding` and
/// the exit fill on `End` (or a partial-leg fill on a Holding-preserving update);
/// `reason` accompanies the exit statuses.
#[derive(Debug, Clone, PartialEq)]
pub struct PositionDelta {
    pub position: PositionId,
    pub rule: RuleId,
    pub mint: Mint,
    pub status: PositionStatus,
    pub fill: Option<Fill>,
    pub reason: Option<ExitReason>,
    /// The intent that drove this transition (for adapter correlation), when one did.
    pub intent: Option<IntentId>,
    /// Scale-out stage index after this transition (`None` = legacy / no ladder).
    /// Set on partial ExitPending submits and on Holding-preserving partial fills.
    pub stage: Option<u8>,
}

/// Why a (token, rule) arming ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisarmReason {
    /// Dead-token verdict (liquidity gone + silent).
    Dead,
    /// The token migrated off the curve before entry.
    Migrated,
    /// A monotonic entry bound was permanently crossed (e.g. `time < 30` at 30 s) —
    /// the entry can never re-satisfy (plan §2.2 "derived unsatisfiability").
    Unsatisfiable,
    /// Rule paused or stopped — entries off; open positions may still drain.
    Paused,
    /// A different mint with the same `(name, symbol)` was traded inside the
    /// duplicate-identity guard's window (`strategy.skip_duplicate_identity`).
    ///
    /// Terminal, unlike `exclusive`'s wait: the block lifts only when the window
    /// expires days later, long past any curve token's life, so staying `Armed`
    /// would re-ask a fixed question on every tick forever.
    DuplicateIdentity,
}

/// A (token, rule) arming transition for the live monitor. Entry/exit are carried
/// by [`PositionDelta`]; this covers arm and disarm.
#[derive(Debug, Clone, PartialEq)]
pub struct ArmedDelta {
    pub mint: Mint,
    pub rule: RuleId,
    pub state: ArmedStateTag,
    /// What the rule was short of when it gave up — set **only** on
    /// [`DisarmReason::Unsatisfiable`], the one ending whose name does not answer
    /// "why". Every other transition carries `None`: `dead`, `migrated`, `paused`
    /// and `duplicate_identity` each state their own cause.
    ///
    /// Boxed so the common `None` costs a pointer: this rides an effect the fold
    /// pushes on every arm and disarm, and only one of those cases ever fills it.
    pub detail: Option<Box<crate::arm::EntryBlockers>>,
}

/// The armed-side state an [`ArmedDelta`] reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArmedStateTag {
    /// The (token, rule) is now armed and evaluating entry.
    Armed,
    /// The (token, rule) disarmed for the given reason.
    Disarmed(DisarmReason),
}

#[cfg(test)]
mod exit_label_tests {
    use super::*;
    use crate::metrics::{metric_id_by_name_kind, MetricKind};

    fn win(name: &str) -> MetricId {
        metric_id_by_name_kind(name, MetricKind::Dynamic).expect("dynamic metric")
    }
    fn stat(name: &str) -> MetricId {
        metric_id_by_name_kind(name, MetricKind::Static).expect("static metric")
    }

    /// The whole point of the qualifier: `m_flow_ix.untagged_buy` and
    /// `m_flow_ix_window.untagged_buy` share a name, so a label without the window
    /// names a monotone lifetime total when a 2 s burst is what fired.
    #[test]
    fn window_qualifier_round_trips_and_selects_the_right_metric() {
        let windowed = ExitReason::Metrics {
            metric: win("untagged_buy"),
            operator: Operator::Gte,
            value: 0.9,
            window: Some(crate::metrics::WindowSpec::secs(2.0)),
        };
        assert_eq!(windowed.label(), "untagged_buy(2s) >= 0.9");
        assert_eq!(parse_exit_reason(&windowed.label()), Some(windowed));

        let lifetime = ExitReason::Metrics {
            metric: stat("untagged_buy"),
            operator: Operator::Gte,
            value: 0.9,
            window: None,
        };
        assert_eq!(lifetime.label(), "untagged_buy >= 0.9");
        assert_eq!(parse_exit_reason(&lifetime.label()), Some(lifetime));

        // The two must not collapse onto each other.
        assert_ne!(windowed.label(), lifetime.label());
        assert_ne!(win("untagged_buy"), stat("untagged_buy"));
    }

    /// One size, three bases, three labels. `1s`, `1sl` and `1p` read different tape,
    /// so an operator reading a stored reason must be able to tell which fired.
    #[test]
    fn each_basis_labels_itself_apart_at_the_same_size() {
        use crate::metrics::WindowSpec;
        let label = |w| {
            ExitReason::Metrics {
                metric: win("untagged_buy"),
                operator: Operator::Gte,
                value: 1.0,
                window: Some(w),
            }
            .label()
            .into_owned()
        };
        let (sec, slot, print) = (
            label(WindowSpec::secs(1.0)),
            label(WindowSpec::slots(1.0, 0.0)),
            label(WindowSpec::prints(1.0, 0.0)),
        );
        assert_eq!(sec, "untagged_buy(1s) >= 1");
        assert_eq!(slot, "untagged_buy(1sl) >= 1");
        assert_eq!(print, "untagged_buy(1p) >= 1");
        assert_eq!(std::collections::BTreeSet::from([&sec, &slot, &print]).len(), 3);
    }

    #[test]
    fn fractional_and_large_windows_survive_the_round_trip() {
        use crate::metrics::WindowSpec;
        for w in [
            WindowSpec::secs(0.5),
            WindowSpec::secs(2.0),
            WindowSpec::secs(45.0),
            WindowSpec::secs(300.0),
            // A slot span and a lagged one must survive the same round trip, or a
            // persisted exit reason cannot name which read fired.
            WindowSpec::slots(1.0, 0.0),
            WindowSpec::slots(30.0, 1.0),
            // Same for a print span. `1p` is the one-transaction read, and its
            // suffix has to stay distinguishable from `1s` and `1sl` - three spans
            // that report different tape must never label identically.
            WindowSpec::prints(1.0, 0.0),
            WindowSpec::prints(20.0, 1.0),
        ] {
            let r = ExitReason::Metrics {
                metric: win("untagged_buy"),
                operator: Operator::Lte,
                value: 0.2,
                window: Some(w),
            };
            assert_eq!(parse_exit_reason(&r.label()), Some(r), "window {w:?}");
        }
    }

    /// Rows written before the qualifier existed are bare whichever group fired.
    /// They must keep parsing — a stored reason that stops resolving reads to the
    /// console exactly like a deleted metric.
    #[test]
    fn legacy_labels_still_parse() {
        assert!(matches!(parse_exit_reason("stall > 3"), Some(ExitReason::Metrics { .. })));
        assert!(matches!(parse_exit_reason("untagged_buy >= 0.9"), Some(ExitReason::Metrics { .. })));
        assert!(matches!(parse_exit_reason("stall>"), Some(ExitReason::Metrics { .. })));
        assert_eq!(parse_exit_reason("TakeProfit"), Some(ExitReason::TakeProfit));
        assert!(is_metric_exit_label("Metrics"));
        assert!(is_metric_exit_label("untagged_buy(2s) >= 0.9"));
        assert!(!is_metric_exit_label("Dead"));
    }

    /// A qualifier that is not `({number}s)` stays on the name, so it fails the
    /// lookup instead of silently parsing as the bare metric.
    #[test]
    fn malformed_qualifiers_do_not_degrade_to_the_lifetime_metric() {
        for s in ["untagged_buy(2) >= 0.9", "untagged_buy(xs) >= 0.9", "untagged_buy(2s >= 0.9"] {
            assert_eq!(parse_exit_reason(s), None, "{s}");
        }
    }
}

#[cfg(test)]
mod portion_tests {
    use super::Portion;

    #[test]
    fn bps_of_initial_clamps_to_remaining() {
        let initial = 10_000u64;
        assert_eq!(Portion::BpsOfInitial(7000).token_amount(initial, 0), 7000);
        assert_eq!(Portion::BpsOfInitial(7000).token_amount(initial, 7000), 3000);
        assert_eq!(Portion::All.token_amount(initial, 7000), 3000);
        assert_eq!(Portion::All.token_amount(initial, 0), 10_000);
        assert_eq!(Portion::BpsOfInitial(5000).token_amount(initial, 8000), 2000);
    }
}
