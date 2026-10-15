/**
 * The owner-split auto-tag rules Flow Discovery offers on the `volume` tag: two
 * families (creation slot, transfers), one toggle per rule. A toggle labels the
 * trades of the wallets its rule marks owner, on the coins it marks them on; the
 * backend stage (`lab/src/strategies/owner_stage.rs`) turns those labels into what
 * the tag stores: the ix rows that pass the owner test and the wallets that come
 * back. Rules and numbers: `_!___owner-split.md` Birth, Hidden and "Flow Discovery
 * auto-tag".
 */

import { coreLabels, coreMarks, coreMarksText } from 'lib/strategy/ixCore';
import { parseIxPatternRow, patternRowKey, type IxPatternFee, type IxPatternRow } from 'lib/strategy/ixPatternRows';
import type { OwnerRuleId, OwnerSplitParams, OwnerStage } from 'types';

export type AutoTagFamilyId = 'creation_slot' | 'transfers';

/** How well the threshold is backed (`_!___owner-split.md` Appendix B). */
export type Evidence = 'measured' | 'weak' | 'unproven';

export const EVIDENCE_HELP: Record<Evidence, string> = {
  measured: 'Measured: the threshold was set from the lake (09-30..10-13) and held on later coins.',
  weak:
    'Weak evidence: the threshold was set on one sign only, how often the marked wallets sell in the ' +
    "same slot as the creator. The creator often sells early, so this sign favours early sellers.",
  unproven: 'Unproven: no sign backs this rule yet. It has no number to tune.',
};

export interface AutoTagRule {
  id: OwnerRuleId;
  label: string;
  evidence: Evidence;
  /** What the rule marks, in plain words, with the numbers the run applied. */
  help: (p: OwnerSplitParams) => string;
  /** Needs the lake's market facts; off when the run could not read them. */
  market?: true;
}

export interface AutoTagFamily {
  id: AutoTagFamilyId;
  label: string;
  help: string;
  rules: readonly AutoTagRule[];
}

const SNIPER = (p: OwnerSplitParams) =>
  `A sniper bot is left out: a wallet that bought the creation slot of ${p.sniper_min_coins}+ coins from ` +
  `${p.sniper_min_creators}+ creators in the ${p.sniper_window_days} day(s) before this coin, with at most ` +
  `${p.sniper_max_family_pct}% of them from this coin's create list.`;

export const AUTO_TAG_FAMILIES: readonly AutoTagFamily[] = [
  {
    id: 'creation_slot',
    label: 'Creation slot',
    help:
      'The creation slot is the slot that holds the create. Apps show a new coin only after it, so a buy ' +
      'inside it is the dev filling the launch, unless it is a sniper bot. These rules label one coin at a ' +
      'time: the wallets are new on the next coin, so what gets saved is the ix rows they used.',
    rules: [
      {
        id: 'instant_buy',
        label: 'Instant buy',
        evidence: 'weak',
        help: (p) =>
          `A buy ${p.instant_buy_ms} ms or less after the create. No real buyer sees the coin that fast. ` +
          `${SNIPER(p)} Needs millisecond times.`,
      },
      {
        id: 'creation_slot',
        label: 'Creation slot',
        evidence: 'weak',
        market: true,
        help: (p) => `Any later buy in the creation slot. ${SNIPER(p)} A wallet that buys twice there stays.`,
      },
      {
        id: 'slot_bundle',
        label: 'Bundle',
        evidence: 'weak',
        help: (p) =>
          `Two or more wallets buy in the creation slot, each within ${p.bundle_gap_ms} ms of another buy of the ` +
          `pack. Marks every wallet of the pack. ${SNIPER(p)} Needs millisecond times.`,
      },
      {
        id: 'bought_twice',
        label: 'Bought twice',
        evidence: 'unproven',
        help: () => 'One wallet buys more than once in the creation slot. A sniper fires once; a second buy is the dev.',
      },
    ],
  },
  {
    id: 'transfers',
    label: 'Transfers',
    help:
      'Tokens moved between wallets with no buy or sell, so the tape shows one side only: a sell of tokens ' +
      'the wallet never bought, or a buy that is never sold. Most of it is a volume machine: wallet A buys, ' +
      'wallet B sells the same amount about a second later, on thousands of coins. It is fake demand, so ' +
      'it is owner. These wallets come back, so the wallets themselves are saved.',
    rules: [
      {
        id: 'below_zero',
        label: 'Sell with no buy',
        evidence: 'measured',
        help: (p) =>
          `A sell that takes the wallet's bag below zero by more than ${p.below_zero_min_pct}% of the sell: those ` +
          `tokens came in by transfer. Saved when it happens on ${p.comes_back_min_coins}+ coins of the group.`,
      },
      {
        id: 'transfer_pair',
        label: 'Transfer pair',
        evidence: 'measured',
        help: (p) =>
          'Two wallets moving one amount off the tape. Same amount: one wallet buys X and never sells, another ' +
          `sells X (within ${p.transfer_match_pct}%) and never bought. Matching holder: a wallet sells more than it ` +
          `holds, and exactly one other wallet holds the missing amount. Saved on ${p.comes_back_min_coins}+ coins.`,
      },
      {
        id: 'never_sells',
        label: 'Buy, never sells',
        evidence: 'measured',
        market: true,
        help: (p) =>
          `A wallet with ${p.never_sells_min_coins}+ coins, ${p.never_sells_min_buys}+ buys and no sell anywhere in ` +
          `${p.never_sells_window_days} days. Its job is to make demand. Saved.`,
      },
    ],
  },
];

