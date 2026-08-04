import { signalGradeClass } from 'lib/signedTone';
import type { FlowDiscoveryStructure } from 'types';

/** Buy-only / sell-only / both-sided (e.g. Axiom's shared swap ix), from a
 *  structure's buy_sol/sell_sol split — a 2% dust tolerance absorbs noise. */
export function sideOf(s: { buy_sol: number; sell_sol: number }): 'buy' | 'sell' | 'both' {
  const total = s.buy_sol + s.sell_sol;
  if (total <= 0) return 'both';
  const buyFrac = s.buy_sol / total;
  if (buyFrac >= 0.98) return 'buy';
  if (buyFrac <= 0.02) return 'sell';
  return 'both';
}

/**
 * 0..1 bot/wash-like grade → theme tone classes (SSOT: `signalGradeClass`).
 * Lift ≈1 is noise; ≥3× is strong — normalize via `(lift - 1) / 2`.
 * Wash trends toward 0 as buys/sells cancel — invert so danger = round-trip.
 */
export const liftGrade = (lift: number) => signalGradeClass((lift - 1) / 2);
export const pctGrade = (pct: number) => signalGradeClass(pct / 100);
export const unitGrade = (v: number) => signalGradeClass(v);
export const washGrade = (wash: number) => signalGradeClass(1 - wash);

const clamp01 = (t: number) => Math.max(0, Math.min(1, t));

// ── auto-suggest ("is this shape manufactured volume?") ──────────────────────
// A client-side composite of the bot-likelihood columns, so the human doesn't
// have to eyeball every row. Deliberately conservative: two hard gates, then a
// vote of the normalized signals. Nothing here is written to the backend — the
// backend still only flags degenerate `ambiguity`; this just pre-checks the
// rows a human almost certainly would (see DISCOVERY_COL_HELP.suggested).

/** Lift below this is "common everywhere" noise — never auto-flag (mirrors the
 *  backend `LIFT_AMBIGUOUS` used for the group `ambiguity` chip). Applied ONLY
 *  when the group's lift is informative (`FlowDiscoveryGroup.lift_defined`): a
 *  group that IS the whole scored corpus scores every shape at exactly 1.0, and
 *  failing every row on a self-comparison is what made this whole verdict dead
 *  on fingerprint-scoped runs. */
export const SUGGEST_MIN_LIFT = 1.25;
/** Size floor — ignore dust-sized shapes (mirrors backend `MIN_STRUCTURE_SOL`). */
export const SUGGEST_MIN_GROSS = 0.05;
/** A family at/above this is "strong" — the wording the tooltip uses. The badge
 *  itself is decided by {@link SUGGEST_SCORE}, not by counting these. */
export const SUGGEST_STRONG = 0.5;
/** Score at/above which a row is Suggested. The score is a mean over families,
 *  so this threshold already encodes "needs about two kinds of evidence": one
 *  family at 1.0 with the rest cold lands at 0.25–0.33 and cannot pass alone. */
export const SUGGEST_SCORE = 0.5;
/** `wallet_reuse` is `1 − distinct/trades`, so 2 trades from 1 wallet reads 0.5 —
 *  "strong" off a coin flip. Below this many trades the signal is dropped as
 *  unavailable rather than believed. */
export const SUGGEST_MIN_REUSE_TRADES = 4;
/** A shape must move meaningful SOL on at least this many of the group's tokens
 *  before it can be auto-selected. A pattern is written onto a FINGERPRINT — it
 *  then classifies every token that fingerprint matches — so a one-token
 *  curiosity is out of scope however bot-like its trades look. */
export const SUGGEST_MIN_TOKENS = 2;

