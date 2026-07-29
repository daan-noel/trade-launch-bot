# Partial exits (tranched scale-out) - engine plan

> Roadmap plan for launch-crew-follower-analysis.md section 5.3. Status: SHIPPED
> (engine → exec → kernel → sweep → FE → manual partial; operator paper smoke /
> fs3-00 re-measure still pending).
> The measured motivation: fixed TP = -10..-18%/event (caps the tail that carries
> all EV), pure trailing gives back 25-30% of every winner; omego is net-positive
> ONLY via his ~19% runner tranche; the crews distribute into strength. Today
> every exit in `reduce.rs` closes 100% - there is no scale-out concept anywhere.
>
> **Why now (07-29):** the primary beneficiary is the currently-armed
> `fs3-00 dev13 base` (bare `retrace >= 7, arm_above_pct 2`) — the "gives back
> 25-30% of every winner" pattern, on a live paper rule. The crew-footprint rider
> in analysis §5.2 was shelved ("do not build the rider now"); its 77-vs-362
> split still motivates the *shape*, but do not read this plan as rider work.

## 0. Design principles (extensibility first)

1. **N ordered stages, not a hardcoded 2-tranche special case.** The authoring
   shape is a `Vec` of stages; "sell most into strength, trail a stub" is just
   the 1-stage instance. Sweeps can later search stage counts/sizes.
2. **Each stage reuses the FULL existing exit grammar** (`SideConditions` +
   per-stage TP sugar + `arm_above_pct`), compiled through the same
   `build_reqs`/`pnl_req` desugar. No parallel mini-language that can drift.
   This makes every exit family per-stage for free: fixed ROI (`take_profit` /
   `pnl`), time stop (`held`), trailing (`retrace`/`bounce` + `arm_above_pct`),
   flow/momentum fade (`m_flow_window.*`, `stall`, liquidity) - and any metric
   added later is automatically a legal stage trigger.
3. **Sizes are `bps of the INITIAL bag`**, not of the remainder - fractions
   compose without compounding drift, and the exec layer does exact integer
   token math. The final/global close is always "all remaining" so dust is swept.
4. **The portion travels on the existing `SubmitSell` effect**, as a new field -
   the same vocabulary later serves manual partial sells from the Console and
   (if ever wanted) scale-IN on the buy side. No new effect variant.
5. **Durable truth is a per-position fills ledger** (`position_fills`), not
   wider one-row columns - any number of legs, both sides, works for manual
   partials and future DCA without another migration.
6. **ONE decision kernel stays law.** Live-real, live-paper, simulate all get
   this by construction (it lands in `reduce`); the grouped-sweep gets a staged
   resolver + parity guard, recorded in `docs/plans/sweep/sim-parity.md`.

## 1. Semantics (the contract)

- Rule gains `scale_out: [ { sell_bps, conditions... }, ... ]` (ordered).
  Stages are one-shot and fire strictly in order: stage k is the only stage
  evaluated while `stage == k`. Firing sells `sell_bps` of the initial bag and
  advances to `k+1`.
- The existing global exits are UNCHANGED and always close **100% of the
  remainder**: `Dead` verdict, desugared `stop_loss`, the authored `exit` side,
  `Migrated`, `Manual`. Priority per event stays `Dead > SL > ... > stages`.
  This is the catastrophe path - a stub in a rug must not wait for its stage.
- After the last partial stage fires, the position continues under the global
  `exit` side - UNLESS the ladder ends with an optional **remainder stage**
  (`sell_bps` omitted => portion `All`): its conditions close the position
  outright with their own reason. This is how the stub gets a DIFFERENT exit
  policy than the pre-banking hold (e.g. trail 25% while full, tighten to 8%
  on the stub) - the global side alone is static across the whole hold and
  cannot express that. Stage progression = stepped/dynamic trailing for free.
  `sum(explicit sell_bps) <= 9900` enforced at parse - a remainder must exist
  for whatever closes it (remainder stage or global exit).
- `PositionCtx` (entry/peak/trough/entered_at) is NOT reset per stage - `pnl`,
  `retrace`, `held` keep anchoring on the original entry. (Per-stage anchors,
  e.g. `since_stage`, are a future metric-group extension, deliberately out of
  scope.)
