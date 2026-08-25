import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  CHART_COLORS,
  TokenPriceChart,
  tradesInBar,
  tradesInRange,
  type ChartEventMarker,
  type ChartMetric,
  type ChartTimeBand,
  type ChartTimeSpan,
  type ChartValueLane,
} from 'components/token-price-chart';
import { BarTradesPanel } from 'components/tokens/BarTradesPanel';
import { useFlowLensContext } from 'context/FlowLensContext';
import { useBarTradesSelection } from 'components/tokens/useBarTradesSelection';
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
  /** Wall-clock crosshair time for sibling panes (metric series). */
  onCrosshairTimeChange?: (timeSec: number | null) => void;
  /** Drive the price-chart crosshair from metric-pane hover (unix seconds). */
  externalCrosshairTimeSec?: number | null;
  /** Visible wall-clock window for sibling panes (time-grouping mode only). */
  onVisibleTimeRangeChange?: (range: { from: number; to: number } | null) => void;
  /**
   * Fingerprint `volume_ix_patterns` keys for the vol/non-vol overlay + Vol
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
  const profileWalletsBase = useProfileWallets();
  // Spotlight the focused wallet: flag it highlighted if it's already tracked,
  // otherwise append a synthetic marker entry so an arbitrary input address
  // still shows its buys/sells. The other tracked wallets ride along unchanged.
  const profileWallets = useMemo(() => {
    const addr = highlightWallet?.trim();
    if (!addr) return profileWalletsBase;
    let matched = false;
    const flagged = profileWalletsBase.map((w) => {
      if (w.address !== addr) return w;
      matched = true;
      return { ...w, isHighlighted: true };
    });
    if (matched) return flagged;
    return [
      ...flagged,
      {
        address: addr,
        label: `${addr.slice(0, 4)}…${addr.slice(-4)}`,
        color: CHART_COLORS.highlightRing,
        isHighlighted: true,
      },
    ];
  }, [profileWalletsBase, highlightWallet]);

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
      />
    </div>
  );
}
