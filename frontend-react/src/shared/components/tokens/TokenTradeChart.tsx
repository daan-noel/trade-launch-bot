import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  CHART_COLORS,
  TokenPriceChart,
  tradeBarSlot,
  tradeBarTime,
  type ChartBarSelection,
  type ChartChainHighlight,
  type ChartEventMarker,
  type ChartMetric,
  type ChartRangeSelectionDetail,
  type ChartSwingLeg,
  type ChartSwingOverlay,
} from 'components/token-price-chart';
import { DataTable } from 'components/table/DataTable';
import { tokenTradeColumns } from 'components/tokens/tokenTradeColumns';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { Badge } from 'components/ui/Badge';
import { usePriceUnit } from 'context/PriceUnitContext';
import { useTimezone } from 'context/TimezoneContext';
import { usePriceDisplay } from 'hooks/usePriceDisplay';
import { useProfileWallets } from 'hooks/useProfileWallets';
import { formatTimestampMs } from 'utils/date';
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
  /** Swing-detection legs to draw as an overlay line (swing1/swing-detection inspection). */
  swingOverlay?: ChartSwingOverlay | null;
  /** Longest swing chain to highlight as a translucent band (batch swing detection). */
  highlightChain?: ChartChainHighlight | null;
  /** Selected swing leg key to highlight on the overlay (swing-detection page). */
  selectedSwingLegKey?: string | null;
  /** Fired when a swing path segment is clicked on the chart. */
  onSwingLegClick?: (leg: ChartSwingLeg | null) => void;
  /** Chain swing reversal points (swing-detection page toggle). */
  connectSwings?: boolean;
  onConnectSwingsChange?: (connected: boolean) => void;
  /** See {@link TokenTradeChartExternalSelection}. */
  externalSelection?: TokenTradeChartExternalSelection | null;
  /** Passed to DataTable so column visibility is persisted per call-site. */
  tableId?: string;
  /** Wallet to spotlight (Trader Analysis): its markers render larger with a
   *  gold glow/ring, standing out among the other tracked wallets. If it isn't
   *  a saved profile wallet, a synthetic marker entry is injected so it shows. */
  highlightWallet?: string | null;
}

/** Maps tx signature → kind for entry/exit row highlighting in the trades table.
 *  Every inspect source carries the real fill signature: TPSL sim/position results
 *  read it off the position, and the grouped-sweep drill-in resolves it from the
 *  `trades` table by (mint, slot, side) — the slim `SweepTrade` the sweep walks has
 *  none, so the backend looks it up before returning the row. */
function buildEntryExitMap(markers: ChartEventMarker[] | null | undefined): Map<string, 'entry' | 'exit'> {
  const m = new Map<string, 'entry' | 'exit'>();
  if (!markers) return m;
  for (const marker of markers) {
    if (marker.txSignature) m.set(marker.txSignature, marker.kind);
  }
  return m;
}

/** Trades within the clicked bar, matched the same way the chart buckets them. */
function tradesInBar(trades: TradeRecord[], bar: ChartBarSelection): TradeRecord[] {
  if (bar.groupMode === 'slot') {
    return trades.filter((t) => t.slot === bar.slot);
  }
  const intervalSec = bar.intervalSec ?? 60;
  return trades.filter((t) => tradeBarTime(t.block_time, intervalSec) === bar.barTime);
}

