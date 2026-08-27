//! Wire types for the metric-combo discovery pipeline result — the TS mirror of
//! `hunter-lab`'s `discovery::dto::PipelineDto`. Every field is a plain string /
//! number the Rust DTO already flattened, so nothing here recomputes.

import type { AxisSpecWire } from '@lab/components/sweep/genericAxes';
import type { FieldFilterValue } from '@lab/components/sweep/fingerprintFilters';

export type ScoreOutcome = 'ranked' | 'below_min_closed' | 'no_fire';

/** A TP/SL bracket. `null` on a side means that guard is omitted. */
export interface Bracket {
  take_profit_pct: number | null;
  stop_loss_pct: number | null;
}

/**
 * One scored row in score **and** in money. The discovery objective is unitless, so
 * `score` alone can only ever rank — these columns are what makes it checkable.
 */
export interface ScoredRow {
  score: number | null;
  outcome: ScoreOutcome;
  n_fired: number;
  n_closed: number;
  win_rate: number;
  median_pnl_pct: number;
  total_pnl_sol: number;
}

/** One point on a metric's Layer-1 response curve. */
export interface ResponsePoint {
  value: number | null; // null = the `off` pick
  score: number | null;
  outcome: ScoreOutcome;
  n_fired: number;
  n_closed: number;
  /** The gate this pick failed, when it was gated. */
  gate: number | null;
  win_rate: number;
  median_pnl_pct: number;
  total_pnl_sol: number;
}

export type ScreenVerdict =
  | 'keep'
  | 'drop_no_edge'
  /** Lifted the baseline but stayed unprofitable — signal on a bracket that doesn't fit. */
  | 'drop_negative'
  | 'drop_spike'
  | 'drop_thin'
  | 'drop_no_baseline';

export interface MetricResponse {
  side: 'entry' | 'exit';
  group: string;
  metric: string;
  operator: string;
  /** The span this metric was screened at, labelled (`30s`, `30sl@1`, `20p`).
   *  Prefer this over {@link window_sec}. */
  window?: string | null;
  /** Legacy seconds scalar. Null on a slot or print span - neither has seconds to
   *  report, so a reader that only knows this key drops the qualifier rather than
   *  calling 30 slots 30 seconds. */
  window_sec: number | null;
  verdict: ScreenVerdict;
  baseline: number | null;
  lift: number | null;
  /** Best pick's absolute score — separates "lifted into profit" from "still losing". */
  best_score: number | null;
  plateau: number | null;
  best_value: number | null;
  narrowed: number[];
  curve: ResponsePoint[];
}

export interface BaselineCandidate extends ScoredRow {
  bracket: Bracket;
}

/** Layer 0 — the measured bracket table. */
export interface BaselineSelection {
  chosen: Bracket;
  chosen_index: number;
  all_unprofitable: boolean;
  combos_scanned: number;
  candidates: BaselineCandidate[];
}

export interface SkippedMetric {
  side: string;
  group: string;
  metric: string;
  reason: string;
}

export interface ScreenDto {
  cohort_tokens: number;
  combos_scanned: number;
  n_gated: number;
  /** The bracket every `lift` on the page is measured against. */
  baseline: Bracket;
  /** What that bracket did bare — the reference line. */
  baseline_stats: ScoredRow | null;
  /** The min-N gate that actually ran, after cohort scaling. */
  effective_min_closed: number;
  /** The configured gate it was relaxed from (equal when it wasn't). */
  min_closed: number;
  shortlist: MetricResponse[];
  responses: MetricResponse[];
  skipped: SkippedMetric[];
  gaps: SkippedMetric[];
}

export interface FamilyMember {
  side: 'entry' | 'exit';
  group: string;
  metric: string;
  operator: string;
  values: number[];
  lift: number;
  /** Supplied by the synergy rescue — its lift is conditional on the pinned winner. */
  rescued: boolean;
}

/** One Layer-1 reject re-screened under a pinned winner. */
export interface Rescue {
  side: 'entry' | 'exit';
  group: string;
  metric: string;
  operator: string;
  pinned: string;
  pinned_score: number;
  /** Same tags as a Layer-1 verdict — `keep` means the rescue succeeded. */
  verdict: ScreenVerdict;
  lift: number | null;
}

export interface DroppedMember {
  metric: string;
  reason: string;
}

export interface BestCombo {
  score: number;
  n_fired: number;
  n_closed: number;
  picks: (number | null)[];
  params: Record<string, unknown>;
}

export interface FamilyResult {
  family: string;
  combos: number;
  n_gated: number;
  members: FamilyMember[];
  dropped: DroppedMember[];
  best: BestCombo | null;
}

