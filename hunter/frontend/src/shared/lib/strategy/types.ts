// Wire types for the generic-engine fingerprint + rule HTTP surface
// (live-bin handlers `strategies::engine`).
//
// A fingerprint's identity is its `criteria` map — see `fingerprintAxes.ts`, the
// TS mirror of the Rust axis registry, which owns the axis list, the predicate
// shapes, and the only SOL <-> lamports conversion on this path. Rule amounts
// below still speak lamports on the wire and SOL in the UI.

import type { WindowSpec } from 'lib/strategy/windowSpec';
import type { Criteria } from 'lib/strategy/fingerprintAxes';

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

/** Auto-name of a fingerprint with nothing to name from its axes — a `wildcard`
 *  row, and the criterion-less draft the write edge rejects. Mirrors the Rust
 *  `models::fingerprint::WILDCARD_NAME`. */
export const WILDCARD_NAME = 'ALL';

/** A `fingerprints` row (response shape). All `*_lamports` axes are lamports. */
export interface Fingerprint {
  id: string;
  /** Picker handle and log label. NOT identity — renaming changes nothing about
   *  what this fingerprint matches. */
  name: string;
  /** Match EVERY token, ignoring every axis (Rust `Fingerprint::wildcard`).
   *
   *  A rule always needs a fingerprint, but one deciding purely on the tape has no
   *  creation shape to name — and clearing every axis means *match nothing*,
   *  because the matcher refuses a criterion-less row on purpose. So "any token" is
   *  said out loud here rather than inferred from a blank form.
   *
   *  Part of match identity (`IDENTITY_WHERE` compares it), and mutually exclusive
   *  with every axis — `Fingerprint::validate` and the
   *  `fingerprints_wildcard_excludes_axes` CHECK both reject a wildcard carrying
   *  one. */
  wildcard: boolean;
  /** The configured axes: one predicate per axis, keyed by axis id. An axis absent
   *  from the map is not part of identity. Bounds are decimal STRINGS over the full
   *  `u64` domain — see `fingerprintAxes.ts`. */
  criteria: Criteria;
  /** Per-metric-group fingerprint-side config (e.g. `m_flow_ix.ix_patterns`).
   *  Absent/`{}` => flow metrics stay NaN. Not part of identity. */
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
  /**
   * Free-form labels for slicing the Rules board — presentational only, and
   * deliberately NOT part of trading identity (see `ruleIdentityOf`). The server
   * canonicalizes them (`normalize_tags`) and always sends the key; optional here
   * so a response cached from a pre-tags bin still types.
   */
  tags?: string[];
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
  /** **Canonical return %** — capital-weighted (`total_pnl_sol /
   *  closed_entry_sol × 100`), so it is sign-locked to `total_pnl_sol`. */
  return_pct?: number;
  total_pnl_sol?: number;
  /** `return_pct`'s own denominator: the entry cost of this rule's CLOSED
   *  positions. Present so the Rules TOTAL tile can aggregate across rules by
   *  capital; weighting the per-rule percents by trade count instead lets a rule
   *  buying 0.05 SOL outvote one buying 1.0 SOL. */
  closed_entry_sol?: number;
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

/**
 * Trade-mode left rail — real rules carry a solid `warning` bar plus a faint
 * neutral wash, paper a thin `info` bar. The rail hue matches the Mode badge, so
 * the rail and the badge are one language; the point is that "this row spends
 * real money" survives any sort order, filter, or scroll position.
 *
 * The wash is white, NOT the rail's amber: `--color-warning` is a pale yellow,
 * and yellow at single-digit alpha over the near-black surface mixes to olive.
 * The rail carries the hue; the wash only has to lift the row off the surface.
 *
 * Painted as a background **image**, never a background **color**. Two
 * constraints force that, both of them load-bearing:
 *   - `DataTable` merges `rowClassName` LAST through tailwind-merge, and
 *     `bg-*` colors collapse to a single winner — so a `bg-warning/6` here
 *     would silently erase the row's selection (`bg-accent/16`) and pin washes.
 *     A gradient is a different merge group, so it composes with both.
 *   - The table is `border-collapse: collapse`, where a row-level `box-shadow`
 *     is never painted (see the comment on `TableRowInner`), which rules out
 *     the inset-rail trick the selected row uses on its cells.
 * Keep the class strings literal — Tailwind only emits what it can scan.
 */
export function modeRuleRowClass(r: Pick<StrategyRule, 'trade_mode'>): string {
  return r.trade_mode === 'real'
    ? 'bg-[linear-gradient(90deg,var(--color-warning)_0_3px,color-mix(in_srgb,white_4%,transparent)_3px)]'
    : 'bg-[linear-gradient(90deg,color-mix(in_srgb,var(--color-info)_45%,transparent)_0_2px,transparent_2px)]';
}

/** The ONE `rowClassName` for every rule table (Rules live/lab + Simulate):
 *  mode rail + soft-archive dimming. Both layers compose — a disabled real rule
 *  keeps its rail while dimming out. */
export function ruleRowClass(
  r: Pick<StrategyRule, 'trade_mode' | 'is_enabled'>,
): string {
  const archived = disabledRuleRowClass(r);
  return archived ? `${modeRuleRowClass(r)} ${archived}` : modeRuleRowClass(r);
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
  tags?: string[];
}

/** PUT /api/strategy-rules/{id} patch — `fingerprint_id`/`is_active`/`is_enabled` NOT
 *  patchable. `trade_mode` is patchable (editor gates it behind unlock).
 *  `tags` follows patch semantics server-side: omit to leave them alone, send
 *  `[]` to clear. */
export type UpdateRuleBody = Partial<
  Pick<
    CreateRuleBody,
    | 'rule_name'
    | 'trade_mode'
    | 'buy_amount_lamports'
    | 'max_concurrent_tokens'
    | 'max_total_tokens'
    | 'params'
    | 'tags'
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
  /** When the engine armed this pair (ISO). Server-stamped, so the Waiting row's
   *  age survives a reconnect and matches the arm ledger's episode. */
  armed_at: string;
}

