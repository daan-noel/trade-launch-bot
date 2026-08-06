/**
 * Signed-value tone — SSOT for glanceable green/red on PnL-like numbers.
 *
 * Rule: `> 0` green · `< 0` red · `0` / null / NaN neutral (dim).
 * Return-magnitude percents use `pctGradeClass` (PnL%, 24h %, etc.).
 * Win-rate fractions use `winRateGradeClass` (50% / 75% / 90% bands).
 * Other threshold metrics (profit-factor) keep using `goodBad(v, pivot)`.
 * SOL amounts stay on `signedToneClass` — magnitude isn't comparable across
 * bag sizes.
 */

import { formatCompact } from 'utils/format';

export type SignedToneClass = 'text-green' | 'text-red' | 'text-text-mid' | 'text-text-dim';

/** StatTile tone names that map onto the same green/red/default palette. */
export type SignedStatTone = 'green' | 'red' | 'default';

/**
 * Magnitude-graded tone for signed percent cells (unit: percent points, e.g. `+120`
 * means +120%). Weight climbs with the bucket so hierarchy survives colorblindness.
 *
 * | range        | tone                          |
 * |--------------|-------------------------------|
 * | `< 0`        | red (bold if ≤ −50)           |
 * | `0`          | mid                           |
 * | `(0, 50)`    | info                          |
 * | `[50, 100)`  | green                         |
 * | `[100, 200)` | warning + semibold            |
 * | `[200, 500)` | accent + bold                 |
 * | `≥ 500`      | primary + extrabold           |
 */
export function pctGradeClass(v: number | null | undefined): string {
  if (v == null || !Number.isFinite(v)) return 'text-text-dim';
  if (v < 0) return v <= -50 ? 'text-red font-bold' : 'text-red';
  if (v === 0) return 'text-text-mid';
  if (v < 50) return 'text-info';
  if (v < 100) return 'text-green';
  if (v < 200) return 'text-warning font-semibold';
  if (v < 500) return 'text-accent font-bold';
  return 'text-primary font-extrabold';
}

/** Bar/fill counterpart of [`pctGradeClass`] — same magnitude bands, `bg-*` tokens. */
export function pctGradeBarClass(v: number | null | undefined): string {
  if (v == null || !Number.isFinite(v)) return 'bg-white/10';
  if (v < 0) return v <= -50 ? 'bg-red/80' : 'bg-red/55';
  if (v === 0) return 'bg-white/20';
  if (v < 50) return 'bg-info/55';
  if (v < 100) return 'bg-green/60';
  if (v < 200) return 'bg-warning/65';
  if (v < 500) return 'bg-accent/70';
  return 'bg-primary/75';
}

/**
 * Graded tone for win-rate fractions (unit: 0..1, e.g. `0.75` = 75%).
 * Coin-flip (50%) is the hard pivot; higher bands flag strong / exceptional
 * rates (often small-N — color only, never a ranking substitute).
 *
 * | range           | tone                          |
 * |-----------------|-------------------------------|
 * | `< 25%`         | red + bold                    |
 * | `[25%, 50%)`    | red                           |
 * | `50%`           | mid                           |
 * | `(50%, 75%)`    | green                         |
 * | `[75%, 90%)`    | warning + semibold            |
 * | `≥ 90%`         | accent + bold                 |
 */
export function winRateGradeClass(v: number | null | undefined): string {
  if (v == null || !Number.isFinite(v)) return 'text-text-dim';
  if (v < 0.25) return 'text-red font-bold';
  if (v < 0.5) return 'text-red';
  if (v <= 0.5) return 'text-text-mid';
  if (v < 0.75) return 'text-green';
  if (v < 0.9) return 'text-warning font-semibold';
  return 'text-accent font-bold';
}

/** Tailwind class for a signed number. Null/NaN → dim; zero → mid (not green). */
export function signedToneClass(v: number | null | undefined): SignedToneClass {
  if (v == null || !Number.isFinite(v)) return 'text-text-dim';
  if (v > 0) return 'text-green';
  if (v < 0) return 'text-red';
  return 'text-text-mid';
}

/**
 * 0..1 "how bot/wash-like" signal grade (Flow Discovery columns / suggest score).
 * Dim at ~0 → warning mid → danger high. Prefer this over ad-hoc rgba blends so
 * warning/danger stay on theme tokens (`--color-warning` / `--color-danger`).
 *
 * | range        | tone                    |
 * |--------------|-------------------------|
 * | `≤ 0.02`     | dim                     |
 * | `(0.02, 0.35)` | warning muted         |
 * | `[0.35, 0.65)` | warning               |
 * | `[0.65, 0.85)` | danger muted          |
 * | `≥ 0.85`     | danger + semibold       |
 */
export function signalGradeClass(t: number | null | undefined): string {
  if (t == null || !Number.isFinite(t)) return 'text-text-dim';
  const c = Math.max(0, Math.min(1, t));
  if (c <= 0.02) return 'text-text-dim';
  if (c < 0.35) return 'text-warning/70';
  if (c < 0.65) return 'text-warning';
  if (c < 0.85) return 'text-red/80';
  return 'text-red font-semibold';
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
