//! The `name_reuse_count` tally - how many earlier tokens of one creation build
//! carried a token's `(name, symbol)` identity, over a trailing window.
//!
//! One structure for every path that stamps the axis: the engine keeps one in
//! [`EngineState`](crate::EngineState) and stamps it at `TokenCreated`; an offline
//! candidate scan builds one from the `tokens` table and stamps the same way. The
//! count lives here once, so the two cannot disagree about which launches were prior.
//!
//! Timestamped rather than counted: a replay folds one corpus, and a same-name launch
//! of the build outside it still happened - so a host primes every creation of the
//! build over `[start - window, end]`, and the count reads the window around each
//! token.

use std::collections::HashMap;

use crate::hash::{fnv1a_byte, fnv1a_bytes, FNV_OFFSET};
use crate::identity::IdentityHash;
use crate::metrics::Ts;

/// Trailing window the count reads: launches strictly before the token and at most
/// this many days before it.
pub const NAME_REUSE_WINDOW_DAYS: i64 = 30;

/// Creation instants and mints per (creation build, identity).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IdentityLaunches {
    by_key: HashMap<u64, Vec<(Ts, u64)>>,
}

impl IdentityLaunches {
    /// Record one creation. A mint already recorded is skipped, so priming and the
    /// fold never count one launch twice.
    pub fn record(&mut self, build: u64, identity: IdentityHash, at: Ts, mint: u64) {
        let list = self.by_key.entry(identity_key(build, identity)).or_default();
        if !list.iter().any(|&(_, m)| m == mint) {
            list.push((at, mint));
        }
    }

    /// Earlier creations of `build` with `identity`: strictly before `at`, at most
    /// [`NAME_REUSE_WINDOW_DAYS`] before it, the token itself excluded.
    pub fn prior(&self, build: u64, identity: IdentityHash, at: Ts, mint: u64) -> u32 {
        let from = at - chrono::Duration::days(NAME_REUSE_WINDOW_DAYS);
        let n = self
            .by_key
            .get(&identity_key(build, identity))
            .map_or(0, |l| l.iter().filter(|&&(t, m)| t < at && t >= from && m != mint).count());
        u32::try_from(n).unwrap_or(u32::MAX)
    }
}

/// The tally key: one hash over the creation build and the identity, unit-separated
/// so the two halves cannot run together.
pub fn identity_key(build: u64, identity: IdentityHash) -> u64 {
    let h = fnv1a_bytes(FNV_OFFSET, &build.to_le_bytes());
    let h = fnv1a_byte(h, 0x1f);
    fnv1a_bytes(h, &identity.to_le_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};

    fn at(days: f64) -> Ts {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap() + Duration::seconds((days * 86_400.0) as i64)
    }

    #[test]
    fn counts_the_trailing_window_strictly_before_and_never_itself() {
        let mut t = IdentityLaunches::default();
        t.record(1, 7, at(0.0), 100);
        t.record(1, 7, at(10.0), 101);
        t.record(1, 7, at(10.0), 101); // the same mint twice is one launch
        t.record(2, 7, at(10.0), 102); // another build
        t.record(1, 8, at(10.0), 103); // another identity
        assert_eq!(t.prior(1, 7, at(10.0), 101), 1, "the day-0 launch; itself excluded");
        assert_eq!(t.prior(1, 7, at(20.0), 104), 2);
        assert_eq!(t.prior(1, 7, at(35.0), 104), 1, "day 0 has left the 30-day window");
        assert_eq!(t.prior(1, 7, at(0.0), 100), 0, "strictly before");
    }
}
