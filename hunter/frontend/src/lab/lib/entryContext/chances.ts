/**
 * **Chances**: how many separate opportunities the market gave for one idea.
 *
 * The scan returns every buy of the selected structure; the idea (the buys table's
 * filters) reads the analysis window before each. A **point** is a buy that passes
 * the filters; a buy that fails them is a **fail**. On one token, in tape order:
 *
 * - a chance starts at a point,
 * - the next points belong to it (each is one more check moment),
 * - it ends at the first fail, or when the next point comes more than
 *   `maxPauseSecs` after the last one.
 *
 * With `failEnds` off, a fail is passed over: only the pause ends a chance.
 *
 * The analysis window only says what a buy reads; it never starts or ends a chance.
 * A buy the scan could not read (`unknown_reason`) is neither a point nor a fail:
 * it is skipped.
 *
 * `pauses` are the gaps between two points in a row on one token (with `failEnds`,
 * no fail between): the gaps `maxPauseSecs` decides to join or split.
 */

import type { ScanMoment } from './types';

/** What ended a chance: a fail, a pause past the limit, or the token's last point. */
export type ChanceEnd = 'fail' | 'pause' | 'last';

export interface Chance {
  mint_address: string;
  /** Its points in tape order; `points[0]` starts it. */
  points: ScanMoment[];
  end: ChanceEnd;
}

export interface GapStats {
  n: number;
  min: number;
  median: number;
  max: number;
}

/** The Max pause values the chances are counted at, seconds. */
export const PAUSE_STEPS = [1, 2, 5, 10, 20, 30, 60, 120, 300] as const;

/** A count that moves less than this share to the next step has settled. */
const SETTLED = 0.02;

const secs = (m: ScanMoment) => Date.parse(m.at) / 1000;

/** Readable buys per token, each token's in tape order `(slot, tx_index)`. */
function byToken(moments: readonly ScanMoment[]): ScanMoment[][] {
  const map = new Map<string, ScanMoment[]>();
  for (const m of moments) {
    if (m.unknown_reason) continue;
    const list = map.get(m.mint_address);
    if (list) list.push(m);
    else map.set(m.mint_address, [m]);
  }
  const out = [...map.values()];
  for (const list of out) list.sort((a, b) => a.slot - b.slot || a.tx_index - b.tx_index);
  return out;
}

/**
 * The chances in `moments` under `isPoint`, joined across pauses up to `maxPauseSecs`;
 * `failEnds` says whether a fail ends one.
 */
export function findChances(
  moments: readonly ScanMoment[],
  isPoint: (m: ScanMoment) => boolean,
  maxPauseSecs: number,
  failEnds = true,
): Chance[] {
  const out: Chance[] = [];
  for (const list of byToken(moments)) {
    let open: Chance | null = null;
    for (const m of list) {
      if (!isPoint(m)) {
        if (!failEnds) continue;
        if (open) open.end = 'fail';
        open = null;
        continue;
      }
      const last = open?.points[open.points.length - 1];
      if (open && last && secs(m) - secs(last) <= maxPauseSecs) {
        open.points.push(m);
      } else {
        if (open) open.end = 'pause';
        open = { mint_address: m.mint_address, points: [m], end: 'last' };
        out.push(open);
      }
    }
  }
  return out;
}

/** Seconds between two points in a row on one token; with `failEnds`, no fail between. */
export function pointPauses(
  moments: readonly ScanMoment[],
  isPoint: (m: ScanMoment) => boolean,
  failEnds = true,
): number[] {
  const out: number[] = [];
  for (const list of byToken(moments)) {
    let last: ScanMoment | null = null;
    for (const m of list) {
      if (!isPoint(m)) {
        if (failEnds) last = null;
        continue;
      }
      if (last) out.push(secs(m) - secs(last));
      last = m;
    }
  }
  return out;
}

export function gapStats(pauses: readonly number[]): GapStats | null {
  if (pauses.length === 0) return null;
  const s = [...pauses].sort((a, b) => a - b);
  const mid = s.length >> 1;
  const median = s.length % 2 ? s[mid] : (s[mid - 1] + s[mid]) / 2;
  return { n: s.length, min: s[0], median, max: s[s.length - 1] };
}

/**
 * The smallest Max pause where the chance count stops moving: the first step whose
 * count differs from the next step's by under {@link SETTLED}. `counts` are
 * `[maxPause, chances]` in rising pause order; `null` when the count never settles
 * or there are no chances.
 */
export function suggestPause(counts: readonly (readonly [number, number])[]): number | null {
  for (let i = 0; i + 1 < counts.length; i++) {
    const [p, n] = counts[i];
    const next = counts[i + 1][1];
    if (n > 0 && (n - next) / n < SETTLED) return p;
  }
  return null;
}
