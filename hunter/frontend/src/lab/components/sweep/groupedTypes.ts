// Records + request shapes for the GROUPED param-sweep endpoints
// (`/api/strategies/sweeps[...]`). ONE generic engine (`strategy_id="generic"`)
// drives these: the swept `params` per combo is a `RuleParams` blob (rendered by
// `genericSweepColumns`), and the axes are the registry-driven `AxisSpec[]`
// (`genericAxes.ts`). There are no per-strategy static axis grids or
// fingerprint-blob serializers — a new strategy is a registry entry, not a page.

import type { CostModelId, FillModelId } from 'lib/strategy/types';
import type { Criteria } from 'lib/strategy/fingerprintAxes';
import type { FieldFilterValue } from './fingerprintFilters';

import type { SweepResultRecord } from './types';

// --- grouping fields --------------------------------------------------------

/** The selectable fingerprint fields, matching the backend `GroupField` serde
 *  tags (snake_case). Selection order is the compound-key order. */
export const GROUP_FIELDS = [
  'cu_limit',
  'cu_price',
  'init_buy_lamports',
  'max_cost_lamports',
  'spendable_lamports_in',
  'first_slot_buy_lamports',
  'first_slot_sell_lamports',
  'ix_labels',
  'ix_count',
  'prior_launches',
  'is_cashback_enabled',
] as const;

export type GroupField = (typeof GROUP_FIELDS)[number];

/** Human labels for the group-by picker + group-key chips. */
export const GROUP_FIELD_LABELS: Record<GroupField, string> = {
  cu_limit: 'CU limit',
  cu_price: 'CU price',
  init_buy_lamports: 'Initial buy',
  max_cost_lamports: 'Max cost',
  spendable_lamports_in: 'Spendable in',
  first_slot_buy_lamports: 'First-slot buy',
  first_slot_sell_lamports: 'First-slot sell',
  ix_labels: 'Instruction labels',
  ix_count: 'Instruction count',
  prior_launches: 'Prior launches',
  is_cashback_enabled: 'Cashback on',
};

/** Group fields whose filter values are typed in **human SOL** (the axis stores
 *  integer lamports). Every other numeric field is typed as its own integer.
 *
 *  This is a UNIT distinction, not a mode: the same grammar (`1.515`, `1.5-1.6`,
 *  `>=1.5`) applies to all of them — only what the digits mean differs. Mirrors the
 *  `lamports` arm of the Rust `AxisUnit`. */
export const LAMPORTS_GROUP_FIELDS: ReadonlySet<GroupField> = new Set<GroupField>([
  'init_buy_lamports',
  'max_cost_lamports',
  'spendable_lamports_in',
  'first_slot_buy_lamports',
  'first_slot_sell_lamports',
]);

/** Group fields a `PartitionSpec.ranges` can bin, and that take the numeric filter
 *  grammar. `ix_labels` is a sequence and the two grouping-only fields are
 *  discrete, so neither is here. */
export const NUMERIC_GROUP_FIELDS: ReadonlySet<GroupField> = new Set<GroupField>([
  ...LAMPORTS_GROUP_FIELDS,
  'cu_limit',
  'cu_price',
  'ix_count',
  'prior_launches',
]);

// --- partitioning ------------------------------------------------------------

/** How one grouped field's values are collapsed into group keys. Mirrors Rust
 *  `hunter_engine::grouping::PartitionSpec`.
 *
 *  **There is no width.** A width defines an infinite implicit lattice that every
 *  consumer has to re-derive identically (and a `0` in it divides by zero);
 *  explicit edges are a finite list that travels with the run and means the same
 *  thing to everyone who reads it. `edges` are decimal STRINGS in the field's own
 *  integer unit — a JS `number` is unsafe past 2^53. */
export type PartitionSpec =
  | { kind: 'distinct' }
  | { kind: 'ranges'; edges: string[] };

/** One group key value, as the backend serializes it. A numeric field carries the
 *  inclusive `[min, max]` WINDOW it selected — which is exactly the predicate a
 *  promoted fingerprint stores, so promote is a copy rather than a re-derivation. */
export type GroupKeyValue =
  | { kind: 'missing' }
  | { kind: 'text'; value: string }
  | { kind: 'flag'; value: boolean }
  | { kind: 'labels'; labels: string[] }
  | { kind: 'window'; min?: string; max?: string };

// --- run / group / result records -------------------------------------------

