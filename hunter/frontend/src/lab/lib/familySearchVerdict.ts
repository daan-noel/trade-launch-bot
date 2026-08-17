/**
 * The one question the family-search board answers: **should this draft be
 * promoted?** — derived from the report and nothing else.
 *
 * The backend deliberately reports its gates apart rather than blending them into
 * a score, so this module blends nothing either: it names which gate decided, and
 * every gate is still rendered beside the headline with its own number. A reader
 * who disagrees with the headline can see exactly which line produced it.
 *
 * Freshness is a *fatal* backend gate (`check_freshness` bails), so a delivered
 * report is fresh by construction and the gate renders as a pass with its own
 * shortfall. It is kept here because the DTO carries the flag and a future
 * badge-instead-of-refuse mode must not read as a clean run.
 *
 * The reliability checks sit **last** among the verdict tiers, and deliberately so:
 * "this cohort cannot pay for execution" and "the gate costs money" are statements
 * about whether there is anything here at all, while "this threshold is a spike" is
 * a statement about a draft that already cleared everything structural. They also
 * never un-select — the backend keeps every diagnostic out of selection so the
 * held-out cohort is not leaked — so the worst they produce here is a warning.
 */
import type { FamilySearchReport } from '@lab/lib/familySearchTypes';

export type FamilyVerdictTone = 'success' | 'warning' | 'danger';

/** One named check, with the number that decided it. `ok: null` = not measurable. */
export interface FamilyGate {
  key: 'execution' | 'family' | 'transfer' | 'beats-ungated' | 'freshness' | 'robustness';
  label: string;
  ok: boolean | null;
  detail: string;
}

/** What the Slice 7 diagnostics found wrong with the draft, each already a verdict
 *  on the backend. Counting is all this layer does — it never re-derives one. */
export interface FamilyRobustness {
  /** Thresholds sitting on a spike rather than a plateau. */
  fragile: string[];
  /** Alarms that left money AND that holding on would have beaten. */
  premature: string[];
  /** Clauses whose contribution does not survive repricing. */
  fillDependent: string[];
  /** Entry clauses another clause already covers for. */
  redundant: string[];
  /** The win-rate clearance is inside this sample's noise. */
  winWithinNoise: boolean;
  /** Anything at all to report. */
  any: boolean;
}

/** Collect the backend's own reliability verdicts. Standing terms are excluded from
 *  the regret count: a mechanic the operator asked for is never a finding. */
export function familyRobustness(r: FamilySearchReport): FamilyRobustness {
  const fragile = (r.threshold_ladders ?? [])
    .filter((l) => l.verdict === 'fragile')
    .map((l) => l.clause);
  const premature = (r.alarm_regret ?? [])
    .filter((a) => !a.standing && a.verdict === 'premature')
    .map((a) => a.label ?? `slot ${a.slot}`);
  const fillDependent = (r.fill_sensitivity ?? [])
    .filter((f) => f.fill_dependent)
    .map((f) => f.clause);
  const redundant = (r.entry_redundancy ?? []).filter((x) => x.redundant).map((x) => x.clause);
  const winWithinNoise = r.selection?.win_within_noise === true;
  return {
    fragile,
    premature,
    fillDependent,
    redundant,
    winWithinNoise,
    any:
      fragile.length > 0 ||
      premature.length > 0 ||
      fillDependent.length > 0 ||
      redundant.length > 0 ||
      winWithinNoise,
  };
}

export interface FamilyVerdict {
  tone: FamilyVerdictTone;
  /** Short pill text. */
  label: string;
  /** The answer, in one line. */
  headline: string;
  /** What to do about it. */
  body: string;
  gates: FamilyGate[];
  /** Draft minus ungated control, in percentage points. Null when either is absent. */
  edgePp: number | null;
  /** What the reliability diagnostics found. Always populated, whatever decided the
   *  headline — a draft can be refused on execution and still have fragile cuts. */
  robustness: FamilyRobustness;
}

const pp = (n: number): string => `${n >= 0 ? '+' : ''}${n.toFixed(1)}pp`;
const pctText = (n: number): string => `${n >= 0 ? '+' : ''}${n.toFixed(1)}%`;

/** Spearman floor below which the pooled ordering is treated as untransferred. */
export const RHO_FLOOR = 0.5;

