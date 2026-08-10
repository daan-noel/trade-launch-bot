import { useEffect, useMemo, useState } from 'react';

import { nearestSeriesIndex } from 'lib/strategy/metricPanes';
import { apiErrorMessage } from 'store/apiSlice';
import {
  useGetArmedMetricsQuery,
  useGetPositionMetricSeriesQuery,
  useGetPositionMetricsQuery,
  type ReadoutAt,
  type RuleConditionRead,
  type RuleReadout,
  type RuleReadoutSeries,
} from '@live/store/liveEndpoints';
import { useCrosshairTimeSec } from './crosshairTime';
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
 * **Hovering the chart moves the instant.** The crosshair drives a second, series
 * request whose rows the pointer indexes — one fold for the modal instead of one per
 * hover, which on a 2vCPU box is the difference between affordable and not. Fetched
 * **lazily, on the first crosshair move**, so a modal that is never scrubbed costs
 * exactly what it costs today. On pointer-out the pinned readout returns; for an open
 * position that means the live strip comes back.
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

  const crosshairTimeSec = useCrosshairTimeSec();
  // Latched: the fetch starts on the FIRST hover and the payload then stays cached
  // for the life of the modal, so leaving and re-entering the plot costs nothing.
  const [scrubbed, setScrubbed] = useState(false);
  useEffect(() => {
    if (crosshairTimeSec != null) setScrubbed(true);
  }, [crosshairTimeSec]);
  const { data: series, isLoading: seriesLoading } = useGetPositionMetricSeriesQuery(
    positionId,
    { skip: !positionId || !scrubbed },
  );

  // Hoisted out of the hover path: the crosshair moves per frame and this is one
  // array as long as the series.
  const atSec = useMemo(() => series?.at.map((ms) => ms / 1000) ?? null, [series]);
  const hovered = useMemo(
    () => readoutAt(series, atSec, crosshairTimeSec),
    [series, atSec, crosshairTimeSec],
  );

  return (
    <RuleConditionStrip
      readout={hovered?.readout ?? data ?? null}
      // The strip's own loading state covers the first series fetch: until it lands
      // there is nothing to show at the hovered instant, and showing the pinned
      // readout instead would silently answer a different question.
      loading={isLoading || (crosshairTimeSec != null && seriesLoading)}
      error={readoutError(error)}
      notFound={isNotFound(error)}
      at={at}
      onAtChange={setAt}
      hoveredAtMs={hovered?.atMs ?? null}
      hoveredBeyondCoverage={hovered?.beyondCoverage ?? false}
    />
  );
}

/**
 * The series row nearest `timeSec`, as the point routes' readout shape.
 *
 * Row lookup is the panes' own `nearestSeriesIndex` — already shared, already what
 * the lab resolves a hovered instant with. The rows are the engine's decision grid,
 * so "nearest" is at worst half a tick from the pointer.
 *
 * `beyondCoverage` is set when the row cap truncated the fold and the pointer is past
 * where it reaches. The nearest row is then the last one, repeated for the rest of
 * the chart — which reads exactly like a token that went quiet, so the strip says so
 * rather than answering a question it no longer has the data for.
 */
function readoutAt(
  series: RuleReadoutSeries | undefined,
  atSec: number[] | null,
  timeSec: number | null,
): { readout: RuleReadout; atMs: number; beyondCoverage: boolean } | null {
  if (!series || !atSec || timeSec == null || !atSec.length) return null;
  const i = nearestSeriesIndex(atSec, timeSec);
  if (i == null) return null;
  // The series' metadata IS the point shape's metadata (the backend flattens one
  // `ConditionMetaOut` into both), so the row is that metadata plus three cells.
  const conditions: RuleConditionRead[] = series.conditions.map(
    ({ values, ok, disarmed, ...meta }) => ({
      ...meta,
      value: values[i] ?? null,
      ok: ok[i] ?? false,
      // Absent on every condition but a gated trail — the only kind the fold skips.
      disarmed: disarmed?.[i] ?? false,
    }),
  );
  return {
    atMs: series.at[i],
    beyondCoverage: series.truncated && timeSec > atSec[atSec.length - 1],
    readout: {
      mint_address: series.mint_address,
      rule_id: series.rule_id,
      // A reconstruction even for a position the engine still holds — the fold keeps
      // one instant of state, not a history.
      source: 'replay',
      arm: null,
      stage: null,
      at: new Date(series.at[i]).toISOString(),
      conditions,
    },
  };
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
