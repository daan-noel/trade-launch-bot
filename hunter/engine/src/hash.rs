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

/// A `Hasher` that passes a `u64` key straight through.
///
/// Every set below is keyed by a value this module already produced — an FNV-1a
/// digest — so re-hashing it is pure overhead. Only [`write_u64`](Self::write_u64)
/// is meaningful; anything else would be a misuse (the set is `u64`-keyed by
/// construction), so `write` folds bytes in rather than silently returning 0.
#[derive(Default, Clone, Copy)]
pub struct IdentityHasher(u64);

impl std::hash::Hasher for IdentityHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
    #[inline]
    fn write_u64(&mut self, n: u64) {
        self.0 = n;
    }
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        self.0 = fnv1a_bytes(self.0, bytes);
    }
}

/// Set of already-hashed `u64` keys (wallets, identities) — a flat hash set with no
/// second hash pass. Replaces a `BTreeSet<u64>`, whose per-lookup pointer chase is
/// paid once per trade per fingerprint on the flow-split classifier's hot path.
///
/// Iteration order is unspecified, so nothing that feeds a deterministic effect
/// stream may iterate one — membership only. (`PartialEq` on a hash set is
/// order-independent, so derived equality stays meaningful.)
pub type HashedSet = std::collections::HashSet<u64, std::hash::BuildHasherDefault<IdentityHasher>>;
