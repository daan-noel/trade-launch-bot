import { Modal } from 'components/ui/Modal';
import { Badge } from 'components/ui/Badge';
import { inspectFromPosition } from 'components/strategy/inspectTarget';
import { exitReasonBadge } from 'components/strategy/strategyColumns';
import { FloorPositionDetailWithFills } from '@live/components/floor/FloorPositionDetailWithFills';
import { useResolvedFlowPatternKeys } from 'hooks/useFlowPatternKeys';
import { resolvePnlPct } from 'lib/pnlPct';
import { formatSigned, formatSignedPct, pctGradeClass, signedToneClass } from 'lib/signedTone';
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
  const pnlPct = resolvePnlPct({
    pnlSol: position.pnl_sol,
    entrySol: position.entry_sol,
    entryPrice: position.entry_price,
    exitPrice: position.exit_price,
  });
  return (
    <Modal
      title={
        <span className="inline-flex flex-wrap items-center gap-2">
          <span>{heading}</span>
          <Badge
            variant={position.status === 'EntryFailed' ? 'danger' : 'primary'}
            size="sm"
          >
            {STATUS_LABEL[position.status] ?? position.status}
          </Badge>
          {exitReasonBadge(
            position.exit_reason,
            position.pnl_sol,
            position.last_entry_error,
            'sm',
          )}
          {pnlPct != null ? (
            <span className={`tabular-nums text-sm ${pctGradeClass(pnlPct)}`}>
              {formatSignedPct(pnlPct, 1)}
            </span>
          ) : null}
          {position.pnl_sol != null ? (
            <span
              className={`tabular-nums text-sm font-semibold ${signedToneClass(position.pnl_sol)}`}
            >
              {formatSigned(position.pnl_sol, 3)}◎
            </span>
          ) : null}
        </span>
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
          ruleName: rule?.rule_name ?? null,
          mode: position.mode ?? rule?.trade_mode ?? null,
          status: STATUS_LABEL[position.status] ?? position.status,
          statusKey: position.status,
          entrySol: position.entry_sol ?? null,
          entryPrice: position.entry_price,
          entryTokenAmount: position.entry_token_amount,
          exitPrice: position.exit_price,
          exitSol: position.exit_sol_total ?? null,
          exitTokenAmount: position.exit_token_amount,
          exitReason: position.exit_reason,
          holdLabel: holdLabel(position),
          pnlSol: position.pnl_sol,
          pnlPct,
          inspect: inspectFromPosition(position),
          flowPatternKeys,
        }}
      />
    </Modal>
  );
}
