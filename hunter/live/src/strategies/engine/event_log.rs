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
//!
//! Writes run on a dedicated OS thread behind a bounded channel so the strategy
//! decision loop never blocks on disk I/O (serialize/flush).

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::thread;

use chrono::{NaiveDate, Utc};
use tracing::{info, warn};

use hunter_engine::event::Event;
use hunter_engine::event_log::LoggedEvent;
use hunter_engine::metrics::Ts;

/// Env: directory the event log is written to (created if missing). A **relative**
/// value is resolved against the loaded `.env`, not the CWD — see
/// [`trading_core::config::env_paths`]; the lab replay inspector resolves the same
/// key the same way, so writer and reader can't drift apart.
const ENV_DIR: &str = "EVENT_LOG_DIR";
/// Env: how many days of rotated logs to retain.
const ENV_RETENTION: &str = "EVENT_LOG_RETENTION_DAYS";
const DEFAULT_DIR: &str = "event_log";
const DEFAULT_RETENTION_DAYS: i64 = 7;
/// Bound how far the decision loop can get ahead of disk. Full = drop the line.
const WRITE_QUEUE_CAP: usize = 4096;
/// Slack scanned beyond the re-arm cutoff in [`recover_armed`]. Event timestamps
/// are *chain* times, so append order is only approximately time-ordered; the
/// margin keeps the reverse scan's early stop robust against that skew.
const RECOVERY_SCAN_MARGIN_SECS: i64 = 300;
/// Reverse-read granularity for [`read_log_tail`] — caps peak memory per file.
const REVERSE_CHUNK_BYTES: u64 = 1 << 20; // 1 MiB
/// Total on-disk budget for `events-*.jsonl`. Age-based retention alone cannot
/// bound this directory: daily volume swings by orders of magnitude (4.3 GB on
/// 2026-07-27 vs 0.87 GB two days later), so [`prune`] also evicts oldest-first
/// until the corpus fits. Recovery reads seconds of tail; the rest is for the lab
/// replay inspector, which tolerates a shorter history far better than the live
/// box tolerates a full disk.
const MAX_TOTAL_LOG_BYTES: u64 = 6 * 1024 * 1024 * 1024; // 6 GiB

/// Append-only recorder with daily rotation + retention pruning. The on-disk
/// format ([`LoggedEvent`]) is defined once in `hunter_engine::event_log` — the SSOT
/// shared with the lab replay inspector (plan 6.1).
///
/// `record` is lock-free/`try_send` only — never awaits disk.
pub struct EventLogRecorder {
    dir: PathBuf,
    tx: SyncSender<LoggedEvent>,
}

impl EventLogRecorder {
    /// Build a recorder from env (`EVENT_LOG_DIR`, `EVENT_LOG_RETENTION_DAYS`),
    /// creating the directory. Returns `None` (logging disabled) if the directory
    /// can't be created — recording is best-effort, never fatal to trading.
    pub fn from_env() -> Option<Self> {
        let dir = trading_core::config::dir_from_env(ENV_DIR, DEFAULT_DIR);
        let retention_days = std::env::var(ENV_RETENTION)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_RETENTION_DAYS);
        if let Err(e) = fs::create_dir_all(&dir) {
            warn!("event log disabled — cannot create {}: {e}", dir.display());
            return None;
        }
        let (tx, rx) = mpsc::sync_channel::<LoggedEvent>(WRITE_QUEUE_CAP);
        let writer_dir = dir.clone();
        thread::Builder::new()
            .name("event-log-writer".into())
            .spawn(move || writer_loop(writer_dir, retention_days, rx))
            .map_err(|e| {
                warn!("event log disabled — cannot spawn writer thread: {e}");
            })
            .ok()?;
        info!(dir = %dir.display(), retention_days, "event log recorder enabled");
        Some(Self { dir, tx })
    }

    /// The directory logs are written to (used by boot recovery).
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Append one event (a no-op for `Tick`/`RulesReloaded`). Best-effort: a full
    /// queue or serialize failure is logged, never propagated — a failed log line
    /// must not stop trading or stall `reduce()`.
    pub fn record(&self, event: &Event) {
        let Some(logged) = LoggedEvent::from_event(event) else {
            return;
        };
        match self.tx.try_send(logged) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                warn!("event log write queue full — dropping line");
            }
            Err(TrySendError::Disconnected(_)) => {
                warn!("event log writer exited — dropping line");
            }
        }
    }
}

