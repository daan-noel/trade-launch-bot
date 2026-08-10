import { useMemo } from 'react';
import { Modal } from 'components/ui/Modal';
import { inspectFromPosition } from 'components/strategy/inspectTarget';
import { FloorPositionDetailWithFills } from '@live/components/floor/FloorPositionDetailWithFills';
import { LivePositionConditions } from '@live/components/floor/LiveRuleConditions';
import {
  OPEN_STATUS_LABEL,
  PositionModalTitle,
} from '@live/components/floor/openPositionStatus';
import { useResolvedFlowPatternKeys } from 'hooks/useFlowPatternKeys';
import { holdLabel } from 'lib/holdLabel';
import { resolvePnlPct } from 'lib/pnlPct';
import type { StrategyRule } from 'lib/strategy/types';
import type { RulePositionRecord } from 'types';

/**
 * The modal for one **closed / persisted** position (a `RulePositionRecord`):
 * chart + fills ledger + the rule's conditions reconstructed at the exit fill.
 *
 * No metric panes: the full registry over a scrubbable series stays the lab's job.
 * What a post-mortem needs is narrower — which of *this rule's* conditions were true
 * when it closed — and that is one fold to one instant, which the deploy box can
 * afford. The strip labels itself as reconstructed, because it is.
 *
 * Rules Evidence opens it with the owning `rule`; Console History opens it with
 * `rule={null}` and just the resolved name, and the flow-overlay keys fall back to
 * the position's `rule_id`.
 *
 * This is the ONE closed-position modal. A second copy for History — same title,
 * same `RulePositionRecord` → `FloorDetailFacts` mapping — means every new
 * persisted column has to be wired in two places.
 */
export function LivePositionInspectModal({
  position,
  rule,
  ruleName,
  onClose,
}: {
  position: RulePositionRecord;
  rule: StrategyRule | null;
  /** Resolved rule name when the caller has the name but not the rule. */
  ruleName?: string | null;
  onClose: () => void;
}) {
  const flowPatternKeys = useResolvedFlowPatternKeys({
    fingerprintId: rule?.fingerprint_id,
    ruleId: position.rule_id ?? rule?.id,
  });

  // Identity matters, not just the value: `FloorPositionDetail` memoizes the chart
  // markers on this object, and a fresh one per render rebuilds the marker set and
  // re-creates the series-markers plugin on every status tick.
  const inspect = useMemo(() => inspectFromPosition(position), [position]);

  const pnlPct = resolvePnlPct({
    pnlSol: position.pnl_sol,
    entrySol: position.entry_sol,
    entryPrice: position.entry_price,
    exitPrice: position.exit_price,
  });

  return (
    <Modal
      title={
        <PositionModalTitle
          mint={position.mint_address}
          symbol={position.symbol}
          status={position.status}
          exitReason={position.exit_reason}
          entryError={position.last_entry_error}
          pnlSol={position.pnl_sol}
          pnlPct={pnlPct}
        />
      }
      open
      onClose={onClose}
      size="xxl"
    >
      <FloorPositionDetailWithFills
        positionId={position.id}
        chartHeight={360}
        facts={{
          mint: position.mint_address,
          symbol: position.symbol,
          ruleId: position.rule_id ?? rule?.id ?? null,
          ruleName: rule?.rule_name ?? ruleName ?? null,
          mode: position.mode ?? rule?.trade_mode ?? null,
          status: OPEN_STATUS_LABEL[position.status] ?? position.status,
          statusKey: position.status,
          entrySol: position.entry_sol ?? null,
          entryPrice: position.entry_price,
          entryTokenAmount: position.entry_token_amount,
          exitPrice: position.exit_price,
          exitSol: position.exit_sol_total ?? null,
          exitTokenAmount: position.exit_token_amount,
          exitReason: position.exit_reason,
          holdLabel: holdLabel(position.entry_time, position.exit_time),
          pnlSol: position.pnl_sol,
          pnlPct,
          inspect,
          flowPatternKeys,
          conditions: <LivePositionConditions positionId={position.id} />,
        }}
      />
    </Modal>
  );
}
