// Records + request shapes for the GROUPED param-sweep endpoints
// (`/api/strategies/sweeps[...]`). Generic across strategies (the `strategy_id`
// resolves the per-strategy tables on the backend); the drill-in combo rows
// reuse the flat `SweepResultRecord` shape, so the existing `buildSweepColumns`
// renders them unchanged.

import type { SweepResultRecord } from './types';

// --- grouping fields --------------------------------------------------------

/** The selectable fingerprint fields, matching the backend `GroupField` serde
 *  tags (snake_case). Selection order is the compound-key order. */
// Note: `creator_wallet` (near-unique per token → singleton groups) and
// `token_program_id` (effectively constant on pump.fun → one group) are poor
// grouping keys, so they're deliberately not offered here. The backend
// `GroupField` enum still accepts them for any legacy run that stored them.
export const GROUP_FIELDS = [
  'cu_limit',
  'cu_price',
  'max_sol_cost',
  'spendable_sol_in',
  'initial_buy_sol',
  'is_cashback_enabled',
  'ix_labels',
] as const;

export type GroupField = (typeof GROUP_FIELDS)[number];

/** Human labels for the group-by picker + group-key chips. */
export const GROUP_FIELD_LABELS: Record<GroupField, string> = {
  cu_limit: 'CU limit',
  cu_price: 'CU price',
  max_sol_cost: 'Max SOL cost',
  spendable_sol_in: 'Spendable SOL in',
  initial_buy_sol: 'Initial buy SOL',
  ix_labels: 'Instruction labels',
  is_cashback_enabled: 'Cashback on',
};

// --- TPSL2 editable axes ----------------------------------------------------

/** The page-editable param grid for TPSL2 — one optional candidate list per
 *  swept knob. `null` inside a nullable axis is that knob's "disabled" option.
 *  Mirrors the backend `AxesSpec`. */
export interface Tpsl2AxesSpec {
  take_profit?: number[];
  stop_loss?: number[];
  trailing_stop_pct?: (number | null)[];
  time_stop_secs?: (number | null)[];
  stall_secs?: (number | null)[];
  liquidity_drop_pct?: (number | null)[];
  cohort_ratio?: (number | null)[];
  entry_min_age_secs?: (number | null)[];
  entry_max_age_secs?: (number | null)[];
  entry_min_alive_sol?: (number | null)[];
  entry_min_organic_sol?: (number | null)[];
  entry_pullback_pct?: (number | null)[];
  entry_higher_low_secs?: (number | null)[];
  entry_max_cohort_held?: (number | null)[];
  entry_min_liquidity_sol?: (number | null)[];
  entry_min_organic_liq?: (number | null)[];
}

/** The page-editable param grid for TPSL1 — the exit ladder only (no scalp
 *  entry gates, no cohort-dump exit). Mirrors the backend tpsl1 `AxesSpec`. */
export interface Tpsl1AxesSpec {
  take_profit?: number[];
  stop_loss?: number[];
  trailing_stop_pct?: (number | null)[];
  time_stop_secs?: (number | null)[];
  stall_secs?: (number | null)[];
  liquidity_drop_pct?: (number | null)[];
}

/** A finer, presentation-only bucket within a `group`. Only swing1 sets it (its
 *  25 knobs split into 6 pipeline-order buckets); TPSL1/TPSL2 leave it undefined
 *  and render as a single grid per group. Wire payloads are unaffected. */
export type AxisSubgroup = 'swing' | 'kill' | 'volume' | 'confirm' | 'ladder' | 'next-kill';

/** One editable axis: its key, label, the param role it belongs to (drives the
 *  entry/exit grouping in the sweep param grid), whether `null` (disabled) is a
 *  valid option, and the default candidate list (mirrors the strategy's
 *  `Axes::default` on the backend, so the projected combo count is accurate and
 *  the grid is prefilled). `key` is a plain string so one `AxisDef[]` shape
 *  serves any strategy's axes ([`TPSL2_AXES`], [`TPSL1_AXES`]). */
export interface AxisDef {
  key: string;
  label: string;
  group: 'entry' | 'exit';
  /** Optional finer bucket within `group` (swing1 only) for the labelled-row
   *  param layout. Undefined ⇒ the axis renders in its group's single grid. */
  subgroup?: AxisSubgroup;
  nullable: boolean;
  default: (number | null)[];
  /** Optional plain-language explanation of what the knob does, rendered as a
   *  dimmed hint next to the field label in the sweep config form. */
  desc?: string;
}

