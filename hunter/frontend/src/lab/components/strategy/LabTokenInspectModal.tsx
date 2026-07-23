import { useMemo } from 'react';
import { Modal } from 'components/ui/Modal';
import { buildEventMarkers, type InspectTarget } from 'components/strategy/inspectTarget';
import type { ChartEventMarker } from 'components/token-price-chart';
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
  eventMarkers = null,
  onClose,
}: {
  target: InspectTarget;
  /** Modal heading suffix, e.g. "Sweep inspect". */
  titleSuffix?: string;
  ruleOverride?: MetricPanesRuleOverride | null;
  /** Precomputed fill markers to draw instead of the single `target`'s — used to
   *  overlay **all re-entry episodes** of the token on one chart. Falls back to the
   *  single-target markers when null/absent. */
  eventMarkers?: ChartEventMarker[] | null;
  onClose: () => void;
}) {
  const {
    data: detail,
    isFetching,
    error,
  } = useGetTokenDetailQuery(target.mint_address, { skip: !target.mint_address });

  const singleMarkers = useMemo(() => buildEventMarkers(target), [target]);
  const extraEventMarkers = eventMarkers ?? singleMarkers;
  const heading = target.symbol || target.mint_address.slice(0, 8);

  // The inspected run's entry fill drives the position-scoped `m_position` panes.
  // Absent on a never-entered (`NoEntry`) target → the group stays hidden.
  const positionEntry = useMemo(
    () =>
      target.entryTime != null && target.entryPrice != null
        ? { time: target.entryTime, price: target.entryPrice }
        : null,
    [target.entryTime, target.entryPrice],
  );

  return (
    <Modal title={`${heading} — ${titleSuffix}`} open onClose={onClose} size="xxl">
      <LabTokenInspect
        detail={detail ?? null}
        loading={isFetching}
        error={apiErrorMessage(error, 'Failed to load detail')}
        tableId="lab_run_inspect"
        extraEventMarkers={extraEventMarkers}
        ruleOverride={ruleOverride}
        positionEntry={positionEntry}
      />
    </Modal>
  );
}