export interface GroupedSweepRunRecord {
  id: string;
  strategy_id: string;
  /** Always `"db"` today (vestigial single-source tag) — carries no signal, not
   *  worth surfacing in the UI. Kept typed only so the wire shape is complete. */
  source: string;
  method: string;
  created_after: string | null;
  created_before: string | null;
  curve_only: boolean;
  /** The grouping fields, in selection order. */
  grouping_spec: GroupField[];
  /** The resolved param axes the run used (`{ axes: AxisSpec[] }` for generic). */
  axes_spec: Record<string, unknown>;
  min_tokens: number;
  token_count: number;
  group_count: number;
  combo_count: number;
  /** Hash of the corpus slice this run scanned — two runs with the same hash swept
   *  the exact same tape (same selection + filters + lake export). `null` on a
   *  trade-less / legacy corpus. Was already sent by the backend but unread here. */
  corpus_hash: string | null;
  /** Block time of the newest trade in the corpus this run scanned — how fresh its
   *  data was. `null` on legacy runs / a trade-less corpus. The sweep reads the
   *  sealed lake only while Simulate splices the fresh PG tail, so a stale export
   *  makes the two disagree without either being wrong: the sweep freezes `Open (est)`
   *  rows at old prices that a simulate watches die. Surfaced as "data through". */
  corpus_last_trade_at: string | null;
  created_at: string;
  /** Lifecycle: `running` | `completed` | `cancelled` (user abort, honestly
   *  partial) | `partial` (the run stopped early — a DB write error, or an
   *  engine failure after some groups had already folded — so its committed
   *  groups are real but the sweep is incomplete) | `failed` (it stopped before
   *  any group folded, e.g. bad axes or a corpus that cannot fit this machine).
   *  A failed run is kept rather than deleted so the attempt stays inspectable. */
  status: 'running' | 'completed' | 'cancelled' | 'partial' | 'failed';
  /** Groups persisted so far; equals `group_count` for a `completed` run. */
  groups_done: number;
  /** The exact-set instruction-label corpus filter the run used, or `null`. */
  ix_labels_filter: string[] | null;
  /** Per-field value filters the corpus was pinned to, or `null` when none. */
  field_filters: Record<string, (number | boolean)[]> | null;
  /** Saved fingerprint the corpus was scoped to (engine match — exact + bucket
   *  axes), or `null` for a manual group-by / filter run. Mutually exclusive with
   *  `ix_labels_filter` / `field_filters`, which are `null` on a scoped run. */
  fingerprint_id: string | null;
  /** The per-run token cap submitted; `null` for legacy runs. */
  token_cap: number | null;
  /** The per-group combo-cap override submitted; `null` ⇒ backend default. */
  max_combos: number | null;
  /** Optional user-given name; `null` = unnamed (UI falls back to timestamp). */
  label: string | null;
  /** Notional (SOL) each simulated round-trip was priced at; `null` on legacy runs. */
  buy_amount_sol: number | null;
  /** How each grouped field was partitioned — `[[field, spec], …]` in group-by
   *  order. Empty on a run swept before the partition replaced the bucket width:
   *  its group keys are rendered labels no longer parsed, so it reads as
   *  one-group-per-value. Re-run to promote. */
  partition: [GroupField, PartitionSpec][];
  /** Corpus-wide volume-ix patterns used when the run swept flow axes. `null` =
   *  non-flow run / legacy. Promote copies these into the fingerprint. */
  ix_patterns: string[][] | null;
  /** Which trade in the fill window priced each leg. `null` on legacy runs ⇒
   *  `worst_case`, what the sweep hardcoded before the model became selectable.
   *  Part of the run's IDENTITY — two runs under different fill models are not
   *  comparable, so this is shown next to the run's PnL. */
  fill_model: FillModelId | null;
  /** Which cost model priced the round-trips. `null` on legacy runs ⇒
   *  `pumpfun_default`, which charges slippage on top of the fill price. */
  cost_model: CostModelId | null;
  /** The candidate scale-out ladder(s) searched in Pass 2 — `ExitStage[][]`
   *  (backend keeps the grid-shaped wire contract for forward compat), but the
   *  FE authors exactly ONE user ladder via `ScaleOutBuilder` and sends it as
   *  the sole entry: comparing many arbitrary ladders against the same small
   *  per-combo sample is a multiple-comparisons trap, not a real search. `null`
   *  = no Pass 2. Each group's top-K combos are independently re-scored against
   *  the ladder(s) here plus their own baseline exit and keep whichever wins —
   *  a combo it doesn't help keeps its own exit, so this field is the search
   *  space, not what any one combo ended up with (see that combo's own
   *  `params.scale_out`). */
  scale_out: unknown[][] | null;
  /** How many best combos/group Pass 2 re-scored. `null` when no overlay. */
  scale_out_top_k: number | null;
}

