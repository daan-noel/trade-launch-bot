import { useMemo, type ReactNode } from 'react';

import { cn } from 'lib/cn';
import { unitSuffix, useStrategyRegistry } from 'lib/strategy/registry';
import type { MetricUnit } from 'lib/strategy/registry';
import type { ReadoutAt, RuleConditionRead, RuleReadout } from '@live/store/liveEndpoints';

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
  loading = false,
  error = null,
  notFound = null,
  at,
  onAtChange,
  className,
}: {
  readout: RuleReadout | null | undefined;
  loading?: boolean;
  error?: string | null;
  /** The backend's own 404 reason (aged-out trades, manual position, …). */
  notFound?: string | null;
  /** Replay instant, when the host offers the entry/exit switch. */
  at?: ReadoutAt;
  onAtChange?: (at: ReadoutAt) => void;
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

  const groups = useMemo(() => groupConditions(readout?.conditions ?? []), [readout]);

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
    return loading ? (
      <StripShell className={className}>
        <span className="text-[11px] text-text-dim">Reading rule…</span>
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
      <ReadoutSourceLine readout={readout} at={at} onAtChange={onAtChange} />
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
              key={`${c.side}-${c.stage ?? ''}-${c.metric}-${c.window_size_sec ?? ''}-${i}`}
              read={c}
              unit={unitOf.get(c.metric) ?? null}
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
 */
function ReadoutSourceLine({
  readout,
  at,
  onAtChange,
}: {
  readout: RuleReadout;
  at?: ReadoutAt;
  onAtChange?: (at: ReadoutAt) => void;
}) {
  if (readout.source === 'engine') {
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
        title="Reconstructed by folding stored trades back through the engine's metric code. Stored rows carry an approximated real-reserve value and any unpersisted trade is absent, so this is close to — not identical with — what the engine read."
      >
        ○ reconstructed at {at === 'entry' ? 'entry' : 'exit'}
      </span>
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
}: {
  read: RuleConditionRead;
  unit: MetricUnit | null;
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

  return (
    <span
      className={cn(
        'inline-flex shrink-0 items-baseline gap-1 rounded border px-1.5 py-0.5 font-mono text-[11px]',
        read.disarmed
          ? 'border-dashed border-white/12 text-text-dim/70'
          : inactiveStage
            ? 'border-white/8 text-text-dim/60'
            : read.ok
              ? 'border-white/25 bg-white/6 text-text'
              : 'border-white/10 text-text-dim',
      )}
      title={chipTitle(read, inactiveStage)}
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
      {read.ok && !dormant ? <span aria-hidden>✓</span> : null}
    </span>
  );
}

/**
 * `retrace >= 12` / `take profit >= 40` — the authored condition, human-side.
 *
 * A desugared TP/SL is renamed but **keeps its threshold**: the chip's value is the
 * live `pnl`, so dropping the target leaves a number with nothing to compare it
 * against — the one thing the chip exists to show.
 */
function conditionLabel(read: RuleConditionRead): string {
  const name =
    read.origin === 'take_profit'
      ? 'take profit'
      : read.origin === 'stop_loss'
        ? 'stop loss'
        : read.window_size_sec != null
          ? `${read.metric}@${read.window_size_sec}s`
          : read.metric;
  const expr = describeConditions(read.conditions);
  return expr ? `${name} ${expr}` : name;
}

function chipTitle(read: RuleConditionRead, inactiveStage: boolean): string {
  const parts: string[] = [`${read.group}.${read.metric}`];
  if (read.window_size_sec != null) parts.push(`${read.window_size_sec}s window`);
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
  } else {
    parts.push(read.ok ? 'satisfied now' : 'not satisfied');
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
function groupConditions(conditions: RuleConditionRead[]): ConditionGroup[] {
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
            ? 'Entry conditions — ALL must hold to buy'
            : c.side === 'exit'
              ? 'Exit conditions — ANY one fires the sell'
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
