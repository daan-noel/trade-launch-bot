// Compact at-a-glance chip cluster for a rule's `params` (TP/SL + entry/exit
// metric conditions). Shared by Rules, Simulate, and the generic sweep tables —
// one SSOT so every surface that shows a rule reads the same.

import { Fragment, type CSSProperties, type ReactNode } from 'react';
import { cn } from 'lib/cn';
import { formatDecimalTrim } from 'utils/format';
import { findMetric, useStrategyRegistry, type Operator } from 'lib/strategy/registry';
import { metricColorStyle } from 'lib/strategy/metricColors';

/** One `{operator, value}` condition (the wire shape). */
interface Cond {
  operator: string;
  value: number;
}

/** Nested `params` blob as stored on `strategy_rules.params` / sweep combos. */
interface RuleParamsJson {
  take_profit?: number | null;
  stop_loss?: number | null;
  entry?: Record<string, Record<string, unknown>>;
  exit?: Record<string, Record<string, unknown>>;
}

interface SideChip {
  group: string;
  metric: string;
  operator: string;
  text: string;
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

/** Terse chips for one side's metric conditions, e.g. `time>10`, `net_flow(10s)>5`. */
function sideChips(side: Record<string, Record<string, unknown>> | undefined): SideChip[] {
  if (!side) return [];
  const out: SideChip[] = [];
  for (const [group, body] of Object.entries(side)) {
    const window = typeof body.window_size_sec === 'number' ? body.window_size_sec : null;
    const suffix = window != null ? `(${window}s)` : '';
    for (const [metric, conds] of Object.entries(body)) {
      if (metric === 'window_size_sec' || !Array.isArray(conds)) continue;
      for (const c of conds as Cond[]) {
        if (typeof c?.operator !== 'string' || typeof c?.value !== 'number') continue;
        out.push({
          group,
          metric,
          operator: c.operator,
          text: `${metric}${suffix}${c.operator}${formatDecimalTrim(c.value, 4)}`,
        });
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
} {
  const p = (raw && typeof raw === 'object' ? raw : {}) as RuleParamsJson;
  return {
    take_profit: typeof p.take_profit === 'number' ? p.take_profit : null,
    stop_loss: typeof p.stop_loss === 'number' ? p.stop_loss : null,
    entry: sideChips(p.entry),
    exit: sideChips(p.exit),
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
  return chip(c.text, undefined, {
    borderColor: tint.border,
    backgroundColor: tint.background,
    color: tint.color,
  });
}

/** Compact chip cluster for a rule's / combo's `RuleParams`. */
export function ruleParamsCell(raw: unknown): ReactNode {
  const { take_profit, stop_loss, entry, exit } = parseParams(raw);
  return (
    <div className="flex flex-wrap items-center gap-1 text-left">
      {take_profit != null && chip(`TP ${formatDecimalTrim(take_profit, 1)}%`, 'text-green')}
      {stop_loss != null && chip(`SL ${formatDecimalTrim(stop_loss, 1)}%`, 'text-red')}
      {entry.length > 0 && (
        <>
          <span className="text-[9px] uppercase text-accent/70">in</span>
          {entry.map((c, i) => (
            <Fragment key={`e${i}`}>
              <MetricCondChip chip={c} />
            </Fragment>
          ))}
        </>
      )}
      {exit.length > 0 && (
        <>
          <span className="text-[9px] uppercase text-warning/70">out</span>
          {exit.map((c, i) => (
            <Fragment key={`x${i}`}>
              <MetricCondChip chip={c} />
            </Fragment>
          ))}
        </>
      )}
      {take_profit == null && stop_loss == null && entry.length === 0 && exit.length === 0 &&
        chip('fingerprint only', 'text-text-dim')}
    </div>
  );
}

/** Flat searchable text for table filters (metric names, ops, TP/SL). */
export function ruleParamsSearchText(raw: unknown): string {
  const { take_profit, stop_loss, entry, exit } = parseParams(raw);
  const parts: string[] = [];
  if (take_profit != null) parts.push(`TP ${formatDecimalTrim(take_profit, 1)}%`);
  if (stop_loss != null) parts.push(`SL ${formatDecimalTrim(stop_loss, 1)}%`);
  for (const c of entry) parts.push(`in ${c.text}`);
  for (const c of exit) parts.push(`out ${c.text}`);
  if (parts.length === 0) return 'fingerprint only';
  return parts.join(' ');
}
