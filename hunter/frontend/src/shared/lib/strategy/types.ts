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

/**
 * Smallest legal fingerprint bucket width (SOL) — mirrors Rust
 * `grouping::MIN_BUCKET_WIDTH_SOL`, enforced by `Fingerprint::validate` and the
 * `fingerprints_bucket_size_amount_positive` CHECK.
 *
 * A width of `0` is **invalid**, not "unset": the matcher divides by it raw, so
 * every positive amount saturates into one bucket and the fingerprint's SOL axes
 * stop discriminating. Note the contrast with the axis VALUES on the same form —
 * there `0` is a real bucket (`[0, width)`) and `null` means "not part of
 * identity". Don't fold the two conventions together.
 */
export const MIN_BUCKET_WIDTH_SOL = 1e-6;

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
  /** Per-metric-group fingerprint-side config (e.g. `m_flow_split.volume_ix_patterns`).
   *  Absent/`{}` ⇒ flow metrics stay NaN. */
  metric_config: Record<string, unknown>;
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
  /** Live arming — Active/Idle. Orthogonal to `is_enabled`. */
  is_active: boolean;
  /** Soft-archive — Disabled keeps the row but hides it from default lists. */
  is_enabled: boolean;
  buy_amount_lamports: number;
  max_concurrent_tokens: number;
  max_total_tokens: number;
  params: Record<string, unknown>;
  created_at: string;
  updated_at: string;
  /** DB scoreboard (list_rules enrichment). Absent/0 until something trades. */
  total_positions?: number;
  open_positions?: number;
  pending_positions?: number;
  win_count?: number;
  loss_count?: number;
  /** 0–100. */
  win_rate?: number;
  avg_pnl_pct?: number;
  total_pnl_sol?: number;
}

/** One activation session of a rule — Evidence run navigator wire shape. */
export interface StrategyRuleRun {
  id: string;
  run_seq: number;
  status: string;
  mode: string;
  started_at: string;
  finished_at: string | null;
  has_metrics: boolean;
  n_closed?: number | null;
  n_open?: number | null;
  win_rate?: number | null;
  total_pnl_sol?: number | null;
  expectancy_sol?: number | null;
  n_exit_take_profit?: number | null;
  n_exit_stop_loss?: number | null;
  n_exit_trailing?: number | null;
  n_exit_stall?: number | null;
  n_exit_time?: number | null;
  n_exit_liquidity?: number | null;
}

/** DataTable `rowClassName` for soft-archived rules (Rules + Simulate). */
export function disabledRuleRowClass(r: Pick<StrategyRule, 'is_enabled'>): string | undefined {
  return r.is_enabled ? undefined : 'opacity-40 bg-white/[0.02] hover:bg-white/[0.04]';
}

/** POST /api/strategy-rules body. `fingerprint_id` required; `is_active` is
 *  forced false and `is_enabled` true on create (lifecycle endpoints toggle them). */
export interface CreateRuleBody {
  rule_name: string;
  fingerprint_id: string;
  trade_mode: TradeMode;
  buy_amount_lamports: number;
  max_concurrent_tokens: number;
  max_total_tokens: number;
  params: Record<string, unknown>;
}

/** PUT /api/strategy-rules/{id} patch — `fingerprint_id`/`is_active`/`is_enabled` NOT
 *  patchable. `trade_mode` is patchable (editor gates it behind unlock). */
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
  /** Present only for dynamic groups (`m_flow_window`, `m_flow_split_window`,
   *  `m_price_window`). Null for static groups (`m_flow_lifetime`, …). */
  window_size_sec: number | null;
  /** One value per event (aligned with `at`); non-finite ⇒ `null`. */
  values: Array<number | null>;
}

/** `GET /api/tokens/{mint}/metric-series` response — every metric's value at every
 *  **event**, as parallel arrays. Computed on demand (never persisted). Lab-only.
 *
 *  Events are trades *plus* engine `TICK_MS` grid ticks, because the time-decaying
 *  metrics (`m_flow_window` decay, `m_price_window` extrema, `stall`/`time`,
 *  deadness) only advance on a tick — a trade-only series silently reports a later
 *  fire than the engine takes. Rows are therefore ∝ the token's lifespan, not its
 *  trade count. */
