import { useMemo } from 'react';
import { Modal } from 'components/ui/Modal';
import { buildEventMarkers, type InspectTarget } from 'components/strategy/inspectTarget';
import { apiErrorMessage, useGetTokenDetailQuery } from 'store/apiSlice';
import { LabTokenInspect } from '@lab/components/strategy/LabTokenInspect';
import type { MetricPanesRuleOverride } from '@lab/components/strategy/MetricPanes';

/**
 * Run-result token inspect (sweep combos + simulate positions) — chart with the
 * run's **fill** entry/exit markers + metric panes. `ruleOverride` pins the panes
 * to the exact params that produced the run; pane fires land as `signal` markers
 * (`name op value`, e.g. `stall > 3`) so they stay distinct from the fill arrows.
 */
export function LabTokenInspectModal({
  target,
  titleSuffix = 'Token inspect',
  ruleOverride = null,
  onClose,
}: {
  target: InspectTarget;
  /** Modal heading suffix, e.g. "Sweep inspect". */
  titleSuffix?: string;
  ruleOverride?: MetricPanesRuleOverride | null;
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
    <Modal title={`${heading} — ${titleSuffix}`} open onClose={onClose} size="xl">
      <LabTokenInspect
        detail={detail ?? null}
        loading={isFetching}
        error={apiErrorMessage(error, 'Failed to load detail')}
        tableId="lab_run_inspect"
        extraEventMarkers={extraEventMarkers}
        ruleOverride={ruleOverride}
      />
    </Modal>
  );
}
