// **The fingerprint axis registry, TS side** — the mirror of Rust
// `hunter_engine::fingerprint::axis`.
//
// One `AxisDef` per matchable creation axis. The form, the summary chips, the
// group-card badge, the auto-name grammar and every filter box derive from this
// table, so **adding an axis is one entry here and one in the Rust `AXES`** — and
// `fingerprintAxes.test.ts` fails if the two lists drift.
//
// Two rules hold the design together, same as the Rust side:
//
// * **Identity is integer.** Every numeric axis is a non-negative integer carried
//   as a decimal STRING (a JS `number` is unsafe past 2^53 and
//   `max_sol_cost = u64::MAX` is real launch data). SOL is a display unit, parsed
//   and formatted only at this edge.
// * **Exact is the degenerate range.** `min === max` IS exact match, so nothing
//   carries a mode flag two readers can disagree about.
// * **A numeric predicate is a set of spans, stored canonically.** `!=` and `|`
//   are complements and unions of inclusive windows, which over the integers are
//   just more windows. Every builder routes through `spanSetFrom`, which sorts,
//   merges and collapses a one-span set back to a plain `range` — so one token set
//   has exactly ONE stored spelling, which is what keeps `criteria` usable as
//   identity: `<=2 | >=4` and `!=3` select the same tokens, so they must be the
//   same fingerprint row.

/** Every matchable axis, in registry order (the order chips render in). */
export type AxisId =
  | 'cu_limit'
  | 'cu_price'
  | 'init_buy_lamports'
  | 'max_cost_lamports'
  | 'spendable_lamports_in'
  | 'first_slot_buy_lamports'
  | 'first_slot_sell_lamports'
  | 'ix_labels'
  | 'ix_count'
  | 'prior_launches';

/** What an axis's numbers *are* — drives how a bound is shown and parsed. */
export type AxisUnit = 'lamports' | 'compute_units' | 'count' | 'labels';

/** When the observed value is known. A `first_slot` axis only settles after the
 *  creation slot closes, so a rule using one cannot fire at birth. */
export type AxisPhase = 'instant' | 'first_slot';

export interface AxisDef {
  id: AxisId;
  /** Human label for the form and the summary. */
  label: string;
  /** Short token the auto-name chip uses (`max=1.5`). */
  chip: string;
  kind: 'numeric' | 'sequence';
  unit: AxisUnit;
  phase: AxisPhase;
  /** **The one definition of this axis**, rendered into the UI from this text. */
  definition: string;
}

export const AXES: readonly AxisDef[] = [
  {
    id: 'cu_limit',
    label: 'CU limit',
    chip: 'cu_limit',
    kind: 'numeric',
    unit: 'compute_units',
    phase: 'instant',
    definition:
      "Compute-unit limit requested by the creation transaction. A launch tool's fixed setting, so it identifies the tool.",
  },
  {
    id: 'cu_price',
    label: 'CU price',
    chip: 'cu_price',
    kind: 'numeric',
    unit: 'compute_units',
    phase: 'instant',
    definition:
      "Compute-unit price (micro-lamports) paid by the creation transaction — the launcher's priority-fee setting.",
  },
  {
    id: 'init_buy_lamports',
    label: 'Initial buy',
    chip: 'init',
    kind: 'numeric',
    unit: 'lamports',
    phase: 'instant',
    definition: 'Lamports the creator spent on their own first buy of the token.',
  },
  {
    id: 'max_cost_lamports',
    label: 'Max cost',
    chip: 'max',
    kind: 'numeric',
    unit: 'lamports',
    phase: 'instant',
    definition:
      'The `max_sol_cost` slippage ceiling on the creator’s initial buy, in lamports. `u64::MAX` is the "fill at any price" sentinel, carried exactly and matchable as itself.',
  },
  {
    id: 'spendable_lamports_in',
    label: 'Spendable in',
    chip: 'spend',
    kind: 'numeric',
    unit: 'lamports',
    phase: 'instant',
    definition: 'Lamports the creator wallet held going into the launch.',
  },
  {
    id: 'first_slot_buy_lamports',
    label: 'First-slot buy',
    chip: 'fs_buy',
    kind: 'numeric',
    unit: 'lamports',
    phase: 'first_slot',
    definition:
      'Buy lamports summed across every trade landing in the creation slot — how funded the launch was. Known only once that slot settles.',
  },
  {
    id: 'first_slot_sell_lamports',
    label: 'First-slot sell',
    chip: 'fs_sell',
    kind: 'numeric',
    unit: 'lamports',
    phase: 'first_slot',
    definition:
      'Sell lamports summed across every trade landing in the creation slot. Known only once that slot settles.',
  },
  {
    id: 'ix_labels',
    label: 'Instruction labels',
    chip: 'ix',
    kind: 'sequence',
    unit: 'labels',
    phase: 'instant',
    definition:
      'The creation transaction’s instruction labels, matched as an EXACT ordered sequence — same length, same label at every position.',
  },
  {
    id: 'ix_count',
    label: 'Instruction count',
    chip: 'ix_count',
    kind: 'numeric',
    unit: 'count',
    phase: 'instant',
    definition:
      'How many instructions the creation transaction carried — launch tooling as one number, without pinning which instructions they were.',
  },
  {
    id: 'prior_launches',
    label: 'Prior launches',
    chip: 'prior',
    kind: 'numeric',
    unit: 'count',
    phase: 'instant',
    definition:
      'How many tokens this creator launched BEFORE this one. A strictly-prior tally, so a first-time creator reads 0; unknown when the creator wallet is not on the creation event.',
  },
] as const;

