//! Disk-backed last-simulation result store.
//!
//! A backtest can run for minutes over uncapped data. The simulate endpoint returns
//! immediately; the detached run stores its terminal outcome here, and the client
//! collects it once the `simulation_finished` SSE fires.
//!
//! **Disk is the source of truth** for a saved rule's last successful run
//! (`$SWEEP_LAKE_DIR/sim-results/{rule_id}.{meta,rows}.json`). That survives lab
//! restarts. Retention is **not** time-based: an entry lives until the rule is
//! re-simulated, its trading config/`sim_key` changes, or the rule is deleted.
//!
//! **RAM stays thin:**
//! - an index of small [`SimMeta`] rollups (loaded at boot from `*.meta.json`)
//! - a single-slot working set of full per-token rows (loaded on demand for paging)
//! - an ephemeral map for draft / cancelled / failed outcomes (never written to disk)
//!
//! The Simulated table still pages/sorts/filters **in memory** over the working-set
//! rows (see `strategies::sim_query`); cold rules pay one disk read to hydrate.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use trading_core::strategies::kernel::{CostModelKind, RunSummary};
use trading_core::strategies::paper_fill::FillModel;
use uuid::Uuid;

use crate::lake;
use crate::strategies::sim_query;

/// Terminal outcome of a backtest, ready to serve from the result endpoints.
#[derive(Clone)]
pub enum SimOutcome {
    /// Success — the per-token results parsed to a JSON array (`Arc`-shared so a
    /// page read is a refcount bump, not a copy of a potentially large payload).
    Done(Arc<Vec<Value>>),
    /// User-requested cancel — served as `{"cancelled": true}` at HTTP 200.
    Cancelled,
    /// Failure — `status` is the HTTP code the result endpoint returns (404 rule
    /// not found, 400 invalid rule, 500 otherwise) and `message` the body error.
    Failed { status: u16, message: String },
}

/// Durable sidecar for one saved rule's last successful simulation.
///
/// Small enough to keep every rule's entry in the RAM index; the heavy per-token
/// rows live on disk (and at most one rule's worth in the working set).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SimMeta {
    pub rule_id: Uuid,
    pub sim_key: String,
    pub fingerprint_id: Uuid,
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub computed_at: DateTime<Utc>,
    /// Row count (fired positions + padded `NoEntry`). **Not** the matched-token
    /// count when the rule re-enters (`RuleParams.reentry`): a token can produce
    /// several fired rows (one per episode), so this over-counts by however many
    /// re-entries happened. See [`n_matched`](Self::n_matched) for the true
    /// distinct-token figure.
    pub n_rows: usize,
    /// Distinct-`mint_address` count over the run's rows — the true matched
    /// candidate-pool size, re-entry-safe (unlike `n_rows`; see
    /// [`crate::strategies::sim_query::count_matched`]). Surfaced on the wire as
    /// `n_matched` (see [`summary_wire`]), the Simulate table's Matched column.
    /// `None` on a legacy meta (pre-field), rather than a misleading `0` — the
    /// wire then reads as "not yet computed" until the rule is re-simulated.
    #[serde(default)]
    pub n_matched: Option<usize>,
    pub n_migrated: u64,
    /// Which fill model priced this run's round-trips — surfaced as the Simulate
    /// table's Fill column so a result reads with the pessimism band it was booked
    /// under. `#[serde(default)]` so legacy metas (pre-field) load as `WorstCase`.
    #[serde(default)]
    pub fill_model: FillModel,
    /// Which execution-cost model priced this run's round-trips — surfaced as the
    /// Simulate table's Cost column, mirroring `fill_model` above. `#[serde(default)]`
    /// so legacy metas (pre-field) load as `PumpfunDefault` — the model they were
    /// actually hardcoded to before this field existed, so their numbers keep meaning.
    #[serde(default)]
    pub cost_model: CostModelKind,
    /// Unfiltered rollup — powers the Simulate page's per-rule columns without
    /// loading rows from disk.
    pub summary: RunSummary,
}

