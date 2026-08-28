import { useCallback, useMemo, useState } from 'react';
import { patternKey } from 'lib/flow/volumePatterns';
import {
  EMPTY_LENS_MATCH,
  type LensMatch,
} from 'components/token-price-chart/lensTint';
import type {
  ChartHighlightLens,
  ChartLensMatches,
} from 'components/token-price-chart/types';
import type { TradeRecord } from 'types';

const EMPTY_MATCHES: ChartLensMatches = {
  wallet: EMPTY_LENS_MATCH,
  structure: EMPTY_LENS_MATCH,
};

export interface TokenHighlight {
  /** Spread onto `TokenPriceChart` alongside `onHighlightLensMatch`. */
  lens: ChartHighlightLens;
  /** True while either lens is armed. */
  active: boolean;
  /** Arm a wallet; passing the armed wallet again disarms it. */
  toggleWallet: (address: string | null) => void;
  /** Arm an ordered ix structure; passing the armed one again disarms it. */
  toggleStructure: (labels: readonly string[] | null) => void;
  clear: () => void;
  /** The armed structure's labels, for a chip that has to name it. */
  structureLabels: readonly string[] | null;
  /** What the chart matched — the chart owns this math, so chips quoting these
   *  numbers can never disagree with the wash the reader is looking at. */
  matches: ChartLensMatches;
  /** Hand to the chart's `onHighlightLensMatch`. */
  onLensMatch: (matches: ChartLensMatches) => void;
  /** Trades on this token whose ix structure was never captured. A structure lens
   *  can say nothing about these, and "0 matches" over a pile of them means "we
   *  never recorded the labels", not "this structure is unique". */
  unlabeled: number;
  isWalletMatch: (trade: TradeRecord) => boolean;
  isStructureMatch: (trade: TradeRecord) => boolean;
}

/**
 * The two ephemeral highlight lenses for ONE token's chart: "when did this wallet
 * trade" and "when did this ix structure appear".
 *
 * View-only, and deliberately NOT the Tagged badge's path. That badge writes
 * `ix_patterns` / `ix_pattern_sets`, which the engine reads to classify
 * flow — a reader arming a lens out of curiosity must not be able to change how a
 * live rule trades. Nothing here is persisted, and both lenses drop when the
 * token changes.
 *
 * @param trades the token's full trade history (what the chart is drawing)
 * @param resetKey identity of the token on screen — a change disarms both lenses
 */
export function useTokenHighlight(
  trades: readonly TradeRecord[],
  resetKey: string,
): TokenHighlight {
  const [wallet, setWallet] = useState<string | null>(null);
  const [structureLabels, setStructureLabels] = useState<readonly string[] | null>(null);
  const [matches, setMatches] = useState<ChartLensMatches>(EMPTY_MATCHES);

  // A new token is a new set of candles — an address or structure carried over
  // from the last one would wash bars that have nothing to do with the pick.
  const [seenKey, setSeenKey] = useState(resetKey);
  if (seenKey !== resetKey) {
    setSeenKey(resetKey);
    setWallet(null);
    setStructureLabels(null);
    setMatches(EMPTY_MATCHES);
  }

  const structureKey = useMemo(
    () => (structureLabels && structureLabels.length > 0 ? patternKey(structureLabels) : null),
    [structureLabels],
  );

  const toggleWallet = useCallback((address: string | null) => {
    const next = address?.trim() || null;
    setWallet((cur) => (next != null && cur === next ? null : next));
  }, []);

  const toggleStructure = useCallback((labels: readonly string[] | null) => {
    const next = labels && labels.length > 0 ? labels : null;
    setStructureLabels((cur) => {
      if (next == null) return null;
      return cur && patternKey(cur) === patternKey(next) ? null : next;
    });
  }, []);

  const clear = useCallback(() => {
    setWallet(null);
    setStructureLabels(null);
  }, []);

  const onLensMatch = useCallback((next: ChartLensMatches) => setMatches(next), []);

  const lens = useMemo<ChartHighlightLens>(
    () => ({ wallet, structureKey }),
    [wallet, structureKey],
  );

  const unlabeled = useMemo(
    () => trades.reduce((n, t) => (t.instruction_labels?.length ? n : n + 1), 0),
    [trades],
  );

  const isWalletMatch = useCallback(
    (t: TradeRecord) => wallet != null && t.wallet_address === wallet,
    [wallet],
  );
  const isStructureMatch = useCallback(
    (t: TradeRecord) =>
      structureKey != null &&
      !!t.instruction_labels?.length &&
      patternKey(t.instruction_labels) === structureKey,
    [structureKey],
  );

  return {
    lens,
    active: wallet != null || structureKey != null,
    toggleWallet,
    toggleStructure,
    clear,
    structureLabels,
    matches: wallet == null && structureKey == null ? EMPTY_MATCHES : matches,
    onLensMatch,
    unlabeled,
    isWalletMatch,
    isStructureMatch,
  };
}

/** Convenience re-export so a host doesn't deep-import the chart folder for a type. */
export type { LensMatch };
