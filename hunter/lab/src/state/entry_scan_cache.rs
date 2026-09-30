//! Disk-backed Entry Context scan results: one file per scan request under
//! `<root>/entry-scan/`, so the page shows a scan again after a reload or a
//! `hunter-lab` restart without reading the market twice.
//!
//! A file is the request key on its first line, then the response JSON as the
//! handler sent it. The key line is compared on read, so two requests whose
//! file names collide read as a miss. The newest [`KEEP`] files stay.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};

use tokio::fs;

/// Scan files kept on disk.
const KEEP: usize = 8;

fn path_of(dir: &Path, key: &str) -> PathBuf {
    let mut h = DefaultHasher::new();
    key.hash(&mut h);
    dir.join(format!("{:016x}.json", h.finish()))
}

/// The response JSON stored for `key`, `None` when there is none.
pub async fn load(dir: &Path, key: &str) -> Option<Vec<u8>> {
    let mut bytes = fs::read(path_of(dir, key)).await.ok()?;
    let line = bytes.iter().position(|&b| b == b'\n')?;
    if &bytes[..line] != key.as_bytes() {
        return None;
    }
    Some(bytes.split_off(line + 1))
}

/// Store `json` as the result of `key`, replacing an earlier one, then drop the
/// files past [`KEEP`], oldest first. `key` holds no newline.
pub async fn store(dir: &Path, key: &str, json: &[u8]) -> io::Result<()> {
    fs::create_dir_all(dir).await?;
    let path = path_of(dir, key);
    let mut bytes = Vec::with_capacity(key.len() + 1 + json.len());
    bytes.extend_from_slice(key.as_bytes());
    bytes.push(b'\n');
    bytes.extend_from_slice(json);
    // Temp file + rename: a crash mid-write leaves no cut file.
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, &bytes).await?;
    fs::rename(&tmp, &path).await?;
    prune(dir).await
}

async fn prune(dir: &Path) -> io::Result<()> {
    let mut files = Vec::new();
    let mut entries = fs::read_dir(dir).await?;
    while let Some(e) = entries.next_entry().await? {
        if e.path().extension().is_some_and(|x| x == "json") {
            files.push((e.metadata().await?.modified()?, e.path()));
        }
    }
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    for (_, path) in files.into_iter().skip(KEEP) {
        fs::remove_file(path).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn a_stored_scan_reads_back_for_its_key_only() {
        let dir = std::env::temp_dir().join(format!("entry-scan-test-{}", Uuid::new_v4()));
        assert_eq!(load(&dir, "a").await, None);
        store(&dir, "a", b"{\"moments\":[]}").await.unwrap();
        assert_eq!(load(&dir, "a").await.as_deref(), Some(&b"{\"moments\":[]}"[..]));
        assert_eq!(load(&dir, "b").await, None);
        store(&dir, "a", b"{}").await.unwrap();
        assert_eq!(load(&dir, "a").await.as_deref(), Some(&b"{}"[..]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn the_file_count_stays_at_the_cap() {
        let dir = std::env::temp_dir().join(format!("entry-scan-test-{}", Uuid::new_v4()));
        for i in 0..KEEP + 3 {
            store(&dir, &format!("k{i}"), b"{}").await.unwrap();
        }
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), KEEP);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
