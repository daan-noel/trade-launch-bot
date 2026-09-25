// Plain-English sentences for conditions, lines and stages, written from the
// registry's one definition per metric (`phrase`, `unit`, `tag_level`):
//
//   m_flow.buy_sol @!volume [10s] >= 2
//   -> SOL bought (trades without `volume`) in the last 10 s is at least 2 SOL
//
// Every surface that explains a rule (the editor under each row, the readout, the
// Guide page examples) goes through here, so a rule reads the same everywhere.

import type { Condition, ConditionExpr } from './grammar';
import { formatMetricThreshold, parseSpan, parseTagRef, refLabel, type MetricRef } from './metricRef';
import { findMetric, type MetricSpec, type MetricUnit, type StrategyRegistry } from './registry';
import type { Cond, Deadline, Line, MetricCond, Stage } from './ruleDoc';
import { unitLabel } from './windowSpec';

const OP_WORDS: Record<Condition['operator'], string> = {
  '>=': 'at least',
  '>': 'above',
  '<=': 'at most',
  '<': 'below',
  '=': '',
  '!=': 'not',
};

/** A value with its unit, for prose: `2 SOL`, `20 s`, `50 %`, `3`. */
export function valueWithUnit(v: number, unit: MetricUnit): string {
  const n = formatMetricThreshold(v);
  switch (unit) {
    case 'sol':
      return `${n} SOL`;
    case 'seconds':
      return `${n} s`;
    case 'percent':
      return `${n} %`;
    case 'lamports':
      return `${n} lamports`;
    default:
      return n;
  }
}

function atom(c: Condition, unit: MetricUnit): string {
  const w = OP_WORDS[c.operator];
  const v = valueWithUnit(c.value, unit);
  return w ? `${w} ${v}` : v;
}

/** `is at least 2 SOL and below 5 SOL`, `is below 1 or at least 5`; a flag reads
 *  `yes` / `no` when the expression is exactly `= 1` / `= 0`. */
export function isPhrase(is: ConditionExpr, unit: MetricUnit): string {
  if (is.length === 0) return 'is ...';
  if (unit === 'flag' && is.length === 1 && is[0].length === 1) {
    const c = is[0][0];
    const yes = (c.operator === '=' && c.value === 1) || (c.operator === '>=' && c.value === 1) || (c.operator === '!=' && c.value === 0);
    const no = (c.operator === '=' && c.value === 0) || (c.operator === '<' && c.value === 1) || (c.operator === '!=' && c.value === 1);
    if (yes) return ': yes';
    if (no) return ': no';
  }
  return `is ${is.map((arm) => arm.map((c) => atom(c, unit)).join(' and ')).join(', or ')}`;
}

/** ` (trades without \`volume\`)`, ` (templates in \`working\`)`, ` (\`bundled\` wallets)`. */
export function tagPhrase(spec: MetricSpec | undefined, tag: string | undefined): string {
  if (!tag) return '';
  const { name, negated } = parseTagRef(tag);
  if (spec?.tag_level === 'wallet_class') return ` (\`${name}\` wallets)`;
  if (spec?.tag_level === 'template') return ` (templates in \`${name}\`)`;
  return negated ? ` (trades without \`${name}\`)` : ` (trades with \`${name}\`)`;
}

/** ` in the last 10 s`, ` in this slot`, ` in the 10 s ending 2 s ago`, ` since age 60 s`.
 *  A life read says so only when the metric can also take a window, so the difference
 *  is visible. */
export function spanPhrase(spec: MetricSpec | undefined, r: Pick<MetricRef, 'span' | 'slice'>): string {
  const m = parseSpan(r.span, r.slice);
  if (typeof m === 'string') return ` [${r.span ?? ''}]`;
  if (m.kind === 'life') return spec?.spans.window ? " over the coin's life" : '';
  if (m.kind === 'since_age') return m.secs === 0 ? ' since creation' : ` since age ${formatMetricThreshold(m.secs)} s`;
  const w = m.window;
  const size = formatMetricThreshold(w.size);
  const unitWord = w.unit === 'sec' ? 's' : unitLabel(w.unit);
  let text: string;
  if (w.lag > 0) {
    text = ` in the ${size} ${unitWord} ending ${formatMetricThreshold(w.lag)} ${w.unit === 'sec' ? 's' : unitLabel(w.unit)} ago`;
  } else if (w.size === 1 && w.unit === 'slot') {
    text = ' in this slot';
  } else if (w.size === 1 && w.unit === 'print') {
    text = ' on this print';
  } else {
    text = ` in the last ${size} ${unitWord}`;
  }
  if (m.slice) text += `, of which the last ${formatMetricThreshold(m.slice.size)} ${m.slice.unit === 'sec' ? 's' : unitLabel(m.slice.unit)}`;
  return text;
}

