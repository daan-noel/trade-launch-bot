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
  /** Its win rate — the bar an entry gate has to beat on the target cohort. */
  ungated_win_pct: number | null;
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
  /** Held-out win rate — the SAFETY half. Entry decides safety, exit decides profit,
   *  so a board printing only return grades half the rule. */
  target_win_pct: number | null;
  target_n_closed: number;
  /** Entry IDEAS, not clauses: a band (floor + ceiling on one quantity) counts once.
   *  This is why a rule showing five entry metrics carries three. */
  n_entry_quantities: number;
  /** Searched alarms in the exit OR, standing terms excluded. */
  n_alarms: number;
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
  n_wins: number;
  /** Wins over closes for this alarm. One large win hides a hundred small losses in
   *  a money column; this is what separates them. */
  win_rate_pct: number | null;
  /** A mechanical exit the operator always wants (sell at migration), not a
   *  discovered alarm — reported so the closes add up, labelled so it is never read
   *  as an edge. */
  standing: boolean;
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
  /** Which side it sits on — the two are graded by different jobs and must never be
   *  ranked in one list: entry buys safety, exit buys profit. */
  is_entry: boolean;
  ret_full_pct: number;
  ret_without_pct: number;
  delta_pct: number;
  win_full_pct: number | null;
  win_without_pct: number | null;
  /** Win-rate points the term is worth — the entry side's own currency. */
  win_delta_pp: number | null;
  /** Dropping it changed nothing at all on this cohort. */
  inert: boolean;
  /** It pays in the currency of its own side. */
  earns_its_place: boolean;
}

/** One idea the enrich stage offered the skeleton, and what it did. */
export interface FamilyEnrichRow {
  label: string;
  is_entry: boolean;
  ret_before_pct: number;
  ret_after_pct: number;
  ret_delta_pct: number;
  win_before_pct: number | null;
  win_after_pct: number | null;
  win_delta_pp: number | null;
  n_closed_after: number;
  accepted: boolean;
  /** Why it was refused. Null when accepted — density is never silent either way. */
  refused: string | null;
}

/** The two-sided bars and what they decided. */
export interface FamilySelection {
  /** The bar the entry side had to clear: the stricter of the operator's floor and
   *  the ungated control's own win rate. */
  win_bar_pct: number;
  control_win_pct: number | null;
  floor_win_pct: number;
  min_closed: number;
  /** Candidates the bars turned away before one cleared. */
  n_rejected: number;
  /** Why the ranking's own head is not the draft. Empty when they agree. */
  top_rejected: string[];
  /** Nothing cleared both bars — there is no draft, and the reason is above. */
  none_cleared: boolean;
  /** Lower bound of the 95% Wilson interval on the draft's held-out win rate. */
  draft_win_low_pct: number | null;
  /** The draft clears the win bar as a point estimate but its lower bound does not —
   *  the safety edge is inside this sample's noise. Diagnostic: it never un-selects. */
  win_within_noise: boolean;
}

/** One threshold tried on a clause's ladder. */
export interface FamilyLadderPoint {
  threshold: number;
  ret_pct: number;
  win_pct: number | null;
  n_closed: number;
  /** The finalist's own value — the point the verdict is about. */
  chosen: boolean;
}

/** What moving one clause's threshold does, everything else held fixed. */
export interface FamilyThresholdLadder {
  clause: string;
  is_entry: boolean;
  /** `flat` (the cohort barely responds) · `plateau` (the cut is a region) ·
   *  `fragile` (one lucky value — a step either way gives back half the range) ·
   *  `sparse` (too few measurable points to say). */
  verdict: string;
  /** Ascending by threshold, the chosen point included. */
  points: FamilyLadderPoint[];
}