// Order + grouping mirror the TPSL2 rule modal field-by-field (Entry Gates ·
// Scalp, then Exit Gates), so the sweep param grid reads the same as the modal.
// The high-leverage knobs ship a real candidate grid; every other knob defaults
// to `[null]` ("off"/unbounded) so it doesn't expand the grid until you type
// values for it — matching the backend `Tpsl2Axes::default`. A blank box → that
// knob stays unbounded (disabled).
export const TPSL2_AXES: AxisDef[] = [
  // Entry gates · scalp — when to buy (matches the modal's Entry section order).
  { key: 'entry_min_age_secs', label: 'Entry min age (s)', group: 'entry', nullable: true, default: [10, 30] },
  { key: 'entry_max_age_secs', label: 'Entry max age (s)', group: 'entry', nullable: true, default: [null] },
  { key: 'entry_min_alive_sol', label: 'Entry min alive (SOL)', group: 'entry', nullable: true, default: [null] },
  { key: 'entry_min_organic_sol', label: 'Entry min organic (SOL)', group: 'entry', nullable: true, default: [null] },
  { key: 'entry_min_organic_liq', label: 'Entry min organic liq (SOL)', group: 'entry', nullable: true, default: [null] },
  { key: 'entry_max_cohort_held', label: 'Entry max cohort held %', group: 'entry', nullable: true, default: [null] },
  { key: 'entry_min_liquidity_sol', label: 'Entry min liq (SOL)', group: 'entry', nullable: true, default: [null, 5] },
  { key: 'entry_pullback_pct', label: 'Entry pullback %', group: 'entry', nullable: true, default: [null, 10] },
  { key: 'entry_higher_low_secs', label: 'Entry higher-low (s)', group: 'entry', nullable: true, default: [null] },
  // Exit gates — when to sell (matches the modal's Exit section order).
  { key: 'take_profit', label: 'Take profit %', group: 'exit', nullable: false, default: [50, 100, 200] },
  { key: 'stop_loss', label: 'Stop loss %', group: 'exit', nullable: false, default: [30, 50] },
  { key: 'trailing_stop_pct', label: 'Trailing stop %', group: 'exit', nullable: true, default: [null, 20, 35] },
  { key: 'time_stop_secs', label: 'Time stop (s)', group: 'exit', nullable: true, default: [null, 120, 300] },
  { key: 'stall_secs', label: 'Stall (s)', group: 'exit', nullable: true, default: [null, 30, 60] },
  { key: 'liquidity_drop_pct', label: 'Liq-drop exit %', group: 'exit', nullable: true, default: [null] },
  { key: 'cohort_ratio', label: 'Cohort-dump exit %', group: 'exit', nullable: true, default: [null] },
];

// --- TPSL1 editable axes ----------------------------------------------------

// TPSL1 is the token-creation-filter strategy: its only per-trade swept knobs
// are the exit ladder (TP/SL lead, then the optional trailing/time/stall/liquidity
// exits). It has NO scalp entry gates and NO cohort-dump exit, so this list is the
// TPSL2 exit block minus cohort. Mirrors the backend `tpsl1::Tpsl1Axes::default`.
export const TPSL1_AXES: AxisDef[] = [
  { key: 'take_profit', label: 'Take profit %', group: 'exit', nullable: false, default: [50, 100, 200] },
  { key: 'stop_loss', label: 'Stop loss %', group: 'exit', nullable: false, default: [30, 50] },
  { key: 'trailing_stop_pct', label: 'Trailing stop %', group: 'exit', nullable: true, default: [null, 20, 35] },
  { key: 'time_stop_secs', label: 'Time stop (s)', group: 'exit', nullable: true, default: [null, 120, 300] },
  { key: 'stall_secs', label: 'Stall (s)', group: 'exit', nullable: true, default: [null, 30, 60] },
  { key: 'liquidity_drop_pct', label: 'Liq-drop exit %', group: 'exit', nullable: true, default: [null] },
];

// --- swing1 editable axes ---------------------------------------------------

/** The page-editable param grid for swing1 — the kill→volume swing-phase param
 *  space (25 knobs). Mirrors the backend `swing1::AxesSpec` field-for-field;
 *  `null` inside a nullable axis is that knob's "disabled / inert" option.
 *  NOTE: the `*_depth_*_pct` knobs are FRACTIONAL (0–1); the `swing_*_pct` /
 *  `entry_pullback_pct` knobs are 0–100. */
