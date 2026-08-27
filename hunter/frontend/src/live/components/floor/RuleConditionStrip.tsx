import { useMemo, type ReactNode } from 'react';

import { cn } from 'lib/cn';
import { unitSuffix, useStrategyRegistry } from 'lib/strategy/registry';
import { formatWindowSpec, readWindow, windowSpecKey } from 'lib/strategy/windowSpec';
import type { MetricUnit } from 'lib/strategy/registry';
import type {
  ReadoutAt,
  RuleConditionMeta,
  RuleConditionRead,
  RuleReadout,
} from '@live/store/liveEndpoints';

/**
 * Where a hovered instant sits relative to the series' recorded span.
 *
 * `'in'` is the only state whose chips answer the question the pointer asked; the
 * other two clamp to an edge row. They stay distinct because the cause differs — the
 * tail is a spent row budget, the head is a recorded window that starts after the
 * token did.
 */
export type HoverCoverage = 'in' | 'before' | 'after';

/**
 * The live rule readout as a chip row: one chip per authored condition, showing the
 * value the **decision loop itself** currently reads and whether the condition holds.
 *
 * These are engine values, not a recomputation — a chip that reads satisfied is one
 * the fold is acting on. That is why this is the open-position surface and the lab's
 * metric panes are the closed-position one: for a held bag the state already exists
 * in RAM, and replaying the token's history to rebuild it would be both slower and
 * a second answer to the same question.
 *
 * Tone is deliberately not good/bad. An entry condition that holds is *why we're in*
 * and an exit condition that holds is *why we're leaving* — the same green would mean
 * opposite things. So a satisfied condition is simply emphasized (`text-text`, solid
 * border) and an unsatisfied one is recessed, with the exit side's `matched` chip
 * additionally marked, since that is the one that fired.
 */
