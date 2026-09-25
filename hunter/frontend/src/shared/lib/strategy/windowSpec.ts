// A trailing **window** — the frontend mirror of `hunter_engine::metrics::WindowSpec`,
// the part of a span written `10s`, `20sl`, `5p` or `10s@2`.
//
// A window is three things, not one: how wide it is, how far back it ENDS, and the
// unit both are counted in. Time is continuous while slots and prints are discrete,
// so the units are not interchangeable: at ~400 ms a one-second window straddles two
// or three slots and merges bursts that landed separately. Every surface that labels,
// keys or parses a window reads it through this module, so a slot or print window can
// never render as a second window or collapse onto one.

/** What a window counts in (wire `WindowUnit`). */
export type WindowUnit = 'sec' | 'slot' | 'print';

/** Every unit, in resolution order (the Rust `WindowUnit::ALL`). */
export const WINDOW_UNITS: readonly WindowUnit[] = ['sec', 'slot', 'print'] as const;

/** One window, as the backend serializes it. */
export interface WindowSpec {
  size: number;
  /** Units back from *now* the window ENDS. `0` = it ends at now. */
  lag: number;
  unit: WindowUnit;
}

/** Short unit suffix: `s` / `sl` / `p`. Identical to the Rust `WindowUnit::suffix`,
 *  because a stored exit reason and a live chip naming the same read must match. */
export function unitSuffix(unit: WindowUnit): string {
  return unit === 'slot' ? 'sl' : unit === 'print' ? 'p' : 's';
}

/** Long unit name for prose and control labels. */
export function unitLabel(unit: WindowUnit): string {
  return unit === 'slot' ? 'slots' : unit === 'print' ? 'prints' : 'seconds';
}

/** True when a unit's cursor is a discrete counter rather than a clock, so its sizes
 *  are whole buckets and its inputs step by 1. */
export function isDiscreteUnit(unit: WindowUnit): boolean {
  return unit !== 'sec';
}

/** Compact number, the mirror of the engine's `format_metric_threshold`: integers
 *  without `.0`, else up to six trimmed decimals. */
export function formatMetricThreshold(v: number): string {
  if (!Number.isFinite(v)) return String(v);
  if (v === 0) return '0';
  if (Number.isInteger(v) && Math.abs(v) < 1e15) return String(v);
  return v.toFixed(6).replace(/\.?0+$/, '');
}

/** `30s`, `30sl`, `20p`, `30sl@1` (the engine's `WindowSpec::label`). The `@lag` half
 *  appears only when there IS a lag. */
export function formatWindowSpec(spec: WindowSpec | null | undefined): string {
  if (!spec || !Number.isFinite(spec.size) || spec.size <= 0) return '';
  const lag = spec.lag > 0 ? `@${formatMetricThreshold(spec.lag)}` : '';
  return `${formatMetricThreshold(spec.size)}${unitSuffix(spec.unit)}${lag}`;
}

/**
 * Parse a window written by {@link formatWindowSpec}: the mirror of the Rust
 * `WindowSpec::parse`.
 *
 * `null` on anything malformed: a caller must not silently read an unrecognised
 * suffix as a bare number, which is how `30sl` would become a 30-SECOND window. A bare
 * number IS seconds.
 */
export function parseWindowSpec(raw: string): WindowSpec | null {
  const text = raw.trim();
  const at = text.indexOf('@');
  let head = text;
  let lag = 0;
  if (at >= 0) {
    const lagText = text.slice(at + 1).trim();
    lag = lagText === '' ? NaN : Number(lagText);
    if (!Number.isFinite(lag) || lag < 0) return null;
    head = text.slice(0, at);
  }
  // Longest suffix first, or `sl` parses as a seconds span with a stray `l`.
  let unit: WindowUnit = 'sec';
  let sizeText = head;
  for (const u of [...WINDOW_UNITS].sort((a, b) => unitSuffix(b).length - unitSuffix(a).length)) {
    if (head.endsWith(unitSuffix(u))) {
      unit = u;
      sizeText = head.slice(0, -unitSuffix(u).length);
      break;
    }
  }
  if (sizeText.trim() === '') return null;
  const size = Number(sizeText.trim());
  if (!Number.isFinite(size) || size <= 0) return null;
  return { size, lag, unit };
}

/**
 * Dedup identity of a window: the same buffer only if size, lag and unit all agree
 * (the frontend half of `WindowSpec::key`).
 */
export function windowSpecKey(spec: WindowSpec | null | undefined): string {
  if (!spec) return '∅';
  // Millisecond-resolution integers, like the Rust `quantize`, so 0.1 + 0.2 and 0.3
  // are one key rather than two.
  return `${spec.unit}:${Math.round(spec.size * 1000)}:${Math.round(spec.lag * 1000)}`;
}

/** True when two windows are the same. */
export function sameWindowSpec(
  a: WindowSpec | null | undefined,
  b: WindowSpec | null | undefined,
): boolean {
  return windowSpecKey(a) === windowSpecKey(b);
}
