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
  target_win_pct: 62,
  target_n_closed: 180,
  n_entry_quantities: 3,
  n_alarms: 4,
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
        ungated_win_pct: 45,
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
      no_upside_pct: (100 * 84) / 264,
      oracle_pnl_sol: 1,
      realized_pnl_sol: 0.31,
    },
    selection: {
      win_bar_pct: 45,
      control_win_pct: 45,
      floor_win_pct: 0,
      min_closed: 8,
      n_rejected: 2,
      top_rejected: [],
      none_cleared: false,
      // 62% of 180 closes: the lower bound clears the 45% bar comfortably.
      draft_win_low_pct: 54.8,
      win_within_noise: false,
    },
    ungated_capture: {
      capture_pct: 12,
      n_with_upside: 200,
      n_no_upside: 180,
      no_upside_pct: (100 * 180) / 380,
      oracle_pnl_sol: 2,
      realized_pnl_sol: 0.24,
    },
    standing_terms: [],
    enrich: [],
    cost_clearance: {
      band_pct: 6.93,
      median_move_pct: 24.0,
      headroom: 24.0 / 6.93,
      n_priced: 264,
      n_with_upside: 180,
      margin: 0,
      refused: false,
      thin: false,
      reason: null,
    },
    spread: {
      authority_ret_pct: 31.0,
      optimistic_ret_pct: 37.9,
      spread_pp: 6.9,
      n_common: 180,
      n_authority_only: 0,
      n_optimistic_only: 0,
      clean: true,
      fill_luck: false,
    },
    entry_timing: [],
    incumbent: null,
    attribution: [],
    attribution_other_n: 0,
    attribution_other_pnl_sol: 0,
    narrow_recheck: [],
    // A clean run: every cut a region, every alarm timed, every contribution
    // surviving the reprice. The tiers below flip one of these at a time.
    threshold_ladders: [
      {
        clause: 'untagged_buy(2s) >= 1.6',
        is_entry: false,
        verdict: 'plateau',
        points: [
          { threshold: 1.36, ret_pct: 29.5, win_pct: 61, n_closed: 178, chosen: false },
          { threshold: 1.6, ret_pct: 31.0, win_pct: 62, n_closed: 180, chosen: true },
          { threshold: 1.84, ret_pct: 28.9, win_pct: 60, n_closed: 182, chosen: false },
        ],
      },
    ],
    alarm_regret: [
      {
        slot: 0,
        label: 'stall >= 30',
        standing: false,
        n: 40,
        n_priced: 36,
        realized_ret_pct: 8,
        best_later_ret_pct: 10,
        forfeit_pp: 2,
        n_terminal: 40,
        realized_vs_terminal_pp: 60,
        band_pct: 6.93,
        verdict: 'timed',
      },
    ],
    entry_redundancy: [
      {
        clause: 'liquidity > 30',
        n_vetoed: 40,
        solo_ret_pct: 10,
        solo_win_pct: 55,
        solo_n_closed: 90,
        max_overlap_pct: 35,
        overlap_with: 'time >= 20',
        redundant: false,
      },
    ],
    fill_sensitivity: [
      {
        clause: 'untagged_buy(2s) >= 1.6',
        is_entry: false,
        delta_authority: 6.0,
        delta_optimistic: 5.4,
        fill_dependent: false,
      },
    ],
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

  it('says the search never ran, not that it found nothing, when the cohort is refused', () => {
    const r = base();
    r.cost_clearance = {
      ...r.cost_clearance!,
      median_move_pct: -0.4,
      headroom: -0.4 / 6.93,
      refused: true,
      reason: 'the typical entry pays -0.40% against a 6.93% round trip',
    };
    r.draft = null;
    const v = familyVerdict(r);
    expect(v.label).toBe('Cohort refused');
    expect(v.tone).toBe('danger');
    // The reason carries the measurement, so the board is not a bare "no".
    expect(v.body).toContain('6.93');
    expect(v.gates.find((g) => g.key === 'execution')?.ok).toBe(false);
  });

  it('puts fill luck ahead of what the number says', () => {
    // The edge is smaller than the run's own uncertainty about the fill, so whether
    // it beats the ungated control is beside the point.
    const r = base();
    r.spread = { ...r.spread!, authority_ret_pct: 4.0, spread_pp: 6.9, fill_luck: true };
    const v = familyVerdict(r);
    expect(v.label).toBe('Priced on fill luck');
    expect(v.edgePp).toBeGreaterThan(0);
  });

  it('refuses to call a thin cohort promotable', () => {
    // Everything else clears, but the whole cohort clears its round trip by 0.9x —
    // and a rule only ever takes a fraction of the best available exit.
    const r = base();
    r.cost_clearance = { ...r.cost_clearance!, median_move_pct: 6.2, headroom: 0.9, thin: true };
    const v = familyVerdict(r);
    expect(v.label).toBe('Thin headroom');
    expect(v.tone).toBe('warning');
    expect(v.gates.find((g) => g.key === 'execution')?.ok).toBe(false);
  });

  it('has no verdict to give without a draft', () => {
    const r = base();
    r.draft = null;
    const v = familyVerdict(r);
    expect(v.tone).toBe('danger');
    expect(v.edgePp).toBeNull();
    expect(v.gates.find((g) => g.key === 'beats-ungated')?.ok).toBeNull();
  });

  it('refuses to call a draft promotable when its own clauses do not hold up', () => {
    // Everything structural clears — the tier exists for exactly this case: a rule
    // that survived the family and the ungated control on a threshold that only
    // works at one value.
    const r = base();
    r.threshold_ladders = [{ ...r.threshold_ladders[0], verdict: 'fragile' }];
    const v = familyVerdict(r);
    expect(v.label).toBe('Fragile draft');
    expect(v.tone).toBe('warning');
    expect(v.headline).toContain('spike');
    expect(v.gates.find((g) => g.key === 'robustness')?.ok).toBe(false);
    // The structural gates still read as passes — the finding is narrower than that.
    expect(v.gates.find((g) => g.key === 'transfer')?.ok).toBe(true);
    expect(v.gates.find((g) => g.key === 'beats-ungated')?.ok).toBe(true);
  });

  it('names every kind of reliability failure it found, not just the first', () => {
    const r = base();
    r.threshold_ladders = [{ ...r.threshold_ladders[0], verdict: 'fragile' }];
    r.alarm_regret = [{ ...r.alarm_regret[0], verdict: 'premature', forfeit_pp: 20 }];
    r.fill_sensitivity = [
      { ...r.fill_sensitivity[0], delta_optimistic: 0.4, fill_dependent: true },
    ];
    r.entry_redundancy = [
      { ...r.entry_redundancy[0], max_overlap_pct: 95, redundant: true },
    ];
    r.selection = { ...r.selection!, draft_win_low_pct: 41, win_within_noise: true };
    const v = familyVerdict(r);

    expect(v.label).toBe('Fragile draft');
    expect(v.robustness.fragile).toEqual(['untagged_buy(2s) >= 1.6']);
    expect(v.robustness.premature).toEqual(['stall >= 30']);
    expect(v.robustness.fillDependent).toEqual(['untagged_buy(2s) >= 1.6']);
    expect(v.robustness.redundant).toEqual(['liquidity > 30']);
    expect(v.robustness.winWithinNoise).toBe(true);
    // Repricing leads: whether the number is real outranks what it says.
    expect(v.headline.indexOf('repricing')).toBeGreaterThan(-1);
    expect(v.headline).toContain('cut winners early');
    expect(v.headline).toContain('sample noise');
  });

  it('never lets a standing term be accused of cutting winners', () => {
    // A mechanic the operator asked for is not a finding about the launch shape.
    const r = base();
    r.alarm_regret = [
      { ...r.alarm_regret[0], standing: true, verdict: 'premature', forfeit_pp: 35 },
    ];
    const v = familyVerdict(r);
    expect(v.robustness.premature).toEqual([]);
    expect(v.label).toBe('Promotable draft');
  });

  it('reports execution and fill luck ahead of any per-clause finding', () => {
    // A cohort that cannot pay for execution is a statement about whether there is
    // anything here at all; a fragile cut is a statement about a draft. Ordering.
    const r = base();
    r.threshold_ladders = [{ ...r.threshold_ladders[0], verdict: 'fragile' }];
    r.spread = { ...r.spread!, authority_ret_pct: 4.0, spread_pp: 6.9, fill_luck: true };
    const v = familyVerdict(r);
    expect(v.label).toBe('Priced on fill luck');
    // ...and the per-clause findings still ride along for the board to render.
    expect(v.robustness.fragile).toEqual(['untagged_buy(2s) >= 1.6']);
  });

  it('does not claim clauses held up when nothing measured them', () => {
    const r = base();
    r.threshold_ladders = [];
    r.alarm_regret = [];
    r.fill_sensitivity = [];
    r.entry_redundancy = [];
    const v = familyVerdict(r);
    expect(v.label).toBe('Promotable draft');
    // Not measurable is distinct from passed — the gate must not read as a ✓.
    expect(v.gates.find((g) => g.key === 'robustness')?.ok).toBeNull();
  });
});