/** Trades whose bar key falls inside the drag-selected range [lo, hi]. */
function tradesInRange(
  trades: TradeRecord[],
  range: ChartRangeSelectionDetail,
): TradeRecord[] {
  const lo = Math.min(range.lo, range.hi);
  const hi = Math.max(range.lo, range.hi);
  return trades.filter((t) => {
    const key =
      range.groupMode === 'slot' ? tradeBarSlot(t) : tradeBarTime(t.block_time, range.intervalSec);
    if (key == null) return false;
    const k = key as number;
    return k >= lo && k <= hi;
  });
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
  swingOverlay = null,
  highlightChain = null,
  selectedSwingLegKey = null,
  onSwingLegClick,
  connectSwings,
  onConnectSwingsChange,
  externalSelection = null,
  tableId,
  highlightWallet = null,
}: TokenTradeChartProps) {
  const { unit, usdRate } = usePriceUnit();
  const { timezone } = useTimezone();
  const price = usePriceDisplay();
  const [metric, setMetric] = useState<ChartMetric>('price');
  // Candle click and range drag are mutually exclusive selections: setting one
  // clears the other so only a single trades table is shown at a time.
  const [selectedBar, setSelectedBar] = useState<ChartBarSelection | null>(null);
  const [selectedRange, setSelectedRange] = useState<ChartRangeSelectionDetail | null>(null);

  const mint = detail?.mint_address ?? '';
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
  useEffect(() => {
    if (externalSelection) {
      setSelectedBar(null);
      setSelectedRange(null);
    }
  }, [externalSelection?.key]);

  const handleBarClick = useCallback((selection: ChartBarSelection | null) => {
    setSelectedBar(selection);
    if (selection) {
      setSelectedRange(null);
      externalSelection?.onClear();
    }
  }, [externalSelection]);

  const handleRangeChange = useCallback((range: ChartRangeSelectionDetail | null) => {
    setSelectedRange(range);
    if (range) {
      setSelectedBar(null);
      externalSelection?.onClear();
    }
  }, [externalSelection]);

  const clearSelection = useCallback(() => {
    setSelectedBar(null);
    setSelectedRange(null);
    externalSelection?.onClear();
  }, [externalSelection]);

  const tradeColumns = useMemo(() => tokenTradeColumns(price.unitLabel), [price.unitLabel]);

  const entryExitMap = useMemo(() => buildEntryExitMap(eventMarkers), [eventMarkers]);

  const myWalletAddresses = useMemo(
    () => new Set(profileWallets.filter((w) => w.isMine).map((w) => w.address)),
    [profileWallets],
  );

  const tradeRowClassName = useMemo(() => {
    if (entryExitMap.size === 0 && myWalletAddresses.size === 0) return undefined;
    return (t: TradeRecord) => {
      const kind = entryExitMap.get(t.tx_signature);
      // Entry/exit tint takes the full-row background; "my trade" layers a
      // left-border accent so both signals stay visible on the same row.
      const base =
        kind === 'entry'
          ? 'bg-[#02c076]/12 hover:bg-[#02c076]/20'
          : kind === 'exit'
            ? 'bg-[#f6465d]/12 hover:bg-[#f6465d]/20'
            : '';
      const mine =
        t.wallet_address && myWalletAddresses.has(t.wallet_address)
          ? 'border-l-2 border-l-[#fbbf24]'
          : '';
      return [base, mine].filter(Boolean).join(' ') || undefined;
    };
  }, [entryExitMap, myWalletAddresses]);

  const selectionTrades = useMemo(() => {
    if (externalSelection) return externalSelection.trades;
    if (selectedRange) return tradesInRange(trades, selectedRange);
    if (selectedBar) return tradesInBar(trades, selectedBar);
    return EMPTY_TRADES;
  }, [trades, selectedBar, selectedRange, externalSelection]);

  if (!detail) return null;

  const tradesError = apiErrorMessage(tradesErrorRaw, 'Failed to load trades');
  const symbol = detail.symbol || detail.name || mint;
  const priceLabel = metric === 'mc' ? `MC (${unit})` : unit;

  const selectionLabel = selectedRange
    ? selectedRange.groupMode === 'slot'
      ? `Slot ${Math.min(selectedRange.lo, selectedRange.hi)} → ${Math.max(selectedRange.lo, selectedRange.hi)}`
      : `${formatTimestampMs(Math.min(selectedRange.lo, selectedRange.hi) * 1000, timezone)} → ${formatTimestampMs(Math.max(selectedRange.lo, selectedRange.hi) * 1000, timezone)}`
    : selectedBar
      ? selectedBar.groupMode === 'slot'
        ? `Slot ${selectedBar.slot}`
        : formatTimestampMs(Number(selectedBar.barTime) * 1000, timezone)
      : '';

  const showSelectionPanel = Boolean(externalSelection || selectedBar || selectedRange);
  const selectionPanelLabel = externalSelection
    ? externalSelection.label
    : selectedRange
      ? 'Range Trades'
      : 'Bar Trades';
  const selectionTimeLabel = externalSelection ? externalSelection.timeLabel : selectionLabel;
  const selectionEmptyMessage = externalSelection
    ? externalSelection.emptyMessage
    : selectedRange
      ? 'No trades in this range.'
      : 'No trades in this bar.';

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
        selectedBar={selectedBar}
        onBarClick={handleBarClick}
        onRangeChange={handleRangeChange}
        athPriceInSol={detail.ath_price ?? null}
        isMigrated={detail.is_migrated}
        isMayhemMode={detail.is_mayhem_mode}
        isCashbackEnabled={detail.is_cashback_enabled}
        tokenCreatedAt={detail.created_at}
        eventMarkers={eventMarkers}
        swingOverlay={swingOverlay}
        highlightChain={highlightChain}
        selectedSwingLegKey={selectedSwingLegKey}
        onSwingLegClick={onSwingLegClick}
        connectSwings={connectSwings}
        onConnectSwingsChange={onConnectSwingsChange}
        profileWallets={profileWallets}
      />

      {showSelectionPanel && (
        <div className="mt-3 border-t border-white/7 pt-2">
          <div className="mb-2 flex flex-wrap items-center gap-2">
            <span className="text-[9px] font-bold uppercase tracking-widest text-text-dim">
              {selectionPanelLabel}
            </span>
            <span className="font-mono text-[11px] text-text-dim">{selectionTimeLabel}</span>
            <Badge variant="primary" className="font-mono font-normal">
              {selectionTrades.length} trade{selectionTrades.length === 1 ? '' : 's'}
            </Badge>
            <button
              type="button"
              onClick={clearSelection}
              className="text-[11px] text-text-dim hover:text-text"
            >
              Clear
            </button>
          </div>
          <DataTable
            tableId={tableId}
            columns={tradeColumns}
            rows={selectionTrades}
            rowKey={(t) => t.id}
            defaultPageSize={25}
            searchable
            colFilters
            hoverable
            rowClassName={tradeRowClassName}
            emptyMessage={selectionEmptyMessage}
          />
        </div>
      )}
    </div>
  );
}