export function familyVerdict(r: FamilySearchReport): FamilyVerdict {
  const draft = r.draft;
  const ungated = r.ungated_control;
  const edgePp =
    draft && ungated ? draft.target_ret_pct - ungated.target_ret_pct : null;

  const nMembers = r.family.members.length;
  const familyOk = !r.family.single_cohort && nMembers > 1;
  const cc = r.cost_clearance;
  const rob = familyRobustness(r);
  const nMeasured =
    (r.threshold_ladders?.length ?? 0) +
    (r.alarm_regret?.length ?? 0) +
    (r.fill_sensitivity?.length ?? 0);
  const gates: FamilyGate[] = [
    {
      key: 'execution',
      label: 'Clears execution',
      ok: cc == null || cc.median_move_pct == null ? null : !cc.refused && !cc.thin,
      detail:
        cc == null || cc.median_move_pct == null
          ? 'the cohort\'s available upside was not measurable'
          : cc.refused
            ? `the typical entry's best available exit pays ${pctText(
                cc.median_move_pct,
              )} against a ${cc.band_pct.toFixed(2)}% round trip — no search was run`
            : `best available exit ${pctText(
                cc.median_move_pct,
              )} against a ${cc.band_pct.toFixed(2)}% round trip — ${(
                cc.headroom ?? 0
              ).toFixed(1)}x headroom${cc.thin ? ', under one round trip' : ''}`,
    },
    {
      key: 'family',
      label: 'Family',
      ok: familyOk,
      detail: familyOk
        ? `${nMembers} cohorts differing only in \`${r.family.varied_axis ?? 'one axis'}\` — ${
            nMembers - 1
          } fitted, 1 held out`
        : 'no sibling fingerprint — nothing was fitted across, nothing held out',
    },
    {
      key: 'transfer',
      label: 'Rank transfer',
      ok: r.rho == null ? null : r.fit_broad_holds,
      detail:
        r.rho == null
          ? 'no second cohort to check the ordering against'
          : r.fit_broad_holds
            ? `rho ${r.rho >= 0 ? '+' : ''}${r.rho.toFixed(2)} clears the ${RHO_FLOOR.toFixed(
                2,
              )} floor — the pooled ordering reached the held-out cohort`
            : `rho ${r.rho >= 0 ? '+' : ''}${r.rho.toFixed(2)} is below the ${RHO_FLOOR.toFixed(
                2,
              )} floor — the pooled ordering did not reach the held-out cohort`,
    },
    {
      key: 'beats-ungated',
      label: 'Beats ungated',
      ok: edgePp == null ? null : edgePp > 0,
      detail:
        edgePp == null
          ? 'no ungated control on this run'
          : `${pctText(draft!.target_ret_pct)} gated vs ${pctText(
              ungated!.target_ret_pct,
            )} buying everything — ${pp(edgePp)}`,
    },
    {
      key: 'robustness',
      label: 'Clauses hold up',
      ok: nMeasured === 0 ? null : !rob.any,
      detail:
        nMeasured === 0
          ? 'no per-clause diagnostics on this run'
          : rob.any
            ? `${robustnessSummary(rob)} — the draft's specifics are not load-bearing`
            : `every threshold is a plateau, every alarm timed, and every contribution survives repricing (${nMeasured} checks)`,
    },
    {
      key: 'freshness',
      label: 'Freshness',
      ok: !r.freshness.stale,
      detail: r.freshness.stale
        ? `the lake ends ${(r.freshness.shortfall_secs / 3600).toFixed(
            1,
          )}h before the requested bound`
        : `the lake reaches the requested bound (${(
            r.freshness.shortfall_secs / 3600
          ).toFixed(1)}h behind, ${(r.freshness.slack_secs / 3600).toFixed(1)}h allowed)`,
    },
  ];

  // The search never ran. That is a different statement from "the search found
  // nothing", and it must not read as one.
  if (cc?.refused) {
    return {
      tone: 'danger',
      label: 'Cohort refused',
      headline: 'This launch shape cannot pay for its own execution.',
      body:
        cc.reason ??
        'The typical entry\'s best available exit does not clear the round trip, so no exit rule can exist here. Nothing was generated.',
      gates,
      edgePp,
      robustness: rob,
    };
  }

  if (!draft) {
    return {
      tone: 'danger',
      label: 'No draft',
      headline: 'Nothing survived this run.',
      body: 'No candidate came out of the fit. Read the diagnostics below, then widen the range or raise the candidate slots.',
      gates,
      edgePp,
      robustness: rob,
    };
  }

  // Whether the number is real at all outranks what the number says.
  if (r.spread?.fill_luck) {
    return {
      tone: 'warning',
      label: 'Priced on fill luck',
      headline: `The same closes repriced at the friendliest honest fill swing ${pp(
        r.spread.spread_pp,
      )} — more than the edge itself.`,
      body: 'Execution, not signal, is deciding this result. Live paper books the pessimistic fill, so treat the draft as unproven until it clears its own spread — or trade a larger target size where the round trip is affordable overhead.',
      gates,
      edgePp,
      robustness: rob,
    };
  }

  // A gate that costs money is the sharpest finding on the page, whatever rho did.
  if (edgePp != null && edgePp <= 0) {
    return {
      tone: 'warning',
      label: 'Gate adds nothing',
      headline: 'Buying every matched token pays at least as well as this draft.',
      body: 'The edge on this launch shape is ungated — selecting within it costs money here. Promote the draft only if you want the exit bag; the entry side is not earning its place.',
      gates,
      edgePp,
      robustness: rob,
    };
  }

  if (!familyOk) {
    return {
      tone: 'warning',
      label: 'Single cohort',
      headline: `The draft pays ${pctText(draft.target_ret_pct)} on this cohort, but nothing was held out.`,
      body: 'This fingerprint has no sibling to fit across, so the ordering that picked the draft was never validated on unseen data. Treat the number as in-sample and confirm it with Simulate before promoting.',
      gates,
      edgePp,
      robustness: rob,
    };
  }

  if (!r.fit_broad_holds) {
    return {
      tone: 'warning',
      label: 'Ordering unvalidated',
      headline: 'The pooled ordering did not transfer to the held-out cohort.',
      body: 'Fitting broad does not apply to this family — the draft is the top of a ranking that means nothing here. Its level is still a real replay of the target cohort, so read it as one measured rule rather than as the winner of a search.',
      gates,
      edgePp,
      robustness: rob,
    };
  }

  // Everything else passed, but the cohort clears its own round trip by less than
  // one. A rule takes a fraction of the best available exit, so this is the shape
  // that looks tradeable offline and is not.
  if (cc?.thin) {
    return {
      tone: 'warning',
      label: 'Thin headroom',
      headline: `The draft pays ${pctText(draft.target_ret_pct)}, but the whole cohort clears its round trip by only ${(
        cc.headroom ?? 0
      ).toFixed(1)}x.`,
      body: 'Execution eats most of what is available on this launch shape, and a rule only ever takes a fraction of the best exit. Confirm with Simulate at the pessimistic fill before promoting, and expect the live number to sit below this one.',
      gates,
      edgePp,
      robustness: rob,
    };
  }

  // Everything structural passed. What is left is whether the draft's own numbers
  // are load-bearing: a cut that only works at one value, an alarm that cuts
  // winners, a contribution that is really the fill model, or a safety edge inside
  // the sample's noise. None of these un-selects the draft — the backend never lets
  // a diagnostic touch selection — but a reader must not promote past them.
  if (rob.any) {
    return {
      tone: 'warning',
      label: 'Fragile draft',
      headline: `The draft pays ${pctText(
        draft.target_ret_pct,
      )}, but ${robustnessSummary(rob)}.`,
      body: 'The structure held up; the specifics did not. Re-run on another family before promoting — a finding that survives a second launch shape is a thesis, and one that does not was this cohort\'s noise. The per-clause tables below name every failure.',
      gates,
      edgePp,
      robustness: rob,
    };
  }

  return {
    tone: 'success',
    label: 'Promotable draft',
    headline: `The draft pays ${pctText(draft.target_ret_pct)} on the held-out cohort, ${pp(
      edgePp ?? 0,
    )} over buying everything.`,
    body: 'The ordering was fitted across the siblings and transferred to a cohort it never saw, and every clause survived its own robustness check. Promote to an inactive paper rule, then Simulate before it touches real money.',
    gates,
    edgePp,
    robustness: rob,
  };
}

/** The reliability failures as one clause, worst first. */
function robustnessSummary(rob: FamilyRobustness): string {
  const parts: string[] = [];
  if (rob.fillDependent.length > 0) {
    parts.push(
      `${rob.fillDependent.length} clause${
        rob.fillDependent.length === 1 ? "'s contribution does" : "s' contributions do"
      } not survive repricing`,
    );
  }
  if (rob.premature.length > 0) {
    parts.push(`${rob.premature.length} alarm${plural(rob.premature.length)} cut winners early`);
  }
  if (rob.fragile.length > 0) {
    parts.push(
      `${rob.fragile.length} threshold${plural(rob.fragile.length)} sit${
        rob.fragile.length === 1 ? 's' : ''
      } on a spike`,
    );
  }
  if (rob.redundant.length > 0) {
    parts.push(
      `${rob.redundant.length} entry clause${plural(rob.redundant.length)} ${
        rob.redundant.length === 1 ? 'is' : 'are'
      } covered for by a sibling`,
    );
  }
  if (rob.winWithinNoise) {
    parts.push('its win-rate edge sits inside the sample noise');
  }
  return parts.join(', ');
}

const plural = (n: number): string => (n === 1 ? '' : 's');
