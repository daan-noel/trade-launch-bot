import { useEffect, useMemo, useState } from 'react';

import {
  CONDITION_LANE_COLOR,
  CONDITION_VALUE_LANE_COLOR,
  seriesIndexAsOf,
} from 'lib/strategy/metricPanes';
import { parseMetricExitTarget } from 'lib/strategy/exitReason';
import { apiErrorMessage } from 'store/apiSlice';
import {
  useGetArmedMetricSeriesQuery,
  useGetArmedMetricsQuery,
  useGetPositionMetricSeriesQuery,
  useGetPositionMetricsQuery,
  type ReadoutAt,
  type RuleConditionRead,
  type RuleConditionSeries,
  type RuleReadout,
  type RuleReadoutSeries,
} from '@live/store/liveEndpoints';
import type { ChartTimeBand } from 'components/token-price-chart';
import { usePublishConditionBands, type ConditionBands } from './conditionBands';
import { useCrosshairTimeSec } from './crosshairTime';
import {
  RuleConditionStrip,
  conditionLabel,
  type HoverCoverage,
} from './RuleConditionStrip';

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
export function LivePositionConditions({
  positionId,
  exitReason,
}: {
  positionId: string;
  /**
   * The position's persisted exit reason, when the host has it. Selects which
   * condition the chart draws as a line — without it the pane falls back to the
   * first exit condition, which may not be the one that closed the position.
   */
  exitReason?: string | null;
}) {
  const [at, setAt] = useState<ReadoutAt>('exit');
  // Latched from the response rather than passed in: the caller (a modal that may be
  // walking a lane with ←/→) does not reliably know whether the engine still holds
  // this row, and the answer is authoritative in the payload.
  const [isReplay, setIsReplay] = useState(false);
  const gate = useConditionSeriesGate();
  const { data, isLoading, error } = useGetPositionMetricsQuery(
    { positionId, at },
    {
      // Paused while the pointer is on the plot: the pin is not on screen then, so
      // each poll would spend the serialized decision loop a round-trip to refresh
      // something nobody is reading. The cached pin renders instantly on pointer-out
      // and the interval resumes, which costs at most one stale second.
      pollingInterval: isReplay || gate.crosshairTimeSec != null ? 0 : READOUT_POLL_MS,
      skip: !positionId,
    },
  );
  useEffect(() => {
    if (data?.source) setIsReplay(data.source === 'replay');
  }, [data?.source]);

  // The crosshair and the timeline lanes want the same payload. One cache entry
  // serves both, so whichever the user reaches for first pays the fold.
  const { data: series, isLoading: seriesLoading } = useGetPositionMetricSeriesQuery(
    positionId,
    { skip: !positionId || !gate.wanted },
  );

  return (
    <ConditionSeriesStrip
      gate={gate}
      series={series}
      seriesLoading={seriesLoading}
      readout={data ?? null}
      loading={isLoading}
      error={error}
      // A post-mortem is opened on "why did it leave", so the drawn line is the exit
      // side's.
      drawSide="exit"
      exitReason={exitReason}
      at={at}
      onAtChange={setAt}
    />
  );
}

/**
 * The crosshair + timeline half of a condition strip, over whichever series its host
 * fetched. **One implementation for both hosts** — a position modal and a Waiting
 * modal must resolve a hovered bar, publish lanes and caption their honesty
 * identically, and two copies of this drift on the first fix to either.
 *
 * Split from the fetch because the two hosts key their series differently, and split
 * at {@link useConditionSeriesGate} because whether to fetch at all is decided *here*
 * (first hover, or lanes on) but must be known *there*.
 */
