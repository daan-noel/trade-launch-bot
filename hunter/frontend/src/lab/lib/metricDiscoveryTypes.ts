//! Wire types for the metric-combo discovery pipeline result — the TS mirror of
//! `hunter-lab`'s `discovery::dto::PipelineDto`. Every field is a plain string /
//! number the Rust DTO already flattened, so nothing here recomputes.

/** One point on a metric's Layer-1 response curve. */
export interface ResponsePoint {
  value: number | null; // null = the `off` pick
  score: number | null;
  outcome: 'ranked' | 'below_min_closed' | 'no_fire';
  n_fired: number;
  n_closed: number;
}

export interface MetricResponse {
  side: 'entry' | 'exit';
  group: string;
  metric: string;
  operator: string;
  window_sec: number | null;
  verdict: 'keep' | 'drop_no_edge' | 'drop_spike' | 'drop_thin' | 'drop_no_baseline';
  baseline: number | null;
  lift: number | null;
  plateau: number | null;
  best_value: number | null;
  narrowed: number[];
  curve: ResponsePoint[];
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

export interface FamilyDto {
  combos_scanned: number;
  families: FamilyResult[];
  interactions: Interaction[];
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
  boundary: string | null;
  candidates: CandidateValidation[];
}

export interface PipelineDto {
  cohort_tokens: number;
  fit_tokens: number;
  screen: ScreenDto;
  family: FamilyDto;
  validation: ValidationDto | null;
  no_validation: string | null;
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
  field_filters?: Record<string, (number | boolean)[]>;
  buy_amount_sol?: number;
  take_profit_pct?: number | null;
  stop_loss_pct?: number | null;
  min_closed?: number;
  split_fraction?: number;
  entry_window_sec?: number;
  exit_window_sec?: number;
  volume_ix_patterns?: string[][];
}
