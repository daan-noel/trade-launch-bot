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

use crate::engine::Engine;
use crate::config::JitoTipCfg;
use crate::error::{Context, Result};
use crate::LAMPORTS_PER_SOL;
use serde::Deserialize;
use solana_sdk::instruction::Instruction;
use solana_sdk::system_instruction;
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
    /// The configured base percentile (`cfg.percentile`) — the level-0 tip.
    fn base(&self, cfg: &JitoTipCfg) -> u64 {
        match cfg.percentile {
            25 => self.p25,
            50 => self.p50,
            95 => self.p95,
            99 => self.p99,
            _ => self.p75,
        }
    }
}

/// Latest tip-floor snapshot + when it was fetched, plus the Jito tuning that
/// sizes each tip. Plain `std::sync::Mutex` on the snapshot: the critical
/// section is a single read/write with no `.await` held (same rationale as
/// `BlockhashCache`). The `cfg` is immutable after construction.
pub struct JitoTipCache {
    inner: Mutex<Option<(TipFloor, Instant)>>,
    /// Jito tuning (percentile / clamp bounds / floor feed) copied from
    /// `TraderConfig.jito` so the trade path sizes tips without touching config.
    pub cfg: JitoTipCfg,
}

impl JitoTipCache {
    /// Build an empty cache that sizes tips per `cfg`.
    pub fn new(cfg: JitoTipCfg) -> Self {
        Self {
            inner: Mutex::new(None),
            cfg,
        }
    }

    /// Store a freshly-fetched tip-floor snapshot, stamped with the current time.
    pub fn store(&self, floor: TipFloor) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = Some((floor, Instant::now()));
        }
    }

    /// The cached snapshot if it was fetched within the max-age window.
    fn fresh(&self) -> Option<TipFloor> {
        let max_age = Duration::from_millis(self.cfg.floor_max_age_ms);
        self.inner.lock().ok().and_then(|guard| match *guard {
            Some((floor, at)) if at.elapsed() <= max_age => Some(floor),
            _ => None,
        })
    }

    /// Tip for retry `level` (0 = first attempt), in lamports. Successive
    /// attempts climb so a tx that lost the last block bids up to win the next.
    ///
    /// Two ladders run in parallel; each level takes the **max**:
    /// 1. **Live percentile ladder** — level 0 = configured percentile, 1 = p95,
    ///    2 = p99, then `p99 × escalation_tail_mult^(n-2)` (or floor-scaled when
    ///    the feed is cold/stale).
    /// 2. **Floor escalation** — `min_sol × escalation_tail_mult^level`.
    ///
    /// (2) matters when live percentiles sit *below* `min_sol` (common: Helius
    /// Sender Max wants ≥ 0.001 SOL while Jito p75 is often ≪ that). Without it,
    /// every retry clamps to the same floor and the auction never climbs. Always
    /// clamped to `[min_sol, max_sol]` — the ceiling is the hard per-trade cost
    /// guardrail. A tx that never lands costs nothing, so bidding up only costs
    /// more once it wins.
    pub fn tip_lamports_for_level(&self, level: u8) -> u64 {
        let floor = sol_to_lamports(self.cfg.min_sol);
        let ceil = sol_to_lamports(self.cfg.max_sol).max(floor);
        let mult = self.cfg.escalation_tail_mult;
        let from_feed = match self.fresh() {
            Some(tf) => match level {
                0 => tf.base(&self.cfg),
                1 => tf.p95.max(tf.base(&self.cfg)),
                2 => tf.p99.max(tf.p95),
                n => scale(tf.p99, (n - 2) as i32, mult),
            },
            None => scale(floor, level as i32, mult),
        };
        // When the feed bid is below the Sender/cost floor, still escalate off
        // that floor so retries aren't identical no-ops.
        let from_floor = scale(floor, level as i32, mult);
        from_feed.max(from_floor).clamp(floor, ceil)
    }
}

