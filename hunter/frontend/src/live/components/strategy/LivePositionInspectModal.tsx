import { Modal } from 'components/ui/Modal';
import { inspectFromPosition } from 'components/strategy/inspectTarget';
import { FloorPositionDetailWithFills } from '@live/components/floor/FloorPositionDetailWithFills';
import { useResolvedFlowPatternKeys } from 'hooks/useFlowPatternKeys';
import { resolvePnlPct } from 'lib/pnlPct';
import type { StrategyRule } from 'lib/strategy/types';
import { formatDurationShort } from 'utils/format';
import type { RulePositionRecord } from 'types';

const STATUS_LABEL: Record<string, string> = {
  BuySubmitted: 'Buy submitted',
  Holding: 'Holding',
  ExitPending: 'Exit pending',
  ExitUnconfirmed: 'Exit unconfirmed',
  ExitStuck: 'Exit stuck',
  End: 'End',
  EntryFailed: 'Entry failed',
};

function holdLabel(r: RulePositionRecord): string | null {
  if (!r.entry_time) return null;
  const start = Date.parse(r.entry_time);
  const end = r.exit_time ? Date.parse(r.exit_time) : Date.now();
  if (!Number.isFinite(start) || !Number.isFinite(end) || end < start) return null;
  return formatDurationShort((end - start) / 1000);
}

/**
 * Live Rules Evidence inspect — chart + fills ledger (no metric panes; the live
 * bin has no `metric-series` route). Same body as Console History's position modal.
 */
export function LivePositionInspectModal({
  position,
  rule,
  onClose,
}: {
  position: RulePositionRecord;
  rule: StrategyRule | null;
  onClose: () => void;
}) {
  const flowPatternKeys = useResolvedFlowPatternKeys({
    fingerprintId: rule?.fingerprint_id,
    ruleId: position.rule_id ?? rule?.id,
  });

  const heading = position.symbol || `${position.mint_address.slice(0, 8)}…`;
  return (
    <Modal title={`${heading} — position`} open onClose={onClose} size="xxl">
      <FloorPositionDetailWithFills
        positionId={position.id}
        chartHeight={420}
        facts={{
          mint: position.mint_address,
          ruleId: position.rule_id ?? rule?.id ?? null,
          ruleName: rule?.rule_name ?? null,
          mode: position.mode ?? rule?.trade_mode ?? null,
          status: STATUS_LABEL[position.status] ?? position.status,
          entrySol: position.entry_sol ?? null,
          entryPrice: position.entry_price,
          exitPrice: position.exit_price,
          holdLabel: holdLabel(position),
          pnlSol: position.pnl_sol,
          pnlPct: resolvePnlPct({
            pnlSol: position.pnl_sol,
            entrySol: position.entry_sol,
            entryPrice: position.entry_price,
            exitPrice: position.exit_price,
          }),
          inspect: inspectFromPosition(position),
          flowPatternKeys,
        }}
      />
    </Modal>
  );
}
