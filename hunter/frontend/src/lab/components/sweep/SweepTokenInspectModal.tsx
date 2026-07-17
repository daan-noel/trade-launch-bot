import { useMemo } from 'react';
import { Modal } from 'components/ui/Modal';
import { buildEventMarkers, type InspectTarget } from 'components/strategy/inspectTarget';
import { apiErrorMessage, useGetTokenDetailQuery } from 'store/apiSlice';
import { LabTokenInspect } from '@lab/components/strategy/LabTokenInspect';

/**
 * Sweep combo token inspect — chart with sweep entry/exit markers + metric panes
 * (same lab inspect stack as Tokens detail).
 */
export function SweepTokenInspectModal({
  target,
  onClose,
}: {
  target: InspectTarget;
  onClose: () => void;
}) {
  const {
    data: detail,
    isFetching,
    error,
  } = useGetTokenDetailQuery(target.mint_address, { skip: !target.mint_address });

  const extraEventMarkers = useMemo(() => buildEventMarkers(target), [target]);
  const heading = target.symbol || target.mint_address.slice(0, 8);

  return (
    <Modal title={`${heading} — Sweep inspect`} open onClose={onClose} size="xl">
      <LabTokenInspect
        detail={detail ?? null}
        loading={isFetching}
        error={apiErrorMessage(error, 'Failed to load detail')}
        tableId="sweep_combo_inspect"
        extraEventMarkers={extraEventMarkers}
      />
    </Modal>
  );
}
