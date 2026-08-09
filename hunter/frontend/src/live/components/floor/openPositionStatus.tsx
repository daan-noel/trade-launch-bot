import type { ReactNode } from 'react';
import { Badge, type BadgeVariant } from 'components/ui/Badge';
import { ModeBadge } from 'components/strategy/ModeBadge';
import { exitReasonBadge } from 'components/strategy/strategyColumns';
import {
  formatSigned,
  formatSignedPct,
  pctGradeClass,
  signedToneClass,
} from 'lib/signedTone';

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

/**
 * The header of a position modal — identity · status · exit · PnL, in that
 * order. ONE definition, shared by the Console's open rows, Console History and
 * Rules Evidence — hand-rolling the same span in three places drifts, most
 * visibly on the status badge color (paint every non-`EntryFailed` row `primary`
 * and a closed winner reads exactly like a stuck exit).
 */
export function PositionModalTitle({
  mint,
  symbol,
  status,
  exitReason,
  entryError,
  pnlSol,
  pnlPct,
}: {
  mint: string;
  /** Falls back to a truncated mint when the row has no symbol. */
  symbol?: string | null;
  /** Raw engine status key (not the display label). */
  status: string;
  exitReason?: string | null;
  /** `last_entry_error` — an `EntryFailed` row has no exit reason to show. */
  entryError?: string | null;
  /** MTM for an open row, realized PnL for a closed one. */
  pnlSol?: number | null;
  pnlPct?: number | null;
}): ReactNode {
  return (
    <span className="inline-flex flex-wrap items-center gap-2">
      <span className={symbol ? undefined : 'font-mono'}>
        {symbol || `${mint.slice(0, 8)}…`}
      </span>
      <Badge variant={openStatusBadgeVariant(status)} size="sm">
        {OPEN_STATUS_LABEL[status] ?? status}
      </Badge>
      {exitReason || entryError
        ? exitReasonBadge(exitReason, pnlSol, entryError ?? null, 'sm')
        : null}
      {pnlPct != null ? (
        <span className={`tabular-nums text-sm ${pctGradeClass(pnlPct)}`}>
          {formatSignedPct(pnlPct, 1)}
        </span>
      ) : null}
      {pnlSol != null ? (
        <span className={`tabular-nums text-sm font-semibold ${signedToneClass(pnlSol)}`}>
          {formatSigned(pnlSol, 3)}◎
        </span>
      ) : null}
    </span>
  );
}
