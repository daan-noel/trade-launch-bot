import type { RunSummary } from 'lib/strategy/runSummary';
import type { CostModelId, FillModelId } from 'lib/strategy/types';
import type { U64Wire } from 'lib/u64Wire';

export type { RunMetrics, RunSummary } from 'lib/strategy/runSummary';
export type { U64Wire } from 'lib/u64Wire';

export type PriceUnit = 'SOL' | 'USD';

export interface PriceUnitState {
  unit: PriceUnit;
  usdRate: number | null;
}

/** The shared token-enrichment fields the backend `TokenEnrichment` struct
 *  (`trading_core::storage::token_enrichment`) flattens onto every token-result row.
 *  All optional here because most result tables receive them only after a batch
 *  enrich resolves (the fully-populated `TokenRecord`/`TokenDetailRecord` redeclare
 *  them required). Single source: interfaces `extends` this instead of re-listing the
 *  block, so a backend field add/rename is one edit, not five. */
export interface TokenEnrichmentFields {
  symbol?: string;
  name?: string;
  creator_wallet?: string;
  initial_buy_sol?: number | null;
  /** Raw on-chain `u64` creation-instruction args — sent as **strings** so values
   *  above 2^53 survive (pump.fun's `u64::MAX` "no slippage cap" ceiling). Read
   *  them through `lib/u64Wire`, never with bare arithmetic. */
  initial_supply_token?: U64Wire;
  token_amount?: U64Wire;
  max_cost_lamports?: U64Wire;
  spendable_lamports_in?: U64Wire;
  min_tokens_out?: U64Wire;
  cu_limit?: number | null;
  cu_price?: number | null;
  is_mayhem_mode?: boolean;
  is_cashback_enabled?: boolean;
  creation_tx_signature?: string;
  ix_labels_count?: number;
  instruction_labels?: unknown;
  trade_count?: number;
  current_price?: number | null;
  volume_sol_total?: number;
  first_slot_buy_sol?: number | null;
  first_slot_sell_sol?: number | null;
  market_cap?: number | null;
  ath_price?: number | null;
  ath_timestamp?: string | null;
  is_migrated?: boolean;
  is_dead?: boolean;
  last_trade_at?: string | null;
  last_synced_at?: string | null;
}

export interface TokenRecord {
  mint_address: string;
  name: string;
  symbol: string;
  creator_wallet: string;
  trade_count: number;
  current_price: number | null;
  volume_sol_total: number;
  /** Buy/sell SOL summed over trades in the token's creation slot (human SOL).
   *  `null` for tokens predating the metric or with no creation-slot activity. */
  first_slot_buy_sol: number | null;
  first_slot_sell_sol: number | null;
  ath_price: number | null;
  ath_timestamp: string | null;
  market_cap: number | null;
  initial_buy_sol: number | null;
  /** Raw on-chain `u64` args — strings on the wire, see `lib/u64Wire`. */
  initial_supply_token: U64Wire;
  token_amount: U64Wire;
  max_cost_lamports: U64Wire;
  spendable_lamports_in: U64Wire;
  min_tokens_out: U64Wire;
  cu_limit: number | null;
  cu_price: number | null;
  ix_labels_count: number;
  instruction_labels: unknown;
  is_migrated: boolean;
  /** Dead-token verdict: liquidity gone + price back at launch + only dust trading
   *  (backend `TokenState::is_dead`). A near-stable flag once true. */
  is_dead: boolean;
  is_mayhem_mode: boolean;
  is_cashback_enabled: boolean;
  created_at: string;
  /** `created_at` pre-parsed to epoch-ms once at ingest (see apiSlice transform),
   *  so age cells never re-parse the ISO string per render/tick. */
  created_at_ms?: number;
  creation_tx_signature: string;
  last_trade_at: string | null;
  /** Gap-aware lifetime in seconds (creation → last non-stray trade); `Some` only
   *  when the token is dead, per backend `TokenState::lifetime_secs`. */
  lifetime_secs: number | null;
  last_synced_at: string | null;
}

/**
 * One exit fill on a scale-out ladder — mirrors the backend `ExitFillLeg`.
 *
 * SSOT: the ONE leg shape. A simulated ladder (`SimulatedTokenResult.exit_legs`)
 * and a traded one (`RulePositionRecord.exit_legs`) serialize the same struct, so
 * `buildEventMarkers` draws both through one path. Present only on a genuine
 * ladder (>= 2 legs); a single close is already carried by the `exit_*` fields.
 */
export interface ExitFillLeg {
  /** Share of the initial bag this leg sold. */
  sell_bps: number;
  price: number;
  time: string;
  tx: string | null;
  reason: string | null;
}

