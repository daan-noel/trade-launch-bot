// The span box stays free text. These are the spellings that box accepts, and
// the one line that says what the current text means. The written examples come
// from the registry (`10s`, `20sl`, `5p`, `10s@2`, `age60s`).

import type { MetricRef } from './metricRef';
import { parseSpan } from './metricRef';
import { spanKind, type MetricSpec, type StrategyRegistry } from './registry';
import { spanPhrase } from './sentences';
import { formatWindowSpec, parseWindowSpec, type WindowSpec } from './windowSpec';

/** One spelling a click writes into the span box or the slice box. */
export interface SpanSpell {
  key: string;
  /** Written into the box. Empty clears the span: the coin's whole life. */
  write: string;
  /** The mono token on the button. */
  face: string;
  /** Short meaning beside the token. */
  label: string;
  /** Registry summary and example, for the hover. */
  detail?: string;
  target: 'span' | 'slice';
}

const MEANING: Record<string, string> = {
  life: 'whole life',
  sec: 'last seconds',
  slot: 'last slots, 1 = this slot',
  print: 'last prints, 1 = this print',
  lag: 'ending earlier',
  since_age: 'from that age until now',
  age0: 'since creation',
};

const SLICE_SPELLS: SpanSpell[] = [
  { key: 'slice-sec', write: '2s', face: '2s', label: 'inside seconds', target: 'slice' },
  { key: 'slice-slot', write: '4sl', face: '4sl', label: 'inside slots', target: 'slice' },
  { key: 'slice-print', write: '2p', face: '2p', label: 'inside prints', target: 'slice' },
];

function detailOf(reg: StrategyRegistry | undefined, key: string): string | undefined {
  const s = spanKind(reg, key);
  if (!s) return undefined;
  return s.example ? `${s.summary} e.g. ${s.example}` : s.summary;
}

/** The spellings this metric's span box accepts, in the order they are offered. */
export function spanSpells(reg: StrategyRegistry | undefined, spec: MetricSpec): { span: SpanSpell[]; slice: SpanSpell[] } {
  const span: SpanSpell[] = [];
  if (spec.spans.life) {
    span.push({ key: 'life', write: '', face: '(empty)', label: MEANING.life, detail: detailOf(reg, 'life'), target: 'span' });
  }
  if (spec.spans.window) {
    for (const key of ['sec', 'slot', 'print', 'lag'] as const) {
      const s = spanKind(reg, key);
      if (!s?.text) continue;
      span.push({ key, write: s.text, face: s.text, label: MEANING[key], detail: detailOf(reg, key), target: 'span' });
    }
  }
  if (spec.spans.since_age) {
    const text = spanKind(reg, 'since_age')?.text || 'age60s';
    span.push({
      key: 'since_age',
      write: text,
      face: text,
      label: MEANING.since_age,
      detail: detailOf(reg, 'since_age'),
      target: 'span',
    });
    span.push({ key: 'age0', write: 'age0s', face: 'age0s', label: MEANING.age0, detail: 'age0s counts from creation.', target: 'span' });
  }
  const slice = spec.spans.slice
    ? SLICE_SPELLS.map((s) => ({ ...s, detail: detailOf(reg, 'slice') }))
    : [];
  return { span, slice };
}

/** What the current span text means, or why it cannot be saved. */
export function spanReading(spec: MetricSpec, r: Pick<MetricRef, 'span' | 'slice'>): { bad: boolean; text: string } {
  const m = parseSpan(r.span, r.slice);
  if (typeof m === 'string') return { bad: true, text: m };
  if (m.kind === 'since_age' && !spec.spans.since_age) return { bad: true, text: 'this metric does not take since-age' };
  if (m.kind === 'life') {
    if (!spec.spans.life) return { bad: true, text: spec.spans.since_age ? 'write age60s, or age0s for since creation' : 'write a window, e.g. 10s' };
  }
  if (m.kind === 'window') {
    if (!spec.spans.window) return { bad: true, text: 'this metric counts the whole life' };
    if (spec.spans.slice && !m.slice) return { bad: true, text: 'needs a slice beside it, e.g. 2s. Same unit, shorter, no @' };
    if (!spec.spans.slice && m.slice) return { bad: true, text: 'this metric takes no slice' };
  }
  const phrase = spanPhrase(spec, r).trim();
  if (m.kind === 'life') return { bad: false, text: phrase || "the coin's whole life" };
  return { bad: false, text: phrase };
}

function alignSpan(span: string | undefined, slice: WindowSpec): string {
  const w = span ? parseWindowSpec(span) : null;
  return formatWindowSpec({ size: Math.max(w?.size ?? 30, slice.size), lag: w?.lag ?? 0, unit: slice.unit });
}

/** The span / slice pair a spelling writes. A slice click keeps the span's size and lag and switches its unit. */
export function applySpanSpell(spec: MetricSpec, r: MetricRef, spell: SpanSpell): Pick<MetricRef, 'span' | 'slice'> {
  if (spell.target === 'slice') {
    const sl = parseWindowSpec(spell.write);
    const span = sl ? alignSpan(r.span, sl) : r.span;
    return { span, slice: spell.write };
  }
  if (!spell.write) return {};
  if (spell.key === 'since_age' || spell.key === 'age0') return { span: spell.write };
  if (!spec.spans.slice) return { span: spell.write };
  const w = parseWindowSpec(spell.write);
  if (!w) return { span: spell.write };
  const cur = r.slice ? parseWindowSpec(r.slice) : null;
  const size = cur && cur.unit === w.unit && cur.size <= w.size ? cur.size : Math.min(2, w.size);
  return { span: spell.write, slice: formatWindowSpec({ size, lag: 0, unit: w.unit }) };
}
