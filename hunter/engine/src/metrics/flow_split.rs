//! Volume/organic flow-split SSOT hashes (V0 of the volume-flow-split plan).
//!
//! Adapters hash once at the ingest/lake boundary; the pure fold only sees
//! `TradeLite::{ix_hash,wallet_hash}` and `TokenCreated::creator_wallet_hash`.
//! Classifier / `FlowState` land in V1 — this module is hash-only for now.
//!
//! FNV-1a 64-bit, stable across processes (no `DefaultHasher` / SipHash seed).

/// FNV-1a offset basis (64-bit).
const FNV_OFFSET: u64 = 0xcbf29ce484222325;
/// FNV-1a prime (64-bit).
const FNV_PRIME: u64 = 0x100000001b3;

#[inline]
fn fnv1a_byte(mut h: u64, b: u8) -> u64 {
    h ^= u64::from(b);
    h.wrapping_mul(FNV_PRIME)
}

#[inline]
fn fnv1a_bytes(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h = fnv1a_byte(h, b);
    }
    h
}

/// Stable hash of an ordered instruction-label sequence (exact-order match
/// semantics, same as the fingerprint matcher's `ix_labels`). Labels are
/// separated with a single `0x1f` unit-separator byte so `["ab","c"]` ≠ `["a","bc"]`.
///
/// Empty input still returns a defined hash; callers that mean "missing labels"
/// should set `TradeLite::ix_hash = None` instead of hashing an empty slice.
pub fn ix_hash(labels: &[impl AsRef<str>]) -> u64 {
    let mut h = FNV_OFFSET;
    let mut first = true;
    for lab in labels {
        if !first {
            h = fnv1a_byte(h, 0x1f);
        }
        first = false;
        h = fnv1a_bytes(h, lab.as_ref().as_bytes());
    }
    h
}

/// `Some(ix_hash(labels))` when `labels` is non-empty; `None` when missing/empty
/// (pre-0002 history, absent lake columns) ⇒ organic unless wallet-tagged/creator.
pub fn ix_hash_opt(labels: &[impl AsRef<str>]) -> Option<u64> {
    if labels.is_empty() {
        None
    } else {
        Some(ix_hash(labels))
    }
}

/// Stable hash of a wallet address string (base58 or the lake's `unknown:{id}`
/// fallback). Contagion and creator checks compare these hashes only.
pub fn wallet_hash(addr: &str) -> u64 {
    fnv1a_bytes(FNV_OFFSET, addr.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ix_hash_is_order_and_boundary_sensitive() {
        let a = ix_hash(&["create", "buy"]);
        let b = ix_hash(&["buy", "create"]);
        let c = ix_hash(&["createbuy"]);
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(a, ix_hash(&["create", "buy"]));
    }

    #[test]
    fn ix_hash_opt_none_on_empty() {
        let empty: &[&str] = &[];
        assert_eq!(ix_hash_opt(empty), None);
        assert_eq!(ix_hash_opt(&["buy"]), Some(ix_hash(&["buy"])));
    }

    #[test]
    fn wallet_hash_stable() {
        assert_eq!(wallet_hash("Abc123"), wallet_hash("Abc123"));
        assert_ne!(wallet_hash("Abc123"), wallet_hash("abc123"));
        assert_ne!(wallet_hash("unknown:7"), wallet_hash("7"));
    }
}