export const AUTO_TAG_RULES: readonly AutoTagRule[] = AUTO_TAG_FAMILIES.flatMap((f) => f.rules);

/** A field a match level pins on top of the ix list. */
export type MatchPin = 'side' | 'marks' | 'cu_limit' | 'cu_price' | 'tip_lamports';

export interface MatchLevel {
  /** `exact` = the whole ix list in order; `core` = the core of it. */
  list: 'core' | 'exact';
  pins: readonly MatchPin[];
  /** What the level pins, in words. */
  help: string;
}

/** The seven match levels, index = level - 1, loosest first (owner_stage `row_keys`).
 *  A level that pins a fee field is read only on trades that carry it. */
export const MATCH_LEVELS: readonly MatchLevel[] = [
  { list: 'core', pins: ['side'], help: 'the core ix list and the side' },
  { list: 'core', pins: ['side', 'marks'], help: 'the core ix list, the side and the marks' },
  { list: 'core', pins: ['side', 'cu_price'], help: 'the core ix list, the side and the CU price' },
  { list: 'core', pins: ['side', 'tip_lamports'], help: 'the core ix list, the side and the tip' },
  { list: 'core', pins: ['side', 'cu_price', 'tip_lamports'], help: 'the core ix list, the side, the CU price and the tip' },
  {
    list: 'core',
    pins: ['side', 'marks', 'cu_price', 'tip_lamports'],
    help: 'the core ix list, the side, the marks, the CU price and the tip',
  },
  { list: 'exact', pins: ['cu_limit', 'cu_price', 'tip_lamports'], help: 'the exact ix list with the CU limit, CU price and tip' },
];

/** The level that stores an exact row; every lower level stores a core row. */
export const EXACT_LEVEL = MATCH_LEVELS.length;

/** One trade as a match level reads it. */
export interface LevelTrade extends IxPatternFee {
  labels: readonly string[];
  side: 'buy' | 'sell';
}

/** The row `level` stores for this trade, as owner_stage `row_json` writes it; `null`
 *  when the level pins a fee field the trade carries no reading for. */
export function levelRow(level: number, t: LevelTrade): IxPatternRow | null {
  const spec = MATCH_LEVELS[level - 1];
  if (!spec) return null;
  const row: IxPatternRow =
    spec.list === 'exact' ? { labels: [...t.labels] } : { labels: coreLabels(t.labels), level: 'core' };
  for (const pin of spec.pins) {
    if (pin === 'side') row.side = t.side;
    else if (pin === 'marks') row.marks = coreMarksText(coreMarks(t.labels));
    else if (t[pin] == null) return null;
    else row[pin] = t[pin];
  }
  return row;
}

