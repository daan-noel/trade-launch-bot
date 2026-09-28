// Rule-role colors. The chain, the In-words marks, the editor, and the summary
// chips all read these. A signal name is `ROLE.signal` everywhere it is drawn.

import type { CSSProperties } from 'react';

export const ROLE = {
  buy: 'var(--color-buy)',
  sell: 'var(--color-sell)',
  info: 'var(--color-info)',
  stage: 'var(--color-accent)',
  signal: 'var(--color-secondary)',
  warn: 'var(--color-warning)',
  text: 'var(--color-text)',
  mid: 'var(--color-text-mid)',
  dim: 'var(--color-text-dim)',
} as const;

/** `pct` of `base`, the rest `toward`. */
export function mix(base: string, pct: number, toward: string): string {
  return `color-mix(in srgb, ${base} ${pct}%, ${toward})`;
}

export function wash(base: string, pct: number): string {
  return `color-mix(in srgb, ${base} ${pct}%, transparent)`;
}

export type BuyRole = 'on' | 'if' | 'giveup' | 'lock';

/** Buy gates stay green, a step apart. Tries stay gray, strictest toward green. */
export function buyRoleColor(key: BuyRole, lock: 'token' | 'slot' | null): string {
  if (key === 'on') return ROLE.buy;
  if (key === 'if') return mix(ROLE.buy, 55, 'white');
  if (key === 'giveup') return mix(ROLE.buy, 62, ROLE.warn);
  if (lock === 'token') return mix(ROLE.dim, 68, ROLE.buy);
  if (lock === 'slot') return ROLE.mid;
  return ROLE.text;
}

/** Sell steps. Always is info, a stage is accent, TP/SL stays neutral. A signal is its own row. */
export function sellRoleColor(key: string): { color: string; dashed: boolean } {
  if (key === 'always') return { color: ROLE.info, dashed: true };
  if (key === 'tpsl') return { color: ROLE.text, dashed: false };
  return { color: ROLE.stage, dashed: false };
}

/**
 * Logic-column words. `if` / `then` are the test blue (then a step lighter).
 * `any` / `or` / `v` are the signal gold. `and` / `all` / `else` stay dim.
 */
export function leadColor(word: string): string {
  if (word === 'if') return ROLE.info;
  if (word === 'then') return mix(ROLE.info, 62, 'white');
  if (word === 'any' || word === 'or' || word === 'v') return ROLE.signal;
  return ROLE.dim;
}

/** The signal name: gold ink, a light fill. Chain, marks, and chips share it. */
export function signalMark(): { color: string; fill: string } {
  return { color: ROLE.signal, fill: wash(ROLE.signal, 12) };
}

export function signalChipStyle(): CSSProperties {
  const { color, fill } = signalMark();
  return { color, borderColor: color, backgroundColor: fill };
}