/** The read without the judgement: `SOL bought (trades without \`volume\`) in the last 10 s`. */
export function readPhrase(reg: StrategyRegistry | undefined, r: MetricRef): string {
  const spec = findMetric(reg, r.metric);
  const phrase = spec?.phrase || r.metric;
  return `${phrase}${tagPhrase(spec, r.tag)}${spanPhrase(spec, r)}`;
}

/** One condition as a sentence. */
export function condSentence(reg: StrategyRegistry | undefined, c: Cond): string {
  if (c.kind === 'signal') return c.not ? `\`${c.signal}\` does not hold` : `\`${c.signal}\` holds`;
  const spec = findMetric(reg, c.ref.metric);
  const is = isPhrase(c.is, spec?.unit ?? 'count');
  return `${readPhrase(reg, c.ref)}${is.startsWith(':') ? is : ` ${is}`}`;
}

/** AND of conditions: `A, and B, and C`. Off conditions are left out. */
export function condsSentence(reg: StrategyRegistry | undefined, cs: Cond[]): string {
  const live = cs.filter((c) => !c.off);
  return live.length ? live.map((c) => condSentence(reg, c)).join(', and ') : 'always';
}

/** The label the engine gives a line that sells with no label of its own: its first
 *  live condition, `m_flow.buy_sol @!volume [10s] >= 2` (engine `auto_label`). */
export function autoLineLabel(l: Line): string {
  for (const c of l.if) {
    if (c.off) continue;
    if (c.kind === 'signal') return c.not ? `not ${c.signal}` : c.signal;
    const first = c.is[0]?.[0];
    return first ? `${refLabel(c.ref)} ${first.operator} ${formatMetricThreshold(first.value)}` : refLabel(c.ref);
  }
  return 'rule';
}

/** The exit reason a line's sell books. */
export function lineExitLabel(l: Line): string {
  return l.sell?.label.trim() || autoLineLabel(l);
}

/** What a line does: `sell everything as "burst"`, `sell 50 % of the first bag, then go to ride`. */
export function actionSentence(l: Line): string {
  const parts: string[] = [];
  if (l.sell) {
    const what = l.sell.pct != null ? `sell ${formatMetricThreshold(l.sell.pct)} % of the first bag` : 'sell everything';
    parts.push(`${what} as "${lineExitLabel(l)}"`);
  }
  if (l.go) parts.push(`go to ${l.go}`);
  return parts.join(', then ') || 'do nothing';
}

/** A whole line: `If A, and B: sell everything as "burst".` */
export function lineSentence(reg: StrategyRegistry | undefined, l: Line): string {
  const when = l.if.filter((c) => !c.off).length ? `If ${condsSentence(reg, l.if)}` : 'Always';
  return `${when}: ${actionSentence(l)}.`;
}

/** `at coin age 20 s`, `after 30 s held`, `after 30 s in this stage`. */
export function deadlineSentence(d: Deadline): string {
  const s = formatMetricThreshold(d.secs);
  switch (d.basis) {
    case 'age_sec':
      return `when the coin is ${s} s old`;
    case 'held_sec':
      return `${s} s after our buy`;
    case 'stage_sec':
      return `${s} s after this stage began`;
  }
}

/** Where a stage leads at its deadline. */
export function stageNext(stages: Stage[], i: number): string | null {
  const s = stages[i];
  if (!s.ends) return null;
  return s.then ?? stages[i + 1]?.name ?? null;
}

/** `early (ends when the coin is 20 s old) -> late -> ride (ends 30 s after this stage began)`. */
export function stageStrip(stages: Stage[]): string {
  return stages
    .map((s) => (s.ends ? `${s.name} (ends ${deadlineSentence(s.ends)})` : s.name))
    .join(' -> ');
}

/** A metric condition's short chip text: `m_flow.buy_sol @!volume [10s] >= 2`. */
export function condLabel(c: MetricCond): string {
  const is = c.is.map((arm) => arm.map((x) => `${x.operator} ${formatMetricThreshold(x.value)}`).join(', ')).join(' | ');
  return `${refLabel(c.ref)} ${is}`.trim();
}
