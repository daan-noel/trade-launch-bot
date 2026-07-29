//! The on-disk event-log **format** — the single serializable projection of
//! [`Event`] that the live recorder writes and the lab inspector reads (plan
//! decision 12, phases 4.6 + 6.1).
//!
//! Living in the pure engine crate makes it the SSOT for the log wire shape: the
//! `live` recorder ([`crate::event_log::LoggedEvent::from_event`] → JSONL) and the
//! `lab` time-travel debugger ([`LoggedEvent::into_event`] ← JSONL) both speak this
//! one type, so the two can never drift. The file I/O (rotation, retention,
//! recovery) stays in the impure bins — only the format lives here.
//!
//! Two events are deliberately **not** loggable: `Tick` (regenerable — replay
//! derives ticks from timestamps) and `RulesReloaded` (rules are reloaded from PG,
//! never the log). Every other variant round-trips.

use serde::{Deserialize, Serialize};

use crate::event::{
    Event, Fill, FillFailReason, IntentId, ManualExit, Mint, Portion, PositionId, RuleId,
};
use crate::grouping::TokenFingerprint;
use crate::metrics::{Ts, TradeLite};

/// The serializable subset of [`Event`] the log persists (no `Tick`, no
/// `RulesReloaded`). Every inner type is already `Serialize`/`Deserialize`, so a
/// line written by the live recorder deserializes byte-for-byte on the lab side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LoggedEvent {
    TokenCreated {
        mint: Mint,
        fp: Box<TokenFingerprint>,
        at: Ts,
        /// Absent on pre-V0 JSONL lines ⇒ `None` (organic creator rule inactive).
        #[serde(default)]
        creator_wallet_hash: Option<u64>,
    },
    FirstSlotSettled { mint: Mint, buy_lamports: u64, sell_lamports: u64, at: Ts },
    Trade { mint: Mint, trade: TradeLite },
    FillConfirmed { intent: IntentId, fill: Fill },
    FillFailed { intent: IntentId, reason: FillFailReason },
    Migrated { mint: Mint, at: Ts },
    ManualBuy { mint: Mint, rule: RuleId, lamports: u64, at: Ts, exit: Option<ManualExit> },
    SetManualExit { position: PositionId, exit: Option<ManualExit> },
    ManualClose {
        position: PositionId,
        /// Absent on pre-portion JSONL lines ⇒ `All` (legacy Sell ALL).
        #[serde(default)]
        portion: Portion,
    },
    ExternallyCleared { position: PositionId, fill: Fill },
}

impl LoggedEvent {
    /// Project an engine event onto the loggable subset (`None` for `Tick` /
    /// `RulesReloaded`, which are never logged).
    pub fn from_event(event: &Event) -> Option<Self> {
        Some(match event {
            Event::TokenCreated { mint, fp, at, creator_wallet_hash } => {
                LoggedEvent::TokenCreated {
                    mint: mint.clone(),
                    fp: fp.clone(),
                    at: *at,
                    creator_wallet_hash: *creator_wallet_hash,
                }
            }
            Event::FirstSlotSettled { mint, buy_lamports, sell_lamports, at } => {
                LoggedEvent::FirstSlotSettled {
                    mint: mint.clone(),
                    buy_lamports: *buy_lamports,
                    sell_lamports: *sell_lamports,
                    at: *at,
                }
            }
            Event::Trade { mint, trade } => {
                LoggedEvent::Trade { mint: mint.clone(), trade: *trade }
            }
            Event::FillConfirmed { intent, fill } => {
                LoggedEvent::FillConfirmed { intent: intent.clone(), fill: *fill }
            }
            Event::FillFailed { intent, reason } => {
                LoggedEvent::FillFailed { intent: intent.clone(), reason: *reason }
            }
            Event::Migrated { mint, at } => LoggedEvent::Migrated { mint: mint.clone(), at: *at },
            Event::ManualBuy { mint, rule, lamports, at, exit } => LoggedEvent::ManualBuy {
                mint: mint.clone(),
                rule: *rule,
                lamports: *lamports,
                at: *at,
                exit: *exit,
            },
            Event::SetManualExit { position, exit } => {
                LoggedEvent::SetManualExit { position: *position, exit: *exit }
            }
            Event::ManualClose { position, portion } => {
                LoggedEvent::ManualClose { position: *position, portion: *portion }
            }
            Event::ExternallyCleared { position, fill } => {
                LoggedEvent::ExternallyCleared { position: *position, fill: *fill }
            }
            Event::Tick { .. } | Event::RulesReloaded { .. } => return None,
        })
    }

    /// The event's mint, when it has one (used to gate recovery / filter by token).
    pub fn mint(&self) -> Option<&str> {
        match self {
            LoggedEvent::TokenCreated { mint, .. }
            | LoggedEvent::FirstSlotSettled { mint, .. }
            | LoggedEvent::Trade { mint, .. }
            | LoggedEvent::ManualBuy { mint, .. }
            | LoggedEvent::Migrated { mint, .. } => Some(mint.as_str()),
            _ => None,
        }
    }

    /// The event's timestamp, when it carries one (used to bound recovery by age
    /// and to interleave synthetic ticks on replay).
    pub fn at(&self) -> Option<Ts> {
        match self {
            LoggedEvent::TokenCreated { at, .. }
            | LoggedEvent::FirstSlotSettled { at, .. }
            | LoggedEvent::ManualBuy { at, .. }
            | LoggedEvent::Migrated { at, .. } => Some(*at),
            LoggedEvent::Trade { trade, .. } => Some(trade.at),
            LoggedEvent::FillConfirmed { fill, .. }
            | LoggedEvent::ExternallyCleared { fill, .. } => Some(fill.at),
            _ => None,
        }
    }

    /// A pre-entry event (safe to replay for re-arming; fills/closes are not).
    pub fn is_pre_entry(&self) -> bool {
        matches!(
            self,
            LoggedEvent::TokenCreated { .. }
                | LoggedEvent::FirstSlotSettled { .. }
                | LoggedEvent::Trade { .. }
                | LoggedEvent::Migrated { .. }
        )
    }

    pub fn into_event(self) -> Event {
        match self {
            LoggedEvent::TokenCreated { mint, fp, at, creator_wallet_hash } => {
                Event::TokenCreated { mint, fp, at, creator_wallet_hash }
            }
            LoggedEvent::FirstSlotSettled { mint, buy_lamports, sell_lamports, at } => {
                Event::FirstSlotSettled { mint, buy_lamports, sell_lamports, at }
            }
            LoggedEvent::Trade { mint, trade } => Event::Trade { mint, trade },
            LoggedEvent::FillConfirmed { intent, fill } => Event::FillConfirmed { intent, fill },
            LoggedEvent::FillFailed { intent, reason } => Event::FillFailed { intent, reason },
            LoggedEvent::Migrated { mint, at } => Event::Migrated { mint, at },
            LoggedEvent::ManualBuy { mint, rule, lamports, at, exit } => {
                Event::ManualBuy { mint, rule, lamports, at, exit }
            }
            LoggedEvent::SetManualExit { position, exit } => {
                Event::SetManualExit { position, exit }
            }
            LoggedEvent::ManualClose { position, portion } => {
                Event::ManualClose { position, portion }
            }
            LoggedEvent::ExternallyCleared { position, fill } => {
                Event::ExternallyCleared { position, fill }
            }
        }
    }
}
