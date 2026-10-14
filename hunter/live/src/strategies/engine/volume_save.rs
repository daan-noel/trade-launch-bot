//! Write a volume list the live label queued. Off the decision loop.
//!
//! The loop only sends `(fingerprint, ix hash)`. This task appends the label text,
//! when the ingest still has it, onto `tags.volume.match.ix_shape`, then asks the
//! loop to reload. A reload keeps the sticky set. A this-coin wallet is not written.
//! Wallets that come back are replaced by [`super::volume_wallets`], on the batch
//! pool, not on this print.

use std::collections::BTreeMap;

use tokio::sync::{broadcast, mpsc};
use tracing::warn;
use uuid::Uuid;

use trading_core::models::ingest::SseEvent;
use trading_core::state::ix_labels;
use trading_core::storage::repositories::fingerprint_repo::FingerprintRepo;

use super::EngineHandle;

/// Start the writer. The decision loop sends hashes; it does not wait.
pub fn spawn(
    mut rx: mpsc::Receiver<(Uuid, u64)>,
    repo: FingerprintRepo,
    engine: EngineHandle,
    sse_tx: broadcast::Sender<SseEvent>,
) {
    tokio::spawn(async move {
        while let Some(first) = rx.recv().await {
            let mut batch = vec![first];
            while let Ok(more) = rx.try_recv() {
                batch.push(more);
            }
            let mut by_fp: BTreeMap<Uuid, Vec<u64>> = BTreeMap::new();
            for (fp, ix) in batch {
                let row = by_fp.entry(fp).or_default();
                if !row.contains(&ix) {
                    row.push(ix);
                }
            }
            let mut changed = false;
            for (fp, hashes) in by_fp {
                match append_lists(&repo, fp, &hashes).await {
                    Ok(true) => changed = true,
                    Ok(false) => {}
                    Err(e) => warn!(fingerprint = %fp, "volume list write failed: {e:#}"),
                }
            }
            if changed {
                engine.schedule_reload(sse_tx.clone());
            }
        }
    });
}

async fn append_lists(repo: &FingerprintRepo, id: Uuid, hashes: &[u64]) -> anyhow::Result<bool> {
    let mut grew = false;
    for &hash in hashes {
        let Some(labels) = ix_labels::labels(hash) else {
            continue;
        };
        if repo.append_volume_shape(id, &labels).await? {
            grew = true;
        }
    }
    Ok(grew)
}

/// The in-memory form of [`FingerprintRepo::append_volume_shape`]. The writer is the
/// SQL statement; this is what the tests judge.
#[cfg(test)]
fn append_shape(tags: &mut serde_json::Value, labels: &[String]) -> bool {
    if labels.is_empty() {
        return false;
    }
    let Some(vol) = tags.get_mut("volume") else {
        return false;
    };
    let Some(obj) = vol.as_object_mut() else {
        return false;
    };
    let match_ = obj.entry("match").or_insert_with(|| serde_json::json!({}));
    let Some(m) = match_.as_object_mut() else {
        return false;
    };
    let shapes = m.entry("ix_shape").or_insert_with(|| serde_json::json!([]));
    let Some(arr) = shapes.as_array_mut() else {
        return false;
    };
    if arr.iter().any(|row| same_labels(row, labels)) {
        return false;
    }
    arr.push(serde_json::Value::Array(
        labels
            .iter()
            .cloned()
            .map(serde_json::Value::String)
            .collect(),
    ));
    true
}

#[cfg(test)]
fn same_labels(row: &serde_json::Value, labels: &[String]) -> bool {
    let Some(existing) = row_labels(row) else {
        return false;
    };
    existing.len() == labels.len() && existing.iter().zip(labels).all(|(a, b)| a == b)
}

#[cfg(test)]
fn row_labels(row: &serde_json::Value) -> Option<Vec<&str>> {
    let arr = match row {
        serde_json::Value::Array(a) => a,
        serde_json::Value::Object(o) => o.get("labels")?.as_array()?,
        _ => return None,
    };
    arr.iter().map(|v| v.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_new_list_is_appended_once() {
        let mut tags = json!({"volume": {"match": {"creator": true, "ix_shape": [["Pump.Fun: Buy"]]}, "sticky": true}});
        assert!(append_shape(
            &mut tags,
            &[
                "Axiom Trade: ix#05".into(),
                "System Program: Transfer".into()
            ]
        ));
        assert!(!append_shape(
            &mut tags,
            &[
                "Axiom Trade: ix#05".into(),
                "System Program: Transfer".into()
            ]
        ));
        let rows = tags["volume"]["match"]["ix_shape"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn a_pinned_row_counts_as_the_same_list() {
        let mut tags =
            json!({"volume": {"match": {"ix_shape": [{"labels": ["A", "B"], "cu_price": 1000}]}}});
        assert!(!append_shape(&mut tags, &["A".into(), "B".into()]));
    }

    #[test]
    fn dump_is_not_a_place_to_write() {
        let mut tags = json!({"dump": {"match": {"ix_shape": [["S"]]}}});
        assert!(!append_shape(&mut tags, &["A".into()]));
    }
}
