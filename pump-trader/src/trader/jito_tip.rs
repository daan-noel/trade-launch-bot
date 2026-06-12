// ============================================================
// Dynamic Jito tip — background-refreshed tip-floor cache.
//
// A static minimum tip silently loses Jito's auction when the tip floor rises
// during hot launches (the tx is "sent" but higher-tipping bundles get included
// instead, so it just doesn't land). This caches Jito's live landed-tip
// percentile feed — refreshed in the background like `BlockhashCache` — and the
// trade path reads the hot value to size each tip, clamped to
// [MIN_JITO_TIP_SOL, MAX_JITO_TIP_SOL] and falling back to the floor whenever the
// feed is cold or stale (startup, network blip, or a stalled refresher).
// ============================================================

use super::PumpFunTrader;
use crate::constants::{
    JITO_TIP_ESCALATION_TAIL_MULT, JITO_TIP_FLOOR_MAX_AGE_MS, JITO_TIP_FLOOR_URL,
    JITO_TIP_PERCENTILE, LAMPORTS_PER_SOL, MAX_JITO_TIP_SOL, MIN_JITO_TIP_SOL,
};
use anyhow::{Context, Result};
use serde::Deserialize;
use solana_sdk::instruction::Instruction;
use solana_sdk::{signature::Signer, system_instruction};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Jito's landed-tip percentiles (lamports). Stored whole — not just the one
/// configured point — so the per-retry escalation ladder can climb real auction
/// percentiles (p75 → p95 → p99) instead of re-sending a single tip that just
/// lost the auction.
#[derive(Clone, Copy, Debug, Default)]
pub struct TipFloor {
    pub p25: u64,
    pub p50: u64,
    pub p75: u64,
    pub p95: u64,
    pub p99: u64,
}

impl TipFloor {
    /// The configured base percentile (`JITO_TIP_PERCENTILE`) — the level-0 tip.
    fn base(&self) -> u64 {
        match JITO_TIP_PERCENTILE {
            25 => self.p25,
            50 => self.p50,
            95 => self.p95,
            99 => self.p99,
            _ => self.p75,
        }
    }
}

/// Latest tip-floor snapshot + when it was fetched. Plain `std::sync::Mutex`:
/// the critical section is a single read/write with no `.await` held (same
/// rationale as `BlockhashCache`).
#[derive(Default)]
pub struct JitoTipCache {
    inner: Mutex<Option<(TipFloor, Instant)>>,
}

impl JitoTipCache {
    /// Store a freshly-fetched tip-floor snapshot, stamped with the current time.
    pub fn store(&self, floor: TipFloor) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = Some((floor, Instant::now()));
        }
    }

    /// The cached snapshot if it was fetched within the max-age window.
    fn fresh(&self) -> Option<TipFloor> {
        self.inner.lock().ok().and_then(|guard| match *guard {
            Some((floor, at))
                if at.elapsed() <= Duration::from_millis(JITO_TIP_FLOOR_MAX_AGE_MS) =>
            {
                Some(floor)
            }
            _ => None,
        })
    }

    /// Tip for the first attempt (level 0) — the configured percentile, clamped
    /// to [floor, ceiling], or the floor when the feed is empty/stale. The buy
    /// path and the first sell attempt use this.
    pub fn tip_lamports(&self) -> u64 {
        self.tip_lamports_for_level(0)
    }

    /// Tip for retry `level` (0 = first attempt), in lamports. Successive sell
    /// attempts climb the auction so a tx that lost the last block bids up to win
    /// the next: level 0 = the configured percentile, 1 = p95, 2 = p99, and
    /// beyond p99 the tip scales by `JITO_TIP_ESCALATION_TAIL_MULT` per extra
    /// level. When the feed is cold/stale every percentile is unknown, so the
    /// climb starts from the floor and scales by the same factor. Always clamped
    /// to [MIN_JITO_TIP_SOL, MAX_JITO_TIP_SOL] — the ceiling is the hard
    /// per-trade cost guardrail, so escalation can never overspend. (A tx that
    /// never lands costs nothing, so bidding up only costs more once it wins.)
    pub fn tip_lamports_for_level(&self, level: u8) -> u64 {
        let floor = sol_to_lamports(MIN_JITO_TIP_SOL);
        let ceil = sol_to_lamports(MAX_JITO_TIP_SOL).max(floor);
        let raw = match self.fresh() {
            Some(tf) => match level {
                0 => tf.base(),
                1 => tf.p95.max(tf.base()),
                2 => tf.p99.max(tf.p95),
                n => scale(tf.p99, (n - 2) as i32),
            },
            None => scale(floor, level as i32),
        };
        raw.clamp(floor, ceil)
    }
}

/// `base × JITO_TIP_ESCALATION_TAIL_MULT^exp`, saturating to `u64::MAX`. `exp`
/// is clamped non-negative so a stray level can't shrink the tip.
fn scale(base: u64, exp: i32) -> u64 {
    let scaled = base as f64 * JITO_TIP_ESCALATION_TAIL_MULT.powi(exp.max(0));
    if scaled >= u64::MAX as f64 {
        u64::MAX
    } else {
        scaled as u64
    }
}

