/**
 * The owner-split auto-tag rules Flow Discovery offers on the `volume` tag: two
 * families (creation slot, transfers), one toggle per rule. The backend runs each
 * rule (`lab/src/strategies/owner_split.rs`) and ships the wallets it marks, per
 * group and per coin; a toggle puts those wallets into the draft's wallet list or
 * takes them out. Rules and numbers: `_!___owner-split.md` Birth and Hidden.
 */

import type { OwnerMarks, OwnerRuleId, OwnerSplitParams } from 'types';

export type AutoTagFamilyId = 'creation_slot' | 'transfers';

export interface AutoTagRule {
  id: OwnerRuleId;
  label: string;
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

const TOGGLE =
  'Each button adds the wallets its rule marks to the wallet list, and a second click takes them out. ' +
  'A listed wallet tags its trades on every coin, and sticky carries its later trades on that coin.';

export const AUTO_TAG_FAMILIES: readonly AutoTagFamily[] = [
  {
    id: 'creation_slot',
    label: 'Creation slot',
    help:
      'The creation slot is the slot that holds the create. Apps show a new coin only after it, ' +
      'so a buy inside it is the dev filling the launch, unless it is a sniper bot. ' +
      'The wallets of the create itself are not here: the Creator switch covers them. ' +
      TOGGLE,
    rules: [
      {
        id: 'instant_buy',
        label: 'Instant buy',
        help: (p) =>
          `A buy ${p.instant_buy_ms} ms or less after the create. No real buyer can see the coin that fast, so it is the dev. ` +
          'Size and ix structure do not matter. Needs millisecond times: a trade synced with whole-second chain time is skipped.',
      },
      {
        id: 'creation_slot',
        label: 'Creation slot',
        market: true,
        help: (p) =>
          'Any later buy in the creation slot, except a sniper. ' +
          `A sniper bought the opening of ${p.sniper_min_coins}+ coins from ${p.sniper_min_creators}+ creators ` +
          `in ${p.market_window_days} days, with at most ${p.sniper_max_group_pct}% of them in this group. ` +
          'A sniper inside a bundle, or one that buys twice, still counts.',
      },
      {
        id: 'slot_bundle',
        label: 'Bundle',
        help: (p) =>
          `Two or more wallets buy in the creation slot, each within ${p.bundle_gap_ms} ms of another buy of the pack. ` +
          'The dev packs the first buys so no stranger lands between them. Marks every wallet of the pack. Needs millisecond times.',
      },
      {
        id: 'bought_twice',
        label: 'Bought twice',
        help: () =>
          'One wallet buys more than once in the creation slot. A sniper fires once; a second buy in the same slot is the dev.',
      },
    ],
  },
  {
    id: 'transfers',
    label: 'Transfers',
    help:
      'The dev moves tokens between their own wallets with no buy or sell, so the tape shows only one side: ' +
      'a sell of tokens the wallet never bought, or a buy that is never sold. ' +
      'The bag is buys minus sells on one coin; a transfer does not move it. ' +
      TOGGLE,
    rules: [
      {
        id: 'below_zero',
        label: 'Sell with no buy',
        help: (p) =>
          `A sell that takes the wallet's bag below zero by more than ${p.below_zero_min_pct}% of the sell: those tokens came in by transfer. ` +
          `This token: every such wallet here. Group: wallets that do it on ${p.below_zero_group_coins}+ coins of the group.`,
      },
      {
        id: 'transfer_pair',
        label: 'Transfer pair',
        help: (p) =>
          'Two wallets moving one amount off the tape. ' +
          `Same amount: one wallet buys X and never sells, another sells X (within ${p.transfer_match_pct}%) and never bought. ` +
          'Matching holder: a wallet sells more than it holds, and exactly one other wallet still holds the missing amount. ' +
          'Marks both wallets of each pair.',
      },
      {
        id: 'never_sells',
        label: 'Buy, never sells',
        market: true,
        help: (p) =>
          `A wallet with ${p.never_sells_min_coins}+ coins, ${p.never_sells_min_buys}+ buys and no sell anywhere ` +
          `in ${p.market_window_days} days. Its job is to make demand. Marks such wallets that bought here.`,
      },
    ],
  },
];

export const AUTO_TAG_RULES: readonly AutoTagRule[] = AUTO_TAG_FAMILIES.flatMap((f) => f.rules);

/** `on` = every wallet of the rule is listed, `some` = part of them, `off` = none. */
export type AutoTagState = 'on' | 'some' | 'off';

export function autoTagState(listed: ReadonlySet<string>, wallets: readonly string[]): AutoTagState {
  if (wallets.length === 0) return 'off';
  const n = wallets.reduce((k, w) => (listed.has(w) ? k + 1 : k), 0);
  return n === wallets.length ? 'on' : n > 0 ? 'some' : 'off';
}

/**
 * The wallet list after a click on rule `id`. An `on` rule takes its wallets out,
 * except a wallet another `on` rule also marks: switching one rule off must not
 * switch another off under it. Any other state adds every wallet the rule marks.
 */
export function toggleAutoTag(list: readonly string[], marks: OwnerMarks, id: OwnerRuleId): string[] {
  const wallets = marks[id] ?? [];
  const listed = new Set(list);
  if (autoTagState(listed, wallets) !== 'on') {
    return [...list, ...wallets.filter((w) => !listed.has(w))];
  }
  const keep = new Set<string>();
  for (const r of AUTO_TAG_RULES) {
    if (r.id === id) continue;
    const other = marks[r.id] ?? [];
    if (autoTagState(listed, other) === 'on') other.forEach((w) => keep.add(w));
  }
  const drop = new Set(wallets.filter((w) => !keep.has(w)));
  return list.filter((w) => !drop.has(w));
}