export function RuleConditionStrip({
  readout,
  hoverUnresolved = false,
  loading = false,
  error = null,
  notFound = null,
  at,
  onAtChange,
  hoveredAtMs = null,
  hoveredCoverage = 'in',
  bandOn = false,
  onBandToggle,
  preEntry = false,
  className,
}: {
  readout: RuleReadout | null | undefined;
  /** The pointer is on the plot but left of the recorded span, so there is no row
   *  to read. Rendered as its own message: the alternative — showing the pinned
   *  readout instead — is a different question answered without saying so. */
  hoverUnresolved?: boolean;
  loading?: boolean;
  error?: string | null;
  /** The backend's own 404 reason (aged-out trades, manual position, …). */
  notFound?: string | null;
  /** Replay instant, when the host offers the entry/exit switch. */
  at?: ReadoutAt;
  onAtChange?: (at: ReadoutAt) => void;
  /** Set while `readout` is a chart-crosshair reconstruction rather than a pin. */
  hoveredAtMs?: number | null;
  /** Whether the pointer is inside the recorded span, and if not, which end it fell
   *  off. Either way the chips are an edge row, not the crosshair's row. */
  hoveredCoverage?: HoverCoverage;
  /** Whether the chart is drawing the per-condition timeline lanes. */
  bandOn?: boolean;
  /** Omit to hide the timeline control — hosts with no chart beside the strip. */
  onBandToggle?: (on: boolean) => void;
  /**
   * There is no fill yet (an arming episode), so the engine gate is
   * `entry_satisfied && !exit_metrics_satisfied` — a token-scoped EXIT condition that
   * holds **blocks the buy**. Without this the exit row reads as future tense and a
   * reader can watch every entry chip go green on a row that was never enterable.
   */
  preEntry?: boolean;
  className?: string;
}) {
  const { data: registry } = useStrategyRegistry();

  /** Registry unit per metric name — the chip's suffix (`%`, `s`, `◎`). */
  const unitOf = useMemo(() => {
    const map = new Map<string, MetricUnit>();
    for (const g of registry?.groups ?? []) {
      for (const m of g.metrics) map.set(m.name, m.unit);
    }
    return map;
  }, [registry]);

  const groups = useMemo(
    () => groupConditions(readout?.conditions ?? [], preEntry),
    [readout, preEntry],
  );

  if (error) {
    return (
      <StripShell className={className}>
        <span className="text-[11px] text-text-dim">Rule readout unavailable — {error}</span>
      </StripShell>
    );
  }
  // The backend names its 404 reasons (manual position, deleted rule, trades aged out
  // of the box's rolling window, never filled). Showing which one beats a blank panel.
  if (notFound) {
    return (
      <StripShell className={className}>
        <span className="text-[11px] text-text-dim">No rule readout — {notFound}.</span>
      </StripShell>
    );
  }
  // Absent readout while loading is the first fetch.
  if (!readout) {
    if (loading) {
      return (
        <StripShell className={className}>
          <span className="text-[11px] text-text-dim">Reading rule…</span>
        </StripShell>
      );
    }
    return hoverUnresolved ? (
      <StripShell className={className}>
        <span className="text-[11px] text-text-dim">
          No reading here — this candle is before the reconstructed window.
        </span>
      </StripShell>
    ) : null;
  }
  if (groups.length === 0) {
    return (
      <StripShell className={className}>
        <span className="text-[11px] text-text-dim">
          This rule authors no conditions — only take-profit / stop-loss / death close it.
        </span>
      </StripShell>
    );
  }

  return (
    <StripShell className={className}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <ReadoutSourceLine
          readout={readout}
          at={at}
          onAtChange={onAtChange}
          hoveredAtMs={hoveredAtMs}
          hoveredCoverage={hoveredCoverage}
        />
        {onBandToggle ? (
          <button
            type="button"
            onClick={() => onBandToggle(!bandOn)}
            className={cn(
              'rounded px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider',
              bandOn ? 'bg-white/10 text-text' : 'text-text-dim hover:text-text',
            )}
            title={
              // Off by default because turning it on is what pays for the fold —
              // the same one the crosshair uses, so whichever comes first covers
              // both and the second is free.
              bandOn
                ? 'Hide the per-condition timeline under the chart'
                : 'Draw each condition as a lane under the chart, filled where it held — the fire windows without scrubbing for them'
            }
          >
            timeline
          </button>
        ) : null}
      </div>
      {groups.map((g) => (
        <div key={g.key} className="flex min-w-0 flex-wrap items-center gap-1.5">
          <span
            className="shrink-0 text-[9px] font-bold uppercase tracking-wider text-text-dim/75"
            title={g.title}
          >
            {g.label}
          </span>
          {g.items.map((c, i) => (
            <ConditionChip
              key={`${c.side}-${c.stage ?? ''}-${c.metric}-${windowSpecKey(readWindow(c))}-${i}`}
              read={c}
              unit={unitOf.get(c.metric) ?? null}
              preEntry={preEntry}
            />
          ))}
        </div>
      ))}
    </StripShell>
  );
}

/**
 * Says where the numbers came from, and — on a replay — lets you move the instant.
 *
 * A live readout needs no caption: it is the engine's own state and the chips update
 * as it moves. A replay does, and prominently: it is a reconstruction from stored
 * trades, at one frozen instant. Presenting the two identically would quietly upgrade
 * an approximation into engine truth.
 *
 * Three cases, not two. A **hovered** instant is a replay like any other, but the
 * instant is the pointer's rather than a fill's, so it says so with a clock instead
 * of a pin name — and keeps the entry/exit buttons visible, because they are what
 * the pointer returns to.
 */