const BY_ID = new Map<AxisId, AxisDef>(AXES.map((a) => [a.id, a]));

/** This axis's registry row. */
export function axisDef(id: AxisId): AxisDef {
  const def = BY_ID.get(id);
  if (!def) throw new Error(`unknown fingerprint axis: ${id}`);
  return def;
}

/** Whether a wire key names a registered axis. */
export function isAxisId(key: string): key is AxisId {
  return BY_ID.has(key as AxisId);
}

// ---------------------------------------------------------------------------
// Predicates
// ---------------------------------------------------------------------------

/** Inclusive `[min, max]`; an absent bound is open. `min === max` is exact.
 *
 *  Bounds are decimal STRINGS — a JS `number` loses precision past 2^53, and a
 *  `max_sol_cost` ceiling exceeds it. Never parse one with `Number()` before
 *  comparing; compare the strings, or use {@link compareBounds}. */
export interface RangePredicate {
  kind: 'range';
  min?: string;
  max?: string;
}

/** One inclusive `[min, max]` interval; an absent bound is open. The element of a
 *  {@link SpansPredicate} and the whole of a {@link RangePredicate}, so the two
 *  shapes are one vocabulary — see {@link predicateSpans}. */
export interface Span {
  min?: string;
  max?: string;
}

/** Two or more **disjoint, ascending, non-adjacent** inclusive spans — the shape
 *  `!=` and `|` produce (`!=3` is `[…2] ∪ [4…]`). Canonical by construction; see
 *  {@link spanSetFrom}. Mirrors Rust `AxisPredicate::Spans`. */
export interface SpansPredicate {
  kind: 'spans';
  spans: Span[];
}

/** Exact ordered label sequence. */
export interface SequencePredicate {
  kind: 'sequence';
  labels: string[];
}

export type AxisPredicate = RangePredicate | SpansPredicate | SequencePredicate;

/** A fingerprint's configured axes. An axis **absent** from the map is not part of
 *  identity — there is no null-as-unset second spelling. */
export type Criteria = Partial<Record<AxisId, AxisPredicate>>;

/** Match one amount and nothing else. */
export function exactPredicate(v: string): RangePredicate {
  return { kind: 'range', min: v, max: v };
}

/** The axis kind a predicate can be applied to. Mirrors Rust `AxisPredicate::kind`. */
export function predicateKind(p: AxisPredicate): 'numeric' | 'sequence' {
  return p.kind === 'sequence' ? 'sequence' : 'numeric';
}

/** The inclusive spans a numeric predicate accepts, ascending — **the one reader**
 *  of a numeric predicate's shape, so every consumer (the name, the summary, the
 *  filter, the form) writes one loop and gains `!=`/`|` for free. Mirrors Rust
 *  `AxisPredicate::spans`. */
