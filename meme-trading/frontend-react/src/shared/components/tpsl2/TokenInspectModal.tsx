import { useMemo } from 'react';
import { Modal } from 'components/ui/Modal';
import { TokenDetailPanel } from 'components/tokens/TokenDetailPanel';
import { TokenTradeChart } from 'components/tokens/TokenTradeChart';
import type { ChartSwingOverlay } from 'components/token-price-chart';
import { apiErrorMessage, useGetTokenDetailQuery } from 'store/apiSlice';

// `InspectTarget` is defined once in the shared strategy module and re-exported here
// so existing `import { type InspectTarget } from '.../TokenInspectModal'` keeps working.
import { buildEventMarkers, type InspectTarget } from 'components/strategy/inspectTarget';
export type { InspectTarget };

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
  } = useGetTokenDetailQuery(target.mint_address, { skip: !target.mint_address });

  const eventMarkers = useMemo(() => buildEventMarkers(target), [target]);

  const heading = target.symbol || target.mint_address.slice(0, 8);

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