function ReadoutSourceLine({
  readout,
  at,
  onAtChange,
  hoveredAtMs = null,
  hoveredCoverage = 'in',
}: {
  readout: RuleReadout;
  at?: ReadoutAt;
  onAtChange?: (at: ReadoutAt) => void;
  /** Set while the chart crosshair drives the readout. */
  hoveredAtMs?: number | null;
  hoveredCoverage?: HoverCoverage;
}) {
  if (readout.source === 'engine' && hoveredAtMs == null) {
    return (
      <span
        className="text-[9px] font-bold uppercase tracking-wider text-text-dim/70"
        title={`Read live from the engine's own state — arm ${readout.arm ?? '—'}`}
      >
        ● live from engine
      </span>
    );
  }
  return (
    <div className="flex flex-wrap items-center gap-2">
      <span
        className="text-[9px] font-bold uppercase tracking-wider text-warning/80"
        title={
          hoveredAtMs != null
            ? "Reconstructed at the instant under the crosshair by folding stored trades back through the engine's metric code, on the same tick grid the engine decides on. Even for a position the engine still holds this is a reconstruction — the fold keeps one instant of state, not a history — and stored rows carry an approximated real-reserve value, so it is close to, not identical with, what the engine read here."
            : "Reconstructed by folding stored trades back through the engine's metric code. Stored rows carry an approximated real-reserve value and any unpersisted trade is absent, so this is close to — not identical with — what the engine read."
        }
      >
        {hoveredAtMs != null
          ? `○ reconstructed at ${formatClock(hoveredAtMs)}`
          : `○ reconstructed at ${at === 'entry' ? 'entry' : 'exit'}`}
      </span>
      {/* The pointer is outside the recorded span, so the chips are an edge row
          repeated rather than the crosshair's row. Saying which end beats letting it
          read as a token that simply went quiet. */}
      {hoveredCoverage !== 'in' ? (
        <span
          className="text-[9px] font-bold uppercase tracking-wider text-warning/80"
          title={
            hoveredCoverage === 'after'
              ? 'The reconstruction hit its row ceiling before this point, so these are the values at the last instant it covers — not at the crosshair.'
              : 'The reconstruction records a window around the entry, and this is earlier than it starts — so these are the values at its first instant, not at the crosshair.'
          }
        >
          {hoveredCoverage === 'after' ? '· past coverage' : '· before coverage'}
        </span>
      ) : null}
      {onAtChange ? (
        <div className="flex items-center gap-1">
          {(['entry', 'exit'] as const).map((k) => (
            <button
              key={k}
              type="button"
              onClick={() => onAtChange(k)}
              className={cn(
                'rounded px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider',
                (at ?? 'exit') === k
                  ? 'bg-white/10 text-text'
                  : 'text-text-dim hover:text-text',
              )}
              title={
                k === 'entry'
                  ? 'What the rule saw at the entry fill'
                  : 'What the rule saw at the exit fill'
              }
            >
              {k}
            </button>
          ))}
        </div>
      ) : null}
      <span className="text-[10px] tabular-nums text-text-dim/70">
        {new Date(readout.at).toLocaleString()}
      </span>
    </div>
  );
}

/** `hh:mm:ss` — the hovered instant, which is read against the chart beside it. */
function formatClock(ms: number): string {
  return new Date(ms).toLocaleTimeString();
}

function StripShell({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        'flex flex-col gap-1.5 rounded-lg border border-white/8 bg-bg-panel/80 px-3 py-2',
        className,
      )}
    >
      {children}
    </div>
  );
}