export interface RulePositionRecord extends TokenEnrichmentFields {
  id: string;
  mint_address: string;
  wallet: string;
  /** Target (trigger-trade) snapshot — the scalp-entry signal trade that armed
   * this position, distinct from the actual entry fill. null until armed. The
   * gap vs. the entry_* fields is derived client-side, not stored. */
  target_price: number | null;
  /** Trigger trade's TOKEN count; SOL derived as `target_price × target_token_amount`. */
  target_token_amount: number | null;
  target_time: string | null;
  target_tx: string | null;
  /** Entry-fill price. null while the position is armed (target set) but the
   *  entry trade hasn't filled yet — the DB columns are nullable, so every
   *  render/computation must null-guard. */
  entry_price: number | null;
  /** Tokens bought at entry; SOL derived as `entry_price × entry_token_amount`.
   *  null until the entry fills (see `entry_price`). */
  entry_token_amount: number | null;
  entry_time: string | null;
  entry_tx: string;
  /** On a scale-out this is the SOL-weighted **average** across legs, not a price
   *  anything filled at — charts prefer `exit_legs` and fall back here only for a
   *  single-leg close, where the two are the same number. */
  exit_price: number | null;
  /** Per-leg sell fills in fire order, present wherever `exit_price` alone cannot
   *  describe them — a ladder, or an open position that has banked a leg. Absent on
   *  a closed single-leg close (identical to `exit_*`) and on the non-paged reads
   *  (the backend attaches these per page). */
  exit_legs?: ExitFillLeg[] | null;
  /** Tokens sold at exit; SOL derived as `exit_price × exit_token_amount`. */
  exit_token_amount: number | null;
  /** Running sum of confirmed sell-leg raw token units (scale-out; mig 0018). */
  sold_token_amount?: number;
  /** Running sum of confirmed sell-leg SOL (scale-out aggregate). */
  exit_sol_total?: number;
  /** Next scale-out stage index (`0` = pre-first / legacy). */
  scale_stage?: number;
  /** Sold fraction of the initial bag in bps. */
  sold_bps?: number;
  exit_time: string | null;
  exit_tx: string | null;
  pnl_percent: number | null;
  /** Realized SOL PnL from the backend (null until the position closes). */
  pnl_sol: number | null;
  status: string;
  strategy: string;
  /** Execution mode (`real` | `paper`). The cross-rule History view mixes both,
   *  so every row carries it; per-rule views infer it from the rule. */
  mode?: string;
  /** SOL spent at entry (from `entry_lamports`) — the History Entry column and
   *  the `pnlPctFromSol` denominator. Null until the entry fills. */
  entry_sol?: number | null;
  /** Owning rule id; null if the rule was deleted (`ON DELETE SET NULL`). */
  rule_id: string | null;
  /** Why the position exited (TakeProfit/StopLoss/TrailingStop/Stall/TimeStop/
   * LiquidityExit); null while still open. */
  exit_reason: string | null;
  /** Owning run's monotonic sequence — populated ONLY by the run-history ("old
   * runs") view, where it drives the run column + per-run banding. null/absent on
   * the current-run/live paths (single run). */
  run_seq?: number | null;
  /** `bot` | `manual` — who opened the position. */
  origin?: string;
  /** Manual-position TP/SL config (`{tp_pct, sl_pct}`); null on bot rows. */
  manual_exit?: { tp_pct?: number | null; sl_pct?: number | null } | null;
  /** Backend-derived B3 flag: stale unresolved BuySubmitted — needs Verify. */
  needs_review?: boolean;
  /** ExitStuck redrive state (mig 0012): parked ⇒ auto-retry stopped. */
  exit_parked?: boolean;
  exit_redrive_count?: number;
  /** Why the most recent buy attempt did not fill — send error or Anchor code
   *  (mig 0017). This is what explains an `EntryFailed` row, which carries no
   *  `exit_reason` (nothing ever exited). Not cleared on a later success, so on a
   *  `Holding` row it is the history of what it took to get in. */
  last_entry_error?: string | null;
  created_at: string;
  updated_at: string;
  // Token enrichment fields (populated by the batch endpoint) come from
  // `TokenEnrichmentFields`; only the record-specific extras stay here.
}

/** Run/rule-wide position aggregates for the Positions Summary panel — computed
 *  server-side over the *entire* population (never a page), mirroring the backend
 *  `PositionsSummary`. `tokens` = entered positions; `open` = holding-index rows;
 *  a win is a clean `End` exit with positive realized SOL. SOL fields are human SOL. */
export interface PositionsSummary {
  tokens: number;
  open: number;
  win: number;
  loss: number;
  closed: number;
  win_rate: number;
  /** **Canonical return %** — capital-weighted realized return over the closed
   *  positions (`total_pnl_sol / closed_entry_sol × 100`). NOT a mean of
   *  per-trade percents: a rule's buy size is editable mid-run, so a 1.0 SOL
   *  trade must outweigh a 0.05 SOL one. Sign-locked to `total_pnl_sol`. */
  return_pct: number;
  /** Realized only — closed positions. Never includes an open position's mark. */
  total_pnl_sol: number;
  /** Unrealized mark-to-current-price PnL of the still-open positions, priced
   *  through the same cost model the sim and the sweep use. Reported beside
   *  `total_pnl_sol`, never folded into it, so a rule holding its losers open
   *  can't read as profitable. */
  open_pnl_sol: number;
  /** Entry cost across ALL entered positions, open ones included — so it is not
   *  the denominator of a realized return. Use `closed_entry_sol`. */
  total_entry_sol: number;
  /** Entry cost of the CLOSED positions — `return_pct`'s exact denominator,
   *  shipped so a caller spanning several scopes can re-weight by capital
   *  (`Σ pnl / Σ closed_entry`) instead of by trade count. */
  closed_entry_sol: number;
  total_holding_sol: number;
  total_gains_sol: number;
  total_losses_sol: number;
  avg_hold_secs: number;
  best_pct: number | null;
  worst_pct: number | null;
  /** Entered tokens that graduated to AMM (`is_migrated`) — counted among
   *  `tokens`, independent of exit reason. */
  migrated: number;
  /** Closed-position counts by `exit_reason`. Deliberately not exhaustive of
   *  `closed` — a close can carry no reason at all — so the summary
   *  reconciles the remainder into a visible `Other` slice. Optional for older
   *  live binaries that predate the exits aggregate. */
  exits?: ExitReasonCounts;
}

