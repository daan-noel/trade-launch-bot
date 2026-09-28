// The In words drawing: signals are their own frame. Watch and buy are two
// rows. Buy gates are rows. Sell sits beside them. Again is its own section
// after sell. Sell and go stay on the then line. A gate, a signal, or a
// deadline the rule does not have is absent.

import { formatDecimalTrim } from 'utils/format';
import { entersOnArm, type Cond, type Deadline, type Line, type RuleDoc, type Stage } from './ruleDoc';
import { condFace, stageNext } from './sentences';

export interface FlowRow {
  head: string;
  gate: string;
  text: string;
  /** Metric path, when the row is one read. The view paints that family. */
  metric?: string;
  /** Signal name, when the row names a signal. The view paints the signal color. */
  signal?: string;
  dim?: boolean;
  /** 0 is a step (Watch, Buy, Sell). 1 is a case under it. 2 is a line under that case. */
  depth?: number;
  /** Further reads of one if: same indent, same tree line, no join mark. */
  along?: boolean;
  /** Reserve the if badge so this read lines up with the first. */
  padIf?: boolean;
  /** v sits further right than if, with no extra tree line. */
  tuck?: boolean;
  /** Vertical line for one if/then pair. `open` is if, `mid` a further read, `close` is then. */
  brace?: 'open' | 'mid' | 'close';
  /** Sell and go, drawn on this row. Then carries them; they are not a new line. */
  acts?: { gate: string; text: string }[];
}

/** One sell block inside the sell part: always, or a stage. */
export interface FlowBlock {
  key: string;
  head: string;
  kind: 'always' | 'stage' | 'empty' | 'again' | 'gate';
  /** Each array is one if/then pair, or one single line. */
  pairs: FlowRow[][];
}

/** The drawing: signals, then watch and buy. Sell beside them. Again after sell. */
export interface FlowSplit {
  /** WATCH, then BUY and its lock. The gates are `buy`. */
  entry: FlowRow[];
  buy: FlowBlock[];
  /** Named conditions. A buy gate or a sell line names one. Absent when the rule defines none. */
  signals: FlowBlock[];
  sell: FlowBlock[];
  /** Buy again, after a sell. Absent when the rule does not buy again. */
  again: FlowBlock | null;
}

function live<T extends { off: boolean }>(xs: T[]): T[] {
  return xs.filter((x) => !x.off);
}

function condText(c: Cond): { text: string; metric?: string; signal?: string } {
  if (c.kind === 'signal') return { text: c.not ? `not ${c.signal}` : c.signal, signal: c.signal };
  return { text: condFace(c), metric: c.ref.metric };
}

/** Sell and go belong on the then line. */
function lineActs(l: Line): { gate: string; text: string }[] {
  const acts: { gate: string; text: string }[] = [];
  if (l.sell) {
    const what = l.sell.pct != null ? `${formatDecimalTrim(l.sell.pct, 1)}% of the first bag` : 'all';
    acts.push({ gate: 'sell', text: what });
  }
  if (l.go) acts.push({ gate: 'go', text: l.go });
  if (!acts.length) acts.push({ gate: '', text: 'do nothing' });
  return acts;
}

function lineRows(l: Line, gate: string, depth = 1): FlowRow[] {
  const rows: FlowRow[] = [];
  const cs = live(l.if);
  const acts = lineActs(l);
  if (gate) rows.push({ head: '', gate, text: '', depth });
  const body = gate ? depth + 1 : depth;
  if (!cs.length) {
    if (rows.length) rows[0].acts = acts;
    else rows.push({ head: '', gate: '', text: '', depth: body, acts });
    return rows;
  }
  cs.forEach((c, i) => {
    const face = condText(c);
    rows.push({
      head: '',
      gate: i === 0 ? 'if' : '',
      text: face.text,
      metric: face.metric,
      signal: face.signal,
      depth: body,
      along: i > 0,
      padIf: i > 0,
      brace: i === 0 ? 'open' : 'mid',
    });
  });
  rows.push({ head: '', gate: 'then', text: '', depth: body, brace: 'close', acts });
  return rows;
}

function deadlineFace(d: Deadline): string {
  const s = formatDecimalTrim(d.secs, 1);
  if (d.basis === 'age_sec') return `coin age ${s}s`;
  if (d.basis === 'held_sec') return `held ${s}s`;
  return `${s}s in this stage`;
}

function condPairs(conds: Cond[]): FlowRow[][] {
  return live(conds).map((c) => {
    const face = condText(c);
    return [{ head: '', gate: '', text: face.text, metric: face.metric, signal: face.signal, depth: 0 }];
  });
}