function ConditionSeriesStrip({
  gate,
  series,
  seriesLoading,
  readout,
  loading,
  error,
  drawSide,
  exitReason,
  at,
  onAtChange,
}: {
  gate: ConditionSeriesGate;
  series: RuleReadoutSeries | undefined;
  seriesLoading: boolean;
  /** The pinned (non-hovered) readout — engine state or a fill-instant replay. */
  readout: RuleReadout | null;
  loading: boolean;
  error: unknown;
  /** Which side's condition gets the chart's value line. */
  drawSide: ReadSide;
  exitReason?: string | null;
  at?: ReadoutAt;
  onAtChange?: (at: ReadoutAt) => void;
}) {
  const { crosshairTimeSec, bandOn, setBandOn } = gate;
  // Hoisted out of the hover path: the crosshair moves per frame and this is one
  // array as long as the series.
  const atSec = useMemo(() => series?.at.map((ms) => ms / 1000) ?? null, [series]);
  const hovered = useMemo(
    () => readoutAt(series, atSec, crosshairTimeSec),
    [series, atSec, crosshairTimeSec],
  );

  const bands = useMemo(
    () => (bandOn ? seriesToBands(series, atSec, drawSide, exitReason) : null),
    [bandOn, series, atSec, drawSide, exitReason],
  );
  usePublishConditionBands(bands);

  // The pointer is ON the plot: the pinned readout must NOT stand in for a hovered
  // one. Every bar now resolves to an instant, so a `null` row means the pointer is
  // genuinely left of the reconstructed window — and saying so is the whole fix. The
  // old `?? readout` fallback showed the pin's values on every unresolvable bar,
  // identical bar after bar, which reads as a metric frozen at its exit value.
  const scrubbing = crosshairTimeSec != null;
  return (
    <RuleConditionStrip
      readout={hovered?.readout ?? (scrubbing ? null : readout)}
      hoverUnresolved={scrubbing && hovered == null && !seriesLoading}
      // The strip's own loading state covers the first series fetch: until it lands
      // there is nothing to show at the hovered instant, and showing the pinned
      // readout instead would silently answer a different question.
      loading={loading || (scrubbing && seriesLoading)}
      error={readoutError(error)}
      notFound={isNotFound(error)}
      at={at}
      onAtChange={onAtChange}
      hoveredAtMs={hovered?.atMs ?? null}
      hoveredCoverage={hovered?.coverage ?? (scrubbing ? 'before' : 'in')}
      bandOn={bandOn}
      onBandToggle={setBandOn}
    />
  );
}

/** Which side of the rule the chart draws as a value line. */
type ReadSide = 'entry' | 'exit';

interface ConditionSeriesGate {
  /** Wall-clock second under the chart crosshair; `null` on pointer-out. */
  crosshairTimeSec: number | null;
  bandOn: boolean;
  setBandOn: (on: boolean) => void;
  /** The series is worth fetching: the plot has been touched at least once, or the
   *  lanes are on. */
  wanted: boolean;
}

/**
 * Whether the (expensive) series fetch has been earned yet.
 *
 * Lives outside {@link ConditionSeriesStrip} because the host owns the query, and the
 * host cannot know the answer — it depends on a crosshair the chart publishes and a
 * toggle the strip renders. `scrubbed` is **latched**: the fetch starts on the FIRST
 * hover and the payload then stays cached for the life of the modal, so leaving and
 * re-entering the plot costs nothing.
 */
function useConditionSeriesGate(): ConditionSeriesGate {
  const crosshairTimeSec = useCrosshairTimeSec();
  const [scrubbed, setScrubbed] = useState(false);
  useEffect(() => {
    if (crosshairTimeSec != null) setScrubbed(true);
  }, [crosshairTimeSec]);
  const [bandOn, setBandOn] = useState(false);
  return { crosshairTimeSec, bandOn, setBandOn, wanted: scrubbed || bandOn };
}

/**
 * The series as chart lanes: per condition, the stretches over which it held.
 *
 * A **disarmed** row is not a satisfied one however `ok` reads. The fold skips a
 * gated trail until its arming gate clears, so filling those rows would draw a stop
 * that was never being evaluated.
 *
 * Conditions with no span keep their (empty) lane. Against the coverage track that
 * reads as "never fired", which is a real answer and the most common one for the
 * exits that did not close the position.
 */
