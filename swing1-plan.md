# Plan: `swing1` — Kill→Volume Swing-Phase Strategy

## Context

**Why this exists.** Meme-coin dev creators intentionally manufacture price swings. The first swings after launch are **kill-swings** — pump then dump to near-death — designed to eat sniper/launch bots. As bots adapt (skip swing #1), devs add *more* kill-swings to eat the next bot generation. These kill-swings are characteristically **short in duration and deep (near-death) lows**. After draining enough bots, the dev shifts into a **volume-making phase**: longer, shallower swings (higher-lows) that attract *real* traders — then rugs on their own logic (fixed SOL accumulated, stall, etc.).

**The edge.** Enter on a confirmed reversal off a swing-low *during the volume-making phase* (after the kill gauntlet), ride the upswing, and exit before the next intentional kill / rug.

**The gap today.** The user already filters tokens by **dev creation-instruction fingerprint** (creator wallet, cu_limit/price, ix_labels, etc. — `trading_core/src/grouping.rs`). That static fingerprint is necessary but far too broad — it admits many unprofitable tokens. What's missing is a **dynamic behavioral classifier** that reads each token's *swing chain* and decides (a) has it transitioned from kill-phase to volume-phase, and (b) is now a confirmed-reversal entry. The whole strategy is one swing-leg classifier applied three times: to read history (find the transition), to trigger entry (volume-phase higher-low), and to detect exit (a leg reverting to the kill profile — **symmetric** to entry).

**Intended outcome.** A new `swing1` strategy, validated **offline first** on the Parquet lake (no ground-truth labels — sweep PnL/win-rate is the sole judge), then — only if an edge is proven — deployed live with an O(1)-per-trade incremental classifier.

## Key decisions (locked with user)

- **Name:** `swing1` — `StrategyImpl::Swing1`, strategy_id `"swing_1"`, module `trading_core/src/strategies/swing_1/`, params `Swing1Params`, sweep tables `swing_1_grouped_sweep_*`.
- **New registry variant, NOT an extension of tpsl2.** Keeps each strategy's swept-axis set focused; tpsl2 stays untouched. Reuse is by *calling* shared pure fns, not by sharing a params struct.
- **Price basis = GMGN canonical curve-spot.** See "Step 0" — this is a project-wide reconciliation, done first.
- **Kill→volume transition is count-free.** Detect the *shape change* (legs get longer + shallower). The floor `min_kills_before_volume` is a **swept** param (incl. 0), not a hardcoded count.
- **Entry = first confirmed higher-low in the volume phase, minimal lag.** Reuse `higher_low_confirmed_index` (tpsl2).
- **Exit watches:** next-kill-starting (same leg classifier, symmetric), liquidity/reserve crash (E4), stall/volume-death (E3).
- **Classifier uses CAUSAL per-leg gates only** — drops the analyzer's non-causal high/low *pair-drop* quality filter, so the live-incremental machine and the backtest-batch run produce identical legs (pinned by a parity test).
- **Sequencing = BACKTEST-FIRST.** Phase 1 builds the classifier + full sweep wiring and validates on the lake. Phase 2 adds the live incremental memo only if Phase 1 shows an edge.

## Architecture facts this builds on (verified)

- **Strategy registry:** `trading_core/src/strategies/registry.rs` — enum `StrategyImpl`, dispatch `from_id`/`id`/`parse_params`/`matches_entry`/`resolve_entry`/`resolve_exit`/`resolve_paper_exit`. Params structs → `params` JSONB on unified `StrategyRule`.
- **Swing analyzer (lab-only today):** `lab/src/analyzers/swing_analyzer.rs` — `detect_swings(&[Trade], &SwingParams) -> Vec<SwingLeg>`. `SwingLeg` carries `leg_type`, `duration_ms`, `start/end/pivot_end_price`, `net_flow`, `trade_count`. `scan()` (`:377`) is already a single forward state machine producing the pre-filter alternating ledger; `apply_quality_filter()` (`:530`) is the **non-causal** pair-drop we will NOT use for the classifier. Already prices via `curve_spot_price().unwrap_or(execution_price)` (`:354`) — i.e. GMGN spot.
- **Higher-low confirmation (reusable, already linearized):** `trading_core/src/strategies/tpsl_sniper_2/entry/scalp.rs:201` `higher_low_confirmed_index` — one-pass swing machine, monotonic in prefix length, generic over `TradeRow`.
- **Exit ladder (reusable, O(1)/trade memo):** `trading_core/src/strategies/tpsl_sniper_1/exit/mod.rs` — `ladder_reason` (`:306`), `ExitWalkState` (peak_price/last_higher_high_time/peak_reserves), `CachedExitState::advance` cursor pattern (`:200`). E3 stall + E4 liquidity already implemented (tpsl2's E4 uses *real* reserves).
- **Sweep engine:** `lab/src/sweep/strategy.rs` (`Strategy` trait: `entry_key`/`prepare_token`/`resolve_entry`/`resolve_exit`), `engine.rs` (entry resolved once per `EntryKey`, `TokenState` once per token), `registry.rs` (per-strategy tables, `run_grouped`, `simulate_one_combo`). `tpsl2.rs` is the template. `SweepTrade` (`projection.rs:29`) currently **drops** `virtual_token_reserves`/`real_token_reserves`.
- **Canonical price:** `Trade::chart_spot_price()` (`trade.rs:93`) = curve→pool→execution. Matches frontend `tradeSpotPriceSol()` (`chartBars.ts:69`) and the analyzer. `price_per_token`/`execution_price` is a *different* number (test `price_per_token_is_execution_not_curve_spot`, `trade.rs:260`).

---

## Step 0 — Reconcile the canonical price (isolated first commit)

Goal: one shared GMGN price fn, provably identical across chart, analyzer, and `swing1`, in both live and backtest.

1. Promote the GMGN spot precedence (`curve_spot → pool_spot → execution`) to a single shared helper over `TradeRow` in `trading_core` (the analyzer's inline `curve_spot_price().unwrap_or(execution_price)` and `Trade::chart_spot_price()` collapse into one definition). Live `Trade`/`CachedTrade` already carry the reserves.
2. **Add `virtual_token_reserves` and `real_token_reserves` to `SweepTrade`** (`lab/src/sweep/projection.rs` + the lake projection that fills it) and implement the `TradeRow` reserve accessors so the **backtest computes the same curve-spot as live + chart** (instead of silently falling back to execution price). Cost: ~+16 B/row corpus RAM — accepted as the price of parity on the 4GB box.
3. Pin with a test: same trades → identical price series across `Trade`, `CachedTrade`, `SweepTrade`.

Files: `trading_core/src/models/trade.rs`, `lab/src/sweep/projection.rs` (+ the lake row builder), `lab/src/analyzers/swing_analyzer.rs`. Keep existing swing tests green before any `swing1` work.

---

## Phase 1 — Classifier + sweep (backtest-first, validate the edge)

### 1a. Move the swing analyzer into `trading_core`, generic over `TradeRow`

Move `detect_swings` + `SwingLeg`/`SwingType`/`SwingParams` from `lab/src/analyzers/swing_analyzer.rs` to `trading_core/src/strategies/swing_1/swing.rs`, generic over `T: TradeRow`, pricing via the Step-0 shared GMGN fn. Lab's existing swing endpoint (`lab/src/api/handlers/tokens/swing.rs`) + `compute_chain_stats` switch to the moved generic version (`Trade: TradeRow`, mechanical). This single move gives both the sweep and (later) live access to the same analyzer.

### 1b. The phase classifier (causal, count-free)

Operate on the **raw alternating leg ledger** (`scan()` output), applying only per-leg *causal* quality gates inline at finalization. Per swing-LOW leg derive (all from `SwingLeg`): `depth_pct = (start_price - pivot_end_price)/start_price`, `duration_ms`, `net_flow_per_sec = net_flow/(duration_ms/1000)`, `trade_count`.

- `is_kill_low(leg)` ≡ `depth_pct ≥ kill_depth_min_pct` AND `duration_ms ≤ kill_max_duration_ms` (deep + short); optional `|net_flow_per_sec| ≥ kill_min_nf_per_sec`.
- `is_volume_low(leg)` ≡ `depth_pct ≤ vol_depth_max_pct` AND `duration_ms ≥ vol_min_duration_ms` AND preceding up-leg `duration_ms ≥ vol_min_up_duration_ms`.
- **Transition latch (count-free):** walking legs in time order, maintain `kills_seen`. Volume phase latches at the first swing-low `L` where `kills_seen ≥ min_kills_before_volume` AND `is_volume_low(L)` AND `L` is a higher-low vs the last kill low. Latch is sticky for entry.

### 1c. `Swing1Params` (all axes swept; R = reuse semantics, N = new)

- Exit ladder (R): `p_exit_take_profit`, `p_exit_stop_loss`, `p_exit_trailing_stop_pct` (E1), `p_exit_stall_secs` (E3), `p_exit_liquidity_drop_pct` (E4, real-reserves variant), `p_exit_time_stop_secs` (E2 backstop).
- Swing detection (N): `p_swing_high_to_low_sol`/`_pct`, `p_swing_low_to_high_sol`/`_pct`, `p_swing_min_leg_trades`.
- Kill profile (N): `p_kill_depth_min_pct`, `p_kill_max_duration_ms`, `p_kill_min_net_flow_per_sec`.
- Volume profile + transition (N): `p_vol_depth_max_pct`, `p_vol_min_duration_ms`, `p_vol_min_up_duration_ms`, `p_min_kills_before_volume`.
- Entry confirmation (R, reuse `higher_low_confirmed_index`): `p_entry_pullback_pct`, `p_entry_higher_low_secs`, `p_entry_max_age_secs` (armer/window ceiling). Optional guard `p_entry_min_liquidity_sol` (R).
- Symmetric next-kill exit (N): `p_exit_next_kill_depth_min_pct`, `p_exit_next_kill_max_duration_ms` — separate from entry `kill_*` so the sweep tunes "flee" thresholds independently.

### 1d. Registry + module wiring (`trading_core`)

- `swing_1/swing.rs` (analyzer + classifier), `swing_1/entry/mod.rs` (`find_phase_entry`: gate on latch → reuse `higher_low_confirmed_index` + worst-case fill), `swing_1/exit/mod.rs` (`find_trade_driven_exit` adding a top-priority **NextKill** `.or_else` arm before reusing E1/E3/E4), `swing_1/mod.rs`.
- `registry.rs`: `StrategyImpl::Swing1` + arms in `from_id`/`id`/`parse_params`/`matches_entry` (no creation gate — watch on trade stream)/`resolve_entry`/`resolve_exit`/`resolve_paper_exit`; `Swing1Params` + `StrategyParams::Swing1`; a dedicated `Swing1Rule` model (`models/strategy.rs`).
- `kernel.rs`: `ExitCode::NextKill` + `from_reason`/`exit_index`; widen `exit_counts` array and `RunMetrics.n_exit_next_kill` (this is the one cross-cutting schema touch).
- `rules.rs`: `validate_swing1` (percent bounds 0–100; require ≥1 swing/kill/volume axis configured; sanity `kill_depth_min_pct ≥ vol_depth_max_pct` and `vol_min_duration_ms ≥ kill_max_duration_ms`) + arms in `validate`/`params_to_value`.

### 1e. Sweep wiring (`lab`)

- `lab/src/sweep/strategies/swing1.rs` (mirror `tpsl2.rs`): `Swing1Params`/`Combo`/`EntryKey`/`TokenState`/`Axes`. **`EntryKey` includes the swing-reversal + kill/volume axes** (they move the entry); exit-only axes (TP/SL/trailing/stall/liq/next-kill) stay out so the engine reuses the per-EntryKey entry across the exit sub-grid. `prepare_token` precomputes only the param-independent ordered tx series; the swing scan runs once per `EntryKey` in `resolve_entry`. `order_for_entry_cache` stable-sorts by entry-key fields.
- `lab/src/sweep/strategies/mod.rs`: `pub mod swing1;`.
- `lab/src/sweep/registry.rs`: `SWING1_TABLES`, `tables_for`/`strategy_ids`, `run_grouped`→`sweep_swing1`, `simulate_one_combo`→`simulate_swing1_one_combo`, `sweep_base_rule_swing1`, `exit_label` NextKill.
- `lab/migrations/NNNN_swing1_grouped_sweep.sql`: `swing_1_grouped_sweep_{runs,groups,results,combos}` (mirror the tpsl2 tables) + `n_exit_next_kill` on run-metrics.

### 1f. Validate

Run grouped sweeps over the lake (group by dev fingerprint to test "within my fingerprinted candidate set"). Use **LHS / coarse→refine**, not full grids — the ~13 axes would blow `MAX_COMBOS` as a grid (default most axes to single `[None]` until the UI supplies values; cap recommended swept axes per run to ~6–8). Judge on `win_rate`, `expectancy_sol`, `profit_factor`, and the robust `score = mean − 1.64·σ/√n`. Inspect where winners cluster (esp. `min_kills_before_volume = 0`, which would mean the kill-phase gate adds no edge).

**Gate:** proceed to Phase 2 only if sweeps show a real edge after costs.

---

## Phase 2 — Live incremental deployment (only if Phase 1 proves edge)

### 2a. `IncrementalSwingState` — O(1)/trade memo

Mirror `CachedExitState`: carry the `scan()` machine locals (`phase`, `current_high`/`current_low` `LegAcc`s, `temp`, `frozen_threshold`, `prev_post_spot`) **plus** the classifier latch (`kills_seen`, `last_completed_low/high` summaries, `volume_phase_latched`) **plus** an absolute `consumed_abs` cursor. `advance(trades, trades_base)` folds only new trades (same cursor arithmetic as `CachedExitState::advance`), stepping one tx at a time; on each finalized leg, run the O(1) classifier predicates. Cost = O(new trades)/ping — same order as the existing exit memo. **Front-trim safety** via absolute cursor, identical to the proven pattern.

Own it **inside the `swing1` `CachedExitState`** (per-position, in `exit_state_by_position`) so one `advance` pass updates peaks + swing legs + evaluates the ladder incl. NextKill, and reuses the existing memo lifecycle (seed on first sight, drop on close, cap-bounded). No new runtime_cache map.

### 2b. Wire into the live service

`exit_state.rs`: `LadderParamsImpl::Swing1` + `CachedExitStateImpl::Swing1` (build/advance/clock arms). `live/src/strategies/service.rs` trade-gate loop (~`:488`) drives the memo; entry watch checks `volume_phase_latched && higher_low_confirmed`; the until-dead armer is bounded by `p_entry_max_age_secs` and `MAX_UNTIL_DEAD_ARMERS`.

### 2c. Frontend (minimal)

Rule-authoring UI for `swing1` params + sweep launch/results, following the existing per-strategy lab pages. (Detail deferred to Phase 2.)

---

## Risks & mitigations

1. **Batch↔incremental parity (highest).** The analyzer's `apply_quality_filter` pair-drop is non-causal. **Mitigation:** classifier runs on the unfiltered `scan()` ledger with causal per-leg gates only; pin with a property test (analog to `linearized_scalp_entry_matches_prefix_oracle` and `cached_state_advance_matches_full_rebuild`): identical trades → identical latch index + identical NextKill firing trade across batch vs incremental.
2. **Sweep-grid explosion.** ~13 axes. **Mitigation:** `[None]` defaults, LHS/refine over grids, minimal `EntryKey`, ~6–8 swept-axis cap in UI defaults.
3. **False entries from minimal confirmation lag.** **Mitigation:** `vol_min_up_duration_ms` (require real accumulation before the low) + fast NextKill exit; residual is a sweep-tuned lag/fill tradeoff judged by expectancy.
4. **RAM from widened `SweepTrade`.** ~+16 B/row. Accepted; the corpus is bounded by the lake export and the 7-day rolling buffer is server-side only (sweeps are local).
5. **Cross-crate analyzer move touches the lab swing API.** Mitigation: do Step 0 + 1a as the first isolated commits with existing swing tests green before any `swing1` logic.

## Verification

- **Backend:** `cargo check -p trading_core` + `cargo check -p lab` + `cargo check -p live` clean; clippy on touched files. New unit tests: classifier kill/volume/transition predicates on hand-built leg chains; the batch↔incremental parity property test; `validate_swing1` bounds. `cargo test -p trading_core` + `cargo test -p lab`.
- **Price parity test (Step 0):** same trades → identical GMGN price series across `Trade`/`CachedTrade`/`SweepTrade`.
- **Backtest end-to-end:** after `cargo run -p lab -- lake-export`, POST a `swing_1` grouped sweep (LHS) over a date window; confirm per-combo metrics populate and exit-reason mix includes `NextKill`. Eyeball a few matched tokens against their GMGN charts to sanity-check that latched entries land in the volume phase, not mid-kill.
- **Phase 2 only:** paper-mode `swing1` rule on `live` against the gRPC feed; confirm live entries/exits match a backtest of the same tokens (decision parity).

## Critical files

- `trading_core/src/models/trade.rs` (Step 0 shared price)
- `lab/src/sweep/projection.rs` + lake row builder (Step 0 `SweepTrade` reserves)
- `lab/src/analyzers/swing_analyzer.rs` → `trading_core/src/strategies/swing_1/swing.rs` (move + classifier)
- `trading_core/src/strategies/registry.rs`, `rules.rs`, `kernel.rs`, `exit_state.rs`, `models/strategy.rs`
- `trading_core/src/strategies/swing_1/{entry,exit,mod}.rs` (new)
- `lab/src/sweep/strategies/swing1.rs` (new, mirror `tpsl2.rs`), `lab/src/sweep/registry.rs`, `lab/migrations/NNNN_swing1_grouped_sweep.sql`
- Phase 2: `live/src/strategies/service.rs`, `trading_core/src/strategies/runtime_cache.rs`
