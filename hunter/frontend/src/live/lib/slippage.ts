/**
 * Parse a slippage percent string into basis points (1% = 100 bps). Shared by the
 * Holdings and Trade page buy/sell forms so the parse/validation rule lives in one
 * place. Blank → `{}` (let the backend use its default); out-of-range/non-numeric →
 * `{ error }` for the caller to surface.
 */
export function parseSlippageBps(raw: string): { bps?: number; error?: string } {
  const trimmed = raw.trim();
  if (!trimmed) return {};
  const pct = parseFloat(trimmed);
  if (!Number.isFinite(pct) || pct < 0 || pct > 50) {
    return { error: 'Enter a valid slippage % between 0 and 50' };
  }
  return { bps: Math.round(pct * 100) };
}
