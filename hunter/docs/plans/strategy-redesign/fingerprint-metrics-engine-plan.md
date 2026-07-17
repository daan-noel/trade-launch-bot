# Strategy redesign — Fingerprint + Metrics generic engine (implementation plan)

Status: **IN PROGRESS** (design settled 2026-07-16; architecture upgraded same day to the
deterministic event-log engine — all five "best solution" upgrades adopted).
**Phase 0 complete 2026-07-16** (plus 1.2 pulled forward). **Phase 1 complete
2026-07-16** (metrics framework: 1.1 registry JSON, 1.3–1.8). **Phase 2 complete
2026-07-16** (fingerprint matcher: `engine/src/fingerprint.rs`, two-phase
first-slot, 11 tests). **Phase 3 complete 2026-07-16** (the engine fold: `event.rs`
+ `state.rs` + `arm.rs` + `reduce.rs` + `deadness.rs` moved from core; 20 golden-log
tests + a seeded-fuzz property test). **Phase 4 built 2026-07-16** (live adapters:
`live/src/strategies/engine/` = `decision_loop` + `producers` + `sinks` + `exec_paper`
+ `exec_real` + `event_log` + `reapers` + `convert`; new engine loop wired into
`main.rs` over the same `strategy_rx` ping channel (old `StrategyRunner` retired from
the runtime; legacy `StrategyService` kept for compile only, reapers NOT spawned);
CRUD/registry/armed HTTP on the live bin; two new `SseEvent` variants; `EVENT_LOG_*`
env; `cargo check` clean on live+lab, clippy-clean on new code. **Runtime paper smoke
(4.9) still pending** — needs the live stack + a matching fresh token.). **Phase 5
part 1 built 2026-07-17** (analysis-simulate cluster: 5.1 `lab/src/strategies/replay.rs`
global-ordered replay driver + sim fills + results collector; 5.2 generic
`POST /api/strategies/simulate` over replay; 5.3 parity by construction — converters
hoisted to `core::strategies::fingerprint_axes` SSOT + engine `TICK_MS` SSOT + guard
tests; 5.7 metric-series endpoint; `cargo check`/tests green on all four bins).
**Phase 5 part 2 built 2026-07-17** (precompute-then-scan sweep: 5.4 generic axes
model + `GenericSweepStrategy` reusing the `Strategy` trait / `grouped_engine`
partition + persistence, `grouped_sweep_*` migration 0003, `"generic"` registry
wiring; 5.5 scan ≡ `run_replay` guard test; 5.6 `promote_group` endpoint.
**5.8 verified end-to-end** on the live lab bin + real lake corpus, after fixing an
`0004` core-migration boot bug — see 5.8). Phase 7 deletion: the **5.4–5.6
sweep-rewrite blocker is cleared** (a generic sweep now exists), but the legacy
sweep files + per-strategy rule handlers + live `StrategyService` (Phase 7
blockers 2–4) still need retiring before the deletion sweep. Frontend FE0–FE6
shipped (deferred polish + runtime smoke share the 4.9 gate).
Scope: **hunter only** — forge untouched.
Origin: `Bot/docs/strategy-redesign-new-plan.md` + design Q&A; params shape example in
`Bot/docs/strategy-redesign-answer-1.md`.

## 0. What this replaces

The named strategies **tpsl_sniper_1 / tpsl_sniper_2 / swing_1 disappear entirely**.
One generic engine remains: a **rule = fingerprint reference + metric conditions**, with
user-chosen operators instead of operators hardcoded in Rust `check_*` fns.

| Today | After |
| --- | --- |
| `StrategyImpl` enum dispatch (`hunter/core/src/strategies/registry.rs`) | one generic engine, no per-strategy variants |
| fingerprint criteria cloned per strategy in `*/entry/mod.rs` `check_*` fns | one matcher over DB `fingerprints` rows |
| hardcoded operators (`min_age_sec` ⇒ `>`, `max_age_sec` ⇒ `<`) | `{operator, value}` lists per metric, all AND-ed |
| exit ladders `*/exit/mod.rs` (`run_exit_walk`, `LadderParams`) | TP `OR` SL `OR` exit-metrics-group |
| `StrategyService` methods + paper/real branches + fill-poll tasks | pure `reduce(state, event) → effects` + thin adapters |
| lab `Strategy`/`ParamSpace` wrappers per strategy | one replay producer + precompute-then-scan sweep |

## 1. Settled design decisions (contract — do not re-litigate)

1. **Fingerprint** is a DB row shared by many rules. Exact-match fields: `cu_limit`,
   `cu_price`, `ix_labels` (exact **ordered** sequence). Bucket-matched fields (via the
   row's own `bucket_size_amount`, SSOT `grouping::same_bucket`): `init_buy_amount`,
   `max_sol_cost`, `spendable_sol_in`, `first_slot_buy` (**sum** of buy SOL in creation
   slot), `first_slot_sell` (sum of sell SOL). A token can match **multiple** fingerprints.
2. **Metrics** live in their own module tree, one file per group, self-describing
   (name, unit, equality tolerance, compute logic). Adding a metric = one file, no schema change.
   - **Static** (rule-independent, one value per token): `m_snapshot` → `time` (sec since
     creation), `liquidity` (SOL reserves); `m_price_path` → `stall` (sec since price last
     moved), `trail` (% off peak).
   - **Dynamic** (needs per-rule strict params): `m_time_window(window_size_sec)` →
     `gross_flow`, `net_flow`, `buy`, `sell` (SOL over trailing window).
3. **Rule storage**: columns say *how* it trades — `fingerprint_id` FK, `is_active`,
   `trade_mode` (paper|real), `buy_amount_lamports BIGINT`, `max_concurrent_tokens`,
   `max_total_tokens`. `params JSONB` says *when* — strict `take_profit`/`stop_loss` +
   `entry`/`exit`, each holding metric groups; groups hold strict params
   (e.g. `window_size_sec`) beside per-metric `{operator, value}` lists
   (shape: `Bot/docs/strategy-redesign-answer-1.md`).
4. **Operators**: `>` `>=` `<` `<=` `=` `!=`. Within one metric, conditions form
   **DNF**: `,` **AND**s inside an arm (ranges, e.g. `10 < stall < 30`); `|` **OR**s
   across arms (e.g. `liquidity < 30 | liquidity >= 70`). A single AND arm that is
   unsatisfiable (same metric, crossed ops — `< 30, >= 70`) is **normalized to OR**
   at parse/input (sweep assemble, rule save, condition editor) so every metric
   input path treats same-field multi-op logically. Across *different* metrics the
   combinator is **side-dependent**: **entry ANDs**; **exit ORs** (any one satisfied
   metric fires the sell — asymmetric with TP/SL/dead). `=` is bucket-equality using
   the **metric's own default tolerance** declared in its metric file (time/stall
   0.5 s, trail 1 %, SOL metrics 0.1 SOL) — deliberately independent of the
   fingerprint's `bucket_size_amount`. Absent group/metric/TP/SL = unconstrained.