/** Mirrors the Rust `ExitReasonCounts`. */
export interface ExitReasonCounts {
  take_profit: number;
  stop_loss: number;
  /** Total metric-condition exits (`metrics_win + metrics_loss`). */
  metrics: number;
  /** Metric exits with positive realized SOL. */
  metrics_win?: number;
  /** Metric exits that are not wins (loss or break-even). */
  metrics_loss?: number;
  dead: number;
  manual: number;
  trailing: number;
  stall: number;
  time: number;
  liquidity: number;
}

export interface SimulatedTokenResult extends TokenEnrichmentFields {
  mint_address: string;
  symbol: string;
  /** Whether the strategy took a position. `false` ⇒ matched fingerprint but
   *  never entered (`exit_reason: "NoEntry"`) — same contract as sweep
   *  `ComboTokenResult.fired`. Absent on legacy resident payloads. */
  fired?: boolean;
  /** Trigger-trade (scalp signal) snapshot that armed the position, distinct
   * from the worst-case `entry_*` fill. The gap is the modeled adverse
   * slippage. null only for legacy paper rows that never recorded a target. */
  target_price: number | null;
  /** Trigger trade's TOKEN count; SOL derived as `target_price × target_token_amount`. */
  target_token_amount: number | null;
  target_time: string | null;
  target_tx: string | null;
  /** Null when not fired (`NoEntry`). */
  entry_price: number | null;
  /** All-time-high price from `tokens_info` (row-owned enrichment); null if the
   *  token has no info row. */
  ath_price: number | null;
  /** SOL notional the rule deployed (rendered as entry size); null when not fired. */
  entry_token_amount: number | null;
  entry_tx: string | null;
  entry_time: string | null;
  exit_price: number | null;
  exit_tx: string | null;
  exit_time: string | null;
  /** Scale-out exit fills in fire order. Absent / empty on legacy single-exit
   *  rows and never-sold opens. Chart markers render one arrow per leg; the
   *  position-level `exit_*` fields still stamp the last leg. */
  exit_legs?: ExitFillLeg[] | null;
  holding_secs: number | null;
  pnl_percent: number | null;
  pnl_sol: number | null;
  /** Includes `"NoEntry"` for matched-but-never-entered candidates. */
  exit_reason: string;
  // Token enrichment fields come from `TokenEnrichmentFields` (the backend bakes
  // them in once per backtest run via `lab::strategies::token_enrich`); only
  // `created_at`, which the base leaves to row-owners, stays here.
  created_at?: string;
}

/** Filtered-population aggregate for the Simulated summary card (server-side over
 *  rows matching the table's search/filters). Mirrors the lab `sim_result_summary`
 *  handler, which serializes the core kernel's `RunSummary` verbatim.
 *
 *  This **is** the shared run-summary shape — the same two-band `realized`/`mtm`
 *  payload the grouped sweep aggregates to and the live/paper positions card maps
 *  onto — so one renderer (`lib/strategy/runSummary`) draws all three. It replaced
 *  a narrow five-field shape whose `total_pnl_sol` silently folded in unrealized
 *  open marks (parity plan B4/F1). */
export interface SimulatedSummary extends RunSummary {
  /** ISO time the run's result was generated — rendered as relative time ("20m
   *  ago") in the Simulate table's Run column. Null/absent for legacy payloads. */
  computed_at?: string | null;
  /** Fired tokens that graduated to AMM (`is_migrated`), over the filtered
   *  cohort. Absent on legacy payloads → the Migrated tile is then hidden. */
  n_migrated?: number;
  /** Distinct tokens whose creation axes matched this rule's fingerprint — the
   *  candidate pool `realized.n_fired` positions are drawn from (entered one or
   *  more times, or matched-but-never-entered `NoEntry`). Counted **once per
   *  token**, unlike `realized.n_fired`/`n_closed`/`n_open`, which count
   *  positions — a re-entry rule (`RuleParams.reentry`) can enter the same token
   *  more than once, so `n_fired` can exceed `n_matched`. On the Simulate
   *  table's per-rule hydrate this is the run's full unfiltered count
   *  (`SimMeta::n_matched`); on the drill-in summary card it's scoped to the
   *  table's current search/filters, like the other counts there. Absent on
   *  legacy payloads computed before this field existed (re-run the simulation
   *  to backfill). */
  n_matched?: number;
  /** Distinct tokens the rule actually **entered** (≥1 fired episode) — narrower
   *  than `n_matched` (the whole candidate pool, entered or not) and different
   *  from `realized.n_fired` (every entry; a re-entry rule's repeat visits to
   *  one mint each add to it). `realized.n_fired - n_tokens_entered` is the
   *  run's re-entry volume: `0` for a one-shot rule, positive whenever a mint
   *  fired more than once. Same filtered/unfiltered scoping as `n_matched`.
   *  Absent on legacy payloads computed before this field existed (re-run the
   *  simulation to backfill). */
  n_tokens_entered?: number;
  /** Which fill model priced this run's round-trips — rendered as the Simulate
   *  table's Fill column. Absent/null on legacy payloads → falls back to the
   *  default (worst-case) label. */
  fill_model?: FillModelId | null;
  /** Which execution-cost model priced this run's round-trips — rendered as the
   *  Simulate table's Cost column. Absent/null on legacy payloads → falls back to
   *  the default (`pumpfun_default`) label. */
  cost_model?: CostModelId | null;
}

