//! Shared tip / priority-fee knobs for **live execution** and **lab CostModel**.
//!
//! Live clamps each send to `[jito_min_tip_sol, jito_max_tip_sol]` (plus the
//! percentile feed + retry ladder). Lab has no tip feed, so simulated round-trips
//! price the **floor** (`jito_min_tip_sol`) + the CU priority fee as the
//! representative per-leg fixed cost — same env keys, same defaults as
//! `hunter/.env.example`. Install once at boot via [`FeeTuning::install`] so
//! [`crate::strategies::kernel::CostModel`] and the trader stay on one source.

use std::sync::OnceLock;

use crate::config::constants::{
    COMPUTE_UNIT_LIMIT_CURVE_BUY, COMPUTE_UNIT_LIMIT_CURVE_SELL, LAMPORTS_PER_SOL,
};

/// Process-wide fee knobs installed after `dotenvy` by both bins. Absent ⇒
/// [`FeeTuning::defaults`] so unit tests stay deterministic without reading the
/// developer's shell env.
static INSTALLED: OnceLock<FeeTuning> = OnceLock::new();

/// Tip floor / ceiling, landed-tip percentile, and CU price — the knobs live
/// applies onto `TraderConfig` and lab folds into `CostModel::fixed_cost_sol_per_leg`.
#[derive(Debug, Clone, PartialEq)]
pub struct FeeTuning {
    /// Helius Sender tip floor (SOL). Also the representative tip CostModel charges
    /// per leg (attempt-0 / typical fill — not the retry max).
    pub jito_min_tip_sol: f64,
    /// Hard per-trade tip ceiling (SOL). Live-only clamp; unused by CostModel.
    pub jito_max_tip_sol: f64,
    /// Landed-tip percentile for level-0 (25|50|75|95|99). Live-only.
    pub jito_tip_percentile: u8,
    /// Compute-unit price (micro-lamports) — the priority-fee rate.
    pub cu_price_micro_lamports: u64,
}

impl FeeTuning {
    /// Defaults matching `hunter/.env.example` (SWQoS-only tip band, not Sender Max).
    pub fn defaults() -> Self {
        Self {
            jito_min_tip_sol: 0.0001,
            jito_max_tip_sol: 0.0005,
            jito_tip_percentile: 50,
            cu_price_micro_lamports: 200_000,
        }
    }

    /// Load from the environment. Missing keys use [`Self::defaults`].
    pub fn from_env() -> anyhow::Result<Self> {
        let d = Self::defaults();
        let jito_min_tip_sol = env_f64("JITO_MIN_TIP_SOL", d.jito_min_tip_sol)?;
        let jito_max_tip_sol = env_f64("JITO_MAX_TIP_SOL", d.jito_max_tip_sol)?;
        if jito_min_tip_sol < 0.0 || jito_max_tip_sol < 0.0 {
            anyhow::bail!("JITO_MIN_TIP_SOL / JITO_MAX_TIP_SOL must be >= 0");
        }
        if jito_max_tip_sol < jito_min_tip_sol {
            anyhow::bail!(
                "JITO_MAX_TIP_SOL ({jito_max_tip_sol}) must be >= JITO_MIN_TIP_SOL ({jito_min_tip_sol})"
            );
        }
        let jito_tip_percentile = env_u64("JITO_TIP_PERCENTILE", d.jito_tip_percentile as u64)? as u8;
        if !matches!(jito_tip_percentile, 25 | 50 | 75 | 95 | 99) {
            anyhow::bail!(
                "JITO_TIP_PERCENTILE must be one of 25|50|75|95|99, got {jito_tip_percentile}"
            );
        }
        let cu_price_micro_lamports =
            env_u64("CU_PRICE_MICRO_LAMPORTS", d.cu_price_micro_lamports)?;
        if cu_price_micro_lamports == 0 {
            anyhow::bail!(
                "CU_PRICE_MICRO_LAMPORTS must be > 0 (Helius Sender requires a priority fee)"
            );
        }
        Ok(Self {
            jito_min_tip_sol,
            jito_max_tip_sol,
            jito_tip_percentile,
            cu_price_micro_lamports,
        })
    }

    /// Install process-wide tuning for [`Self::current`] / CostModel. Call once
    /// after `dotenvy` in each bin's `main`. A second call is ignored (first wins)
    /// so probes / re-entrant boot paths don't panic.
    pub fn install(self) {
        let _ = INSTALLED.set(self);
    }

    /// Process tuning: installed value, else [`Self::defaults`].
    pub fn current() -> Self {
        INSTALLED.get().cloned().unwrap_or_else(Self::defaults)
    }

    /// Representative fixed cost (SOL) charged **per leg**: tip floor + average
    /// curve buy/sell priority fee at this CU price. CostModel subtracts this twice
    /// per round-trip.
    pub fn fixed_cost_sol_per_leg(&self) -> f64 {
        let avg_priority_sol = (priority_fee_sol(self.cu_price_micro_lamports, COMPUTE_UNIT_LIMIT_CURVE_BUY)
            + priority_fee_sol(self.cu_price_micro_lamports, COMPUTE_UNIT_LIMIT_CURVE_SELL))
            / 2.0;
        self.jito_min_tip_sol + avg_priority_sol
    }
}

fn priority_fee_sol(cu_price_micro_lamports: u64, cu_limit: u32) -> f64 {
    let lamports = cu_price_micro_lamports as f64 * cu_limit as f64 / 1_000_000.0;
    lamports / LAMPORTS_PER_SOL as f64
}

fn env_f64(key: &str, default: f64) -> anyhow::Result<f64> {
    match std::env::var(key) {
        Ok(val) => val
            .parse::<f64>()
            .map_err(|e| anyhow::anyhow!("Invalid value for {key}={val:?}: {e}")),
        Err(_) => Ok(default),
    }
}

fn env_u64(key: &str, default: u64) -> anyhow::Result<u64> {
    match std::env::var(key) {
        Ok(val) => val
            .parse::<u64>()
            .map_err(|e| anyhow::anyhow!("Invalid value for {key}={val:?}: {e}")),
        Err(_) => Ok(default),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_cost_is_tip_plus_avg_priority() {
        let t = FeeTuning {
            jito_min_tip_sol: 0.0001,
            jito_max_tip_sol: 0.0005,
            jito_tip_percentile: 50,
            cu_price_micro_lamports: 200_000,
        };
        let buy = priority_fee_sol(200_000, COMPUTE_UNIT_LIMIT_CURVE_BUY);
        let sell = priority_fee_sol(200_000, COMPUTE_UNIT_LIMIT_CURVE_SELL);
        let want = 0.0001 + (buy + sell) / 2.0;
        assert!((t.fixed_cost_sol_per_leg() - want).abs() < 1e-15);
    }

    #[test]
    fn defaults_match_env_example_tip_band() {
        let d = FeeTuning::defaults();
        assert_eq!(d.jito_min_tip_sol, 0.0001);
        assert_eq!(d.jito_max_tip_sol, 0.0005);
        assert_eq!(d.jito_tip_percentile, 50);
        assert_eq!(d.cu_price_micro_lamports, 200_000);
    }
}
