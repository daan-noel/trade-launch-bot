// **The fingerprint condition grammar, TS side** — the mirror of Rust
// `hunter_engine::fingerprint::grammar`. One text ⇄ predicate translation, shared
// by the axis form, the fingerprint table's filter boxes and every chip that has
// to be pasted back into either.
//
//   expr    := arm ( '|' arm )*          OR   — union of the arms
//   arm     := atom ( ',' atom )*        AND  — intersection of the atoms
//   atom    := op? operand
//   op      := '>=' | '<=' | '>' | '<' | '=' | '==' | '!='
//   operand := n | n '..' n | n '-' n | n '–' n
//
// Every atom denotes a set of values, so the whole expression is set algebra over
// the span helpers in `fingerprintAxes` — and because the domain is the
// non-negative integers, a union or a complement of windows is just more windows.
//
// Three things this grammar commits to:
//
// * **`..` is inclusive, `-` is half-open.** `1..2` is `[1, 2]`; `1-2` is `[1, 2)`,
//   which is what a group chip spans — so a chip's own text pasted into a filter
//   box selects exactly that chip's tokens. The parsed result is always echoed back
//   in the inclusive form, so which one was typed is never hidden.
// * **`>` and `<` are exact, not approximate.** The domain is integer, so `>1.5◎`
//   is `>= 1500000001` lamports — the same set, named in the storage vocabulary.
// * **Amounts parse as decimal text, never `Number()`.** `max_sol_cost = u64::MAX`
//   is real launch data past 2^53, so a float round-trip maps distinct amounts
//   onto one.
//
// Strict: any malformed fragment fails the whole parse (`null`). A dropped
// fragment would read as "no constraint", which WIDENS a match instead of failing
// the write.

import {
  axisDef,
  formatBound,
  parseBound,
  predicateFromSpans,
  predicateSpans,
  spanSetComplement,
  spanSetFrom,
  spanSetIntersect,
  spanSetIsAll,
  spanSetUnion,
  type AxisId,
  type AxisPredicate,
  type AxisUnit,
  type Span,
} from './fingerprintAxes';

/** The whole axis domain — the identity element of an AND arm. */
const ALL: Span[] = [{}];

/** Comparison operators, longest first: `>=` must not be read as `>` with a `=`
 *  operand. */
const OPS = ['>=', '<=', '==', '!=', '>', '<', '='] as const;

/**
 * Parse a condition expression in the axis's own display unit.
 *
 * `null` on anything malformed **and** on an expression that constrains nothing
 * (empty text, `>=0`, `<=2 | >=3`) — an axis that constrains nothing is not part
 * of identity, and that has exactly one spelling: absent from the criteria map.
 */
export function parseAxisPredicate(text: string, unit: AxisUnit): AxisPredicate | null {
  const set = parseSpanSet(text, unit);
  if (set == null || set.length === 0 || spanSetIsAll(set)) return null;
  return predicateFromSpans(set);
}

/** The value set an expression denotes, or `null` if malformed. Separate from
 *  {@link parseAxisPredicate} so a caller can tell "you typed nonsense" from "you
 *  typed something that selects nothing". */
export function parseSpanSet(text: string, unit: AxisUnit): Span[] | null {
  const t = text.trim();
  if (t === '') return null;
  let union: Span[] = [];
  for (const rawArm of t.split('|')) {
    const arm = rawArm.trim();
    if (arm === '') return null; // an empty OR arm is malformed, never "everything"
    let acc = ALL;
    for (const rawAtom of arm.split(',')) {
      const atom = parseAtom(rawAtom.trim(), unit);
      if (atom == null) return null;
      acc = spanSetIntersect(acc, atom);
    }
    union = spanSetUnion(union, acc);
  }
  return union;
}