/** One arming episode from `POST /api/strategies/arms/query` — the durable twin
 *  of a Waiting row. `end_reason` is `null` while the episode is still live. */
export interface StrategyArmRecord {
  rule_id: string;
  mint_address: string;
  mode: string;
  armed_at: string;
  ended_at: string | null;
  /** `entered` | `dead` | `migrated` | `unsatisfiable` | `paused` |
   *  `duplicate_identity`; `null` while the engine is still evaluating entry. */
  end_reason: string | null;
  /** The position this episode became — set only with `end_reason === 'entered'`. */
  position_id: string | null;
  symbol: string | null;
  /** Seconds from `armed_at` to `ended_at`, or to now while live (server-computed). */
  waited_sec: number | null;
  /** What the entry was short of, captured by the fold at the disarm instant.
   *  Set only with `end_reason === 'unsatisfiable'` — every other ending states
   *  its own cause. */
  end_detail: ArmEndDetail | null;
}

/** One entry condition still failing when the arm gave up. */
export interface ArmUnmetCondition {
  /** `group.metric`, e.g. `m_flow_window.gross_flow`. */
  metric: string;
  /** Legacy scalar: the SIZE of a wall-clock window, in seconds. `null` for a
   *  static metric AND for a slot window, which names itself in `window` instead. */
  window_size_sec: number | null;
  /** The full span: size, lag and unit. Absent on a static metric, and on rows
   *  written before slot windows existed. */
  window?: WindowSpec | null;
  /** The reading that failed it; `null` when the metric was unreadable. */
  value: number | null;
  /** The authored DNF, `OR` of `AND` arms, as `{operator, value}` objects — the
   *  same shape the readout strip renders. */
  conditions: unknown;
}

/** `strategy_arms.end_detail` — why an entry became permanently unsatisfiable. */
export interface ArmEndDetail {
  /** The representative blocker (first unmet in the rule's own order), and the
   *  key the `blocked_by` column filters and the summary groups on. `null` when
   *  nothing but the clock was unmet — the token qualified too late. */
  blocked_by: string | null;
  /** The deadline that ended the episode. Always the `time` bound today, so it
   *  says *when*, never *why*. */
  killed_by: { metric: string; threshold: number; operator: string };
  unmet: ArmUnmetCondition[];
}

/** One bar of the `unsatisfiable` breakdown. */
export interface ArmBlockedBy {
  blocked_by: string;
  n: number;
}

/** `POST /api/strategies/arms/summary` — the arm funnel over one cohort. */
export interface ArmFunnel {
  armed: number;
  entered: number;
  live: number;
  dead: number;
  migrated: number;
  unsatisfiable: number;
  paused: number;
  duplicate_identity: number;
  /** `entered / armed × 100` (0 when nothing armed). */
  entry_rate_pct: number;
  median_waited_sec: number | null;
  /** Which entry condition held the `unsatisfiable` episodes out, busiest first.
   *  Empty when the cohort holds none carrying a detail. */
  blocked_by: ArmBlockedBy[];
}

