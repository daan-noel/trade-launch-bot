import { useCallback, useMemo, useState } from 'react';
import { abbreviateIxLabels } from 'lib/ixLabels';
import { coreLabels } from 'lib/strategy/ixCore';
import {
  formatFeePins,
  patternRowKey,
  rowsOverlap,
  rowFromTrade,
  type IxPatternFeeMask,
  type IxPatternRow,
} from 'lib/strategy/ixPatternRows';
import { coreRowFromTrade, type StageTrade } from 'hooks/useIxPatternTarget';
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
const NO_PINS: IxPatternFeeMask = {};

/** How a structure click arms: `level: 'core'` arms the ix core - pinned to the
 *  clicked trade's side and marks when `from` is a trade, the grain a core tag
 *  click writes; `mask` replaces the hook's own `pinMask` for this click. */
export interface StructureLensOpts {
  level?: 'core';
  mask?: IxPatternFeeMask;
}

/** The row a structure click arms: the labels (or their core) plus the fee fields
 *  the mask copies off `from`. A core row off a trade also pins its side and marks. */
export function structureLensRow(
  labels: readonly string[],
  from: StageTrade | null | undefined,
  mask: IxPatternFeeMask | null | undefined,
  level?: 'core',
): IxPatternRow | null {
  if (level !== 'core') return rowFromTrade(labels, from ?? {}, from ? mask : null);
  if (from?.trade_type) return coreRowFromTrade(labels, from, mask);
  const core = coreLabels(labels);
  if (core.length === 0) return null;
  const row = rowFromTrade(core, from ?? {}, from ? mask : null);
  row.level = 'core';
  return row;
}

/** One armed target, as the chips and table rows need it. */
export type TokenHighlightItem = ChartLensItem;

export interface TokenHighlight {
  /** Spread onto `TokenPriceChart` alongside `onHighlightLensMatch`. */
  lens: ChartHighlightLens;
  /** Armed targets in arming order — wallets and structures together. */
  items: readonly TokenHighlightItem[];
  /** True while anything is armed. */
  active: boolean;
  /** Add a wallet; passing an armed one removes it; `null` removes every wallet. */
  toggleWallet: (address: string | null) => void;
  /** Arm an `ix_shape` row; the same row armed removes it; `null` removes every
   *  structure. Every surface arms through this, with the row its tag click writes. */
  toggleRow: (row: IxPatternRow | null) => void;
  /** The color `row` washes in when it is armed itself, else `null`. */
  rowColor: (row: IxPatternRow | null | undefined) => string | null;
  /** Colors of every armed row that shares a trade with `row` (`rowsOverlap`), arming order:
   *  what lights a listed row, whichever surface armed it. */
  overlapColors: (row: IxPatternRow | null | undefined) => string[];
  /** `toggleRow` of {@link structureLensRow}`(labels, from, pinMask or opts.mask,
   *  opts.level)`: a click on a trade or a bare label list. */
  toggleStructure: (labels: readonly string[] | null, from?: StageTrade | null, opts?: StructureLensOpts) => void;
  /** Remove one armed item. */
  remove: (item: Pick<ChartLensItem, 'kind' | 'key'>) => void;
  clear: () => void;
  /** The color an armed wallet washes in, or `null` when it isn't armed. */
  walletColor: (address: string | null | undefined) => string | null;
  /** `rowColor` of the row `toggleStructure(labels, from, opts)` would toggle. */
  structureColor: (
    labels: readonly string[] | null | undefined,
    from?: StageTrade | null,
    opts?: StructureLensOpts,
  ) => string | null;
  /** Which fee fields a structure click copies off its trade (`FeePinToggles`).
   *  Its own mask, not the tag stage's: arming a view-only highlight never changes
   *  what a tag click writes. Remembered across tokens and pages. */
  pinMask: IxPatternFeeMask;
  setPinMask: (mask: IxPatternFeeMask) => void;
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
  /** `colorsOf` over the structure items only: what lights a trade's ix icon. */
  structureColorsOf: (trade: TradeRecord) => string[];
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
  row: IxPatternRow | null,
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
  const narrow = row ? [row.side, formatFeePins(row)].filter(Boolean).join(' ') : '';
  const label = row
    ? `${row.level === 'core' ? 'core ' : ''}${abbreviateIxLabels(row.labels)}${narrow ? ` · ${narrow}` : ''}`
    : key.length > 12
      ? `${key.slice(0, 4)}…${key.slice(-4)}`
      : key;
  next.push({ kind, key, color, label, ...(row ? { row } : {}) });
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
  const [pinMask, setPinMask] = useUiToggle('lensPins', NO_PINS);

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

  const toggleRow = useCallback((row: IxPatternRow | null) => {
    if (!row || row.labels.length === 0) {
      setItems((cur) => cur.filter((i) => i.kind !== 'structure'));
      return;
    }
    setItems((cur) => toggleItem(cur, 'structure', patternRowKey(row), row));
  }, []);

  // What a structure click would arm. One function for the toggle and the color,
  // so a button is lit exactly when its click would disarm.
  const structureTarget = useCallback(
    (labels: readonly string[] | null | undefined, from?: StageTrade | null, opts?: StructureLensOpts) =>
      labels && labels.length > 0 ? structureLensRow(labels, from, opts?.mask ?? pinMask, opts?.level) : null,
    [pinMask],
  );

  const toggleStructure = useCallback(
    (labels: readonly string[] | null, from?: StageTrade | null, opts?: StructureLensOpts) =>
      toggleRow(structureTarget(labels, from, opts)),
    [toggleRow, structureTarget],
  );

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
  const rowColor = useCallback(
    (row: IxPatternRow | null | undefined) =>
      row ? (colorByKey.get(lensItemId({ kind: 'structure', key: patternRowKey(row) })) ?? null) : null,
    [colorByKey],
  );
  const overlapColors = useCallback(
    (row: IxPatternRow | null | undefined) =>
      row ? items.filter((i) => i.row && rowsOverlap(i.row, row)).map((i) => i.color) : [],
    [items],
  );
  const structureColor = useCallback(
    (labels: readonly string[] | null | undefined, from?: StageTrade | null, opts?: StructureLensOpts) =>
      rowColor(structureTarget(labels, from, opts)),
    [rowColor, structureTarget],
  );

  const colorsOf = useCallback(
    (t: TradeRecord) => items.filter((i) => lensItemMatches(i, t)).map((i) => i.color),
    [items],
  );
  const structureColorsOf = useCallback(
    (t: TradeRecord) => items.filter((i) => i.kind === 'structure' && lensItemMatches(i, t)).map((i) => i.color),
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
    toggleRow,
    rowColor,
    overlapColors,
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
    pinMask,
    setPinMask,
    colorsOf,
    structureColorsOf,
  };
}

/** Convenience re-exports so a host doesn't deep-import the chart folder for a type. */
export type { LensMatch };
export { EMPTY_LENS_MATCH };
