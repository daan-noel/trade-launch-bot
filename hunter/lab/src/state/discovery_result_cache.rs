//! Disk-backed last-flow-discovery-result cache.
//!
//! Flow discovery folds the whole corpus (cross-token pattern scoring) and can
//! take a while; the result is authoring-UI-only but was RAM-only, so a
//! `hunter-lab` restart lost it and forced a re-run just to look at it again.
//! Single-slot twin of [`super::sim_results::SimResults`]: one
//! `<root>/flow-discovery/last.json` file holding `{run_id, result}`,
//! overwritten atomically on each successful run and loaded once at boot.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::lake;
use crate::strategies::flow_discovery::DiscoveryResult;

#[derive(Serialize, Deserialize)]
struct StoredDiscoveryResult {
    run_id: Uuid,
    result: DiscoveryResult,
}

pub struct DiscoveryResultCache {
    path: PathBuf,
    slot: RwLock<Option<(Uuid, DiscoveryResult)>>,
}

impl Default for DiscoveryResultCache {
    fn default() -> Self {
        Self::open(lake::discovery_result_path(&lake::lake_root()))
    }
}

impl DiscoveryResultCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open the on-disk slot at `path`, loading a prior result if present.
    pub fn open(path: PathBuf) -> Self {
        let slot = match load(&path) {
            Ok(loaded) => loaded,
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "flow-discovery: could not load cached result; starting empty"
                );
                None
            }
        };
        Self {
            path,
            slot: RwLock::new(slot),
        }
    }

    /// Borrow the stored result if its `run_id` matches.
    pub async fn get(&self, run_id: Uuid) -> Option<DiscoveryResult> {
        let guard = self.slot.read().await;
        guard
            .as_ref()
            .and_then(|(id, r)| (*id == run_id).then(|| r.clone()))
    }

    /// Borrow whatever is cached, regardless of `run_id` — page-reload rehydrate
    /// (the frontend has no run_id yet at that point).
    pub async fn get_last(&self) -> Option<(Uuid, DiscoveryResult)> {
        self.slot.read().await.clone()
    }

    /// Overwrite the slot with a fresh result, persisting it to disk. A disk
    /// write failure logs and keeps the result RAM-only for this process.
    pub async fn store(&self, run_id: Uuid, result: DiscoveryResult) {
        let stored = StoredDiscoveryResult {
            run_id,
            result: result.clone(),
        };
        if let Err(e) = write_atomic(&self.path, &stored) {
            tracing::warn!(
                path = %self.path.display(),
                error = %e,
                "flow-discovery: disk persist failed; result stays RAM-only"
            );
        }
        let mut guard = self.slot.write().await;
        *guard = Some((run_id, result));
    }
}

fn load(path: &Path) -> io::Result<Option<(Uuid, DiscoveryResult)>> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let stored: StoredDiscoveryResult = serde_json::from_slice(&bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(Some((stored.run_id, stored.result)))
}

/// Write `value` via a temp file + rename so a crash mid-write can't leave a
/// truncated durable file. On Windows, remove the destination first (rename
/// won't replace).
fn write_atomic(path: &Path, value: &StoredDiscoveryResult) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let bytes = serde_json::to_vec(value)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::flow_discovery::DiscoveryResult;

    #[tokio::test]
    async fn durable_roundtrip_survives_reopen() {
        let path = std::env::temp_dir().join(format!("discovery-result-test-{}.json", Uuid::new_v4()));
        let _ = fs::remove_file(&path);
        let cache = DiscoveryResultCache::open(path.clone());
        let run_id = Uuid::new_v4();
        // Exact precision + a label filter: the identity a rehydrating page must
        // read back off the RUN, since its form state is not the run's.
        let result = DiscoveryResult {
            groups: vec![],
            plan: Default::default(),
            ix_labels_filter: Some(vec!["Pump.Fun: Create_v2".into(), "Pump.Fun: Buy".into()]),
            fingerprint_id: None,
        };
        cache.store(run_id, result).await;
        assert!(cache.get(run_id).await.is_some());

        drop(cache);
        let cache2 = DiscoveryResultCache::open(path.clone());
        let reopened = cache2.get(run_id).await.expect("result survives reopen");
        // The whole point of caching the identity: exact must not come back as a
        // width, and the filter must not come back empty — either would rebuild a
        // different fingerprint than the run's groups were selected by.
        assert_eq!(reopened.plan, Default::default());
        assert_eq!(
            reopened.ix_labels_filter.as_deref(),
            Some(["Pump.Fun: Create_v2".to_string(), "Pump.Fun: Buy".to_string()].as_slice()),
        );
        assert!(cache2.get(Uuid::new_v4()).await.is_none());

        let _ = fs::remove_file(&path);
    }
}