/** One alarm's closes against its two counterfactuals. */
export interface FamilyAlarmRegret {
  slot: number;
  label: string | null;
  standing: boolean;
  n: number;
  /** Closes with a print after them to grade against. */
  n_priced: number;
  realized_ret_pct: number;
  best_later_ret_pct: number;
  /** Points the best still-available exit beat the realized close by. */
  forfeit_pp: number;
  n_terminal: number;
  /** Realized minus hold-to-the-end. Positive ⇒ firing beat holding on. */
  realized_vs_terminal_pp: number;
  band_pct: number;
  /** `timed` · `protective` · `premature` · `unmeasured`. */
  verdict: string;
}

/** One entry clause's standing in the AND: what it filters alone, and how much of
 *  that filtering a sibling clause already does. */
export interface FamilyEntryRedundancy {
  clause: string;
  /** Tokens this clause alone turns away from the full rule's entry set. */
  n_vetoed: number;
  solo_ret_pct: number;
  solo_win_pct: number | null;
  solo_n_closed: number;
  max_overlap_pct: number | null;
  overlap_with: string | null;
  /** Nearly all its vetoes are also another clause's — dead weight that drop-one
   *  ablation cannot see, because the sibling covers for it. */
  redundant: boolean;
}

/** One clause's drop-one contribution under both pricings, in its side's currency. */
export interface FamilyClauseFill {
  clause: string;
  is_entry: boolean;
  delta_authority: number | null;
  delta_optimistic: number | null;
  /** Flips sign or keeps under a quarter of itself between the two pricings — the
   *  contribution is fill luck, not signal. */
  fill_dependent: boolean;
}

/** How much of the money that was available the finalist took (D3). */
export interface FamilyCaptureDto {
  capture_pct: number | null;
  n_with_upside: number;
  n_no_upside: number;
  /** `n_no_upside` as a share of the entries — the ENTRY's own score, readable with
   *  no exit rule at all. Against the ungated control's share it says whether the
   *  gate filters losers or merely trades less. */
  no_upside_pct: number | null;
  oracle_pnl_sol: number;
  realized_pnl_sol: number;
}

/** How fresh the scanned data is, against the range the run asked for (D7). */
export interface FamilyFreshnessDto {
  last_trade_at: string | null;
  /** The upper bound the request named. Null ⇒ open-ended: the lake's tail is the
   *  range, so there is nothing for it to fall short of. */
  requested_until: string | null;
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
  /** The finalist: highest-ranked candidate clearing BOTH the win-rate and the
   *  return bar on the held-out cohort. Null when nothing cleared. */
  draft: FamilyCandidateRow | null;
  /** What the bars decided, and why the ranking's head is or is not the draft. */
  selection: FamilySelection | null;
  /** What the fingerprint pays with NO gate — a property of the cohort. */
  ungated_control: FamilyCandidateRow | null;
  capture: FamilyCaptureDto;
  /** The same for the ungated control — the entry side's comparison. */
  ungated_capture: FamilyCaptureDto | null;
  /** Standing exit terms this run carried, as the operator wrote them. */
  standing_terms: string[];
  /** Every idea the enrich stage offered the skeleton, accepted or refused. */
  enrich: FamilyEnrichRow[];
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
  /** Each clause's threshold replayed at neighbouring cuts — plateau or spike. */
  threshold_ladders: FamilyThresholdLadder[];
  /** Each alarm's closes against the best exit still available after them, and
   *  against holding to the last print. */
  alarm_regret: FamilyAlarmRegret[];
  /** Solo scores and veto-set overlap per entry clause. */
  entry_redundancy: FamilyEntryRedundancy[];
  /** Each clause's drop-one contribution under both pricings. */
  fill_sensitivity: FamilyClauseFill[];
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
  /** Mechanical alarms that ride into EVERY candidate and the ungated control, written
   *  as the board prints them (`"liquidity >= 85"`). None is searched, ablated or
   *  credited; one that does not parse fails the run rather than being dropped. */
  standing_exit?: string[];
  /** Absolute win-rate floor in percent, on top of the ungated control's own rate. */
  min_win_rate_pct?: number;
  /** Closes a candidate needs before its win rate is believed. */
  min_closed?: number;
  incumbent_rule_id?: string;
}
