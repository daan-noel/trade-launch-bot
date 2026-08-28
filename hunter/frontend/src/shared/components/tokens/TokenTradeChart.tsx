import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  CHART_COLORS,
  compareWalletColor,
  TokenPriceChart,
  tradesInBar,
  tradesInRange,
  type ChartEventMarker,
  type ChartMetric,
  type ChartTimeBand,
  type ChartTimeSpan,
  type ChartValueLane,
  type ProfileWalletInfo,
} from 'components/token-price-chart';
import { BarTradesPanel } from 'components/tokens/BarTradesPanel';
import { useFlowLensContext } from 'context/FlowLensContext';
import { useBarTradesSelection } from 'components/tokens/useBarTradesSelection';
import { useTokenHighlight } from 'components/tokens/useTokenHighlight';
import { usePriceUnit } from 'context/PriceUnitContext';
import { useFlowReasons } from 'hooks/useFlowReasons';
import { useProfileWallets } from 'hooks/useProfileWallets';
import { useWatchTokenTradesLive } from 'hooks/useTokenTradesLive';
import { apiErrorMessage, useGetTokenTradesQuery } from 'store/apiSlice';
import type { TokenDetailRecord, TradeRecord } from 'types';

/** Stable empty reference so the chart doesn't re-aggregate on every render. */
const EMPTY_TRADES: TradeRecord[] = [];

/** A selection that lives outside the chart's own bar/range click (e.g. a swing
 *  leg picked in a separate results table) but should drive the trades panel
 *  below the chart the same way. Setting this clears the chart's internal
 *  bar/range selection; clicking a bar/range inside the chart calls `onClear`
 *  to hand control back. */
interface TokenTradeChartExternalSelection {
  /** Stable identity for the current selection — used to detect a new pick. */
  key: string;
  /** Panel heading, e.g. "Swing trades". */
  label: string;
  /** Rendered next to the heading, e.g. a formatted time range. */
  timeLabel: string;
  trades: TradeRecord[];
  emptyMessage: string;
  onClear: () => void;
}

interface TokenTradeChartProps {
  detail: TokenDetailRecord | null;
  /** Strategy entry/exit points to overlay (TPSL result inspection). */
  eventMarkers?: ChartEventMarker[] | null;
  /** See {@link TokenTradeChartExternalSelection}. */
  externalSelection?: TokenTradeChartExternalSelection | null;
  /** Passed to DataTable so column visibility is persisted per call-site. */
  tableId?: string;
  /** Wallet to spotlight (Trader Analysis): its markers render larger with a
   *  gold glow/ring at ~2.4x the regular marker size, standing out among the other
   *  tracked wallets. If it isn't a saved profile wallet, a synthetic marker entry
   *  is injected so it shows. Its rows in the trades panel are painted gold too. */
  highlightWallet?: string | null;
  /** Wallets under comparison against {@link highlightWallet} (Trader Analysis
   *  "Compare with"), in comparison-slot order. Slot order is what drives their
   *  colors, so a comparison wallet keeps ONE hue across every chart on the page.
   *  Their markers take the tier between the focused wallet and the crowd — a
   *  square silhouette at ~1.7x with a ring in their own color — and every other
   *  tracked wallet DIMS while this list is non-empty. An address that isn't a
   *  saved profile wallet gets a synthetic entry, same as {@link highlightWallet}. */
  compareWallets?: readonly string[] | null;
  /** Wall-clock crosshair time for sibling panes (metric series). */
  onCrosshairTimeChange?: (timeSec: number | null) => void;
  /** Drive the price-chart crosshair from metric-pane hover (unix seconds). */
  externalCrosshairTimeSec?: number | null;
  /** Visible wall-clock window for sibling panes (time-grouping mode only). */
  onVisibleTimeRangeChange?: (range: { from: number; to: number } | null) => void;
  /**
   * Fingerprint `ix_patterns` keys for the vol/non-vol overlay + Vol
   * badge. Omit/empty still draws the overlay (creator-vs-rest split); it only
   * hides the per-trade Vol badge, which is a structural match by definition.
   */
  flowPatternKeys?: ReadonlySet<string> | null;
  /** Fingerprint {@link flowPatternKeys} came from — the trades table's Vol-badge
   *  write target (see `BarTradesPanel`). Pass it wherever the host knows one. */
  flowFingerprintId?: string | null;
  /** A stored run's frozen patterns — display only (see `BarTradesPanel`). */
  flowReadOnly?: boolean;
  /** Bottom-pane on/off lanes — the inspect's rule-condition timeline. */
  timeBands?: ChartTimeBand[] | null;
  /** The stretch those lanes speak for. */
  timeBandCoverage?: ChartTimeSpan | null;
  /** One condition's reading drawn against its threshold, in its own pane. */
  valueLane?: ChartValueLane | null;
}

