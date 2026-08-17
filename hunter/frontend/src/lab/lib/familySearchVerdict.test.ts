import { describe, expect, it } from 'vitest';

import { familyVerdict } from './familySearchVerdict';
import type { FamilyCandidateRow, FamilySearchReport } from './familySearchTypes';

const row = (retPct: number): FamilyCandidateRow => ({
  key: 'k',
  params: {},
  families: [],
  flags: [],
  fit_ret_pct: -1.24,
  target_ret_pct: retPct,
  target_pnl_sol: 0.31,
  target_n_tokens: 180,
  target_enter_pct: 0.68,
});

/** The charter's reference family: six cohorts varying `spendable_lamports_in`. */
function base(): FamilySearchReport {
  return {
    fingerprint_id: 'fp',
    fingerprint_name: '3ix:BuyExactSolIn · spend=5',
    family: {
      varied_axis: 'spendable_lamports_in',
      single_cohort: false,
      members: Array.from({ length: 6 }, (_, i) => ({
        fp_id: `m${i}`,
        name: `spend=${i}`,
        axis_value: i,
        is_target: i === 5,
        n_matched: 264,
        ungated_ret_pct: 1,
      })),
    },
    freshness: {
      last_trade_at: '2026-08-16T00:00:00Z',
      requested_until: '2026-08-16T00:10:00Z',
      shortfall_secs: 600,
      slack_secs: 3600,
      stale: false,
    },
    library: { n_candidates: 40, dropped_by_quota: 3, by_family: [['flow', 12]] },
    rho: 0.833,
    fit_broad_holds: true,
    draft: row(31.0),
    ungated_control: row(12.4),
    capture: {
      capture_pct: 31,
      n_with_upside: 180,
      n_no_upside: 84,
      oracle_pnl_sol: 1,
      realized_pnl_sol: 0.31,
    },
    incumbent: null,
    attribution: [],
    attribution_other_n: 0,
    attribution_other_pnl_sol: 0,
    narrow_recheck: [],
    entry_gates: [],
    archive: [],
    portrait: [],
    diagnostics: [],
  };
}

describe('familyVerdict', () => {
  it('promotes only when the family, the transfer and the ungated control all clear', () => {
    const v = familyVerdict(base());
    expect(v.tone).toBe('success');
    expect(v.label).toBe('Promotable draft');
    // The edge is percentage POINTS between two percents, never a percent itself.
    expect(v.edgePp).toBeCloseTo(18.6, 5);
    expect(v.gates.every((g) => g.ok !== false)).toBe(true);
  });

  it('reports the ungated control winning ahead of every other gate', () => {
    // A rho that also failed must not steal the headline: a gate that costs money
    // is actionable on its own, and the ordering is beside the point once it does.
    const r = base();
    r.ungated_control = row(40.0);
    r.rho = 0.1;
    r.fit_broad_holds = false;
    const v = familyVerdict(r);
    expect(v.label).toBe('Gate adds nothing');
    expect(v.edgePp).toBeLessThan(0);
  });

  it('never calls a family of one validated, however well the draft paid', () => {
    const r = base();
    r.family.single_cohort = true;
    r.family.varied_axis = null;
    r.family.members = [r.family.members[0]];
    r.rho = null;
    r.fit_broad_holds = false;
    const v = familyVerdict(r);
    expect(v.label).toBe('Single cohort');
    expect(v.tone).toBe('warning');
    // Not measurable is distinct from failed — the transfer gate must not read as a ✕.
    expect(v.gates.find((g) => g.key === 'transfer')?.ok).toBeNull();
    expect(v.gates.find((g) => g.key === 'family')?.ok).toBe(false);
  });

  it('says the ordering is unestablished when rho collapses on a real family', () => {
    const r = base();
    r.rho = 0.12;
    r.fit_broad_holds = false;
    const v = familyVerdict(r);
    expect(v.label).toBe('Ordering unvalidated');
    expect(v.gates.find((g) => g.key === 'transfer')?.ok).toBe(false);
  });

  it('has no verdict to give without a draft', () => {
    const r = base();
    r.draft = null;
    const v = familyVerdict(r);
    expect(v.tone).toBe('danger');
    expect(v.edgePp).toBeNull();
    expect(v.gates.find((g) => g.key === 'beats-ungated')?.ok).toBeNull();
  });
});