/** Hold + wall-clock bins for the Temporal summary band — mirrors lab
 *  `sim_query::time_summary` / FE `buildTemporalSummary`. */
export interface TemporalSummaryPayload {
  hold: import('lib/strategy/temporalSummary').HoldBinStats[];
  /** Hold-duration scale actually used (auto or override). */
  holdScheme?: import('lib/strategy/temporalSummary').HoldScheme;
  /** Auto pick for this cohort (present even when `holdScheme` was overridden). */
  holdSchemeAuto?: import('lib/strategy/temporalSummary').HoldScheme;
  wall: import('lib/strategy/temporalSummary').WallCellStats[];
  wallGrain: import('lib/strategy/temporalSummary').WallGrain;
  /** Auto pick for this cohort (present even when `wallGrain` was overridden). */
  wallGrainAuto?: import('lib/strategy/temporalSummary').WallGrain;
  /** max−min wall timestamps in ms. */
  wallSpanMs?: number;
  wallField: 'entry_time' | 'created_at';
  nFired: number;
}

/** Payload of the `simulation_progress` SSE event: `processed` of `total`
 *  candidate tokens resolved for the in-flight backtest of `rule_id`. */
export interface SimulationProgressEvent {
  rule_id: string;
  processed: number;
  total: number;
}

/** Payload of the `sweep_progress` SSE event: `processed` of `total` tokens
 *  folded across all surviving groups of the in-flight grouped sweep. `phase`
 *  identifies which phase is reporting: `"corpus"` (lake load; `total: 0` ⇒
 *  indeterminate) | `"coarse"` (refine runs only) | `"sweep"`. */
export interface SweepProgressEvent {
  strategy_id: string;
  phase: string;
  processed: number;
  total: number;
}

/** Payload of the `sweep_group_done` SSE event: one grouped-sweep group is
 *  fully folded AND committed — its rows are readable from the run's `groups`
 *  endpoint right now, mid-run. One frame per persisted group plus an announce
 *  frame (`group_index: null`, `groups_done: 0`) when the surviving counts are
 *  first known. Drives the live groups-table refresh + "persisted N/M". */
export interface SweepGroupDoneEvent {
  strategy_id: string;
  run_id: string;
  group_index: number | null;
  groups_done: number;
  group_count: number;
}

/** Payload of the `sweep_notice` SSE event: a NON-terminal advisory for a running
 *  grouped sweep — today, that the engine degraded its sizing to fit free RAM
 *  (fewer threads / smaller fold buffers). The run continues and its results are
 *  unaffected; it is only slower. Surfaced as an info toast so a degraded run
 *  reads as "slow, and here's why" rather than as a stall. */
export interface SweepNoticeEvent {
  strategy_id: string;
  message: string;
}

/** Payload of the `sweep_finished` SSE event: the single-flight grouped sweep
 *  for `strategy_id` ended (`cancelled` = user abort vs normal finish/error).
 *  `error` is set on a failure or a short write. The run row is kept in every
 *  case (status `partial` when some groups committed, `failed` when none did),
 *  so this toast explains a run the user can still open and inspect. */
export interface SweepFinishedEvent {
  strategy_id: string;
  cancelled: boolean;
  error?: string | null;
}

/** Payload of the `simulation_finished` SSE event: the backtest for `rule_id`
 *  ended (`cancelled` = user abort vs normal finish/error). */
export interface SimulationFinishedEvent {
  rule_id: string;
  cancelled: boolean;
}

/** Response of `GET /api/jobs/status` — a snapshot of every running background
 *  job, used to recover the progress UI after a page load/refresh (SSE only
 *  delivers future frames). `sweep` is present iff the single-flight sweep runs. */
export interface JobsStatus {
  sweep: { processed: number; total: number } | null;
  simulations: { rule_id: string; processed: number; total: number }[];
  /** Present iff the single-flight flow-discovery job is running. */
  discovery: { processed: number; total: number } | null;
}