/** One condition: `metric op threshold` with the live value. */
function ConditionChip({
  read,
  unit,
  preEntry = false,
}: {
  read: RuleConditionRead;
  unit: MetricUnit | null;
  /** No fill yet, so a satisfied EXIT condition is holding the buy back. */
  preEntry?: boolean;
}) {
  const suffix = unit ? unitSuffix(unit) : '';
  const label = conditionLabel(read);
  const value = read.value != null ? `${formatValue(read.value)}${suffix}` : '—';

  // An inactive ladder stage: shown so the ladder is visible, dimmed because the
  // fold only ever evaluates the stage the position is on.
  const inactiveStage = read.side === 'stage' && read.stage_active === false;
  // `disarmed` = the fold is skipping this req (a held trail under its gate).
  // Dormant, not failing — a dashed chip, never the plain unsatisfied style.
  const dormant = read.disarmed || inactiveStage;
  // Pre-entry, a satisfied exit metric is the `can_enter` veto — the ✓ alone would
  // read as progress toward a buy when it is the thing preventing one.
  const blocksEntry = preEntry && read.side === 'exit' && read.ok && !dormant;

  return (
    <span
      className={cn(
        'inline-flex shrink-0 items-baseline gap-1 rounded border px-1.5 py-0.5 font-mono text-[11px]',
        read.disarmed
          ? 'border-dashed border-white/12 text-text-dim/70'
          : inactiveStage
            ? 'border-white/8 text-text-dim/60'
            : blocksEntry
              ? 'border-warning/40 bg-warning/8 text-text'
              : read.ok
                ? 'border-white/25 bg-white/6 text-text'
                : 'border-white/10 text-text-dim',
      )}
      title={chipTitle(read, inactiveStage, blocksEntry)}
    >
      <span>{label}</span>
      <span className={cn('tabular-nums', !dormant && read.ok && 'font-semibold')}>
        {value}
      </span>
      {/* The arming gate rides on the condition whether or not the fold is
          currently skipping it — pre-entry it is not skipped, and it is still the
          thing that decides when this trail starts to matter. */}
      {read.arm_above_pct != null ? (
        <span className="text-[9px] uppercase tracking-wider text-text-dim/80">
          arms +{read.arm_above_pct}%
        </span>
      ) : null}
      {blocksEntry ? (
        <span className="text-[9px] font-bold uppercase tracking-wider text-warning/90">
          blocks entry
        </span>
      ) : read.ok && !dormant ? (
        <span aria-hidden>✓</span>
      ) : null}
    </span>
  );
}

/**
 * `retrace >= 12` / `take profit >= 40` — the authored condition, human-side.
 *
 * A desugared TP/SL is renamed but **keeps its threshold**: the chip's value is the
 * live `pnl`, so dropping the target leaves a number with nothing to compare it
 * against — the one thing the chip exists to show.
 *
 * Exported because the chart's condition lanes label themselves with it. The band
 * is only legible as a legend for these chips, which it can only be if the two
 * names come from one place.
 */
export function conditionLabel(read: RuleConditionMeta): string {
  const name =
    read.origin === 'take_profit'
      ? 'take profit'
      : read.origin === 'stop_loss'
        ? 'stop loss'
        : formatWindowSpec(readWindow(read))
          ? `${read.metric}@${formatWindowSpec(readWindow(read))}`
          : read.metric;
  const expr = describeConditions(read.conditions);
  return expr ? `${name} ${expr}` : name;
}

function chipTitle(
  read: RuleConditionRead,
  inactiveStage: boolean,
  blocksEntry = false,
): string {
  const parts: string[] = [`${read.group}.${read.metric}`];
  const span = formatWindowSpec(readWindow(read));
  if (span) parts.push(`${span} window`);
  if (read.origin !== 'authored') {
    parts.push(`${describeConditions(read.conditions) || ''} (desugared ${read.origin})`.trim());
  }
  if (read.disarmed) {
    parts.push(
      `trailing stop not armed — arms at +${read.arm_above_pct}% PnL; the engine is skipping this condition`,
    );
  } else if (inactiveStage) {
    parts.push(
      `stage ${(read.stage ?? 0) + 1} — not the active stage, so the engine is not evaluating it`,
    );
  } else if (blocksEntry) {
    parts.push(
      'satisfied now — and with no fill yet that BLOCKS the buy: the engine enters only while every entry condition holds and no exit metric does',
    );
  } else {
    parts.push(read.ok ? 'satisfied now' : 'not satisfied');
  }
  // The backend already resolved which arm of the DNF matched; naming it saves
  // reading the expression against the value by hand.
  if (read.ok && read.matched_operator != null && read.matched_value != null) {
    parts.push(`matched ${read.matched_operator} ${formatValue(read.matched_value)}`);
  }
  if (read.value == null) {
    parts.push('no reading — an unreadable metric satisfies nothing');
  }
  return parts.join(' · ');
}