/**
 * Trade-history price chart for the selected token's detail panel. Pulls the
 * per-mint trades from the shared RTK Query cache (same key as the Swing
 * detection page, so a token already viewed there renders instantly) and feeds
 * them to the reusable {@link TokenPriceChart}. Clicking a candle — or
 * drag-selecting a time range — lists the underlying trades in a table below.
 */
export function TokenTradeChart({
  detail,
  eventMarkers = null,
  externalSelection = null,
  tableId,
  highlightWallet = null,
  compareWallets = null,
  onCrosshairTimeChange,
  externalCrosshairTimeSec = null,
  onVisibleTimeRangeChange,
  flowPatternKeys = null,
  flowFingerprintId = null,
  flowReadOnly = false,
  timeBands = null,
  timeBandCoverage = null,
  valueLane = null,
}: TokenTradeChartProps) {
  const { unit, usdRate } = usePriceUnit();
  const [metric, setMetric] = useState<ChartMetric>('price');
  // A pick inside the chart hands control back from any external selection.
  const selection = useBarTradesSelection(externalSelection?.onClear);

  const mint = detail?.mint_address ?? '';
  // Live append: `trade_executed` → RTK `getTokenTrades` cache (shared watch set).
  useWatchTokenTradesLive(mint || null);
  const {
    data: tradesData,
    isFetching: tradesLoading,
    error: tradesErrorRaw,
  } = useGetTokenTradesQuery(mint, { skip: !mint });
  const trades = tradesData ?? EMPTY_TRADES;
  // Ephemeral per-token highlight lenses. Keyed on the mint so switching tokens
  // can't carry a wallet or a structure onto candles it has nothing to do with.
  const highlight = useTokenHighlight(trades, mint);

  const profileWalletsBase = useProfileWallets();
  // Spotlight the focused wallet: flag it highlighted if it's already tracked,
  // otherwise append a synthetic marker entry so an arbitrary input address
  // still shows its buys/sells. The other tracked wallets ride along unchanged.
  // An armed wallet lens gets the focus treatment on the marker layer too — the
  // wash says WHEN, the oversized gold marker says which leg and which side. The
  // page's own `highlightWallet` still wins when both are set, since that one is
  // the reason the page is open.
  const spotlightWallet = highlightWallet?.trim() || highlight.lens.wallet || null;
  // Comparison list flattened to a primitive so the memo below survives a caller
  // that rebuilds the array every render.
  const compareKey = (compareWallets ?? []).join(',');
  // Three marker tiers out of one pass: the spotlight wallet, the comparison set,
  // and the rest of the tracked crowd — which DIMS while a comparison is running.
  const profileWallets = useMemo(() => {
    const addr = spotlightWallet;
    const compareSlot = new Map(
      (compareKey ? compareKey.split(',') : []).filter(Boolean).map((a, i) => [a, i] as const),
    );
    if (!addr && compareSlot.size === 0) return profileWalletsBase;
    // Dimming is only ever relative to something worth reading against: with no
    // comparison armed the crowd IS the content, so it stays at full strength.
    const comparing = compareSlot.size > 0;

    let matched = false;
    const flagged: ProfileWalletInfo[] = profileWalletsBase.map((w) => {
      const isHighlighted = w.address === addr;
      if (isHighlighted) matched = true;
      const slot = compareSlot.get(w.address);
      if (slot == null) {
        if (isHighlighted) return { ...w, isHighlighted: true };
        return comparing ? { ...w, dimmed: true } : w;
      }
      compareSlot.delete(w.address);
      return {
        ...w,
        isCompared: true,
        isHighlighted,
        // Slot-keyed hue replaces the rotating palette (see `compareWalletColor`).
        color: compareWalletColor(slot, w),
      };
    });

    // Anything still in the map has no profile entry — append it in slot order so
    // a synthetic comparison wallet keeps the color its slot owns.
    for (const [address, slot] of compareSlot) {
      flagged.push({
        address,
        label: `${address.slice(0, 4)}…${address.slice(-4)}`,
        color: compareWalletColor(slot),
        isCompared: true,
      });
    }
    if (addr && !matched) {
      flagged.push({
        address: addr,
        label: `${addr.slice(0, 4)}…${addr.slice(-4)}`,
        color: CHART_COLORS.highlightRing,
        isHighlighted: true,
      });
    }
    return flagged;
  }, [profileWalletsBase, spotlightWallet, compareKey]);

  const toChartValue = useCallback(
    (sol: number) => (unit === 'USD' && usdRate != null ? sol * usdRate : sol),
    [unit, usdRate],
  );

  // An externally-driven pick (e.g. a swing-leg row selected in another table)
  // takes over the panel below — clear this chart's own bar/range selection so
  // only one selection is shown at a time. Keyed off `.key`, not the object
  // reference, since callers rebuild the object every render.
  const clearOwnSelection = selection.clear;
  useEffect(() => {
    if (externalSelection) clearOwnSelection();
  }, [externalSelection?.key, clearOwnSelection]);

  const clearSelection = useCallback(() => {
    clearOwnSelection();
    externalSelection?.onClear();
  }, [clearOwnSelection, externalSelection]);

  const myWalletAddresses = useMemo(
    () => new Set(profileWallets.filter((w) => w.isMine).map((w) => w.address)),
    [profileWallets],
  );

  // Classified over the full history, not the selection — contagion is
  // forward-only, so a bar's rows alone can't reconstruct it.
  // Under a page-wide lens the table's reasons must be computed the SAME way the
  // overlay lines were (structural-only, exclusions) or the badge and the line
  // disagree on the same trade.
  const lens = useFlowLensContext();
  const flowReasons = useFlowReasons(trades, flowPatternKeys, detail?.creator_wallet, {
    contagion: lens?.contagion,
    excludeWallets: lens?.excludeWallets ?? null,
    side: lens?.side ?? null,
  });

  const selectionTrades = useMemo(() => {
    if (externalSelection) return externalSelection.trades;
    if (selection.range) return tradesInRange(trades, selection.range);
    if (selection.bar) return tradesInBar(trades, selection.bar);
    return EMPTY_TRADES;
  }, [trades, selection.bar, selection.range, externalSelection]);

  if (!detail) return null;

  const tradesError = apiErrorMessage(tradesErrorRaw, 'Failed to load trades');
  const symbol = detail.symbol || detail.name || mint;
  const priceLabel = metric === 'mc' ? `MC (${unit})` : unit;

  return (
    <div className="border-t border-white/7 pt-2">
      <TokenPriceChart
        symbol={symbol}
        id={mint}
        trades={trades}
        loading={tradesLoading}
        error={tradesError}
        toValue={toChartValue}
        priceLabel={priceLabel}
        priceUnit={unit}
        metric={metric}
        onMetricChange={setMetric}
        {...selection.chartProps}
        athPriceInSol={detail.ath_price ?? null}
        creatorWallet={detail.creator_wallet}
        isMigrated={detail.is_migrated}
        isMayhemMode={detail.is_mayhem_mode}
        isCashbackEnabled={detail.is_cashback_enabled}
        tokenCreatedAt={detail.created_at}
        eventMarkers={eventMarkers}
        profileWallets={profileWallets}
        onCrosshairTimeChange={onCrosshairTimeChange}
        externalCrosshairTimeSec={externalCrosshairTimeSec}
        onVisibleTimeRangeChange={onVisibleTimeRangeChange}
        flowPatternKeys={flowPatternKeys}
        highlightLens={highlight.lens}
        onHighlightLensMatch={highlight.onLensMatch}
        timeBands={timeBands}
        timeBandCoverage={timeBandCoverage}
        valueLane={valueLane}
      />

      <BarTradesPanel
        trades={selectionTrades}
        bar={selection.bar}
        range={selection.range}
        external={externalSelection}
        onClear={clearSelection}
        tableId={tableId}
        eventMarkers={eventMarkers}
        myWalletAddresses={myWalletAddresses}
        highlightWallet={highlightWallet}
        flowPatternKeys={flowPatternKeys}
        flowFingerprintId={flowFingerprintId}
        flowReadOnly={flowReadOnly}
        flowReasons={flowReasons}
        highlight={highlight}
      />
    </div>
  );
}
