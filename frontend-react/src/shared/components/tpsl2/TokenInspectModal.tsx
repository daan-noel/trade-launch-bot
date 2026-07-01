import { useMemo } from 'react';
import { Modal } from 'components/ui/Modal';
import { TokenDetailPanel } from 'components/tokens/TokenDetailPanel';
import { TokenTradeChart } from 'components/tokens/TokenTradeChart';
import type { ChartEventMarker, ChartSwingOverlay } from 'components/token-price-chart';
import { apiErrorMessage, useGetTokenDetailQuery } from 'store/apiSlice';

/** A token to inspect from a TPSL result table, with its strategy entry/exit
 *  fill so the chart can mark them. Built from a sim/paper result row or a
 *  rule position via the adapters in TpslPage. */
export interface InspectTarget {
  mint: string;
  symbol?: string | null;
  entryTime: string | null;
  /** null for an armed position whose entry hasn't filled yet. */
  entryPrice: number | null;
  entryTx?: string | null;
  exitTime: string | null;
  exitPrice: number | null;
  exitTx?: string | null;
  /** Exit reason / position status (e.g. "TakeProfit"); appended to the exit label. */
  exitLabel?: string | null;
}

function buildEventMarkers(target: InspectTarget): ChartEventMarker[] {
  const markers: ChartEventMarker[] = [];
  if (target.entryTime != null && target.entryPrice != null) {
    markers.push({
      kind: 'entry',
      time: target.entryTime,
      priceInSol: target.entryPrice,
      txSignature: target.entryTx ?? null,
      label: 'Entry',
    });
  }
  if (target.exitTime != null && target.exitPrice != null) {
    markers.push({
      kind: 'exit',
      time: target.exitTime,
      priceInSol: target.exitPrice,
      txSignature: target.exitTx ?? null,
      label: target.exitLabel ? `Exit · ${target.exitLabel}` : 'Exit',
    });
  }
  return markers;
}

interface TokenInspectModalProps {
  target: InspectTarget;
  /** Swing-detection legs to draw on the chart (swing1 sweep inspection). The
   *  caller owns the fetch + leg→overlay shaping so this shared modal stays
   *  mode-agnostic; absent for tpsl flows, which show no swing overlay. */
  swingOverlay?: ChartSwingOverlay | null;
  onClose: () => void;
}

/** Modal showing a token's detail panel and trade-history chart, with the
 *  strategy's entry/exit points marked on the chart. Opened by selecting a row
 *  in a TPSL paper/simulation/position result table. */
export function TokenInspectModal({ target, swingOverlay = null, onClose }: TokenInspectModalProps) {
  const {
    data: detail,
    isFetching,
    error,
  } = useGetTokenDetailQuery(target.mint, { skip: !target.mint });

  const eventMarkers = useMemo(() => buildEventMarkers(target), [target]);

  const heading = target.symbol || target.mint.slice(0, 8);

  return (
    <Modal title={`${heading} — Trade History`} open onClose={onClose} size="xl">
      <div className="flex flex-col gap-2.5">
        <TokenDetailPanel
          detail={detail ?? null}
          loading={isFetching}
          error={apiErrorMessage(error, 'Failed to load detail')}
        />
        <TokenTradeChart
          tableId="tpsl2_inspect_trades"
          detail={detail ?? null}
          eventMarkers={eventMarkers}
          swingOverlay={swingOverlay}
        />
      </div>
    </Modal>
  );
}