5. **Lifecycle**: armed per **(token, rule)**. Evaluation on **every trade AND every
   500 ms clock tick** (tick sized to ~400 ms slot latency; entry and exit both fire on
   ticks). Exit = TP hit `OR` SL hit `OR` any exit metric true. Disarm on dead-token
   verdict, migration, or **derived unsatisfiability** (entry upper bound on a monotonic
   metric permanently crossed, e.g. `time < 30` at 30 s). After exit a token is **done
   forever for that rule**; concurrent positions across rules are allowed. (Both have
   future-toggle designs in §9 — not built now.)
6. **Parity**: identical decisions on identical data across live paper, live real,
   simulate/backtest, and sweep — guaranteed **by construction** via decisions 8–12.
7. Old rules are **not migrated** — params vocabularies are incompatible. Legacy table is
   renamed and kept for reference.

**Architecture upgrades (adopted 2026-07-16):**

8. **Deterministic event-log engine.** The engine is one pure fold —
   `reduce(&mut EngineState, Event) -> Vec<Effect>` — over one ordered event stream
   (`TokenCreated | Trade | Tick | FillConfirmed | FillFailed | Migrated | RulesReloaded |
   ManualClose`). Live, replay, simulate, and sweep differ **only** in who produces events
   and who consumes effects. No decision code exists outside the fold.
9. **Purity enforced by the crate graph.** The engine lives in a new crate
   `hunter/engine` whose dependency list contains no tokio, no sqlx, no chrono-now, no
   rand — impurity is a compile error, not a review comment. `grouping` (bucket math) and
   `is_dead_verdict` move into it (re-exported from `trading_core` during transition).
10. **Fills are events, not polls.** The executor adapter submits and returns; fill
    confirmation arrives from the ingest trade feed as `FillConfirmed` (our wallet's trade
    in the stream we already consume). One **confirmation watchdog** provides the fallback:
    no feed fill within N slots ⇒ a single off-hot-path RPC cross-check ⇒ `FillConfirmed`
    or `FillFailed`. This deletes `spawn_entry_fill_poll` / `buy_until_filled_or_give_up`
    loops AND closes the audited sell feed-confirm gap (stranded Holding / phantom
    re-sell) in one component. Sell-confirm stays feed-first (hunter perf budget intact).
11. **One serialized decision loop — deliberately NOT mint-sharded.** Determinism is what
    makes 8–10 work; the deploy box is 2 vCPU; per-event work is microseconds over armed
    tokens only. Mint-sharding is the documented future scale path (events are already
    keyed by mint) — do not build it now.
12. **Event-log recording.** Live appends every engine event to a cheap local log
    (rotated daily, bounded). Any live decision is reproducible offline by replaying the
    log ("time-travel debugging"), and boot recovery rebuilds armed state by replaying the
    recent log (bounded by `MAX_SNIPE_AGE_SECS`). Positions in PG stay authoritative;
    replay reconciles against them.
13. **Sweep = precompute-then-scan.** Per token per sweep: one replay pass emits metric
    *series* (static + one per distinct `window_size_sec` in the axes). Per combo: a cheap
    scan over the precomputed series using the **same evaluator fns**. A guard test
    asserts full-engine replay ≡ scan on a sample corpus, so the optimization can never
    silently drift.

## 2. Workflows at a glance

### 2.1 Data model

```
┌─────────────────────────┐         ┌──────────────────────────────────────────┐
│  fingerprints (table)   │         │  strategy_rules (table)                  │
│─────────────────────────│         │──────────────────────────────────────────│
│ id                      │◄────────│ fingerprint_id (FK)                      │
│ cu_limit      (exact)   │  many   │ is_active            ─┐                  │
│ cu_price      (exact)   │  rules  │ trade_mode  paper/real │ columns:        │
│ ix_labels     (exact,   │  per    │ buy_amount_lamports    │ "HOW it trades" │
│                ordered) │  fp     │ max_concurrent_tokens  │                 │
│ init_buy_amount    ─┐   │         │ max_total_tokens      ─┘                 │
│ max_sol_cost        │ bucket-     │ params JSONB: "WHEN it trades"           │
│ spendable_sol_in    │ matched     │  ├ take_profit / stop_loss (strict)      │
│ first_slot_buy      │ by row's    │  ├ entry ┐  m_snapshot   {metric:[{op,v}]}│
│ first_slot_sell    ─┘ width       │  └ exit  ┘  m_price_path {metric:[{op,v}]}│
│ bucket_size_amount      │         │             m_time_window{window_size_sec │
└─────────────────────────┘         │                          + metric:[{op,v}]}│
                                    └──────────────────────────────────────────┘
         metrics module (code, extensible — one file per group)
         ├─ snapshot.rs    time, liquidity          ── static (per token)
         ├─ price_path.rs  stall, trail             ── static (per token)
         └─ time_window.rs gross/net_flow, buy, sell── dynamic (per rule params)
            each file: name · unit · eq-tolerance · compute logic
```

### 2.2 Token lifecycle

```
 token created
      │
      ▼
┌───────────────────┐   no match
│ fingerprint match │──────────────▶ ignored
│ (can match MANY)  │
└───────────────────┘
      │ match
      ▼
 ARMED  per (token, rule) — one arming for each rule on each matched fingerprint
      │
      │◄─────────────── every trade  AND  every 500ms tick ───────────────┐
      ▼                                                                   │
┌────────────────────────────┐                                            │
│ recompute metrics          │                                            │
│ evaluate rule conditions   │────────── no match ────────────────────────┘
└────────────────────────────┘
      │                        ┌──────────────────────────────────────────┐
      │ disarm checks          │ DISARM (token,rule) when:                │
      ├───────────────────────▶│  · dead-token verdict                    │
      │                        │  · migration                             │
      │                        │  · derived unsatisfiable: entry upper    │
      │                        │    bound on monotonic metric crossed     │
      ▼                        └──────────────────────────────────────────┘
 ALL entry conditions true (AND)
      │
      ▼
 ENTER ── buy_amount_lamports ──▶ OPEN POSITION   (concurrent across rules OK*)
      │
      │◄─────────────── every trade  AND  every 500ms tick ──────────────┐
      ▼                                                                  │
 exit check:  TP hit   OR   SL hit   OR   any exit metric true ── no ───┘
      │ yes
      ▼
 EXIT ──▶ CLOSED ──▶ token done forever for this rule*

 * future toggles (§9): re-entry after exit · single-position-per-token
```

### 2.3 Per-event evaluation pipeline

```
        trade event ──────┐                ┌────── 500ms clock tick
                          ▼                ▼
              ┌─────────────────────────────────┐
              │        tracked token state      │
              └─────────────────────────────────┘
                          │
          ┌───────────────┴────────────────────────────┐
          ▼                                            ▼
  STATIC metrics — computed ONCE per token     DYNAMIC metrics — computed per
  shared by all armed rules                    distinct rule params (deduped:
  · m_snapshot:   time, liquidity              rules sharing window_size_sec=10
  · m_price_path: stall, trail                 share one computation)
                                               · m_time_window(window_size_sec):
                                                 gross_flow, net_flow, buy, sell
          └───────────────┬────────────────────────────┘
                          ▼
        for each armed rule / open position on this token:
        ┌──────────────────────────────────────────────┐
        │ AND-evaluate {operator,value} lists           │
        │  operators: > >= < <= = !=                    │
        │  "=" uses the metric's own eq-tolerance       │
        └──────────────────────────────────────────────┘
                          │
             armed ──▶ entry decision      open ──▶ exit decision
```