export interface Swing1AxesSpec {
  take_profit?: number[];
  stop_loss?: number[];
  trailing_stop_pct?: (number | null)[];
  time_stop_secs?: (number | null)[];
  stall_secs?: (number | null)[];
  liquidity_drop_pct?: (number | null)[];
  swing_high_to_low_sol?: (number | null)[];
  swing_high_to_low_pct?: (number | null)[];
  swing_low_to_high_sol?: (number | null)[];
  swing_low_to_high_pct?: (number | null)[];
  swing_min_leg_trades?: (number | null)[];
  dust_frac?: (number | null)[];
  kill_depth_min_pct?: (number | null)[];
  kill_max_duration_ms?: (number | null)[];
  kill_min_net_flow_per_sec?: (number | null)[];
  vol_depth_max_pct?: (number | null)[];
  vol_min_duration_ms?: (number | null)[];
  vol_min_up_duration_ms?: (number | null)[];
  min_kills_before_volume?: (number | null)[];
  entry_pullback_pct?: (number | null)[];
  entry_higher_low_secs?: (number | null)[];
  entry_max_age_secs?: (number | null)[];
  entry_min_liquidity_sol?: (number | null)[];
  entry_max_cohort_held?: (number | null)[];
  exit_next_kill_depth_min_pct?: (number | null)[];
  exit_next_kill_max_duration_ms?: (number | null)[];
}

