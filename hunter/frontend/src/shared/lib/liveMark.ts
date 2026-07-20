import type { LiveTrade } from 'types';

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
