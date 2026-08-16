//! Disk-backed last-family-search-result cache. Twin of [`super::rule_search_cache`].
//!
//! Disk-backed for the same reason: a family run folds six cohorts, which takes a
//! while, and looking at the board again after a lab restart must not re-run it.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::family_search::report::Report;
use crate::lake;

#[derive(Serialize, Deserialize)]
struct Stored {
    run_id: Uuid,
    result: Report,
}

pub struct FamilySearchCache {
    path: PathBuf,
    slot: RwLock<Option<(Uuid, Report)>>,
}

impl Default for FamilySearchCache {
    fn default() -> Self {
        Self::open(lake::family_search_result_path(&lake::lake_root()))
    }
}

impl FamilySearchCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(path: PathBuf) -> Self {
        let slot = match load(&path) {
            Ok(loaded) => loaded,
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "family-search: could not load cached result; starting empty"
                );
                None
            }
        };
        Self { path, slot: RwLock::new(slot) }
    }

    pub async fn get(&self, run_id: Uuid) -> Option<Report> {
        let guard = self.slot.read().await;
        guard.as_ref().and_then(|(id, r)| (*id == run_id).then(|| r.clone()))
    }

    pub async fn get_last(&self) -> Option<(Uuid, Report)> {
        self.slot.read().await.clone()
    }

    pub async fn store(&self, run_id: Uuid, result: Report) {
        let stored = Stored { run_id, result: result.clone() };
        if let Err(e) = write_atomic(&self.path, &stored) {
            tracing::warn!(
                path = %self.path.display(),
                error = %e,
                "family-search: disk persist failed; result stays RAM-only"
            );
        }
        *self.slot.write().await = Some((run_id, result));
    }
}

fn load(path: &Path) -> io::Result<Option<(Uuid, Report)>> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let stored: Stored =
        serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(Some((stored.run_id, stored.result)))
}

fn write_atomic(path: &Path, value: &Stored) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let bytes =
        serde_json::to_vec(value).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
    }
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}
