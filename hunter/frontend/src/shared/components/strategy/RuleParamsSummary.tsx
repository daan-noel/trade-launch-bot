// Compact at-a-glance summary of a rule's `params` (format 2): the buy conditions,
// then each sell line as `conditions -> action`, stage by stage. Shared by Rules,
// Simulate, Fingerprints (used-by) and the search tables, so every surface that shows
// a rule reads the same. The full sentences live in `rule/RuleSentences.tsx`.

import { Fragment, type CSSProperties, type ReactNode } from 'react';
import { cn } from 'lib/cn';
import { formatDecimalTrim } from 'utils/format';
import { familyName, findMetric, useStrategyRegistry, type Operator } from 'lib/strategy/registry';
import { metricColorStyle } from 'lib/strategy/metricColors';
import { refLabel } from 'lib/strategy/metricRef';
import { ruleDocFromJson, type Cond, type Line, type RuleDoc } from 'lib/strategy/ruleDoc';
import { condLabel, condSentence, deadlineSentence, lineExitLabel } from 'lib/strategy/sentences';

function chip(text: ReactNode, cls?: string, style?: CSSProperties, title?: string): ReactNode {
  return (
    <span
      className={cn(
        'inline-block rounded border border-white/10 bg-surface px-1.5 py-0.5 font-mono text-[11px] leading-tight',
        cls,
      )}
      style={style}
      title={title}
    >
      {text}
    </span>
  );
}

/** The rule, or why it cannot be read (a format-1 document from an old sweep combo). */
function parse(raw: unknown): { doc: RuleDoc } | { error: string } {
  try {
    return { doc: ruleDocFromJson(raw ?? {}) };
  } catch (e) {
    return { error: e instanceof Error ? e.message : String(e) };
  }
}

function CondChip({ c }: { c: Cond }) {
  const { data: reg } = useStrategyRegistry();
  if (c.kind === 'signal') {
    return <>{chip(c.not ? `not ${c.signal}` : c.signal, cn('text-accent', c.off && 'line-through opacity-50'), undefined, condSentence(reg, c))}</>;
  }
  const spec = findMetric(reg, c.ref.metric);
  const tint = metricColorStyle({
    hue: spec?.hue,
    group: familyName(c.ref.metric),
    metric: spec?.name ?? c.ref.metric,
    operator: c.is[0]?.[0]?.operator as Operator | undefined,
  });
  return <>{chip(condLabel(c), cn(c.off && 'line-through opacity-50'), tint.style, condSentence(reg, c))}</>;
}

function Conds({ conds }: { conds: Cond[] }) {
  return (
    <>
      {conds.map((c, i) => (
        <Fragment key={c.id}>
          {i > 0 && <span className="font-mono text-[10px] text-text-dim/70">∧</span>}
          <CondChip c={c} />
        </Fragment>
      ))}
    </>
  );
}

function actionChip(l: Line): ReactNode {
  const parts: string[] = [];
  if (l.sell) parts.push(`${l.sell.pct != null ? `sell ${formatDecimalTrim(l.sell.pct, 1)}%` : 'sell'} "${lineExitLabel(l)}"`);
  if (l.go) parts.push(`→ ${l.go}`);
  return chip(parts.join(' '), l.sell ? 'text-warning' : 'text-accent');
}

function LineRow({ l, tag }: { l: Line; tag: string }) {
  return (
    <div className={cn('flex flex-wrap items-center gap-1', l.off && 'opacity-50')}>
      <span className="text-[9px] font-bold uppercase text-warning/70">{tag}</span>
      {l.if.length ? <Conds conds={l.if} /> : chip('always', 'text-text-dim')}
      <span className="font-mono text-[10px] text-text-dim">⇒</span>
      {actionChip(l)}
    </div>
  );
}

function BuyRow({ label, conds, cls }: { label: string; conds: Cond[]; cls: string }) {
  if (!conds.length) return null;
  return (
    <div className="flex flex-wrap items-center gap-1">
      <span className={cn('text-[9px] font-bold uppercase', cls)}>{label}</span>
      <Conds conds={conds} />
    </div>
  );
}

function headline(doc: RuleDoc): ReactNode[] {
  const out: ReactNode[] = [];
  if (doc.stop_loss != null) out.push(chip(`SL ${formatDecimalTrim(doc.stop_loss, 1)}%`, 'text-red'));
  if (doc.take_profit != null) out.push(chip(`TP ${formatDecimalTrim(doc.take_profit, 1)}%`, 'text-green'));
  if (doc.enter.size_pct_of_pool != null) out.push(chip(`size ${formatDecimalTrim(doc.enter.size_pct_of_pool, 2)}% pool`, 'text-accent'));
  if (doc.reentry) out.push(chip(`again ${formatDecimalTrim(doc.reentry.cooldown_sec, 1)}s ×${doc.reentry.max_per_coin}`, 'text-accent'));
  if (doc.exclusive) out.push(chip(`exclusive P${doc.priority}`, 'text-warning'));
  return out;
}

