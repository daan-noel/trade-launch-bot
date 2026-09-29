/**
 * **Entry Context axes** — every number the page reads per entry, defined once.
 *
 * An axis is a named per-entry quantity: what it measures, its unit, and how it is
 * read off the server row. The entry table's columns (and so its filter row) and
 * the summary render from this list, so a new axis is one entry here and appears
 * everywhere with its definition attached. Presence of the target (hits, their
 * SOL, the control count, the nearest lag) is not an axis: it is the pre-entry
 * probe's verdict, shown by the probe's own columns. Keep one axis = one quantity,
 * stated in one line; the window/control reads behind them are the engine's
 * `m_flow` reads (see `entry_context.rs`).
 */

import type { EntryRow } from './types';

export type AxisUnit = 'pct' | 'pp' | 'tx' | 'sol';

/** The column group an axis sits in: his own buy, the target in the last `W`
 *  seconds, everyone in the last `W` seconds, or the `W` seconds before that. */
export type AxisGroup = 'buy' | 'target' | 'all' | 'control';

/** Group banner labels for the buys table, with the time spelled out. `w` is the
 *  analysis window in seconds, `probeSlots` the probe's (its own column group). */
export function entryGroupLabels(w: number, probeSlots?: number): Record<string, string> {
  return {
    buy: 'His buy',
    pre_entry: probeSlots != null ? `Probe: last ${probeSlots} slots before him` : 'Probe: slots before him',
    target: `Target: last ${w}s before him`,
    all: `Everyone: last ${w}s before him`,
    control: `Earlier: ${2 * w}s to ${w}s before him`,
  };
}

export interface EntryAxis {
  key: string;
  /** Short column / chip label. */
  label: string;
  group: AxisGroup;
  unit: AxisUnit;
  /** Fraction digits the value renders with. */
  digits: number;
  /** What it measures, over which seconds, with a numeric example; the header
   *  tooltip. `w` is the window in seconds. */
  definition: (w: number) => string;
  /** `null` = no reading (an empty window has no share), which no filter passes. */
  get: (e: EntryRow) => number | null;
  /** The same quantity one step earlier (the control stretch), when it has one: a
   *  buys-table filter on this axis is then also checked there (`entryLogic`). */
  earlier?: (e: EntryRow) => number | null;
}

/** "in the last 30s before his buy" */
const last = (w: number) => `in the last ${w}s before his buy`;
/** The earlier stretch, spelled out: the same length, just before the last one. */
const earlier = (w: number) => `from ${2 * w}s to ${w}s before his buy`;
const NOT_HIS = 'His own buys are never counted.';

