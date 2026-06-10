import { useCallback, useMemo, useState } from 'react';
import {
  TokenPriceChart,
  tradeBarSlot,
  tradeBarTime,
  type ChartBarSelection,
  type ChartEventMarker,
  type ChartMetric,
  type ChartRangeSelectionDetail,
} from 'components/token-price-chart';
import { DataTable } from 'components/table/DataTable';
import { tokenTradeColumns } from 'components/transactions/tokenTradeColumns';
import { Badge } from 'components/ui/Badge';
import { usePriceUnit } from 'context/PriceUnitContext';
import { useTimezone } from 'context/TimezoneContext';
import { usePriceDisplay } from 'hooks/usePriceDisplay';
import { formatTimestampMs } from 'utils/date';
import { apiErrorMessage, useGetTokenTradesQuery } from 'store/apiSlice';
import type { TokenDetailRecord, TradeRecord } from 'types';

/** Stable empty reference so the chart doesn't re-aggregate on every render. */
const EMPTY_TRADES: TradeRecord[] = [];

interface TokenTradeChartProps {
  detail: TokenDetailRecord | null;
  /** Strategy entry/exit points to overlay (TPSL result inspection). */
  eventMarkers?: ChartEventMarker[] | null;
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
export function TokenTradeChart({ detail, eventMarkers = null }: TokenTradeChartProps) {
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

  const toChartValue = useCallback(
    (sol: number) => (unit === 'USD' && usdRate != null ? sol * usdRate : sol),
    [unit, usdRate],
  );

  const handleBarClick = useCallback((selection: ChartBarSelection | null) => {
    setSelectedBar(selection);
    if (selection) setSelectedRange(null);
  }, []);

  const handleRangeChange = useCallback((range: ChartRangeSelectionDetail | null) => {
    setSelectedRange(range);
    if (range) setSelectedBar(null);
  }, []);

  const clearSelection = useCallback(() => {
    setSelectedBar(null);
    setSelectedRange(null);
  }, []);

  const tradeColumns = useMemo(() => tokenTradeColumns(price), [price]);

  const selectionTrades = useMemo(() => {
    if (selectedRange) return tradesInRange(trades, selectedRange);
    if (selectedBar) return tradesInBar(trades, selectedBar);
    return EMPTY_TRADES;
  }, [trades, selectedBar, selectedRange]);

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

  return (
    <div className="border-t border-white/7 pt-2">
      <div className="mb-1.5 text-[9px] font-bold uppercase tracking-widest text-text-dim">
        Trade History
      </div>
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
      />

      {(selectedBar || selectedRange) && (
        <div className="mt-3 border-t border-white/7 pt-2">
          <div className="mb-2 flex flex-wrap items-center gap-2">
            <span className="text-[9px] font-bold uppercase tracking-widest text-text-dim">
              {selectedRange ? 'Range Trades' : 'Bar Trades'}
            </span>
            <span className="font-mono text-[11px] text-text-dim">{selectionLabel}</span>
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
            columns={tradeColumns}
            rows={selectionTrades}
            rowKey={(t) => t.id}
            defaultPageSize={25}
            searchable
            colFilters
            hoverable
            emptyMessage={
              selectedRange ? 'No trades in this range.' : 'No trades in this bar.'
            }
          />
        </div>
      )}
    </div>
  );
}
