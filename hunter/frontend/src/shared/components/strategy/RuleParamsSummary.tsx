// Compact at-a-glance chip cluster for a rule's `params` (TP/SL + entry/exit
// metric conditions). Shared by Rules, Simulate, Fingerprints (used-by), and the
// generic sweep tables — one SSOT so every surface that shows a rule reads the same.

import { Fragment, type CSSProperties, type ReactNode } from 'react';
import { cn } from 'lib/cn';
import { formatDecimalTrim } from 'utils/format';
import { findMetric, useStrategyRegistry, type Operator } from 'lib/strategy/registry';
import { metricColorStyle } from 'lib/strategy/metricColors';
import { conditionExprFromJson } from 'lib/strategy/grammar';

/** Nested `params` blob as stored on `strategy_rules.params` / sweep combos. A
 *  group's value is a plain object (one window) OR an array of them (one per
 *  window) — the multi-window-per-group wire form. */
interface RuleParamsJson {
  take_profit?: number | null;
  stop_loss?: number | null;
  entry?: Record<string, unknown>;
  exit?: Record<string, unknown>;
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
      const window = typeof inst.window_size_sec === 'number' ? inst.window_size_sec : null;
      const suffix = window != null ? `(${window}s)` : '';
      for (const [metric, raw] of Object.entries(inst)) {
        if (metric === 'window_size_sec' || !Array.isArray(raw)) continue;
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

function parseParams(raw: unknown): {
  take_profit: number | null;
  stop_loss: number | null;
  entry: SideChip[];
  exit: SideChip[];
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
  return {
    take_profit: typeof p.take_profit === 'number' ? p.take_profit : null,
    stop_loss: typeof p.stop_loss === 'number' ? p.stop_loss : null,
    entry: sideChips(p.entry),
    exit: sideChips(p.exit),
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

function SideBlock({
  side,
  chips,
  labelCls,
}: {
  side: 'in' | 'out';
  chips: SideChip[];
  labelCls: string;
}): ReactNode {
  if (chips.length === 0) return null;
  return (
    <div className="grid grid-cols-[1.5rem_auto_1fr] items-start gap-x-1.5 gap-y-0.5">
      {chipsByGroup(chips).map(({ group, chips: groupChips }, gi) => (
        <Fragment key={`${side}-${group}`}>
          <span className={cn('pt-0.5 text-[9px] uppercase leading-tight', labelCls)}>
            {gi === 0 ? side : ''}
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
  const { take_profit, stop_loss, entry, exit, reentry, exclusive, priority } = parseParams(raw);
  const hasTpsl = take_profit != null || stop_loss != null;
  const hasFlags = reentry != null || exclusive;
  const empty = !hasTpsl && !hasFlags && entry.length === 0 && exit.length === 0;
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
      <SideBlock side="in" chips={entry} labelCls="text-accent/70" />
      <SideBlock side="out" chips={exit} labelCls="text-warning/70" />
      {empty && chip('fingerprint only', 'text-text-dim')}
    </div>
  );
}

/** Flat searchable text for table filters (metric names, ops, TP/SL). */
export function ruleParamsSearchText(raw: unknown): string {
  const { take_profit, stop_loss, entry, exit, reentry, exclusive, priority } = parseParams(raw);
  const parts: string[] = [];
  if (take_profit != null) parts.push(`TP ${formatDecimalTrim(take_profit, 1)}%`);
  if (stop_loss != null) parts.push(`SL ${formatDecimalTrim(stop_loss, 1)}%`);
  if (reentry != null) {
    parts.push(`Re-entry ${formatDecimalTrim(reentry.cooldown_sec, 1)}s/${reentry.max_episodes_per_token}`);
  }
  if (exclusive) parts.push(`Exclusive P${priority}`);
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
  const { take_profit, stop_loss, entry, exit } = parseParams(raw);
  return {
    take_profit,
    stop_loss,
    entry_count: entry.length > 0 ? entry.length : null,
    exit_count: exit.length > 0 ? exit.length : null,
  };
}