- Validation (zero-as-unbound rules): `sell_bps` in `[1, 9900]`, stage
  conditions non-empty, `scale_out: []` folds to "not set" at the wire boundary
  (the `configured_labels` precedent). **Hard cap: at most 3 explicit stages
  (+ optional remainder)** at parse — fee cost (~1% notional/extra leg at 0.1 SOL)
  and in-flight-sell blindness (no new decision while a sell is pending; each
  stage multiplies that window) both argue against unbounded ladders. Raise only
  when measurement demands it.

## 1b. Status + multi buy/sell (re-entry × scale-out)

Re-entry already multiplies **episodes**; scale-out multiplies **legs inside one
episode**. They are orthogonal — do not merge them into one status concept.

### Locked grain (best solution)

```text
(token, rule) ── episode 1 ── strategy_positions row A
                  buy 1  +  sell legs (partial…partial…final)  → End
                  └── arm → Cooldown → Armed
             ── episode 2 ── strategy_positions row B   (fresh next_position())
                  buy 2  +  sell legs …                 → End
             ── …
```

| Axis | Unit of truth | Owns status? |
| --- | --- | --- |
| **Episode** (re-entry) | one `strategy_positions` row = one buy → fully flat | YES — today's machine |
| **Leg** (scale-out) | one `position_fills` row under that position | NO — parent stays open until flat |

This is already how re-entry works today (`rearm_after_close` only after `End`;
`boot_seed_episodes` COUNTs closed rows; each Enter mints a new `PositionId`).
Scale-out nests inside that grain: legs never open a new row, never bump the
episode counter, never call `rearm_after_close`.

**Rejected alternatives (and why):**

1. **One fat row that accumulates every re-entry buy+sell** — status becomes
   meaningless ("Holding" with 5 entries?), `CLOSED_PRED` / win-loss / per-episode
   PnL all break, and omego-style 31 episodes/mint is unreadable.
2. **New status `PartialExited` / `ScalingOut`** — FE partitions, attention lane,
   reaper predicates, and `status_partition_guard` all explode for no gain; a
   brief `ExitPending` flash already says "sell in flight."
3. **Treat each partial as `End` + immediate re-buy of the stub** — burns an
   extra fee round-trip, double-counts episodes, resets `PositionCtx` (peak/
   retrace), and fights the whole point of a trailed stub on the *same* entry.
4. **Per-stage re-entry** (re-arm while still holding a stub) — out of scope;
   concurrency / token-account / cap semantics become undefined. One open
   position per (token, rule) arm stays law.

### Status transitions (unchanged domain)

```text
BuySubmitted -> Holding
Holding      -> ExitPending   (any sell starts — partial OR full)
ExitPending  -> Holding       (PARTIAL fill confirmed: stage advances, bag remains)
ExitPending  -> End           (FULL / remainder fill confirmed — ONLY then may rearm)
ExitPending  -> ExitStuck | ExitUnconfirmed  (same as today; bag = remainder)
```

- FE open/closed partitions stay byte-identical. No new status chip / lane.
- Mid-ladder: row shows `Holding` + "banked X%" chip; brief `ExitPending` while
  a leg is in flight.
- Caps: `open` stays held until `End` (mid-ladder keeps its concurrency slot).
  `total` increments per Enter (per episode) — unchanged.
- Re-entry: `rearm_after_close` / episode++ / `Cooldown` fire **only** on the
  final `End`. Partial fill reasons are ledger-only; they do not qualify as a
  close. `reason_allows_reentry` still reads the **last** leg's reason.
- Boot: `count_closed_by_rule_mint` still seeds `episodes` — mid-ladder
  `Holding` rows are not closed, so a restart cannot inflate the budget. A
  mid-ladder adopted position resumes `stage`/`sold_*` from the row + ledger.

### Two layers of truth (within one episode)

| Layer | What it stores | Who reads it |
| --- | --- | --- |
| **`strategy_positions` (1 row / episode)** | Lifecycle status + entry snapshot + **running aggregates** (`sold_token_amount`, `exit_sol_lamports_total`, `scale_stage`) + on `End` the weighted-avg exit stamp so existing PnL SQL keeps working | Console list, `CLOSED_PRED`, portfolio summaries, win/loss, episode counts |
| **`position_fills` (N rows / episode)** | Every leg: entry + each sell — `seq, side, price, sol_lamports, token_amount, at, reason, stage` | Row dialog ledger, per-leg chart markers, multi-leg cost kernel, SSOT guard |