/** Payload of the `flow_discovery_progress` SSE event. */
export interface FlowDiscoveryProgressEvent {
  run_id: string;
  phase: string;
  processed: number;
  total: number;
}

/** Payload of the `flow_discovery_finished` SSE event. */
export interface FlowDiscoveryFinishedEvent {
  run_id: string;
  cancelled: boolean;
  error?: string | null;
}

/** Payload of the `flow_discovery_notice` SSE event. */
export interface FlowDiscoveryNoticeEvent {
  run_id: string;
  message: string;
}

/** One wallet's gross SOL contribution to a `FlowDiscoveryStructure`. */
export interface FlowDiscoveryWalletGross {
  wallet_hash: string;
  gross_sol: number;
}

/** One ranked ix-structure from a flow-discovery group. */
export interface FlowDiscoveryStructure {
  ix_labels: string[];
  volume_share: number;
  wash_symmetry: number;
  cross_token_recurrence: number;
  group_lift: number;
  slot_burst: number;
  wallet_reuse: number;
  wallet_overlap: number;
  n_trades: number;
  gross_sol: number;
  buy_sol: number;
  sell_sol: number;
  /** Gross SOL of this shape that landed in its token's **creation slot** — the
   *  launch-bundle share, which `firstSlotPurity` turns into the Launch% column.
   *  `null`/absent on a result cached before the backend computed it: unknown,
   *  NOT 0% (see the Rust `StructureScore` doc). */
  first_slot_gross_sol?: number | null;
  /** Trade count behind `first_slot_gross_sol`, and the input to
   *  `isFirstSlotPresent` (the *Launch shapes · group* predicate: `> 0`). Same
   *  unknown-vs-zero contract — `null` selects nothing rather than guessing. */
  first_slot_trades?: number | null;
  wallets: FlowDiscoveryWalletGross[];
}

/** One token's aggregate contribution to a `FlowDiscoveryGroup` — a cheap
 *  roster (no trade payload) driving the per-token preview picker. */
export interface FlowDiscoveryTokenGross {
  mint_address: string;
  gross_sol: number;
  n_trades: number;
  /** This token's creation slot, or `null` when no corpus trade carries one. */
  first_slot?: number | null;
  /** **Every** distinct ix shape that traded in THIS token's creation slot,
   *  ranked by first-slot gross desc — uncapped and unfloored, unlike the
   *  group-wide `structures` list (which is ranked, truncated server-side, and
   *  read through a dust floor). Drives the per-token *Select launch shapes*.
   *
   *  `null`/absent on a result cached before the backend computed it: **unknown**,
   *  not "this token had no launch bundle" — `[]` is that real zero. */
  first_slot_ix_labels?: string[][] | null;
}

/** One fingerprint group in a flow-discovery result. */
export interface FlowDiscoveryGroup {
  group_key: Record<string, string>;
  n_tokens: number;
  n_trades_scored: number;
  ambiguity: boolean;
  /** Whether `group_lift` carries information. `false` when this group IS the
   *  whole scored corpus (a fingerprint-scoped run, or no group-by): every lift
   *  is then exactly 1.0 by construction. **Skip the lift gate when false, never
   *  fail it** — failing it rejects every row of the run. Absent on a result
   *  cached before the backend echoed it; treat as `true`, which is how those
   *  runs were already read. */
  lift_defined?: boolean;
  structures: FlowDiscoveryStructure[];
  /** Ranked (desc gross_sol) member-token roster, capped server-side. */
  tokens: FlowDiscoveryTokenGross[];
}

/** `GET /api/strategies/flow-discovery/{run_id}` response. */
export interface FlowDiscoveryResult {
  run_id: string;
  groups: FlowDiscoveryGroup[];
  /** Bucket width (SOL) the run binned the continuous SOL group axes at, or
   *  `null` when it keyed them on their **exact** amount (`SolPrecision::Exact`).
   *  Precision is part of fingerprint identity, so never substitute a default.
   *
   *  Read this — not the page's live form state — whenever a group key is turned
   *  into a fingerprint identity: the page rehydrates a disk-cached result on
   *  mount, so the form can describe a completely different run. Absent only on a
   *  result cached before the backend echoed it, where the backend substitutes the
   *  0.1 default those runs actually used. */
  bucket_width_sol?: number | null;
  /** The exact-set instruction-label filter the run applied to its corpus, or
   *  `null`. Part of the identity a group binds to — the group key never carries
   *  it — so it must be re-attached via `withIxLabelsFilter` before matching. */
  ix_labels_filter?: string[] | null;
  /** Saved fingerprint the corpus was scoped to, or `null` when unscoped. Every
   *  group is a sub-slice of it, so it is the authoritative attribution. */
  fingerprint_id?: string | null;
}

/** The live (real) strategy managing a held mint — the Holdings bot badge and the
 *  manual-vs-bot double-sell guard. Mirrors the backend `ManagedMint`. */
export interface ManagedBy {
  mint_address: string;
  rule_id: string | null;
  rule_name: string | null;
  /** Open-partition status (`BuySubmitted` | `Holding` | `ExitPending` |
   *  `ExitStuck` | `ExitUnconfirmed`). */
  status: string;
  /** `real` | `paper` (Holdings only ever surfaces `real`). */
  mode: string;
}

