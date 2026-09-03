import type { CostModel, LiveTrade } from 'types';

/**
 * Venue-neutral spot in SOL per raw token unit — same convention as
 * `price_per_token` / chart `tradeSpotPriceSol` / cost-basis avg entry.
 * Prefer post-trade reserves when present; else execution price.
 */
export function liveTradeSpotSolPerRaw(t: LiveTrade): number | null {
  const sol = t.reserve_sol;
  const token = t.reserve_token;
  if (sol != null && token != null && token > 0) {
    const spot = sol / token;
    if (spot > 0 && Number.isFinite(spot)) return spot;
  }
  if (t.price_per_token > 0 && Number.isFinite(t.price_per_token)) {
    return t.price_per_token;
  }
  return null;
}

/** Jupiter-shaped USD per UI token from a SOL/raw spot. */
export function spotSolPerRawToUsd(
  spotSolPerRaw: number,
  decimals: number,
  usdRate: number,
): number | null {
  if (!(usdRate > 0) || !Number.isFinite(spotSolPerRaw) || spotSolPerRaw <= 0) {
    return null;
  }
  const usd = spotSolPerRaw * 10 ** decimals * usdRate;
  return Number.isFinite(usd) ? usd : null;
}

/** Mark-to-market SOL value of a raw token balance at the given spot. */
export function valueSolAtSpot(spotSolPerRaw: number, rawAmount: number): number | null {
  if (!(spotSolPerRaw > 0) || !Number.isFinite(rawAmount)) return null;
  const v = spotSolPerRaw * rawAmount;
  return Number.isFinite(v) ? v : null;
}

/**
 * TS mirror of `hunter_core::strategies::kernel::mark_open_bag`'s exit side: what
 * `valueSol` of an open bag is worth after the sell that would realize it.
 *
 * The browser cannot hold the cost definition -- `costs` comes from
 * `GET /api/meta/cost-model`, so the fee and the tip are this box's configured
 * ones, not a copy that drifts. What is mirrored here is only the arithmetic, and
 * `netProceedsMatchesRust` in `liveMark.test.ts` pins it to the same vectors the
 * Rust `net_proceeds_golden_vectors` test asserts -- change one and the other
 * fails.
 *
 * `reserveSol` is the SOL-side pool depth the sell would land in; `null` charges
 * no impact rather than a guessed one, exactly as the Rust degrades.
 */
export function netProceedsSol(
  valueSol: number,
  reserveSol: number | null | undefined,
  costs: CostModel,
): number {
  if (!Number.isFinite(valueSol) || valueSol <= 0) return 0;
  const fee = costs.fee_bps_per_leg / 10_000;
  const impact =
    costs.price_impact && reserveSol != null && reserveSol > 0
      ? valueSol / reserveSol
      : 0;
  const afterImpact = valueSol * Math.max(1 - impact, 0);
  return afterImpact * (1 - fee) - costs.fixed_cost_sol_per_leg;
}

/**
 * Net unrealized PnL of an open bag marked at `valueSol`, against the all-in
 * `costBasisSol` the server already computed (curve cost + the entry leg's fee
 * and fixed cost). Returns `null` for `pct` with no basis -- a tile renders a dash
 * rather than asserting a break-even nobody measured.
 */
export function unrealizedFromValue(
  valueSol: number,
  costBasisSol: number,
  reserveSol: number | null | undefined,
  costs: CostModel,
): { pnlSol: number; pnlPct: number | null } {
  const pnlSol = netProceedsSol(valueSol, reserveSol, costs) - costBasisSol;
  return {
    pnlSol,
    pnlPct: costBasisSol > 0 ? (pnlSol / costBasisSol) * 100 : null,
  };
}
