//! Wire types for the family-search job result — the TS mirror of `hunter-lab`'s
//! `family_search::report::Report`.
//!
//! The one field pair a reader must never conflate is on {@link FamilyCandidateRow}:
//! `fit_ret_pct` produced the ORDERING and is rank-only; `target_ret_pct` is the
//! LEVEL and the only number to quote. On the reference family every candidate is
//! negative on the fit set while the winner pays +31% on the held-out target.

/** One family member and what the cohort itself pays, before any rule. */
export interface FamilySiblingRow {
  fp_id: string;
  name: string;
  /** The varied axis's value (SOL for a lamports axis). Null on a family of one. */
  axis_value: number | null;
  /** The held-out cohort the reported level comes from. */
  is_target: boolean;
  n_matched: number;
  /** The ungated control on this cohort: what it pays with no gate at all. */
  ungated_ret_pct: number | null;
}

/** One candidate, scored broad (rank) and narrow (level). */
export interface FamilyCandidateRow {
  key: string;
  params: Record<string, unknown>;
  /** End-event families the exit draws on. */
  families: string[];
  /** Non-empty ⇒ show the flag. A price-trail term always carries one. */
  flags: string[];
  /** Pooled `Σpnl ÷ Σentry` across the FIT cohorts. Rank-only — never a level. */
  fit_ret_pct: number;
  /** Held-out TARGET cohort return — the number to report. */
  target_ret_pct: number;
  target_pnl_sol: number;
  target_n_tokens: number;
  /** Share of the target cohort's matched tokens this candidate entered (0..1). */
  target_enter_pct: number;
}

/** One entry clause measured against the family's varied fingerprint axis. */
export interface FamilyEntryGateRow {
  clause: string;
  rho: number | null;
  refused: boolean;
  reason: string | null;
}

/** One authored exit term's share of the finalist's outcome. */
export interface FamilyAlarmRow {
  slot: number;
  label: string | null;
  n: number;
  pnl_sol: number;
  entry_sol: number;
  /** Money over capital — a count-only table ranks 200 small losses over 20 wins. */
  pnl_pct: number;
  /** The threshold the rule authored. */
  authored_level: number | null;
  /** Mean GROSS return at the close — the quantity `m_position.pnl` itself reads. */
  realized_level_pct: number | null;
  /** Only then may the two be printed side by side (percent vs seconds). */
  level_is_return: boolean;
  /** Points past the authored level the term actually closed. A `pnl <= -8` that
   *  realizes −20 is a stop that does not stop: price gaps straight through it. */
  level_overshoot_pp: number | null;
}

/** The same rule and the same taken set, priced two ways (D8 corollary). */
export interface FamilySpread {
  authority_ret_pct: number;
  optimistic_ret_pct: number;
  spread_pp: number;
  n_common: number;
  n_authority_only: number;
  n_optimistic_only: number;
  /** Both passes closed the same positions, so the numbers describe one set. */
  clean: boolean;
  /** The edge is no larger than the run's own uncertainty about the fill. */
  fill_luck: boolean;
}

/** Whether the cohort can pay for its own execution, measured before any rule (D8). */
export interface FamilyCostClearance {
  /** One round trip's cost at this buy size and the cohort's median pool depth. */
  band_pct: number;
  /** Median NET oracle round trip over every priceable entry, losers included. */
  median_move_pct: number | null;
  /** `median_move_pct ÷ band_pct` — round trips of headroom the best exit leaves. */
  headroom: number | null;
  n_priced: number;
  n_with_upside: number;
  margin: number;
  /** The search never ran: this cohort is untradeable at this buy size. */
  refused: boolean;
  /** It clears, but by less than one round trip. */
  thin: boolean;
  reason: string | null;
}

/** One entry clause's effect on the entry INSTANT. Diagnostic, never a refusal. */
export interface FamilyEntryTimingRow {
  clause: string;
  /** Seconds the clause adds to the mean entry delay. Positive ⇒ it holds entries back. */
  delay_added_secs: number;
  /** Points of capture the clause costs. Negative ⇒ removing it captured more. */
  capture_delta_pp: number | null;
  admit_delta_pct: number;
  /** Binds the instant AND its entries have less upside left — a gate the move creates. */
  lagging: boolean;
  note: string | null;
}