/** One enriched wallet holding from `GET /api/portfolio/holdings`. Extends the
 *  shared {@link TokenEnrichmentFields} (the backend flattens the same enrichment
 *  SSOT onto each row) and adds the live wallet fields, SOL valuation, cost basis,
 *  unrealized PnL, and the bot-managed tag. `is_migrated`/`is_cashback_enabled`/
 *  `symbol` carry the live-authoritative values. PnL fields are `undefined` on the
 *  lean single-mint confirmation response until the next full refresh. */
/** Wallet classification — mirrors Rust `AssetKind` (`cash` | `wrapped_sol` | `meme`). */
export type WalletAssetKind = 'cash' | 'wrapped_sol' | 'meme';

export interface WalletHolding extends TokenEnrichmentFields {
  mint_address: string;
  amount: number;
  ui_amount: number;
  decimals: number;
  token_account: string;
  token_program_id: string;
  /** Server classification; cash = dry powder (USDC), not a trading position. */
  asset_kind?: WalletAssetKind;
  price_usd: number | null;
  value_usd: number | null;
  liquidity: number | null;
  price_change_24h: number | null;
  token_created_at: string | null;
  is_migrated: boolean;
  is_cashback_enabled: boolean;
  /** Mark-to-market SOL value of the bag; `null`/absent when no live SOL mark. */
  value_sol?: number | null;
  /** Remaining bag's cost basis in SOL; `null`/absent when no recorded buys / cash. */
  cost_basis_sol?: number | null;
  unrealized_pnl_sol?: number | null;
  unrealized_pnl_pct?: number | null;
  /** The live strategy managing this bag, or `null` when unmanaged (orphan/manual). */
  managed_by?: ManagedBy | null;
}

/** Wallet-wide roll-up from `GET /api/portfolio/summary` — the Home KPI row.
 *  Mirrors the backend `PortfolioSummary`. Value totals include cash; cost basis /
 *  unrealized PnL / `position_count` are meme positions only. */
export interface PortfolioSummary {
  total_value_sol: number;
  total_value_usd: number;
  cash_value_usd: number;
  cash_value_sol: number;
  positions_value_sol: number;
  positions_value_usd: number;
  total_cost_basis_sol: number;
  total_unrealized_pnl_sol: number;
  /** Held meme bags (excludes cash). */
  position_count: number;
  /** Realized SOL PnL from real positions that cleanly exited since 00:00 UTC. */
  realized_pnl_today_sol: number;
  /** Active real-mode rules. */
  active_rules: number;
  /** Open real strategy positions across all rules. */
  open_position_count: number;
}

/** One open strategy position from `GET /api/portfolio/positions` — the fields the
 *  Home per-strategy strip and the Live-Trading roll-up read (a subset of the
 *  backend `StrategyPosition`). */
export interface OpenStrategyPosition {
  /** `strategy_positions.id` — required for the per-row Sell ALL close path. */
  id: string;
  strategy_id: string;
  rule_id: string | null;
  mint_address: string;
  mode: string;
  status: string;
  entry_price?: number | null;
  entry_sol?: number | null;
  entry_time?: string | null;
  /** Raw token units at entry (for sold_bps when SSE omits it). */
  entry_token_amount?: number | null;
  /** Running sum of confirmed sell-leg raw token units (scale-out). */
  sold_token_amount?: number;
  /** Sold fraction of the initial bag in bps. */
  sold_bps?: number;
  /** Next scale-out stage index. */
  scale_stage?: number;
  /** `bot` | `manual` — who opened the position. */
  origin?: string;
  /** Backend-derived B3 flag: stale unresolved BuySubmitted, needs Verify. */
  needs_review?: boolean;
  /** ExitStuck redrive state (mig 0012). */
  exit_parked?: boolean;
  exit_redrive_count?: number;
}

/** One `position_fills` ledger row (`GET …/positions/{id}/fills`). */
export interface PositionFill {
  position_id: string;
  seq: number;
  side: 'buy' | 'sell' | string;
  price: number;
  sol_lamports: number;
  token_amount: number;
  at: string;
  reason?: string | null;
  stage?: number | null;
  tx_signature?: string | null;
}

/** `GET /api/portfolio/performance` — Portfolio page rollup. */
export interface PortfolioRulePnl {
  rule_id: string;
  rule_name: string | null;
  closed: number;
  win: number;
  loss: number;
  win_rate: number;
  realized_pnl_sol: number;
  /** Capital deployed across the window's closed positions (human SOL). */
  closed_entry_sol: number;
  /** `realized_pnl_sol / closed_entry_sol × 100` — computed server-side so this
   *  percent is the same definition as the Rules board's. */
  return_pct: number;
}

export interface PortfolioPerformance {
  range: 'today' | '7d' | 'all' | string;
  mode: string;
  since: string | null;
  realized_pnl_sol: number;
  closed: number;
  win: number;
  loss: number;
  win_rate: number;
  /** Capital deployed across every closed position in the window (human SOL). */
  closed_entry_sol: number;
  /** Window-wide capital-weighted return — Σ pnl / Σ capital across the rules,
   *  never a mean of their percents. */
  return_pct: number;
  by_rule: PortfolioRulePnl[];
}