export function predicateSpans(p: AxisPredicate): Span[] {
  if (p.kind === 'range') {
    return [{ ...(p.min != null && { min: p.min }), ...(p.max != null && { max: p.max }) }];
  }
  return p.kind === 'spans' ? p.spans : [];
}

// ---------------------------------------------------------------------------
// Span algebra — the working form the condition grammar evaluates in
// ---------------------------------------------------------------------------
//
// `bigint`, never `number`: bounds are u128 decimal strings and a `u64::MAX`
// ceiling is real launch data, so every comparison and every ±1 step here has to
// be exact. Mirrors Rust `SpanSet`.

/** One span with numeric bounds; `null` is an open edge. */
interface NSpan {
  lo: bigint | null;
  hi: bigint | null;
}

/** Low edge as a number. Identity is a non-negative integer, so "unbounded below"
 *  and "from 0" are the same set — used for ordering and overlap only, never to
 *  rewrite a stored open bound, which would change the identity of every row that
 *  already spells one (and `0 … 0` is a real, different gate). */
const lowOf = (s: NSpan): bigint => s.lo ?? 0n;

const nspanOk = (s: NSpan): boolean => s.lo == null || s.hi == null || s.lo <= s.hi;

/** Whether `b` starts at or before the first value `a` leaves uncovered, so the
 *  two are one interval. Adjacency counts: the domain is integer, so `[0,2]` and
 *  `[3,5]` cover exactly `[0,5]` and storing them apart would be a second spelling
 *  of one set. */
const touches = (a: NSpan, b: NSpan): boolean => a.hi == null || lowOf(b) <= a.hi + 1n;

function toNSpans(spans: readonly Span[]): NSpan[] {
  return spans.map((s) => ({
    lo: s.min == null ? null : BigInt(s.min),
    hi: s.max == null ? null : BigInt(s.max),
  }));
}

function fromNSpans(spans: readonly NSpan[]): Span[] {
  return spans.map((s) => ({
    ...(s.lo != null && { min: s.lo.toString() }),
    ...(s.hi != null && { max: s.hi.toString() }),
  }));
}

function canonical(spans: readonly NSpan[]): NSpan[] {
  const kept = spans.filter(nspanOk).map((s) => ({ ...s }));
  // By low edge, then by high edge with an open end LAST — an open end absorbs
  // everything after it, so it must not sort before a span it covers.
  kept.sort((a, b) => {
    const la = lowOf(a);
    const lb = lowOf(b);
    if (la !== lb) return la < lb ? -1 : 1;
    if (a.hi == null || b.hi == null) return a.hi == null ? (b.hi == null ? 0 : 1) : -1;
    return a.hi === b.hi ? 0 : a.hi < b.hi ? -1 : 1;
  });
  const out: NSpan[] = [];
  for (const s of kept) {
    const prev = out[out.length - 1];
    if (prev && touches(prev, s)) {
      if (prev.hi != null && (s.hi == null || s.hi > prev.hi)) prev.hi = s.hi;
    } else {
      out.push(s);
    }
  }
  return out;
}

/** Canonicalise an arbitrary span list: drop the unsatisfiable, sort, merge
 *  everything that overlaps or touches. Mirrors Rust `SpanSet::from_spans`. */
export function spanSetFrom(spans: readonly Span[]): Span[] {
  return fromNSpans(canonical(toNSpans(spans)));
}

/** Everything in either set (`|`). */
export function spanSetUnion(a: readonly Span[], b: readonly Span[]): Span[] {
  return spanSetFrom([...a, ...b]);
}

/** Everything in both sets (`,`). Pairwise, because both sides are already
 *  disjoint and ascending, so no pair can produce more than one span. */
export function spanSetIntersect(a: readonly Span[], b: readonly Span[]): Span[] {
  const out: NSpan[] = [];
  for (const x of toNSpans(a)) {
    for (const y of toNSpans(b)) {
      const lo = x.lo == null ? y.lo : y.lo == null ? x.lo : x.lo > y.lo ? x.lo : y.lo;
      const hi = x.hi == null ? y.hi : y.hi == null ? x.hi : x.hi < y.hi ? x.hi : y.hi;
      if (nspanOk({ lo, hi })) out.push({ lo, hi });
    }
  }
  return fromNSpans(canonical(out));
}

