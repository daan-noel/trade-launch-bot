/**
 * **Entry Context axes** — every number the page reads per entry, defined once.
 *
 * An axis is a named per-entry quantity: what it measures, its unit, and how it is
 * read off the server row. The filter bar, the entry columns, the token roll-up
 * and the summary all render from this list, so a new axis is one entry here and
 * appears everywhere with its definition attached. Keep one axis = one quantity,
 * stated in one line; the window/control reads behind them are the engine's
 * `m_flow` reads (see `entry_context.rs`).
 */

import type { EntryRow } from './types';

export type AxisUnit = 'pct' | 'pp' | 'tx' | 'sol';

export interface EntryAxis {
  key: string;
  /** Short column / chip label. */
  label: string;
  unit: AxisUnit;
  /** Fraction digits the value renders with. */
  digits: number;
  /** One line: what it measures, unit, basis and window. `w` is the window in seconds. */
  definition: (w: number) => string;
  /** `null` = no reading (an empty window has no share), which no filter passes. */
  get: (e: EntryRow) => number | null;
}

const win = (w: number) => `in the ${w}s before his buy (his own trades excluded)`;
const ctl = (w: number) => `in the control window, the ${w}s before the window`;

export const ENTRY_AXES: readonly EntryAxis[] = [
  {
    key: 'tx_share',
    label: 'Tag tx %',
    unit: 'pct',
    digits: 0,
    definition: (w) => `Tagged buy transactions over every buy transaction ${win(w)}, %.`,
    get: (e) => e.window.tx_share_pct,
  },
  {
    key: 'sol_share',
    label: 'Tag SOL %',
    unit: 'pct',
    digits: 0,
    definition: (w) => `Tagged buy SOL over every buy SOL ${win(w)}, %.`,
    get: (e) => e.window.sol_share_pct,
  },
  {
    key: 'tag_buy_tx',
    label: 'Tag buys',
    unit: 'tx',
    digits: 0,
    definition: (w) => `Tagged buy transactions ${win(w)}.`,
    get: (e) => e.window.tag_buy_tx,
  },
  {
    key: 'tag_buy_sol',
    label: 'Tag SOL',
    unit: 'sol',
    digits: 2,
    definition: (w) => `SOL the tagged buys spent ${win(w)}.`,
    get: (e) => e.window.tag_buy_sol,
  },
  {
    key: 'buy_tx',
    label: 'All buys',
    unit: 'tx',
    digits: 0,
    definition: (w) => `Every buy transaction ${win(w)}.`,
    get: (e) => e.window.buy_tx,
  },
  {
    key: 'buy_sol',
    label: 'All buy SOL',
    unit: 'sol',
    digits: 2,
    definition: (w) => `SOL every buy spent ${win(w)}.`,
    get: (e) => e.window.buy_sol,
  },
  {
    key: 'ctl_tx_share',
    label: 'Ctl tag tx %',
    unit: 'pct',
    digits: 0,
    definition: (w) => `Tagged buy transactions over every buy transaction ${ctl(w)}, %.`,
    get: (e) => e.control.tx_share_pct,
  },
  {
    key: 'ctl_sol_share',
    label: 'Ctl tag SOL %',
    unit: 'pct',
    digits: 0,
    definition: (w) => `Tagged buy SOL over every buy SOL ${ctl(w)}, %.`,
    get: (e) => e.control.sol_share_pct,
  },
  {
    key: 'tx_share_lift',
    label: 'Tx % lift',
    unit: 'pp',
    digits: 0,
    definition: (w) =>
      `Tag tx % in the window minus the same in the control window (${w}s each), percentage points.`,
    get: (e) =>
      e.window.tx_share_pct == null || e.control.tx_share_pct == null
        ? null
        : e.window.tx_share_pct - e.control.tx_share_pct,
  },
  {
    key: 'top_tx_share',
    label: 'Top group tx %',
    unit: 'pct',
    digits: 0,
    definition: (w) =>
      `The largest structure's buy transactions over every buy transaction ${win(w)}, %.`,
    get: (e) => e.groups[0]?.buy_tx_share_pct ?? null,
  },
  {
    key: 'entry_sol',
    label: 'His buy SOL',
    unit: 'sol',
    digits: 3,
    definition: () => 'SOL his buy legs spent in this transaction.',
    get: (e) => e.sol,
  },
];

export const AXIS_BY_KEY: ReadonlyMap<string, EntryAxis> = new Map(
  ENTRY_AXES.map((a) => [a.key, a] as const),
);

const UNIT_SUFFIX: Record<AxisUnit, string> = { pct: '%', pp: 'pp', tx: '', sol: '' };

/** A value in the axis' unit, `-` for no reading. */
export function formatAxis(axis: EntryAxis, v: number | null): string {
  if (v == null || !Number.isFinite(v)) return '-';
  return `${v.toFixed(axis.digits)}${UNIT_SUFFIX[axis.unit]}`;
}
