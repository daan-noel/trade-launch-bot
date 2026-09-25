// One metric read, fully named: which metric, whose trades, over what span.
// `m_flow.buy_sol @!volume [10s]`. The frontend mirror of the engine's `MetricRef`
// (`hunter_engine::metrics::metric_ref`) and `Span` (`metrics::span`): the same JSON
// keys, the same label, the same acceptance checks, so an error shows while typing
// rather than at save.

import { findMetric, type MetricSpec, type StrategyRegistry } from './registry';
import {
  formatMetricThreshold,
  formatWindowSpec,
  parseWindowSpec,
  type WindowSpec,
} from './windowSpec';

export { formatMetricThreshold };

/** The `metric` / `tag` / `span` / `slice` keys of a condition. */
export interface MetricRef {
  /** `m_flow.buy_sol`. */
  metric: string;
  /** `volume` (trades with the tag) or `!volume` (trades without it). */
  tag?: string;
  /** Absent = the coin's life. `10s`, `20sl`, `5p`, `10s@2`, `age60s`. */
  span?: string;
  /** The nested window of a `slice_*_share_pct` metric, e.g. `2s`. */
  slice?: string;
}

/** Prefix of a since-age span: `age60s`. */
const AGE_PREFIX = 'age';

/** A span, parsed. */
export type SpanModel =
  | { kind: 'life' }
  | { kind: 'window'; window: WindowSpec; slice: WindowSpec | null }
  | { kind: 'since_age'; secs: number };

/** Parse a `span` / `slice` pair. Returns the error text on a malformed pair, worded
 *  like the engine's. */
export function parseSpan(span?: string, slice?: string): SpanModel | string {
  const s = span?.trim() ?? '';
  const sl = slice?.trim() ?? '';
  let out: SpanModel = { kind: 'life' };
  if (s) {
    if (s.startsWith(AGE_PREFIX)) {
      const rest = s.slice(AGE_PREFIX.length);
      const secs = rest.endsWith('s') ? Number(rest.slice(0, -1).trim()) : NaN;
      if (rest.slice(0, -1).trim() === '' || !Number.isFinite(secs) || secs < 0) {
        return `span \`${s}\`: a since-age span is \`age<seconds>s\`, e.g. \`age60s\``;
      }
      out = { kind: 'since_age', secs };
    } else {
      const w = parseWindowSpec(s);
      if (!w) {
        return `span \`${s}\`: expected \`10s\` (seconds), \`20sl\` (slots), \`5p\` (prints), \`10s@2\` (lagged) or \`age60s\``;
      }
      out = { kind: 'window', window: w, slice: null };
    }
  }
  if (sl) {
    if (out.kind !== 'window') return 'a slice needs a window span beside it';
    const w = parseWindowSpec(sl);
    if (!w) return `slice \`${sl}\`: expected e.g. \`2s\``;
    if (w.unit !== out.window.unit) {
      return `slice \`${sl}\` must count in the span's unit (\`${formatWindowSpec(out.window)}\`)`;
    }
    if (w.lag !== 0) return `slice \`${sl}\` takes the span's lag; write the lag on the span`;
    if (w.size > out.window.size) {
      return `slice \`${sl}\` is wider than its span \`${formatWindowSpec(out.window)}\``;
    }
    out = { ...out, slice: { size: w.size, lag: out.window.lag, unit: w.unit } };
  }
  return out;
}

/** The `span` / `slice` keys for a parsed span (the inverse of {@link parseSpan}). */
export function spanKeys(m: SpanModel): Pick<MetricRef, 'span' | 'slice'> {
  switch (m.kind) {
    case 'life':
      return {};
    case 'since_age':
      return { span: `${AGE_PREFIX}${formatMetricThreshold(m.secs)}s` };
    case 'window':
      return m.slice
        ? { span: formatWindowSpec(m.window), slice: formatWindowSpec({ ...m.slice, lag: 0 }) }
        : { span: formatWindowSpec(m.window) };
  }
}

/** `[10s]`, `[30s, slice 2s]`, `[age60s]`, or empty for life: the one spelling in labels
 *  and exit reasons (engine `Span::bracket`). */
export function spanBracket(r: Pick<MetricRef, 'span' | 'slice'>): string {
  const m = parseSpan(r.span, r.slice);
  if (typeof m === 'string') return r.span ? `[${r.span}]` : '';
  const k = spanKeys(m);
  if (!k.span) return '';
  return k.slice ? `[${k.span}, slice ${k.slice}]` : `[${k.span}]`;
}

