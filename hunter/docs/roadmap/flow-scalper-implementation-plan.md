# Flow scalper - implementation plan (metrics -> lifecycle -> sizing -> validation)

Goal: implement the "dip-reversion scalper" strategy reverse-engineered in
[flow-reversion-scalper.md](flow-reversion-scalper.md) (read that first for the WHY and
all target numbers) on the generic hunter engine, validate it against the observed
wallet-family distributions, and take it to paper trading. This file is the HOW: exact
seams, files, phases, and acceptance gates, self-contained enough to execute in a fresh
session.

Engine anatomy (so you don't re-explore):

| File | Role |
| --- | --- |
| `hunter/engine/src/metrics/mod.rs` | `REGISTRY` - SSOT of groups/metrics/units/tolerances/hues; `registry_json()` feeds the FE rule builder (new metrics auto-surface) |
| `hunter/engine/src/metrics/track.rs` | `TokenTrack` - per-token metric state; `value()` routes MetricId -> group; `ensure_window()` dedupes window buffers |
| `hunter/engine/src/metrics/price_lifetime.rs` | `stall`/`trail` incremental state (lifetime peak) - the model for the new windowed group |
| `hunter/engine/src/metrics/flow_window.rs` | `WindowState` - trailing flow window (the dynamic-group precedent) |
| `hunter/engine/src/rule_params.rs` | `RuleParams::parse` - registry-guided JSONB walk; validation lives here |
| `hunter/engine/src/arm.rs` | `CompiledRule` (pre-chewed rule, `MetricReq` reads, mono-kills), `ArmState` lifecycle |
| `hunter/engine/src/reduce.rs` | THE fold; `decide_arm` = entry gate + exit priority `Dead > SL > TP > Metrics` |
| `hunter/engine/src/state.rs` | `EngineState`, `TokenState { track, arms }`, `RuleCounters { open, total }` |
| `hunter/lab/src/strategies/replay.rs` + `engine_sim.rs` | simulate/replay - drives the REAL fold (inherits new metrics for free) |
| `hunter/lab/src/sweep/generic/{axes,strategy,exit_index,guard}.rs` | sweep scan - a PARALLEL impl of the fold (see gotchas): new metrics need explicit wiring here |
| `hunter/live/src/strategies/engine/` | live adapters (producers, exec, sinks) around the same fold |

Commands: `cargo check -p hunter-engine -p hunter-live -p hunter-lab`,
`cargo test -p hunter-engine`, `--target-dir "C:/Users/User/Documents/Bot/target-check"`
when a bin is running. FE: `npm run build:live` + `npm run lint`.

## Phase 0 - metric vocabulary audit (current metrics: keep / update / new)

Verdict on every existing metric vs what the strategy needs:

| Metric | Verdict | Use in this strategy |
| --- | --- | --- |
| `m_snapshot.time` | keep as-is | universe gate: `time >= ~120` |
| `m_snapshot.liquidity` | keep as-is | curve `reserve_sol` band: `liquidity` in ~[45, 110] |
| `m_price_lifetime.stall` | keep as-is | safety-net exit only (`stall >= 15`) |
| `m_price_lifetime.trail` | keep as-is, do NOT retrofit | lifetime-peak anchor is wrong for the dip trigger; see below |
| `m_flow_window.{gross_flow,net_flow,buy,sell}` | keep as-is | hot gate `gross_flow(30s) >= ~10`; later `net_flow(2s)` exhaustion refinement |
| `m_flow_split` / `m_flow_split_window` | untouched | orthogonal |
| `take_profit` / `stop_loss` params | keep as SUGAR, desugar in the fold (Ph2) | expand to `m_position.pnl` exit reqs; catastrophe stop = `pnl <= -25` |
| NEW `m_price_window.{trail,rise}` | Phase 1 | dip trigger: % below the ROLLING window high |
| NEW `m_position.{retrace,pnl,held}` | Phase 2 | trailing stop + the SSOT exit vocabulary (subsumes TP/SL) |

**Minimal core = exactly the 2 new metrics.** The irreducible, tradeable loop is two
conditions: entry `m_price_window(30s).trail >= 12` (buy the dip), exit
`m_position.retrace >= 3` (trail out). `retrace` doubles as the stop — at entry the
since-entry peak is the fill price, so if the dip keeps dipping it fires at ~-3% from
entry (soft stop); if price runs up first it becomes a trailing take-profit off the new
peak. One exit metric gives both behaviors, which is why no separate SL is needed for
the *mechanism*. Everything else below (time/liquidity/gross_flow gates, `pnl <= -25`
catastrophe stop) is UNIVERSE FILTERING — it keeps the logic off dead/illiquid/rugging
tokens; it is not part of the logic. Build and validate the 2-metric core first, then
layer the gates.

Why not add `window_size_sec` to `m_price_lifetime` instead of a new group: that flips the
group from Static to Dynamic, changing the JSONB shape and sweep axes of every stored
rule that uses `trail`/`stall` today. The registry already has the right precedent:
`m_flow_split` (lifetime) vs `m_flow_split_window` (windowed) are sibling groups sharing
metric NAMES with distinct `MetricId`s. Mirror that: `m_price_lifetime` (lifetime) vs
`m_price_window` (windowed).

TP/SL vs `m_position.pnl` — DESUGAR, don't keep two mechanisms. `take_profit: 100` IS
`m_position.pnl >= 100`; `stop_loss: 30` IS `pnl <= -30`. Two code paths deciding the
same exit is an SSOT violation. Resolution (implemented in Phase 2, because it needs the
position scope): the top-level `take_profit`/`stop_loss` fields STAY as authoring sugar
(so the sweep TP/SL axes, live UI, and all 106 stored rules keep working with NO DB
migration), but `CompiledRule::compile` expands them into `m_position.pnl` exit reqs and
the special-cased TP/SL branch in `reduce.rs::decide_arm` is DELETED. One evaluation
path. The SL-before-TP tiebreak vanishes for free — both are arms on the same number
(`pnl`), so they're mutually exclusive and can never both fire. See Phase 2 step 6.

## Phase 1 - `m_price_window` group (windowed price extrema)

New dynamic group, strict param `window_size_sec` (required), metrics:

- `trail` - percent the current price sits below the highest price of the trailing
  window: `(win_high - price) / win_high * 100`, >= 0. The dip trigger: entry condition
  `trail >= 12` with `window_size_sec: 30` means "12%+ below the 30s high".
- `rise` - percent above the trailing-window low (symmetric, near-free once the deque
  machinery exists; enables breakout/momentum entries later). Optional - drop if you
  want the minimal diff, but the second deque is ~20 lines.

Semantics: window covers `(now - W, now]` and INCLUDES the current trade. Empty window
(no trade for W seconds) => `NaN` (engine convention: NaN satisfies no condition, so a
flow-dead token cannot fire a dip entry - consistent and safe; the `gross_flow` hot
gate would exclude it anyway). Non-finite prices ignored (same as `PricePathState`).

Implementation:

1. `hunter/engine/src/metrics/price_window.rs` (new): `PriceWindowState` holding a
   monotonic deque of `(price, at)` - front = window max, evict front-expired on read
   or fold, pop back while `back.price <= new price` on push. O(1) amortized per trade,
   no per-event alloc after warmup (`VecDeque` reuse). Second increasing deque for
   `rise`. Floor eviction against block_time regression the same way `stall` floors at
   zero (corpus is slot-ordered; block_time can regress a few seconds - never let
   eviction panic or clear the window on a regressed timestamp; treat `at <= front.at`
   as in-window).
2. `metrics/mod.rs`: `MetricGroupId::PriceWindow` ("m_price_window", Dynamic, strict
   `window_size_sec`), `MetricId::{WinTrail, WinRise}` (JSON names "trail"/"rise" -
   name reuse across sibling groups is the established flow-group pattern), unit
   Percent, `eq_tolerance` 1.0, `monotonic: false`. Hues: see the hue gotcha below.
3. `metrics/track.rs`: `price_windows: BTreeMap<u64, PriceWindowState>` keyed by
   `window_key`, `ensure_price_window()`, fold in `on_trade` (push) + `on_tick`
   (evict), route `WinTrail | WinRise` in `value()`.
4. Window plumbing: `CompiledRule.windows` currently carries one undifferentiated
   union used to `ensure_window` (flow buffers). Split by group at compile time
   (`flow_windows` vs `price_windows`, or a tagged list) so a rule using only
   `m_flow_window(30)` does not pay for a price deque and vice versa. Update
   `EngineState::{all_windows, reload, new_track, ensure_track_windows_and_flow}`
   accordingly.
5. Nothing else changes by design: `rule_params.rs` parses/validates the new group via
   the registry walk automatically; `registry_json()` auto-surfaces it to the FE rule
   builder; replay/simulate inherit it because they drive the real fold. No DB
   migration (params are JSONB).
6. Tests (engine): deque max/min correctness vs a naive scan over random series;
   eviction on tick; NaN before first trade / after quiet gap; window dedupe;
   regressed-timestamp guard; a `rule_params` round-trip using the new group.

## Phase 2 - `m_position` group (position-scoped metrics - new metric class)

Everything in the registry today is token-scoped. This group's state anchors on YOUR
entry fill, so it introduces a scope concept - design it once, generically:

- `retrace` - percent below the highest price since entry fill: the trailing stop.
  Exit condition `retrace >= 3` = classic 3% trailing stop. >= 0, NaN before entry.
- `pnl` - signed percent vs entry price (`(price - entry) / entry * 100`).
- `held` - seconds since entry fill. (Gives time-stop exits for free.)

Static group (no window param), unit Percent/Percent/Seconds, tolerances 1.0/1.0/0.5,
`monotonic: false` (the flag powers ENTRY disarm; these are exit-only).

Implementation:

1. Registry: add `scope` to `GroupSpec` - `enum MetricScope { Token, Position }`,
   default Token for all existing groups. Mirror into `registry_json()` (`"scope":
   "position"`) so the FE builder can hide the group on the entry side without
   hardcoding names.
2. Validation (`rule_params.rs::validate_group` or a sibling check): reject
   position-scoped groups under `entry` with a clear message ("m_position metrics only
   exist while holding - exit side only"). Note `can_enter`'s "exit must not already
   hold" pre-entry gate needs no change: with no position context the metrics read NaN
   => never satisfied.
3. State (`arm.rs`): extend `ArmState::Entered` to
   `{ position, entry_price, entered_at: Ts, peak_price: f64 }` (peak initialized to
   the fill price). In `reduce.rs::evaluate_token`, before `decide_arm`, fold the
   current price into each Entered arm's `peak_price` (one `max` per armed rule per
   event - hot-path fine, no alloc). Adapters that construct `Entered` (live boot
   adoption, replay) seed `entered_at`/`peak_price` from the fill.
4. Evaluation routing: at compile time split `exit_reqs` into token-scoped and
   position-scoped lists (by `group_of(metric).scope`). `exit_metrics_fired` takes an
   optional `PositionCtx { entry_price, peak_price, entered_at }`; position reqs read
   from a tiny `position_value(metric, ctx, price, now)` helper, token reqs keep
   `track.value(..)`.
5. Exit priority collapses from `Dead > SL > TP > Metrics` to `Dead > Metrics`. Dead
   stays first and special (liquidity-based, not price). SL/TP become ordinary position
   metrics; the trailing stop and every price-based exit now flow through
   `ExitReason::Metrics` with the metric stamped.
6. DESUGAR TP/SL (the SSOT unification): in `CompiledRule::compile`, expand
   `rule.params.take_profit`/`stop_loss` into position-scoped exit reqs
   (`pnl >= tp` / `pnl <= -sl`) and PREPEND them so a catastrophe stop still evaluates
   before softer metric exits; DELETE the `entry_price * (1 ± x/100)` branch in
   `decide_arm`. Preserve the exit-reason labels: when a fired `pnl` req originated from
   the `take_profit`/`stop_loss` sugar, stamp `ExitReason::TakeProfit`/`StopLoss` instead
   of `Metrics{pnl}` (tag the `MetricReq` with its origin at compile time), so live UI /
   analytics that group by exit reason keep working. Trade OUTCOME is byte-identical to
   today; only the internal path changes. Keep the top-level fields in `RuleParams` and
   the FE — they are sugar now, not a second mechanism.
7. Tests: golden fold test - enter, price runs +30%, retrace 3% fires within one event
   of the 3% crossing and stamps `Metrics{retrace,>=,3}`; TP/SL desugaring produces
   byte-identical effects to the pre-refactor fold on the existing golden logs (the
   labels must still read `TakeProfit`/`StopLoss`); `held` time-stop; entry-side
   rejection of position groups; NaN-before-entry never fires; determinism.

Extensibility payoff: any future since-entry metric (PnL path, adverse excursion,
time-in-drawdown) is now just another `m_position` metric - no new machinery.

## Phase 3 - one-shot backtest validation (BEFORE lifecycle/sizing work)

Validate the two metrics reproduce the observed edge with the ONE-SHOT lifecycle that
already exists (enter once, trail out, done). Use simulate/replay (real fold - zero
extra wiring), not the sweep, for this phase.

> **DONE 2026-07-21 — results + verdict live in
> [flow-reversion-scalper.md](flow-reversion-scalper.md) "Phase 3 — one-shot backtest
> results".** Two premises below were stale and are corrected there: costs **are**
> modelled (`CostModel::pumpfun_default` ≈ 4%/round, applied by `round_trip_with_costs`
> — step 2 needs no new knob), and the sim already fills **worst-case adverse in the
> post-signal slot window** (step 3 fill realism is done). Outcome: the core is
> directionally sound but the one-shot is ~breakeven-before-costs / net-negative after,
> so per §4 this is a STOP-and-re-examine before Phase 4/5 — not a green light.
>
> **Lever #1 follow-up DONE 2026-07-22 — STOP verdict RECONFIRMED.** The single-
> `m_flow_window`-per-side schema limit is lifted: a side now carries **multiple windows
> per group** (`SideConditions` → `Vec<GroupConditions>`; dynamic groups parse as a JSON
> array of window clauses — engine-only, no DB migration; multi-window per group also
> unblocks any future strategy needing two windows on one group). Re-ran the exhaustion
> probe with BOTH the 30s `gross_flow` hot gate and the 2s `net_flow` floor in one rule:
> the result is **byte-identical to net-flow-only** — the 30s gross gate is non-binding on
> this universe. The earlier probe did not understate; flow-gating is not the missing
> edge. Details + table in the analysis doc's "Both gates together" subsection. **Phase
> 4/5 stay gated.**

1. Author two rules and compare. (a) MINIMAL CORE (proves the 2 metrics are the whole
   logic): broad fingerprint, entry `m_price_window(30).trail >= 12`, exit
   `m_position.retrace >= 3` — nothing else. Expect it to trade junk / knife-catch, but
   the winning episodes should already show the target shape. (b) GATED: add
   `m_snapshot.time >= 120`, `liquidity in [45,110]`, `m_flow_window(30).gross_flow >= 10`
   to entry; add catastrophe stop `m_position.pnl <= -25` and safety net
   `m_price_lifetime.stall >= 15` to exit. The delta between (a) and (b) quantifies how much
   the universe filter is worth. (Ranges: blueprint section of the analysis doc.)
2. Costs are NOT modeled in the sim kernel today (verified: no fee/haircut knob in
   `lab/src/strategies/`). The strategy's median round trip is ~0% - WITHOUT a cost
   model the backtest will overstate the edge by roughly the full ~2%/round (1%/side
   pump.fun fee + tip + impact). Add a per-side haircut to the one-shot validation:
   either a `fee_pct_per_side` knob on the sim summary kernel (preferred - realized
   pnl gets `(1 - fee)^2` applied at close), or at minimum apply it in the result
   analysis. Do not skip this.
3. Fill realism: sim fills at the signal price; omego lands same-slot but we react via
   the feed. Book fills at the NEXT trade's price after the signal (or add a
   configurable 1-trade/1-slot delay) for entries; exits likewise. If the sim already
   fills market-style on the next event, verify and document rather than re-add.
