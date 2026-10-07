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

import type { EntryGroupRow, EntryRow } from './types';

/** The breakdown row of the ix-pick signal's structure. */
export const signalGroup = (e: EntryRow): EntryGroupRow | undefined => e.groups.find((g) => g.signal);
/** The breakdown row of the reserve-match signal's structure. */
export const reserveGroup = (e: EntryRow): EntryGroupRow | undefined => e.groups.find((g) => g.reserve);

export type AxisUnit = 'pct' | 'pp' | 'tx' | 'sol';

/** The column group an axis sits in. `signal` is the instruction-pick print.
 *  `reserve` is the reserve-match print. */
export type AxisGroup = 'buy' | 'signal' | 'reserve' | 'target' | 'control' | 'all' | 'holders';

/**
 * An idea family: one aspect of the tape at his buy, one sub-tab of the buys
 * table. A sub-tab shows its family's column groups plus the ones no family owns
 * (his buy, the probe); every row and every filter are shared, so a condition set
 * in one sub-tab keeps applying in the others. A new family is one entry here and
 * one group on its axes.
 */
export interface AxisFamily {
  key: string;
  /** Sub-tab label. */
  label: string;
  groups: readonly AxisGroup[];
}

export const AXIS_FAMILIES: readonly AxisFamily[] = [
  { key: 'ix', label: 'IX structure', groups: ['signal', 'reserve', 'target', 'control', 'all'] },
  { key: 'holders', label: 'Top holders', groups: ['holders'] },
];

/** The family a buys-table column belongs to; `null` = shown in every sub-tab
 *  (his buy, the probe). */
export const familyOf = (key: string): string | null => {
  const g = AXIS_BY_KEY.get(key)?.group;
  return AXIS_FAMILIES.find((f) => g != null && f.groups.includes(g))?.key ?? null;
};

/** The column groups `family`'s sub-tab hides: every other family's. */
export const groupsHiddenIn = (family: string): AxisGroup[] =>
  AXIS_FAMILIES.filter((f) => f.key !== family).flatMap((f) => f.groups);

/** A group read through the selected IXs (the target): a filter on it only means
 *  something in the pool. `buy` and `all` read the same whatever IXs came before. */
export const needsIxs = (g: AxisGroup): boolean => g === 'target' || g === 'control';

/** Group banner labels for the buys table, with the time spelled out. `w` is the
 *  analysis window in seconds, `probeSlots` the probe's (its own column group). */