### 2.4 The event-log engine — everything is a replay

```
                                THE ENGINE (hunter/engine — pure, deterministic)
                     ┌────────────────────────────────────────────────┐
   ordered events    │  reduce(state, event) -> effects               │   effects
  ───────────────▶   │                                                │ ───────────▶
                     │  state:  TokenTrack (metrics)                  │
   TokenCreated      │          ArmState per (token, rule)            │  SubmitBuy{intent,rule,mint,lamports}
   Trade             │          open positions + rule counters        │  SubmitSell{intent,position,reason}
   Tick(now)         │  logic:  fingerprint match → arm               │  PositionUpdate (persist + SSE)
   FillConfirmed     │          metrics update → entry/exit/disarm    │  ArmedChanged   (SSE)
   FillFailed        │          Dead > SL > TP > Metrics priority     │
   Migrated          │  NO clock · NO DB · NO tokio · NO randomness   │
   RulesReloaded     │  (enforced by the crate's dependency list)     │
   ManualClose       └────────────────────────────────────────────────┘
        ▲                                                                    │
        │ producers                                              consumers   ▼
 ┌──────┴──────────────────────────┐            ┌────────────────────────────────────┐
 │ LIVE:   ingest feed → events    │            │ LIVE:   executor adapter (real) or │
 │         tokio 500ms → Tick      │            │         paper fill model →         │
 │         confirmation watchdog → │            │         FillConfirmed back in;     │
 │         FillConfirmed/Failed    │            │         PG position writer; SSE;   │
 │         event-log RECORDER ─────┼──▶ disk    │         confirmation watchdog      │
 │ REPLAY: lake trades → events    │            │ REPLAY: sim fill model →           │
 │         synthetic Ticks between │            │         FillConfirmed inline;      │
 │         timestamps up to as_of  │            │         results collector          │
 │ BOOT:   recent log → events     │            │ (adapters live in hunter-live /    │
 │         (armed-state recovery)  │            │  hunter-lab, never in the engine)  │
 └─────────────────────────────────┘            └────────────────────────────────────┘
```

### 2.5 Crate graph — purity as a compile guarantee

```
                    ┌────────────────────────────────────────────┐
                    │  hunter/engine  (NEW crate, lib hunter_engine)
                    │  deps: serde, smallvec, chrono(no-clock use)│
                    │  — NO tokio, NO sqlx, NO rand               │
                    │  metrics/ · fingerprint match · evaluator   │
                    │  · rule_params · reduce() · ArmState        │
                    │  · grouping (bucket SSOT) · deadness        │
                    └───────────────┬────────────────────────────┘
                          ▲         ▲                ▲
                ┌─────────┘         │                └──────────┐
        hunter/core            hunter/live                hunter/lab
        (models, repos,        (adapters: ingest→events,  (adapters: lake→events,
         SSE bridge, HTTP)      tick, executor+watchdog,   replay, sweep scan, sim)
                                PG/SSE sinks, log recorder)
```

### 2.6 Sweep — precompute-then-scan

```
 per token (ONCE per sweep):                     per combo (thousands, cheap):
 ┌─────────────────────────────────────┐        ┌────────────────────────────────┐
 │ replay trades+ticks → metric SERIES │        │ scan the precomputed series    │
 │  static: time[] liquidity[] stall[] │  ───▶  │  entry = first idx where ALL   │
 │          trail[]     (per event)    │        │          entry conditions true │
 │  dynamic: per DISTINCT window in    │        │  exit  = first later idx where │
 │           the axes: flows[]         │        │          TP/SL/exit-conds true │
 └─────────────────────────────────────┘        │  (same evaluator fns as engine)│
   groups partition (group_key) unchanged       └────────────────────────────────┘
   GUARD TEST: full-engine replay ≡ scan on a sample corpus
```

## 3. Target structure

```
hunter/engine/                        NEW crate (pkg hunter-engine, lib hunter_engine)
├── Cargo.toml                        deps: serde, serde_json, smallvec, chrono (types only)
└── src/
    ├── lib.rs
    ├── event.rs                      Event, Effect, IntentId (deterministic: (rule, mint, seq))
    ├── reduce.rs                     reduce(&mut EngineState, Event) -> SmallVec<Effect>
    ├── state.rs                      EngineState: tracks, arm states, positions view, counters
    ├── arm.rs                        ArmState machine + derived-unsatisfiability precompute
    ├── fingerprint.rs                Fingerprint + TokenFingerprint matcher (multi-match)
    ├── rule_params.rs                RuleParams serde + validation (§5)
    ├── grouping.rs                   MOVED from core — bucket SSOT (same_bucket/bucket_index/group_key)
    ├── deadness.rs                   MOVED is_dead_verdict + death-point logic (SSOT)
    └── metrics/
        ├── mod.rs                    registry: groups → metrics → unit/tolerance/kind/monotonic
        ├── evaluator.rs              Operator, Condition, eval fns (shared with sweep scan)
        ├── track.rs                  TokenTrack: on_trade/on_tick, deduped dynamic states
        ├── series.rs                 MetricSeries emit (sweep precompute) — same compute code
        ├── snapshot.rs               time, liquidity                 (static)
        ├── price_path.rs             stall, trail                    (static)
        └── time_window.rs            gross_flow, net_flow, buy, sell (dynamic)

hunter/core/                          keeps: models, repos, SSE bridge, HTTP framework
├── src/models/fingerprint.rs         NEW DB row model (lamports at rest ↔ SOL accessors)
├── src/models/strategy.rs            StrategyRule reworked (fingerprint_id, lamports)
├── src/storage/repositories/…        FingerprintRepo NEW; StrategyRepo retargeted
└── (re-exports hunter_engine::grouping etc. during transition)

hunter/live/src/strategies/           REWRITTEN as adapters around the engine
├── loop.rs                           THE one serialized decision loop (select!)
├── producers.rs                      ingest→Event bridge, 500ms Tick producer, first-slot gate
├── exec_real.rs                      SubmitBuy/Sell → executor; feed-confirm; watchdog
├── exec_paper.rs                     worst-case fill model → FillConfirmed
├── sinks.rs                          PositionUpdate→PG writer, ArmedChanged/status→SSE
├── event_log.rs                      recorder (append, rotate) + boot replay recovery
└── http.rs                           rules/fingerprints CRUD, registry + armed endpoints

hunter/lab/src/strategies/            analysis adapters
├── replay.rs                         lake→events producer (+synthetic ticks, as_of), sim fills
└── (sweep scan lives in hunter/lab/src/sweep/)
```

**Deleted at the end (Phase 7):** `core/src/strategies/{tpsl_sniper_1,tpsl_sniper_2,swing_1}/`,
`registry.rs` (`StrategyImpl`), `runtime_cache.rs` strategy memos + `exit_state.rs`,
`models/{tpsl1,tpsl2,swing1}_strategy_rule.rs`, `live/src/strategies/{service,runner,
execution/*}.rs` (replaced by the adapter files above),
`lab/src/sweep/strategies/{tpsl1,tpsl2,swing1}.rs`,
`lab/src/strategies/{tpsl_sniper_1,tpsl_sniper_2,swing_1}/`.

## 4. DB schema (migration `hunter/core/migrations/0004_strategy_redesign.sql`)