// Order + grouping: the swing/kill/volume detection + entry-confirmation knobs
// gate the BUY, so they're `'entry'`; the exit ladder + symmetric next-kill flee
// are `'exit'`. Defaults mirror the backend `Swing1Axes::default` so the
// projected combo count is honest. Depth knobs are 0–1 (labelled so), the
// swing/pullback knobs are 0–100.
export const SWING1_AXES: AxisDef[] = [
  // Entry — swing detection (reversal thresholds + leg quality floor).
  // Defaults are a real SWEEP spread centered on the probe-validated firing
  // values (swing % ≈ 15, SOL floor off, kill depth ≈ 0.4, vol depth ≈ 0.6,
  // min_kills 0..1, pullback ≈ 10). Tighter thesis-pure values are included as
  // extra candidates so one run compares loose-vs-thesis. See the swing1 memory's
  // `swing-probe` / `swing-census` findings before narrowing these.
  { key: 'swing_high_to_low_pct', label: 'Swing high→low % (0–100)', group: 'entry', subgroup: 'swing', nullable: true, default: [15, 20, 25],
    desc: 'how far price must drop from a local high to count that down-leg as a swing' },
  { key: 'swing_low_to_high_pct', label: 'Swing low→high % (0–100)', group: 'entry', subgroup: 'swing', nullable: true, default: [15, 20, 25],
    desc: 'how far price must rise from a local low to count that up-leg as a swing' },
  { key: 'swing_high_to_low_sol', label: 'Swing high→low (SOL)', group: 'entry', subgroup: 'swing', nullable: true, default: [null],
    desc: 'same down-leg gate, but in absolute SOL move instead of % (off = use % only)' },
  { key: 'swing_low_to_high_sol', label: 'Swing low→high (SOL)', group: 'entry', subgroup: 'swing', nullable: true, default: [null],
    desc: 'same up-leg gate, but in absolute SOL move instead of % (off = use % only)' },
  { key: 'swing_min_leg_trades', label: 'Swing min leg trades', group: 'entry', subgroup: 'swing', nullable: true, default: [null, 2],
    desc: 'min trades a leg needs to count — filters out noise legs of 1–2 trades' },
  { key: 'dust_frac', label: 'Dust frac (0–1)', group: 'entry', subgroup: 'swing', nullable: true, default: [null],
    desc: 'drop a trade smaller than this fraction of the active leg’s biggest trade (e.g. 0.05 = ignore trades under 5% of the leg’s real size). scale-free — no SOL amount to guess. off = keep all non-zero trades' },
  // Entry — kill-low profile (deep + short).
  { key: 'kill_depth_min_pct', label: 'Kill depth min (0–1)', group: 'entry', subgroup: 'kill', nullable: true, default: [0.4, 0.5, 0.6],
    desc: 'a "kill" low must drop at least this fraction of the prior high (0.6 = −60%)' },
  { key: 'kill_max_duration_ms', label: 'Kill max duration (ms)', group: 'entry', subgroup: 'kill', nullable: true, default: [8000, 10000],
    desc: 'a kill must happen fast — within this many ms (devs dump quickly to eat snipers)' },
  { key: 'kill_min_net_flow_per_sec', label: 'Kill min net flow (SOL/s)', group: 'entry', subgroup: 'kill', nullable: true, default: [null],
    desc: 'min SOL/s of selling during the kill — confirms it was a real flush (off = ignore)' },
  // Entry — volume-low profile + count-free transition floor.
  { key: 'vol_depth_max_pct', label: 'Vol depth max (0–1)', group: 'entry', subgroup: 'volume', nullable: true, default: [0.4, 0.6],
    desc: 'a "volume-phase" low must be shallower than this — the dev is now drawing traders, not flushing' },
  { key: 'vol_min_duration_ms', label: 'Vol min duration (ms)', group: 'entry', subgroup: 'volume', nullable: true, default: [null, 10000],
    desc: 'a volume-phase low must last at least this long — slower, organic accumulation' },
  { key: 'vol_min_up_duration_ms', label: 'Vol min up-leg (ms)', group: 'entry', subgroup: 'volume', nullable: true, default: [null],
    desc: 'min duration of the recovery up-leg after a volume low (off = no floor)' },
  { key: 'min_kills_before_volume', label: 'Min kills before volume', group: 'entry', subgroup: 'volume', nullable: true, default: [0, 1],
    desc: 'how many kill lows must occur before we accept the volume phase (0 = "buy first dip", ≥1 = require the thesis)' },
  // Entry — confirmation (pullback / higher-low / age + guards).
  { key: 'entry_pullback_pct', label: 'Entry pullback % (0–100)', group: 'entry', subgroup: 'confirm', nullable: true, default: [null, 10, 20],
    desc: 'wait for this % pullback off the confirming high before buying — avoids buying the spike top' },
  { key: 'entry_higher_low_secs', label: 'Entry higher-low (s)', group: 'entry', subgroup: 'confirm', nullable: true, default: [null],
    desc: 'require a higher-low to hold for this many seconds before entering (off = enter on first confirm)' },
  { key: 'entry_max_age_secs', label: 'Entry max age (s)', group: 'entry', subgroup: 'confirm', nullable: true, default: [null],
    desc: 'skip the token if it is already older than this at entry (off = no age cap)' },
  { key: 'entry_min_liquidity_sol', label: 'Entry min liq (SOL)', group: 'entry', subgroup: 'confirm', nullable: true, default: [null],
    desc: 'only enter if pool liquidity is at least this many SOL (off = no minimum)' },
  { key: 'entry_max_cohort_held', label: 'Entry max cohort held %', group: 'entry', subgroup: 'confirm', nullable: true, default: [null],
    desc: 'skip if the launch-cohort still holds more than this % of supply — dump risk (off = ignore)' },
  // Exit — the reused ladder (TP/SL lead, then trailing/time/stall/liq).
  { key: 'take_profit', label: 'Take profit %', group: 'exit', subgroup: 'ladder', nullable: false, default: [50, 100, 200],
    desc: 'sell once unrealized gain reaches this %' },
  { key: 'stop_loss', label: 'Stop loss %', group: 'exit', subgroup: 'ladder', nullable: false, default: [30, 50],
    desc: 'sell once unrealized loss reaches this %' },
  { key: 'trailing_stop_pct', label: 'Trailing stop %', group: 'exit', subgroup: 'ladder', nullable: true, default: [null, 25],
    desc: 'sell if price falls this % from its peak since entry (locks in run-ups; off = no trail)' },
  { key: 'time_stop_secs', label: 'Time stop (s)', group: 'exit', subgroup: 'ladder', nullable: true, default: [null],
    desc: 'hard exit after holding this many seconds regardless of PnL (off = no time cap)' },
  { key: 'stall_secs', label: 'Stall (s)', group: 'exit', subgroup: 'ladder', nullable: true, default: [null, 60],
    desc: 'exit if no meaningful price move for this many seconds — the move is dead (off = ignore)' },
  { key: 'liquidity_drop_pct', label: 'Liq-drop exit %', group: 'exit', subgroup: 'ladder', nullable: true, default: [null],
    desc: 'bail if pool liquidity drops this % from entry — rug/drain signal (off = ignore)' },
  // Exit — symmetric next-kill flee (separate thresholds from the entry kill_*).
  { key: 'exit_next_kill_depth_min_pct', label: 'Next-kill depth min (0–1)', group: 'exit', subgroup: 'next-kill', nullable: true, default: [null, 0.6],
    desc: 'flee if a fresh kill-style drop of at least this depth starts after entry — the dev is rugging (off = ignore)' },
  { key: 'exit_next_kill_max_duration_ms', label: 'Next-kill max duration (ms)', group: 'exit', subgroup: 'next-kill', nullable: true, default: [8000],
    desc: 'how fast that fresh drop must be to count as a next-kill flee trigger' },
];