fn writer_loop(dir: PathBuf, retention_days: i64, rx: mpsc::Receiver<LoggedEvent>) {
    let mut open: Option<(NaiveDate, BufWriter<File>)> = None;
    while let Ok(logged) = rx.recv() {
        let today = Utc::now().date_naive();
        if open.as_ref().map(|(d, _)| *d) != Some(today) {
            match rotate(&dir, retention_days, today) {
                Ok(w) => open = Some((today, w)),
                Err(e) => {
                    warn!("event log rotate failed: {e}");
                    continue;
                }
            }
        }
        if let Some((_, w)) = open.as_mut() {
            match serde_json::to_string(&logged) {
                Ok(line) => {
                    let _ = writeln!(w, "{line}");
                    // Flush per line so a crash loses at most the in-flight buffer;
                    // this runs off the decision loop so latency is acceptable.
                    let _ = w.flush();
                }
                Err(e) => warn!("event log serialize failed: {e}"),
            }
        }
    }
}

/// Open today's file (append) and prune files older than the retention window.
fn rotate(
    dir: &Path,
    retention_days: i64,
    today: NaiveDate,
) -> std::io::Result<BufWriter<File>> {
    let path = dir.join(format!("events-{today}.jsonl"));
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    prune(dir, retention_days, today);
    Ok(BufWriter::new(file))
}

/// Delete `events-*.jsonl` older than `retention_days`, then evict oldest-first
/// until the directory fits [`MAX_TOTAL_LOG_BYTES`].
///
/// The size cap is the operative bound: retention is expressed in days but the
/// cost is bytes, and daily volume is not stable enough for days to bound bytes.
/// `today`'s file is never evicted — it is the one being appended to.
fn prune(dir: &Path, retention_days: i64, today: NaiveDate) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut kept: Vec<(NaiveDate, PathBuf, u64)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(date) = name
            .strip_prefix("events-")
            .and_then(|s| s.strip_suffix(".jsonl"))
            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        else {
            continue;
        };
        if (today - date).num_days() > retention_days {
            let _ = fs::remove_file(entry.path());
            continue;
        }
        let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
        kept.push((date, entry.path(), bytes));
    }

    let mut total: u64 = kept.iter().map(|(_, _, b)| *b).sum();
    if total <= MAX_TOTAL_LOG_BYTES {
        return;
    }
    kept.sort_by_key(|(d, _, _)| *d); // oldest first
    for (date, path, bytes) in kept {
        if total <= MAX_TOTAL_LOG_BYTES || date == today {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(bytes);
            warn!(
                file = %path.display(),
                freed_bytes = bytes,
                "event log: size cap exceeded — evicted oldest log"
            );
        }
    }
}

/// Replay the recent log tail into recovery events that **re-arm** tokens which had
/// no open position at crash time. Returns pre-entry events (in file order),
/// bounded by `max_age_secs` and excluding any mint in `held_mints` or any mint
/// that reached a fill/close in the log (so a held token is never re-armed).
///
/// The scan is bounded at **both** ends before anything is parsed — the whole
/// corpus is never materialized. Boot recovery only ever needs the last
/// [`MAX_SNIPE_AGE_SECS`](trading_core::config::constants::MAX_SNIPE_AGE_SECS)
/// (30 s) of events, but this used to read every `events-*.jsonl` front-to-back
/// into one `Vec` — on 2026-07-30 that was ~8.2 GB of JSONL on a 4 GB box, so the
/// process ballooned to 2.4 GB, starved the runtime until the ingest watchdog
/// force-exited it at 90 s, and the loop below was never reached **once across 70
/// boots**: the engine sat deaf (every ping shed on a full queue) for 14 h with no
/// position ever entered. Keep both bounds:
///   * [`recent_log_files`] drops files whose date is wholly older than the window;
///   * [`read_log_tail`] reads each kept file **backwards** and stops at the first
///     event older than the window.
pub fn recover_armed(
    dir: &Path,
    max_age_secs: i64,
    now: Ts,
    held_mints: &HashSet<String>,
) -> Vec<Event> {
    let cutoff = now - chrono::Duration::seconds(max_age_secs);
    // Stop the scan a margin *before* the re-arm cutoff: `at()` is chain time, so
    // append order is only approximately time-ordered, and a fill that settles a
    // mint must be seen before that mint is considered for re-arming.
    let stop_before = cutoff - chrono::Duration::seconds(RECOVERY_SCAN_MARGIN_SECS);

    // Newest file first, stopping as soon as one is read back past `stop_before`;
    // per-file buffers are re-assembled oldest-first to preserve log order.
    let mut per_file: Vec<Vec<LoggedEvent>> = Vec::new();
    for path in recent_log_files(dir, stop_before).into_iter().rev() {
        let mut buf = Vec::new();
        let reached_cutoff = read_log_tail(&path, stop_before, &mut buf);
        per_file.push(buf);
        if reached_cutoff {
            break;
        }
    }
    per_file.reverse();
    let all: Vec<LoggedEvent> = per_file.into_iter().flatten().collect();

    // A mint that reached a fill/close in the log had (or attempted) a position;
    // never re-arm it from the log — PG + the reapers own its fate.
    let mut settled: HashSet<String> = held_mints.clone();
    for ev in &all {
        if let LoggedEvent::FillConfirmed { intent, .. } | LoggedEvent::FillFailed { intent, .. } = ev
        {
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

/// Log files that can contain events at or after `stop_before`, oldest first.
///
/// The date in the name bounds every event in the file, so a file dated before
/// `stop_before`'s day is skipped without being opened. Retention (7 days) is NOT
/// a usable bound here — at ~4 GB/day the corpus is fatal long before day 7.
fn recent_log_files(dir: &Path, stop_before: Ts) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let oldest_useful = stop_before.date_naive();
    let mut files: Vec<(NaiveDate, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name();
            let name = name.to_str()?;
            let date = name
                .strip_prefix("events-")
                .and_then(|s| s.strip_suffix(".jsonl"))
                .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())?;
            (date >= oldest_useful).then(|| (date, e.path()))
        })
        .collect();
    files.sort_by_key(|(d, _)| *d);
    files.into_iter().map(|(_, p)| p).collect()
}