```sql
-- 1. fingerprints (shared by many rules)
CREATE TABLE fingerprints (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name                TEXT NOT NULL,
    cu_limit            BIGINT,            -- exact match; NULL = not part of identity
    cu_price            BIGINT,            -- exact match
    init_buy_lamports        BIGINT,       -- bucket-matched ┐
    max_cost_lamports        BIGINT,       --                │ all via this row's
    spendable_lamports_in    BIGINT,       --                │ bucket_size_amount
    first_slot_buy_lamports  BIGINT,       -- sum in slot    │
    first_slot_sell_lamports BIGINT,       -- sum in slot    ┘
    bucket_size_amount  DOUBLE PRECISION NOT NULL DEFAULT 0.1,  -- SOL width
    ix_labels           TEXT[],            -- exact ordered sequence
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 2. legacy rules kept read-only for reference (params vocab incompatible)
ALTER TABLE strategy_rules RENAME TO strategy_rules_legacy;

-- 3. new rules table
CREATE TABLE strategy_rules (
    id                    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    rule_name             TEXT NOT NULL,
    fingerprint_id        UUID NOT NULL REFERENCES fingerprints(id),
    trade_mode            TEXT NOT NULL CHECK (trade_mode IN ('paper','real')),
    is_active             BOOLEAN NOT NULL DEFAULT false,
    buy_amount_lamports   BIGINT NOT NULL,
    max_concurrent_tokens BIGINT NOT NULL DEFAULT 1,
    max_total_tokens      BIGINT NOT NULL DEFAULT 0,      -- 0 = unlimited
    params                JSONB NOT NULL,                  -- TP/SL + entry/exit
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_strategy_rules_active ON strategy_rules (is_active, trade_mode);
CREATE INDEX idx_strategy_rules_fingerprint ON strategy_rules (fingerprint_id);
```

Notes:
- Lamports `BIGINT` at rest, SOL `f64` in models — project unit convention
  (`sol-lamports-naming` rule). Fingerprint UI speaks SOL; models convert.
- `strategy_positions` keeps its lifecycle
  (`Arming→BuySubmitted→Holding→ExitPending→End/ExitFailed/ExitUnconfirmed`) but
  `strategy_id TEXT` loses meaning → repurpose to constant `'generic'` in this migration;
  drop in a later cleanup migration once nothing reads it.
- `exit_reason` vocabulary becomes: `TakeProfit | StopLoss | Metrics | Dead | Manual | Migrated`.
- The **event log is NOT a DB table** — it's a rotated local append-only file
  (`$EVENT_LOG_DIR`, length-prefixed bincode or JSONL; daily rotation; retention env-capped).
  PG stays the position source of truth; the log is for determinism/recovery/debugging.

## 5. `params` JSONB — parsing & validation rules

Canonical example: `Bot/docs/strategy-redesign-answer-1.md`. Serde model in
`hunter/engine/src/rule_params.rs`:

```rust
pub struct RuleParams {
    pub take_profit: Option<f64>,          // % of entry price, e.g. 100 = +100%
    pub stop_loss:   Option<f64>,          // % drop, e.g. 30 = -30%
    pub entry: Option<SideConditions>,     // None = enter on arm (fingerprint alone)
    pub exit:  Option<SideConditions>,     // None = TP/SL/death only
}
pub struct SideConditions(pub BTreeMap<MetricGroupId, GroupConditions>);
pub struct GroupConditions {
    pub strict: GroupParams,               // e.g. window_size_sec — validated per group
    pub metrics: BTreeMap<MetricId, ConditionExpr>, // DNF: Vec<Vec<Condition>>
}
pub struct Condition { pub operator: Operator, pub value: f64 }
pub enum Operator { Gt, Gte, Lt, Lte, Eq, Ne }   // ">" ">=" "<" "<=" "=" "!="
```