/// One row of Jito's tip-floor feed. Percentiles are landed-tip amounts in SOL.
#[derive(Deserialize)]
struct TipFloorRow {
    landed_tips_25th_percentile: f64,
    landed_tips_50th_percentile: f64,
    landed_tips_75th_percentile: f64,
    landed_tips_95th_percentile: f64,
    landed_tips_99th_percentile: f64,
}

/// Fetch Jito's tip-floor feed and store all percentiles (as lamports). Called
/// once to prime the cache, then on a background interval (see `init.rs`).
pub(super) async fn refresh_tip_floor(http: &reqwest::Client, cache: &JitoTipCache) -> Result<()> {
    let rows: Vec<TipFloorRow> = http
        .get(JITO_TIP_FLOOR_URL)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
        .context("parse Jito tip-floor response")?;
    let row = rows.first().context("empty Jito tip-floor response")?;
    cache.store(TipFloor {
        p25: sol_to_lamports(row.landed_tips_25th_percentile),
        p50: sol_to_lamports(row.landed_tips_50th_percentile),
        p75: sol_to_lamports(row.landed_tips_75th_percentile),
        p95: sol_to_lamports(row.landed_tips_95th_percentile),
        p99: sol_to_lamports(row.landed_tips_99th_percentile),
    });
    Ok(())
}

fn sol_to_lamports(sol: f64) -> u64 {
    (sol * LAMPORTS_PER_SOL as f64) as u64
}

impl PumpFunTrader {
    /// Build the per-trade Jito tip instruction, sized from the live tip-floor
    /// cache for retry `level` (0 = first attempt). The destination account is
    /// fixed per trader instance; only the amount varies with the auction and the
    /// escalation level. See [`JitoTipCache::tip_lamports_for_level`].
    pub(super) fn jito_tip_ix(&self, level: u8) -> Instruction {
        system_instruction::transfer(
            &self.config.keypair.pubkey(),
            &self.jito_tip_account,
            self.jito_tip_cache.tip_lamports_for_level(level),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lamports(sol: f64) -> u64 {
        (sol * LAMPORTS_PER_SOL as f64) as u64
    }

    fn sample_floor() -> TipFloor {
        // Percentiles between the floor (0.0002) and ceiling (0.005) so the
        // ladder rungs themselves aren't clamped.
        TipFloor {
            p25: lamports(0.0003),
            p50: lamports(0.0005),
            p75: lamports(0.0008),
            p95: lamports(0.0015),
            p99: lamports(0.0025),
        }
    }

    #[test]
    fn cold_cache_level_0_is_the_floor() {
        let c = JitoTipCache::default();
        assert_eq!(c.tip_lamports_for_level(0), lamports(MIN_JITO_TIP_SOL));
        // tip_lamports() is the level-0 alias used by the buy path.
        assert_eq!(c.tip_lamports(), c.tip_lamports_for_level(0));
    }

    #[test]
    fn cold_cache_escalates_then_saturates_at_ceiling() {
        let c = JitoTipCache::default();
        let ceil = lamports(MAX_JITO_TIP_SOL);
        assert!(
            c.tip_lamports_for_level(1) > c.tip_lamports_for_level(0),
            "a retry must bid above the first attempt even with a cold feed"
        );
        // A far-out level can never exceed the cost guardrail.
        assert_eq!(c.tip_lamports_for_level(20), ceil);
    }

    #[test]
    fn fresh_cache_climbs_the_percentile_ladder() {
        let c = JitoTipCache::default();
        let tf = sample_floor();
        c.store(tf);
        assert_eq!(c.tip_lamports_for_level(0), tf.base()); // configured p75
        assert_eq!(c.tip_lamports_for_level(1), tf.p95);
        assert_eq!(c.tip_lamports_for_level(2), tf.p99);
        assert!(
            c.tip_lamports_for_level(3) > tf.p99,
            "beyond p99 the tip keeps climbing"
        );
    }

    #[test]
    fn escalation_never_decreases_with_level() {
        let c = JitoTipCache::default();
        c.store(sample_floor());
        let mut prev = 0;
        for level in 0..10u8 {
            let tip = c.tip_lamports_for_level(level);
            assert!(tip >= prev, "tip dropped at level {level}: {tip} < {prev}");
            prev = tip;
        }
    }

    #[test]
    fn every_level_stays_within_the_clamp() {
        let c = JitoTipCache::default();
        c.store(sample_floor());
        let floor = lamports(MIN_JITO_TIP_SOL);
        let ceil = lamports(MAX_JITO_TIP_SOL);
        for level in 0..12u8 {
            let tip = c.tip_lamports_for_level(level);
            assert!((floor..=ceil).contains(&tip), "level {level} tip {tip} out of [{floor},{ceil}]");
        }
    }
}