/** Everything a set excludes (`!`). Over the non-negative integers, so the gap
 *  below the first span is open-ended — the same spelling `<=` already stores, so
 *  `!=3` and `<=2 | >=4` land on one value. */
export function spanSetComplement(spans: readonly Span[]): Span[] {
  const out: NSpan[] = [];
  // `null` = "from the bottom of the domain", where the first gap starts;
  // afterwards it is one past the previous span.
  let cursor: bigint | null = null;
  for (const s of canonical(toNSpans(spans))) {
    // Only the first span of a canonical set can be open below, and a low edge of
    // 0 leaves no room beneath it — both are "no gap here", not a span.
    if (s.lo != null && s.lo > 0n) out.push({ lo: cursor, hi: s.lo - 1n });
    if (s.hi == null) return fromNSpans(canonical(out)); // nothing above an open top
    cursor = s.hi + 1n;
  }
  out.push({ lo: cursor, hi: null });
  return fromNSpans(canonical(out));
}

/** The whole domain — the state that configures nothing, so a write edge clears it
 *  rather than storing a row that reads as narrowed and matches everything. `>= 0`
 *  counts: a floor of zero excludes nothing on a non-negative axis. Mirrors Rust
 *  `SpanSet::is_all`. */
export function spanSetIsAll(spans: readonly Span[]): boolean {
  const [only] = spans;
  return spans.length === 1 && only.max == null && (only.min == null || BigInt(only.min) === 0n);
}

/** The predicate that STORES a span set: one span is a plain `range`, so the shape
 *  already in every row stays the spelling for everything `!=`/`|` did not widen.
 *  Mirrors Rust `SpanSet::into_predicate`. */
export function predicateFromSpans(spans: readonly Span[]): AxisPredicate {
  const canon = spanSetFrom(spans);
  return canon.length === 1 ? { kind: 'range', ...canon[0] } : { kind: 'spans', spans: canon };
}

/** Match everything EXCEPT the inclusive window `[min, max]` — the `!=` atom. */
export function notRangePredicate(min?: string, max?: string): AxisPredicate {
  const hole: Span = { ...(min != null && { min }), ...(max != null && { max }) };
  return predicateFromSpans(spanSetComplement([hole]));
}

/** The single amount this predicate pins, if it pins one. **The one reader** of
 *  "is this exact" — nothing else compares `min` against `max`. */
export function asExact(p: AxisPredicate | undefined): string | null {
  if (!p || p.kind !== 'range') return null;
  if (p.min == null || p.max == null || p.min !== p.max) return null;
  return p.min;
}

/** Whether any value can satisfy this predicate. An unsatisfiable one is refused
 *  at the write edge: stored, it silently disarms every rule bound to the
 *  fingerprint while the row still reads as configured. */
export function isSatisfiable(p: AxisPredicate): boolean {
  if (p.kind === 'sequence') return p.labels.length > 0;
  // An empty list is how "no value at all" arrives — `<=2, >=7`, or a `!=` of an
  // open range.
  if (p.kind === 'spans') return p.spans.length > 0 && p.spans.every(spanOk);
  return spanOk(p);
}

const spanOk = (s: Span): boolean =>
  s.min == null || s.max == null || compareBounds(s.min, s.max) <= 0;

/** Why a span list is malformed **as stored**, beyond being empty. A `spans`
 *  predicate is only ever produced canonical, so a non-canonical one reached the
 *  row by hand or by a writer that skipped {@link spanSetFrom} — refused rather
 *  than normalised on read, because two spellings of one token set key as two
 *  fingerprints. Mirrors Rust `AxisPredicate::shape_problem`. */