/** One computed metric column from `GET /api/tokens/{mint}/metric-series`. */
export interface MetricSeriesColumn {
  metric: string;
  group: string;
  unit: string;
  /** The WHOLE span this column was computed over — size, lag and unit. Present only
   *  for dynamic groups (`m_flow_window`, `m_flow_ix_window`, `m_price_window`);
   *  null for static ones (`m_flow_lifetime`, …). Prefer this over
   *  {@link MetricSeriesColumn.window_size_sec}. */
  window?: WindowSpec | null;
  /** The nested SLICE span, for the two-window metrics alone
   *  (`m_flow_window.trade_share` / `.sol_share`). Their reading is a ratio ACROSS the
   *  pair, so a column labelled by `window` alone names a different number than it
   *  holds. Null everywhere else. */
  slice?: WindowSpec | null;
  /** Legacy seconds scalar, for readers that predate `window`. Null on a slot or
   *  print span — neither has seconds to report, so a reader that only knows this key
   *  drops the column rather than calling 30 slots 30 seconds. */
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
 *  `first_in_window`). The `next_slot_*` pair drops the signal's own slot, whose
 *  prints a +1-slot landing can never reach. */
export type FillModelId =
  | 'worst_case'
  | 'first_in_window'
  | 'next_slot_first'
  | 'next_slot_median'
  | 'signal_price'
  /** Wall-clock reaction lag in ms (`lag_115`). The only model keyed to a MEASURED
   *  decide-to-fill latency rather than to slot structure — it can fill inside the
   *  signal's own slot and still charge a delay, which the slot-shaped models
   *  bracket but cannot express. Backend `FillModel::LagMs`. */
  | `lag_${number}`;

/** Selectable fill models for the Simulate / dry-run controls, ordered as a
 *  pessimism spectrum. `worst_case` is the default (live-paper parity) and
 *  `signal_price` the unreachable ceiling; the two `next_slot_*` models are the
 *  reachable middle — same window, minus the signal's own slot. The two `lag_*`
 *  presets are the bot's own measured decide-to-fill p50 / p90. */
export const FILL_MODELS: ReadonlyArray<{ id: FillModelId; label: string; hint: string }> = [
  { id: 'worst_case', label: 'Worst-case', hint: 'Adverse fill — live paper + sweep parity (default)' },
  { id: 'first_in_window', label: 'First-in-window', hint: 'Next print after the signal — may be same-slot, so partly unreachable' },
  { id: 'lag_115', label: 'Lag 115 ms (p50)', hint: "The bot's measured decide-to-fill median — first print at least 115 ms after the signal" },
  { id: 'lag_235', label: 'Lag 235 ms (p90)', hint: "The bot's decide-to-fill p90 — the stress read; a real edge survives it" },
  { id: 'next_slot_first', label: 'Next-slot first', hint: 'First print at slot S+1 — earliest a +1-slot landing can hit' },
  { id: 'next_slot_median', label: 'Next-slot median', hint: 'Adverse median at slot S+1 — mid-dispersion, still a real print' },
  { id: 'signal_price', label: 'Signal price', hint: 'Zero-slippage — optimistic bound' },
];

/** The lag in ms a `lag_<ms>` id carries, or `null` for every other model. */
export function fillModelLagMs(id: string | null | undefined): number | null {
  const m = /^lag_(\d+)$/.exec(id ?? '');
  return m ? Number(m[1]) : null;
}

/** Display label for ANY fill model id, including a `lag_<ms>` the preset list does
 *  not name. Never returns an object: the backend used to serialize the lag model as
 *  `{lag_ms: 115}`, which React renders as a crash, so every call site goes through
 *  this instead of printing the raw value. */
export function fillModelLabel(id: string | null | undefined): string {
  if (!id) return FILL_MODELS[0].label;
  const known = FILL_MODELS.find((m) => m.id === id);
  if (known) return known.label;
  const lag = fillModelLagMs(id);
  return lag != null ? `Lag ${lag} ms` : String(id);
}

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
  /** Override the box copycat guard for this run. Absent ⇒ inherit Settings. */
  skip_duplicate_identity?: boolean;
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
  /** Disarm reason (`dead` | `migrated` | `unsatisfiable` | `paused` |
   *  `duplicate_identity` | `entered`) when disarmed. */
  reason?: string | null;
  /** When the episode this frame describes was armed (ISO) — present on the arm
   *  AND the disarm, so the client never stamps its own arrival time. */
  armed_at?: string | null;
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
  /** Entry-fill SOL spent and fill instant (ISO). Present from the entry fill
   *  onward, so a row hydrated only from deltas can still draw the chart's entry
   *  marker and print the entry size — the REST snapshot refetches on mount /
   *  SSE reopen / tab-visible, which a mid-session entry misses entirely. */
  entry_sol?: number | null;
  entry_time?: string | null;
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