/// Read `path` **backwards** in fixed chunks, appending the events at or after
/// `stop_before` to `out` in file order. Returns `true` once an event older than
/// `stop_before` is reached — the caller then has a complete window and can stop
/// opening older files.
///
/// Peak memory is one chunk plus the events actually kept, independent of file
/// size; the 4 GB/day files this guards against must never be materialized.
fn read_log_tail(path: &Path, stop_before: Ts, out: &mut Vec<LoggedEvent>) -> bool {
    read_log_tail_chunked(path, stop_before, REVERSE_CHUNK_BYTES, out)
}

/// [`read_log_tail`] with an explicit chunk size so tests can force the
/// line-straddles-a-chunk-boundary path cheaply.
fn read_log_tail_chunked(
    path: &Path,
    stop_before: Ts,
    chunk_bytes: u64,
    out: &mut Vec<LoggedEvent>,
) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let Ok(len) = file.metadata().map(|m| m.len()) else {
        return false;
    };

    let mut pos = len;
    // Bytes of the line straddling the front of the last chunk read — completed by
    // the chunk before it.
    let mut carry: Vec<u8> = Vec::new();
    let mut newest_first: Vec<LoggedEvent> = Vec::new();
    let mut reached_cutoff = false;

    while pos > 0 && !reached_cutoff {
        let take = chunk_bytes.max(1).min(pos);
        pos -= take;

        let mut buf = vec![0u8; take as usize];
        if file.seek(SeekFrom::Start(pos)).is_err() || file.read_exact(&mut buf).is_err() {
            break;
        }
        buf.append(&mut carry);

        let mut lines: Vec<&[u8]> = buf.split(|b| *b == b'\n').collect();
        // The first slice is a partial line unless this chunk reached the file start.
        if pos > 0 && !lines.is_empty() {
            carry = lines.remove(0).to_vec();
        }

        for line in lines.into_iter().rev() {
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            match serde_json::from_slice::<LoggedEvent>(line) {
                Ok(ev) => {
                    // Undated events (fill failures, manual-exit edits) carry no
                    // position in time — keep them, never let them stop the scan.
                    if ev.at().is_some_and(|t| t < stop_before) {
                        reached_cutoff = true;
                        break;
                    }
                    newest_first.push(ev);
                }
                Err(e) => {
                    warn!("event log: skipping unparseable line in {}: {e}", path.display())
                }
            }
        }
    }

    newest_first.reverse();
    out.extend(newest_first);
    reached_cutoff
}

#[cfg(test)]
mod tests {
    use super::*;
    use hunter_engine::event::Mint;

    /// A unique scratch dir per test (no `tempfile` dev-dep in this crate).
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hunter-event-log-test-{tag}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn at(secs: i64) -> Ts {
        chrono::DateTime::from_timestamp(1_800_000_000 + secs, 0).expect("ts")
    }

    /// Small, dated, pre-entry event — enough to exercise ordering + cutoff.
    fn migrated(n: i64) -> LoggedEvent {
        LoggedEvent::Migrated { mint: Mint::from(format!("mint{n}").as_str()), at: at(n) }
    }

    fn write_log(path: &Path, events: &[LoggedEvent]) {
        let mut body = String::new();
        for ev in events {
            body.push_str(&serde_json::to_string(ev).expect("serialize"));
            body.push('\n');
        }
        fs::write(path, body).expect("write log");
    }

    fn mints(events: &[LoggedEvent]) -> Vec<String> {
        events.iter().filter_map(|e| e.mint().map(str::to_string)).collect()
    }

