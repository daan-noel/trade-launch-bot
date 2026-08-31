// Compact at-a-glance chip cluster for a rule's `params` (TP/SL + entry/exit
// metric conditions). Shared by Rules, Simulate, Fingerprints (used-by), and the
// generic sweep tables — one SSOT so every surface that shows a rule reads the same.

import { Fragment, type CSSProperties, type ReactNode } from 'react';
import { cn } from 'lib/cn';
import { formatDecimalTrim } from 'utils/format';
import { findMetric, useStrategyRegistry, type Operator } from 'lib/strategy/registry';
import { metricColorStyle } from 'lib/strategy/metricColors';
import { conditionExprFromJson } from 'lib/strategy/grammar';
import { formatWindowSpec, windowSpecFromStrict } from 'lib/strategy/windowSpec';

/** Nested `params` blob as stored on `strategy_rules.params` / sweep combos. A
 *  group's value is a plain object (one window) OR an array of them (one per
 *  window) — the multi-window-per-group wire form. */
interface RuleParamsJson {
  take_profit?: number | null;
  stop_loss?: number | null;
  entry?: Record<string, unknown>;
  entry_event?: Record<string, unknown>;
  entry_lock?: 'slot' | null;
  /** Object-form OR array-form DNF. */
  exit?: Record<string, unknown> | Record<string, unknown>[];
  scale_out?: Array<{
    sell_bps?: number | null;
    take_profit?: number | null;
    conditions?: Record<string, unknown>;
  }> | null;
  reentry?: { cooldown_sec?: number; max_episodes_per_token?: number } | null;
  exclusive?: boolean;
  priority?: number;
}

interface SideChip {
  group: string;
  metric: string;
  operator: string;
  text: string;
  /** When true, render a `|` separator before this chip (OR between arms). */
  orBefore?: boolean;
  /** When true, render `∧` before this chip (AND inside a DNF clause). */
  andBefore?: boolean;
  /** When true, render `∨` before this chip (OR between DNF clauses). */
  clauseOrBefore?: boolean;
}

function chip(text: ReactNode, cls?: string, style?: CSSProperties): ReactNode {
  return (
    <span
      className={cn(
        'inline-block rounded border border-white/10 bg-surface px-1.5 py-0.5 font-mono text-[11px] leading-tight',
        cls,
      )}
      style={style}
    >
      {text}
    </span>
  );
}

/** Terse chips for one side's metric conditions, e.g. `time>10`, `liquidity<30 | liquidity>=70`.
 *  A group value is a plain object (one window) or an array of them (one per window). */
function sideChips(side: Record<string, unknown> | undefined): SideChip[] {
  if (!side) return [];
  const out: SideChip[] = [];
  for (const [group, groupVal] of Object.entries(side)) {
    const instances = Array.isArray(groupVal) ? groupVal : [groupVal];
    for (const body of instances) {
      if (!body || typeof body !== 'object' || Array.isArray(body)) continue;
      const inst = body as Record<string, unknown>;
      // The whole span, not just a size: two slot windows of one metric would
      // otherwise chip identically, and a slot window would read as a lifetime one.
      const numeric = Object.fromEntries(
        Object.entries(inst).filter((e): e is [string, number] => typeof e[1] === 'number'),
      );
      const spec = windowSpecFromStrict(numeric);
      const suffix = spec ? `(${formatWindowSpec(spec)})` : '';
      for (const [metric, raw] of Object.entries(inst)) {
        if (!Array.isArray(raw)) continue;
        const arms = conditionExprFromJson(raw);
        if (!arms) continue;
        for (let ai = 0; ai < arms.length; ai++) {
          const arm = arms[ai];
          for (let ci = 0; ci < arm.length; ci++) {
            const c = arm[ci];
            out.push({
              group,
              metric,
              operator: c.operator,
              text: `${metric}${suffix}${c.operator}${formatDecimalTrim(c.value, 4)}`,
              orBefore: ai > 0 && ci === 0,
            });
          }
        }
      }
    }
  }
  return out;
}

function exitChips(exit: RuleParamsJson['exit']): SideChip[] {
  if (Array.isArray(exit)) {
    const out: SideChip[] = [];
    exit.forEach((clause, ci) => {
      if (!clause || typeof clause !== 'object' || Array.isArray(clause)) return;
      const chips = sideChips(clause);
      chips.forEach((c, i) => {
        out.push({
          ...c,
          clauseOrBefore: ci > 0 && i === 0,
          andBefore: i > 0,
          orBefore: c.orBefore,
        });
      });
    });
    return out;
  }
  return sideChips(exit);
}

