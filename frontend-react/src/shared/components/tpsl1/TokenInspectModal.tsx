import { useMemo } from 'react';
import { Modal } from 'components/ui/Modal';
import { TokenDetailPanel } from 'components/tokens/TokenDetailPanel';
import { TokenTradeChart } from 'components/tokens/TokenTradeChart';
import type { ChartEventMarker } from 'components/token-price-chart';
import { apiErrorMessage, useGetTokenDetailQuery } from 'store/apiSlice';

// `InspectTarget` is defined once in the shared strategy module and re-exported here
// so existing `import { type InspectTarget } from '.../TokenInspectModal'` keeps working.
import type { InspectTarget } from 'components/strategy/inspectTarget';
export type { InspectTarget };

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
  onClose: () => void;
}

/** Modal showing a token's detail panel and trade-history chart, with the
 *  strategy's entry/exit points marked on the chart. Opened by selecting a row
 *  in a TPSL paper/simulation/position result table. */
export function TokenInspectModal({ target, onClose }: TokenInspectModalProps) {
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
        <TokenTradeChart tableId="tpsl1_inspect_trades" detail={detail ?? null} eventMarkers={eventMarkers} />
      </div>
    </Modal>
  );
}