/// Strategy-agnostic store of finished simulation outcomes, keyed by `rule_id`
/// (or a draft run id).
pub struct SimResults {
    dir: PathBuf,
    /// Durable metas scanned from disk + updated on each successful persist.
    index: DashMap<Uuid, SimMeta>,
    /// Draft / cancelled / failed — never written under `dir`.
    ephemeral: DashMap<Uuid, SimOutcome>,
    /// At most one durable rule's full row set resident for interactive paging.
    working: Mutex<Option<(Uuid, Arc<Vec<Value>>)>>,
}

impl Default for SimResults {
    fn default() -> Self {
        Self::open(lake::sim_results_dir(&lake::lake_root()))
    }
}

impl SimResults {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open (or create) the on-disk cache directory and load every `*.meta.json`
    /// into the RAM index. Row files stay on disk until first page/summary hit.
    pub fn open(dir: PathBuf) -> Self {
        if let Err(e) = fs::create_dir_all(&dir) {
            tracing::warn!(
                path = %dir.display(),
                error = %e,
                "sim-results: could not create cache dir"
            );
        }
        let index = DashMap::new();
        match fs::read_dir(&dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("json") {
                        continue;
                    }
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if !name.ends_with(".meta.json") {
                        continue;
                    }
                    match load_meta(&path) {
                        Ok(meta) => {
                            index.insert(meta.rule_id, meta);
                        }
                        Err(e) => {
                            tracing::warn!(
                                path = %path.display(),
                                error = %e,
                                "sim-results: skipping corrupt meta"
                            );
                        }
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::warn!(
                    path = %dir.display(),
                    error = %e,
                    "sim-results: could not scan cache dir"
                );
            }
        }
        let n = index.len();
        if n > 0 {
            tracing::info!(count = n, path = %dir.display(), "sim-results: loaded disk index");
        }
        Self {
            dir,
            index,
            ephemeral: DashMap::new(),
            working: Mutex::new(None),
        }
    }

    fn range_ts(from: Option<DateTime<Utc>>, to: Option<DateTime<Utc>>) -> (Option<i64>, Option<i64>) {
        (from.map(|t| t.timestamp()), to.map(|t| t.timestamp()))
    }

    fn meta_path(&self, rule_id: &Uuid) -> PathBuf {
        self.dir.join(format!("{rule_id}.meta.json"))
    }

    fn rows_path(&self, rule_id: &Uuid) -> PathBuf {
        self.dir.join(format!("{rule_id}.rows.json"))
    }

    /// Store a run's terminal outcome. Overwrites any prior outcome for the same id.
    ///
    /// When `persist` is true and `outcome` is [`SimOutcome::Done`], rows + meta are
    /// written under the lake's `sim-results/` dir (saved-rule runs). Drafts and
    /// non-success outcomes stay RAM-only.
    pub fn insert(
        &self,
        rule_id: Uuid,
        sim_key: impl Into<String>,
        fingerprint_id: Uuid,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
        fill_model: FillModel,
        cost_model: CostModelKind,
        outcome: SimOutcome,
        persist: bool,
    ) {
        let sim_key = sim_key.into();
        let (from_ts, to_ts) = Self::range_ts(from, to);

        // A fresh write supersedes anything previously held for this id.
        self.ephemeral.remove(&rule_id);
        if let Ok(mut w) = self.working.lock() {
            if w.as_ref().is_some_and(|(id, _)| *id == rule_id) {
                *w = None;
            }
        }

        match &outcome {
            SimOutcome::Done(rows) if persist => {
                let computed_at = Utc::now();
                let summary = sim_query::summarize(rows);
                let n_migrated = count_migrated(rows);
                let n_matched = sim_query::count_matched(rows);
                let meta = SimMeta {
                    rule_id,
                    sim_key,
                    fingerprint_id,
                    from: from_ts,
                    to: to_ts,
                    computed_at,
                    n_rows: rows.len(),
                    n_matched: Some(n_matched),
                    n_migrated,
                    fill_model,
                    cost_model,
                    summary,
                };
                if let Err(e) = self.write_durable(&meta, rows) {
                    tracing::error!(
                        rule_id = %rule_id,
                        error = %e,
                        "sim-results: disk write failed; keeping result in RAM only"
                    );
                    self.ephemeral.insert(rule_id, outcome);
                    return;
                }
                self.index.insert(rule_id, meta);
                if let Ok(mut w) = self.working.lock() {
                    *w = Some((rule_id, Arc::clone(rows)));
                }
            }
            SimOutcome::Done(_) | SimOutcome::Cancelled | SimOutcome::Failed { .. } => {
                // Drop any durable files so a re-sim that cancels/fails can't leave
                // a previous success looking current, and drafts never touch disk.
                self.delete_files(&rule_id);
                self.index.remove(&rule_id);
                self.ephemeral.insert(rule_id, outcome);
            }
        }
    }

