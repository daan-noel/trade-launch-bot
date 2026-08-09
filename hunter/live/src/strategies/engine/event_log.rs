//! Event-log recorder + boot recovery (plan 4.6, decision 12).
//!
//! The live loop appends every *loggable* engine event to a cheap local
//! append-only JSONL file (rotated by day **and** by size, retention-capped).
//! `Tick` is **not** logged — it's regenerable — and `RulesReloaded` is **not**
//! logged (rules are reloaded from PG on boot). Any live decision is therefore
//! reproducible offline by replaying the log ("time-travel debugging"),
//! and boot recovery replays the recent tail to rebuild **armed** state.
//!
//! **Why rotation is size-driven, not just daily.** The byte budget below is the
//! operative bound — but a cap can only be enforced by deleting something, and the
//! file currently open for append can never be deleted. With one file per day, a
//! day that exceeds the budget on its own (4.3 GB/day measured against a 6 GiB cap)
//! left `prune` with nothing it was allowed to evict: it deleted every other file,
//! reached the open one, and stopped. The directory then grew without bound — 11 GB
//! observed on 2026-08-09. Segmenting the day means the open file is a bounded slice
//! of it, so everything behind it is evictable and the cap actually binds.
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
use hunter_engine::event_log::{log_file_name, parse_log_file_name, LogFileName, LoggedEvent};
use hunter_engine::metrics::Ts;

/// Env: directory the event log is written to (created if missing). A **relative**
/// value is resolved against the loaded `.env`, not the CWD — see
/// [`trading_core::config::env_paths`]; the lab replay inspector resolves the same
/// key the same way, so writer and reader can't drift apart.
const ENV_DIR: &str = "EVENT_LOG_DIR";
/// Env: how many days of rotated logs to retain. A **secondary** bound — see
/// [`Limits::max_total_bytes`], which is the one that actually holds.
const ENV_RETENTION: &str = "EVENT_LOG_RETENTION_DAYS";
/// Env: total on-disk budget for the whole directory, in bytes.
const ENV_MAX_BYTES: &str = "EVENT_LOG_MAX_BYTES";
/// Env: roll to a new segment once the open file passes this many bytes.
const ENV_SEGMENT_BYTES: &str = "EVENT_LOG_SEGMENT_BYTES";
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
/// Default total on-disk budget. Age-based retention alone cannot bound this
/// directory: daily volume swings by orders of magnitude (4.3 GB on 2026-07-27 vs
/// 0.87 GB two days later), so [`prune`] also evicts oldest-first until the corpus
/// fits. Recovery reads seconds of tail; the rest is for the lab replay inspector,
/// which tolerates a shorter history far better than the live box tolerates a full
/// disk — so the deployed box overrides this downward via [`ENV_MAX_BYTES`].
const DEFAULT_MAX_TOTAL_BYTES: u64 = 6 * 1024 * 1024 * 1024; // 6 GiB
/// Default segment size. Must stay comfortably larger than the recovery window
/// (`MAX_SNIPE_AGE_SECS` + [`RECOVERY_SCAN_MARGIN_SECS`] ≈ 5.5 min ≈ 15 MB at the
/// measured 50 KB/s) so a boot scan almost never has to open a second file, and
/// comfortably smaller than the total budget so there is always something evictable
/// behind the open segment.
const DEFAULT_SEGMENT_BYTES: u64 = 256 * 1024 * 1024; // 256 MiB
/// Floor for the segment size. Below the recovery window, rotation would push the
/// events boot recovery needs into files the byte cap is free to evict.
const MIN_SEGMENT_BYTES: u64 = 1024 * 1024; // 1 MiB

/// The three tunables that bound the directory, resolved once from env.
#[derive(Debug, Clone, Copy)]
struct Limits {
    retention_days: i64,
    max_total_bytes: u64,
    segment_bytes: u64,
}