/**
 * The authored DNF as text. Wire shape mirrors the engine's: a flat
 * `[{operator,value}]` is one AND arm; nested arrays are OR arms. `,` reads as AND
 * and `|` as OR, matching the rule editor's own grammar.
 */
function describeConditions(conditions: unknown): string {
  if (!Array.isArray(conditions) || conditions.length === 0) return '';
  const arms: unknown[][] = Array.isArray(conditions[0])
    ? (conditions as unknown[][])
    : [conditions as unknown[]];
  return arms
    .map((arm) =>
      arm
        .map((c) => {
          const cond = c as { operator?: unknown; value?: unknown };
          return typeof cond.operator === 'string' && typeof cond.value === 'number'
            ? `${cond.operator} ${formatValue(cond.value)}`
            : '';
        })
        .filter(Boolean)
        .join(', '),
    )
    .filter(Boolean)
    .join(' | ');
}

/** Compact metric number — same shape the lab's metric panes use. */
function formatValue(v: number): string {
  if (!Number.isFinite(v)) return '—';
  const a = Math.abs(v);
  if (a >= 1000) return v.toFixed(0);
  if (a >= 100) return v.toFixed(1);
  if (a >= 1) return v.toFixed(2);
  if (a >= 0.01) return v.toFixed(3);
  return v.toPrecision(2);
}

interface ConditionGroup {
  key: string;
  label: string;
  title: string;
  items: RuleConditionRead[];
}

/**
 * Bucket by side, preserving the backend's order (which is the fold's order:
 * entry, then stop-loss → take-profit → authored exits, then each ladder stage).
 * One row per stage so a ladder reads as a ladder.
 */
function groupConditions(
  conditions: RuleConditionRead[],
  preEntry: boolean,
): ConditionGroup[] {
  const buckets = new Map<string, ConditionGroup>();
  for (const c of conditions) {
    const key = c.side === 'stage' ? `stage-${c.stage ?? 0}` : c.side;
    let bucket = buckets.get(key);
    if (!bucket) {
      bucket = {
        key,
        label:
          c.side === 'entry'
            ? 'Entry'
            : c.side === 'exit'
              ? 'Exit'
              : `Stage ${(c.stage ?? 0) + 1}`,
        title:
          c.side === 'entry'
            ? preEntry
              ? 'Entry conditions — ALL must hold to buy, and no exit condition may hold either'
              : 'Entry conditions — ALL must hold to buy'
            : c.side === 'exit'
              ? preEntry
                ? // The pre-entry half of `can_enter`. Stated on the group because it
                  // is true of the whole row, not only of the members holding now.
                  'Exit conditions — ANY one fires the sell, and with no fill yet ANY one also BLOCKS the buy'
                : 'Exit conditions — ANY one fires the sell'
              : `Scale-out stage ${(c.stage ?? 0) + 1}${
                c.stage_active ? ' (active)' : ' (not the active stage)'
              }`,
        items: [],
      };
      buckets.set(key, bucket);
    }
    bucket.items.push(c);
  }
  // Entry, exit, then stages in ladder order. Stages sort NUMERICALLY — a string
  // compare puts `stage-10` before `stage-2` and silently scrambles the ladder.
  const order = (k: string) => (k === 'entry' ? 0 : k === 'exit' ? 1 : 2);
  const stageIndex = (g: ConditionGroup) => g.items[0]?.stage ?? 0;
  return [...buckets.values()].sort(
    (a, b) => order(a.key) - order(b.key) || stageIndex(a) - stageIndex(b),
  );
}
