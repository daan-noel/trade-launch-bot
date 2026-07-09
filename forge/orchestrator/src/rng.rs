//! A tiny **deterministic** PRNG (SplitMix64) — the orchestrator disguises must be
//! reproducible, not cryptographically random: a persona is *sticky* (the same
//! wallet always draws the same persona) and a per-op seed makes jitter
//! reproducible for tests + replay while still differing op-to-op. We seed from a
//! stable FNV-1a hash of `(wallet, op_id)` rather than a wall clock (no
//! `SystemTime`/`rand` dep, and a plan re-run reproduces its disguise exactly).
//!
//! This is NOT for anything security-sensitive — key material never touches it.

/// SplitMix64 — a fast, well-distributed 64-bit generator from a single seed.
#[derive(Debug, Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    /// Seed the generator. Any `u64` is a valid seed.
    pub fn new(seed: u64) -> Self {
        SplitMix64 { state: seed }
    }

    /// Seed from a string (a wallet pubkey) mixed with a per-op salt, so a
    /// wallet's ops share its persona (derived from the pubkey alone) yet each op
    /// jitters independently.
    pub fn seeded(pubkey: &str, salt: u64) -> Self {
        SplitMix64::new(fnv1a(pubkey.as_bytes()) ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15))
    }

    /// Next raw 64-bit value.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A uniform `u64` in `[lo, hi]` (inclusive). `lo == hi` returns `lo`; a
    /// reversed range is treated as the single value `lo`.
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        let span = hi - lo + 1;
        lo + (self.next_u64() % span)
    }

    /// A uniform index in `[0, len)`. Returns `0` for an empty range (callers
    /// guard against empty slices before indexing).
    pub fn index(&mut self, len: usize) -> usize {
        if len <= 1 {
            return 0;
        }
        (self.next_u64() % len as u64) as usize
    }

    /// A biased coin: `true` with probability `pct/100`.
    pub fn chance(&mut self, pct: u8) -> bool {
        if pct == 0 {
            return false;
        }
        if pct >= 100 {
            return true;
        }
        self.range(1, 100) <= pct as u64
    }
}

/// FNV-1a — a small, stable, allocation-free string hash. Used for the sticky
/// persona assignment (a wallet's persona is `hash(pubkey) % n_personas`) and as
/// the seed base. Deterministic across runs/platforms (unlike `DefaultHasher`).
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_reproduces() {
        let mut a = SplitMix64::new(42);
        let mut b = SplitMix64::new(42);
        for _ in 0..8 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn seeded_is_sticky_per_wallet_but_varies_per_salt() {
        // Same wallet + same salt ⇒ identical stream (sticky/replayable).
        let mut a = SplitMix64::seeded("WalletAaa", 0);
        let mut b = SplitMix64::seeded("WalletAaa", 0);
        assert_eq!(a.next_u64(), b.next_u64());
        // Same wallet, different op salt ⇒ different stream (per-op jitter).
        let mut c = SplitMix64::seeded("WalletAaa", 1);
        assert_ne!(SplitMix64::seeded("WalletAaa", 0).next_u64(), c.next_u64());
    }

    #[test]
    fn range_is_inclusive_and_bounded() {
        let mut r = SplitMix64::new(7);
        for _ in 0..1000 {
            let v = r.range(10, 20);
            assert!((10..=20).contains(&v));
        }
        assert_eq!(r.range(5, 5), 5);
        assert_eq!(r.range(9, 3), 9); // reversed ⇒ lo
    }
}