    fn write_durable(&self, meta: &SimMeta, rows: &[Value]) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let rows_bytes = serde_json::to_vec(rows)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let meta_bytes = serde_json::to_vec_pretty(meta)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        write_atomic(&self.rows_path(&meta.rule_id), &rows_bytes)?;
        write_atomic(&self.meta_path(&meta.rule_id), &meta_bytes)?;
        Ok(())
    }

    fn delete_files(&self, rule_id: &Uuid) {
        for path in [self.meta_path(rule_id), self.rows_path(rule_id)] {
            if let Err(e) = fs::remove_file(&path) {
                if e.kind() != io::ErrorKind::NotFound {
                    tracing::warn!(path = %path.display(), error = %e, "sim-results: remove failed");
                }
            }
        }
    }

    /// Load rows for a durable entry into the working set (evicting any other rule).
    fn hydrate_rows(&self, rule_id: &Uuid) -> Option<Arc<Vec<Value>>> {
        if let Ok(w) = self.working.lock() {
            if let Some((id, rows)) = w.as_ref() {
                if id == rule_id {
                    return Some(Arc::clone(rows));
                }
            }
        }
        let path = self.rows_path(rule_id);
        let rows = match load_rows(&path) {
            Ok(r) => Arc::new(r),
            Err(e) => {
                tracing::warn!(
                    rule_id = %rule_id,
                    path = %path.display(),
                    error = %e,
                    "sim-results: row load failed; dropping index entry"
                );
                self.index.remove(rule_id);
                self.delete_files(rule_id);
                return None;
            }
        };
        if let Ok(mut w) = self.working.lock() {
            *w = Some((*rule_id, Arc::clone(&rows)));
        }
        Some(rows)
    }

    /// Borrow the stored outcome — the Simulated table's pager reads repeatedly.
    /// Durable `Done` results hydrate from disk into the working set on miss.
    pub fn peek(&self, rule_id: &Uuid) -> Option<SimOutcome> {
        if let Some(e) = self.ephemeral.get(rule_id) {
            return Some(e.clone());
        }
        if self.index.contains_key(rule_id) {
            return self.hydrate_rows(rule_id).map(SimOutcome::Done);
        }
        None
    }

    /// Wall-clock time the stored result was generated, or `None` if absent.
    pub fn computed_at(&self, rule_id: &Uuid) -> Option<DateTime<Utc>> {
        self.index.get(rule_id).map(|m| m.computed_at)
    }

    /// Fill model the stored result was priced under, or `None` if absent.
    pub fn fill_model(&self, rule_id: &Uuid) -> Option<FillModel> {
        self.index.get(rule_id).map(|m| m.fill_model)
    }

    /// Cost model the stored result was priced under, or `None` if absent.
    pub fn cost_model(&self, rule_id: &Uuid) -> Option<CostModelKind> {
        self.index.get(rule_id).map(|m| m.cost_model)
    }

    /// Durable meta for a rule, if any (no row load).
    pub fn meta(&self, rule_id: &Uuid) -> Option<SimMeta> {
        self.index.get(rule_id).map(|e| e.clone())
    }

    /// Unfiltered summary JSON extras for the Simulate table hydrate — meta only.
    pub fn cached_summary_json(&self, rule_id: &Uuid) -> Option<Value> {
        let meta = self.meta(rule_id)?;
        Some(summary_wire(&meta))
    }

    /// Borrow only when the stored entry matches the current rule config + window.
    pub fn peek_if(
        &self,
        rule_id: &Uuid,
        sim_key: &str,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
    ) -> Option<SimOutcome> {
        let (from_ts, to_ts) = Self::range_ts(from, to);
        if let Some(meta) = self.index.get(rule_id) {
            if meta.sim_key != sim_key || meta.from != from_ts || meta.to != to_ts {
                return None;
            }
            drop(meta);
            return self.hydrate_rows(rule_id).map(SimOutcome::Done);
        }
        match self.ephemeral.get(rule_id).map(|e| e.clone()) {
            Some(SimOutcome::Done(_)) => None, // drafts have no keyed short-circuit today
            other => other,
        }
    }

    /// Drop any stored outcome for a rule — re-sim start, rule delete, or stale
    /// `sim_key` after a param edit. Removes disk files when present.
    pub fn clear(&self, rule_id: &Uuid) {
        self.ephemeral.remove(rule_id);
        self.index.remove(rule_id);
        if let Ok(mut w) = self.working.lock() {
            if w.as_ref().is_some_and(|(id, _)| *id == *rule_id) {
                *w = None;
            }
        }
        self.delete_files(rule_id);
    }

    /// Clear the durable result when its stored `sim_key` no longer matches the
    /// rule's current config (param / fingerprint_id / caps / buy-size edit).
    pub fn invalidate_unless_key(&self, rule_id: &Uuid, current_sim_key: &str) {
        let stale = self
            .index
            .get(rule_id)
            .is_some_and(|m| m.sim_key != current_sim_key);
        if stale {
            self.clear(rule_id);
        }
    }

    /// Drop every durable result that used this fingerprint (content edit).
    pub fn clear_for_fingerprint(&self, fingerprint_id: Uuid) {
        let ids: Vec<Uuid> = self
            .index
            .iter()
            .filter(|e| e.fingerprint_id == fingerprint_id)
            .map(|e| *e.key())
            .collect();
        for id in ids {
            self.clear(&id);
        }
    }

    /// Take the stored outcome for the legacy whole-blob collector.
    ///
    /// Ephemeral entries are removed (single delivery). Durable disk results are
    /// **not** deleted — the Simulated table still needs them after a legacy fetch.
    pub fn take(&self, rule_id: &Uuid) -> Option<SimOutcome> {
        if let Some((_, outcome)) = self.ephemeral.remove(rule_id) {
            return Some(outcome);
        }
        self.peek(rule_id)
    }
}

