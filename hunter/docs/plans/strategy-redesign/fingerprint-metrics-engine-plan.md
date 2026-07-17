# Strategy redesign — Fingerprint + Metrics generic engine (implementation plan)

Status: **IN PROGRESS** (design settled 2026-07-16; architecture upgraded same day to the
deterministic event-log engine — all five "best solution" upgrades adopted).
**Phase 0 complete 2026-07-16** (plus 1.2 pulled forward). **Phase 1 complete
2026-07-16** (metrics framework: 1.1 registry JSON, 1.3–1.8); next: Phase 2
(fingerprint matcher).
Scope: **hunter only** — forge untouched. Backend first; frontend has its own plan:
[frontend-plan.md](frontend-plan.md) (phases there map onto backend phases here).
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
4. **Operators**: `>` `>=` `<` `<=` `=` `!=`. All conditions **AND**. `=` is
   bucket-equality using the **metric's own default tolerance** declared in its metric
   file (time/stall 0.5 s, trail 1 %, SOL metrics 0.1 SOL) — deliberately independent of
   the fingerprint's `bucket_size_amount`. Absent group/metric/TP/SL = unconstrained.
5. **Lifecycle**: armed per **(token, rule)**. Evaluation on **every trade AND every
   500 ms clock tick** (tick sized to ~400 ms slot latency; entry and exit both fire on
   ticks). Exit = TP hit `OR` SL hit `OR` all exit metrics true. Disarm on dead-token
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
 exit check:  TP hit   OR   SL hit   OR   ALL exit metrics true ── no ───┘
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
    pub metrics: BTreeMap<MetricId, Vec<Condition>>,
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

### Phase 2 — Fingerprint matcher (`hunter/engine/src/fingerprint.rs`)

- [ ] 2.1 `matches(fp, tf) -> bool` — port semantics from the three `entry/mod.rs`
      `check_*` sets (source of truth for edge cases): exact `cu_limit`/`cu_price`/
      `ix_labels` (ordered), SOL axes via `same_bucket(v, fp_val, fp.bucket_size_amount)`;
      `NULL` field = not part of identity; require ≥1 configured criterion
      (**never match-everything**).
- [ ] 2.2 `match_all(fps, tf) -> SmallVec<FingerprintId>` (multi-match). First-slot
      fields evaluate in two phases: instant axes at `TokenCreated`, first-slot axes at
      `FirstSlotSettled` (the event replaces today's `pending_first_slot` +
      1 s sweep backstop — the **producer** owns slot-close detection).
- [ ] 2.3 Guard tests: multi-match, bucket edges at width boundaries, ix_labels order
      sensitivity, first-slot two-phase resolution, ≥1-criterion guard.

### Phase 3 — The engine fold (`reduce`) + golden-log spec

- [ ] 3.1 `event.rs` + `state.rs`: types from §6; `EngineState` holds tracks, arm states
      (per token→rules SmallVec), a positions view (id, rule, mint, entry fill, status),
      per-rule counters (armed/holding/pending/total), intent seq.
- [ ] 3.2 `arm.rs`: `ArmState` machine (`Armed → EntryPending → Entered → ExitPending →
      Done | Disarmed(reason)`); derived-unsatisfiability precompute at `RulesReloaded`
      (monotonic-metric upper bounds from entry conditions; incl. `=` once
      value > bound + tol). Unit tests incl. non-monotonic metrics never derived-disarm.
- [ ] 3.3 `reduce.rs`: full fold — arm on match, metrics update, disarm checks
      (dead via `deadness.rs`, migration, derived), entry check (caps
      `max_concurrent`/`max_total` enforced here, at entry not arm), `SubmitBuy` intent,
      fill handling (`FillConfirmed` → Entered/closed; `FillFailed` → retry policy or
      Done), exit priority `Dead > StopLoss > TakeProfit > Metrics`, `ManualClose`,
      TP/SL vs `entry_price` on canonical spot (tick uses last known price).
- [ ] 3.4 Move deadness: `is_dead_verdict` + death-point logic → `engine/src/deadness.rs`
      (SSOT; `trading_core` re-exports for the token-cache consumer).