export function entryGroupLabels(w: number, probeSlots?: number, slotsBefore = 2): Record<string, string> {
  return {
    buy: 'His buy',
    pre_entry: probeSlots != null ? `Probe: last ${probeSlots} slots before him` : 'Probe: slots before him',
    signal: `IX signal: ${slotsBefore} slots before him`,
    reserve: `Reserve signal: ${slotsBefore} slots before him`,
    target: `Target: last ${w}s before him`,
    control: `Target earlier: ${2 * w}s to ${w}s before him`,
    all: `Everyone: last ${w}s before him`,
    holders: 'Top holders: price drop if they sell all at once',
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
const IX =
  'The ix signal = the closest buy in the slots before him that survives instruction pick: racers drop when a plain buy is present, and a shape that also buys in the slot after him drops. It does not depend on the Target IX set.';
const RESERVE =
  'The reserve signal = the print in the slots before him whose quote matches his ceiling at one of his slippages, within the lamport slack. One print in that window is the signal on its own. Blank when several prints sit there and his buy ceiling is not stored, or none of them match.';

/** A top-holder column's tooltip: which wallets, then how the drop is worked out. */
const holderDrop = (who: string, sold: string, example: string) =>
  `How far the price falls if ${who} sold all their tokens at once, right before his buy.\n` +
  `1. Each wallet's tokens = bought - sold, from every trade since the coin was made (his own wallet left out).\n` +
  `2. T = ${sold}. P = tokens in the pool after the last trade before him.\n` +
  `3. Drop = 1 - (P / (P + T))^2. The pool keeps SOL x tokens fixed, so selling T tokens into it divides the price by ((P + T) / P)^2. Fees left out.\n` +
  `Example: P = 500M, ${example}\n` +
  `Blank = the coin is older than the stored trades, so its early holders are unknown.`;

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
    key: 'sig_buy_tx',
    label: 'IX TXs',
    group: 'signal',
    unit: 'tx',
    digits: 0,
    definition: (w) =>
      `How many buy transactions the signal's structure made ${last(w)}, target or not.
` +
      `Example: 25 buys, 12 of them with that structure = 12.
${IX} Blank = no signal.`,
    get: (e) => signalGroup(e)?.buy_tx ?? null,
  },
  {
    key: 'sig_tx_share',
    label: 'IX tx %',
    group: 'signal',
    unit: 'pct',
    digits: 0,
    definition: (w) =>
      `Of all buy transactions ${last(w)}, the % made with the signal's structure.
` +
      `Example: 25 buys, 12 with it = 48%.
${IX}`,
    get: (e) => signalGroup(e)?.buy_tx_share_pct ?? null,
  },
  {
    key: 'sig_buy_sol',
    label: 'IX SOL',
    group: 'signal',
    unit: 'sol',
    digits: 2,
    definition: (w) =>
      `SOL the signal's structure spent on buys ${last(w)}, target or not.
${IX}`,
    get: (e) => signalGroup(e)?.buy_sol ?? null,
  },
  {
    key: 'sig_sol_share',
    label: 'IX SOL %',
    group: 'signal',
    unit: 'pct',
    digits: 0,
    definition: (w) =>
      `Of all SOL spent on buys ${last(w)}, the % spent with the signal's structure.
` +
      `Example: 10 SOL of buys, 4 SOL with it = 40%.
${IX}`,
    get: (e) => signalGroup(e)?.buy_sol_share_pct ?? null,
  },
  {
    key: 'rsv_buy_tx',
    label: 'Reserve TXs',
    group: 'reserve',
    unit: 'tx',
    digits: 0,
    definition: (w) =>
      `How many buy transactions the reserve signal's structure made ${last(w)}.\n` +
      `Example: 25 buys, 12 of them with that structure = 12.\n${RESERVE}`,
    get: (e) => reserveGroup(e)?.buy_tx ?? null,
  },
  {
    key: 'rsv_tx_share',
    label: 'Reserve tx %',
    group: 'reserve',
    unit: 'pct',
    digits: 0,
    definition: (w) =>
      `Of all buy transactions ${last(w)}, the % made with the reserve signal's structure.\n` +
      `Example: 25 buys, 12 with it = 48%.\n${RESERVE}`,
    get: (e) => reserveGroup(e)?.buy_tx_share_pct ?? null,
  },
  {
    key: 'rsv_buy_sol',
    label: 'Reserve SOL',
    group: 'reserve',
    unit: 'sol',
    digits: 2,
    definition: (w) =>
      `SOL the reserve signal's structure spent on buys ${last(w)}.\n${RESERVE}`,
    get: (e) => reserveGroup(e)?.buy_sol ?? null,
  },
  {
    key: 'rsv_sol_share',
    label: 'Reserve SOL %',
    group: 'reserve',
    unit: 'pct',
    digits: 0,
    definition: (w) =>
      `Of all SOL spent on buys ${last(w)}, the % spent with the reserve signal's structure.\n` +
      `Example: 10 SOL of buys, 4 SOL with it = 40%.\n${RESERVE}`,
    get: (e) => reserveGroup(e)?.buy_sol_share_pct ?? null,
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
    key: 'top_buy_tx',
    label: 'Top structure TXs',
    group: 'all',
    unit: 'tx',
    digits: 0,
    definition: (w) =>
      `How many buy transactions the one busiest structure made ${last(w)}, target or not.\n` +
      `Example: 25 buys, 15 by one structure = 15. ${NOT_HIS}`,
    get: (e) => e.groups[0]?.buy_tx ?? null,
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
    key: 'top_buy_sol',
    label: 'Top structure SOL',
    group: 'all',
    unit: 'sol',
    digits: 2,
    definition: (w) =>
      `SOL the same busiest structure (most buy transactions) spent on buys ${last(w)}, target or not. ${NOT_HIS}`,
    get: (e) => e.groups[0]?.buy_sol ?? null,
  },
  {
    key: 'top_sol_share',
    label: 'Top structure SOL %',
    group: 'all',
    unit: 'pct',
    digits: 0,
    definition: (w) =>
      `Of all SOL spent on buys ${last(w)}, the % spent by the same busiest structure.\n` +
      `Example: 10 SOL of buys, 6 SOL by that structure = 60%.`,
    get: (e) => e.groups[0]?.buy_sol_share_pct ?? null,
  },
  {
    key: 'top1_drop',
    label: 'Top 1',
    group: 'holders',
    unit: 'pct',
    digits: 1,
    definition: () =>
      holderDrop('the biggest holder', "the biggest holder's tokens", 'T = 30M: 1 - (500 / 530)^2 = 11.0%.'),
    get: (e) => e.holders?.top1_drop_pct ?? null,
  },
  {
    key: 'top10_drop',
    label: 'Top 10',
    group: 'holders',
    unit: 'pct',
    digits: 1,
    definition: () =>
      holderDrop(
        'the 10 biggest holders together',
        'the tokens of the 10 biggest holders (all of them when there are fewer)',
        'T = 95M: 1 - (500 / 595)^2 = 29.4%.',
      ),
    get: (e) => e.holders?.top10_drop_pct ?? null,
  },
  {
    key: 'top1pct_drop',
    label: 'Top 1%',
    group: 'holders',
    unit: 'pct',
    digits: 1,
    definition: () =>
      holderDrop(
        'the biggest 1% of holders together',
        'the tokens of the biggest 1% of holders, rounded up (300 holders = 3, under 100 = 1)',
        '3 wallets hold T = 60M: 1 - (500 / 560)^2 = 20.3%.',
      ),
    get: (e) => e.holders?.top1pct_drop_pct ?? null,
  },
  {
    key: 'top10pct_drop',
    label: 'Top 10%',
    group: 'holders',
    unit: 'pct',
    digits: 1,
    definition: () =>
      holderDrop(
        'the biggest 10% of holders together',
        'the tokens of the biggest 10% of holders, rounded up (300 holders = 30)',
        '30 wallets hold T = 140M: 1 - (500 / 640)^2 = 39.0%.',
      ),
    get: (e) => e.holders?.top10pct_drop_pct ?? null,
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