function seriesToBands(
  series: RuleReadoutSeries | undefined,
  atSec: number[] | null,
  drawSide: ReadSide,
  exitReason: string | null | undefined,
): ConditionBands | null {
  if (!series || !atSec || atSec.length === 0) return null;
  const lanes: ChartTimeBand[] = series.conditions.map((c, idx) => {
    const spans: Array<{ from: number; to: number }> = [];
    let start = -1;
    for (let i = 0; i < atSec.length; i++) {
      const on = (c.ok[i] ?? false) && !(c.disarmed?.[i] ?? false);
      if (on && start < 0) start = i;
      else if (!on && start >= 0) {
        spans.push({ from: atSec[start], to: atSec[i - 1] });
        start = -1;
      }
    }
    if (start >= 0) spans.push({ from: atSec[start], to: atSec[atSec.length - 1] });
    return {
      key: `${c.side}-${c.stage ?? ''}-${c.metric}-${c.window_size_sec ?? ''}-${idx}`,
      label: conditionLabel(c),
      color: CONDITION_LANE_COLOR,
      spans,
    };
  });
  const drawn = valueLaneCondition(series, drawSide, exitReason);
  return {
    lanes,
    valueLane: drawn
      ? {
          key: `value-${drawn.metric}-${drawn.window_size_sec ?? ''}`,
          label: conditionLabel(drawn),
          color: CONDITION_VALUE_LANE_COLOR,
          points: atSec.map((t, i) => ({ timeSec: t, value: drawn.values[i] ?? null })),
          threshold: soleThreshold(drawn),
        }
      : null,
    coverage: { from: atSec[0], to: atSec[atSec.length - 1] },
  };
}

/**
 * The condition whose reading gets drawn.
 *
 * `drawSide` is the question the host is asking. A post-mortem asks "why did it
 * leave" ⇒ the exit condition the reason names, else the first one. A Waiting row
 * asks "why has it NOT entered" ⇒ an entry condition, where no reason exists to name
 * one, so the first authored one is drawn — stable across refreshes, unlike
 * "whichever is currently failing", which would swap lines as the token moves.
 */
function valueLaneCondition(
  series: RuleReadoutSeries,
  drawSide: ReadSide,
  exitReason: string | null | undefined,
) {
  const side: RuleConditionSeries[] = series.conditions.filter((c) => c.side === drawSide);
  if (side.length === 0) return null;
  const named = drawSide === 'exit' && exitReason ? matchExitReason(side, exitReason) : null;
  return named ?? side[0];
}

/**
 * Resolve a persisted exit reason to the condition it names.
 *
 * Matching by name ALONE would be free to pick the lifetime condition for a windowed
 * exit, which is the mismatch this whole pane exists to make impossible — so a label
 * carrying no window qualifier only matches when it is unambiguous.
 */
function matchExitReason(exits: RuleConditionSeries[], reason: string) {
  const target = parseMetricExitTarget(reason);
  if (!target) return null;
  const byName = exits.filter((c) => c.metric === target.metric);
  if (byName.length === 0) return null;
  if (target.windowSec != null) {
    return byName.find((c) => c.window_size_sec === target.windowSec) ?? null;
  }
  return byName.length === 1 ? byName[0] : null;
}

/** The condition's threshold when it authors exactly one — a DNF with several arms
 *  has no single line to draw, and inventing one would misstate the rule. */
function soleThreshold(c: RuleConditionSeries): number | null {
  const conds = c.conditions;
  if (!Array.isArray(conds) || conds.length !== 1) return null;
  const only = conds[0] as { value?: unknown };
  return typeof only?.value === 'number' && Number.isFinite(only.value) ? only.value : null;
}