Validation (rejected at rule save, `rules.rs::validate` calling the engine registry):
- unknown group / metric / operator names (registry-checked, so a typo can't silently no-op);
- metric listed under the wrong group; strict param missing for a group that requires it
  (`m_time_window` without `window_size_sec`);
- contradictory pairs on one metric (e.g. `> 30` AND `< 10`) — impossible entry;
- non-finite values; TP/SL ≤ 0.
- Parse **once at rule load** into typed structs (delivered to the engine via a
  `RulesReloaded` event) — never parse JSON per event.

## 6. Engine contract — events, effects, determinism rules

```rust
pub enum Event {
    TokenCreated { mint, fp: TokenFingerprint, at: Ts },   // fp may be first-slot-incomplete
    FirstSlotSettled { mint, buy_lamports: u64, sell_lamports: u64 }, // resolves deferred gate
    Trade        { mint, trade: TradeLite, at: Ts },       // TradeLite: side, sol, price, reserves
    Tick         { now: Ts },
    FillConfirmed{ intent: IntentId, fill: Fill },         // entry or exit fill
    FillFailed   { intent: IntentId, reason: FillFailReason },
    Migrated     { mint, at: Ts },
    RulesReloaded{ rules: Arc<[LoadedRule]>, fps: Arc<[Fingerprint]> },
    ManualClose  { position: PositionId },                 // manual sell / stop-all
}
pub enum Effect {
    SubmitBuy    { intent: IntentId, rule: RuleId, mint, lamports: u64 },
    SubmitSell   { intent: IntentId, position: PositionId, reason: ExitReason },
    PositionUpdate(PositionDelta),      // consumer: PG writer + SSE
    ArmedChanged (ArmedDelta),          // consumer: SSE (frontend live monitor)
}
```

Determinism rules (violation = bug):
- `IntentId` is derived, never random: `(rule_id, mint, monotonic seq in EngineState)`.
- All time comes from events (`at`/`now`); `Utc::now()` inside the engine cannot compile.
- Iteration order over rules/tokens is fixed (sorted keys / insertion-ordered), so effect
  order is reproducible.
- `reduce` is infallible: malformed input is rejected at the adapter boundary, not inside.
- Golden-log tests are the spec: event vector in → exact effect vector out.

## 7. Implementation phases

> Definition of done for every phase: `cargo check` clean on `hunter-live` + `hunter-lab`
> + `hunter-engine` (`--target-dir "C:/Users/User/Documents/Bot/target-check"` if a bin is
> running), clippy clean on touched code, tests listed in the phase green, no new warnings.

### Phase 0 — Schema + crate scaffold (foundation, no behavior change) ✅ 2026-07-16

- [x] 0.1 Write migration `hunter/core/migrations/0004_strategy_redesign.sql` (§4).
      *Note:* existing `strategy_positions.strategy_id` values were left as-is
      (historical reference); only new engine writes will use `'generic'`.
- [x] 0.2 Create crate `hunter/engine` (workspace member; deps: serde, serde_json,
      smallvec, chrono default-features-off). Purity guard test in `src/lib.rs`
      (comment-stripped manifest scan for tokio/sqlx/rand/actix/reqwest + asserts
      chrono keeps default features off).
- [x] 0.3 Move `grouping.rs` core→engine (git-mv, verbatim). `trading_core` shim:
      `pub use hunter_engine::{grouping, metrics};` in `lib.rs` — every existing
      `crate::grouping`/`trading_core::grouping` path (incl. the `sol_bucket_sql` twin
      guard test in `creation_stats_repo.rs`) compiles unchanged against the new SSOT.
- [x] 0.4 New model `hunter/core/src/models/fingerprint.rs` + rework
      `models/strategy.rs::StrategyRule`. *Implementation:* the old struct was renamed
      **`LegacyStrategyRule`** (still consumed by the not-yet-deleted tpsl/swing code —
      dies in Phase 7); the canonical `StrategyRule` name belongs to the new model, so
      Phases 1–6 build against final vocabulary.
- [x] 0.5 `engine/src/rule_params.rs`: registry-guided parse (`RuleParams::parse`, the
      one save/load entry point) + canonical `to_value()` + validation (§5); unit tests
      cover the docs-example round-trip, every rejection case, absent-side semantics.
      *Pulled forward:* `engine/src/metrics/mod.rs` (registry data: groups → metrics →
      unit/tolerance/kind/monotonic/strict-params — the 1.1 skeleton; JSON serialization
      for the registry endpoint still pending) and `metrics/evaluator.rs` (1.2, done).
- [x] 0.6 `FingerprintRepo` (insert/find/list/update/delete + `find_or_create` over the
      identity predicate — `IS NOT DISTINCT FROM` per axis, `name` excluded).
      *Deviation:* new-table rule CRUD lives on a new **`RuleRepo`**
      (`storage/repositories/rule_repo.rs`) rather than inside `StrategyRepo` — the
      legacy rule fns keep their names (55+ call sites in Phase-7-doomed files) but now
      point at `strategy_rules_legacy`; `managed_mints` resolves rule names from both
      tables via COALESCE. End state: rules CRUD = `RuleRepo`, runs/positions/metrics =
      `StrategyRepo`.
- [x] 0.7 Rule CRUD domain `core/src/strategies/rules.rs`: new `RuleDraft`
      (fingerprint_id + raw params JSON) → `build_rule`/`create`/`save` over `RuleRepo`;
      params persisted in canonical `RuleParams::to_value()` form. Legacy tpsl/swing
      draft shapes renamed (`LegacyRuleDraft`/`create_legacy`/`save_legacy`), deleted
      with their handler callers in Phases 4/7.

### Phase 1 — Metrics framework (`hunter/engine/src/metrics/`)

- [x] 1.1 `metrics/mod.rs`: `MetricId`/`MetricGroupId`, `Unit { Seconds, Sol, Percent }`,
      `MetricKind { Static, Dynamic }`, compile-time **registry** (groups → metrics →
      unit → eq-tolerance → monotonic flag → strict params). Registry serializes to JSON
      via `registry_json()` for the `/api/meta/strategy-registry` endpoint (frontend
      contract — FE plan §1): `{operators, groups:[{name,kind,strict_params,metrics}]}`.
      Shared input primitives `Ts`/`Side`/`TradeLite` + `secs_between` also live here.
- [x] 1.2 `metrics/evaluator.rs`: `eval(conditions, value, tol) -> bool` — `=`/`!=` via
      `|v-x| <= tol/2`; exhaustive unit tests incl. tolerance edges and NaN guards.
      (Done in Phase 0 alongside `rule_params`.)
- [x] 1.3 `metrics/snapshot.rs`: `time` (monotonic ✓, s, tol 0.5), `liquidity` (SOL,
      tol 0.1) — from creation time + last `reserve_sol`. `liquidity` = `NaN` before the
      first trade (no market data ⇒ satisfies nothing).
- [x] 1.4 `metrics/price_path.rs`: incremental `{peak_price, last_price, last_move_at}` →
      `stall` (s, tol 0.5; clock starts at creation, any price change resets it),
      `trail` (% off peak, tol 1.0; `NaN` pre-first-trade). Price = canonical curve-spot.
- [x] 1.5 `metrics/time_window.rs`: per-`window_size_sec` `VecDeque` of `(ts, signed_sol)`
      + running `buy`/`sell` sums → `gross_flow`/`net_flow`/`buy`/`sell` (SOL, tol 0.1).
      **Dedup key = `window_key(w)` (ms-rounded).** Trailing window `(now−w, now]`; evict
      on every trade/tick; O(1) read/fold, no per-event alloc.
- [x] 1.6 `metrics/track.rs`: `TokenTrack` — static states + `BTreeMap` of deduped
      windows; `new`/`ensure_window`/`on_trade`/`on_tick`; `value(id, window?, now)` routes
      by group; `values(reqs, now, &mut out)` batch into caller buffer (no alloc).
- [x] 1.7 `metrics/series.rs`: `MetricSeries` wraps `TokenTrack` (literal shared compute) —
      `SeriesColumn::{Static, Window}`, `push_trade`/`push_tick` record a value row per
      event; `column_values` extracts one series for the scan.
- [x] 1.8 Determinism test (`series.rs`): script of trades + interleaved ticks replayed
      twice ⇒ `to_bits()`-identical rows across runs AND identical to a bare-`TokenTrack`
      reference (no drift between the two compute paths).

### Phase 2 — Fingerprint matcher (`hunter/engine/src/fingerprint.rs`) ✅ 2026-07-16

- [x] 2.1 `matches(fp, tf) -> bool` — ported semantics from the `entry/mod.rs`
      `check_*` sets: exact `cu_limit`/`cu_price`/`ix_labels` (ordered), SOL axes via
      `same_bucket(v, fp_val, fp.bucket_size_amount)`; `None` axis = not part of
      identity; ≥1 configured criterion required (`has_any_criterion`, **never
      match-everything**). *Impl:* engine-pure `Fingerprint` (criteria, lamports at
      rest + `*_sol` accessors) + `FingerprintId(Uuid)`; reuses `grouping::TokenFingerprint`
      as the single observed-token axes type. Added `uuid` dep (default-features off,
      no `v4`/rng → still pure; not in the purity-guard banned list). SSOT
      `grouping::LAMPORTS_PER_SOL_F64` added for the in-engine lamports→SOL divisor.
- [x] 2.2 `match_all(fps, tf, phase) -> SmallVec<[FingerprintId; 4]>` (multi-match).
      Two-phase via `MatchPhase::{Instant, Full}`: `Instant` (at `TokenCreated`) judges
      only instant axes so a first-slot fingerprint stays *pending*; `Full` (at
      `FirstSlotSettled`) judges every axis. `has_first_slot_criteria`/`has_instant_criterion`
      let the Phase-4 producer classify pending vs armed. Empty `Some([])` ix_labels
      treated as inert (mirrors legacy).
- [x] 2.3 Guard tests (11): multi-match order, bucket edge at width boundary, ix_labels
      order/subset/superset sensitivity, first-slot two-phase resolution, ≥1-criterion
      guard, per-fingerprint width (coarse+fine both match same token), lamports→SOL
      conversion for max_cost/spendable.

### Phase 3 — The engine fold (`reduce`) + golden-log spec ✅ 2026-07-16

- [x] 3.1 `event.rs` + `state.rs`: types from §6. `event.rs` = `Mint`/`RuleId`/
      `PositionId`/`IntentId` (derived `(rule, mint, seq)`), `Event`/`Effect`,
      `LoadedRule`, `Fill`/`FillFailReason`, `ExitReason`, `PositionDelta`/`ArmedDelta`.
      `EngineState` (sorted `BTreeMap`s for determinism) holds compiled rules, loaded
      fingerprints, `all_windows` union, per-rule `RuleCounters {open,total}`, tracked
      `TokenState`s (track + `last_meaningful_at` + arms), a `positions` owner view for
      ManualClose, and the intent/position seq generators.
- [x] 3.2 `arm.rs`: `CompiledRule` (rule pre-chewed at reload into flat `MetricReq`
      lists + distinct windows + `MonoBound`s) and the `ArmState` machine
      (`PendingFirstSlot → Armed → EntryPending → Entered → ExitPending → Done |
      Disarmed(reason)`). Derived-unsatisfiability = every monotonic-metric entry upper
      bound (`<`⇒≥, `<=`⇒>, `=`⇒>value+tol/2); 6 unit tests incl. non-monotonic and
      lower-bound never derive-disarm.
- [x] 3.3 `reduce.rs`: full fold — arm on instant/full match, metrics update, disarm
      (dead via `deadness.rs`, migration, derived), entry check with caps
      (`max_concurrent`/`max_total` at entry not arm; over-cap ⇒ wait), `SubmitBuy`
      intent + `BuySubmitted` position, fill handling (`FillConfirmed`→Holding/End;
      `FillFailed`→bounded retry, entry give-up rolls counters back, exit `Unconfirmed`⇒
      terminal never-resell), exit priority `Dead > StopLoss > TakeProfit > Metrics`,
      `ManualClose`, TP/SL vs `entry_price` on canonical spot (tick uses last known
      price). Two-phase decide/apply keeps borrows disjoint + decisions side-effect-free.
      *Decisions taken:* Migration disarms **pre-entry** arms only — open positions ride
      it out (AMM trades keep pricing them). Dead-exit is emitted uniformly (paper acts
      on it via `Dead`; the real adapter's book-vs-noop choice is a Phase-4 concern).
- [x] 3.4 Move deadness: `is_dead_verdict` + the `DEAD_*` constants → `engine/src/
      deadness.rs` (SSOT); `trading_core` re-exports both (`config::constants` for the
      constants, `state::token_cache::is_dead_verdict` for the fn). `death.rs`'s
      `find_death_point` stays in core (needs the `TradeRow` trait) and now calls the
      moved verdict via that re-export — the fold computes deadness incrementally from
      folded state, so it needs the verdict, not the trades-slice death-point walk.
- [x] 3.5 **Golden-log tests** (`tests/golden.rs`, 20): arm→enter→TP, SL, metrics-exit,
      stall-exit on quiet token (tick-driven), disarm-derived, disarm-dead, migration,
      multi-rule concurrent entry, concurrent + total caps, entry fill-failure retry→
      give-up, manual close, unconfirmed-sell terminal, `Dead > SL` priority (via a dust
      trade that crashes price without resetting the quiet clock), first-slot two-phase
      arm/drop, untracked/non-matching ignored, determinism.
- [x] 3.6 Property tests (`tests/property.rs`, seeded xorshift — `rand` is banned by
      the purity guard so the PRNG is inline): 39 random 400-event streams never panic;
      every `SubmitSell` references a live position + every `SubmitBuy` a known rule; the
      `open` counter equals the live in-flight/held arm count and stays within caps;
      `positions` map size tracks live arms exactly.

### Phase 4 — Live adapters (hunter-live)

- [x] 4.1 THE serialized decision loop — `strategies/engine/decision_loop.rs` (`loop`
      is a keyword). `select!` over the same `strategy_rx` ping channel, a 500 ms `Tick`,
      a `fill_rx` (executor fills), and a `cmd_rx` (rule reloads / manual closes). Every
      `reduce` call happens here; effects dispatched two-pass (state effects → PG+SSE
      first, so a durable row exists before the submit is spawned; then submit effects).
      `spawn_engine` returns `EngineHandles { handle, armed, positions, task }`.
- [x] 4.2 `producers.rs`: `StrategyPing` + `TokenCache` → `Event`s (mirrors the old
      runner dispatch). `TokenCreated` (observed axes via `convert::observed_axes`, gated
      by `token_is_fresh`), one `Trade` per new cached trade via a per-mint absolute
      cursor (a dropped/coalesced ping never loses flow), `FirstSlotSettled` on the
      window-closed latch, `Migrated`. `reserve_sol` fed from `real_reserve_sol` (deadness
      parity). `retain`/`forget` bound the per-mint maps.
- [x] 4.3 `exec_real.rs`: `SubmitBuy` → `buy_token_snipe_write_ahead` (write-ahead
      `mark_buy_submitted` in the `on_signed` hook) → feed-confirm (`find_fill_by_signature`,
      woken by `TradeSignals`) with an **RPC watchdog** (`signature_state_detailed`).
      `SubmitSell` → `sell_token_once`/`amm_sell` (escalating tip) → confirm by
      `sum_legs_by_signatures` vs held amount. **Double-fire safety preserved:** emits
      `FillFailed` (⇒ engine resubmit) ONLY when safe (never-signed / confirmed revert /
      definitively-unsold); a truly-ambiguous outcome emits **nothing** and leaves the
      durable row for the reaper. Reuses `snipe_reserves_from_cache`.
- [x] 4.4 `exec_paper.rs`: transaction-free fill at the token's canonical spot at the
      instant the engine decided (parity with the sim path — both fill at the spot the
      triggering event carries). No sigs stashed. Death close stays engine-driven (`Dead`).
- [x] 4.5 `sinks.rs`: `PositionUpdate` → `strategy_positions` writer (lazy one-run-per-rule
      via `insert_run`; `record_entry_fill`/`close`; fill sigs threaded via the
      intent-keyed `FillSigStore` since the pure `Fill` carries none) + a **new**
      `SseEvent::StrategyPositionUpdate` (the legacy `TpslPositionsChanged` shape needs the
      legacy runtime cache — the new FE plan consumes the new variant). `ArmedChanged` →
      `SseEvent::StrategyArmedChanged` + the `ArmedRegistry` snapshot behind
      `GET /api/strategies/armed`.
- [x] 4.6 `event_log.rs`: JSONL recorder (loggable-subset `LoggedEvent`, no `Tick`/
      `RulesReloaded`; daily rotation; `EVENT_LOG_DIR`/`EVENT_LOG_RETENTION_DAYS` added to
      `hunter/.env` + `.env.example`). **Boot recovery** is conservative — `recover_armed`
      replays the recent tail (bounded by `MAX_SNIPE_AGE_SECS`) to **re-arm only** tokens
      with no open PG position (held mints + any mint that reached a fill in the log are
      excluded, so no re-entry). Effects discarded on replay. *Deferred:* full re-adoption
      of open positions into engine state (PG + the reaper own them).
- [x] 4.7 Reaper — `reapers.rs` PG **backstop** (`fail_stale_exit_pending` +
      `delete_stale_unentered`, both modes, 60 s). *Deviation from "emit events":* an opaque
      engine intent isn't reconstructible from a bare PG row, so the reaper settles the
      durable row directly, well past the point the engine could still act — same safety
      property, no event round-trip. Old `StrategyService` reapers are NOT spawned (they'd
      race the engine over `strategy_positions`).
- [x] 4.8 HTTP on the live bin (`api/handlers/strategies/engine.rs` + routes):
      `/api/strategy-rules` CRUD + activate/pause (over `RuleRepo` + `strategies::rules`),
      `/api/fingerprints` CRUD, `GET /api/meta/strategy-registry` (`registry_json()`),
      `GET /api/strategies/armed`. Every mutation ends in `engine.reload_rules()`.
      `DeployState` gained `engine`/`armed`/`rule_repo`/`fingerprint_repo`.
- [ ] 4.9 Runtime smoke (paper) — **PENDING**: needs the live stack (Postgres w/ migration
      0004 + Helius ingest) and a fresh token matching a rule's fingerprint. Compile-clean
      + engine golden/property tests + core tests green; the decision logic is the
      Phase-3-tested `reduce`, the adapters are thin.

### Phase 5 — Analysis adapters (hunter-lab)

**Progress 2026-07-17:** 5.1 + 5.2 + 5.3 + 5.7 built (analysis-simulate cluster).
**5.4 + 5.5 + 5.6 built + 5.8 verified 2026-07-17** — precompute-then-scan sweep
(`lab/src/sweep/generic/`); 5.8 run end-to-end on the live lab bin + real lake
(Dead-not-Open + win-rate curve + promote all confirmed). **Phase 5 COMPLETE.**

- [x] 5.1 `strategies/replay.rs`: lake→events producer — `ReplayToken`s expanded into
      one **globally time-ordered** event stream (`TokenCreated`/`FirstSlotSettled`/
      `Trade` merged by `(time, mint, kind)`), synthetic 500 ms `Tick`s between event
      timestamps and after last trade up to `as_of` = run-time now (deadness as-of),
      with tick emission stopping the moment no token is active (bounded by
      `DEAD_QUIET_SECS`). Sim fill model mirrors `exec_paper` (fills at the deciding
      event's spot, `FillConfirmed` fed back inline); `PositionOutcome` collector →
      `EngineBacktestResult` rows (legacy serde shape; PnL via the shared `CostModel`).
      **One shared `EngineState` in global order** so cross-token caps apply exactly
      as live. 7 tests incl. the cross-token concurrency-cap parity + determinism.
- [x] 5.2 Generic simulate: `POST /api/strategies/simulate` accepting `rule_id` **or an
      inline params draft** (`EngineSimRequest`), over the optional `{since,until}`
      window; candidate scan = instant-fingerprint match (two-phase resolves inside
      the fold); reuses `analysis_cache` single-flight + `SimResults`/`SimProgress`/SSE;
      results served by the strategy-agnostic `positions::sim_result_page/summary`.
      New handlers `api/handlers/strategies/engine.rs` + `strategies/engine_sim.rs`.
      Caps applied **in the fold**, not post-hoc (no `select_simulated_tokens`).
- [x] 5.3 **Live↔replay parity** (keystone): guaranteed *by construction* — both edges
      call the one `reduce`, feed observed axes through the shared
      `fingerprint_axes::{observed_axes,fp_to_engine,rule_to_loaded}` SSOT (moved to
      core; live `convert.rs` now re-exports), and derive the tick from the engine's
      `TICK_MS` SSOT. Guard tests: replay ticks at `TICK_MS` (`tick_matches_engine_ssot`),
      cross-token cap parity, determinism. *(A cross-crate event-vector diff harness is
      deferred; the shared-`reduce` + shared-converters design makes divergence a
      compile/const mismatch, which the guards catch.)*
- [x] 5.4 Sweep scan (`hunter/lab/src/sweep/generic/`): kept `grouped_engine.rs`
      partitioning + `GroupedSweepRepo` streaming persistence; **`GenericSweepStrategy`
      implements the existing `Strategy` trait** (`prepare_token` = per-token
      `MetricSeries` precompute; `resolve_entry`/`resolve_exit` = the scan reusing the
      engine's `eval` + `Dead>SL>TP>Metrics` priority + `round_trip_with_costs`), so
      the whole handler/partition/refine/persistence is reused. Axes model = `axes.rs`
      (`(side, group, metric, operator, values[]; window)` + TP/SL axes → `RuleParams`,
      registry-resolved). Tables collapsed to one `grouped_sweep_{runs,groups,results,
      combos}` set (`0003_generic_grouped_sweep.sql`, `"generic"` registry id).
      `MetricSeries` extended with per-row `price`/`reserve_sol`/`dead`; `prepare_token`
      widened to `&CorpusToken` for real `created_at` (carried from the lake); new
      `ExitCode::Metrics` + `n_exit_metrics` for the single metric-exit taxonomy.
- [x] 5.5 **Scan≡engine guard test** (`generic/guard.rs`, 4 tests): single-token
      `run_replay` (real fold) ≡ scan on a corpus covering TP/SL/Metrics/Dead/Open +
      entry-gated rules — identical fired/exit-code/entry+exit price/PnL. Parity holds
      by construction (shared `TokenTrack` values, tick grid, `TradeLite` map, evaluator,
      cost model); the guard fails first on any drift. **Proves the 5.8 "Dead not Open"
      invariant** directly (the dead token books `ExitCode::Dead` both ways).
- [x] 5.6 Promotion `POST …/groups/{group_id}/promote?strategy_id=generic[&combo_id=N]`:
      rebuilds the group's `Fingerprint` from its `group_key` at the run's bucket width
      (SOL fields → bucket lower-edge representative) → `FingerprintRepo::find_or_create`,
      returns a pre-filled `RuleDraft`-shaped body for the editor (review → dry-run →
      save; rule not persisted here).
- [x] 5.7 Metric-series endpoint `GET /api/tokens/{mint}/metric-series?windows=…` —
      replays one token's full history (lake ∪ PG tail) through `metrics/series.rs` on
      demand, returning every metric's value at every trade as parallel arrays
      (`m_time_window` metrics per requested window; non-finite ⇒ `null`). Never
      persisted. `api/handlers/tokens/metric_series.rs`.
- [x] 5.8 Standing verification checks — **run end-to-end 2026-07-17** against the live
      lab bin + real lake corpus (after fixing the `0004` core-migration boot bug, below).
      A `"generic"` TP×SL grid over 3770 lake tokens confirmed **dead tokens book `Dead`,
      not `Open`** (only 3/3770 `Open`; 2733–3415 `Dead` per combo) and a **sane monotonic
      win-rate curve** (TP 50→30.9%, 100→21.2%, 200→14.1%; `n_exit_metrics`=0 with no exit
      metrics). A grouped sweep (`group_by=[cu_limit]`, entry `time>2/5`) → **promote**
      produced a fingerprint with exact `cu_limit=520000` at the run's 0.1 width + the
      winning combo's params as a ready-to-save draft; a second promote reused the same
      fingerprint (`find_or_create` dedup verified, 1 row).
      - **Migration fix (prereq):** core `0004` had an index-name collision — `0001`
        creates `idx_strategy_rules_active` on `strategy_rules`; `0004` renames that table
        to `_legacy` (index name moves with it) then `CREATE INDEX idx_strategy_rules_active`
        on the new table → "already exists", so `0004` failed + rolled back on **every**
        boot (it had never applied on any DB seeded from `0001`). Fixed by renaming the
        legacy index (`ALTER INDEX … RENAME TO idx_strategy_rules_legacy_active`) right
        after the table rename.

### Phase 6 — Event-log tooling (small, high leverage) ✅ 2026-07-17

- [x] 6.1 Log replay endpoint on lab: `POST /api/replay/inspect` loads a recorded
      live log (all day-files, or one `date`), re-runs `reduce` over it, and dumps every
      `event → effects` decision as JSON. `hunter/lab/src/api/handlers/replay.rs`
      (handler, loads rules from PG) + `hunter/lab/src/strategies/replay_inspect.rs`
      (fold driver). Rules come from PG (the log omits `RulesReloaded`) → replays against
      the *current* rule set; synthetic 500 ms ticks are interleaved on the
      `TICK_MS` grid (empty-tracked-set skip keeps quiet gaps O(1)); the real logged
      `FillConfirmed`/`FillFailed` are replayed verbatim (no sim fills). `mint`/`since`/
      `until` narrow only the **output** (whole log still folded — cross-token caps
      honored); `date` narrows the loaded files. `Effect` is projected to a serializable
      `effect`-tagged dump. **SSOT move:** `LoggedEvent` (the on-disk format) lifted from
      `live/.../event_log.rs` into `hunter/engine/src/event_log.rs` so the writer (live
      recorder) and reader (lab inspector) share one definition. 5 lab tests + engine
      compiles/tests green; clippy clean. Frontend viewer = FE6 (`ReplayViewerPage`).
- [x] 6.2 Event-log deep-dive (format SSOT, rotation, retention, env vars, recorder,
      boot-recovery, replay/inspection endpoint + slicing caveats, parity, file map)
      — implemented in code (`engine/src/event_log.rs`, live recorder, lab inspector).

### Phase 7 — Deletion + docs (leave no dead vocabulary)

> **⛔ BLOCKED (assessed 2026-07-17; blocker 1 cleared same day).** Deletion targets
> in §3 still have compile-time references *outside* the deletion set. Remaining
> blockers are prerequisite unfinished work, not Phase-7 work itself:
> 1. ~~**Phase 5.4–5.6 (sweep rewrite)**~~ — **cleared.** Generic sweep is wired;
>    legacy `Tpsl1/Tpsl2/Swing1` sweep strategies can be retired with the rest.
> 2. **Legacy per-strategy rule handlers — never retired.** `lab/src/api/handlers/
>    strategies/{tpsl1,tpsl2,swing1}.rs` (+ `tokens/swing1_detect.rs`, `swing_probe.rs`),
>    routed at `lab/src/api/mod.rs:77-255`, still call `crate::strategies::
>    {tpsl_sniper_1,tpsl_sniper_2,swing_1}::run_backtest` and `StrategyImpl::from_id`.
>    These block the lab backtest dirs + `registry.rs` (`StrategyImpl`) + the typed models.
> 3. **Legacy live service kept for compile (Phase-4 debt).** `StrategyService` is still
>    constructed at `live/main.rs:702` and held on `DeployState.strategy`, read by the
>    live `rules.rs`/`positions.rs` handlers → blocks `service.rs`/`runner.rs`/`execution/*`.
> 4. **Shared-core domain still on the registry:** `core/strategies/{match_keys,rules,
>    runtime_cache}.rs` + `core/models/mod.rs:32-34` import `StrategyImpl`/typed models.
>
> Only `live/src/strategies/execution/*.rs` is otherwise ref-free, and even it waits on
> `service.rs` (blocker 3). **Retire the legacy lab rule handlers + live `StrategyService`
> first**, then Phase 7 becomes a clean sweep.

- [ ] 7.1 Delete (list in §3 "Deleted at the end"); grep-sweep for `tpsl`, `swing_1`,
      `strategy_id`, `scalp`, `LadderParams`, `StrategyImpl`, `exit_state`.
- [ ] 7.2 Follow-up migration: drop `strategy_positions.strategy_id`; decide fate of
      `strategy_rules_legacy` + old per-strategy sweep tables (keep read-only or drop).
- [ ] 7.3 Docs: rewrite `hunter/docs/arch/strategies.md` around §2 diagrams; update
      `arch/sweep.md`, `arch/database.md`, `arch/architecture.md` (new crate);
      hunter/CLAUDE.md crate table + hot-path notes (500 ms tick, event loop, watchdog);
      new deep-dive `metrics-reference.md` (per-metric formula/unit/tolerance/
      monotonicity — grows with every added metric).
- [ ] 7.4 Full `cargo test -p hunter-engine -p trading_core -p hunter-lab -p hunter-live`;
      clippy; paper runtime smoke post-deletion.

## 8. Extensibility contract (adding a metric later)

1. Add the metric to its group file (or a new `metrics/<group>.rs`): compute logic + one
   registry entry (name, unit, eq-tolerance, static/dynamic, monotonic flag, strict
   params if a new group).
2. Nothing else server-side: params validation, evaluator, engine, replay, sweep axes,
   and the `/api/meta/strategy-registry` payload all read the registry — and the frontend
   renders from that payload, so **no frontend change either** (FE plan §1).
3. Update `metrics-reference.md` (formula + rationale).

## 9. Future toggles — designed now, built later

### 9.1 Re-entry after exit (per-rule toggle)

- Column `allow_reentry BOOLEAN NOT NULL DEFAULT false` on `strategy_rules` (+ optional
  strict param `reentry_cooldown_sec` in params).
- Engine: `ArmState::Done` becomes `Done | Cooldown { until }`; after exit the
  (token, rule) returns to `Armed` after cooldown **iff** no disarm reason holds (dead /
  migrated / derived-unsatisfiable still terminal). Derived disarm naturally caps
  re-entry for time-bounded rules.
- Caps: `max_total_tokens` counts **entries**, not tokens, once re-entry exists — rename
  or document then. Positions already support multiple rows per (rule, mint) via
  `run_id`; verify indexes/PnL view aggregate correctly.

### 9.2 Single-position-per-token (exclusivity toggle)

- Per-rule `exclusive BOOLEAN` (an exclusive rule skips entry if ANY rule holds the
  token; non-exclusive rules ignore others) — or a global env; decide when building.
- Engine: entry-check consults the positions view inside `reduce` — the serialized loop
  makes the claim race-free by construction (single transition point).
- Priority if two rules match on the same event: deterministic order (rule `created_at`)
  — decide when implemented.

## 10. Risks / open edges (tracked, not blockers)

- **Live rewrite scope**: Phase 4 replaces `service.rs`/`runner.rs`/`execution/*`
  wholesale rather than retargeting — larger diff, but the golden-log spec (3.5) and
  parity test (5.3) catch regressions the old incremental path couldn't.
- **Watchdog tuning**: N-slot feed-confirm timeout must buffer the feed's index lag
  (preserve the current "poll the full window before retrying" wisdom from
  `execution/real.rs` when porting).
- **Event-log size**: trades dominate; daily rotation + retention env; ticks are NOT
  logged (they're regenerable — replay derives them from timestamps, decision 12's
  recovery replays with synthetic ticks exactly like analysis).
- **Tick cost**: 500 ms over armed+holding sets only; ≥1-criterion guard +
  `MAX_SNIPE_AGE_SECS` + derived disarm bound the set. Add an armed-count gauge before
  tuning further.
- **Paper fill realism** stays worst-case-fill — unchanged by this redesign.
- **Sweep scan memory**: per-token series are transient (compute, scan all combos for
  that token, drop) — never materialize the whole corpus's series at once.
