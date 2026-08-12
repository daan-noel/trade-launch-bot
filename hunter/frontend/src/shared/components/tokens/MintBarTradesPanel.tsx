import { useMemo } from 'react';
import { tradesInBar, tradesInRange } from 'components/token-price-chart/barTrades';
import { BarTradesPanel } from 'components/tokens/BarTradesPanel';
import { useFlowReasons } from 'hooks/useFlowReasons';
import { useProfileWallets } from 'hooks/useProfileWallets';
import { useGetTokenDetailQuery, useGetTokenTradesQuery } from 'store/apiSlice';
import type { ChartEventMarker } from 'components/token-price-chart/types';
import type { TradeRecord } from 'types';
import type { BarTradesSelection } from './useBarTradesSelection';

const EMPTY_TRADES: TradeRecord[] = [];

/**
 * {@link BarTradesPanel} for a chart that renders one mint: reads that mint's
 * trades from the shared RTK Query cache — the same key the chart itself loads,
 * so listing a bar costs no extra request — and narrows them to the selection.
 *
 * Deep-imports the bucket matcher rather than the `components/token-price-chart`
 * barrel: a host that mounts this statically must not pull `lightweight-charts`
 * into its chunk (the chart itself stays behind `LazyFloorMintChart`).
 */
export function MintBarTradesPanel({
  mint,
  selection,
  tableId,
  eventMarkers = null,
  flowPatternKeys = null,
  className,
}: {
  mint: string;
  selection: BarTradesSelection;
  tableId?: string;
  eventMarkers?: ChartEventMarker[] | null;
  flowPatternKeys?: ReadonlySet<string> | null;
  className?: string;
}) {
  // Only subscribes once something is picked — an unopened detail row pays
  // nothing, and by then the chart has already filled these cache entries. The
  // detail is read for the creator wallet alone: it seeds flow contagion, so
  // without it the badge's verdict would drift from the lines above it.
  const skip = !mint || !selection.active;
  const { data } = useGetTokenTradesQuery(mint, { skip });
  const { data: detail } = useGetTokenDetailQuery(mint, { skip });
  const trades = data ?? EMPTY_TRADES;

  const myWalletAddresses = useProfileWallets();
  const mine = useMemo(
    () => new Set(myWalletAddresses.filter((w) => w.isMine).map((w) => w.address)),
    [myWalletAddresses],
  );

  const flowReasons = useFlowReasons(trades, flowPatternKeys, detail?.creator_wallet);

  const rows = useMemo(() => {
    if (selection.range) return tradesInRange(trades, selection.range);
    if (selection.bar) return tradesInBar(trades, selection.bar);
    return EMPTY_TRADES;
  }, [trades, selection.bar, selection.range]);

  return (
    <BarTradesPanel
      trades={rows}
      bar={selection.bar}
      range={selection.range}
      onClear={selection.clear}
      tableId={tableId}
      eventMarkers={eventMarkers}
      myWalletAddresses={mine}
      flowPatternKeys={flowPatternKeys}
      flowReasons={flowReasons}
      className={className}
    />
  );
}