    #[test]
    fn tail_read_keeps_file_order_and_stops_at_cutoff() {
        let dir = scratch("order");
        let path = dir.join("events-2026-07-30.jsonl");
        let events: Vec<LoggedEvent> = (0..10).map(migrated).collect();
        write_log(&path, &events);

        let mut out = Vec::new();
        // Reading backwards must still hand back ascending file order.
        let reached = read_log_tail_chunked(&path, at(7), 64, &mut out);

        assert!(reached, "an event older than the cutoff was present — scan should stop");
        assert_eq!(mints(&out), vec!["mint7", "mint8", "mint9"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tail_read_reassembles_lines_across_chunk_boundaries() {
        let dir = scratch("chunks");
        let path = dir.join("events-2026-07-30.jsonl");
        let events: Vec<LoggedEvent> = (0..40).map(migrated).collect();
        write_log(&path, &events);

        // A chunk far smaller than one line forces every line to straddle a
        // boundary — the `carry` path that a naive reverse reader corrupts.
        let mut out = Vec::new();
        let reached = read_log_tail_chunked(&path, at(0), 7, &mut out);

        assert!(!reached, "cutoff precedes the whole file — scan runs to the start");
        assert_eq!(out.len(), 40, "no line may be lost or split across chunks");
        assert_eq!(mints(&out), mints(&events));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tail_read_consumes_whole_file_when_nothing_is_older() {
        let dir = scratch("whole");
        let path = dir.join("events-2026-07-30.jsonl");
        let events: Vec<LoggedEvent> = (5..9).map(migrated).collect();
        write_log(&path, &events);

        let mut out = Vec::new();
        // `false` is what tells `recover_armed` it must also open the older file.
        let reached = read_log_tail_chunked(&path, at(0), 1024, &mut out);

        assert!(!reached);
        assert_eq!(mints(&out), vec!["mint5", "mint6", "mint7", "mint8"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recent_log_files_skips_days_wholly_outside_the_window() {
        let dir = scratch("select");
        for day in ["2026-07-27", "2026-07-28", "2026-07-29", "2026-07-30"] {
            fs::write(dir.join(format!("events-{day}.jsonl")), b"").expect("write");
        }
        fs::write(dir.join("not-a-log.txt"), b"").expect("write");

        let stop_before = chrono::DateTime::from_timestamp(0, 0)
            .expect("ts")
            .with_timezone(&chrono::Utc)
            + chrono::Duration::days(20_663); // 2026-07-29
        let picked = recent_log_files(&dir, stop_before);

        let names: Vec<String> =
            picked.iter().map(|p| p.file_name().unwrap().to_string_lossy().into()).collect();
        // The 4.3 GB 2026-07-27 file is the one that must never be opened.
        assert_eq!(names, vec!["events-2026-07-29.jsonl", "events-2026-07-30.jsonl"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_evicts_oldest_until_the_size_cap_is_met() {
        let dir = scratch("prune");
        let today = NaiveDate::parse_from_str("2026-07-30", "%Y-%m-%d").expect("date");
        // Three in-retention files that together blow the byte budget.
        let big = vec![b'x'; (MAX_TOTAL_LOG_BYTES / 2) as usize + 1];
        for day in ["2026-07-28", "2026-07-29", "2026-07-30"] {
            fs::write(dir.join(format!("events-{day}.jsonl")), &big).expect("write");
        }

        prune(&dir, 7, today);

        let left: HashSet<String> = fs::read_dir(&dir)
            .expect("read dir")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        // Age alone would have kept all three (all within 7 days) — that is exactly
        // the bound that failed in production; bytes must win.
        assert!(!left.contains("events-2026-07-28.jsonl"), "oldest must be evicted");
        assert!(left.contains("events-2026-07-30.jsonl"), "today's file is never evicted");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recover_armed_reads_only_the_recent_window() {
        let dir = scratch("recover");
        let now = at(1_000);
        // An out-of-window file whose contents would be unparseable if opened.
        fs::write(dir.join("events-2020-01-01.jsonl"), b"{not json}\n").expect("write");

        let day = now.date_naive();
        let path = dir.join(format!("events-{day}.jsonl"));
        write_log(&path, &[migrated(700), migrated(985), migrated(995)]);

        // 30 s re-arm window; the 700 event is far outside it.
        let out = recover_armed(&dir, 30, now, &HashSet::new());

        assert_eq!(out.len(), 2, "only events inside the re-arm window are replayed");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recover_armed_never_rearms_a_held_mint() {
        let dir = scratch("held");
        let now = at(1_000);
        let day = now.date_naive();
        write_log(&dir.join(format!("events-{day}.jsonl")), &[migrated(990), migrated(995)]);

        let held: HashSet<String> = ["mint990".to_string()].into_iter().collect();
        let out = recover_armed(&dir, 30, now, &held);

        assert_eq!(out.len(), 1, "a held mint must never be re-armed from the log");
        let _ = fs::remove_dir_all(&dir);
    }
}