// ── first-slot (launch) presence ─────────────────────────────────────────────
// Two separate facts about WHEN a shape trades, both kept apart from the
// composite above on purpose — that one votes on how bot-like a shape looks,
// these are identity claims about the launch itself:
//   * `firstSlotPurity` — the Launch% column: how much of the shape landed at
//     launch. Informational ranking/sorting only.
//   * `isFirstSlotPresent` — the bulk-select predicate: did the shape trade in
//     the creation slot AT ALL. The launch bundle is the set of shapes that
//     appear in that slot, and a bundler shape that also trades later is still
//     bundler tooling — so this is presence, not purity.
// The creation instruction needs no special case: a shape carrying it is in the
// creation slot by construction, so the presence test already takes it.

/** Fraction (0..1) of a structure's gross SOL that landed in its token's creation
 *  slot, or `null` when the run predates the backend field — unknown, never 0. */
export function firstSlotPurity(s: FlowDiscoveryStructure): number | null {
  const fs = s.first_slot_gross_sol;
  if (fs == null) return null;
  if (s.gross_sol <= 0) return null;
  return clamp01(fs / s.gross_sol);
}

/** Launch tooling: traded in at least one matched token's creation slot AND above
 *  the dust floor. `first_slot_trades == null` is *unknown* (a result cached
 *  before the backend field existed), never 0 — such a row is excluded rather
 *  than guessed at. No lift gate — appearing in the creation slot is its own
 *  identity claim, whereas lift measures group-vs-window concentration and would
 *  drop a bundler shape that's ambient across the whole window. */
export function isFirstSlotPresent(s: FlowDiscoveryStructure): boolean {
  const trades = s.first_slot_trades;
  return trades != null && trades > 0 && s.gross_sol >= SUGGEST_MIN_GROSS;
}

/** One kind of evidence. A family collapses signals that move together — two
 *  columns lighting up off the SAME underlying fact must count once, not twice
 *  (a single launch bundle trips both same-slot clustering and few-wallets, and
 *  under the old two-strong-signals vote that alone was a pass). Members combine
 *  by `max`: the family is as strong as its best available member. */
export interface SuggestFamily {
  label: string;
  /** 0..1, or `null` when this kind of evidence doesn't apply / isn't reliable
   *  here. A null family is left out of the mean rather than scored as 0. */
  value: number | null;
  /** Why it reads the way it does — shown verbatim in the row's tooltip. */
  detail: string;
}

export interface StructureSuggestion {
  /** Cleared the gates AND scored ≥ SUGGEST_SCORE. */
  suggested: boolean;
  /** Excluded by a gate before any evidence was weighed. */
  gated: boolean;
  /** Which gate, for the tooltip. `null` when not gated. */
  gatedReason: string | null;
  /** Mean of the applicable families (0..1) — the shown %, and the ONE number
   *  the badge is decided on. */
  score: number;
  /** Every family, applicable or not — the "why" AND the "why not". */
  families: SuggestFamily[];
  /** Labels of the families at/above SUGGEST_STRONG (compact display). */
  reasons: string[];
}

/** Composite bot-likelihood verdict for one structure.
 *
 *  Three properties this deliberately holds, each a fix for a way the previous
 *  vote misled:
 *
 *  - **The number IS the decision.** Score is a mean over families and the badge
 *    is `score >= SUGGEST_SCORE` — nothing else. Previously the % was a mean
 *    while the badge counted strong signals, so 49% could be un-badged next to a
 *    badged 33%.
 *  - **Correlated columns count once** (see {@link SuggestFamily}).
 *  - **The verdict does not move as you check rows.** Contagion% is deliberately
 *    NOT an input: it is defined against the current draft, so folding it in made
 *    the same row score differently depending on what you had already clicked,
 *    and made the bulk-select non-idempotent (pressing it twice took more rows
 *    than pressing it once). It stays a column you read when deciding by hand.
 *
 *  `ctx.liftDefined` is the group's `lift_defined`; `ctx.groupTokens` its
 *  `n_tokens` (turns Recur% back into the token count the floor needs). */