function shapeProblem(p: AxisPredicate): string | null {
  if (p.kind !== 'spans') return null;
  if (p.spans.length < 2) {
    return 'a multi-span predicate needs two or more spans — one span is spelled as a plain range';
  }
  // Pairwise rather than "does canonicalising change the length": a DESCENDING
  // pair canonicalises to the same count, so a length check would pass a list the
  // matcher's early-exit scan reads wrongly.
  const spans = toNSpans(p.spans);
  for (let i = 0; i + 1 < spans.length; i += 1) {
    if (touches(spans[i], spans[i + 1])) {
      return 'spans must be disjoint and ascending — overlapping or touching spans are two spellings of one window';
    }
  }
  return null;
}

/** Compare two decimal-string bounds numerically without `Number()` — which would
 *  round a `u64::MAX` ceiling and call two distinct amounts equal. */
export function compareBounds(a: string, b: string): number {
  const x = a.replace(/^0+(?=\d)/, '');
  const y = b.replace(/^0+(?=\d)/, '');
  if (x.length !== y.length) return x.length < y.length ? -1 : 1;
  return x === y ? 0 : x < y ? -1 : 1;
}

/** Whether an observed integer (decimal string) satisfies the predicate. */
export function predicateMatches(p: AxisPredicate, value: string): boolean {
  if (p.kind === 'sequence') return false;
  return predicateSpans(p).some((s) => spanContains(s, value));
}

/** Whether an observed integer falls in one span. */
export function spanContains(s: Span, value: string): boolean {
  if (s.min != null && compareBounds(value, s.min) < 0) return false;
  if (s.max != null && compareBounds(value, s.max) > 0) return false;
  return true;
}

/** Whether two predicates can both hold for some token — the question a filter box
 *  asks of a row ("could this row match anything I typed"). For a bare value this
 *  is containment, so it generalises the older "row's window contains it" rule
 *  rather than replacing it. */
export function predicatesOverlap(a: AxisPredicate, b: AxisPredicate): boolean {
  if (a.kind === 'sequence' || b.kind === 'sequence') {
    return a.kind === 'sequence' && b.kind === 'sequence' && predicatesSameLabels(a, b);
  }
  return spanSetIntersect(predicateSpans(a), predicateSpans(b)).length > 0;
}

function predicatesSameLabels(a: SequencePredicate, b: SequencePredicate): boolean {
  return a.labels.length === b.labels.length && a.labels.every((v, i) => v === b.labels[i]);
}

/** Every reason a criteria map is unusable, as operator-facing sentences. Empty ⇒
 *  valid. Mirrors Rust `Criteria::problems`, so the form shows what the write edge
 *  would reject rather than discovering it on save. */
export function criteriaProblems(criteria: Criteria): string[] {
  const out: string[] = [];
  for (const [key, pred] of Object.entries(criteria) as [AxisId, AxisPredicate][]) {
    if (!pred) continue;
    const def = axisDef(key);
    if (predicateKind(pred) !== def.kind) {
      out.push(`${def.label}: a ${def.kind} axis cannot carry a ${pred.kind} predicate`);
      continue;
    }
    if (!isSatisfiable(pred)) {
      out.push(
        pred.kind === 'sequence'
          ? `${def.label}: an empty label sequence configures nothing`
          : `${def.label}: no value can satisfy that window — the low bound must not exceed the high one`,
      );
      continue;
    }
    const shape = shapeProblem(pred);
    if (shape) out.push(`${def.label}: ${shape}`);
  }
  // `ix_count` IS `ix_labels.length`, so a row carrying both must agree with
  // itself. Left unchecked the contradiction is invisible: the row reads as fully
  // configured and matches nothing, which looks like a cohort that stopped
  // launching.
  const labels = criteria.ix_labels;
  const count = criteria.ix_count;
  if (labels?.kind === 'sequence' && count && !predicateMatches(count, String(labels.labels.length))) {
    out.push(
      `Instruction count excludes ${labels.labels.length}, the length of the instruction-label sequence on the same fingerprint — no token can satisfy both`,
    );
  }
  return out;
}

/** Configured axes in registry order — the order every derived rendering uses, so
 *  nothing has to sort. */
export function configuredAxes(criteria: Criteria): [AxisId, AxisPredicate][] {
  return AXES.flatMap((def) => {
    const p = criteria[def.id];
    return p ? ([[def.id, p]] as [AxisId, AxisPredicate][]) : [];
  });
}

