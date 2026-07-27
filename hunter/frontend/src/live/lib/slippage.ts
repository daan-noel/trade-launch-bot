/**
 * Parse a slippage percent string into basis points (1% = 100 bps). Shared by the
 * Holdings and Trade page buy/sell forms so the parse/validation rule lives in one
 * place. Blank → `{}` (let the backend use its default — which for a sell is NO
 * floor, "sell all"); out-of-range/non-numeric → `{ error }` for the caller to
 * surface.
 *
 * `0` is NOT accepted: the backend honors a typed value literally, so `0` would
 * mean "revert on any price movement at all" and is refused with a 400. Blank is
 * how you ask for no floor. `MIN_SLIPPAGE_PCT` is also the smallest percent that
 * survives the `Math.round(pct * 100)` conversion as a non-zero bps value.
 */
export const MIN_SLIPPAGE_PCT = 0.01;

export function parseSlippageBps(raw: string): { bps?: number; error?: string } {
  const trimmed = raw.trim();
  if (!trimmed) return {};
  const pct = parseFloat(trimmed);
  if (!Number.isFinite(pct) || pct < MIN_SLIPPAGE_PCT || pct > 50) {
    return { error: `Enter a slippage % between ${MIN_SLIPPAGE_PCT} and 50, or leave it blank for no limit` };
  }
  return { bps: Math.round(pct * 100) };
}