/** One group's summary row: its fingerprint key, sample size, and winning combo. */
export interface GroupedSweepGroupRecord {
  id: string;
  group_index: number;
  /** `{ "cu_price": "1000", "max_cost_lamports": "1.0–1.1" }`; `{}` = the ALL group. */
  group_key: Record<string, string>;
  token_count: number;
  /** The best combo's `n_fired` — sample size behind the headline pick. */
  fired_count: number;
  best_combo_id: number;
  /** Checklist score of the winning combo; `null` when never fired. */
  best_score: number | null;
  best_expectancy_sol: number;
  best_win_rate: number;
  best_total_pnl_sol: number;
  /** Winning combo's unrealized PnL over its still-open positions — excluded
   *  from `best_total_pnl_sol` by design, surfaced so a profitable-looking
   *  realized total can't hide a pile of open losers. */
  best_open_pnl_sol: number;
  /** Winning combo's still-open / closed split of `fired_count`. Shows how much
   *  of the headline sample is unrealized. */
  best_n_open: number;
  best_n_closed: number;
  /** Winning combo's profit factor; `null` = no losing trades (UI shows ∞). */
  best_profit_factor: number | null;
  best_mean_pnl_pct: number;
  best_median_pnl_pct: number;
  best_p90_pnl_pct: number;
  best_std_pnl_pct: number;
  best_avg_holding_secs: number;
  best_median_holding_secs: number;
  /** The winning combo's params — a `RuleParams` blob for the generic engine. */
  best_params: Record<string, unknown>;
  /** What this group's tokens were actually selected by. `undefined` only on a
   *  response from an older backend. See {@link GroupSelection}. */
  selection?: GroupSelection;
}

/** One axis of a group's selection, as the backend resolved it. */
export interface SelectionClause {
  /** `GroupField` tag — `"max_cost_lamports"`, `"ix_labels"`, … */
  field: string;
  /** Where the clause came from: the scope fingerprint, a run filter, or group-by. */
  origin: 'scope' | 'filter' | 'group_by';
  /** Pre-rendered value (`"0.324"`, `"1.5–1.6"`, `"∅ (absent)"`). Rendered by the
   *  backend on purpose — the frontend must not re-derive a selection. */
  display: string;
  /** The underlying predicate, adjacently tagged by the backend `AxisPredicate`:
   *  `kind` is the discriminant (`lamports` / `labels` / `absent` / …) and `value`
   *  the payload (absent for `absent`). Read `display` to render — this is here for
   *  discriminating, never for re-deriving a selection. */
  predicate: { kind: string; value?: unknown };
}

/**
 * What selected a group's tokens: the scope fingerprint's axes ∧ the run's manual
 * filters ∧ the group key — resolved ONCE by the backend
 * (`lab/src/sweep/selection.rs`) and consumed verbatim here.
 *
 * Never re-derive this from `group_key` on the frontend: a run that pins its
 * corpus with `ix_labels_filter` / `field_filters` then renders as "ALL tokens",
 * because those clauses live on the RUN, never in the group key.
 */
export interface GroupSelection {
  /** The saved fingerprint the run was scoped to, if any. */
  scope_fingerprint_id: string | null;
  clauses: SelectionClause[];
  /** Whether Promote can express this selection as a fingerprint. */
  promotable: boolean;
  /** Why not — one message per unexpressible clause. Empty when promotable. */
  blockers: string[];
  /** Identity fields of the fingerprint Promote would find-or-create. Compare
   *  (never rebuild) to badge a group with an existing fingerprint. */
  identity?: {
    /** The axes the promoted fingerprint would carry — the same map
     *  `IDENTITY_WHERE` compares. */
    criteria: Criteria;
    /** Always `false` — a promoted group is a set of axis values, never "every
     *  token". Carried because `IDENTITY_WHERE` compares it, so an identity that
     *  omits it is not the identity it claims to be. */
    wildcard: boolean;
  };
}

/** Drill-in combo rows reuse the flat sweep-result shape (same metric set). */
export type GroupedSweepResultRecord = SweepResultRecord;

