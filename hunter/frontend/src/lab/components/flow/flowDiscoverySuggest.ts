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
 *  backend `LIFT_AMBIGUOUS` used for the group `ambiguity` chip). */
export const SUGGEST_MIN_LIFT = 1.25;
/** Size floor — ignore dust-sized shapes (mirrors backend `MIN_STRUCTURE_SOL`). */
export const SUGGEST_MIN_GROSS = 0.05;
/** A normalized 0..1 signal at/above this counts as "strong". */
export const SUGGEST_STRONG = 0.5;
/** How many strong signals a row needs before it's Suggested (a single hot
 *  column isn't enough — averages hide a lone spike). */
export const SUGGEST_MIN_STRONG = 2;

export interface StructureSuggestion {
  /** Cleared both gates AND tripped ≥ SUGGEST_MIN_STRONG signals. */
  suggested: boolean;
  /** Excluded by the lift/size gate before signals were even weighed. */
  gated: boolean;
  /** Mean of the applicable normalized signals (0..1) — the shown %. */
  score: number;
  /** Human labels of the signals that crossed SUGGEST_STRONG (the "why"). */
  reasons: string[];
}

/** Composite bot-likelihood verdict for one structure. `contagion` is the
 *  client-side Contagion% for this row (or null when nothing's checked / it's
 *  itself checked); it substitutes for Wash on one-sided rows, whose Wash is
 *  meaningless (see DISCOVERY_COL_HELP.side / .suggested). */
export function suggestStructure(
  s: FlowDiscoveryStructure,
  side: 'buy' | 'sell' | 'both',
  contagion: number | null,
): StructureSuggestion {
  const gated = s.group_lift < SUGGEST_MIN_LIFT || s.gross_sol < SUGGEST_MIN_GROSS;
  const signals: { label: string; value: number }[] = [
    { label: 'Recur', value: clamp01(s.cross_token_recurrence / 100) },
    { label: 'Burst', value: clamp01(s.slot_burst / 100) },
    { label: 'Reuse', value: clamp01(s.wallet_reuse) },
  ];
  if (side === 'both') {
    signals.push({ label: 'Wash', value: clamp01(1 - s.wash_symmetry) });
  } else if (contagion != null) {
    signals.push({ label: 'Contagion', value: clamp01(contagion / 100) });
  }
  const strong = signals.filter((sig) => sig.value >= SUGGEST_STRONG);
  const score = signals.reduce((a, sig) => a + sig.value, 0) / signals.length;
  return {
    gated,
    score,
    reasons: strong.map((sig) => sig.label),
    suggested: !gated && strong.length >= SUGGEST_MIN_STRONG,
  };
}