/// Wire shape shared by the unfiltered batch hydrate and the filtered summary card.
pub fn summary_wire(meta: &SimMeta) -> Value {
    let mut body = serde_json::to_value(&meta.summary).unwrap_or_else(|_| serde_json::json!({}));
    if let Some(obj) = body.as_object_mut() {
        obj.insert("computed_at".into(), serde_json::json!(meta.computed_at));
        obj.insert("n_migrated".into(), serde_json::json!(meta.n_migrated));
        // Distinct-token count (re-entry safe — see `SimMeta::n_matched`'s doc),
        // unfiltered. The sibling `n_matched` `positions::summary_json_from_rows`
        // computes is the same statistic over a search/filter-scoped row slice,
        // for the drill-in summary card.
        obj.insert("n_matched".into(), serde_json::json!(meta.n_matched));
        obj.insert("fill_model".into(), serde_json::json!(meta.fill_model));
        obj.insert("cost_model".into(), serde_json::json!(meta.cost_model));
    }
    body
}

fn count_migrated(rows: &[Value]) -> u64 {
    rows.iter()
        .filter(|r| {
            let fired = r.get("fired").and_then(Value::as_bool).unwrap_or_else(|| {
                r.get("exit_reason").and_then(Value::as_str) != Some("NoEntry")
            });
            fired && r.get("is_migrated").and_then(Value::as_bool).unwrap_or(false)
        })
        .count() as u64
}

