import { useCallback, useMemo, useState } from 'react';
import { patternKey } from 'lib/flow/volumePatterns';
import { useUiToggle } from 'hooks/useUiPrefs';
import { LENS_COLORS } from 'components/token-price-chart/constants';
import {
  EMPTY_LENS_MATCH,
  lensItemId,
  lensItemMatches,
  type LensMatch,
} from 'components/token-price-chart/lensTint';
import type {
  ChartHighlightLens,
  ChartLensItem,
  ChartLensKind,
  ChartLensMatches,
  ChartLensSizeLabels,
} from 'components/token-price-chart/types';
import type { TradeRecord } from 'types';

const EMPTY_MATCHES: ChartLensMatches = new Map();

/** One armed target, as the chips and table rows need it. */
export interface TokenHighlightItem extends ChartLensItem {
  /** A structure's ordered labels, for a chip that has to name it; `null` on a wallet. */
  labels: readonly string[] | null;
}

export interface TokenHighlight {
  /** Spread onto `TokenPriceChart` alongside `onHighlightLensMatch`. */
  lens: ChartHighlightLens;
  /** Armed targets in arming order — wallets and structures together. */
  items: readonly TokenHighlightItem[];
  /** True while anything is armed. */
  active: boolean;
  /** Add a wallet; passing an armed one removes it; `null` removes every wallet. */
  toggleWallet: (address: string | null) => void;
  /** Add an ordered ix structure; an armed one removes it; `null` removes every structure. */
  toggleStructure: (labels: readonly string[] | null) => void;
  /** Remove one armed item. */
  remove: (item: Pick<ChartLensItem, 'kind' | 'key'>) => void;
  clear: () => void;
  /** The color an armed wallet washes in, or `null` when it isn't armed. */
  walletColor: (address: string | null | undefined) => string | null;
  /** The color an armed structure (by `patternKey`) washes in, or `null`. */
  structureColor: (key: string | null | undefined) => string | null;
  /** What the chart matched per item (keyed by `lensItemId`) — the chart owns this
   *  math, so chips quoting these numbers can never disagree with the wash. */
  matches: ChartLensMatches;
  /** Hand to the chart's `onHighlightLensMatch`. */
  onLensMatch: (matches: ChartLensMatches) => void;
  /** Trades on this token whose ix structure was never captured. A structure lens
   *  can say nothing about these, and "0 matches" over a pile of them means "we
   *  never recorded the labels", not "this structure is unique". */
  unlabeled: number;
  /** Size labels under the washes; remembered across tokens and pages. */
  sizeLabels: ChartLensSizeLabels;
  setSizeLabels: (mode: ChartLensSizeLabels) => void;
  /** Colors of every armed item this trade matches, arming order; empty when none. */
  colorsOf: (trade: TradeRecord) => string[];
}

/**
 * Add or remove one item of `kind`. A new item takes the first free color slot of
 * its family, so arming or disarming one never repaints the others. With the
 * family full, the oldest of that kind makes room — its slot goes to the new one.
 */
function toggleItem(
  cur: readonly TokenHighlightItem[],
  kind: ChartLensKind,
  key: string,
  labels: readonly string[] | null,
): TokenHighlightItem[] {
  if (cur.some((i) => i.kind === kind && i.key === key)) {
    return cur.filter((i) => !(i.kind === kind && i.key === key));
  }
  const palette: readonly string[] = LENS_COLORS[kind];
  let next = [...cur];
  const sameKind = next.filter((i) => i.kind === kind);
  if (sameKind.length >= palette.length) next = next.filter((i) => i !== sameKind[0]);
  const used = new Set(next.filter((i) => i.kind === kind).map((i) => i.color));
  const color = palette.find((c) => !used.has(c)) ?? palette[0];
  next.push({ kind, key, color, labels });
  return next;
}

/**
 * The ephemeral highlight lenses for ONE token's chart: "when did these wallets
 * trade" and "when did these ix structures appear" — up to `LENS_COLORS[kind]`
 * of each at once, each in its own color.
 *
 * View-only, and deliberately NOT the tag badge's path. That badge writes a
 * fingerprint tag (which the engine reads) or an `ix_pattern_sets` lens - a reader
 * arming a lens out of curiosity must not be able to change how a live rule
 * trades. Nothing armed is persisted (only the size-label display mode is), and
 * every item drops when the token changes.
 *
 * @param trades the token's full trade history (what the chart is drawing)
 * @param resetKey identity of the token on screen — a change disarms every item
 */
export function useTokenHighlight(
  trades: readonly TradeRecord[],
  resetKey: string,
): TokenHighlight {
  const [items, setItems] = useState<readonly TokenHighlightItem[]>([]);
  const [matches, setMatches] = useState<ChartLensMatches>(EMPTY_MATCHES);
  const [sizeLabels, setSizeLabels] = useUiToggle('lensSizeLabels', 'buys');

  // A new token is a new set of candles — an address or structure carried over
  // from the last one would wash bars that have nothing to do with the pick.
  const [seenKey, setSeenKey] = useState(resetKey);
  if (seenKey !== resetKey) {
    setSeenKey(resetKey);
    setItems([]);
    setMatches(EMPTY_MATCHES);
  }

  const toggleWallet = useCallback((address: string | null) => {
    const next = address?.trim() || null;
    setItems((cur) =>
      next == null ? cur.filter((i) => i.kind !== 'wallet') : toggleItem(cur, 'wallet', next, null),
    );
  }, []);

  const toggleStructure = useCallback((labels: readonly string[] | null) => {
    setItems((cur) =>
      labels && labels.length > 0
        ? toggleItem(cur, 'structure', patternKey(labels), labels)
        : cur.filter((i) => i.kind !== 'structure'),
    );
  }, []);

  const remove = useCallback((item: Pick<ChartLensItem, 'kind' | 'key'>) => {
    setItems((cur) => cur.filter((i) => !(i.kind === item.kind && i.key === item.key)));
  }, []);

  const clear = useCallback(() => setItems([]), []);

  const onLensMatch = useCallback((next: ChartLensMatches) => setMatches(next), []);

  const lens = useMemo<ChartHighlightLens>(() => ({ items, sizeLabels }), [items, sizeLabels]);

  const colorByKey = useMemo(() => {
    const m = new Map<string, string>();
    for (const i of items) m.set(lensItemId(i), i.color);
    return m;
  }, [items]);

  const walletColor = useCallback(
    (address: string | null | undefined) =>
      address ? (colorByKey.get(lensItemId({ kind: 'wallet', key: address })) ?? null) : null,
    [colorByKey],
  );
  const structureColor = useCallback(
    (key: string | null | undefined) =>
      key ? (colorByKey.get(lensItemId({ kind: 'structure', key })) ?? null) : null,
    [colorByKey],
  );

  const colorsOf = useCallback(
    (t: TradeRecord) => items.filter((i) => lensItemMatches(i, t)).map((i) => i.color),
    [items],
  );

  const unlabeled = useMemo(
    () => trades.reduce((n, t) => (t.instruction_labels?.length ? n : n + 1), 0),
    [trades],
  );

  return {
    lens,
    items,
    active: items.length > 0,
    toggleWallet,
    toggleStructure,
    remove,
    clear,
    walletColor,
    structureColor,
    matches: items.length === 0 ? EMPTY_MATCHES : matches,
    onLensMatch,
    unlabeled,
    sizeLabels,
    setSizeLabels,
    colorsOf,
  };
}

/** Convenience re-exports so a host doesn't deep-import the chart folder for a type. */
export type { LensMatch };
export { EMPTY_LENS_MATCH };
