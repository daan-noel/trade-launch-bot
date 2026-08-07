//! FNV-1a — the engine's one stable string hasher.
//!
//! Every hash the engine compares across process boundaries (flow-split wallet /
//! instruction hashes, [`crate::identity`]'s token identity) is built from these
//! primitives, so a value written by the live producer, the lake exporter, or a
//! replay driver is bit-identical by construction. Non-cryptographic and
//! deliberately stable: changing the algorithm invalidates every persisted hash.

/// FNV-1a offset basis (64-bit).
pub(crate) const FNV_OFFSET: u64 = 0xcbf29ce484222325;
/// FNV-1a prime (64-bit).
const FNV_PRIME: u64 = 0x100000001b3;

#[inline]
pub(crate) fn fnv1a_byte(mut h: u64, b: u8) -> u64 {
    h ^= u64::from(b);
    h.wrapping_mul(FNV_PRIME)
}

#[inline]
pub(crate) fn fnv1a_bytes(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h = fnv1a_byte(h, b);
    }
    h
}
