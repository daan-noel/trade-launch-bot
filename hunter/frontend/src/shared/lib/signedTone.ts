/**
 * Signed-value tone — SSOT for glanceable green/red on PnL-like numbers.
 *
 * Rule: `> 0` green · `< 0` red · `0` / null / NaN neutral (dim).
 * Threshold metrics (win-rate, profit-factor) keep using `goodBad(v, pivot)`
 * with a non-zero pivot — that path is intentionally separate.
 */

import { formatCompact } from 'utils/format';

export type SignedToneClass = 'text-green' | 'text-red' | 'text-text-mid' | 'text-text-dim';

/** StatTile tone names that map onto the same green/red/default palette. */
export type SignedStatTone = 'green' | 'red' | 'default';

/** Tailwind class for a signed number. Null/NaN → dim; zero → mid (not green). */
export function signedToneClass(v: number | null | undefined): SignedToneClass {
  if (v == null || !Number.isFinite(v)) return 'text-text-dim';
  if (v > 0) return 'text-green';
  if (v < 0) return 'text-red';
  return 'text-text-mid';
}

/** StatTile `tone` for the same sign rule. */
export function signedStatTone(v: number | null | undefined): SignedStatTone {
  if (v == null || !Number.isFinite(v) || v === 0) return 'default';
  return v > 0 ? 'green' : 'red';
}

/** `+`-prefixed compact number for signed PnL displays (negatives keep `-`). */
export function formatSigned(v: number, digits: number): string {
  return `${v > 0 ? '+' : ''}${formatCompact(v, digits)}`;
}

/** `+`-prefixed percent string, e.g. `+1.2%` / `-0.5%` / `0.0%`. */
export function formatSignedPct(v: number, digits = 1): string {
  return `${v > 0 ? '+' : ''}${v.toFixed(digits)}%`;
}