impl Limits {
    fn from_env() -> Self {
        let retention_days = parse_env(ENV_RETENTION).unwrap_or(DEFAULT_RETENTION_DAYS);
        let max_total_bytes = parse_env(ENV_MAX_BYTES).unwrap_or(DEFAULT_MAX_TOTAL_BYTES);
        // A segment larger than the whole budget can never be evicted behind, which
        // is the exact deadlock this module is being fixed for — clamp instead.
        let segment_bytes = parse_env(ENV_SEGMENT_BYTES)
            .unwrap_or(DEFAULT_SEGMENT_BYTES)
            .clamp(MIN_SEGMENT_BYTES, max_total_bytes.max(MIN_SEGMENT_BYTES));
        Self { retention_days, max_total_bytes, segment_bytes }
    }
}

fn parse_env<T: std::str::FromStr>(key: &str) -> Option<T> {
    std::env::var(key).ok()?.trim().parse().ok()
}

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
        let limits = Limits::from_env();
        if let Err(e) = fs::create_dir_all(&dir) {
            warn!("event log disabled — cannot create {}: {e}", dir.display());
            return None;
        }
        let (tx, rx) = mpsc::sync_channel::<LoggedEvent>(WRITE_QUEUE_CAP);
        let writer_dir = dir.clone();
        thread::Builder::new()
            .name("event-log-writer".into())
            .spawn(move || writer_loop(writer_dir, limits, rx))
            .map_err(|e| {
                warn!("event log disabled — cannot spawn writer thread: {e}");
            })
            .ok()?;
        info!(
            dir = %dir.display(),
            retention_days = limits.retention_days,
            max_total_bytes = limits.max_total_bytes,
            segment_bytes = limits.segment_bytes,
            "event log recorder enabled"
        );
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

/// The file currently open for append. `written` tracks its size so the size check
/// costs no `stat` per line; it is seeded from the file's real length on open, so a
/// mid-day restart continues a partial segment instead of undercounting it.
struct Segment {
    date: NaiveDate,
    seq: u32,
    written: u64,
    writer: BufWriter<File>,
}

impl Segment {
    /// True when this segment is full, or belongs to a day that is over.
    fn should_roll(&self, today: NaiveDate, limits: &Limits) -> bool {
        self.date != today || self.written >= limits.segment_bytes
    }
}

fn writer_loop(dir: PathBuf, limits: Limits, rx: mpsc::Receiver<LoggedEvent>) {
    let mut open: Option<Segment> = None;
    while let Ok(logged) = rx.recv() {
        let today = Utc::now().date_naive();
        if open.as_ref().is_none_or(|s| s.should_roll(today, &limits)) {
            // Continue the day's newest segment on a fresh start; roll past it when
            // the current one is full.
            let seq = match &open {
                Some(s) if s.date == today => s.seq + 1,
                _ => next_seq_for(&dir, today),
            };
            match rotate(&dir, &limits, today, seq) {
                Ok(seg) => open = Some(seg),
                Err(e) => {
                    warn!("event log rotate failed: {e}");
                    continue;
                }
            }
        }
        if let Some(seg) = open.as_mut() {
            match serde_json::to_string(&logged) {
                Ok(line) => {
                    let _ = writeln!(seg.writer, "{line}");
                    // Flush per line so a crash loses at most the in-flight buffer;
                    // this runs off the decision loop so latency is acceptable.
                    let _ = seg.writer.flush();
                    seg.written += line.len() as u64 + 1; // +1 for the newline
                }
                Err(e) => warn!("event log serialize failed: {e}"),
            }
        }
    }
}

/// The segment a fresh writer should append to for `today`: the highest `seq`
/// already on disk for that day, or 0 when the day has no file yet. Resuming the
/// newest rather than starting a new one keeps a restart from littering the
/// directory with tiny segments; `should_roll` immediately rolls past it if it is
/// already full.
fn next_seq_for(dir: &Path, today: NaiveDate) -> u32 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter_map(|e| parse_log_file_name(e.file_name().to_str()?))
        .filter(|n| n.date == today)
        .map(|n| n.seq)
        .max()
        .unwrap_or(0)
}