- [ ] 3.5 **Golden-log tests** (the engine's spec): scripted event vectors → exact
      effect vectors; scenarios: arm→enter→TP, SL, metrics-exit, stall-exit on quiet
      token (tick-driven), disarm-derived, disarm-dead, migration, multi-rule concurrent
      entry, caps, fill-failure retry, manual close.
- [ ] 3.6 Property tests (fast — crate has no heavy deps): random event streams never
      panic; effects only reference known intents/positions; counters never negative.

### Phase 4 — Live adapters (hunter-live)

- [ ] 4.1 `strategies/loop.rs`: THE serialized decision loop (`select!` over ingest pings,
      500 ms `Tick` interval, fill notifications, rule reloads, manual commands) — every
      `reduce` call happens here; effects dispatched after each call. Replaces
      `runner.rs` + `service.rs` orchestration. (Decision 11: no mint-sharding.)
- [ ] 4.2 `producers.rs`: ingest→`Event` bridge (`TokenCreated` with instant fp axes,
      `Trade`, `Migrated`); slot-close detector emitting `FirstSlotSettled`;
      `token_is_fresh` (`MAX_SNIPE_AGE_SECS`) live-only gate applied at the producer.
- [ ] 4.3 `exec_real.rs`: `SubmitBuy`/`SubmitSell` → executor submit-and-return; fills
      confirmed from the **trades feed** (own-wallet match → `FillConfirmed`);
      **confirmation watchdog**: no feed fill within N slots ⇒ one RPC cross-check ⇒
      `FillConfirmed`/`FillFailed`. Sell-confirm budget preserved: feed-first, RPC only
      as timeout fallback (this closes the audited feed-confirm gap). Buy retry/give-up
      policy expressed as engine reaction to `FillFailed` (bounded attempts).
- [ ] 4.4 `exec_paper.rs`: worst-case fill model (port `resolve_paper_*` semantics) →
      emits `FillConfirmed` from real indexed trades; death close for paper stays
      engine-driven (Dead exit reason), no separate sweep task.
- [ ] 4.5 `sinks.rs`: `PositionUpdate` → PG writer (channel, existing repo) + position
      SSE (keep today's delta events so `useRulePositions` keeps working);
      `ArmedChanged` → new SSE event type (coalesced ≤2/s per token) + armed snapshot
      endpoint `GET /api/strategies/armed` (frontend live monitor).
- [ ] 4.6 `event_log.rs`: recorder (append every event pre-reduce; rotate daily;
      `EVENT_LOG_DIR`/retention envs — add to `hunter/.env` + `.env.example` per env
      rules) + **boot recovery**: replay log tail (bounded by `MAX_SNIPE_AGE_SECS`) to
      rebuild armed state; reconcile positions against PG (PG wins).
- [ ] 4.7 Keep recovery reapers (`redrive_orphaned_*`, `reconcile_externally_cleared_*`,
      `fail_stale_exit_pending`) — retarget to emit `ManualClose`/`FillFailed` events
      instead of mutating positions directly (single transition point preserved).
- [ ] 4.8 `http.rs`: rules + fingerprints CRUD on the live bin;
      `GET /api/meta/strategy-registry` (serialized engine registry — FE contract).
- [ ] 4.9 Runtime smoke (paper): permissive rule → watch arm→enter→exit via SSE/logs;
      quiet-token stall exit fires on tick; kill the bin mid-arm → boot recovery
      restores armed state from the log.

### Phase 5 — Analysis adapters (hunter-lab)

- [ ] 5.1 `strategies/replay.rs`: lake→events producer — `CorpusTrade`s in canonical
      order (`slot → tx_index → leg_index → block_time`), synthetic 500 ms `Tick`s
      between timestamps and after last trade up to `as_of` = run-time now (deadness
      as-of precedent); sim fill model emits `FillConfirmed` inline; results collector
      consumes `PositionUpdate`s.
- [ ] 5.2 Generic simulate: `POST /api/strategies/simulate` accepting `rule_id` **or an
      inline params draft** (frontend dry-run needs unsaved drafts) + corpus window;
      replaces per-strategy simulate routes; keep result-cache plumbing
      (`state/{sim_results,sim_summary}.rs`), swap the resolver.
- [ ] 5.3 **Live↔replay parity test** (keystone): same event vector through the live
      loop's dispatch path and the replay driver ⇒ identical effects; guard test that
      both use the one tick constant.
- [ ] 5.4 Sweep scan (`hunter/lab/src/sweep/`): keep `grouped_engine.rs` partitioning
      (`group_key`) + `GroupedSweepRepo` streaming persistence; replace the three
      wrappers with the generic axes model — axis = (side, group, metric, operator,
      values[]; window for dynamic) — combos scan precomputed `MetricSeries` (§2.6).
      Tables collapse to one set `grouped_sweep_{runs,groups,results,combos}`
      (lab migration).
- [ ] 5.5 **Scan≡engine guard test**: full replay vs scan agree on a sample corpus
      (decision 13's drift lock).
- [ ] 5.6 Promotion: winning combo + group → `FingerprintRepo::find_or_create` (at the
      run's bucket width) + `rules::create`; returns the draft for the frontend editor.
- [ ] 5.7 Metric-series endpoint `GET /api/tokens/{mint}/metric-series?windows=…` —
      replay one token through `metrics/series.rs` on demand (chart panes; metrics are
      never persisted).
- [ ] 5.8 Re-run standing verification checks (bf61547f-style): dead tokens book `Dead`
      not `Open`; win-rates comparable to pre-redesign baselines for equivalent conditions.

### Phase 6 — Event-log tooling (small, high leverage)

- [ ] 6.1 Log replay CLI/endpoint on lab: load a recorded live log (or a slice by
      mint/time), re-run `reduce`, dump every event→effect decision as JSON
      (`POST /api/replay/inspect`). This is the time-travel debugger backend
      (frontend viewer = FE plan phase FE6).
- [ ] 6.2 Doc: `docs/plans/strategy-redesign/event-log.md` — format, rotation,
      retention, replay semantics, recovery procedure.

### Phase 7 — Deletion + docs (leave no dead vocabulary)

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
