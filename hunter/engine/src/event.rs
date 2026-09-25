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
use crate::metrics::{Ts, TradeLite};
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

/// Why a position closed. Persisted as its label ([`ExitReason::label`]):
/// `TakeProfit | StopLoss | Dead | Manual | Migrated`, or the label of the rule line
/// that sold (`"spike"`, `"top"`, or one generated from the line's first condition,
/// `m_flow.buy_sol @!volume [10s] >= 2`). Rows written before rule lines existed carry
/// labels like `stall > 3`; they read back as lines with that label.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExitReason {
    /// The `take_profit` shortcut: `m_position.pnl_pct` reached it.
    TakeProfit,
    /// The `stop_loss` shortcut: `m_position.pnl_pct` fell to minus it.
    StopLoss,
    /// A rule line sold; its label (interned, so the reason stays `Copy`).
    Line(&'static str),
    /// The coin was judged dead (liquidity gone + silent).
    Dead,
    /// A manual sell / stop-all closed it.
    Manual,
    /// The coin migrated off the curve.
    Migrated,
}

impl ExitReason {
    /// The persisted / displayed label.
    pub fn label(self) -> Cow<'static, str> {
        Cow::Borrowed(match self {
            Self::TakeProfit => "TakeProfit",
            Self::StopLoss => "StopLoss",
            Self::Dead => "Dead",
            Self::Manual => "Manual",
            Self::Migrated => "Migrated",
            Self::Line(label) => label,
        })
    }

    /// A rule line sold (not a shortcut, not the engine's own verdict, not a person).
    pub fn is_line(self) -> bool {
        matches!(self, Self::Line(_))
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

/// Read a persisted exit-reason label back: a named reason, else the label of the rule
/// line that wrote it. `None` only for an empty label.
pub fn parse_exit_reason(s: &str) -> Option<ExitReason> {
    Some(match s.trim() {
        "" => return None,
        "TakeProfit" => ExitReason::TakeProfit,
        "StopLoss" => ExitReason::StopLoss,
        "Dead" => ExitReason::Dead,
        "Manual" => ExitReason::Manual,
        "Migrated" => ExitReason::Migrated,
        other => ExitReason::Line(crate::intern::intern(other)),
    })
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
/// One creation build's previous-day tally — a row of the daily launch-build
/// stats, keyed by the build's [`trade_keys::ix_hash`](crate::metrics::trade_keys::ix_hash)
/// over its exact ordered creation labels. Delivered on a
/// [`Event::LaunchBuildStatsReloaded`]; `reduce` stamps
/// `build_prev_day_launches` / `build_prev_day_runner_bps` from it at
/// `TokenCreated`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchBuildStat {
    /// FNV-1a of the build's ordered creation `ix_labels`.
    pub build_hash: u64,
    /// Tokens of this build created on the previous UTC day.
    pub launches: u32,
    /// Of those, how many became runners (curve peak reserve at or above the
    /// runner threshold, reached at or after the minimum age, and before the day).
    pub runners: u32,
}

/// One build recipe's app on the previous day — a row of the daily build-breadth
/// table, keyed by [`trade_keys::build_hash`](crate::metrics::trade_keys::build_hash). The
/// app is [`trade_keys::recipe_app`](crate::metrics::trade_keys::recipe_app) (the recipe
/// itself for a direct pump.fun call), counted across every recipe it sent. Delivered
/// on a [`Event::BuildBreadthReloaded`]; `reduce` classes each row with
/// [`holder_book::is_public_app`](crate::metrics::holder_book::is_public_app) and
/// stamps [`TradeLite::build_day_public`](crate::metrics::TradeLite::build_day_public)
/// on every buy while a loaded rule reads `m_holder_book`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildBreadth {
    /// FNV-1a of the build recipe (ordered labels without setup, teardown, memos).
    pub build_hash: u64,
    /// Distinct wallets that bought through this recipe's app, on any token, on the
    /// previous UTC day.
    pub app_buyers: u32,
    /// Buy transactions sent through this recipe's app, on any token, on the previous
    /// UTC day.
    pub app_buys: u32,
}

impl LaunchBuildStat {
    /// The runner share in basis points, integer division — the exact value the
    /// `build_prev_day_runner_bps` axis carries, so SQL mirrors and the engine agree
    /// to the unit.
    pub fn runner_bps(self) -> u32 {
        if self.launches == 0 {
            0
        } else {
            (u64::from(self.runners) * 10_000 / u64::from(self.launches)) as u32
        }
    }
}

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
    ///
    /// `creator_stand_in_wallet_hash` is the creation slot's first buyer, present
    /// only when the creator wallet did NOT buy in that slot (or is unknown). The
    /// engine then treats that wallet as the creator for every creator-keyed metric
    /// (`m_flow_ix.creator_is_tagged`, `m_dump_ix.creator_is_listed`,
    /// `m_crowd_after_age`): a launch client that creates from a throwaway signer
    /// and buys from the operator's wallet has its operator, not its signer, as the
    /// creator. Producers compute it; the engine never guesses it.
    FirstSlotSettled {
        mint: Mint,
        buy_lamports: u64,
        sell_lamports: u64,
        at: Ts,
        creator_stand_in_wallet_hash: Option<u64>,
    },
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
    /// The launch-build stats the two `build_prev_day_*` fingerprint axes are
    /// stamped from changed (a new UTC day computed, or a boot load). Replaces the
    /// whole map; tokens already tracked keep the stamp they were born under.
    LaunchBuildStatsReloaded { stats: Arc<[LaunchBuildStat]> },
    /// The day's build breadth changed (a new UTC day computed, or a boot load).
    /// Replaces the whole table; a holder already classed keeps the class its first
    /// buy was stamped with, so no tracked token's reading moves.
    BuildBreadthReloaded { breadth: Arc<[BuildBreadth]> },
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

    #[test]
    fn every_reason_round_trips_through_its_label() {
        for r in [
            ExitReason::TakeProfit,
            ExitReason::StopLoss,
            ExitReason::Dead,
            ExitReason::Manual,
            ExitReason::Migrated,
            ExitReason::Line("spike"),
            ExitReason::Line("m_flow.buy_sol @!volume [10s] >= 2"),
        ] {
            assert_eq!(parse_exit_reason(&r.label()), Some(r));
        }
    }

    /// A label written before rule lines existed reads back as a line with that label,
    /// never as nothing: a stored reason that stopped resolving would read like a
    /// deleted metric.
    #[test]
    fn a_stored_label_from_before_lines_still_reads() {
        assert_eq!(parse_exit_reason("stall > 3"), Some(ExitReason::Line("stall > 3")));
        assert_eq!(parse_exit_reason(""), None);
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
