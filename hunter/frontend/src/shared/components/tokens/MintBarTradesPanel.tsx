import { useMemo } from 'react';
import { tradesInBar, tradesInRange } from 'components/token-price-chart/barTrades';
import { BarTradesPanel } from 'components/tokens/BarTradesPanel';
import type { TokenHighlight } from 'components/tokens/useTokenHighlight';
import { useFlowReasons } from 'hooks/useFlowReasons';
import type { IxPatternTarget } from 'hooks/useIxPatternTarget';
import { useProfileWallets } from 'hooks/useProfileWallets';
import { classifyOptsForTape } from 'lib/flow/tapeClassify';
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
  flowFingerprintId = null,
  flowReadOnly = false,
  highlight = null,
  patternTarget = null,
  className,
}: {
  mint: string;
  selection: BarTradesSelection;
  tableId?: string;
  eventMarkers?: ChartEventMarker[] | null;
  flowPatternKeys?: ReadonlySet<string> | null;
  /** Fingerprint the keys came from — the Tagged badge's write target
   *  (see {@link BarTradesPanel}). */
  flowFingerprintId?: string | null;
  /** A stored run's frozen patterns — display only (see {@link BarTradesPanel}). */
  flowReadOnly?: boolean;
  highlight?: TokenHighlight | null;
  /** Host-owned tape so the overlay above classifies the same list. */
  patternTarget?: IxPatternTarget | null;
  className?: string;
}) {
  // Fetch whenever the mint is on screen — highlight chips stay mounted after
  // the bar is cleared, and contagion needs the full history.
  const skip = !mint;
  const { data } = useGetTokenTradesQuery(mint, { skip });
  const { data: detail } = useGetTokenDetailQuery(mint, { skip });
  const trades = data ?? EMPTY_TRADES;

  const myWalletAddresses = useProfileWallets();
  const mine = useMemo(
    () => new Set(myWalletAddresses.filter((w) => w.isMine).map((w) => w.address)),
    [myWalletAddresses],
  );

  const classifyOpts = useMemo(
    () =>
      classifyOptsForTape({
        list: patternTarget?.list ?? 'tagged',
        keys: patternTarget?.keys ?? flowPatternKeys,
        rows:
          patternTarget && patternTarget.list !== 'working' ? patternTarget.rows : null,
        creatorWallet: detail?.creator_wallet,
      }),
    [patternTarget, flowPatternKeys, detail?.creator_wallet],
  );
  const flowReasons = useFlowReasons(trades, classifyOpts);

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
      flowFingerprintId={flowFingerprintId}
      flowReadOnly={flowReadOnly}
      patternTarget={patternTarget}
      flowReasons={flowReasons}
      highlight={highlight}
      className={className}
    />
  );
}