function parseParams(raw: unknown): {
  take_profit: number | null;
  stop_loss: number | null;
  entry: SideChip[];
  event: SideChip[];
  entryLock: boolean;
  exit: SideChip[];
  scale_out: Array<{ sellPct: number | null; take_profit: number | null; conds: SideChip[] }>;
  reentry: { cooldown_sec: number; max_episodes_per_token: number } | null;
  exclusive: boolean;
  priority: number;
} {
  const p = (raw && typeof raw === 'object' ? raw : {}) as RuleParamsJson;
  const re = p.reentry;
  const reentry =
    re && typeof re.cooldown_sec === 'number' && typeof re.max_episodes_per_token === 'number'
      ? { cooldown_sec: re.cooldown_sec, max_episodes_per_token: re.max_episodes_per_token }
      : null;
  const scale_out = Array.isArray(p.scale_out)
    ? p.scale_out.map((s) => ({
        sellPct:
          typeof s.sell_bps === 'number' && Number.isFinite(s.sell_bps) ? s.sell_bps / 100 : null,
        take_profit: typeof s.take_profit === 'number' ? s.take_profit : null,
        conds: sideChips(s.conditions),
      }))
    : [];
  return {
    take_profit: typeof p.take_profit === 'number' ? p.take_profit : null,
    stop_loss: typeof p.stop_loss === 'number' ? p.stop_loss : null,
    entry: sideChips(p.entry),
    event: sideChips(p.entry_event),
    entryLock: p.entry_lock === 'slot',
    exit: exitChips(p.exit),
    scale_out,
    reentry,
    exclusive: p.exclusive === true,
    priority: typeof p.priority === 'number' ? p.priority : 0,
  };
}

/** One metric-condition chip tinted from the registry hue (+ fixed op shade). */
function MetricCondChip({ chip: c }: { chip: SideChip }) {
  const { data: registry } = useStrategyRegistry();
  const hue = findMetric(registry, c.group, c.metric)?.hue;
  const tint = metricColorStyle({
    hue,
    group: c.group,
    metric: c.metric,
    operator: c.operator as Operator,
  });
  return (
    <>
      {c.clauseOrBefore && (
        <span className="font-mono text-[10px] text-text-dim/70" aria-hidden>
          ∨
        </span>
      )}
      {c.andBefore && (
        <span className="font-mono text-[10px] text-text-dim/70" aria-hidden>
          ∧
        </span>
      )}
      {c.orBefore && (
        <span className="font-mono text-[10px] text-text-dim/70" aria-hidden>
          |
        </span>
      )}
      {chip(c.text, undefined, {
        borderColor: tint.border,
        backgroundColor: tint.background,
        color: tint.color,
      })}
    </>
  );
}

/** Preserve first-seen group order; chips within a group stay contiguous. */
function chipsByGroup(chips: SideChip[]): { group: string; chips: SideChip[] }[] {
  const order: string[] = [];
  const map = new Map<string, SideChip[]>();
  for (const c of chips) {
    let bucket = map.get(c.group);
    if (!bucket) {
      bucket = [];
      map.set(c.group, bucket);
      order.push(c.group);
    }
    bucket.push(c);
  }
  return order.map((group) => ({ group, chips: map.get(group)! }));
}

function isDnfChips(chips: SideChip[]): boolean {
  return chips.some((c) => c.andBefore || c.clauseOrBefore);
}

function SideBlock({
  side,
  chips,
  labelCls,
  lock,
}: {
  side: 'in' | 'out' | 'event';
  chips: SideChip[];
  labelCls: string;
  lock?: boolean;
}): ReactNode {
  if (chips.length === 0) return null;
  const label = side === 'event' ? (lock ? 'evt/s' : 'evt') : side;
  const labelTitle =
    side === 'event'
      ? lock
        ? 'Completing-print event, once per slot'
        : 'Completing-print event, every print'
      : undefined;
  // Array-form DNF spans groups inside a way. Grouping by group would pull
  // liquidity from two clauses onto one row and park ∧ / ∨ on the wrong chip.
  if (isDnfChips(chips)) {
    return (
      <div className="grid grid-cols-[2rem_1fr] items-start gap-x-1.5">
        <span className={cn('pt-0.5 text-[9px] uppercase leading-tight', labelCls)} title={labelTitle}>
          {label}
        </span>
        <div className="flex flex-wrap items-center gap-1">
          {chips.map((c, i) => (
            <Fragment key={`${side}-dnf-${c.group}-${i}`}>
              <span className="pt-0.5 text-[10px] leading-tight text-text-dim" title={c.group}>
                {c.group}
              </span>
              <MetricCondChip chip={c} />
            </Fragment>
          ))}
        </div>
      </div>
    );
  }
  return (
    <div className="grid grid-cols-[2rem_auto_1fr] items-start gap-x-1.5 gap-y-0.5">
      {chipsByGroup(chips).map(({ group, chips: groupChips }, gi) => (
        <Fragment key={`${side}-${group}`}>
          <span className={cn('pt-0.5 text-[9px] uppercase leading-tight', labelCls)} title={labelTitle}>
            {gi === 0 ? label : ''}
          </span>
          <span className="pt-0.5 text-[10px] leading-tight text-text-dim" title={group}>
            {group}
          </span>
          <div className="flex flex-wrap items-center gap-1">
            {groupChips.map((c, i) => (
              <MetricCondChip key={`${side}-${group}-${i}`} chip={c} />
            ))}
          </div>
        </Fragment>
      ))}
    </div>
  );
}