/** Per-token outcome for a single re-simulated combo (the token-results drill-in). */
export interface ComboTokenResult {
  mint_address: string;
  symbol: string;
  fired: boolean;
  pnl_sol: number;
  pnl_pct: number;
  holding_secs: number;
  /** `"TakeProfit"` | `"StopLoss"` | `"Metrics"` | `"Dead"` | `"Open"` | `"NoEntry"`. */
  exit: string;
  entry_time: string | null;
  entry_price: number | null;
  /** Real base58 tx signature of the entry fill. Null when not fired / unresolved. */
  entry_tx: string | null;
  exit_time: string | null;
  exit_price: number | null;
  /** Real base58 tx signature of the exit fill. Null when open / not fired. */
  exit_tx: string | null;
  created_at: string | null;
  ath_price: number | null;
  creator_wallet: string;
  ath_timestamp: string | null;
  current_price: number | null;
  market_cap: number | null;
  volume_sol_total: number;
  trade_count: number;
  is_migrated: boolean;
  is_dead: boolean;
}

/** `GET …/token-results` response: the drill-in's per-token rows plus an **exact**
 *  (no-sketch) metrics summary over exactly those rows. */
export interface ComboTokenResultsResponse {
  rows: ComboTokenResult[];
  metrics: SweepResultRecord;
}

// --- start request ----------------------------------------------------------

export interface GroupedSweepStartArgs {
  strategy_id: string;
  /** RFC3339 UTC; selection lower bound (inclusive). */
  created_after?: string;
  /** RFC3339 UTC; selection upper bound (exclusive). */
  created_before?: string;
  curve_only?: boolean;
  group_by: GroupField[];
  /** Exact-set instruction-label corpus filter (mutually exclusive with grouping
   *  by `ix_labels`). Omitted ⇒ no filter. */
  ix_labels_filter?: string[];
  /** Per-field value filters (key = GroupField tag; value = allowed numbers/bools). */
  field_filters?: Record<string, FieldFilterValue[]>;
  /** Scope the corpus to a saved fingerprint using the ENGINE matcher (exact +
   *  bucket axes — the same gate live arms on). When set the backend ignores
   *  `ix_labels_filter` / `field_filters`; `group_by` still partitions within the
   *  matched slice. Omitted ⇒ manual group-by / filters. */
  fingerprint_id?: string;
  min_tokens?: number;
  /** `grid` | `random:N` | `lhs:N` | `refine:N[:K]`. */
  method?: string;
  /** Strategy-specific axes — for `"generic"` this is `AxesRequest { axes: [...] }`.
   *  Resolved by `strategy_id` on the backend. */
  axes?: unknown;
  token_cap?: number;
  /** Per-group combo cap override. Omitted ⇒ backend default. */
  max_combos?: number;
  /** Notional (SOL) to price every simulated round-trip at. Omitted ⇒ 1.0 SOL. */
  buy_amount_sol?: number;
  /** Host RAM (MB) the run leaves free for OS + desktop; every sizing ceiling is
   *  `host free − this`. A preference, not a limit — a run that does not fit
   *  degrades (fewer threads / smaller batches) rather than being refused.
   *  Omitted ⇒ backend default (1024). */
  ram_reserve_mb?: number;
  /** Opt into the AVX-512 vectorized per-`(combo × token)` exit scan (lab-only
   *  speedup). Honored only when the host has AVX-512 — otherwise the backend forces
   *  the scalar scan and toasts a notice. Like `ram_reserve_mb`, it's a property of
   *  *how the box computed*, not the analysis, so it isn't persisted on the run row.
   *  Omitted ⇒ scalar. */
  use_avx512?: boolean;
  /** Corpus-wide volume-ix patterns when axes reference `m_flow_ix` /
   *  `m_flow_ix_window` (not aggregate `m_flow_lifetime` / `m_flow_window`).
   *  Required by the backend for those runs; omitted otherwise. */
  ix_patterns?: string[][];
  /** Which trade in the fill window prices each simulated leg. Omitted ⇒
   *  `worst_case` (what the sweep hardcoded before this was selectable), so stored
   *  and replayed runs keep their meaning. Unlike `use_avx512` this changes the
   *  RESULT, not just how it was computed — it is persisted on the run row. */
  fill_model?: FillModelId;
  /** Which execution-cost model prices the round-trips. Omitted ⇒
   *  `pumpfun_default`. Pair an explicit `fill_model` with `pumpfun_fee_only`:
   *  the fill price already prices slippage. */
  cost_model?: CostModelId;
  /** Pass-2 ladder for the run — `ExitStage[][]` on the wire (backend grid
   *  contract), but the FE sends exactly one user-authored ladder as its sole
   *  entry. Omitted / empty ⇒ no Pass 2. Each top-K combo per group is
   *  re-scored against it plus its own baseline and keeps whichever wins
   *  (per combo — never forced onto a combo it doesn't help). */
  scale_out?: unknown[][];
  /** Top-K combos per group for Pass 2. Default 3. */
  scale_out_top_k?: number;
}