Per-leg **PnL% and hold time are never stored columns** — they are derived at
read time from the ledger + the position's entry:

```text
leg_pnl_pct     = (fill.price - entry_price) / entry_price * 100
leg_hold_secs   = epoch(fill.at - entry_time)
leg_pnl_sol     = fill.sol_lamports/1e9 - entry_sol * (fill.token_amount / entry_token_amount)
position_realized_sol (closed) = Σ exit_fill.sol_lamports - entry_lamports
position_mtm_sol (open)        = Σ banked exit_sol + mark * remaining_tokens - entry_sol
```

Same pattern as today's `strategy_position_pnl` view (derived, never stored) —
extended to N legs. The list row shows **position-level** numbers (banked SOL,
remaining MTM, overall %); the dialog shows the **leg table**.

### Aggregates on the position row (denormalized, guarded)

On every confirmed sell fill (partial or final) the sink:

1. Appends one `position_fills` row (`seq = max+1`).
2. Updates `sold_token_amount += leg.token_amount`,
   `exit_sol_lamports_total += leg.sol_lamports`, `scale_stage` from the delta.
3. Appends the tx sig into `exit_tx_signatures` (array grows — see index note).
4. On final close only: stamps `exit_price` = SOL-weighted average across exit
   legs, `exit_token_amount` = `sold_token_amount`, `exit_lamports` =
   `exit_sol_lamports_total`, `exit_time` = last leg, `exit_reason` = last leg's
   reason, `status = End`.

**SSOT guard (required):** a no-DB (or cheap-DB) test asserting
`exit_sol_lamports_total == Σ position_fills.sol_lamports` for sell legs and
`sold_token_amount == Σ sell token_amount` — same class of guard as the
fingerprint sentinel bugs. The ledger is authority; the aggregates are a cache.

### Index / unique-sig note (mig 0018 must fix)