4. Acceptance gates (vs the analysis doc's distributions, on 8+ sealed lake days):
   - entry dip depth distribution of taken entries overlaps the family band
     (median in ~[-20%, -8%] vs 30s high);
   - hold median in ~[5s, 60s]; win rate 55-70% before costs;
   - POSITIVE total pnl after the 2%/round haircut on at least the busy-hour subset;
   - losses bounded: p10 episode pnl >= ~-25% (the SL is doing its job).
   If the one-shot variant is not clearly positive after costs, STOP and re-examine
   (entry refinement, exhaustion gate) before building re-entry/sizing on top.

## Phase 4 - re-entry lifecycle (re-arm after Done)

> **DONE 2026-07-22 (`strategy-redesign`, committed `2db03ba4` engine + `5eddd54e`
> validation/FE).** Gate met after the
> fill-sensitivity correction (see analysis doc): the honest `realFee` bottom line is
> positive under realistic fills, so re-entry now amplifies a positive per-episode edge.
> Shipped: `RuleParams.reentry { cooldown_sec, max_episodes_per_token }` (parse/validate/
> round-trip, absent ⇒ one-shot — no DB migration); `ArmState::Cooldown { until }` (a
> **parallel `TokenState.episodes: BTreeMap<RuleId,u32>`** carries the count — NOT the
> planned `ArmSlot` struct: that would force every `arms.insert` to carry episodes
> forward, a reset footgun; the map leaves all `ArmState` transitions untouched and dies
> with the token); close-path re-arm on **normal exits only** (TP/SL/Metrics — never
> Dead/Manual/Migrated); `evaluate_token` promotes Cooldown→Armed (trade/tick-driven,
> emits `ArmedChanged(Armed)`); Migrated disarms a cooling arm; `max_total` counts
> episodes (documented on `LoadedRule`); live boot seeds `episodes` from a batched
> `count_closed_by_rule_mint` over the boot-adopted mints. 6 new golden tests (re-arm
> timing, tick promotion, episode cap, Manual/Dead no-rearm, migration disarm) + the
> pre-existing one-shot goldens as the non-regression; `cargo check` + 113 sweep guards +
> engine/property green; clippy clean on touched code. Sweep wiring stays Phase 6 (the
> sweep is a parallel impl; re-entry validates via simulate/replay for now). **FE editor
> SHIPPED** (`5eddd54e`): re-entry toggle + cooldown/episode inputs in `RuleEditor.tsx`,
> `reentry` round-trips through `ruleParams.ts` (silent-strip regression test), validated
> in `validate.ts` — absent ⇒ one-shot, so legacy rules are unaffected.

`ArmState` is one-shot: `Done` is terminal per (token, rule), and the observed edge
depends on rapid re-entry (median gap ~31s, up to 31 episodes/token).

1. Rule config (params JSONB, no migration): optional
   `"reentry": { "cooldown_sec": 5, "max_episodes_per_token": 10 }`. Absent = today's
   one-shot behavior (backward compatible with every stored rule).
2. Engine: `token.arms` value becomes `ArmSlot { state: ArmState, episodes: u32 }`
   (mechanical refactor), or add episodes into a parallel map - prefer the struct. On
   position close with reentry configured and `episodes < max`: transition to a new
   `ArmState::Cooldown { until: Ts }` instead of `Done`; `evaluate_token` promotes
   `Cooldown -> Armed` when `now >= until` (trades/ticks both drive it - no new timer;
   NB `is_active()` must treat Cooldown as active so the token is not pruned).
3. Counters: `RuleCounters.open` already rolls correctly per episode (increment at
   entry, decrement at close). `total`/`max_total` now counts EPISODES, not tokens -
   document the semantic shift (it is the safer direction: caps fire sooner).
4. Live restart caveat: cooldown/episode state is in-RAM. After a restart the adapter
   can seed `episodes` from a one-time `COUNT(*)` of closed positions per (rule, mint)
   at boot (cheap, indexed) - do this to avoid over-trading a token after a crash.
   Cross-check the boot-adopt paths touched by the position-status crash-safety work
   (uncommitted on this branch) so re-arm does not fight BuySubmitted adoption.
5. Sinks/DB: each episode mints a fresh `PositionId` -> a new `strategy_positions`
   row; verify no UNIQUE (rule, mint) constraint assumes one position per token.
6. Tests: cooldown promotion timing; episode cap stops re-arm; one-shot rules
   unaffected (golden logs unchanged); restart-seed test at the adapter level.

## Phase 5 - liquidity-proportional sizing (pct of curve `reserve_sol`) — DEFERRED (future)

> **DEFERRED 2026-07-22 — not needed now.** Dynamic `buy_amount` (size as a pct of the
> curve SOL reserve) is parked as future work. Phase 6 runs on the fixed
> `buy_amount_lamports` path (unchanged default); the size sweep axis and the reserve-pct
> scale-up step are dropped from Phase 6 until this lands. Revisit after paper/real-SOL
> validation of the fixed-size strategy shows sizing is the next lever.

**Which reserve — read before implementing (this is the whole design decision).** The
sizing base is `TokenTrack::current_reserves()` = the metric `Liquidity` = the field
`reserve_sol`, which is the **curve VIRTUAL SOL reserve** (renamed from
`virtual_sol_reserves`; the same pair the canonical curve-spot `price` and the dead-token
verdict already read). It is deliberately NOT `real_sol`:
- **Impact, not exit-liquidity, is what sizing controls.** On pump.fun's constant-product
  curve, buy slippage ≈ `ΔSOL / virtual_sol_reserve`, so a fixed pct of `reserve_sol`
  yields ~constant expected slippage per entry — the actual goal of liquidity-proportional
  sizing. Real reserve (`virtual − ~30 SOL` early) understates liquidity on young pools and
  would OVERsize exactly where impact is worst.
- **`real_sol` is not on the hot path.** The engine track carries only `reserve_sol`; real
  reserves are reconstructed offline (`approx_real_sol_reserves`, lab load path). Sizing off
  real would need new hot-path plumbing or an RPC per entry — violates the hot-path budget
  and "spend Helius sparingly." If a future goal is instead bounding EXIT liquidity ("don't
  buy more than I can sell back out"), `real_sol` is the honest ceiling — but that is a
  different sizing philosophy (exit-risk cap, not impact cap) and needs a real-reserve value
  the engine doesn't carry today. Decide which one before building.

1. Rule config (params JSONB): optional
   `"sizing": { "pct_reserve_sol": 1.0, "min_sol": 0.4, "max_sol": 1.3 }` — `pct_reserve_sol`
   is percent of the curve virtual `reserve_sol` (see the decision note above). Absent =
   fixed `buy_amount_lamports` (unchanged default). Validation: pct > 0, 0 < min <= max.
2. Engine: at the `Enter` decision (`reduce.rs::apply_decision` / the decide tuple),
   compute `lamports = clamp(pct_reserve_sol/100 * track.current_reserves(), min, max)` and
   put it on `Effect::SubmitBuy`. `current_reserves()` is already on the track (= curve
   virtual `reserve_sol`); NaN reserves => fall back to fixed size.
3. Live: executor takes lamports from the effect already - verify no path re-reads the
   rule's fixed amount. Paper/sim: same effect, nothing extra.
4. FE: small editor for the sizing block on the rule form.

## Phase 6 - sweep support + grid + paper rollout

1. Sweep wiring (the sweep scan is a parallel impl - see gotchas): register the new
   metrics as sweep axes (`lab/src/sweep/generic/axes.rs`) and teach the columnar scan
   (`strategy.rs`) to compute windowed-high `trail` (same deque over the corpus
   series) and to resolve the trailing-stop exit (running max since entry until
   `retrace` crossing - same shape as the existing SL/TP first-crossing scan;
   `docs/roadmap/exit-index-plan.md`'s prefix-extrema design is the O(log n) upgrade
   if this becomes the bottleneck - `resolve_exit` already is the measured hot spot).
   Add parity tests: sweep result == replay result for identical params (extend
   `guard.rs`).
2. Grid (empirically supported ranges - analysis doc): dip `trail` {8,15,25}%, window
   {30,60}s, trailing `retrace` {1.5,3,5,10}%, cooldown {5,30}s, liquidity band edges,
   `gross_flow` threshold {5,10,20}. Rank net of the cost haircut, not gross. (Size stays
   the fixed `buy_amount_lamports`; the `size {0.5,1,2}%` of `reserve_sol` axis is deferred
   with Phase 5.)
3. Paper: run the best 2-3 configs as `TradeMode::Paper` rules live for some days;
   compare live-paper episode distributions to the sweep's (entry dip, hold, pnl) -
   divergence means feed/latency effects the backtest missed.
4. Real-SOL: tiny fixed size first (fixed `buy_amount_lamports`), busy hours only.
   (Scaling via `pct_reserve_sol` is deferred with Phase 5.) Watch the ~23% submit-fail rate
   omego shows - our retry policy
   (`MAX_ENTRY_ATTEMPTS`) may need a "reprice on retry" look before real money.

## Gotchas (will bite if skipped)

- **Sweep is a parallel impl of the fold.** Replay/simulate inherit engine changes for
  free; the generic sweep scan does NOT (see memory: sweep-sim SSOT divergence).
  Phases 1-5 deliberately validate via simulate; the sweep only matters at Phase 6.
  Never claim "backtested" from the sweep until the parity guard covers the new
  metrics.
- **Registry hue guard tests WILL fail** when you add two groups: the hue wheel is
  effectively full under `MIN_CROSS_GROUP_GAP=30` + `MIN_DIRECTION_GAP=35` (checked:
  no free slot exists). Intended fix: extend the sibling-family exemption (the
  flow_split/flow_window precedent in `distinct_groups_use_distinct_hues`) to a price
  family `{m_price_lifetime, m_price_window, m_position}` sharing the amber band 40-62
  (e.g. price_window 44/48, m_position 52/56/58). Three views of one price path -
  same rationale as the flow pair. Do NOT lower the gap constants.
- **Hot path budgets**: deques are O(1) amortized, but the per-Entered-arm peak fold
  runs per event - keep it a bare compare, no alloc, no iteration beyond armed rules.
  No DB/RPC anywhere in the fold (engine stays pure); zero new Helius calls anywhere
  in this plan.
- **block_time regression** (the `stall` floor precedent): never let window eviction
  or `held` go negative / clear state on a timestamp that regresses a few seconds.
- **NaN discipline**: NaN satisfies no condition - keep that invariant for every new
  read (pre-entry position metrics, empty price windows, NaN reserves in sizing).
- **Costs**: every ranking/acceptance in Phases 3+6 must be net of the ~2%/round
  haircut. Gross numbers on this strategy are noise.
- **Event-log compat**: new `TradeLite`/params fields must keep serde defaults so old
  event-log lines and stored rules still parse (existing pattern).

## Suggested commit slicing

One phase per commit on `strategy-redesign` (project convention): Ph1 metrics group,
Ph2 position group, Ph3 sim haircut + validation notes (results into the analysis
doc), Ph4 lifecycle, Ph5 sizing, Ph6 sweep wiring. `cargo check -p hunter-engine -p
hunter-live -p hunter-lab` + engine tests green per commit; FE builds only where FE
touched (Ph2 scope flag, Ph5 editor, Ph6 axes UI).

## Open decisions (defaults chosen; change here if you disagree)

- Metric names `m_price_window.{trail,rise}` and `m_position.{retrace,pnl,held}`
  (sibling-name reuse per the flow-group precedent).
- Empty price window reads NaN (not 0) - blocks dip entries on flow-dead tokens.
- TP/SL DESUGAR into `m_position.pnl` (one evaluation path); the top-level fields survive
  as authoring sugar, so no DB migration and the sweep/FE keep working. Outcome-identical;
  exit-reason labels preserved via origin tagging.
- The bot's LOGIC is the 2 new metrics alone; existing metrics are universe filtering.
  Validate the 2-metric core before adding gates.
- `max_total` counts episodes after Phase 4 (stricter, documented).