/**
 * The series row nearest `timeSec`, as the point routes' readout shape.
 *
 * Row lookup is `seriesIndexAsOf` — the last row AT OR BEFORE the instant, never the
 * nearest one. The chart resolves a hovered candle to the end of its coverage, so the
 * pair answers "what had the rule seen by the end of this candle". Nearest-row would
 * be free to answer with a row from *after* the hovered instant, reporting trades that
 * had not happened there — on a launch, where a whole position can live inside one
 * candle, that is the difference between seeing the crossing and not.
 *
 * `coverage` says whether the pointer is inside the recorded span, and if not, which
 * end it fell off. Either way `nearestSeriesIndex` clamps to an edge row and the strip
 * would otherwise present that row's values as if they were the crosshair's:
 *
 * - `'after'` — the row cap truncated the fold. The last row repeats for the rest of
 *   the chart, which reads exactly like a token that went quiet.
 * - `'before'` — the server recorded a window around the entry (`record_from`), so
 *   rows to its left exist and were not shipped. Without a window there is nothing to
 *   the left to miss, so that case stays `'in'`.
 */
function readoutAt(
  series: RuleReadoutSeries | undefined,
  atSec: number[] | null,
  timeSec: number | null,
): { readout: RuleReadout; atMs: number; coverage: HoverCoverage } | null {
  if (!series || !atSec || timeSec == null || !atSec.length) return null;
  const i = seriesIndexAsOf(atSec, timeSec);
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
    coverage: hoverCoverage(series, atSec, timeSec),
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
 * Which side of the recorded span the pointer is on.
 *
 * Both edges clamp to a real row, so the danger is identical at either end: the strip
 * would show that row's values as though they were the crosshair's. They are reported
 * separately because the fix differs — the tail wants a bigger budget, the head wants
 * a different window.
 */
function hoverCoverage(
  series: RuleReadoutSeries,
  atSec: number[],
  timeSec: number,
): HoverCoverage {
  if (timeSec > atSec[atSec.length - 1]) {
    // Past the end without truncation is not a gap: the fold ran to the tail, so the
    // last row IS the token's final state.
    return series.truncated ? 'after' : 'in';
  }
  // Left of the start only hides something when a window was recorded. Without one the
  // first row is the token's first trade and there is nothing earlier to miss.
  if (timeSec < atSec[0]) return series.record_from ? 'before' : 'in';
  return 'in';
}

/**
 * The same readout for an ARMED (token, rule) pair — the Waiting lane's "why has
 * this not entered yet". Exit conditions come back too, position-scoped ones with
 * no reading, which is exactly what the pre-entry `can_enter` gate sees.
 *
 * Beside a chart it gets the **whole** surface a held position gets: hovering moves
 * the instant and the timeline draws each condition's fire windows. A Waiting row is
 * the one a reader stares at longest — the question is always "how close is it" — and
 * that is a question about the approach, which one live instant cannot answer.
 *
 * The series is fetched lazily (first hover / lanes on) and never polled, while the
 * pinned readout keeps polling the engine at ~1 Hz: the pin is live state and moves,
 * the fold is the past and does not.
 */
export function ArmedRuleConditions({
  mint,
  ruleId,
  armedAt,
}: {
  mint: string;
  ruleId: string | null;
  /** When the arm was raised — centres the series' row budget on why this row is
   *  *still* waiting rather than on the token's first minutes. */
  armedAt?: string | null;
}) {
  const gate = useConditionSeriesGate();
  const { data, isLoading, error } = useGetArmedMetricsQuery(
    { mint, ruleId: ruleId ?? '' },
    {
      // Same pause as the position strip: while the pointer is on the plot the pin is
      // off screen, so polling it spends the decision loop a round-trip on something
      // nobody is reading.
      pollingInterval: gate.crosshairTimeSec != null ? 0 : READOUT_POLL_MS,
      skip: !mint || !ruleId,
    },
  );
  const { data: series, isLoading: seriesLoading } = useGetArmedMetricSeriesQuery(
    { mint, ruleId: ruleId ?? '', armedAt },
    { skip: !mint || !ruleId || !gate.wanted },
  );
  if (!ruleId) return null;
  return (
    <ConditionSeriesStrip
      gate={gate}
      series={series}
      seriesLoading={seriesLoading}
      readout={data ?? null}
      loading={isLoading}
      error={error}
      // Nothing has exited, so there is no exit reason and no exit line worth
      // drawing — the row is being held out by an ENTRY condition.
      drawSide="entry"
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