/// Live, fast-changing market data for one mint (Jupiter). Fetched separately
/// from the slow wallet balance read so the wallet table can refresh values on
/// a poll without re-scanning the chain; merged onto {@link WalletHolding}.
export interface WalletPrice {
  price_usd: number | null;
  liquidity: number | null;
  price_change_24h: number | null;
  token_created_at: string | null;
}

/** Accrued pump.fun cashback for one venue pot ("curve" / "amm"). Lamports are
 *  raw integers (JS-safe at these magnitudes); render as SOL. */
export interface CashbackPot {
  label: string;
  exists: boolean;
  claimable_lamports: number;
  stable_claimable: number;
}

export interface CashbackStatus {
  pots: CashbackPot[];
  total_claimable_lamports: number;
}

/** One pot's outcome from POST /api/cashback/claim. */
export interface CashbackClaimOutcome {
  label: string;
  /** SOL pots: claimed lamports. Stable pot (`is_stable`): raw stable-mint units. */
  claimable_lamports: number;
  /** True for the curve stable pot — claim lands as an SPL token balance, not SOL. */
  is_stable: boolean;
  signature: string | null;
  error: string | null;
}

export interface CashbackClaimResult {
  claimed_lamports: number;
  /** Raw stable-mint units swept from the curve stable pot (separate token). */
  claimed_stable: number;
  pots: CashbackClaimOutcome[];
}

export interface TokenDetailRecord {
  mint_address: string;
  name: string;
  symbol: string;
  creator_wallet: string;
  bonding_curve_address: string | null;
  /** Raw on-chain `u64` args — strings on the wire, see `lib/u64Wire`. */
  initial_supply_token: U64Wire;
  initial_buy_sol: number | null;
  token_amount: U64Wire;
  max_cost_lamports: U64Wire;
  spendable_lamports_in: U64Wire;
  min_tokens_out: U64Wire;
  cu_limit: number | null;
  cu_price: number | null;
  instruction_labels: unknown;
  is_mayhem_mode: boolean;
  is_cashback_enabled: boolean;
  creation_tx_signature: string;
  created_at: string;
  // Coalesced to 0 by the backend (matches the list endpoint), so never null —
  // unlike the other tokens_info-derived fields below.
  trade_count: number;
  volume_sol_total: number;
  first_slot_buy_sol: number | null;
  first_slot_sell_sol: number | null;
  market_cap: number | null;
  current_price: number | null;
  ath_price: number | null;
  ath_timestamp: string | null;
  is_migrated: boolean;
  unique_wallets: number | null;
  last_trade_at: string | null;
  last_synced_at: string | null;
}

/** One row of the Trader Analysis token table — a full {@link TokenRecord} (so it
 *  renders through the same columns as the All Tokens table) plus the wallet's
 *  interaction stats on that mint. Returned by `GET /api/wallets/:wallet/tokens`,
 *  most-recent-trade first. */
/**
 * One Trader Analysis table row: the full token record plus the wallet's
 * interaction stats AND a reconstructed avg-cost PnL on that mint (backend
 * `wallets.rs::WalletTokenRow` / `kernel::wallet_mint_pnl` — see those doc
 * comments for exactly how each figure is derived and what `wallet_partial_data`
 * means). All scoped to the same look-back window, so a mint the wallet only
 * *exited* can show 0 buys and every PnL figure flagged `wallet_partial_data`.
 */
export interface TraderTokenRow extends TokenRecord {
  /** The wallet's first trade on this mint *within the window* — paired with
   *  `wallet_last_trade_at` as a hold-duration proxy at this per-mint grain. */
  wallet_first_trade_at: string;
  /** `wallet_first_trade_at` pre-parsed to epoch-ms (see labEndpoints transform),
   *  so the analytics panel never re-parses the ISO string per chart render. */
  wallet_first_trade_at_ms?: number;
  /** The wallet's most-recent trade on this mint — the table's default sort. */
  wallet_last_trade_at: string;
  wallet_last_trade_at_ms?: number;
  /** The wallet's buy/sell counts on this mint within the look-back window.
   *  Scoped to the window, so a mint the wallet only *exited* can show 0 buys. */
  wallet_buy_count: number;
  wallet_sell_count: number;
  /** Σ SOL bought/sold in the window (recorded curve-side amount, pre-fee). */
  wallet_buy_sol: number;
  wallet_sell_sol: number;
  /** SOL per raw token unit (same convention as `current_price`); `null` when
   *  that side has no legs in the window. */
  wallet_avg_buy_price: number | null;
  wallet_avg_sell_price: number | null;
  /** `buy_token_amount - sell_token_amount` (raw units). Positive = still
   *  holding a bag; negative only when `wallet_partial_data` is true. */
  wallet_net_token_amount: number;
  /** Realized PnL on the matched (closed) portion, gross of the pump.fun fee. */
  wallet_realized_pnl_sol: number;
  /** Same, net of the measured ~125bps/leg pump.fun protocol fee. */
  wallet_realized_pnl_sol_net_of_fee: number;
  /** `null` when there's no matched cost basis to divide by (no buys). */
  wallet_realized_pnl_pct: number | null;
  /** Mark-to-market PnL on the still-open bag; `null` when there's no open bag
   *  or the current price is unknown. */
  wallet_unrealized_pnl_sol: number | null;
  /** `realized_pnl_sol + (unrealized_pnl_sol ?? 0)` — the one ranking number. */
  wallet_total_pnl_sol: number;
  /** `net_token_amount > 0` — still holding some of this mint. */
  wallet_is_open: boolean;
  /** The wallet sold more than it bought in the window (opening buy predates the
   *  window) — every PnL figure above is a partial-window estimate. */
  wallet_partial_data: boolean;
}

