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

/** Exact ordered label sequence. */
export interface SequencePredicate {
  kind: 'sequence';
  labels: string[];
}

export type AxisPredicate = RangePredicate | SequencePredicate;

/** A fingerprint's configured axes. An axis **absent** from the map is not part of
 *  identity — there is no null-as-unset second spelling. */
export type Criteria = Partial<Record<AxisId, AxisPredicate>>;

/** Match one amount and nothing else. */
export function exactPredicate(v: string): RangePredicate {
  return { kind: 'range', min: v, max: v };
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
  if (p.min == null || p.max == null) return true;
  return compareBounds(p.min, p.max) <= 0;
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
  if (p.kind !== 'range') return false;
  if (p.min != null && compareBounds(value, p.min) < 0) return false;
  if (p.max != null && compareBounds(value, p.max) > 0) return false;
  return true;
}

/** Every reason a criteria map is unusable, as operator-facing sentences. Empty ⇒
 *  valid. Mirrors Rust `Criteria::problems`, so the form shows what the write edge
 *  would reject rather than discovering it on save. */
export function criteriaProblems(criteria: Criteria): string[] {
  const out: string[] = [];
  for (const [key, pred] of Object.entries(criteria) as [AxisId, AxisPredicate][]) {
    if (!pred) continue;
    const def = axisDef(key);
    const wantKind = def.kind === 'numeric' ? 'range' : 'sequence';
    if (pred.kind !== wantKind) {
      out.push(`${def.label}: a ${def.kind} axis cannot carry a ${pred.kind} predicate`);
      continue;
    }
    if (!isSatisfiable(pred)) {
      out.push(
        pred.kind === 'range'
          ? `${def.label}: no value can satisfy that window — the low bound must not exceed the high one`
          : `${def.label}: an empty label sequence configures nothing`,
      );
    }
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

/** A predicate as one readable value: `1.515`, `1.5–2`, `≥1.5`, `≤2`. */
export function formatPredicate(id: AxisId, p: AxisPredicate): string {
  const def = axisDef(id);
  if (p.kind === 'sequence') return p.labels.join(' | ');
  const f = (v: string) => formatBound(v, def.unit);
  if (p.min != null && p.max != null) {
    return p.min === p.max ? f(p.min) : `${f(p.min)}–${f(p.max)}`;
  }
  if (p.min != null) return `≥${f(p.min)}`;
  if (p.max != null) return `≤${f(p.max)}`;
  return 'any';
}
