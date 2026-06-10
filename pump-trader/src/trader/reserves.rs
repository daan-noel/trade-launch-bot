// ============================================================
// Live reserve cache — WS-fed, read on the slippage / AMM-reserve hot path.
//
// The ingest pipeline pushes the post-trade reserve snapshot from every tracked
// token's trade in here; the trade path reads it (cache-first, with a freshness
// bound) instead of an on-chain RPC. A stale or missing entry transparently
// falls back to RPC, so the trader never trades on bad/old data.
//
// Entries are venue-tagged (curve vs AMM): a curve snapshot is never served to
// an AMM swap (their reserves are different accounts on different scales). This
// closes the migration-window gap where the last bonding-curve snapshot would
// otherwise be read for the first AMM sell.
//
// Units match the trader's on-chain readers exactly:
//   token          — raw token base units (curve: virtual_token; AMM: pool base)
//   quote_lamports — lamports          (curve: virtual_sol;   AMM: pool quote/WSOL)
// ============================================================

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
struct ReserveEntry {
    token: u128,
    quote_lamports: u128,
    is_amm: bool,
    at: Instant,
}

/// Concurrent, freshness-bounded cache of the latest reserve snapshot per mint.
/// Uses a plain `std::sync::Mutex`: every critical section is a single map
/// op with no `.await` held, so it never blocks the async runtime.
#[derive(Default)]
pub struct ReserveCache {
    inner: Mutex<HashMap<String, ReserveEntry>>,
}

impl ReserveCache {
    /// Record a post-trade reserve snapshot for `mint`. `token_reserves` is in
    /// raw token units; `sol_reserves` is in SOL (the ingest's unit) and is
    /// converted to lamports here. Zero / non-finite inputs are ignored so an
    /// "empty" snapshot can never shadow a real on-chain read.
    pub fn update(&self, mint: &str, token_reserves: f64, sol_reserves: f64, is_amm: bool) {
        // Reject zero, negative, NaN, and ±inf — a non-finite value would
        // saturate to u128::MAX on cast and poison the cache.
        if !token_reserves.is_finite()
            || token_reserves <= 0.0
            || !sol_reserves.is_finite()
            || sol_reserves <= 0.0
        {
            return;
        }
        let token = token_reserves as u128;
        let quote_lamports = (sol_reserves * 1_000_000_000.0) as u128;
        if token == 0 || quote_lamports == 0 {
            return;
        }
        if let Ok(mut guard) = self.inner.lock() {
            guard.insert(
                mint.to_string(),
                ReserveEntry {
                    token,
                    quote_lamports,
                    is_amm,
                    at: Instant::now(),
                },
            );
        }
    }

    /// Return `(token_reserves, quote_lamports)` when a fresh snapshot for `mint`
    /// of the requested venue exists. `want_amm` must match the cached venue, so
    /// a curve snapshot is never served to an AMM swap (or vice-versa). `None`
    /// (miss / stale / wrong-venue) signals the caller to read on-chain.
    pub fn get_fresh(&self, mint: &str, max_age: Duration, want_amm: bool) -> Option<(u128, u128)> {
        let guard = self.inner.lock().ok()?;
        let entry = guard.get(mint)?;
        if entry.is_amm == want_amm && entry.at.elapsed() <= max_age {
            Some((entry.token, entry.quote_lamports))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A window long enough that no test trips the freshness bound by accident.
    const BIG: Duration = Duration::from_secs(60);

    #[test]
    fn update_then_get_roundtrips_units() {
        let c = ReserveCache::default();
        // 1e9 raw tokens, 2.5 SOL → token stays raw, quote becomes lamports.
        c.update("mint", 1_000_000_000.0, 2.5, false);
        assert_eq!(
            c.get_fresh("mint", BIG, false),
            Some((1_000_000_000, 2_500_000_000))
        );
    }

    #[test]
    fn venue_tag_is_enforced() {
        let c = ReserveCache::default();

        c.update("curve", 100.0, 1.0, false);
        assert!(
            c.get_fresh("curve", BIG, true).is_none(),
            "a curve snapshot must never be served to an AMM read"
        );
        assert!(c.get_fresh("curve", BIG, false).is_some());

        c.update("amm", 100.0, 1.0, true);
        assert!(
            c.get_fresh("amm", BIG, false).is_none(),
            "an AMM snapshot must never be served to a curve read"
        );
        assert!(c.get_fresh("amm", BIG, true).is_some());
    }

    #[test]
    fn zero_negative_and_nonfinite_inputs_are_ignored() {
        let c = ReserveCache::default();
        c.update("zero_token", 0.0, 1.0, false);
        c.update("zero_sol", 1.0, 0.0, false);
        c.update("neg", 1.0, -1.0, false);
        c.update("nan", f64::NAN, 1.0, false);
        c.update("inf", f64::INFINITY, 1.0, false);
        for key in ["zero_token", "zero_sol", "neg", "nan", "inf"] {
            assert!(
                c.get_fresh(key, BIG, false).is_none(),
                "{key} should not have been cached"
            );
        }
    }

    #[test]
    fn stale_entry_falls_through_to_none() {
        let c = ReserveCache::default();
        c.update("m", 100.0, 1.0, false);
        std::thread::sleep(Duration::from_millis(10));
        assert!(
            c.get_fresh("m", Duration::from_millis(1), false).is_none(),
            "entry older than max_age must read as a miss"
        );
        // The same entry is still served under a generous window.
        assert!(c.get_fresh("m", BIG, false).is_some());
    }

    #[test]
    fn latest_snapshot_wins() {
        let c = ReserveCache::default();
        c.update("m", 100.0, 1.0, false);
        c.update("m", 200.0, 2.0, false);
        assert_eq!(c.get_fresh("m", BIG, false), Some((200, 2_000_000_000)));
    }

    #[test]
    fn missing_mint_is_none() {
        let c = ReserveCache::default();
        assert!(c.get_fresh("never-seen", BIG, false).is_none());
    }
}