export interface MetricSeriesResponse {
  mint_address: string;
  /** RFC3339 timestamps aligned with every column's `values`. */
  at: string[];
  /** Spot price (SOL) at each event — aligned with `at`; non-finite ⇒ `null`. */
  price?: Array<number | null>;
  series: MetricSeriesColumn[];
  /** The backend's row ceiling cut the series short: it covers only
   *  `[first trade, covered_until]`. Rows that ARE present stay exact — only the
   *  span is bounded — so surface it rather than silently drawing a partial token. */
  truncated?: boolean;
  /** Last instant the series reaches (RFC3339); null when there are no events. */
  covered_until?: string | null;
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

/** Which trade in the fill window prices a sim fill (backend
 *  `trading_core::strategies::paper_fill::FillModel`). `worst_case` is what live
 *  paper + the sweep book; the others reprice the SAME taken set for the
 *  fill-sensitivity analysis (the honest bottom line was measured under
 *  `first_in_window`). */
export type FillModelId = 'worst_case' | 'first_in_window' | 'signal_price';

/** Selectable fill models for the Simulate / dry-run controls. `worst` is the
 *  default (pessimistic, live-paper parity); `first` is the realistic fast-bot
 *  next-print; `signal` is the zero-slippage optimistic bound. */
export const FILL_MODELS: ReadonlyArray<{ id: FillModelId; label: string; hint: string }> = [
  { id: 'worst_case', label: 'Worst-case', hint: 'Adverse fill — live paper + sweep parity (default)' },
  { id: 'first_in_window', label: 'First-in-window', hint: 'Next print after the signal — realistic fast bot' },
  { id: 'signal_price', label: 'Signal price', hint: 'Zero-slippage — optimistic bound' },
];

/** Which execution-cost model prices a simulated round-trip (backend
 *  `trading_core::strategies::kernel::CostModelKind`). */
export type CostModelId = 'pumpfun_default' | 'pumpfun_fee_only' | 'pumpfun_impact';

/** Selectable cost models. `pumpfun_default` charges `slippage_bps` on top of the
 *  fill price — which already prices slippage — so it DOUBLE-COUNTS execution cost
 *  whenever a fill model is chosen explicitly. It stays the default only so stored
 *  runs keep the meaning they were computed under; `pumpfun_fee_only` is the honest
 *  partner for any fill model, and the one the fill-sensitivity analysis reported.
 *
 *  `pumpfun_impact` is the only one whose cost varies with `buy_amount_sol`: the
 *  other two are size-blind, so a run under them is a ZERO-IMPACT upper bound. On
 *  the measured median pool (~70 SOL) a 1 SOL buy really costs 1.42%/leg against
 *  the flat 1% the legacy model guesses. See docs/plans/strategies/execution-costs.md. */
export const COST_MODELS: ReadonlyArray<{ id: CostModelId; label: string; hint: string }> = [
  {
    id: 'pumpfun_impact',
    label: 'Fee + real impact',
    hint: 'Fee + tip + our own buy_amount/reserve_sol price impact — the honest model',
  },
  {
    id: 'pumpfun_fee_only',
    label: 'Fee only',
    hint: 'Fee + tip + priority — no size impact, so an optimistic bound for large buys',
  },
  {
    id: 'pumpfun_default',
    label: 'Fee + slippage',
    hint: 'Legacy: also charges slippage_bps, double-counting what the fill already priced',
  },
];

/** `POST /api/strategies/simulate` body — a saved rule (`rule_id`) or an inline
 *  `draft` (ignored if `rule_id` is set), over an optional creation window.
 *  `fill_model` (top-level, default `worst_case`) and `cost_model` (default
 *  `pumpfun_default`) together decide what the round-trip PnL means — pairing an
 *  explicit fill model with `pumpfun_default` double-counts slippage (see
 *  `COST_MODELS`). */
export interface EngineSimRequest {
  rule_id?: string;
  draft?: EngineRuleDraft;
  since?: string;
  until?: string;
  fill_model?: FillModelId;
  cost_model?: CostModelId;
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
  /** `"real"` | `"paper"` when the engine still has the rule loaded. */
  trade_mode?: string | null;
  rule_name?: string | null;
}

/** `strategy_position_update` SSE payload — one generic-engine position delta. */
export interface StrategyPositionUpdateEvent {
  rule_id: string;
  mint_address: string;
  position_id: string;
  /** `strategy_positions` lifecycle: `BuySubmitted` | `Holding` | `ExitPending` |
   *  `End` | `EntryFailed` | `ExitStuck` | `ExitUnconfirmed`. */
  status: string;
  exit_reason?: string | null;
  entry_price?: number | null;
  exit_price?: number | null;
  /** `"real"` | `"paper"` when the engine still has the rule loaded. */
  trade_mode?: string | null;
  rule_name?: string | null;
  /** `true` on a stale unresolved BuySubmitted (B3) — needs manual Verify. */
  needs_review?: boolean | null;
  /** Confirmed sell-leg raw token units so far (scale-out). Omitted when zero. */
  sold_token_amount?: number | null;
  /** Sold fraction of the initial bag in bps. Omitted when zero. */
  sold_bps?: number | null;
  /** Next scale-out stage index. Omitted when unset / legacy. */
  scale_stage?: number | null;
}