/** Whether any configured axis settles only after the creation slot — the one
 *  reader of "this fingerprint cannot resolve at birth". */
export function hasDeferredAxis(criteria: Criteria): boolean {
  return configuredAxes(criteria).some(([id]) => axisDef(id).phase === 'first_slot');
}

// ---------------------------------------------------------------------------
// Display + parsing (the ONLY place SOL exists)
// ---------------------------------------------------------------------------

const LAMPORTS_PER_SOL = 1_000_000_000n;

/** Lamports (decimal string) → human SOL, exactly. Integer arithmetic: dividing
 *  through `Number` is lossless only below 2^53, and a ceiling's low digits vanish
 *  there — mapping distinct amounts onto one label. Mirrors Rust `sol_label`. */
export function lamportsToSolLabel(lamports: string): string {
  let n: bigint;
  try {
    n = BigInt(lamports);
  } catch {
    return lamports;
  }
  const whole = n / LAMPORTS_PER_SOL;
  const frac = n % LAMPORTS_PER_SOL;
  if (frac === 0n) return whole.toString();
  return `${whole}.${frac.toString().padStart(9, '0')}`.replace(/0+$/, '');
}

/** Human SOL → lamports (decimal string), rounded. `null` on junk — never `0`,
 *  which is a real amount, and never the `u64::MAX` sentinel. */
export function solLabelToLamports(sol: string): string | null {
  const t = sol.trim();
  if (!/^\d+(\.\d*)?$/.test(t)) return null;
  const [whole, frac = ''] = t.split('.');
  const padded = (frac + '000000000').slice(0, 9);
  const round = frac.length > 9 && Number(frac[9]) >= 5 ? 1n : 0n;
  return (BigInt(whole) * LAMPORTS_PER_SOL + BigInt(padded) + round).toString();
}

/** One bound, in the axis's own display unit. */
export function formatBound(value: string, unit: AxisUnit): string {
  return unit === 'lamports' ? lamportsToSolLabel(value) : value;
}

/** One typed bound, from the axis's display unit into the integer identity carries.
 *  `null` on junk — a dropped bound reads as "unbounded", which WIDENS the match. */
export function parseBound(text: string, unit: AxisUnit): string | null {
  const t = text.trim();
  if (t === '') return null;
  if (unit === 'lamports') return solLabelToLamports(t);
  return /^\d+$/.test(t) ? t.replace(/^0+(?=\d)/, '') : null;
}

/** A predicate as one readable value: `1.515`, `1.5–2`, `≥1.5`, `≤2`, `≠3`,
 *  `1–2 | 7–8`. Display only — nothing parses this back, which is why it is free
 *  to use the glyphs. For text a form can re-parse, use `formatAxisPredicate` in
 *  `fingerprintGrammar`. */
export function formatPredicate(id: AxisId, p: AxisPredicate): string {
  return formatPredicateInUnit(p, axisDef(id).unit);
}

/** {@link formatPredicate} for a caller that has the unit but not the axis (a
 *  group key's tag can name a grouping-only field). */
export function formatPredicateInUnit(p: AxisPredicate, unit: AxisUnit): string {
  if (p.kind === 'sequence') return p.labels.join(' | ');
  const spans = predicateSpans(p);
  // A gap set reads as the hole it names, not the two half-lines around it.
  const holes = spans.length > 1 ? spanSetComplement(spans) : [];
  if (holes.length === 1 && holes[0].min != null && holes[0].max != null) {
    return `≠${formatSpan(holes[0], unit)}`;
  }
  return spans.map((s) => formatSpan(s, unit)).join(' | ');
}

/** One span, in the axis's display unit. */
function formatSpan(s: Span, unit: AxisUnit): string {
  const f = (v: string) => formatBound(v, unit);
  if (s.min != null && s.max != null) {
    return s.min === s.max ? f(s.min) : `${f(s.min)}–${f(s.max)}`;
  }
  if (s.min != null) return `≥${f(s.min)}`;
  if (s.max != null) return `≤${f(s.max)}`;
  return 'any';
}