// --- subgroup metadata + bucketing helper -----------------------------------

/** Ordered, presentation-only metadata for the swing1 sub-buckets: the labelled
 *  row layout renders these in this pipeline order (swing → kill → volume →
 *  confirm, then ladder → next-kill). Shared by the sweep config form and the
 *  swing1 detect page so the two screens read identically. */
export const SWING1_SUBGROUPS: { key: AxisSubgroup; label: string; hint: string; accent: string }[] = [
  { key: 'swing', label: 'Swing', hint: 'reversal legs', accent: 'text-sky-300' },
  { key: 'kill', label: 'Kill', hint: 'deep + fast flush', accent: 'text-red' },
  { key: 'volume', label: 'Volume', hint: 'shallow accumulation', accent: 'text-green' },
  { key: 'confirm', label: 'Confirm', hint: 'pullback + guards', accent: 'text-accent' },
  { key: 'ladder', label: 'Exit ladder', hint: 'TP/SL + trails', accent: 'text-warning' },
  { key: 'next-kill', label: 'Next-kill flee', hint: 'symmetric exit', accent: 'text-orange-300' },
];

/** Bucket axes into the ordered [`SWING1_SUBGROUPS`], dropping empty buckets. Any
 *  axes without a `subgroup` (TPSL1/TPSL2) collapse into a single trailing bucket
 *  with `meta = null`, so callers can fall back to one flat grid for those. Pure
 *  — safe to call inside a `useMemo`. */
export function groupAxesBySubgroup(
  axes: AxisDef[],
): { meta: (typeof SWING1_SUBGROUPS)[number] | null; axes: AxisDef[] }[] {
  const out: { meta: (typeof SWING1_SUBGROUPS)[number] | null; axes: AxisDef[] }[] = [];
  for (const meta of SWING1_SUBGROUPS) {
    const inBucket = axes.filter((a) => a.subgroup === meta.key);
    if (inBucket.length) out.push({ meta, axes: inBucket });
  }
  const untagged = axes.filter((a) => a.subgroup == null);
  if (untagged.length) out.push({ meta: null, axes: untagged });
  return out;
}

// --- run / group / result records -------------------------------------------

export interface GroupedSweepRunRecord {
  id: string;
  strategy_id: string;
  source: string;
  method: string;
  created_after: string | null;
  created_before: string | null;
  curve_only: boolean;
  /** The grouping fields, in selection order. */
  grouping_spec: GroupField[];
  /** The resolved param axes the run used. */
  axes_spec: Record<string, unknown>;
  min_tokens: number;
  token_count: number;
  group_count: number;
  combo_count: number;
  corpus_hash: string | null;
  created_at: string;
  /** Lifecycle: `running` (in flight), `completed` (full sweep), or `cancelled`
   *  (cancelled / crash-recovered → only `groups_done` of `group_count` groups
   *  present). A `cancelled` run is honestly partial — render it with a banner so
   *  it's never mistaken for a complete sweep. */
  status: 'running' | 'completed' | 'cancelled';
  /** Groups persisted so far; equals `group_count` for a `completed` run. */
  groups_done: number;
  /** The exact-set instruction-label corpus filter the run used (the JSON array
   *  the form submitted), or `null` when unfiltered / grouped by `ix_labels`. */
  ix_labels_filter: string[] | null;
  /** Per-field value filters the corpus was pinned to (`{"cu_price":[1000],…}`),
   *  or `null` when none. Values are numbers or booleans (cashback). */
  field_filters: Record<string, (number | boolean)[]> | null;
  /** The per-run token cap submitted (distinct from realized `token_count`);
   *  `null` for legacy runs. */
  token_cap: number | null;
  /** The per-group combo-cap override submitted (distinct from realized
   *  `combo_count`); `null` ⇒ backend default. */
  max_combos: number | null;
  /** Optional user-given name; `null` = unnamed (UI falls back to timestamp). */
  label: string | null;
  /** Notional (SOL) each simulated round-trip was priced at; `null` on legacy
   *  runs (backend defaulted to 1.0 SOL). */
  buy_amount_sol: number | null;
}