/** Per-token live stats pushed alongside each trade (backend `live_stats`).
 *  Field names mirror {@link TokenRecord} so they patch straight into a row. */
export interface TokenLiveStats {
  current_price: number | null;
  volume_sol_total: number;
  market_cap: number | null;
  trade_count: number;
  ath_price: number | null;
  ath_timestamp: string | null;
  last_trade_at: string | null;
}

export interface LiveTrade {
  mint_address: string;
  wallet: string;
  trade_type: string;
  amount_sol: number;
  token_amount: number;
  price_per_token: number;
  /** Per-transaction network fee in SOL — see `TradeRecord.fee_sol`. Absent on
   *  frames from a bin predating the field. */
  fee_sol?: number | null;
  tx_signature: string;
  /** Canonical intra-slot order — must match `TradeRecord` / chart sort. */
  tx_index: number;
  leg_index: number;
  /** Venue-neutral post-trade reserves (optional on older frames). */
  reserve_sol?: number | null;
  reserve_token?: number | null;
  venue?: 'curve' | 'amm' | string;
  /** Ordered ix-structure labels — see `TradeRecord.instruction_labels`. Drives the
   *  chart's vol/non-vol classification, so a frame without them appends a row that
   *  reads as non-vol and never tags its wallet. Absent on frames from a bin
   *  predating the field; `[]` when the decoder captured none. */
  instruction_labels?: string[] | null;
  slot: number;
  timestamp: string;
  /** Snapshot of the mint's stats after this trade; absent if the token isn't
   *  in the live cache. Used to patch the token grid without re-polling. */
  live?: TokenLiveStats | null;
}

export interface SyncProgressEvent {
  type: 'progress';
  stage: string;
  current: number;
  total: number;
  message: string;
}

export interface SyncCompleteEvent {
  type: 'complete';
  token: TokenDetailRecord;
  trades: TradeRecord[];
}

export interface SyncErrorEvent {
  type: 'error';
  message: string;
}

export type SyncStreamEvent = SyncProgressEvent | SyncCompleteEvent | SyncErrorEvent;

export interface TradeRecord {
  id: string;
  mint_address: string;
  wallet_address: string;
  trade_type: 'buy' | 'sell';
  amount_sol: number;
  token_amount: number;
  price_per_token: number;
  /** Network fee paid to land this trade's TRANSACTION — base signature fee +
   *  priority fee, in SOL. Charged once per tx, so every leg of a multi-leg tx
   *  repeats the same value: collapse by `tx_signature` before summing.
   *  Excludes the Jito tip (a transfer, not a fee) and the venue protocol fee
   *  (already inside `amount_sol`). Null on trades ingested before the column
   *  existed — unbackfillable, and distinct from a zero fee, which a landed
   *  transaction never pays. */
  fee_sol?: number | null;
  tx_signature: string;
  /** Position of this trade's transaction within its block — the real intra-slot
   *  ordering key. Part of the canonical trade order `slot → tx_index → leg_index`. */
  tx_index: number;
  leg_index: number;
  slot: number;
  block_time: string;
  received_at?: string;
  /** Reserve pair this row prices from (venue-neutral): curve virtual reserves on
   *  curve rows, pool real reserves on amm rows. Spot = reserve_sol / reserve_token. */
  reserve_sol?: number | null;
  reserve_token?: number | null;
  real_reserve_sol?: number | null;
  real_token_reserves?: number | null;
  /** Trading venue: 'curve' (bonding curve) or 'amm' (post-migration PumpSwap). */
  venue?: 'curve' | 'amm';
  /** Ordered ix-structure labels for this trade's tx, when the ingest pipeline
   *  captured them — null/absent on pre-labeling history. Drives flow-split
   *  structural matching (`hunter_engine::metrics::flow_split::ix_hash`). */
  instruction_labels?: string[] | null;
}

export type ProfileType = 'mine' | 'trader' | 'whale' | 'dev';

export interface WalletProfileTag {
  id: string;
  name: string;
  color: string;
  comment: string | null;
  created_at: string;
}

export interface WalletEntry {
  id: string;
  profile_id: string;
  address: string;
  is_tracked: boolean;
  comment: string | null;
  created_at: string;
  last_seen_at: string | null;
}

export interface WalletProfile {
  id: string;
  name: string;
  profile_type: ProfileType;
  created_at: string;
  wallets: WalletEntry[];
  tags: WalletProfileTag[];
}
