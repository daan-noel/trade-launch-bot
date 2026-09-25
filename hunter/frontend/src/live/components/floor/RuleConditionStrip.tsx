import { useMemo, type ReactNode } from 'react';

import { cn } from 'lib/cn';
import { unitSuffix, useStrategyRegistry, type MetricUnit, type StrategyRegistry } from 'lib/strategy/registry';
import type { MetricRef } from 'lib/strategy/metricRef';
import { readPhrase } from 'lib/strategy/sentences';
import type {
  ReadoutAt,
  RuleConditionMeta,
  RuleConditionRead,
  RuleLineRead,
  RulePart,
  RuleReadout,
} from '@live/store/liveEndpoints';

/**
 * Where a hovered instant sits relative to the series' recorded span.
 *
 * `'in'` is the only state whose chips answer the question the pointer asked; the
 * other two clamp to an edge row. They stay distinct because the cause differs: the
 * tail is a spent row budget, the head is a recorded window that starts after the
 * token did.
 */
export type HoverCoverage = 'in' | 'before' | 'after';

/**
 * The live rule readout, laid out the way the engine reads the rule: the buy parts,
 * the signals, then the sell lines (`always`, then each stage's lines). Each condition
 * is a chip with the value the **decision loop itself** reads and whether it holds;
 * each line says whether ALL of it holds and what it does.
 *
 * These are engine values, not a recomputation: a chip that reads satisfied is one the
 * fold is acting on.
 *
 * Tone is deliberately not good/bad. A buy condition that holds is *why we're in* and
 * a sell line that holds is *why we're leaving*, so a satisfied condition is simply
 * emphasized and an unsatisfied one is recessed.
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
   *  to read. Rendered as its own message: showing the pinned readout instead would be
   *  a different question answered without saying so. */
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
  hoveredCoverage?: HoverCoverage;
  /** Whether the chart is drawing the per-condition timeline lanes. */
  bandOn?: boolean;
  /** Omit to hide the timeline control (hosts with no chart beside the strip). */
  onBandToggle?: (on: boolean) => void;
  /**
   * No fill yet (an arming episode). The engine never buys while an `always` sell line
   * or a first-stage sell line already holds, so such a line that holds **blocks the
   * buy**; without this it would read as future tense.
   */
  preEntry?: boolean;
  className?: string;
}) {
  const { data: registry } = useStrategyRegistry();
  const sections = useMemo(() => buildSections(readout), [readout]);

  if (error) {
    return (
      <StripShell className={className}>
        <span className="text-[11px] text-text-dim">Rule readout unavailable — {error}</span>
      </StripShell>
    );
  }
  if (notFound) {
    return (
      <StripShell className={className}>
        <span className="text-[11px] text-text-dim">No rule readout — {notFound}.</span>
      </StripShell>
    );
  }
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
  if (sections.length === 0) {
    return (
      <StripShell className={className}>
        <span className="text-[11px] text-text-dim">
          This rule has no conditions: it buys on arming and only take profit, stop loss or death close it.
        </span>
      </StripShell>
    );
  }
  const current = readout.stage?.index ?? null;
  const stageName = stageNames(readout);

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
              bandOn
                ? 'Hide the per-condition timeline under the chart'
                : 'Draw each condition as a lane under the chart, filled where it held'
            }
          >
            timeline
          </button>
        ) : null}
      </div>
      {readout.stage && (
        <span className="text-[11px] text-text-dim">
          In stage <b className="font-mono text-text">{readout.stage.name || `#${readout.stage.index + 1}`}</b>: the
          engine reads the Always lines, then this stage's lines.
        </span>
      )}
      {sections.map((s) => {
        const inactive = s.stage != null && current != null && s.stage !== current;
        const veto = preEntry && s.vetoes;
        return (
          <div key={s.key} className={cn('flex flex-col gap-1', inactive && 'opacity-50')}>
            <span className="text-[9px] font-bold uppercase tracking-wider text-text-dim/75" title={s.title}>
              {s.label}
              {inactive && ' (not the current stage)'}
            </span>
            {s.rows.map((r) => (
              <div key={r.key} className="flex min-w-0 flex-wrap items-center gap-1.5 pl-2">
                {r.conditions.map((c, i) => (
                  <span key={`${r.key}-${i}`} className="inline-flex items-center gap-1.5">
                    {i > 0 && <span className="text-[9px] font-bold uppercase tracking-wider text-text-dim/55">and</span>}
                    <ConditionChip read={c} registry={registry} />
                  </span>
                ))}
                {r.conditions.length === 0 && r.line && (
                  <span className="text-[10px] text-text-dim">{r.line.holds ? 'signals hold' : 'signals / no metric condition'}</span>
                )}
                {r.signal && (
                  <span className={cn('text-[10px] font-semibold', r.signal.holds ? 'text-text' : 'text-text-dim')}>
                    ⇒ {r.signal.holds ? 'holds ✓' : 'does not hold'}
                  </span>
                )}
                {r.line && <LineAction line={r.line} stageName={stageName} blocksBuy={veto && r.line.holds && r.line.sells != null} />}
              </div>
            ))}
          </div>
        );
      })}
    </StripShell>
  );
}