`uq_strategy_positions_exit_sig0` unique-indexes `(exit_tx_signatures->>0)` for
real-mode double-sell safety. With N exit legs the array has N sigs; uniqueness
on element 0 alone is no longer enough (a later leg's sig could collide with
another position's first). Mig 0018: drop that index; uniqueness moves to
`position_fills` (`UNIQUE (tx_signature) WHERE side = 'sell' AND mode-real` via
join, or store `tx_signature` on the fill row with a partial unique). Entry-side
`uq_strategy_positions_entry_sig0` is unchanged (still one buy).

### What the FE shows

- List row (open, mid-ladder): status `Holding`, chip `70% banked @ +12%`,
  remaining MTM on the stub. Brief `ExitPending` flash while a leg is in flight.
- List row (closed): status `End`, position-level realized PnL (sum of legs),
  last `exit_reason` — existing columns still work.
- Row dialog: ledger table (seq / side / stage / price / pnl% / hold / sol / reason).
- Chart: one marker per exit leg (not one "exit" at the end).

## 2. Engine (`hunter/engine`)

- `rule_params.rs`: `RuleParams.scale_out: Option<Vec<ExitStage>>` where
  `ExitStage { sell_bps: Option<u16>, take_profit: Option<f64>, conditions: SideConditions }`
  (`sell_bps: None` = remainder/`All` stage). Parse + validate + `to_value`
  round-trip; JSONB stays the storage.
- `arm.rs`:
  - `CompiledRule.scale_out: Vec<CompiledStage { sell_bps: Option<u16>, reqs: Vec<MetricReq> }>`
    - per-stage TP desugars via the same `pnl_req` (origin `TakeProfit`), stage
    windows merged into `flow_windows`/`price_windows`.
  - `ArmState::Entered` gains `stage: u8, sold_bps: u16` (both 0 = legacy).
  - `ArmState::ExitPending` gains `portion: Portion` and, for a partial, the
    Entered snapshot to restore on fill (peak/trough/entry/entered_at/stage) -
    factor the Entered payload into an `EnteredCtx` struct shared by both
    variants instead of duplicating five fields.
  - `exit_fired` unchanged (global side); new `stage_fired(stage, track, ctx, now)`
    using the same req walk (`trailing_armed` included).
- `event.rs`:
    - `Portion { All, BpsOfInitial(u16) }` on `Effect::SubmitSell` and
      `Event::ManualClose`. `All` = today's semantics.
  - `PositionDelta` gains `stage: Option<u8>` + carries the partial fill with
    `status: Holding` (a partial exit is a Holding-preserving fill, NOT a new
    status - the status machine and its FE partitions stay untouched).
  - `ExitReason` unchanged; each fill records its own reason in the ledger, the
    terminal `End` keeps the last leg's reason.
- `reduce.rs`:
  - `ArmDecision::PartialExit { reason, sell_bps }` decided only from `Entered`
    when no global exit fired; apply -> `ExitPending { portion: BpsOfInitial }`
    + `SubmitSell` + `PositionUpdate(ExitPending, stage)`.
  - `FillConfirmed` on a partial `ExitPending` -> back to `Entered` with
    `stage+1`, `sold_bps += leg`, peak/trough resumed (NOT reseeded) - emit
    `PositionUpdate(Holding, fill, stage)`. Caps/`decrement_open`/re-entry
    untouched (position still open; `rearm_after_close` only on final close).
  - `FillFailed` on a partial: same retry ladder; on exhaust/`Fatal` the
    position goes `ExitStuck` exactly like a full-sell exhaust (bag = the
    remainder; the reaper's bag-based redrive already handles arbitrary
    amounts). `Unconfirmed` -> `ExitUnconfirmed` unchanged.
  - While any sell is in flight no new decision is made (today's rule) - a
    global exit that becomes true mid-partial fires on the next event after the
    fill resolves. Document; do not build in-flight escalation. This window is
    why stage count is capped in §1.
- Golden tests: stage fires partial + advances; global SL mid-ladder closes the
  remainder; last-stage-then-trail; partial fill-fail -> ExitStuck; re-entry
  only after final close; legacy rules byte-identical effects (regression).

## 3. Execution + persistence (`live`)

- `exec_real`: `Portion::BpsOfInitial(b)` -> `tokens = initial_token_amount * b / 10_000`
  clamped to the on-record remaining; sell that amount from the position's own
  `token_account`. `All` -> current full-bag path. `exec_paper`: synthesize the
  portion fill the same way.
- Mig 0018:
  - `position_fills(position_id, seq, side, price, sol_lamports BIGINT,
    token_amount BIGINT, at, reason, stage, tx_signature)` - append-only ledger,
    written by the sink for EVERY fill delta (entry included) so one query shows
    a position's whole leg history. Partial unique on `tx_signature` for real
    sells replaces `uq_strategy_positions_exit_sig0` (see §1b).
  - `strategy_positions`: add `sold_token_amount BIGINT NOT NULL DEFAULT 0` +
    `exit_sol_lamports_total BIGINT` aggregate (running sum) + `scale_stage
    SMALLINT NOT NULL DEFAULT 0`. On `End`, keep stamping the existing exit
    columns with the **weighted-average** exit price and the SOL total so every
    existing PnL query/report stays correct without a JOIN. `CLOSED_PRED`
    unchanged. Aggregate SSOT guard required (§1b).
  - Refresh `strategy_position_pnl` (or a sibling view) so closed realized PnL
    prefers `exit_sol_lamports_total - entry_lamports` when the aggregate is set.
- Sinks/SSE: partial delta -> ledger append + aggregate update + SSE (FE shows
  "banked X% @ +Y%"). Reaper: `heal_exit_pending_cleared`'s "net <= dust" check
  must become "net <= expected remaining is gone", i.e. compare against
  `initial - sold` rather than 0 - audit `find_bags_by_status` thresholds the
  same way (they are amount-based, so mostly fall out).

## 4. Cost/PnL kernel (`core::strategies::kernel`)

- `round_trip_with_costs` gains a multi-leg sibling: entry leg + N exit legs,
  each leg charged fee bps + fixed-per-leg + impact(leg_size / reserve_at_leg).
  Fixed cost scales with leg count - this is the real economic bound on stage
  count (at 0.1 SOL size a 2-leg exit adds ~1% of notional; surface it, don't
  hide it). `TokenOutcome` carries the exit legs (small vec of
  `(bps, price, reserve)`), aggregates (`RunAgg`/`RunSummary`) unchanged in
  shape - realized PnL is now a sum over legs.
- Simulate/replay: falls out (same engine + kernel). Verify the episode markers
  and chart exit markers render one marker per leg.

## 5. Grouped-sweep (`lab/src/sweep/generic`)

- Staged resolution: resolve stage-1 with the existing `ExitClass` fast paths
  from the entry tick; resume `resolve_exit` from that tick for stage-2 ... then
  the global side for the stub (the from-an-anchor machinery exists - re-entry
  episodes already do this). PnL through the multi-leg kernel fn (SSOT).
- Cost is x(stages+1) resolve calls ONLY for rules with `scale_out`; legacy
  combos pay nothing.
- Parity: new entries in `docs/plans/sweep/sim-parity.md` for any divergence
  (e.g. mid-flight-sell blindness does not exist in the sweep) + a
  `guard.rs` test folding one golden token through engine vs sweep with a
  2-stage rule and asserting identical legs.

## 6. Frontend + manual (last)

- Rule editor: a "Scale-out" section of rows (the row-based editor pattern):
  each row = sell % + its condition rows; global exit section unchanged.
- Console/position rows: sold-fraction chip (`70% banked`), ledger table in the
  row dialog, per-leg chart markers. Status chips/partitions untouched (§1b).
- Manual partial sell: `ManualClose { portion }` + a Console "Sell N%" control
  on Holding rows - pure reuse of the Portion plumbing. **DONE (07-29).**

## 7. Order of work / done criteria

1. Engine (params -> compile -> reduce -> golden tests) - `cargo test -p hunter-engine`. **DONE (07-29).**
2. Exec + mig 0018 + sinks/reaper audit + aggregate SSOT guard - paper smoke on
   live box. **DONE (07-29)** — Portion→token sizing (live/paper/replay/orphan),
   mig 0018 (`position_fills` + sold_*/scale_stage + drop exit_sig0 + pnl view),
   sink entry-vs-partial Holding + `record_sell_fill`, boot adopt resumes
   stage/sold, remaining-based orphan/reaper sizing, writer-owned aggregate
   guard. Paper smoke on the live box still pending operator run.
3. Kernel multi-leg + simulate verification. **DONE (07-29)** —
   `ExitLeg` + `round_trip_multi_leg` (single-exit wrapper unchanged), replay
   collects per-leg fills, `outcome_to_row` / chart markers render one exit
   per leg, fixed cost scales with leg count.
4. Sweep staged resolver + parity guard - then re-measure on **`fs3-00`** with a
   banked tranche (primary payoff). Cheap first shape to try: one partial into
   strength + remainder stage as a pure time-stop (`held >= N`, no trail on the
   stub) — direct probe of the open-at-cap vs trail-out split without searching
   trail widths. Crew-rider re-measure stays optional / later.
   **DONE (07-29)** — `resolve_exit_staged` (Dead > global > stage, multi-leg
   PnL via `round_trip_multi_leg`); scale-out forces scalar (no index/SIMD);
   D5/D6 recorded in `sim-parity.md`; guards
   `scan_matches_replay_scale_out_two_stage` + `…_global_sl_mid_ladder`.
   fs3-00 re-measure still pending operator run. **Pass-2 overlay (07-29, v3
   single ladder):** optional run-level `scale_out` (`ExitStage[][]` on the
   wire, FE always sends one user-authored ladder) + `scale_out_top_k`
   re-scores each group's top-K combos against that ladder PLUS each combo's
   own baseline after the cheap axes pass, independently per combo. FE authors
   the ladder via `ScaleOutBuilder` (Rule Editor's stage editor) as a
   hypothesis to test, not a grid of guesses to search — see
   `docs/roadmap/scale-out-sweep-overlay-plan.md`.
5. FE. **DONE (07-29)** — RuleParams `scale_out`/`ExitStage` + validate +
   ScaleOutBuilder in RuleEditor; summary chips; PositionResponse
   sold_*/scale_stage/sold_bps; `GET …/positions/{id}/fills` (live+lab);
   Console banked chip + SSE `sold_bps`; dialog ledger + per-leg chart markers.
6. Manual partial sell. **DONE (07-29)** — `ManualClose { portion }` (+
   LoggedEvent serde default `All`); live `?sell_bps=` → Portion; Console Holding
   Sell ALL + 25%/50% presets; CloseRule/CloseMode/orphan stay full-bag.

Explicitly out of scope (extension points left open, not built): per-stage
position metrics (`since_stage` anchors), scale-in/DCA buys, in-flight sell
escalation, per-stage re-entry, continuous dynamic trail (`trail_pct = f(pnl)`
as a new position metric or dynamic `arm_above_pct` - approximated arbitrarily
well today by adding stages, so build only if measurement demands it).