/** One `op? operand` atom. */
function parseAtom(text: string, unit: AxisUnit): Span[] | null {
  if (text === '') return null;
  for (const op of OPS) {
    if (!text.startsWith(op)) continue;
    const rest = text.slice(op.length).trim();
    // An inequality bounds one edge, so a range operand has no meaning on it —
    // `>1..2` is a typo, not a wide gate.
    if (op === '>=' || op === '<=' || op === '>' || op === '<') {
      const v = parseBound(rest, unit);
      if (v == null) return null;
      // Integer domain: a strict edge is the next representable value, so this is
      // the same set, spelled the way the row stores it.
      if (op === '>=') return [{ min: v }];
      if (op === '<=') return [{ max: v }];
      if (op === '>') return [{ min: (BigInt(v) + 1n).toString() }];
      return BigInt(v) === 0n ? [] : [{ max: (BigInt(v) - 1n).toString() }];
    }
    const operand = parseOperand(rest, unit);
    if (operand == null) return null;
    return op === '!=' ? spanSetComplement(operand) : operand;
  }
  return parseOperand(text, unit);
}

/** `n`, `n..n` (inclusive) or `n-n` / `n–n` (half-open, the chip form). */
function parseOperand(text: string, unit: AxisUnit): Span[] | null {
  const dots = text.indexOf('..');
  if (dots >= 0) {
    const min = parseBound(text.slice(0, dots), unit);
    const max = parseBound(text.slice(dots + 2), unit);
    if (min == null || max == null) return null;
    return spanSetFrom([{ min, max }]);
  }
  for (const sep of ['–', '-']) {
    const at = text.indexOf(sep);
    if (at <= 0) continue; // a leading separator is not a range
    const lo = parseBound(text.slice(0, at), unit);
    const hi = parseBound(text.slice(at + sep.length), unit);
    if (lo == null || hi == null) return null;
    // Half-open `[lo, hi)`: an empty window is a typo, not a gate matching nothing.
    if (BigInt(hi) <= BigInt(lo)) return null;
    return [{ min: lo, max: (BigInt(hi) - 1n).toString() }];
  }
  const v = parseBound(text, unit);
  return v == null ? null : [{ min: v, max: v }];
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/**
 * Canonical text for a predicate — **round-trips through
 * {@link parseAxisPredicate}**, so what the form shows is what it would re-parse.
 *
 * Always the inclusive spelling, never the half-open one: `-` exists so a chip can
 * be pasted in, not so a stored window can hide which edge it includes.
 */
export function formatAxisPredicate(pred: AxisPredicate, unit: AxisUnit): string {
  if (pred.kind === 'sequence') return pred.labels.join(' | ');
  const spans = predicateSpans(pred);
  // A gap set reads as the hole it names, not as the two half-lines around it —
  // `!=3` rather than `<=2 | >=4`. Derived from the set, so it is still one text
  // per set.
  if (spans.length > 1) {
    const holes = spanSetComplement(spans);
    if (holes.length === 1 && (holes[0].min != null || holes[0].max != null)) {
      return `!=${formatSpanBody(holes[0], unit)}`;
    }
  }
  return spans.map((s) => formatSpanAtom(s, unit)).join(' | ');
}

/** The text a form shows for one axis: the canonical expression, or `''` when the
 *  axis is unset. */
export function axisPredicateText(id: AxisId, pred: AxisPredicate | undefined): string {
  return pred == null ? '' : formatAxisPredicate(pred, axisDef(id).unit);
}

/** One span as a standalone atom (`1.5`, `1.5..2`, `>=1.5`, `<=2`). */
function formatSpanAtom(s: Span, unit: AxisUnit): string {
  if (s.min != null && s.max == null) return `>=${formatBound(s.min, unit)}`;
  if (s.min == null && s.max != null) return `<=${formatBound(s.max, unit)}`;
  return formatSpanBody(s, unit);
}

/** A span's operand text, with no leading operator (`1.5`, `1.5..2`). */
function formatSpanBody(s: Span, unit: AxisUnit): string {
  const f = (v: string) => formatBound(v, unit);
  if (s.min != null && s.max != null) {
    return s.min === s.max ? f(s.min) : `${f(s.min)}..${f(s.max)}`;
  }
  if (s.min != null) return `>=${f(s.min)}`;
  if (s.max != null) return `<=${f(s.max)}`;
  return 'any';
}
