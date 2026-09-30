/**
 * His tokens against the market's: where the two sets overlap, token by token.
 *
 * A token of his takes the best class any of his buys on it reaches (`HisClass`).
 * A token sits in one market zone (`Zone`). An area is one zone by one class, and
 * holds the tokens in both: the diagram draws the areas, the token table lists one.
 */

import { hisBuys, inPool } from './counts';
import type { EntryLogic } from './logic';
import type { EntryRow } from './types';

/**
 * - `point`: a buy of his in the pool that passes the filters.
 * - `signal`: a buy of his in the pool; the filters fail on it.
 * - `bought`: he bought the token, never in the pool.
 */
export type HisClass = 'point' | 'signal' | 'bought';

const RANK: Record<HisClass, number> = { bought: 0, signal: 1, point: 2 };

export interface HisToken {
  cls: HisClass;
  /** His buys on the token. */
  buys: number;
  /** Of those, the ones at a point. */
  atPoint: number;
}

/** Each token he bought (readable, in scope), with its class. */
export function hisTokens(
  entries: readonly EntryRow[],
  logic: EntryLogic,
  signaled: (e: EntryRow) => boolean,
  probeOn: boolean,
): Map<string, HisToken> {
  const pool = inPool(logic, signaled, probeOn);
  const out = new Map<string, HisToken>();
  for (const e of hisBuys(entries, logic)) {
    const cls: HisClass = !pool(e) ? 'bought' : logic.idea(e) ? 'point' : 'signal';
    const t = out.get(e.mint_address) ?? { cls, buys: 0, atPoint: 0 };
    if (RANK[cls] > RANK[t.cls]) t.cls = cls;
    t.buys += 1;
    if (cls === 'point') t.atPoint += 1;
    out.set(e.mint_address, t);
  }
  return out;
}

/**
 * - `pass`: a market token with a buy that passes the filters.
 * - `pool`: a market token in the pool with no such buy.
 * - `outside`: a token of his with no buy made with the selected IXs.
 */
export type Zone = 'pass' | 'pool' | 'outside';
export type AreaKey = `${Zone}:${HisClass | 'none'}`;
/** The tokens of each area. `outside:none` is always empty. */
export type Overlap = Record<AreaKey, string[]>;

export function overlap(
  his: ReadonlyMap<string, HisToken>,
  poolMints: readonly string[],
  passMints: readonly string[],
): Overlap {
  const out = {} as Overlap;
  for (const z of ['pass', 'pool', 'outside'] as const) {
    for (const c of ['point', 'signal', 'bought', 'none'] as const) out[`${z}:${c}`] = [];
  }
  const pass = new Set(passMints);
  const market = new Set([...poolMints, ...passMints]);
  for (const mint of market) out[`${pass.has(mint) ? 'pass' : 'pool'}:${his.get(mint)?.cls ?? 'none'}`].push(mint);
  for (const [mint, t] of his) if (!market.has(mint)) out[`outside:${t.cls}`].push(mint);
  return out;
}