/** `@volume` / `@!volume`. */
export function tagAt(tag: string): string {
  return `@${tag}`;
}

/** Split a tag reference: `!volume` -> `{ name: 'volume', negated: true }`. */
export function parseTagRef(tag: string): { name: string; negated: boolean } {
  return tag.startsWith('!') ? { name: tag.slice(1), negated: true } : { name: tag, negated: false };
}

/** `m_flow.buy_sol @!volume [10s]`: the engine's `MetricRef::label`, character for
 *  character, so a stored exit reason and an editor label agree. */
export function refLabel(r: MetricRef): string {
  let s = r.metric;
  if (r.tag) s += ` ${tagAt(r.tag)}`;
  const b = spanBracket(r);
  if (b) s += ` ${b}`;
  return s;
}

/** Identity of a read: two conditions with the same key read the same number. */
export function refKey(r: MetricRef): string {
  return refLabel(r);
}

/** The built-in wallet class names (`bundled`, `public_app`). */
export function builtinTagNames(reg: StrategyRegistry | undefined): string[] {
  return reg?.tags.builtin.map((b) => b.name) ?? [];
}

/** Whether a metric accepts this tag and this span; the error text when not (mirror
 *  of the engine's `MetricRef::check` plus `Span::check_allowed`). `definedTags`, when
 *  given, also flags a tag the fingerprint does not define. */
export function checkRef(
  reg: StrategyRegistry | undefined,
  r: MetricRef,
  definedTags?: readonly string[],
): string | null {
  const spec = findMetric(reg, r.metric);
  if (!spec) return `unknown metric \`${r.metric}\``;
  const tagErr = checkTag(reg, spec, r.tag, definedTags);
  if (tagErr) return tagErr;
  const m = parseSpan(r.span, r.slice);
  if (typeof m === 'string') return `${spec.path}: ${m}`;
  return checkSpan(spec, m);
}

function checkTag(
  reg: StrategyRegistry | undefined,
  spec: MetricSpec,
  tag: string | undefined,
  definedTags?: readonly string[],
): string | null {
  const path = spec.path;
  if (!tag) {
    return spec.tags === 'required' ? `${path} needs a tag: whose trades? e.g. @volume` : null;
  }
  if (spec.tags === 'none') return `${path} takes no tag (got @${tag})`;
  const { name, negated } = parseTagRef(tag);
  if (!/^[a-z0-9_]{1,24}$/.test(name)) return `tag \`${name}\` may use only a-z, 0-9 and _ (1 to 24)`;
  const builtin = builtinTagNames(reg).includes(name);
  if (spec.tag_level === 'wallet_class' && !builtin) {
    return `${path} reads a wallet class: ${builtinTagNames(reg).join(' or ')}`;
  }
  if (spec.tag_level !== 'wallet_class' && builtin) {
    return `${path}: \`${name}\` is a wallet class, not a fingerprint tag`;
  }
  if (negated && spec.tag_level !== 'trade') {
    return `${path} reads the tagged trades only; @!${name} has no meaning here`;
  }
  if (!builtin && definedTags && !definedTags.includes(name)) {
    return `the fingerprint defines no tag \`${name}\`, so this reads nothing and never holds`;
  }
  return null;
}

function checkSpan(spec: MetricSpec, m: SpanModel): string | null {
  const u = spec.spans;
  const path = spec.path;
  if (m.kind === 'since_age') return u.since_age ? null : `${path} does not take a since-age span`;
  if (m.kind === 'life') {
    if (u.life) return null;
    return u.since_age
      ? `${path} needs a since-age span, e.g. age60s`
      : `${path} needs a window span, e.g. 10s`;
  }
  if (!u.window) return `${path} counts over the whole life and takes no window`;
  if (u.slice && !m.slice) return `${path} needs a slice beside its span, e.g. slice 2s`;
  if (!u.slice && m.slice) return `${path} takes no slice`;
  return null;
}

/** The default read of a metric: the span it can take with the least typing. */
export function defaultRef(spec: MetricSpec, tag?: string): MetricRef {
  const r: MetricRef = { metric: spec.path };
  if (spec.tags === 'required' && tag) r.tag = tag;
  if (!spec.spans.life) {
    if (spec.spans.since_age) r.span = 'age0s';
    else if (spec.spans.slice) {
      r.span = '30s';
      r.slice = '2s';
    } else r.span = '10s';
  }
  return r;
}