/** Compact summary for a rule's / combo's `params`: one row per buy part, then one
 *  row per sell line (`always`, then each stage). */
export function ruleParamsCell(raw: unknown): ReactNode {
  const p = parse(raw);
  if ('error' in p) return chip('format-1 params', 'text-text-dim', undefined, p.error);
  const { doc } = p;
  const head = headline(doc);
  const e = doc.enter;
  const empty = !head.length && !e.event.length && !e.filters.length && !e.final_filters.length && !doc.always.length && !doc.stages.length;
  return (
    <div className="flex flex-col items-start gap-1 text-left">
      {head.length > 0 && (
        <div className="flex flex-wrap items-center gap-1">
          {head.map((h, i) => (
            <Fragment key={i}>{h}</Fragment>
          ))}
        </div>
      )}
      <BuyRow label={e.lock ? `on/${e.lock}` : 'on'} conds={e.event} cls="text-accent" />
      <BuyRow label="if" conds={e.filters} cls="text-accent/70" />
      <BuyRow label="if/quit" conds={e.final_filters} cls="text-accent/70" />
      {doc.signals.map((s) => (
        <div key={s.id} className="flex flex-wrap items-center gap-1">
          <span className="text-[9px] font-bold uppercase text-accent">{s.name} =</span>
          {s.groups.map((g, gi) => (
            <Fragment key={gi}>
              {gi > 0 && <span className="font-mono text-[10px] text-text-dim/70">∨</span>}
              <Conds conds={g} />
            </Fragment>
          ))}
        </div>
      ))}
      {doc.always.map((l) => (
        <LineRow key={l.id} l={l} tag="always" />
      ))}
      {doc.stages.map((s) => (
        <Fragment key={s.id}>
          <div className="flex flex-wrap items-center gap-1">
            {chip(s.name, 'text-accent', undefined, s.ends ? `ends ${deadlineSentence(s.ends)}` : 'no deadline')}
            {s.ends && <span className="font-mono text-[10px] text-text-dim">⏱ {s.ends.basis.replace('_sec', '')} {formatDecimalTrim(s.ends.secs, 1)}s{s.then ? ` → ${s.then}` : ''}</span>}
          </div>
          {s.on.map((l) => (
            <LineRow key={l.id} l={l} tag={s.name} />
          ))}
          {s.at_end.map((l) => (
            <LineRow key={l.id} l={l} tag="⏱" />
          ))}
        </Fragment>
      ))}
      {empty && chip('fingerprint only', 'text-text-dim')}
    </div>
  );
}

/** Single-row strip for modal headers: the same facts, laid out horizontally. */
export function ruleParamsHeaderStrip(raw: unknown): ReactNode {
  return <div className="max-h-24 overflow-auto">{ruleParamsCell(raw)}</div>;
}

/** Flat searchable text for table filters: every read label, stage and exit label. */
export function ruleParamsSearchText(raw: unknown): string {
  const p = parse(raw);
  if ('error' in p) return 'format-1';
  const { doc } = p;
  const parts: string[] = [];
  if (doc.take_profit != null) parts.push(`TP ${formatDecimalTrim(doc.take_profit, 1)}%`);
  if (doc.stop_loss != null) parts.push(`SL ${formatDecimalTrim(doc.stop_loss, 1)}%`);
  const conds = (cs: Cond[]) => cs.map((c) => (c.kind === 'signal' ? c.signal : condLabel(c)));
  parts.push(...conds(doc.enter.event), ...conds(doc.enter.filters), ...conds(doc.enter.final_filters));
  for (const s of doc.signals) parts.push(s.name, ...s.groups.flat().map((c) => refLabel(c.ref)));
  for (const l of doc.always) parts.push(...conds(l.if), lineExitLabel(l));
  for (const s of doc.stages) {
    parts.push(s.name);
    for (const l of [...s.on, ...s.at_end]) parts.push(...conds(l.if), lineExitLabel(l));
  }
  return parts.length ? parts.join(' ') : 'fingerprint only';
}

/** Numeric sort keys for the params multi-sort header (null = unset / empty). */
export function ruleParamsSortParts(raw: unknown): {
  take_profit: number | null;
  stop_loss: number | null;
  entry_count: number | null;
  exit_count: number | null;
} {
  const p = parse(raw);
  if ('error' in p) return { take_profit: null, stop_loss: null, entry_count: null, exit_count: null };
  const { doc } = p;
  const buy = doc.enter.event.length + doc.enter.filters.length + doc.enter.final_filters.length;
  const sell = doc.always.length + doc.stages.reduce((n, s) => n + s.on.length + s.at_end.length, 0);
  return {
    take_profit: doc.take_profit,
    stop_loss: doc.stop_loss,
    entry_count: buy > 0 ? buy : null,
    exit_count: sell > 0 ? sell : null,
  };
}