/** One term's narrow contribution — the finalist re-scored with it dropped. */
export interface FamilyTermRow {
  label: string;
  ret_full_pct: number;
  ret_without_pct: number;
  delta_pct: number;
  /** Dropping it changed nothing at all on this cohort. */
  inert: boolean;
}

/** How much of the money that was available the finalist took (D3). */
export interface FamilyCaptureDto {
  capture_pct: number | null;
  n_with_upside: number;
  n_no_upside: number;
  oracle_pnl_sol: number;
  realized_pnl_sol: number;
}

/** How fresh the scanned data is, against the range the run asked for (D7). */
export interface FamilyFreshnessDto {
  last_trade_at: string | null;
  requested_until: string;
  shortfall_secs: number;
  slack_secs: number;
  stale: boolean;
}

/** The family, as a run resolved it. */
export interface FamilyDto {
  /** The `fingerprints` column the members differ on. Null ⇒ a family of one. */
  varied_axis: string | null;
  /** Nothing to fit across and nothing to hold out — the run degraded and says so. */
  single_cohort: boolean;
  members: FamilySiblingRow[];
}

/** What the candidate library actually covered. */
export interface FamilyLibraryDto {
  n_candidates: number;
  /** Candidates the per-family quota turned away. Reported, never silent. */
  dropped_by_quota: number;
  /** `[end-event family, slots taken]`. */
  by_family: [string, number][];
}

export interface FamilySearchReport {
  fingerprint_id: string;
  fingerprint_name: string;
  family: FamilyDto;
  freshness: FamilyFreshnessDto;
  library: FamilyLibraryDto;
  /** Spearman(fit rank, held-out rank) — the PROCEDURE's self-test, not a result. */
  rho: number | null;
  /** False ⇒ fit-broad does not hold here; the ordering is unestablished. */
  fit_broad_holds: boolean;
  /** The finalist: best pooled fit, level from the target cohort. */
  draft: FamilyCandidateRow | null;
  /** What the fingerprint pays with NO gate — a property of the cohort. */
  ungated_control: FamilyCandidateRow | null;
  capture: FamilyCaptureDto;
  /** Whether the cohort clears its own execution cost. Measured on the ungated
   *  control before the generator runs — `refused` means no search happened. */
  cost_clearance: FamilyCostClearance | null;
  /** The draft, priced twice on one taken set. */
  spread: FamilySpread | null;
  /** What each entry clause does to the entry instant. */
  entry_timing: FamilyEntryTimingRow[];
  /** Display only. An incumbent is an artifact, not a baseline. */
  incumbent: FamilyCandidateRow | null;
  attribution: FamilyAlarmRow[];
  attribution_other_n: number;
  attribution_other_pnl_sol: number;
  narrow_recheck: FamilyTermRow[];
  entry_gates: FamilyEntryGateRow[];
  archive: FamilyCandidateRow[];
  /** Plain-language sentences in creator terms — the product. */
  portrait: string[];
  diagnostics: string[];
}

/** `GET …/family-search/{run_id}` and `/last` both return this envelope. */
export interface FamilySearchResult {
  run_id: string;
  result: FamilySearchReport;
}

/** The varied axis, pinned. Absent ⇒ the backend resolves it from the table. */
export type FamilyAxisName =
  | 'cu_limit'
  | 'cu_price'
  | 'init_buy'
  | 'max_cost'
  | 'spendable_in'
  | 'first_slot_buy'
  | 'first_slot_sell';

/**
 * Body for `POST /api/strategies/family-search`.
 *
 * Every knob a run reads is here — that is charter D5 made structural. There is
 * deliberately no path by which a saved rule supplies a buy size, a cap, or a
 * threshold; `incumbent_rule_id` renders one display column and nothing else.
 */
export interface FamilySearchStartArgs {
  fingerprint_id: string;
  created_after?: string;
  created_before?: string;
  buy_amount_sol?: number;
  fill_model?: string;
  cost_model?: string;
  skip_duplicate_identity?: boolean;
  max_concurrent_tokens?: number;
  max_total_tokens?: number;
  token_cap?: number;
  varied_axis?: FamilyAxisName;
  slots?: number;
  freshness_slack_secs?: number;
  /** Round trips of headroom the cohort's typical best exit must leave before a
   *  search runs. `0` refuses only the unarguable case. */
  cost_clearance_margin?: number;
  incumbent_rule_id?: string;
}