/** What the stage says to save: the passing rows and the passing wallets. */
export function stagedPicks(stage: OwnerStage | null | undefined): { rows: IxPatternRow[]; wallets: string[] } {
  if (!stage) return { rows: [], wallets: [] };
  const rows = stage.rows.map((r) => parseIxPatternRow(r.row)).filter((r): r is IxPatternRow => r != null);
  const wallets = stage.wallets.filter((w) => w.reason == null).map((w) => w.wallet);
  return { rows, wallets };
}

/** What auto-tag put in the draft, and what the user took out of it. */
export interface AutoStage {
  rows: ReadonlySet<string>;
  wallets: ReadonlySet<string>;
  rejectedRows: ReadonlySet<string>;
  rejectedWallets: ReadonlySet<string>;
}

export const EMPTY_AUTO_STAGE: AutoStage = {
  rows: new Set(),
  wallets: new Set(),
  rejectedRows: new Set(),
  rejectedWallets: new Set(),
};

/**
 * The draft after a new stage. An item auto-tag added that the new stage does not
 * pick leaves; a newly picked item joins unless the user took it out; an item the
 * user added by hand is never touched.
 */
export function syncAutoStage(
  draft: { rows: readonly IxPatternRow[]; wallets: readonly string[] },
  auto: AutoStage,
  next: { rows: readonly IxPatternRow[]; wallets: readonly string[] },
): { rows: IxPatternRow[]; wallets: string[]; auto: AutoStage } {
  const sync = <T>(items: readonly T[], key: (t: T) => string, prev: ReadonlySet<string>, rejected: ReadonlySet<string>, want: readonly T[]) => {
    const wantKeys = new Set(want.map(key));
    const kept = items.filter((t) => !(prev.has(key(t)) && !wantKeys.has(key(t))));
    const have = new Set(kept.map(key));
    const added = want.filter((t) => !have.has(key(t)) && !rejected.has(key(t)));
    const nextAuto = new Set([...[...prev].filter((k) => wantKeys.has(k)), ...added.map(key)]);
    return { items: [...kept, ...added], auto: nextAuto };
  };
  const r = sync(draft.rows, patternRowKey, auto.rows, auto.rejectedRows, next.rows);
  const w = sync(draft.wallets, (x) => x, auto.wallets, auto.rejectedWallets, next.wallets);
  return { rows: r.items, wallets: w.items, auto: { ...auto, rows: r.auto, wallets: w.auto } };
}

/** The draft after the user ticks (`on`) or unticks a staged row. */
export function pickAutoRow(
  rows: readonly IxPatternRow[],
  auto: AutoStage,
  row: IxPatternRow,
  on: boolean,
): { rows: IxPatternRow[]; auto: AutoStage } {
  const k = patternRowKey(row);
  const without = rows.filter((r) => patternRowKey(r) !== k);
  const autoRows = new Set(auto.rows);
  const rejected = new Set(auto.rejectedRows);
  if (on) {
    rejected.delete(k);
    autoRows.add(k);
    return { rows: [...without, row], auto: { ...auto, rows: autoRows, rejectedRows: rejected } };
  }
  rejected.add(k);
  autoRows.delete(k);
  return { rows: without, auto: { ...auto, rows: autoRows, rejectedRows: rejected } };
}

/** The draft after the user ticks (`on`) or unticks a staged wallet. */
export function pickAutoWallet(
  wallets: readonly string[],
  auto: AutoStage,
  wallet: string,
  on: boolean,
): { wallets: string[]; auto: AutoStage } {
  const without = wallets.filter((w) => w !== wallet);
  const autoW = new Set(auto.wallets);
  const rejected = new Set(auto.rejectedWallets);
  if (on) {
    rejected.delete(wallet);
    autoW.add(wallet);
    return { wallets: [...without, wallet], auto: { ...auto, wallets: autoW, rejectedWallets: rejected } };
  }
  rejected.add(wallet);
  autoW.delete(wallet);
  return { wallets: without, auto: { ...auto, wallets: autoW, rejectedWallets: rejected } };
}
