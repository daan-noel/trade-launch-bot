//! Per-tx variation of a single-tx buy/sell's decorations — the compute-budget
//! pair and the instruction order — so consecutive trades do not repeat one
//! shape. Solana-free like [`crate::ix_layout`]; the venue's interpreter turns
//! the drawn values into instructions. Every draw is pure CPU (one RNG call
//! plus integer math, or an index into a list built once per process).

use crate::config::FeeShapeCfg;
use crate::ix_layout::{DecoStep, IxLayout, LayoutKind};
use rand::Rng;
use std::sync::OnceLock;

/// Draw a `(cu_limit, cu_price_micro_lamports)` pair whose priority fee
/// (`limit x price`) never exceeds `floor_cu x price`, the configured pair.
///
/// The limit is `floor_cu + k x limit_step_cu`, up to `floor_cu x
/// limit_span_mult`; `low_band_weight` of the draws take `k` from the lowest
/// third of that range. The price is the budget over the limit, rounded DOWN to
/// `price_step_micro_lamports`. A config that leaves no room to vary (span under
/// one step, or a price below one step) returns the configured pair unchanged.
pub fn draw_compute_budget<R: Rng + ?Sized>(
    floor_cu: u32,
    price_micro_lamports: u64,
    cfg: &FeeShapeCfg,
    rng: &mut R,
) -> (u32, u64) {
    let limit_step = u64::from(cfg.limit_step_cu.max(1));
    let price_step = u128::from(cfg.price_step_micro_lamports.max(1));
    let span_cu = (f64::from(floor_cu) * (cfg.limit_span_mult - 1.0)).max(0.0);
    let max_k = ((span_cu / limit_step as f64) as u64)
        .min((u64::from(u32::MAX) - u64::from(floor_cu)) / limit_step);
    if max_k == 0 || u128::from(price_micro_lamports) < price_step {
        return (floor_cu, price_micro_lamports);
    }
    let k = if rng.gen_bool(cfg.low_band_weight.clamp(0.0, 1.0)) {
        rng.gen_range(0..=max_k / 3)
    } else {
        rng.gen_range(0..=max_k)
    };
    let limit = u64::from(floor_cu) + k * limit_step;
    let budget = u128::from(floor_cu) * u128::from(price_micro_lamports);
    let price = budget / u128::from(limit) / price_step * price_step;
    if price == 0 {
        return (floor_cu, price_micro_lamports);
    }
    (limit as u32, price as u64)
}

/// A random legal order of `[CuLimit, CuPrice, (CreateAta), Core, Tip]`:
/// 60 orders with an account-creation block, 24 without. Each list is built
/// once per process and filtered through [`IxLayout::validate`], so a pick is
/// one index into it.
pub fn pick_layout<R: Rng + ?Sized>(with_ata: bool, rng: &mut R) -> &'static IxLayout {
    static WITH_ATA: OnceLock<Vec<IxLayout>> = OnceLock::new();
    static WITHOUT_ATA: OnceLock<Vec<IxLayout>> = OnceLock::new();
    let list = if with_ata {
        WITH_ATA.get_or_init(|| legal_orders(true))
    } else {
        WITHOUT_ATA.get_or_init(|| legal_orders(false))
    };
    &list[rng.gen_range(0..list.len())]
}

fn legal_orders(with_ata: bool) -> Vec<IxLayout> {
    let mut steps = vec![DecoStep::CuLimit, DecoStep::CuPrice, DecoStep::Core, DecoStep::Tip];
    if with_ata {
        steps.push(DecoStep::CreateAta);
    }
    let mut out = Vec::new();
    permute(&mut steps, 0, &mut out);
    out.retain(|l| l.validate(LayoutKind::Buy, true).is_ok());
    out
}

fn permute(steps: &mut Vec<DecoStep>, at: usize, out: &mut Vec<IxLayout>) {
    if at == steps.len() {
        out.push(IxLayout { steps: steps.clone() });
        return;
    }
    for i in at..steps.len() {
        steps.swap(at, i);
        permute(steps, at + 1, out);
        steps.swap(at, i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::StdRng, SeedableRng};
    use std::collections::HashSet;

    fn on() -> FeeShapeCfg {
        FeeShapeCfg { enabled: true, ..FeeShapeCfg::default() }
    }

    /// The whole point of the cap: no draw may cost more than the configured pair.
    #[test]
    fn a_draw_never_costs_more_than_the_configured_pair() {
        let mut rng = StdRng::seed_from_u64(7);
        for &(floor, price) in &[(110_000u32, 200_000u64), (100_000, 200_000), (180_000, 200_000), (110_000, 50_000)] {
            let budget = u128::from(floor) * u128::from(price);
            for _ in 0..5_000 {
                let (l, p) = draw_compute_budget(floor, price, &on(), &mut rng);
                assert!(u128::from(l) * u128::from(p) <= budget, "{l} x {p} over {floor} x {price}");
                assert!(l >= floor && u64::from(l) <= u64::from(floor) * 2);
                assert_eq!(l % 5_000, floor % 5_000);
                assert_eq!(p % 5_000, 0);
                assert!(p > 0);
            }
        }
    }

    /// The draws spread over the whole range, and most sit in the high-price third.
    #[test]
    fn draws_cover_the_range_and_favor_the_high_price_third() {
        let mut rng = StdRng::seed_from_u64(11);
        let draws: Vec<_> = (0..10_000).map(|_| draw_compute_budget(110_000, 200_000, &on(), &mut rng)).collect();
        let limits: HashSet<u32> = draws.iter().map(|d| d.0).collect();
        assert!(limits.contains(&110_000) && limits.contains(&220_000));
        assert!(limits.len() >= 20, "only {} distinct limits", limits.len());
        let low = draws.iter().filter(|d| d.0 <= 145_000).count();
        assert!(low > 6_000, "{low} of 10000 in the low third");
    }

    /// No room to vary: the configured pair comes back as-is (forge's tiny prices,
    /// a unit test's price of 1).
    #[test]
    fn a_degenerate_config_keeps_the_configured_pair() {
        let mut rng = StdRng::seed_from_u64(3);
        assert_eq!(draw_compute_budget(110_000, 1, &on(), &mut rng), (110_000, 1));
        let flat = FeeShapeCfg { limit_span_mult: 1.0, ..on() };
        assert_eq!(draw_compute_budget(110_000, 200_000, &flat, &mut rng), (110_000, 200_000));
    }

    #[test]
    fn every_legal_order_is_listed_once() {
        assert_eq!(legal_orders(true).len(), 60);
        assert_eq!(legal_orders(false).len(), 24);
        let set: HashSet<Vec<&str>> = legal_orders(true)
            .iter()
            .map(|l| l.steps.iter().map(|s| s.as_str()).collect())
            .collect();
        assert_eq!(set.len(), 60);
        for l in legal_orders(true) {
            let a = l.steps.iter().position(|s| *s == DecoStep::CreateAta).unwrap();
            let c = l.steps.iter().position(|s| *s == DecoStep::Core).unwrap();
            assert!(a < c);
        }
    }

    #[test]
    fn pick_layout_reaches_many_orders() {
        let mut rng = StdRng::seed_from_u64(5);
        let seen: HashSet<Vec<&str>> = (0..2_000)
            .map(|_| pick_layout(false, &mut rng).steps.iter().map(|s| s.as_str()).collect())
            .collect();
        assert_eq!(seen.len(), 24);
    }
}
