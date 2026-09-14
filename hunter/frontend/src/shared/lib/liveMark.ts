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
 * TS mirror of `hunter_core::strategies::kernel::sell_value_proceeds` for the sell
 * that empties a bag: what `valueSol` (the bag at spot) returns to the wallet. The
 * curve pays `g / (1 + g/vsol)`, the venue keeps its fee, and the sell's fixed cost
 * and the rent-reclaim close come off the rest.
 *
 * The browser cannot hold the cost definition -- `costs` comes from
 * `GET /api/meta/cost-model`, so the fee and the tip are this box's configured
 * ones, not a copy that drifts. What is mirrored here is only the arithmetic, and
 * `netProceedsMatchesRust` in `liveMark.test.ts` pins it to the same vectors the
 * Rust `sell_value_proceeds_golden_vectors` test asserts -- change one and the
 * other fails.
 *
 * `reserveSol` is the priced SOL depth the sell would land in; `null` charges no
 * impact rather than a guessed one, exactly as the Rust degrades.
 */
export function netProceedsSol(
  valueSol: number,
  reserveSol: number | null | undefined,
  costs: CostModel,
): number {
  const fee = costs.fee_bps_per_leg / 10_000;
  const value = Number.isFinite(valueSol) ? Math.max(valueSol, 0) : 0;
  const out =
    costs.price_impact && reserveSol != null && Number.isFinite(reserveSol) && reserveSol > 0
      ? value / (1 + value / reserveSol)
      : value;
  return out * (1 - fee) - costs.fixed_sell_sol - costs.close_fee_sol;
}

/**
 * Net unrealized PnL of an open bag marked at `valueSol`, against the
 * `costBasisSol` the server already computed (the SOL the entry took from the
 * wallet). Returns `null` for `pct` with no basis -- a tile renders a dash rather
 * than asserting a break-even nobody measured.
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
