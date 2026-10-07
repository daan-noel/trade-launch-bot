/**
 * Reserve match, as both pages count and filter it.
 * It names the transaction. A definite entry is one print, quiet, slippage read here.
 */

import type { EntryRow, ReserveKind, SlippageReading } from './types';

export type SignalFocus = 'all' | 'definite' | 'single' | 'crowded' | 'blank';

export interface SignalCounts {
  definite: number;
  single: number;
  crowded: number;
  noCeiling: number;
  noMatch: number;
  several: number;
  empty: number;
}

const ZERO: SignalCounts = {
  definite: 0,
  single: 0,
  crowded: 0,
  noCeiling: 0,
  noMatch: 0,
  several: 0,
  empty: 0,
};

export function signalCounts(entries: readonly EntryRow[]): SignalCounts {
  const c = { ...ZERO };
  for (const e of entries) {
    switch (e.reserve_call?.kind) {
      case 'definite':
        c.definite += 1;
        break;
      case 'single':
        c.single += 1;
        break;
      case 'crowded':
        c.crowded += 1;
        break;
      case 'no_ceiling':
        c.noCeiling += 1;
        break;
      case 'no_match':
        c.noMatch += 1;
        break;
      case 'several':
        c.several += 1;
        break;
      default:
        c.empty += 1;
        break;
    }
  }
  return c;
}

export function matchesSignal(e: EntryRow, focus: SignalFocus): boolean {
  const kind: ReserveKind | undefined = e.reserve_call?.kind;
  switch (focus) {
    case 'all':
      return true;
    case 'definite':
      return kind === 'definite';
    case 'single':
      return kind === 'single';
    case 'crowded':
      return kind === 'crowded';
    case 'blank':
      return kind === 'no_ceiling' || kind === 'no_match' || kind === 'several';
  }
}

/** Gold rail for a definite entry. The only filled gold on either page. */
export const DEFINITE_ROW =
  'bg-warning/10 hover:bg-warning/15 shadow-[inset_3px_0_0_0_var(--color-warning)]';

export function isDefinite(e: EntryRow): boolean {
  return e.reserve_call?.kind === 'definite';
}

/** `20` or `15.5`. */
export function formatSlip(pct: number | null | undefined): string {
  if (pct == null || !Number.isFinite(pct)) return '';
  const r = Math.round(pct * 10) / 10;
  return Number.isInteger(r) ? String(r) : r.toFixed(1);
}

export function formatQuote(sol: number): string {
  return sol.toFixed(4);
}

/** Slippage percents from a comma-separated box. Blank leaves the reading to the entries. */
export function parseSlippagePct(raw: string): number[] {
  return raw
    .split(/[,\s]+/)
    .map((s) => Number(s))
    .filter((n) => Number.isFinite(n) && n >= 0 && n < 500);
}

export function readingLabel(r: SlippageReading): string {
  const family = r.family === 'floor' ? 'floor ' : r.family === 'ceiling' ? 'ceiling ' : '';
  return `${family}${formatSlip(r.pct)}% · ${r.entries}`;
}