/** Compact chip cluster for a rule's / combo's `RuleParams`.
 *  Metric conditions stack one row per metric group; side label on the first
 *  row only so later groups stay indented under it. */
export function ruleParamsCell(raw: unknown): ReactNode {
  const { take_profit, stop_loss, entry, event, entryLock, exit, scale_out, reentry, exclusive, priority } =
    parseParams(raw);
  const hasTpsl = take_profit != null || stop_loss != null;
  const hasFlags = reentry != null || exclusive;
  const hasScale = scale_out.length > 0;
  const empty =
    !hasTpsl && !hasFlags && !hasScale && entry.length === 0 && event.length === 0 && exit.length === 0;
  return (
    <div className="flex flex-col items-start gap-1 text-left">
      {hasTpsl && (
        <div className="flex flex-wrap items-center gap-1">
          {take_profit != null && chip(`TP ${formatDecimalTrim(take_profit, 1)}%`, 'text-green')}
          {stop_loss != null && chip(`SL ${formatDecimalTrim(stop_loss, 1)}%`, 'text-red')}
        </div>
      )}
      {hasFlags && (
        <div className="flex flex-wrap items-center gap-1">
          {reentry != null &&
            chip(
              `Re-entry ${formatDecimalTrim(reentry.cooldown_sec, 1)}s/${reentry.max_episodes_per_token}`,
              'text-accent',
            )}
          {exclusive && chip(`Exclusive P${priority}`, 'text-warning')}
        </div>
      )}
      {hasScale && (
        <div className="flex flex-wrap items-center gap-1">
          {scale_out.map((s, i) => {
            const pct =
              s.sellPct != null ? `${formatDecimalTrim(s.sellPct, 0)}%` : 'rem';
            const tp =
              s.take_profit != null ? `@TP${formatDecimalTrim(s.take_profit, 0)}` : '';
            return (
              <Fragment key={`scale-${i}`}>
                {chip(`Scale ${pct}${tp}`, 'text-accent')}
              </Fragment>
            );
          })}
        </div>
      )}
      <SideBlock side="event" chips={event} labelCls="text-accent" lock={entryLock} />
      <SideBlock side="in" chips={entry} labelCls="text-accent/70" />
      <SideBlock side="out" chips={exit} labelCls="text-warning/70" />
      {empty && chip('fingerprint only', 'text-text-dim')}
    </div>
  );
}

function headerSep(): ReactNode {
  return (
    <span className="px-0.5 font-mono text-[10px] text-text-dim/35" aria-hidden>
      ·
    </span>
  );
}

function headerSideChips(
  side: string,
  chips: SideChip[],
  labelCls = side === 'out' ? 'text-warning/70' : 'text-accent/70',
): ReactNode[] {
  if (chips.length === 0) return [];
  const out: ReactNode[] = [];
  let sideShown = false;
  for (let i = 0; i < chips.length; i++) {
    const c = chips[i];
    out.push(
      <Fragment key={`${side}-${c.group}-${i}`}>
        {c.clauseOrBefore && (
          <span className="font-mono text-[10px] text-text-dim/70" aria-hidden>
            ∨
          </span>
        )}
        {c.andBefore && (
          <span className="font-mono text-[10px] text-text-dim/70" aria-hidden>
            ∧
          </span>
        )}
        {c.orBefore && (
          <span className="font-mono text-[10px] text-text-dim/70" aria-hidden>
            |
          </span>
        )}
        {!sideShown && (
          <span className={cn('text-[9px] font-bold uppercase leading-none', labelCls)}>{side}</span>
        )}
        <span className="font-mono text-[10px] text-text-dim/80" title={c.group}>
          {c.group}
        </span>
        <MetricCondChip
          chip={{ ...c, orBefore: false, andBefore: false, clauseOrBefore: false }}
        />
      </Fragment>,
    );
    sideShown = true;
  }
  return out;
}

