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

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

/// Why a position closed. Persisted vocabulary (plan §4):
/// `TakeProfit | StopLoss | Metrics | Dead | Manual | Migrated`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExitReason {
    /// Price reached `entry_price · (1 + take_profit/100)`.
    TakeProfit,
    /// Price fell to `entry_price · (1 − stop_loss/100)`.
    StopLoss,
    /// All of the rule's `exit` metric conditions held.
    Metrics,
    /// The token was judged dead (liquidity gone + silent).
    Dead,
    /// A manual sell / stop-all closed it.
    Manual,
    /// The token migrated off the curve.
    Migrated,
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
}

/// A confirmed fill (entry or exit). `sol` is the SOL spent (entry) or received
/// (exit); `price` is the canonical curve-spot at the fill.
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
    /// Cap on concurrently open+in-flight tokens (`0` ⇒ treated as `1`).
    pub max_concurrent_tokens: u32,
    /// Cap on total successful entries over the rule's life (`0` ⇒ unlimited).
    pub max_total_tokens: u32,
    pub params: RuleParams,
}

impl LoadedRule {
    /// Effective concurrency cap (`0` in the DB means the default of 1).
    pub fn concurrent_cap(&self) -> u32 {
        if self.max_concurrent_tokens == 0 {
            1
        } else {
            self.max_concurrent_tokens
        }
    }
}

/// The ordered input stream. One variant per thing that can change a decision.
pub enum Event {
    /// A new token appeared. `fp` carries its instant creation axes (the
    /// first-slot axes are still unknown — resolved by [`Event::FirstSlotSettled`]).
    TokenCreated { mint: Mint, fp: Box<TokenFingerprint>, at: Ts },
    /// The token's creation slot closed; the two first-slot SOL sums are now known.
    /// Resolves any fingerprint whose identity includes a first-slot axis (plan §2.2).
    FirstSlotSettled { mint: Mint, buy_lamports: u64, sell_lamports: u64, at: Ts },
    /// A trade printed for the token.
    Trade { mint: Mint, trade: TradeLite },
    /// The 500 ms clock tick (or a replay's synthetic tick). Advances every tracked
    /// token to `now` so quiet-token metrics (stall/time/decayed flows) fire.
    Tick { now: Ts },
    /// A submitted buy/sell confirmed with a fill.
    FillConfirmed { intent: IntentId, fill: Fill },
    /// A submitted buy/sell failed to confirm.
    FillFailed { intent: IntentId, reason: FillFailReason },
    /// The token migrated off the curve.
    Migrated { mint: Mint, at: Ts },
    /// The active rule set (and the fingerprints they reference) changed. Parsed
    /// once at load; the engine recompiles per-rule metric requests + derived
    /// bounds here, never per event.
    RulesReloaded { rules: Arc<[LoadedRule]>, fps: Arc<[Fingerprint]> },
    /// A manual sell / stop-all targeting one open position.
    ManualClose { position: PositionId },
    /// One open position whose token bag was already cleared **off-chain** (an
    /// external / manual wallet sell) — book it closed at `fill` WITHOUT emitting a
    /// `SubmitSell` (the bag is gone; a sell would only revert into an empty wallet).
    /// The live adapter resolves `fill` from the wallet's last sell (or the entry as
    /// a fallback). Mirrors the retired `reconcile_externally_cleared_mint`.
    ExternallyCleared { position: PositionId, fill: Fill },
}

/// The ordered output stream. `SubmitBuy`/`SubmitSell` are the trade *decisions*
/// (the golden-log spec asserts on these); `PositionUpdate`/`ArmedChanged` are the
/// persistence + SSE side-effects the adapters consume.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Buy `lamports` of `mint` for `rule`. The consumer submits and, on fill,
    /// feeds a `FillConfirmed { intent, .. }` back into the engine.
    SubmitBuy { intent: IntentId, rule: RuleId, mint: Mint, lamports: u64 },
    /// Sell the position out, `reason` recording why.
    SubmitSell { intent: IntentId, position: PositionId, reason: ExitReason },
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
    /// The sell reverted / gave up with nothing sold — terminal, loss booked.
    ExitFailed,
    /// The sell may or may not have cleared and the feed never confirmed —
    /// terminal, alarmed for manual review, never auto-re-sold.
    ExitUnconfirmed,
}

/// One position lifecycle transition. `fill` is the entry fill on `Holding` and
/// the exit fill on `End`; `reason` accompanies the exit statuses.
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
}

/// A (token, rule) arming transition for the live monitor. Entry/exit are carried
/// by [`PositionDelta`]; this covers arm and disarm.
#[derive(Debug, Clone, PartialEq)]
pub struct ArmedDelta {
    pub mint: Mint,
    pub rule: RuleId,
    pub state: ArmedStateTag,
}

/// The armed-side state an [`ArmedDelta`] reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArmedStateTag {
    /// The (token, rule) is now armed and evaluating entry.
    Armed,
    /// The (token, rule) disarmed for the given reason.
    Disarmed(DisarmReason),
}
