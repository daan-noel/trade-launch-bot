//! Unknown-program harvest — an offline diagnostic over stored `raw_txs`.
//!
//! Feed it the verbatim protobuf payloads persisted in `raw_txs.payload` (each a
//! prost-encoded `SubscribeUpdateTransaction`, see [`crate::raw_tx`]) and it tallies
//! the top-level-instruction program IDs that the labeler still can't name — i.e.
//! the ones that render as `Unknown (...suffix)`. Ranking those by frequency tells
//! you exactly which programs to add to [`super::program_registry`] next, in the
//! order that most reduces `Unknown` labels on *your* traffic.
//!
//! DB-agnostic on purpose: the caller streams payloads from Postgres and pushes
//! them through [`UnknownProgramCounter::ingest_payload`]. Only top-level
//! instructions are counted (that is exactly what the labeler names); inner CPIs
//! are ignored.

use std::collections::HashMap;

use ingest_core::raw_tx::decode_payload;

use super::program_registry::program_friendly_name;

/// Accumulates unnamed top-level program IDs across many `raw_txs` payloads.
#[derive(Default)]
pub struct UnknownProgramCounter {
    counts: HashMap<String, u64>,
    txs_scanned: u64,
    txs_failed: u64,
}

impl UnknownProgramCounter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Decode one `raw_txs.payload` and tally its top-level program IDs that are
    /// absent from [`program_friendly_name`]. Malformed/empty payloads are counted
    /// in [`Self::txs_failed`] and skipped — never panics.
    pub fn ingest_payload(&mut self, payload: &[u8]) {
        let update = match decode_payload(payload) {
            Ok(u) => u,
            Err(_) => {
                self.txs_failed += 1;
                return;
            }
        };
        let Some(info) = update.transaction.as_ref() else { return };
        let Some(tx) = info.transaction.as_ref() else { return };
        let Some(message) = tx.message.as_ref() else { return };
        self.txs_scanned += 1;

        // Program-id indices can point past the static keys into ALT-loaded
        // addresses, so reconstruct the full list exactly as the live decoder does
        // (static keys ++ loaded writable ++ loaded readonly).
        let mut keys: Vec<&[u8]> = message.account_keys.iter().map(|k| k.as_slice()).collect();
        if let Some(meta) = info.meta.as_ref() {
            keys.extend(meta.loaded_writable_addresses.iter().map(|k| k.as_slice()));
            keys.extend(meta.loaded_readonly_addresses.iter().map(|k| k.as_slice()));
        }

        for ix in &message.instructions {
            let Some(pid) = keys.get(ix.program_id_index as usize).copied() else {
                continue;
            };
            let id = bs58::encode(pid).into_string();
            if program_friendly_name(&id).is_none() {
                *self.counts.entry(id).or_insert(0) += 1;
            }
        }
    }

    pub fn txs_scanned(&self) -> u64 {
        self.txs_scanned
    }

    pub fn txs_failed(&self) -> u64 {
        self.txs_failed
    }

    /// Distinct unnamed program IDs seen so far.
    pub fn distinct_programs(&self) -> usize {
        self.counts.len()
    }

    /// Unnamed program IDs with their top-level-instruction counts, most frequent
    /// first (ties broken by id for a stable ordering).
    pub fn into_ranked(self) -> Vec<(String, u64)> {
        let mut ranked: Vec<(String, u64)> = self.counts.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        ranked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn garbage_payload_is_counted_as_failed_not_panicking() {
        let mut c = UnknownProgramCounter::new();
        c.ingest_payload(&[0xff, 0x00, 0x13, 0x37]);
        assert_eq!(c.txs_scanned(), 0);
        assert_eq!(c.txs_failed(), 1);
        assert!(c.into_ranked().is_empty());
    }

    #[test]
    fn ranking_orders_by_count_desc() {
        let mut c = UnknownProgramCounter::new();
        c.counts.insert("aaa".into(), 2);
        c.counts.insert("bbb".into(), 9);
        c.counts.insert("ccc".into(), 9);
        let ranked = c.into_ranked();
        assert_eq!(ranked[0], ("bbb".to_string(), 9)); // higher count first
        assert_eq!(ranked[1], ("ccc".to_string(), 9)); // tie broken by id
        assert_eq!(ranked[2], ("aaa".to_string(), 2));
    }
}
