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
 */
import type { FamilySearchReport } from '@lab/lib/familySearchTypes';

export type FamilyVerdictTone = 'success' | 'warning' | 'danger';

/** One named check, with the number that decided it. `ok: null` = not measurable. */
export interface FamilyGate {
  key: 'execution' | 'family' | 'transfer' | 'beats-ungated' | 'freshness';
  label: string;
  ok: boolean | null;
  detail: string;
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
    };
  }

  return {
    tone: 'success',
    label: 'Promotable draft',
    headline: `The draft pays ${pctText(draft.target_ret_pct)} on the held-out cohort, ${pp(
      edgePp ?? 0,
    )} over buying everything.`,
    body: 'The ordering was fitted across the siblings and transferred to a cohort it never saw. Promote to an inactive paper rule, then Simulate before it touches real money.',
    gates,
    edgePp,
  };
}
