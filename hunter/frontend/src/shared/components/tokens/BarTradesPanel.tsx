import { useMemo } from 'react';
import { DataTable } from 'components/table/DataTable';
import { tokenTradeColumns } from 'components/tokens/tokenTradeColumns';
import { Badge } from 'components/ui/Badge';
import { useTimezone } from 'context/TimezoneContext';
import { usePriceDisplay } from 'hooks/usePriceDisplay';
import { formatTimestampMs } from 'utils/date';
import type {
  ChartBarSelection,
  ChartEventMarker,
  ChartRangeSelectionDetail,
} from 'components/token-price-chart/types';
import type { TradeRecord } from 'types';

const EMPTY_TRADES: TradeRecord[] = [];
const tradeRowKey = (t: TradeRecord) => t.id;

/** A selection that lives outside the chart's own bar/range click (e.g. a swing
 *  leg picked in a separate results table) but drives this panel the same way.
 *  Takes over the heading/empty text; the host is responsible for handing the
 *  chart's own selection back (see `useBarTradesSelection`'s `onPick`). */
export interface BarTradesExternalSelection {
  /** Panel heading, e.g. "Swing trades". */
  label: string;
  /** Rendered next to the heading, e.g. a formatted time range. */
  timeLabel: string;
  emptyMessage: string;
}

export interface BarTradesPanelProps {
  /** Rows to list — already narrowed to the selection by the host. */
  trades: TradeRecord[];
  bar: ChartBarSelection | null;
  range: ChartRangeSelectionDetail | null;
  /** When set, overrides the labels and renders even with no chart selection. */
  external?: BarTradesExternalSelection | null;
  onClear: () => void;
  /** Passed to DataTable so column visibility persists per call-site. */
  tableId?: string;
  /** Entry/exit markers — tints the matching fill rows. */
  eventMarkers?: ChartEventMarker[] | null;
  /** Our own wallets — adds a left-border accent to their rows. */
  myWalletAddresses?: ReadonlySet<string> | null;
  /** Fingerprint `volume_ix_patterns` keys — adds the Vol/Non-vol column. */
  flowPatternKeys?: ReadonlySet<string> | null;
  /** Outer spacing — override in a host that already spaces its children (e.g.
   *  a `flex flex-col gap-*`, where the default top margin double-spaces). */
  className?: string;
}

/** Maps tx signature → kind for entry/exit row highlighting. Every inspect source
 *  carries the real fill signature: position/sim results read it off the position,
 *  and the grouped-sweep drill-in resolves it from `trades` by (mint, slot, side). */
function buildEntryExitMap(
  markers: ChartEventMarker[] | null | undefined,
): Map<string, 'entry' | 'exit'> {
  const m = new Map<string, 'entry' | 'exit'>();
  if (!markers) return m;
  for (const marker of markers) {
    if (marker.txSignature) m.set(marker.txSignature, marker.kind);
  }
  return m;
}

/**
 * The trades table under a price chart: what a clicked candle / dragged range is
 * actually made of. The ONE panel for that — Token detail, Console/Portfolio
 * position detail and the flow preview all render this, so a column or a row
 * highlight added here shows up on every chart instead of one of them.
 *
 * Renders nothing when nothing is selected, so a host can mount it
 * unconditionally.
 */
export function BarTradesPanel({
  trades,
  bar,
  range,
  external = null,
  onClear,
  tableId,
  eventMarkers = null,
  myWalletAddresses = null,
  flowPatternKeys = null,
  className = 'mt-3 border-t border-white/7 pt-2',
}: BarTradesPanelProps) {
  const { timezone } = useTimezone();
  const price = usePriceDisplay();

  const columns = useMemo(
    () => tokenTradeColumns(price.unitLabel, { flowPatternKeys }),
    [price.unitLabel, flowPatternKeys],
  );

  const entryExitMap = useMemo(() => buildEntryExitMap(eventMarkers), [eventMarkers]);

  const rowClassName = useMemo(() => {
    const mineCount = myWalletAddresses?.size ?? 0;
    if (entryExitMap.size === 0 && mineCount === 0) return undefined;
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
        t.wallet_address && myWalletAddresses?.has(t.wallet_address)
          ? 'border-l-2 border-l-[#fbbf24]'
          : '';
      return [base, mine].filter(Boolean).join(' ') || undefined;
    };
  }, [entryExitMap, myWalletAddresses]);

  if (!external && !bar && !range) return null;

  const chartTimeLabel = range
    ? range.groupMode === 'slot'
      ? `Slot ${Math.min(range.lo, range.hi)} → ${Math.max(range.lo, range.hi)}`
      : `${formatTimestampMs(Math.min(range.lo, range.hi) * 1000, timezone)} → ${formatTimestampMs(Math.max(range.lo, range.hi) * 1000, timezone)}`
    : bar
      ? bar.groupMode === 'slot'
        ? `Slot ${bar.slot}`
        : formatTimestampMs(Number(bar.barTime) * 1000, timezone)
      : '';

  const label = external ? external.label : range ? 'Range Trades' : 'Bar Trades';
  const timeLabel = external ? external.timeLabel : chartTimeLabel;
  const emptyMessage = external
    ? external.emptyMessage
    : range
      ? 'No trades in this range.'
      : 'No trades in this bar.';
  const rows = trades.length > 0 ? trades : EMPTY_TRADES;

  return (
    <div className={className}>
      <div className="mb-2 flex flex-wrap items-center gap-2">
        <span className="text-[9px] font-bold uppercase tracking-widest text-text-dim">
          {label}
        </span>
        <span className="font-mono text-[11px] text-text-dim">{timeLabel}</span>
        <Badge variant="primary" className="font-mono font-normal">
          {rows.length} trade{rows.length === 1 ? '' : 's'}
        </Badge>
        <button
          type="button"
          onClick={onClear}
          className="text-[11px] text-text-dim hover:text-text"
        >
          Clear
        </button>
      </div>
      <DataTable
        tableId={tableId}
        columns={columns}
        rows={rows}
        rowKey={tradeRowKey}
        searchable
        colFilters
        hoverable
        rowClassName={rowClassName}
        emptyMessage={emptyMessage}
      />
    </div>
  );
}