fn load_meta(path: &Path) -> io::Result<SimMeta> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn load_rows(path: &Path) -> io::Result<Vec<Value>> {
    let bytes = fs::read(path)?;
    let v: Value =
        serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    match v {
        Value::Array(rows) => Ok(rows),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "rows file is not a JSON array",
        )),
    }
}

/// Write `bytes` via a temp file + rename so a crash mid-write can't leave a
/// truncated durable file. On Windows, remove the destination first (rename
/// won't replace).
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durable_roundtrip_survives_reopen() {
        let dir = std::env::temp_dir().join(format!("sim-results-test-{}", Uuid::new_v4()));
        let _ = fs::remove_dir_all(&dir);
        let store = SimResults::open(dir.clone());
        let rule_id = Uuid::new_v4();
        let fp = Uuid::new_v4();
        let rows = Arc::new(vec![serde_json::json!({
            "fired": true,
            "exit_reason": "TakeProfit",
            "is_migrated": true,
            "pnl_sol": 1.0,
            "pnl_percent": 10.0,
        })]);
        store.insert(
            rule_id,
            "engine:test",
            fp,
            None,
            None,
            FillModel::WorstCase,
            CostModelKind::PumpfunDefault,
            SimOutcome::Done(rows.clone()),
            true,
        );
        assert!(store.meta(&rule_id).is_some());
        assert!(matches!(store.peek(&rule_id), Some(SimOutcome::Done(_))));

        // Drop RAM; reopen from disk.
        drop(store);
        let store2 = SimResults::open(dir.clone());
        let meta = store2.meta(&rule_id).expect("meta reloaded");
        assert_eq!(meta.sim_key, "engine:test");
        assert_eq!(meta.n_migrated, 1);
        match store2.peek(&rule_id) {
            Some(SimOutcome::Done(loaded)) => assert_eq!(loaded.len(), 1),
            Some(SimOutcome::Cancelled) => panic!("expected Done, got Cancelled"),
            Some(SimOutcome::Failed { message, .. }) => {
                panic!("expected Done, got Failed: {message}")
            }
            None => panic!("expected Done, got None"),
        }

        store2.clear(&rule_id);
        assert!(store2.meta(&rule_id).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn draft_not_written_to_disk() {
        let dir = std::env::temp_dir().join(format!("sim-results-draft-{}", Uuid::new_v4()));
        let _ = fs::remove_dir_all(&dir);
        let store = SimResults::open(dir.clone());
        let rule_id = Uuid::new_v4();
        store.insert(
            rule_id,
            "engine:draft",
            Uuid::new_v4(),
            None,
            None,
            FillModel::WorstCase,
            CostModelKind::PumpfunDefault,
            SimOutcome::Done(Arc::new(vec![serde_json::json!({"fired": false})])),
            false,
        );
        assert!(store.meta(&rule_id).is_none());
        assert!(!store.meta_path(&rule_id).exists());
        assert!(matches!(store.peek(&rule_id), Some(SimOutcome::Done(_))));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalidate_unless_key_clears_stale() {
        let dir = std::env::temp_dir().join(format!("sim-results-inv-{}", Uuid::new_v4()));
        let _ = fs::remove_dir_all(&dir);
        let store = SimResults::open(dir.clone());
        let rule_id = Uuid::new_v4();
        store.insert(
            rule_id,
            "key-a",
            Uuid::new_v4(),
            None,
            None,
            FillModel::WorstCase,
            CostModelKind::PumpfunDefault,
            SimOutcome::Done(Arc::new(vec![])),
            true,
        );
        store.invalidate_unless_key(&rule_id, "key-a");
        assert!(store.meta(&rule_id).is_some());
        store.invalidate_unless_key(&rule_id, "key-b");
        assert!(store.meta(&rule_id).is_none());
        let _ = fs::remove_dir_all(&dir);
    }
}
