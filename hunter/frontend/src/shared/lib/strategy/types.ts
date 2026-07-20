// Wire types for the generic-engine fingerprint + rule HTTP surface
// (live-bin handlers `strategies::engine`). The HTTP boundary speaks **lamports**
// for every amount axis (the models store `BIGINT` lamports and the `*_sol()`
// accessors are non-serialized); `bucket_size_amount` is already SOL. The UI
// speaks SOL, so the form components convert with the helpers below.

/** 1 SOL in lamports — the one divisor for the fingerprint/rule amount axes. */
export const LAMPORTS_PER_SOL = 1_000_000_000;

/** Lamports (i64 wire) → human SOL, or `null` passthrough. */
export function lamportsToSol(l: number | null | undefined): number | null {
  return l == null ? null : l / LAMPORTS_PER_SOL;
}

/** Human SOL → lamports (i64 wire, rounded), or `null` passthrough. */
export function solToLamports(s: number | null | undefined): number | null {
  return s == null ? null : Math.round(s * LAMPORTS_PER_SOL);
}

/** A `fingerprints` row (response shape). All `*_lamports` axes are lamports. */
export interface Fingerprint {
  id: string;
  name: string;
  cu_limit: number | null;
  cu_price: number | null;
  init_buy_lamports: number | null;
  max_cost_lamports: number | null;
  spendable_lamports_in: number | null;
  first_slot_buy_lamports: number | null;
  first_slot_sell_lamports: number | null;
  /** SOL width of the match bucket (default 0.1). */
  bucket_size_amount: number;
  ix_labels: string[] | null;
  created_at: string;
  updated_at: string;
  /** How many rules reference this fingerprint — folded in by the list endpoint
   *  (`GET /api/fingerprints`); absent on single-row responses. */
  used_by?: number;
}

/** The create/update body for a fingerprint (lamports on the wire). `id`,
 *  `created_at`, `updated_at`, `used_by` are server-owned and never sent. */
export type FingerprintDraft = Omit<
  Fingerprint,
  'id' | 'created_at' | 'updated_at' | 'used_by'
>;

/** Trade modes for a rule. */
export type TradeMode = 'paper' | 'real';

/** A `strategy_rules` row (response shape). `buy_amount_lamports` is lamports;
 *  `params` is the canonical rule-params JSONB. */
export interface StrategyRule {
  id: string;
  rule_name: string;
  fingerprint_id: string;
  trade_mode: TradeMode;
  is_active: boolean;
  buy_amount_lamports: number;
  max_concurrent_tokens: number;
  max_total_tokens: number;
  params: Record<string, unknown>;
  created_at: string;
  updated_at: string;
}

/** POST /api/strategy-rules body. `fingerprint_id` required; `is_active` is
 *  forced false on create (lifecycle endpoints toggle it). */
export interface CreateRuleBody {
  rule_name: string;
  fingerprint_id: string;
  trade_mode: TradeMode;
  buy_amount_lamports: number;
  max_concurrent_tokens: number;
  max_total_tokens: number;
  params: Record<string, unknown>;
}

/** PUT /api/strategy-rules/{id} patch — `fingerprint_id`/`is_active` NOT patchable. */
export type UpdateRuleBody = Partial<
  Pick<
    CreateRuleBody,
    'rule_name' | 'trade_mode' | 'buy_amount_lamports' | 'max_concurrent_tokens' | 'max_total_tokens' | 'params'
  >
>;

/** `POST /api/strategies/sweeps/{run}/groups/{group}/promote` response (sweep
 *  redesign 5.6): a ready-to-save rule draft the editor opens (StrategyRule-shaped
 *  but id-less — save creates it), plus the find-or-created `fingerprint` echoed so
 *  the editor renders its criteria without a refetch. Amounts are lamports. */
export interface PromotedRuleDraft {
  rule_name: string;
  fingerprint_id: string;
  trade_mode: TradeMode;
  buy_amount_lamports: number;
  max_concurrent_tokens: number;
  max_total_tokens: number;
  params: Record<string, unknown>;
  fingerprint: Fingerprint;
}

/** One armed (token, rule) pair from `GET /api/strategies/armed`. */
export interface ArmedEntry {
  rule_id: string;
  mint_address: string;
  state: string;
}

/** One computed metric column from `GET /api/tokens/{mint}/metric-series`. */
export interface MetricSeriesColumn {
  metric: string;
  group: string;
  unit: string;
  /** Present only for dynamic (`m_time_window`) metrics. */
  window_size_sec: number | null;
  /** One value per event (aligned with `at`); non-finite ⇒ `null`. */
  values: Array<number | null>;
}

/** `GET /api/tokens/{mint}/metric-series` response — every metric's value at every
 *  trade, as parallel arrays. Computed on demand (never persisted). Lab-only. */
export interface MetricSeriesResponse {
  mint_address: string;
  /** RFC3339 timestamps aligned with every column's `values`. */
  at: string[];
  /** Spot price (SOL) at each event — aligned with `at`; non-finite ⇒ `null`. */
  price?: Array<number | null>;
  series: MetricSeriesColumn[];
}

/** Inline dry-run draft for `POST /api/strategies/simulate`. NOTE: this uses
 *  `buy_amount_sol` (f64 SOL) — the one amount that is SOL, not lamports, on the
 *  wire (the simulate handler's draft contract). */
export interface EngineRuleDraft {
  fingerprint_id: string;
  params: Record<string, unknown>;
  buy_amount_sol: number;
  max_concurrent_tokens?: number;
  max_total_tokens?: number;
  trade_mode?: TradeMode;
}

/** `POST /api/strategies/simulate` body — a saved rule (`rule_id`) or an inline
 *  `draft` (ignored if `rule_id` is set), over an optional creation window. */
export interface EngineSimRequest {
  rule_id?: string;
  draft?: EngineRuleDraft;
  since?: string;
  until?: string;
}

/** `202` response of `POST /api/strategies/simulate`. `run_id` = the rule id for
 *  a saved rule, or a fresh id for a draft; the `simulation_finished` SSE carries
 *  it back as `rule_id`. */
export interface SimStartResponse {
  started: boolean;
  run_id: string;
}

/** `strategy_armed_changed` SSE payload — a (token, rule) arm/disarm transition. */
export interface ArmedChangedEvent {
  rule_id: string;
  mint_address: string;
  /** `"armed"` | `"disarmed"`. */
  state: string;
  /** Disarm reason (`dead` | `migrated` | `unsatisfiable`) when disarmed. */
  reason?: string | null;
}

/** `strategy_position_update` SSE payload — one generic-engine position delta. */
export interface StrategyPositionUpdateEvent {
  rule_id: string;
  mint_address: string;
  position_id: string;
  /** `strategy_positions` lifecycle: `BuySubmitted` | `Holding` | `ExitPending` |
   *  `End` | `ExitFailed` | `ExitUnconfirmed`. */
  status: string;
  exit_reason?: string | null;
  entry_price?: number | null;
  exit_price?: number | null;
  /** `"real"` | `"paper"` when the engine still has the rule loaded. */
  trade_mode?: string | null;
  rule_name?: string | null;
}