export const ENTRY_AXES: readonly EntryAxis[] = [
  {
    key: 'entry_sol',
    label: 'His SOL',
    group: 'buy',
    unit: 'sol',
    digits: 3,
    definition: () => 'SOL he spent on this buy.',
    get: (e) => e.sol,
  },
  {
    key: 'tx_share',
    label: 'Target tx %',
    group: 'target',
    unit: 'pct',
    digits: 0,
    definition: (w) =>
      `Of all buy transactions ${last(w)}, the % made by the target.\n` +
      `Example: 25 buys, 20 of them the target = 80%.\n` +
      `High = the target was most of the buying right before him. ${NOT_HIS}`,
    get: (e) => e.window.tx_share_pct,
    earlier: (e) => e.control.tx_share_pct,
  },
  {
    key: 'sol_share',
    label: 'Target SOL %',
    group: 'target',
    unit: 'pct',
    digits: 0,
    definition: (w) =>
      `Of all SOL spent on buys ${last(w)}, the % spent by the target.\n` +
      `Example: 10 SOL of buys, 8 SOL by the target = 80%.\n${NOT_HIS}`,
    get: (e) => e.window.sol_share_pct,
    earlier: (e) => e.control.sol_share_pct,
  },
  {
    key: 'tag_buy_tx',
    label: 'Target buys',
    group: 'target',
    unit: 'tx',
    digits: 0,
    definition: (w) => `How many buy transactions the target made ${last(w)}.`,
    get: (e) => e.window.tag_buy_tx,
    earlier: (e) => e.control.tag_buy_tx,
  },
  {
    key: 'tag_buy_sol',
    label: 'Target SOL',
    group: 'target',
    unit: 'sol',
    digits: 2,
    definition: (w) => `SOL the target spent on buys ${last(w)}.`,
    get: (e) => e.window.tag_buy_sol,
    earlier: (e) => e.control.tag_buy_sol,
  },
  {
    key: 'buy_tx',
    label: 'All buys',
    group: 'all',
    unit: 'tx',
    digits: 0,
    definition: (w) => `How many buy transactions everyone made ${last(w)}, target or not. ${NOT_HIS}`,
    get: (e) => e.window.buy_tx,
    earlier: (e) => e.control.buy_tx,
  },
  {
    key: 'buy_sol',
    label: 'All buy SOL',
    group: 'all',
    unit: 'sol',
    digits: 2,
    definition: (w) => `SOL everyone spent on buys ${last(w)}, target or not. ${NOT_HIS}`,
    get: (e) => e.window.buy_sol,
    earlier: (e) => e.control.buy_sol,
  },
  {
    key: 'top_tx_share',
    label: 'Top structure tx %',
    group: 'all',
    unit: 'pct',
    digits: 0,
    definition: (w) =>
      `The one structure that made the most buy transactions ${last(w)}, and its % of them, target or not.\n` +
      `Example: 25 buys, 15 by one structure = 60%.\n` +
      `Equal to Target tx % = the target was that structure.`,
    get: (e) => e.groups[0]?.buy_tx_share_pct ?? null,
  },
  {
    key: 'ctl_tx_share',
    label: 'Earlier tx %',
    group: 'control',
    unit: 'pct',
    digits: 0,
    definition: (w) =>
      `The same as Target tx %, but one step earlier: ${earlier(w)}.\n` +
      `It answers: was the target already this busy before?\n` +
      `Example: Target tx % 80%, Earlier tx % 10% = the target showed up right before him.`,
    get: (e) => e.control.tx_share_pct,
  },
  {
    key: 'ctl_sol_share',
    label: 'Earlier SOL %',
    group: 'control',
    unit: 'pct',
    digits: 0,
    definition: (w) =>
      `The same as Target SOL %, but one step earlier: ${earlier(w)}.\n` +
      `Example: Target SOL % 80%, Earlier SOL % 10% = the target's money came in right before him.`,
    get: (e) => e.control.sol_share_pct,
  },
  {
    key: 'tx_share_lift',
    label: 'Tx % jump',
    group: 'control',
    unit: 'pp',
    digits: 0,
    definition: () =>
      'Target tx % minus Earlier tx %.\n' +
      'Example: 80% now, 20% earlier = +60 pts.\n' +
      'Big plus = the target arrived right before his buy. Near 0 = it was there all along. Minus = it faded.',
    get: (e) =>
      e.window.tx_share_pct == null || e.control.tx_share_pct == null
        ? null
        : e.window.tx_share_pct - e.control.tx_share_pct,
  },
];

export const AXIS_BY_KEY: ReadonlyMap<string, EntryAxis> = new Map(
  ENTRY_AXES.map((a) => [a.key, a] as const),
);

const UNIT_SUFFIX: Record<AxisUnit, string> = { pct: '%', pp: ' pts', tx: '', sol: '' };

/** A value in the axis' unit, `-` for no reading. */
export function formatAxis(axis: EntryAxis, v: number | null): string {
  if (v == null || !Number.isFinite(v)) return '-';
  // A difference carries its sign: +60 pts up, -20 pts down.
  const sign = axis.unit === 'pp' && v > 0 ? '+' : '';
  const n = `${sign}${v.toFixed(axis.digits)}${UNIT_SUFFIX[axis.unit]}`;
  return axis.unit === 'sol' ? `◎${n}` : n;
}
