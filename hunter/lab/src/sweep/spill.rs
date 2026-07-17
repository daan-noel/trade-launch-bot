//! Disk spill for [`ComboMetrics`] batches — keeps peak RAM independent of total
//! combo count when pass-outer / sharded sweeps finalize more rows than fit in
//! the usable host budget.
//!
//! Format: repeated records of `u32 LE length` + JSON bytes (serde).

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use uuid::Uuid;

use crate::sweep::aggregate::ComboMetrics;

/// Append-only spill file for finalized combo metric rows.
pub struct MetricsSpill {
    path: PathBuf,
    file: Option<File>,
    rows: usize,
}

impl MetricsSpill {
    /// Create a unique spill file under the process temp dir.
    pub fn create() -> Result<Self> {
        let path = std::env::temp_dir().join(format!("hunter-sweep-spill-{}.bin", Uuid::new_v4()));
        let file = File::create(&path)
            .with_context(|| format!("create spill {}", path.display()))?;
        Ok(Self {
            path,
            file: Some(file),
            rows: 0,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Append one batch of finalized metrics.
    pub fn push_batch(&mut self, batch: &[ComboMetrics]) -> Result<()> {
        if batch.is_empty() {
            return Ok(());
        }
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| anyhow!("spill already persisted"))?;
        let bytes = serde_json::to_vec(batch).context("serialize spill batch")?;
        let len = bytes.len() as u32;
        file.write_all(&len.to_le_bytes())
            .context("write spill len")?;
        file.write_all(&bytes).context("write spill bytes")?;
        self.rows += batch.len();
        Ok(())
    }

    /// Flush + close, keep the file on disk (caller deletes via [`merge_spill_paths`]).
    pub fn persist(mut self) -> Result<PathBuf> {
        if let Some(mut f) = self.file.take() {
            f.flush().ok();
        }
        let path = std::mem::take(&mut self.path);
        Ok(path)
    }

    /// Flush, load every row, delete the file.
    pub fn finish_load(mut self) -> Result<Vec<ComboMetrics>> {
        if let Some(mut f) = self.file.take() {
            f.flush().ok();
        }
        let path = std::mem::take(&mut self.path);
        let out = load_spill_file(&path)?;
        let _ = fs::remove_file(&path);
        Ok(out)
    }
}

impl Drop for MetricsSpill {
    fn drop(&mut self) {
        if !self.path.as_os_str().is_empty() {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Load a spill file without deleting it.
pub fn load_spill_file(path: &Path) -> Result<Vec<ComboMetrics>> {
    let mut file = File::open(path).with_context(|| format!("open spill {}", path.display()))?;
    let mut out = Vec::new();
    let mut len_buf = [0u8; 4];
    loop {
        match file.read_exact(&mut len_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e).context("read spill len"),
        }
        let len = u32::from_le_bytes(len_buf) as usize;
        if len == 0 || len > 256 * 1024 * 1024 {
            return Err(anyhow!("corrupt spill record length {len}"));
        }
        let mut buf = vec![0u8; len];
        file.read_exact(&mut buf).context("read spill payload")?;
        let batch: Vec<ComboMetrics> =
            serde_json::from_slice(&buf).context("deserialize spill batch")?;
        out.extend(batch);
    }
    Ok(out)
}

/// Merge several spill files (in order) into one metrics vec; deletes each file.
pub fn merge_spill_paths(paths: Vec<PathBuf>) -> Result<Vec<ComboMetrics>> {
    let mut out = Vec::new();
    for path in paths {
        out.extend(load_spill_file(&path)?);
        let _ = fs::remove_file(&path);
    }
    Ok(out)
}

/// Write a complete metrics vec to a new spill file; returns its path.
pub fn write_metrics_spill(metrics: &[ComboMetrics]) -> Result<PathBuf> {
    let mut spill = MetricsSpill::create()?;
    spill.push_batch(metrics)?;
    spill.persist()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sweep::aggregate::ComboAgg;

    #[test]
    fn spill_round_trips_batches() {
        let mut spill = MetricsSpill::create().unwrap();
        let a: Vec<_> = (0..3)
            .map(|i| ComboAgg::default().finalize(i))
            .collect();
        let b: Vec<_> = (3..5)
            .map(|i| ComboAgg::default().finalize(i))
            .collect();
        spill.push_batch(&a).unwrap();
        spill.push_batch(&b).unwrap();
        let loaded = spill.finish_load().unwrap();
        assert_eq!(loaded.len(), 5);
        assert_eq!(loaded[0].combo_id, 0);
        assert_eq!(loaded[4].combo_id, 4);
    }

    #[test]
    fn write_and_merge_paths() {
        let p1 = write_metrics_spill(&[ComboAgg::default().finalize(0)]).unwrap();
        let p2 = write_metrics_spill(&[ComboAgg::default().finalize(1)]).unwrap();
        let merged = merge_spill_paths(vec![p1, p2]).unwrap();
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[1].combo_id, 1);
    }
}
