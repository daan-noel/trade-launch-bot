import { useEffect, useState } from 'react';

import { apiErrorMessage } from 'store/apiSlice';
import {
  useGetArmedMetricsQuery,
  useGetPositionMetricsQuery,
  type ReadoutAt,
} from '@live/store/liveEndpoints';
import { RuleConditionStrip } from './RuleConditionStrip';

/**
 * How often an OPEN position's readout re-reads. The engine ticks at `TICK_MS`
 * (200 ms), so this is deliberately coarser than the decision cadence: the strip
 * answers "what is this position waiting on", which a human reads at ~1 Hz, and each
 * poll costs the serialized decision loop a command round-trip. Faster would buy
 * nothing and spend it on the one thread that must not be busy.
 */
const READOUT_POLL_MS = 1000;

/**
 * Rule conditions for one position — live while the engine holds it, reconstructed
 * from stored trades once it does not. One endpoint answers both; `source` says which
 * came back.
 *
 * Mounted only inside the position modal, so nothing runs for the Console's table
 * rows. Polling **stops as soon as a replay comes back**: a closed position's readout
 * is a fixed instant in the past, and re-folding its trade history every second would
 * be pure waste on the deploy box.
 *
 * A `404` is expected, not exceptional — a manual position with no rule, or a token
 * whose trades have aged out of the box's rolling window. The strip renders the
 * reason instead of an error.
 */
export function LivePositionConditions({ positionId }: { positionId: string }) {
  const [at, setAt] = useState<ReadoutAt>('exit');
  // Latched from the response rather than passed in: the caller (a modal that may be
  // walking a lane with ←/→) does not reliably know whether the engine still holds
  // this row, and the answer is authoritative in the payload.
  const [isReplay, setIsReplay] = useState(false);
  const { data, isLoading, error } = useGetPositionMetricsQuery(
    { positionId, at },
    { pollingInterval: isReplay ? 0 : READOUT_POLL_MS, skip: !positionId },
  );
  useEffect(() => {
    if (data?.source) setIsReplay(data.source === 'replay');
  }, [data?.source]);
  return (
    <RuleConditionStrip
      readout={data ?? null}
      loading={isLoading}
      error={readoutError(error)}
      notFound={isNotFound(error)}
      at={at}
      onAtChange={setAt}
    />
  );
}

/**
 * The same readout for an ARMED (token, rule) pair — the Waiting lane's "why has
 * this not entered yet". Exit conditions come back too, position-scoped ones with
 * no reading, which is exactly what the pre-entry `can_enter` gate sees.
 */
export function ArmedRuleConditions({
  mint,
  ruleId,
}: {
  mint: string;
  ruleId: string | null;
}) {
  const { data, isLoading, error } = useGetArmedMetricsQuery(
    { mint, ruleId: ruleId ?? '' },
    { pollingInterval: READOUT_POLL_MS, skip: !mint || !ruleId },
  );
  if (!ruleId) return null;
  return (
    <RuleConditionStrip
      readout={data ?? null}
      loading={isLoading}
      error={readoutError(error)}
      notFound={isNotFound(error)}
    />
  );
}

/** RTK Query's `FetchBaseQueryError` shape, narrowly. */
function statusOf(error: unknown): number | string | null {
  if (error && typeof error === 'object' && 'status' in error) {
    return (error as { status: number | string }).status;
  }
  return null;
}

/** A missing arm / aged-out token is absence, not failure. */
function isNotFound(error: unknown): string | null {
  if (statusOf(error) !== 404) return null;
  // The backend distinguishes its 404 reasons on purpose (manual position, deleted
  // rule, aged-out trades, never filled) — surface the one it sent, not a generic.
  const body = (error as { data?: { error?: unknown } }).data;
  return typeof body?.error === 'string' ? body.error : 'no readout for this position';
}

/** Only a real fault reaches the strip as an error; a 404 is handled above. */
function readoutError(error: unknown): string | null {
  if (!error || statusOf(error) === 404) return null;
  return apiErrorMessage(error as never) ?? 'read failed';
}