/** Single-row chip strip for modal headers — same facts as {@link ruleParamsCell}, laid out
 *  horizontally so params stay visible above a chart. */
export function ruleParamsHeaderStrip(raw: unknown): ReactNode {
  const { take_profit, stop_loss, entry, event, entryLock, exit, scale_out, reentry, exclusive, priority } =
    parseParams(raw);
  const nodes: ReactNode[] = [];
  let needsSep = false;
  const push = (node: ReactNode) => {
    if (needsSep) nodes.push(<Fragment key={`sep-${nodes.length}`}>{headerSep()}</Fragment>);
    nodes.push(node);
    needsSep = true;
  };

  if (take_profit != null) {
    push(chip(`TP ${formatDecimalTrim(take_profit, 1)}%`, 'text-green'));
  }
  if (stop_loss != null) {
    push(chip(`SL ${formatDecimalTrim(stop_loss, 1)}%`, 'text-red'));
  }
  if (reentry != null) {
    push(
      chip(
        `Re ${formatDecimalTrim(reentry.cooldown_sec, 1)}s/${reentry.max_episodes_per_token}`,
        'text-accent',
      ),
    );
  }
  if (exclusive) push(chip(`Excl P${priority}`, 'text-warning'));
  for (const [i, s] of scale_out.entries()) {
    const pct = s.sellPct != null ? `${formatDecimalTrim(s.sellPct, 0)}%` : 'rem';
    const tp = s.take_profit != null ? `@TP${formatDecimalTrim(s.take_profit, 0)}` : '';
    push(<Fragment key={`scale-${i}`}>{chip(`Scale ${pct}${tp}`, 'text-accent')}</Fragment>);
  }

  const condNodes = [
    ...headerSideChips(entryLock ? 'evt/s' : 'evt', event, 'text-accent'),
    ...headerSideChips('in', entry),
    ...headerSideChips('out', exit),
  ];
  if (condNodes.length > 0) {
    if (needsSep) nodes.push(<Fragment key={`sep-${nodes.length}`}>{headerSep()}</Fragment>);
    nodes.push(
      <div key="conds" className="flex flex-wrap items-center gap-1">
        {condNodes}
      </div>,
    );
  }

  if (nodes.length === 0) {
    return chip('fingerprint only', 'text-text-dim');
  }

  return <div className="flex flex-wrap items-center gap-1">{nodes}</div>;
}

/** Flat searchable text for table filters (metric names, ops, TP/SL). */
export function ruleParamsSearchText(raw: unknown): string {
  const { take_profit, stop_loss, entry, event, entryLock, exit, scale_out, reentry, exclusive, priority } =
    parseParams(raw);
  const parts: string[] = [];
  if (take_profit != null) parts.push(`TP ${formatDecimalTrim(take_profit, 1)}%`);
  if (stop_loss != null) parts.push(`SL ${formatDecimalTrim(stop_loss, 1)}%`);
  if (reentry != null) {
    parts.push(`Re-entry ${formatDecimalTrim(reentry.cooldown_sec, 1)}s/${reentry.max_episodes_per_token}`);
  }
  if (exclusive) parts.push(`Exclusive P${priority}`);
  for (const s of scale_out) {
    const pct = s.sellPct != null ? `${formatDecimalTrim(s.sellPct, 0)}%` : 'rem';
    parts.push(`Scale ${pct}`);
  }
  for (const c of event) {
    parts.push(`${entryLock ? 'evt/s' : 'evt'} ${c.text}`);
  }
  for (const c of entry) {
    if (c.orBefore) parts.push('|');
    parts.push(`in ${c.text}`);
  }
  for (const c of exit) {
    if (c.orBefore) parts.push('|');
    parts.push(`out ${c.text}`);
  }
  if (parts.length === 0) return 'fingerprint only';
  return parts.join(' ');
}

/** Numeric sort keys for the params multi-sort header (null = unset / empty). */
export function ruleParamsSortParts(raw: unknown): {
  take_profit: number | null;
  stop_loss: number | null;
  entry_count: number | null;
  exit_count: number | null;
} {
  const { take_profit, stop_loss, entry, event, exit } = parseParams(raw);
  const buy = entry.length + event.length;
  return {
    take_profit,
    stop_loss,
    entry_count: buy > 0 ? buy : null,
    exit_count: exit.length > 0 ? exit.length : null,
  };
}
