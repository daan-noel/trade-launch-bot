//! Event-log recorder + boot recovery (plan 4.6, decision 12).
//!
//! The live loop appends every *loggable* engine event to a cheap local
//! append-only JSONL file (rotated daily, retention-capped). `Tick` is **not**
//! logged — it's regenerable — and `RulesReloaded` is **not** logged (rules are
//! reloaded from PG on boot). Any live decision is therefore reproducible offline
//! by replaying the log ("time-travel debugging", Phase 6), and boot recovery
//! replays the recent tail to rebuild **armed** state.
//!
//! Recovery is deliberately conservative: it re-arms only tokens that had **no
//! open position** at crash time (so a held token can never be re-entered — PG
//! stays authoritative and the reapers resolve in-flight rows). It replays only
//! the pre-entry events (`TokenCreated`/`FirstSlotSettled`/`Trade`/`Migrated`) for
//! those mints; fills/manual-closes are never replayed, so no engine position is
//! ever resurrected from the log.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDate, Utc};
use tracing::{info, warn};

use hunter_engine::event::Event;
use hunter_engine::event_log::LoggedEvent;
use hunter_engine::metrics::Ts;

/// Env: directory the event log is written to (created if missing).
const ENV_DIR: &str = "EVENT_LOG_DIR";
/// Env: how many days of rotated logs to retain.
const ENV_RETENTION: &str = "EVENT_LOG_RETENTION_DAYS";
const DEFAULT_DIR: &str = "event_log";
const DEFAULT_RETENTION_DAYS: i64 = 7;

/// Append-only recorder with daily rotation + retention pruning. The on-disk
/// format ([`LoggedEvent`]) is defined once in `hunter_engine::event_log` — the SSOT
/// shared with the lab replay inspector (plan 6.1).
pub struct EventLogRecorder {
    dir: PathBuf,
    retention_days: i64,
    /// The currently-open file's date + writer (reopened on the first write of a
    /// new UTC day).
    open: Option<(NaiveDate, BufWriter<File>)>,
}

impl EventLogRecorder {
    /// Build a recorder from env (`EVENT_LOG_DIR`, `EVENT_LOG_RETENTION_DAYS`),
    /// creating the directory. Returns `None` (logging disabled) if the directory
    /// can't be created — recording is best-effort, never fatal to trading.
    pub fn from_env() -> Option<Self> {
        let dir = PathBuf::from(std::env::var(ENV_DIR).unwrap_or_else(|_| DEFAULT_DIR.to_string()));
        let retention_days = std::env::var(ENV_RETENTION)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_RETENTION_DAYS);
        if let Err(e) = fs::create_dir_all(&dir) {
            warn!("event log disabled — cannot create {}: {e}", dir.display());
            return None;
        }
        info!(dir = %dir.display(), retention_days, "event log recorder enabled");
        Some(Self { dir, retention_days, open: None })
    }

    /// The directory logs are written to (used by boot recovery).
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Append one event (a no-op for `Tick`/`RulesReloaded`). Best-effort: a write
    /// error is logged, never propagated — a failed log line must not stop trading.
    pub fn record(&mut self, event: &Event) {
        let Some(logged) = LoggedEvent::from_event(event) else { return };
        let today = Utc::now().date_naive();
        if self.open.as_ref().map(|(d, _)| *d) != Some(today) {
            if let Err(e) = self.rotate(today) {
                warn!("event log rotate failed: {e}");
                return;
            }
        }
        if let Some((_, w)) = self.open.as_mut() {
            match serde_json::to_string(&logged) {
                Ok(line) => {
                    let _ = writeln!(w, "{line}");
                    let _ = w.flush();
                }
                Err(e) => warn!("event log serialize failed: {e}"),
            }
        }
    }

    /// Open today's file (append) and prune files older than the retention window.
    fn rotate(&mut self, today: NaiveDate) -> std::io::Result<()> {
        let path = self.dir.join(format!("events-{today}.jsonl"));
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        self.open = Some((today, BufWriter::new(file)));
        self.prune(today);
        Ok(())
    }

    /// Delete `events-*.jsonl` older than `retention_days`.
    fn prune(&self, today: NaiveDate) {
        let Ok(entries) = fs::read_dir(&self.dir) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(date) = name
                .strip_prefix("events-")
                .and_then(|s| s.strip_suffix(".jsonl"))
                .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
            else {
                continue;
            };
            if (today - date).num_days() > self.retention_days {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

/// Replay the recent log tail into recovery events that **re-arm** tokens which had
/// no open position at crash time. Returns pre-entry events (in file order),
/// bounded by `max_age_secs` and excluding any mint in `held_mints` or any mint
/// that reached a fill/close in the log (so a held token is never re-armed).
pub fn recover_armed(
    dir: &Path,
    max_age_secs: i64,
    now: Ts,
    held_mints: &HashSet<String>,
) -> Vec<Event> {
    let cutoff = now - chrono::Duration::seconds(max_age_secs);
    let mut all: Vec<LoggedEvent> = Vec::new();
    for path in recent_log_files(dir) {
        read_log_file(&path, &mut all);
    }

    // A mint that reached a fill/close in the log had (or attempted) a position;
    // never re-arm it from the log — PG + the reapers own its fate.
    let mut settled: HashSet<String> = held_mints.clone();
    for ev in &all {
        if let LoggedEvent::FillConfirmed { intent, .. } | LoggedEvent::FillFailed { intent, .. } = ev {
            settled.insert(intent.mint.to_string());
        }
    }

    let mut out = Vec::new();
    for ev in all {
        if !ev.is_pre_entry() {
            continue;
        }
        if ev.at().is_some_and(|t| t < cutoff) {
            continue;
        }
        if ev.mint().is_some_and(|m| settled.contains(m)) {
            continue;
        }
        out.push(ev.into_event());
    }
    if !out.is_empty() {
        info!(events = out.len(), "event log: replaying recent tail to re-arm tokens");
    }
    out
}

/// The recent (retention-window) log files, oldest first, by their date-stamped name.
fn recent_log_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<(NaiveDate, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name();
            let name = name.to_str()?;
            let date = name
                .strip_prefix("events-")
                .and_then(|s| s.strip_suffix(".jsonl"))
                .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())?;
            Some((date, e.path()))
        })
        .collect();
    files.sort_by_key(|(d, _)| *d);
    files.into_iter().map(|(_, p)| p).collect()
}

fn read_log_file(path: &Path, out: &mut Vec<LoggedEvent>) {
    let Ok(file) = File::open(path) else { return };
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<LoggedEvent>(&line) {
            Ok(ev) => out.push(ev),
            Err(e) => warn!("event log: skipping unparseable line in {}: {e}", path.display()),
        }
    }
}

/// Parse a UTC timestamp helper (kept for symmetry / Phase 6 tooling).
#[allow(dead_code)]
fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc))
}