/// `base × mult^exp`, saturating to `u64::MAX`. `exp` is clamped non-negative so
/// a stray level can't shrink the tip.
fn scale(base: u64, exp: i32, mult: f64) -> u64 {
    let scaled = base as f64 * mult.powi(exp.max(0));
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
pub async fn refresh_tip_floor(http: &reqwest::Client, cache: &JitoTipCache) -> Result<()> {
    let rows: Vec<TipFloorRow> = http
        .get(&cache.cfg.floor_url)
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
    // Guard the cast: a NaN/±inf or negative percentile from a malformed feed
    // row would otherwise saturate to 0 or u64::MAX. Returning 0 lets the
    // [MIN_JITO_TIP_SOL, MAX_JITO_TIP_SOL] clamp in `tip_lamports_for_level`
    // pull it back to the floor instead of seeding the cache with garbage.
    if !sol.is_finite() || sol <= 0.0 {
        return 0;
    }
    (sol * LAMPORTS_PER_SOL as f64) as u64
}

impl Engine {
    /// Build the per-trade tip instruction for a **single-tx** buy/sell, sized from
    /// the live tip-floor cache for retry `level` (0 = first attempt). These txs
    /// submit through the Helius `/fast` sender, so the tip goes to
    /// `sender_tip_account` (a Helius Sender wallet) — NOT `jito_tip_account`: the
    /// sender rejects a Jito-account tip with `-32602` and the tx never broadcasts.
    /// The account is fixed per trader instance; only the amount varies with the
    /// auction and the escalation level. See [`JitoTipCache::tip_lamports_for_level`].
    pub fn jito_tip_ix(&self, level: u8) -> Instruction {
        system_instruction::transfer(
            &self.config.signer.pubkey(),
            &self.sender_tip_account,
            self.jito_tip_cache.tip_lamports_for_level(level),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Default Jito tuning the tests size against.
    fn cfg() -> JitoTipCfg {
        JitoTipCfg::default()
    }
    fn cache() -> JitoTipCache {
        JitoTipCache::new(cfg())
    }
    const MIN_JITO_TIP_SOL: f64 = 0.001;
    const MAX_JITO_TIP_SOL: f64 = 0.005;

    fn lamports(sol: f64) -> u64 {
        (sol * LAMPORTS_PER_SOL as f64) as u64
    }

    fn sample_floor() -> TipFloor {
        // Percentiles between the floor (0.001) and ceiling (0.005) so the
        // ladder rungs themselves aren't clamped.
        TipFloor {
            p25: lamports(0.0012),
            p50: lamports(0.0015),
            p75: lamports(0.0020),
            p95: lamports(0.0030),
            p99: lamports(0.0040),
        }
    }

    /// Live percentiles all below `min_sol` — the common Sender-Max case.
    fn quiet_floor() -> TipFloor {
        TipFloor {
            p25: lamports(0.000001),
            p50: lamports(0.000002),
            p75: lamports(0.000004),
            p95: lamports(0.000011),
            p99: lamports(0.000043),
        }
    }

    #[test]
    fn cold_cache_level_0_is_the_floor() {
        let c = cache();
        assert_eq!(c.tip_lamports_for_level(0), lamports(MIN_JITO_TIP_SOL));
    }

    #[test]
    fn cold_cache_escalates_then_saturates_at_ceiling() {
        let c = cache();
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
        let c = cache();
        let tf = sample_floor();
        c.store(tf);
        assert_eq!(c.tip_lamports_for_level(0), tf.base(&cfg())); // configured p75
        assert_eq!(c.tip_lamports_for_level(1), tf.p95);
        assert_eq!(c.tip_lamports_for_level(2), tf.p99);
        assert!(
            c.tip_lamports_for_level(3) > tf.p99,
            "beyond p99 the tip keeps climbing"
        );
    }

    #[test]
    fn quiet_market_escalates_off_the_floor() {
        let c = cache();
        c.store(quiet_floor());
        let l0 = c.tip_lamports_for_level(0);
        let l1 = c.tip_lamports_for_level(1);
        let l2 = c.tip_lamports_for_level(2);
        assert_eq!(l0, lamports(MIN_JITO_TIP_SOL), "level 0 = Sender floor");
        assert!(l1 > l0, "retry must climb even when feed << floor");
        assert!(l2 > l1, "further retries keep climbing");
        assert_eq!(l1, scale(lamports(MIN_JITO_TIP_SOL), 1, cfg().escalation_tail_mult));
    }

    #[test]
    fn escalation_never_decreases_with_level() {
        let c = cache();
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
        let c = cache();
        c.store(sample_floor());
        let floor = lamports(MIN_JITO_TIP_SOL);
        let ceil = lamports(MAX_JITO_TIP_SOL);
        for level in 0..12u8 {
            let tip = c.tip_lamports_for_level(level);
            assert!((floor..=ceil).contains(&tip), "level {level} tip {tip} out of [{floor},{ceil}]");
        }
    }
}
