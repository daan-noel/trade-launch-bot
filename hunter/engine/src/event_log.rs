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

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::event::{
    Event, Fill, FillFailReason, IntentId, ManualExit, Mint, Portion, PositionId, RuleId,
};
use crate::grouping::TokenFingerprint;
use crate::metrics::{Ts, TradeLite};

/// Filename prefix shared by every log file.
pub const LOG_FILE_PREFIX: &str = "events-";
/// Filename suffix shared by every log file.
pub const LOG_FILE_SUFFIX: &str = ".jsonl";

/// A parsed log-file name. Lives here, next to the wire format, because THREE
/// separate places need this parse — the live recorder's `prune` and
/// `recent_log_files`, and the lab inspector's `read_logs`. A private copy in each
/// (plus one for the segmented name) is four spellings of one convention, so the
/// parse is single-sourced here.
///
/// Two shapes, and the ordering between them matters:
///
/// * `events-YYYY-MM-DD.jsonl`     — a day's **first** segment, `seq == 0`. This is
///   also every file written before segmentation existed, so old directories keep
///   parsing unchanged.
/// * `events-YYYY-MM-DD.NN.jsonl`  — later segments of that day, `seq >= 1`.
///
/// Field order is the sort order: `(date, seq)` ascending is chronological, and a
/// legacy whole-day file sorts before that same day's later segments — which is
/// correct, since it *is* the start of that day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LogFileName {
    pub date: NaiveDate,
    pub seq: u32,
}

/// Render a log file name. `seq == 0` yields the un-suffixed legacy form, so a day
/// that never overflows one segment keeps the exact name it always had.
pub fn log_file_name(date: NaiveDate, seq: u32) -> String {
    if seq == 0 {
        format!("{LOG_FILE_PREFIX}{date}{LOG_FILE_SUFFIX}")
    } else {
        format!("{LOG_FILE_PREFIX}{date}.{seq:02}{LOG_FILE_SUFFIX}")
    }
}

/// Parse a log file name, or `None` if it is not one (the directory may hold
/// unrelated files). `.00` is deliberately rejected: `seq 0` has exactly one
/// spelling, so a name can never round-trip to two different strings.
pub fn parse_log_file_name(name: &str) -> Option<LogFileName> {
    let stem = name.strip_prefix(LOG_FILE_PREFIX)?.strip_suffix(LOG_FILE_SUFFIX)?;
    match stem.split_once('.') {
        None => Some(LogFileName { date: parse_date(stem)?, seq: 0 }),
        Some((date, seq)) => {
            let parsed: u32 = seq.parse().ok()?;
            (parsed > 0).then_some(LogFileName { date: parse_date(date)?, seq: parsed })
        }
    }
}

fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

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
        /// Absent on pre-guard JSONL lines ⇒ `None` (no identity known, so the
        /// duplicate-identity guard neither blocks nor records on replay).
        #[serde(default)]
        identity: Option<crate::identity::IdentityHash>,
    },
    FirstSlotSettled { mint: Mint, buy_lamports: u64, sell_lamports: u64, at: Ts },
    Trade { mint: Mint, trade: TradeLite },
    FillConfirmed { intent: IntentId, fill: Fill },
    FillFailed {
        intent: IntentId,
        reason: FillFailReason,
        /// Absent on pre-`at` JSONL lines ⇒ `None` (an entry retry replays
        /// unqualified, as it originally ran).
        #[serde(default)]
        at: Option<Ts>,
    },
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
            Event::TokenCreated { mint, fp, at, creator_wallet_hash, identity } => {
                LoggedEvent::TokenCreated {
                    mint: mint.clone(),
                    fp: fp.clone(),
                    at: *at,
                    creator_wallet_hash: *creator_wallet_hash,
                    identity: *identity,
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
            Event::FillFailed { intent, reason, at } => {
                LoggedEvent::FillFailed { intent: intent.clone(), reason: *reason, at: *at }
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
            // Present only on lines written after `at` was added; `None` on older
            // ones, which is exactly what `Option` already means here.
            LoggedEvent::FillFailed { at, .. } => *at,
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
            LoggedEvent::TokenCreated { mint, fp, at, creator_wallet_hash, identity } => {
                Event::TokenCreated { mint, fp, at, creator_wallet_hash, identity }
            }
            LoggedEvent::FirstSlotSettled { mint, buy_lamports, sell_lamports, at } => {
                Event::FirstSlotSettled { mint, buy_lamports, sell_lamports, at }
            }
            LoggedEvent::Trade { mint, trade } => Event::Trade { mint, trade },
            LoggedEvent::FillConfirmed { intent, fill } => Event::FillConfirmed { intent, fill },
            LoggedEvent::FillFailed { intent, reason, at } => {
                Event::FillFailed { intent, reason, at }
            }
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

#[cfg(test)]
mod name_tests {
    use super::*;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").expect("date")
    }

    #[test]
    fn names_round_trip_in_both_shapes() {
        for (date, seq, expect) in [
            ("2026-08-09", 0, "events-2026-08-09.jsonl"),
            ("2026-08-09", 1, "events-2026-08-09.01.jsonl"),
            ("2026-08-09", 42, "events-2026-08-09.42.jsonl"),
            ("2026-08-09", 300, "events-2026-08-09.300.jsonl"),
        ] {
            let name = log_file_name(day(date), seq);
            assert_eq!(name, expect);
            let parsed = parse_log_file_name(&name).expect("parses");
            assert_eq!((parsed.date, parsed.seq), (day(date), seq));
        }
    }

    #[test]
    fn legacy_whole_day_files_still_parse_and_sort_first() {
        // Directories written before segmentation must keep working, and a legacy
        // file IS the start of its day, so it must precede that day's segments.
        let legacy = parse_log_file_name("events-2026-08-09.jsonl").expect("legacy parses");
        assert_eq!(legacy.seq, 0);

        let mut names = vec![
            parse_log_file_name("events-2026-08-10.jsonl").expect("n"),
            parse_log_file_name("events-2026-08-09.02.jsonl").expect("n"),
            parse_log_file_name("events-2026-08-09.jsonl").expect("n"),
            parse_log_file_name("events-2026-08-09.01.jsonl").expect("n"),
        ];
        names.sort();
        assert_eq!(
            names.iter().map(|n| (n.date.to_string(), n.seq)).collect::<Vec<_>>(),
            vec![
                ("2026-08-09".into(), 0),
                ("2026-08-09".into(), 1),
                ("2026-08-09".into(), 2),
                ("2026-08-10".into(), 0),
            ]
        );
    }

    #[test]
    fn non_log_names_are_rejected() {
        for name in [
            "not-a-log.txt",
            "events-2026-08-09",             // no suffix
            "events-2026-08-09.jsonl.bak",   // trailing junk
            "events-not-a-date.jsonl",
            "events-2026-08-09.xx.jsonl",    // non-numeric seq
            "events-2026-08-09.00.jsonl",    // seq 0 has ONE spelling
            "events-2026-08-09.1.2.jsonl",   // seq must be a single field
        ] {
            assert!(parse_log_file_name(name).is_none(), "{name} must not parse");
        }
    }
}