function gateBlock(key: string, head: string, conds: Cond[]): FlowBlock | null {
  const pairs = condPairs(conds);
  if (!pairs.length) return null;
  return { key, head, kind: 'gate', pairs };
}

/** TP, then SL, as rows in one column, ahead of always. */
function bracketBlock(doc: RuleDoc): FlowBlock | null {
  const pairs: FlowRow[][] = [];
  if (doc.take_profit != null) pairs.push([{ head: '', gate: 'TP', text: `+${formatDecimalTrim(doc.take_profit, 1)}%`, depth: 0 }]);
  if (doc.stop_loss != null) pairs.push([{ head: '', gate: 'SL', text: `-${formatDecimalTrim(doc.stop_loss, 1)}%`, depth: 0 }]);
  if (!pairs.length) return null;
  return { key: 'tpsl', head: '', kind: 'gate', pairs };
}

function stageBlock(stages: Stage[], s: Stage, i: number): FlowBlock {
  const pairs: FlowRow[][] = [];
  for (const l of live(s.on)) pairs.push(lineRows(l, '', 0));
  for (const l of live(s.at_end)) pairs.push(lineRows(l, 'at end', 0));
  if (s.ends) {
    const next = stageNext(stages, i) ?? 'stop';
    pairs.push([{ head: '', gate: 'deadline', text: deadlineFace(s.ends), depth: 0, acts: [{ gate: 'go', text: next }] }]);
  }
  if (!pairs.length) pairs.push([{ head: '', gate: '', text: 'no line', dim: true, depth: 0 }]);
  return { key: s.id, head: s.name, kind: 'stage', pairs };
}

/** Signals, then watch and buy. Sell beside them. Again after that. `watch` is the fingerprint name. */
export function flowSplit(doc: RuleDoc, watch = 'the fingerprint'): FlowSplit {
  const entry: FlowRow[] = [{ head: 'WATCH', gate: '', text: watch, depth: 0 }];
  const lock = doc.enter.lock;
  entry.push({ head: 'BUY', gate: lock === 'token' ? 'Once Per Coin' : lock === 'slot' ? 'Once Per Slot' : 'Any Print', text: '', depth: 0 });
  if (doc.exclusive) {
    entry.push({ head: '', gate: '', text: `skip while another rule holds (priority ${doc.priority})`, depth: 1 });
  }

  const buy: FlowBlock[] = [];
  if (entersOnArm(doc)) {
    const text = lock === 'token' ? 'buys on the first print' : lock === 'slot' ? 'buys on the first print of a slot' : 'buys when the coin matches';
    entry.push({ head: '', gate: '', text, depth: 1 });
  } else {
    for (const block of [
      gateBlock('on', 'On', doc.enter.event),
      gateBlock('if', 'Only If', doc.enter.filters),
      gateBlock('give', 'Give Up', doc.enter.final_filters),
    ]) {
      if (block) buy.push(block);
    }
  }

  const signals: FlowBlock[] = [];
  for (const s of doc.signals) {
    const pairs: FlowRow[][] = [];
    s.groups.forEach((g, gi) => {
      if (gi > 0) pairs.push([{ head: '', gate: 'v', text: '', depth: 0 }]);
      pairs.push(...condPairs(g));
    });
    if (pairs.length) signals.push({ key: s.id, head: s.name, kind: 'gate', pairs });
  }

  const sell: FlowBlock[] = [];
  const bracket = bracketBlock(doc);
  if (bracket) sell.push(bracket);
  if (live(doc.always).length) {
    sell.push({
      key: 'always',
      head: 'Always',
      kind: 'always',
      pairs: live(doc.always).map((l) => lineRows(l, '', 0)),
    });
  }
  doc.stages.forEach((s, i) => sell.push(stageBlock(doc.stages, s, i)));
  if (!sell.length) {
    sell.push({
      key: 'none',
      head: '',
      kind: 'empty',
      pairs: [[{ head: '', gate: '', text: 'nothing sells', dim: true, depth: 0 }]],
    });
  }

  const again: FlowBlock | null = doc.reentry
    ? {
        key: 'again',
        head: 'AGAIN',
        kind: 'again',
        pairs: [[
          {
            head: '',
            gate: '',
            text: `wait ${formatDecimalTrim(doc.reentry.cooldown_sec, 1)}s, up to ${doc.reentry.max_per_coin} buys`,
            depth: 0,
          },
        ]],
      }
    : null;
  return { entry, buy, signals, sell, again };
}