/** What a line does and whether it holds now. */
function LineAction({
  line,
  stageName,
  blocksBuy,
}: {
  line: RuleLineRead;
  stageName: (i: number) => string;
  blocksBuy: boolean;
}) {
  const parts: string[] = [];
  if (line.sells != null) parts.push(line.sell_pct != null ? `sell ${line.sell_pct}% "${line.sells}"` : `sell "${line.sells}"`);
  if (line.goes_to != null) parts.push(`→ ${stageName(line.goes_to)}`);
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1 rounded px-1.5 py-0.5 font-mono text-[10px]',
        blocksBuy ? 'bg-warning/10 text-warning' : line.holds ? 'bg-white/8 text-text' : 'text-text-dim',
      )}
      title={
        blocksBuy
          ? 'This sell line holds now, and with no fill yet that BLOCKS the buy: the engine never buys into a line that would sell at once'
          : line.holds
            ? 'Every condition of this line holds now'
            : 'Not every condition of this line holds'
      }
    >
      ⇒ {parts.join(' ')}
      {blocksBuy ? ' · blocks buy' : line.holds ? ' ✓' : ''}
    </span>
  );
}

/** Stage index -> name, from any part that names it. */
function stageNames(readout: RuleReadout): (i: number) => string {
  const m = new Map<number, string>();
  if (readout.stage) m.set(readout.stage.index, readout.stage.name);
  for (const x of [...readout.conditions, ...readout.lines]) {
    if (x.stage != null && x.stage_name) m.set(x.stage, x.stage_name);
  }
  return (i) => m.get(i) || `stage ${i + 1}`;
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

/** One condition: `label op threshold` with the live value. */
function ConditionChip({ read, registry }: { read: RuleConditionRead; registry: StrategyRegistry | undefined }) {
  const suffix = unitSuffix(read.unit as MetricUnit);
  const value = read.value != null ? `${formatValue(read.value)}${suffix}` : '—';
  return (
    <span
      className={cn(
        'inline-flex shrink-0 items-baseline gap-1 rounded border px-1.5 py-0.5 font-mono text-[11px]',
        read.ok ? 'border-white/25 bg-white/6 text-text' : 'border-white/10 text-text-dim',
      )}
      title={chipTitle(read, registry)}
    >
      <span>{conditionLabel(read)}</span>
      <span className={cn('tabular-nums', read.ok && 'font-semibold')}>{value}</span>
      {read.ok ? <span aria-hidden>✓</span> : null}
    </span>
  );
}

/**
 * `m_flow.buy_sol @!volume [10s] >= 2`: the read's full label and its authored
 * condition. Exported because the chart's condition lanes label themselves with it, so
 * a lane and its chip name the same thing.
 */
export function conditionLabel(read: RuleConditionMeta): string {
  const expr = describeConditions(read.conditions);
  return expr ? `${read.label} ${expr}` : read.label;
}

/** The chip's hover: the read in words, the value, and the verdict. */
function chipTitle(read: RuleConditionRead, registry: StrategyRegistry | undefined): string {
  const ref: MetricRef = { metric: read.metric, tag: read.tag, span: read.span, slice: read.slice };
  const parts: string[] = [readPhrase(registry, ref)];
  parts.push(read.ok ? 'holds now' : 'does not hold now');
  if (read.ok && read.matched_operator != null && read.matched_value != null) {
    parts.push(`matched ${read.matched_operator} ${formatValue(read.matched_value)}`);
  }
  if (read.value == null) parts.push('no reading: an unreadable metric satisfies nothing');
  return parts.join(' · ');
}

interface StripRow {
  key: string;
  conditions: RuleConditionRead[];
  /** The line's own verdict and action, when the readout carries it (the pinned read
   *  does; a hovered series row has conditions only). */
  line?: RuleLineRead;
  /** A signal's verdict, on a signal row. */
  signal?: { name: string; holds: boolean };
}

interface StripSection {
  key: string;
  label: string;
  title: string;
  /** Stage index, for a stage section. */
  stage?: number;
  /** Its sell lines are read before a buy (the pre-entry veto). */
  vetoes: boolean;
  rows: StripRow[];
}

/** One key per line (or per buy part / signal group): conditions and their line meet
 *  on it. */
export function partKey(p: RulePart): string {
  switch (p.part) {
    case 'signal':
      return `signal:${p.signal}:${p.group}`;
    case 'always':
      return `always:${p.line}`;
    case 'stage':
      return `stage:${p.stage}:${p.at_end ? 'end' : 'on'}:${p.line}`;
    default:
      return p.part;
  }
}

const BUY_TITLES: Record<string, [string, string]> = {
  event: ['Buy on', 'The print that triggers the buy: every condition must hold on it'],
  filter: ['Only if', 'Must also hold at that moment; if one fails, the rule keeps watching'],
  final_filter: ['Only if, else give up', 'Checked on the print that would buy; if one fails, the rule stops watching this coin'],
};

/** Group the readout into the rule's sections, in the fold's order. */
function buildSections(readout: RuleReadout | null | undefined): StripSection[] {
  if (!readout) return [];
  const sections = new Map<string, StripSection>();
  const rows = new Map<string, StripRow>();
  const section = (p: RulePart): StripSection => {
    let key: string;
    let label: string;
    let title: string;
    let stage: number | undefined;
    let vetoes = false;
    if (p.part === 'signal') {
      key = `signal:${p.signal}`;
      label = `Signal ${p.signal}`;
      title = 'Holds when any group holds (every condition of that group)';
    } else if (p.part === 'always') {
      key = 'always';
      label = 'Always';
      title = 'Sell lines read first, in every stage (stop loss and take profit first)';
      vetoes = true;
    } else if (p.part === 'stage') {
      key = `stage:${p.stage}:${p.at_end ? 'end' : 'on'}`;
      const name = p.stage_name || `#${(p.stage ?? 0) + 1}`;
      label = p.at_end ? `Stage ${name} · at the deadline` : `Stage ${name}`;
      title = p.at_end ? 'Read once, when the stage deadline is reached' : 'Read on every print and tick while in this stage; the first line that holds acts';
      stage = p.stage;
      vetoes = !p.at_end && p.stage === 0;
    } else {
      key = p.part;
      [label, title] = BUY_TITLES[p.part] ?? [p.part, ''];
    }
    let s = sections.get(key);
    if (!s) {
      s = { key, label, title, stage, vetoes, rows: [] };
      sections.set(key, s);
    }
    return s;
  };
  const row = (p: RulePart): StripRow => {
    const k = partKey(p);
    let r = rows.get(k);
    if (!r) {
      r = { key: k, conditions: [] };
      rows.set(k, r);
      section(p).rows.push(r);
    }
    return r;
  };
  for (const c of readout.conditions) row(c).conditions.push(c);
  for (const l of readout.lines ?? []) row(l).line = l;
  for (const s of readout.signals ?? []) {
    const sec = sections.get(`signal:${s.name}`);
    const first = sec?.rows[0];
    if (first) first.signal = s;
  }
  const order = (s: StripSection) =>
    s.key === 'event' ? 0 : s.key === 'final_filter' ? 1 : s.key === 'filter' ? 2 : s.key.startsWith('signal') ? 3 : s.key === 'always' ? 4 : 5;
  return [...sections.values()].sort((a, b) => order(a) - order(b) || (a.stage ?? 0) - (b.stage ?? 0));
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