/// Open the `(today, seq)` segment for append and prune the directory around it.
fn rotate(dir: &Path, limits: &Limits, today: NaiveDate, seq: u32) -> std::io::Result<Segment> {
    let path = dir.join(log_file_name(today, seq));
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    // Seed from the real length: this may be a partial segment resumed after a
    // restart, in which case counting from 0 would let it grow to 2x the cap.
    let written = file.metadata().map(|m| m.len()).unwrap_or(0);
    // Prune on every rotation, not just at the date change. Size-based rotation
    // gives this a natural cadence — no timer needed — and it is the only thing
    // between the directory and unbounded growth within a single day.
    prune(dir, limits, today, &path);
    Ok(Segment { date: today, seq, written, writer: BufWriter::new(file) })
}

/// Delete log files older than `retention_days`, then evict oldest-first until the
/// directory fits [`Limits::max_total_bytes`].
///
/// The size cap is the operative bound: retention is expressed in days but the cost
/// is bytes, and daily volume is not stable enough for days to bound bytes.
///
/// `open` — the segment currently held for append — is the ONE file that is never
/// evicted, and the guard is on the **path**, not on "is it today's". That
/// distinction is the whole bug: with one file per day, "today's file" and "the open
/// file" were the same thing, so a day that outgrew the budget by itself left the
/// loop with nothing it was permitted to delete. It evicted every other file,
/// reached the open one, broke, and the directory kept growing (11 GB against a
/// 6 GiB cap, 2026-08-09). Now only the live slice is protected, so the rest of
/// today is evictable like any other day and the cap holds continuously.
fn prune(dir: &Path, limits: &Limits, today: NaiveDate, open: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut kept: Vec<(LogFileName, PathBuf, u64)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(parsed) = name.to_str().and_then(parse_log_file_name) else {
            continue;
        };
        let path = entry.path();
        if path != open && (today - parsed.date).num_days() > limits.retention_days {
            let _ = fs::remove_file(&path);
            continue;
        }
        let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
        kept.push((parsed, path, bytes));
    }

    let mut total: u64 = kept.iter().map(|(_, _, b)| *b).sum();
    if total <= limits.max_total_bytes {
        return;
    }
    kept.sort_by_key(|(n, _, _)| *n); // oldest first: (date, seq)
    for (_, path, bytes) in kept {
        if total <= limits.max_total_bytes {
            break;
        }
        // Skip, don't break: the open segment may sort anywhere once a stale file
        // from a future date exists (clock skew, a restored backup), and stopping
        // there would strand everything behind it.
        if path == open {
            continue;
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
/// (30 s) of events; reading every `events-*.jsonl` front-to-back into one `Vec`
/// scales with the whole log dir, which on a 4 GB box is multiple GB of JSONL. That
/// starves the runtime, the ingest watchdog force-exits the process mid-recovery,
/// and the loop below is never reached at all — a kill loop, not a recovery. Keep
/// both bounds:
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

/// Log files that can contain events at or after `stop_before`, oldest first
/// (`(date, seq)` order — a day's segments read in the order they were written).
///
/// The date in the name bounds every event in the file, so a file dated before
/// `stop_before`'s day is skipped without being opened. Retention in days is NOT a
/// usable bound here — at ~4 GB/day the corpus is fatal long before day 7.
fn recent_log_files(dir: &Path, stop_before: Ts) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let oldest_useful = stop_before.date_naive();
    let mut files: Vec<(LogFileName, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name();
            let parsed = parse_log_file_name(name.to_str()?)?;
            (parsed.date >= oldest_useful).then(|| (parsed, e.path()))
        })
        .collect();
    files.sort_by_key(|(n, _)| *n);
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

    /// Small limits so the byte-cap tests write kilobytes, not gigabytes.
    fn limits(max_total_bytes: u64, segment_bytes: u64) -> Limits {
        Limits { retention_days: 7, max_total_bytes, segment_bytes }
    }

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").expect("date")
    }

    fn names_in(dir: &Path) -> HashSet<String> {
        fs::read_dir(dir)
            .expect("read dir")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn prune_evicts_oldest_until_the_size_cap_is_met() {
        let dir = scratch("prune");
        let today = day("2026-07-30");
        let lim = limits(6_000, 2_000);
        // Three in-retention files that together blow the byte budget.
        let big = vec![b'x'; 3_001];
        for d in ["2026-07-28", "2026-07-29", "2026-07-30"] {
            fs::write(dir.join(format!("events-{d}.jsonl")), &big).expect("write");
        }

        prune(&dir, &lim, today, &dir.join("events-2026-07-30.jsonl"));

        let left = names_in(&dir);
        // Age alone would have kept all three (all within 7 days) — that is exactly
        // the bound that failed in production; bytes must win.
        assert!(!left.contains("events-2026-07-28.jsonl"), "oldest must be evicted");
        assert!(left.contains("events-2026-07-30.jsonl"), "the open segment is never evicted");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_bounds_a_single_day_that_exceeds_the_cap_by_itself() {
        // THE production bug: one file per day meant "today's file" WAS "the open
        // file", so a day bigger than the budget left prune nothing it could delete.
        // It evicted every other file, hit today's, broke, and the directory grew to
        // 11 GB against a 6 GiB cap. Segments make the rest of today evictable.
        let dir = scratch("prune-single-day");
        let today = day("2026-08-09");
        let lim = limits(6_000, 2_000);
        let seg = vec![b'x'; 2_500];
        for name in [
            "events-2026-08-09.jsonl",    // seq 0 — first segment of the day
            "events-2026-08-09.01.jsonl",
            "events-2026-08-09.02.jsonl",
            "events-2026-08-09.03.jsonl", // the open one
        ] {
            fs::write(dir.join(name), &seg).expect("write");
        }
        let open = dir.join("events-2026-08-09.03.jsonl");

        prune(&dir, &lim, today, &open);

        let left = names_in(&dir);
        assert!(left.contains("events-2026-08-09.03.jsonl"), "open segment survives");
        assert!(!left.contains("events-2026-08-09.jsonl"), "oldest segment of today evicted");
        let total: u64 = fs::read_dir(&dir)
            .expect("read dir")
            .flatten()
            .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
            .sum();
        assert!(total <= lim.max_total_bytes, "cap must hold within one day, got {total}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_keeps_the_open_segment_even_when_it_alone_exceeds_the_cap() {
        // The cap cannot be met by deleting the file being appended to. Prune must
        // free everything else and stop — never delete the live segment, never loop.
        let dir = scratch("prune-open-too-big");
        let today = day("2026-08-09");
        let lim = limits(1_000, 500);
        fs::write(dir.join("events-2026-08-08.jsonl"), vec![b'x'; 900]).expect("write");
        let open = dir.join("events-2026-08-09.jsonl");
        fs::write(&open, vec![b'x'; 5_000]).expect("write");

        prune(&dir, &lim, today, &open);

        let left = names_in(&dir);
        assert_eq!(left.len(), 1, "everything evictable is gone");
        assert!(left.contains("events-2026-08-09.jsonl"), "the open segment stays");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_never_age_evicts_the_open_segment() {
        // A clock jump (or a resumed box) can make the open segment look older than
        // the retention window. Deleting it would leave the writer appending to an
        // unlinked inode — the log silently stops existing while writes "succeed".
        let dir = scratch("prune-age-open");
        let today = day("2026-08-09");
        let open = dir.join("events-2026-07-01.jsonl"); // 39 days "old"
        fs::write(&open, b"x").expect("write");

        prune(&dir, &limits(6_000, 2_000), today, &open);

        assert!(open.exists(), "the open segment must survive age-based pruning");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn next_seq_resumes_the_days_newest_segment() {
        let dir = scratch("next-seq");
        let today = day("2026-08-09");
        assert_eq!(next_seq_for(&dir, today), 0, "a day with no file starts at seq 0");

        fs::write(dir.join("events-2026-08-09.jsonl"), b"").expect("write");
        fs::write(dir.join("events-2026-08-09.01.jsonl"), b"").expect("write");
        fs::write(dir.join("events-2026-08-09.02.jsonl"), b"").expect("write");
        // A different day must not influence today's counter.
        fs::write(dir.join("events-2026-08-08.07.jsonl"), b"").expect("write");

        assert_eq!(next_seq_for(&dir, today), 2, "resume the newest, don't skip past it");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recover_armed_spans_segments_of_one_day() {
        // Segmentation must not cost recovery events: a 30 s window that straddles a
        // roll has to yield every event, in order, across both files.
        let dir = scratch("recover-segments");
        let now = at(1_000);
        let d = now.date_naive();
        write_log(&dir.join(log_file_name(d, 0)), &[migrated(975), migrated(980)]);
        write_log(&dir.join(log_file_name(d, 1)), &[migrated(985), migrated(990)]);
        write_log(&dir.join(log_file_name(d, 2)), &[migrated(995)]);

        let out = recover_armed(&dir, 30, now, &HashSet::new());

        assert_eq!(
            mints(&out.iter().filter_map(LoggedEvent::from_event).collect::<Vec<_>>()),
            vec!["mint975", "mint980", "mint985", "mint990", "mint995"],
            "every in-window event, in write order, across all three segments"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recent_log_files_orders_segments_within_a_day() {
        let dir = scratch("select-segments");
        for name in [
            "events-2026-07-30.02.jsonl",
            "events-2026-07-30.jsonl",
            "events-2026-07-30.01.jsonl",
            "events-2026-07-31.jsonl",
        ] {
            fs::write(dir.join(name), b"").expect("write");
        }

        let stop_before = chrono::DateTime::from_timestamp(0, 0)
            .expect("ts")
            .with_timezone(&chrono::Utc)
            + chrono::Duration::days(20_664); // 2026-07-30
        let picked = recent_log_files(&dir, stop_before);

        let names: Vec<String> =
            picked.iter().map(|p| p.file_name().unwrap().to_string_lossy().into()).collect();
        // Legacy seq-0 file first: it IS the start of that day.
        assert_eq!(
            names,
            vec![
                "events-2026-07-30.jsonl",
                "events-2026-07-30.01.jsonl",
                "events-2026-07-30.02.jsonl",
                "events-2026-07-31.jsonl",
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_segment_rolls_on_size_and_the_cap_then_holds() {
        // End-to-end over the real rotate/prune pair: writing far past the budget
        // must leave the directory inside it, which is the property that failed.
        let dir = scratch("roll");
        let today = day("2026-08-09");
        let lim = limits(4_000, 1_000);

        let mut seq = next_seq_for(&dir, today);
        let mut seg = rotate(&dir, &lim, today, seq).expect("open");
        for i in 0..400 {
            if seg.should_roll(today, &lim) {
                seq += 1;
                seg = rotate(&dir, &lim, today, seq).expect("roll");
            }
            let line = serde_json::to_string(&migrated(i)).expect("serialize");
            writeln!(seg.writer, "{line}").expect("write");
            seg.writer.flush().expect("flush");
            seg.written += line.len() as u64 + 1;
        }
        drop(seg);

        let total: u64 = fs::read_dir(&dir)
            .expect("read dir")
            .flatten()
            .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
            .sum();
        assert!(seq > 0, "the day must have rolled into segments");
        // One open segment may sit above the cap; everything behind it is evicted.
        assert!(
            total <= lim.max_total_bytes + lim.segment_bytes,
            "directory unbounded: {total} bytes over a {} cap",
            lim.max_total_bytes
        );
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
