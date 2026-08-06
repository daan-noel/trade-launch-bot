import type { ReactNode } from 'react';
import { Badge, type BadgeVariant } from 'components/ui/Badge';
import { ModeBadge } from 'components/strategy/ModeBadge';
import { exitReasonBadge } from 'components/strategy/strategyColumns';

/** Display labels for engine position statuses — Console + modal title SSOT. */
export const OPEN_STATUS_LABEL: Record<string, string> = {
  BuySubmitted: 'Buy submitted',
  Holding: 'Holding',
  ExitPending: 'Exit pending',
  ExitUnconfirmed: 'Exit unconfirmed',
  ExitStuck: 'Exit stuck',
  End: 'End',
  EntryFailed: 'Entry failed',
};

/** Badge color for a raw engine status key (and Waiting). */
export function openStatusBadgeVariant(statusKey: string): BadgeVariant {
  switch (statusKey) {
    case 'ExitPending':
    case 'ExitUnconfirmed':
    case 'Waiting':
      return 'warning';
    case 'Holding':
    case 'End':
      return 'success';
    case 'BuySubmitted':
      return 'info';
    case 'ExitStuck':
    case 'EntryFailed':
      return 'danger';
    default:
      return 'neutral';
  }
}

export interface OpenPositionChipFacts {
  status: string;
  origin?: string | null;
  mode?: string | null;
  /** Show real-mode badge even when the page filter is already `real`. */
  showRealMode?: boolean;
  soldBps?: number;
  scaleStage?: number;
  exitParked?: boolean;
  exitRedriveCount?: number;
  needsReview?: boolean;
  isDead?: boolean;
  exitReason?: string | null;
  /** PnL for coloring metric exit reasons. */
  pnlSol?: number | null;
}

/**
 * Status / ops chips shared by Console table rows and the position modal hero
 * so the two surfaces cannot drift.
 */
export function OpenPositionStatusChips({
  facts,
  size = 'sm',
}: {
  facts: OpenPositionChipFacts;
  size?: 'sm' | 'md';
}): ReactNode {
  const label = OPEN_STATUS_LABEL[facts.status] ?? facts.status;
  return (
    <span className="inline-flex flex-wrap items-center gap-1">
      <Badge variant={openStatusBadgeVariant(facts.status)} size={size}>
        {label}
      </Badge>
      {facts.mode === 'paper' ? <ModeBadge mode="paper" size={size} /> : null}
      {facts.mode === 'real' && facts.showRealMode ? (
        <ModeBadge mode="real" size={size} />
      ) : null}
      {facts.origin === 'manual' ? (
        <Badge variant="accent" size={size}>
          manual
        </Badge>
      ) : null}
      {facts.exitReason
        ? exitReasonBadge(facts.exitReason, facts.pnlSol, null, size)
        : null}
      {facts.soldBps != null && facts.soldBps > 0 ? (
        <Badge
          variant="accent"
          size={size}
          title={
            facts.scaleStage != null
              ? `Scale stage ${facts.scaleStage}`
              : 'Fraction of initial bag sold'
          }
        >
          {Math.round(facts.soldBps / 100)}% banked
        </Badge>
      ) : null}
      {facts.isDead ? (
        <Badge variant="danger" size={size} title="Dead pool — liquidity gone">
          ❗ dead
        </Badge>
      ) : null}
      {facts.status === 'ExitStuck' &&
        (facts.exitParked ? (
          <Badge variant="danger" size={size}>
            PARKED
          </Badge>
        ) : facts.exitRedriveCount != null ? (
          <Badge variant="warning" size={size}>
            retry {Math.min(facts.exitRedriveCount, 2)}/2
          </Badge>
        ) : null)}
      {facts.status === 'BuySubmitted' && facts.needsReview ? (
        <Badge variant="warning" size={size}>
          stale — verify
        </Badge>
      ) : null}
    </span>
  );
}