export function suggestStructure(
  s: FlowDiscoveryStructure,
  ctx: { liftDefined: boolean; groupTokens: number },
): StructureSuggestion {
  const side = sideOf(s);
  // Tokens on which this shape moved meaningful SOL — `cross_token_recurrence`
  // is that count as a % of the group, so recover the count itself.
  const meaningfulTokens = Math.round((s.cross_token_recurrence / 100) * ctx.groupTokens);

  let gatedReason: string | null = null;
  if (s.gross_sol < SUGGEST_MIN_GROSS) {
    gatedReason = `Dust — ${s.gross_sol.toFixed(3)}◎ is under the ${SUGGEST_MIN_GROSS}◎ floor`;
  } else if (ctx.liftDefined && s.group_lift < SUGGEST_MIN_LIFT) {
    gatedReason = `Ambient — Lift ${s.group_lift.toFixed(2)}× is under ${SUGGEST_MIN_LIFT}× (as common outside this group as in it)`;
  } else if (meaningfulTokens < SUGGEST_MIN_TOKENS) {
    gatedReason = `Too few tokens — meaningful on ${meaningfulTokens} of ${ctx.groupTokens}; a pattern is written onto the whole fingerprint`;
  }

  const reuseUsable = s.n_trades >= SUGGEST_MIN_REUSE_TRADES;
  const wallets = reuseUsable
    ? Math.max(clamp01(s.wallet_reuse), clamp01(s.wallet_overlap))
    : clamp01(s.wallet_overlap);
  const families: SuggestFamily[] = [
    {
      label: 'Recur',
      value: clamp01(s.cross_token_recurrence / 100),
      detail: `meaningful on ${meaningfulTokens}/${ctx.groupTokens} tokens`,
    },
    {
      label: 'Burst',
      value: clamp01(s.slot_burst / 100),
      detail: `${s.slot_burst.toFixed(0)}% of trades cluster within ±1 slot`,
    },
    {
      label: 'Wallets',
      value: wallets,
      detail: reuseUsable
        ? `reuse ${s.wallet_reuse.toFixed(2)}, cross-token overlap ${s.wallet_overlap.toFixed(2)}`
        : `overlap ${s.wallet_overlap.toFixed(2)} (reuse ignored — only ${s.n_trades} trades)`,
    },
    {
      label: 'Wash',
      value: side === 'both' ? clamp01(1 - s.wash_symmetry) : null,
      detail:
        side === 'both'
          ? `net/gross ${s.wash_symmetry.toFixed(2)} — 0 is a full round-trip`
          : `n/a on a ${side}-only shape (nothing here to net against)`,
    },
  ];

  const scored = families.filter((f): f is SuggestFamily & { value: number } => f.value != null);
  const score = scored.length ? scored.reduce((a, f) => a + f.value, 0) / scored.length : 0;
  return {
    gated: gatedReason != null,
    gatedReason,
    score,
    families,
    reasons: scored.filter((f) => f.value >= SUGGEST_STRONG).map((f) => f.label),
    suggested: gatedReason == null && score >= SUGGEST_SCORE,
  };
}

/** Multi-line "why / why not" for a row's Auto cell tooltip — every family, its
 *  value, and whether it counted, plus the gate when one fired. */
export function suggestExplain(sug: StructureSuggestion): string {
  const lines = sug.families.map((f) => {
    if (f.value == null) return `  —     ${f.label}: ${f.detail}`;
    const mark = f.value >= SUGGEST_STRONG ? 'STRONG' : '  weak';
    return `  ${mark} ${f.label} ${(f.value * 100).toFixed(0)}% — ${f.detail}`;
  });
  const head = sug.gated
    ? `Not eligible: ${sug.gatedReason}`
    : `Score ${(sug.score * 100).toFixed(0)}% (mean of what applies) — ${
        sug.suggested ? 'at or above' : 'below'
      } the ${SUGGEST_SCORE * 100}% bar`;
  return [head, '', ...lines].join('\n');
}