/** One group's summary row: its fingerprint key, sample size, and winning combo. */
export interface GroupedSweepGroupRecord {
  id: string;
  group_index: number;
  /** `{ "creator_wallet": "4f3a…", "max_sol_cost": "12345" }`; `{}` = the ALL group. */
  group_key: Record<string, string>;
  token_count: number;
  /** The best combo's `n_fired` — sample size behind the headline pick. */
  fired_count: number;
  best_combo_id: number;
  /** Robust realized score (`μ−Z·σ/√n` over closed trades) of the winning combo
   *  — the headline ranking metric; `null` when it has < 2 closed trades. */
  best_score: number | null;
  best_expectancy_sol: number;
  best_params: Record<string, number | null>;
}

/** Drill-in combo rows reuse the flat sweep-result shape (same metric set). */
export type GroupedSweepResultRecord = SweepResultRecord;

/** Per-token outcome for a single re-simulated combo (the token-results drill-in). */
export interface ComboTokenResult {
  mint: string;
  symbol: string;
  fired: boolean;
  pnl_sol: number;
  pnl_pct: number;
  holding_secs: number;
  /** `"TakeProfit"` | `"StopLoss"` | `"TrailingStop"` | `"Stall"` |
   *  `"TimeStop"` | `"LiquidityExit"` | `"CohortExit"` | `"Open"` | `"NoEntry"` */
  exit: string;
  // Simulation fill details
  entry_time: string | null;
  entry_price: number | null;
  /** Real base58 tx signature of the entry fill, resolved server-side from the
   *  trades table by (mint, slot, buy). Null when not fired or unresolved. */
  entry_tx: string | null;
  exit_time: string | null;
  exit_price: number | null;
  /** Real base58 tx signature of the exit fill (sell side). Null when open,
   *  not fired, or unresolved. */
  exit_tx: string | null;
  // Token metadata (from tokens + tokens_info)
  created_at: string | null;
  creator_wallet: string | null;
  ath_price: number | null;
  ath_timestamp: string | null;
  current_price: number | null;
  market_cap: number | null;
  volume_sol: number | null;
  trade_count: number | null;
  is_migrated: boolean | null;
  is_dead: boolean | null;
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
  /** Exact-set instruction-label filter — restrict the corpus to tokens whose
   *  `ix_labels` set equals these labels, then sweep. The page sends this only
   *  when grouping by `ix_labels` is OFF (the two are mutually exclusive: group
   *  by the label set, or pin a single set and sweep it). Omitted ⇒ no filter. */
  ix_labels_filter?: string[];
  /** Per-field value filters: restrict the corpus to tokens whose fingerprint
   *  value for the named field is in the allowed set. Map key = GroupField tag
   *  (e.g. `"cu_price"`); value = allowed numbers. Empty map or omitted ⇒ no
   *  filter. `"ix_labels"` is handled by `ix_labels_filter` above.
   *  Applied post-fingerprint, in-memory, alongside `ix_labels_filter`. */
  field_filters?: Record<string, (number | boolean)[]>;
  min_tokens?: number;
  /** `grid` | `random:N` | `lhs:N`. */
  method?: string;
  /** Strategy-specific axes — TPSL2's full grid, TPSL1's exit-ladder-only grid,
   *  or swing1's kill→volume swing-phase grid. Resolved by `strategy_id` on the
   *  backend. */
  axes?: Tpsl2AxesSpec | Tpsl1AxesSpec | Swing1AxesSpec;
  token_cap?: number;
  /** Per-group combo cap override. Omitted ⇒ backend default (5000); the backend
   *  clamps to its hard backstop. */
  max_combos?: number;
  /** Notional (SOL) to price every simulated round-trip at. Set to the live
   *  `buy_amount` so backtest PnL% matches live results. Omitted ⇒ 1.0 SOL. */
  buy_amount_sol?: number;
}