export interface Interaction {
  pinned: string;
  swept: string;
  verdict: 'independent' | 'interacting' | 'inconclusive';
  alone: (number | null)[];
  given: (number | null)[];
  score_alone: number;
  score_given: number | null;
}

export interface JointResult {
  families: string[];
  combos: number;
  n_gated: number;
  members: FamilyMember[];
  dropped: DroppedMember[];
  best: BestCombo | null;
}

export interface FamilyDto {
  combos_scanned: number;
  families: FamilyResult[];
  interactions: Interaction[];
  /** Present on new runs; older cached results may omit it. */
  joints?: JointResult[];
  /** Present on new runs; older cached results may omit it. */
  rescues?: Rescue[];
}

export interface SliceScore {
  tokens: number;
  score: number | null;
  outcome: 'ranked' | 'below_min_closed' | 'no_fire';
  n_fired: number;
  n_closed: number;
  win_rate: number;
  median_pnl_pct: number;
  total_pnl_sol: number;
}

export type ValidationVerdict =
  | 'holds'
  | 'degraded'
  | 'failed'
  | 'thin_validate'
  | 'no_fire_validate'
  | 'unrankable_train';

export interface CandidateValidation {
  label: string;
  verdict: ValidationVerdict;
  retention: number | null;
  train: SliceScore;
  validate: SliceScore;
  params: Record<string, unknown>;
}

export interface ValidationDto {
  train_tokens: number;
  validate_tokens: number;
  /**
   * Closed trades a candidate needs on the held-out slice to return a verdict at all.
   * Without it `thin_validate` reads as a verdict on the candidate when it is a
   * statement about the slice's size.
   */
  effective_min_closed: number;
  boundary: string | null;
  candidates: CandidateValidation[];
}

export interface SeedCluster {
  families: string[];
  interacting: boolean;
  members: string[];
}

/** Discovery → grouped-sweep handoff (mirrors `discovery::seed::SweepSeed`). */
export interface SweepSeed {
  axes: AxisSpecWire[];
  optional_axes: AxisSpecWire[];
  clusters: SeedCluster[];
  combo_estimate: number;
  notes: string[];
}

/**
 * Payload written to sessionStorage when the user clicks "Open as sweep".
 * The generic sweep form applies it once, then clears the key.
 */
export interface DiscoverySweepHandoff {
  seed: SweepSeed;
  /** When true, merge `optional_axes` into the axis rows. */
  includeOptional: boolean;
  createdAfter: string;
  createdBefore: string;
  curveOnly: boolean;
  tokenCap: number;
  fingerprintId: string | null;
  buyAmountSol: number;
  volumeIxPatterns: string[][];
  ixLabelsFilter: string;
}

export interface PipelineDto {
  cohort_tokens: number;
  fit_tokens: number;
  /** The cohort was truncated to the newest `token_cap` matches. */
  cohort_capped?: boolean;
  token_cap?: number | null;
  /** Layer 0 — absent when the caller named a single baseline. */
  baseline_selection?: BaselineSelection | null;
  /** Run-level findings in plain language. Present on new runs. */
  diagnostics?: string[];
  screen: ScreenDto;
  family: FamilyDto;
  validation: ValidationDto | null;
  no_validation: string | null;
  /** Present on new runs; older cached `/last` results may omit it. */
  sweep_seed?: SweepSeed;
}

/** `GET …/metric-discovery/{run_id}` and `/last` both return this envelope. */
export interface MetricDiscoveryResult {
  run_id: string;
  result: PipelineDto;
}

/** Body for `POST /api/strategies/metric-discovery`. */
export interface MetricDiscoveryStartArgs {
  created_after?: string;
  created_before?: string;
  curve_only?: boolean;
  token_cap?: number;
  fingerprint_id?: string;
  ix_labels_filter?: string[];
  field_filters?: Record<string, FieldFilterValue[]>;
  buy_amount_sol?: number;
  take_profit_pct?: number | null;
  stop_loss_pct?: number | null;
  /**
   * Candidate TP rungs for baseline selection; crossed with `stop_loss_menu` into the
   * bracket grid Layer 0 measures. A `null` entry means "no take-profit guard".
   * Omit (or send one value) to screen against `take_profit_pct` directly.
   */
  take_profit_menu?: (number | null)[];
  stop_loss_menu?: (number | null)[];
  min_closed?: number;
  split_fraction?: number;
  /** A bare number is SECONDS - what this field has always meant; a string is a
   *  full span (`"30sl@1"`, `"20p"`) in the same grammar every other span uses. */
  entry_window_sec?: number | string;
  exit_window_sec?: number | string;
  volume_ix_patterns?: string[][];
}
