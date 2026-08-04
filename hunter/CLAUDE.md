# CLAUDE.md — hunter

Meme-coin trading bot — **massive token + trade volume; performance outranks
everything.** Read [../CLAUDE.md](../CLAUDE.md) first for the monorepo-wide rules
(SSOT, backend-latency-first, EC2 constraint, `.env`, docs discipline). This file is
hunter-specific only.

## Hunter-specific priorities

- **SSOT — the token-data key is `mint_address`.** The mint of any token-data
  row/request/response names its field `mint_address` — the ONE key across DB columns,
  Rust/TS DTOs, the table filter/sort grammar, and frontend column keys
  (`tokens.mint_address`, `trades.mint_address`, `strategy_positions.mint_address`). A
  bare `mint` field/key on a token-data path is a bug (the frontend `TokenTable` mint
  accessor and the `in`-op mint-set filter rely on this — no `mintOf` prop). The
  executor + ingest crates and the `lake/` schema keep their own decoupled `mint` vocab.
  Existing SSOT anchors to reuse, never re-derive: `MARKET_CAP_SQL`, `market_cap_sol`,
  `config::constants::{sol_to_lamports,lamports_to_sol}`, the lake `schema.rs` column
  names, `token_enrichment::ENRICH_SELECT`, the TS `TokenEnrichmentFields` base.
- **Efficient frontend state.** RTK Query / SSE cache; memoize high-freq ticks (SOL/USD,
  live trades). Build UI from `components/ui/`, `components/table/DataTable`, shared hooks.

## Architecture

Six Rust crates + `frontend-react` SPA. The old single `backend` crate was split into two
bins over a shared core (`live`/`lab` topology). The two standalone drop-in crates moved
to the monorepo's `shared/` home (see [../CLAUDE.md](../CLAUDE.md)); hunter links them as
intra-workspace deps.

| Crate | Kind | Role |
| --- | --- | --- |
| `trading_core` | lib | config, models, storage, core services/state (`CoreState`), api framework + auth + SSE bridge, core handlers, strategy domain (fingerprint matching + metric-series + the shared cost/summary kernel; the pure fold lives in `hunter-engine`), **ingest contract** (`trading_core::ingest`) |
| `pump-trader` (dep key; pkg `executor-pumpfun` at `shared/executor/pumpfun`, lib `pump_trader`; + `executor-core`) | lib | buy/sell executor; **standalone drop-in** (no workspace deps). Signs via `Arc<dyn Signer>`; typed `error::TradeError`. `probe`/`claim` off-by-default features |
| `ingest-laserstream` (dep key; pkg `ingest-pumpfun` at `shared/ingest/pumpfun`, lib `ingest_laserstream`; + `ingest-core`) | lib | Helius LaserStream gRPC transport (client→pipeline→db_writer) + watchdog. **Standalone drop-in** (NOT `trading_core`); exposes raw transport API, bridged onto `trading_core::ingest` by `live`'s host adapter |
| `live` | **bin** | LIVE box: strategies, trader, deploy services/state (`DeployState`), live/trading handlers, `probe`. Ships to EC2 |
| `lab` | **bin** | ANALYSIS box: sweep/backtest, replay/simulate over the generic engine, local state (`LocalState`), rule-authoring + sweep handlers. NO keys / NO gRPC; never depends on the executor |

Each bin is its own composition root (`tokio::select!`). `live/main.rs` starts ingest
(`live::ingest::spawn_ingest`, host adapter over the raw transport) + trading + strategy +
HTTP; `lab/main.rs` is thin (SOL-price poller + token-cache seed + HTTP, no ingest/trader).
Helius LaserStream (gRPC) is the **sole** live transport; the `trades` table *is* that feed.
The frontend split is build-time (no runtime capability advertisement); the frontend uses
`live`/`lab` vocabulary throughout (`@live`/`@lab` aliases, `src/live`/`src/lab`,
`liveApi`/`labApi`).

**Read `docs/arch/` instead of re-exploring source. Deep-dive detail lives in `docs/plans/`.**

| Doc | Covers |
| --- | --- |
| [docs/arch/architecture.md](docs/arch/architecture.md) | crate map, two bins' `main.rs` wiring, three state structs, ingest interface |
| [docs/arch/ingest.md](docs/arch/ingest.md) | ingest crate + host adapter: client→pipeline→db_writer, file map |
| [docs/arch/strategies.md](docs/arch/strategies.md) | the generic fingerprint+metrics engine: the pure `hunter-engine` fold + the live `strategies/engine/` adapters (decision loop, producers, exec, sinks, event-log) |
| [docs/arch/trade-execution.md](docs/arch/trade-execution.md) | executor crate: module map, key behaviors |
| [docs/arch/database.md](docs/arch/database.md) | Postgres schema, pools, every repo→table→fns |
| [docs/arch/frontend.md](docs/arch/frontend.md) | `frontend-react/src/`: pages, components, hooks, RTK Query/SSE |
| [docs/arch/sweep.md](docs/arch/sweep.md) | `sweep/`: param-sweep engine, grouping, persistence, API |

Deep-dive references: [docs/plans/database/lake-pg-read-paths.md](docs/plans/database/lake-pg-read-paths.md)
(which trade reads hit the lake vs PG), [docs/plans/frontend/token-list-backend.md](docs/plans/frontend/token-list-backend.md)
(`/api/tokens` differs by bin), [docs/plans/strategies/execution-costs.md](docs/plans/strategies/execution-costs.md)
(**what a round trip costs — read before believing any backtest**),
[docs/plans/strategies/wallet-analysis.md](docs/plans/strategies/wallet-analysis.md)
(reverse-engineered external scalper wallets — the calibration source for the
flow-scalper rule ladder), plus the per-subsystem docs under `docs/plans/`.

**Active / unfinished plans** (WIP roadmaps — the strategy redesign, the audit) live in
[`docs/roadmap/`](docs/roadmap/), kept separate from the permanent deep-dive references in
`docs/plans/`. A plan is deleted (or folded into a deep-dive) once its work lands;
`docs/plans/` never holds a throwaway plan. Volume-flow-split is **shipped** — canonical
ref [`docs/plans/strategies/metrics-reference.md`](docs/plans/strategies/metrics-reference.md);
roadmap kept only for §8 future toggles.

## Commands

```powershell
cargo check -p hunter-live             # typecheck the live bin
cargo check -p hunter-lab              # typecheck the analysis bin
cargo check -p hunter-core             # typecheck the shared lib
cargo test  -p hunter-live             # live unit tests (strategies, trader edge)
cargo test  -p hunter-lab              # lab unit tests (sweep, replay/simulate)
cargo test  -p hunter-live -- --ignored  # integration; needs DATABASE_URL + HELIUS_RPC_URL
cargo test  -p executor-pumpfun        # trader crate tests
cargo run   -p hunter-live             # live box: loads .env; needs Postgres + Helius gRPC (binds LIVE_PORT :8130)
cargo run   -p hunter-lab              # analysis box: needs Postgres; NO keys / NO gRPC (binds LAB_PORT :8140)
cargo run   -p hunter-lab -- lake-export # batch: export sealed days local-PG -> Parquet lake ($SWEEP_LAKE_DIR)
cargo run   -p hunter-live -- probe <ladder|fanout|pin-senders|simulate-*|sim-matrix|holdings> [args]
cd frontend; npm run dev               # both apps concurrently: live :5173, lab :5174 (separate dev servers)
npm run lint                           # ESLint boundary gate ONLY (shared⊬@live/@lab, live⊬@lab, lab⊬@live); not a general lint
npm run dev:live                       # live app only (:5173, proxies /api -> live bin :8130)
npm run dev:lab                        # lab app only  (:5174, proxies /api -> lab bin :8140)
npm run build:live                     # tsc (checks BOTH trees) && vite build (live config) → LIVE-ONLY dist/index.html
npm run build:lab                      # tsc (checks BOTH trees) && vite build (lab config)  → workstation lab.html (never deployed)
```

**Frontend is two apps over a shared core** (mirrors the backend two-bin split):
`src/shared` · `src/live` (`@live/*`) · `src/lab` (`@lab/*`), two Vite entries + two dev
servers (`index.html`→live :5173, `lab.html`→lab :5174; `lab.html` is dev-only). Mode is
build-time, not runtime — no `useCapabilities` gating. Ship the **live** build to EC2
(`npm run build:live` emits lab-free `dist/index.html`). One split `createApi`: `baseApi`
shell + per-mode `injectEndpoints`; import mode hooks from `@live|@lab/store/*Endpoints`,
never the shared `store/apiSlice` barrel. See [docs/arch/frontend.md](docs/arch/frontend.md).

Stay in the owning crate. Use `--target-dir target-check` if a bin `.exe` is running.
Clippy `too_many_arguments` is `#[allow]`-ed on trade-path fns by design.

## Performance budgets (hot path — violation = bug)

- **Sell-confirm:** no new RPC call — confirm via the `trades` gRPC feed. An RPC poll
  reintroduces latency + double-sell risk. The exit loop polls the **full** window before
  retrying (buffers the feed's index lag) — preserve when editing `execution/real.rs`.
- **Ingest pipeline:** no blocking I/O, `.await`-on-lock, or unbounded per-event alloc.
  DB/SSE writes through channels only.
- **Strategy eval:** read from `runtime_cache.rs` (in-memory), never DB-per-event.

## Data-scale guardrails

- Bound every query — paginate/time-window/stream. Never `SELECT *` the full
  `trades`/`raw_txs`. Hot tables are **TimescaleDB hypertables** with declarative
  compression + retention (in `0001_init.sql`); the old hand-rolled `maintenance.rs`
  partition loop is gone.
- **Trade-history reads: lake vs PG.** Single-rule simulate + all `lab` analysis read the
  sealed Parquet lake (same corpus/`SweepTrade` as the sweep); only two indexed lookups
  stay on PG. There is ONE deliberate full-history PG carve-out (`GET
  /api/tokens/:mint/trades`, `limit<=0` ⇒ no LIMIT) — **don't re-add a row cap.**
  `MAX_TRADES_RETAINED` is the live in-RAM cache trim, never an analysis bound. Full rules:
  [docs/plans/database/lake-pg-read-paths.md](docs/plans/database/lake-pg-read-paths.md).
- **`/api/tokens` backend differs by bin** (same `POST TableRequest` wire contract): `live`
  pages straight from Postgres (full 100K+ universe, no in-RAM cap); `lab` runs the in-RAM
  engine over a full snapshot. `SEED_TRACKING_LIMIT` is the tracking-cache seed cap, not the
  list cap. Details + parity guards:
  [docs/plans/frontend/token-list-backend.md](docs/plans/frontend/token-list-backend.md).

## Deployed server (EC2: 2vCPU / 4GB — see [../CLAUDE.md](../CLAUDE.md))

- **Ship `live` + the ingest crate to EC2 only.** `lab` (sweep/arrow/parquet/rayon +
  bundled `duckdb` + the `lab/src/lake/` pipeline) stays on the workstation — never deploy.
- Sweeps/backtests: local only (server = 7-day rolling ingest buffer). Analysis:
  server→local DB sync (`scripts/db-incremental-sync.ps1`, incremental DB→DB over SSH).
- Don't raise `MAX_TRADES_RETAINED`, `SEED_TRACKING_LIMIT`, or cache TTLs on the server.

## Definition of done (hunter-specific)

- **Backend:** `cargo check -p hunter-live` + `-p hunter-lab` clean; clippy on touched
  code; test when logic changed.
- **Frontend:** `npm run build:live` clean + `npm run lint` clean (the import-boundary
  gate: never cross shared→`@live`/`@lab`, live→`@lab`, lab→`@live` — relocate the code
  instead); no extra re-render on SOL/USD tick or live-trade stream.
- **Docs — update the tier that changed** (see [../CLAUDE.md](../CLAUDE.md) docs
  discipline): rules → this file; structure/data-flow → `docs/arch/[subsystem].md`;
  algorithm/decision detail → `docs/plans/[subsystem]/[topic].md`.

## SOL vs lamports naming (locked, no exceptions)

Every field/column/variable denoting an amount of SOL names its unit. `_lamports` = exact
integer (`BIGINT`/`i64`/`u64`); `_sol` = human `f64`. Same base concept keeps the same base
name across layers, unit-only suffix differs (DB `entry_lamports` → model `entry_sol`,
converted at the repo boundary via the **one shared**
`config::constants::{sol_to_lamports,lamports_to_sol}` pair — no private copies). If a name
held lamports but read like SOL, drop the `sol` (`reserve_sol`→`reserve_lamports`, not
`reserve_sol_lamports`). Ratios/rates are **not** amounts — keep `_price`/`_pct`. JSONB keys
follow the same rule. A new SOL column that skips the suffix is a bug (caused the
`find_tx_by_fill` lamports-vs-SOL mismatch). Codified in `0009_sol_lamports_naming.sql`; the
executor + `lake/` schema keep their own decoupled vocab.

## Raw `u64` on-chain args (locked)

A creation-instruction arg's domain is **`u64`**, and real data uses all of it:
pump.fun's `max_sol_cost` is a *slippage ceiling*, so "fill at any price" is
`u64::MAX` (≈1.84e10 SOL). It is a **sentinel, not an amount**.

- **One decode seam:** `hunter_engine::grouping::extract_lamports` → `Option<u64>`,
  never narrowed. Need an `i64` (a bucket, a `BIGINT` axis)? Go through
  `bucketable_lamports`, which returns `None` instead of a wrapped `-1` or a
  saturated `i64::MAX`. `MAX_BUCKETABLE_LAMPORTS` is the one threshold — the SQL
  mirror interpolates that same constant.
- **A ceiling is its own group**, rendered as exact digits by `exact_sol_label_u64`
  (integer math) in both precision modes — never binned, and never folded into the
  `∅` missing key: "no cap set" and "field absent" are different facts.
- **`float8` is banned wherever the value can pass 2^53** (exact labels, exact
  compares → `numeric`); **bucket** arithmetic stays `float8` because the engine
  bins in `f64` and the mirror must reproduce that rounding, not improve on it.
  Divide-by-`1e9` is banned in the exact SQL path — `select_div_scale` truncates it;
  multiply by `0.000000001` instead.
- **Wire = JSON string** (`trading_core::serde_wire::u64_as_string`), applied to the
  whole family of raw `u64` args, because a JSON number is an f64 to the browser.
  Storage keeps the JSON **number** — `jsonb` holds it as arbitrary-precision
  `numeric`, so nothing is lost at rest. Frontend reads via `lib/u64Wire`.

Full reference incl. the known `BIGINT`-axis limitation:
[docs/plans/database/u64-instruction-args.md](docs/plans/database/u64-instruction-args.md).

## Zero-as-unbound (locked)

`0` may mean "off / unbounded" **only where 0 is not a valid value of the domain** — the
governance caps `max_total_tokens` (`0 ⇒ unlimited`) and `max_concurrent_tokens` (`0 ⇒ the
default of 1`; the API rejects `< 1`). Both decode through the ONE reader
`hunter_engine::Cap` (`zero_unlimited` / `zero_defaults_to`, `UNLIMITED = u32::MAX` so
`allows()` stays a single `<` on the hot path) — never a re-derived `!= 0` at a call site.
**Slippage bps is NOT such a field**: a typed value is honored literally and `0` is a 400
(`validate_slippage_bps`), because blank — not `0` — is what carries the per-side policy
(buy ⇒ default, sell ⇒ no floor). See
[docs/plans/trade-execution/slippage-logic-buy-sell.md](docs/plans/trade-execution/slippage-logic-buy-sell.md).
Anything **measured** — a SOL amount, a count, a bucket edge, a width — uses `Option`/NULL
for "not set", because 0 is a real observation there. Never fold the two: a fingerprint axis of `0`
lamports is the bucket `[0, width)`, and only `None` drops the axis from the fingerprint's
identity (`bucket_axis` / `IS NOT DISTINCT FROM` / the `∅` grouping sentinel all rely on it).
**An empty collection is the same sentinel** — `ix_labels: Some([])` means "not set", so it
collapses to `None` via the ONE decider `hunter_engine::fingerprint::configured_labels`, and
`from_json` folds `[]` → `None` at the wire boundary so the ambiguous state never reaches
storage (normalized by `0015_fingerprint_empty_ix_labels_null.sql`).

A *sentinel* field always needs ONE reader; two readers of the same sentinel is the bug, and
it fails **in opposite directions** on the fingerprint path — the engine matcher turns
"no criteria" into *matches nothing* (rules go silently dead) while `fingerprint_scope_clauses`
turns it into *matches every token in the window*. Both of the fixed bugs had that shape:
`bucket_size_amount` (`0 ⇒ default 0.1` in the SQL mirror vs literal `0` in the matcher, where
it saturates every positive amount into one bucket and arms on any non-zero value) and
`ix_labels` (`is_some()` in the model vs empty-filtered in the engine). Locked by
`has_any_criterion_agrees_with_engine` + `fingerprint_scope_sql_buckets_every_sol_axis_at_the_engine_width`.
The slippage bug was the third shape — a *writer* that clamped the sentinel away before any
reader saw it, inverting `0` from "accept any fill" into the tightest possible floor on the
bot's own exits (locked by `a_typed_percent_reaches_the_trader_unchanged`).
Where a sentinel stays, the UI marks it with the `Input` `blankZero` prop, never a truthiness
check — today that is exactly one field, the rule editor's **Max total** (blank/`∞`).

## Gotchas (hot-path landmines)

- **A backtest is only as honest as its `CostModelKind`.** Default to
  **`pumpfun_impact`** — it is the only kind that charges our own price impact
  (`buy_amount_sol / reserve_sol` per leg), so the only one whose cost responds to buy
  size at all. `pumpfun_fee_only` is size-blind (an optimistic bound) and
  `pumpfun_default` double-counts against any explicit `FillModel`. Two corrections
  landed 2026-07-28: the pump.fun fee is **125 bps/leg, not 100**, and nothing charged
  impact before that — so **every run older than 2026-07-28 understates cost by up to
  ~3 pp/round-trip** and does not compare to a new one (the constants are not persisted
  per run). Cost is U-shaped in size — the Jito tip is fixed SOL/leg — so the optimal
  fixed buy is `sqrt(fixed_per_leg × vsol)`, ~0.27 SOL on a 70 SOL pool, NOT 0.1 or 1.0.
  Full derivation: [docs/plans/strategies/execution-costs.md](docs/plans/strategies/execution-costs.md).
- **Exit conditions that do not do what they look like.** `m_price_lifetime.stall` is
  *seconds since the last all-time HIGH* and resets only on a new high, so on a
  dip-entry rule it silently caps every hold (~15 s measured) **and** doubles as an
  entry filter, since `can_enter` refuses while an exit metric holds. Use
  `m_position.held >= N` for a time stop. Likewise an authored `m_position.retrace >= N`
  is a hard −N% stop from entry until the price rises, because `PositionCtx::at_fill`
  seeds `peak = entry_price` — arm it with `arm_above_pct`. Both in
  [docs/plans/strategies/flow-scalper-findings.md](docs/plans/strategies/flow-scalper-findings.md).
- **A silent shed on a bounded queue hides a total outage.** `ping_strategy`
  (`ingest/consumer.rs`) `try_send`s onto a 512-deep queue and sheds on full — correct
  (the engine must never back-pressure ingest), but a *wedged* engine sheds 100% of
  pings while ingest keeps writing tokens/trades to PG, so every external signal looks
  healthy while no rule is evaluated. It shed for 14 h on 2026-07-30 without one log
  line. Any `try_send`-and-drop on a path that decides trades must be **loud**
  (rate-limited `warn!`, never a bare counter). Same shape as the 2026-07-22 heartbeat
  bug: a liveness signal that stays green through a total failure.
- **Boot work must be bounded, and the watchdog must not police a booting process.**
  `recover_armed` read the whole event-log corpus (~8.2 GB) to use its last 30 s; the
  ingest watchdog then force-exited it mid-recovery at 90 s, **70 boots in a row**, so
  the decision loop never started. Both halves are now guarded (bounded tail scan +
  `BootGate`) — don't reintroduce either. Diagnostic: **`strategy engine loop running`
  absent from the log = the engine never started**, regardless of how healthy ingest
  looks. Detail: [docs/arch/strategies.md](docs/arch/strategies.md).
- **An unemitted fill event leaks a concurrency slot permanently.** The
  `BuySubmitted` row is durable *before* the send, so every exit from
  `dispatch_buy`/`run_entry` MUST emit `FillConfirmed`/`FillFailed` (use
  `decision_loop::fail_entry`) — a bare `return` strands the arm in `EntryPending`,
  and boot re-adopts the row, so the `max_concurrent_tokens` slot is lost across
  restarts too. Ten such rows filled a live rule's cap and silenced it for ~17 h on
  2026-08-02; the cause was a buy send with **no timeout** (the sell path had
  `SELL_SEND_TIMEOUT`, the buy path had no mirror until `BUY_SEND_TIMEOUT`). Same
  family as the silent-shed and false-heartbeat bugs above: a failure that leaves
  every visible signal green. Detail:
  [docs/arch/position-lifecycle.md](docs/arch/position-lifecycle.md).
- **Deferred entry fingerprint gates:** a fingerprint axis whose source data isn't settled at
  `TokenCreated` (`first_slot_{buy,sell}_lamports`) can't match synchronously. The engine arms
  it as `PendingFirstSlot` and resolves it on the `FirstSlotSettled` event (fired when the
  creation slot closes) — never a sleep/poll on the hot path. Instant axes still match
  synchronously on `TokenCreated`. (See `hunter-engine` `reduce.rs` / the `MatchPhase` split.)
- **Stale-creator `ConstraintSeeds` (2006) self-heal is unified**, not sell-only:
  `pump-trader::trader::swap_retry::classify_swap_revert` is the one SSOT decision (route ×
  direction × error code) both crates use — `live`'s sell loop + curve-buy snipe retry import
  it, no local copy. See [docs/arch/trade-execution.md](docs/arch/trade-execution.md).
- **ONE decision kernel — live, paper, simulate are literally the same code; sweep is the
  only sanctioned approximation (ROOT RULE).** Entry / exit / caps / re-entry / retries for
  **live-real, live-paper, and single-rule simulate** are ALL decided inside
  `hunter-engine::reduce` — real vs paper fork *only* at the fill layer (`exec_real` vs
  `exec_paper`), simulate *only* at who feeds events (`lab`'s `replay.rs`). Never add a
  second decision path or a per-strategy clone (the tpsl1/tpsl2/swing1 stack was retired in
  Phase 7). A decision fix lands in exactly one place; live closes route through the engine
  (`EngineHandle::manual_close` / `reconcile_cleared`), never a separate service. The
  **grouped-sweep** is the ONE allowed re-implementation — a precomputed `MetricSeries` scan
  (`lab/src/sweep/generic/strategy.rs`) that trades exactness for speed. Its hard contract:
  **(a)** every fact it *can* share with the engine — deadness verdict, death-point, cost/PnL
  kernel, leaf-condition `eval`, `CompiledRule::compile`, fill model, `TICK_MS` — is
  single-sourced from `hunter-engine`/`core`, never copied; **(b)** every deliberate
  divergence from `reduce` (bounded per-token tail, stripped concurrency caps, sketched
  quantiles) is recorded in [docs/plans/sweep/sim-parity.md](docs/plans/sweep/sim-parity.md)
  **and** locked by a `sweep/generic/guard.rs` parity test. **Simulate is the PnL authority;
  a sweep result is a ranking screener, NOT a backtest — always re-run a promoted combo
  through simulate before trusting its PnL** (the sweep's uncapped, per-token-tail numbers
  are optimistic upper bounds).
- **Analysis-only death-close (`ExitReason::Dead`):** sim/grouped-sweep no longer mislabel
  silent-death tokens as `Open` at a stale price — the shared deadness verdict
  (`hunter-engine::deadness` / `token_cache::is_dead_verdict` SSOT, via
  `strategies::death::find_death_point` for the exact point) books a Dead exit. The live
  engine folds the same verdict; a dead **real** pool has no liquidity to sell into. See
  [docs/arch/strategies.md](docs/arch/strategies.md).
- **Truncated logs drop trade legs:** the validator truncates a tx's logs past a byte limit,
  so the curve decoder under-counts legs on multi-buy bundle txs; `decode_curve_pb` recovers
  them from inner-instruction self-CPI events when logs are empty OR truncated — never revert
  to an `is_empty()`-only fallback. AMM path still log-only (latent gap). Full detail in
  [docs/arch/ingest.md](docs/arch/ingest.md).
- **`trades`↔`wallet_dict` resolution:** the address lives only in `wallet_dict` (interned
  `wallet_id`), no FK on `trades.wallet_id`, so a missing dict row must never hide a trade —
  all `trades` read paths **LEFT JOIN** with `COALESCE(w.address,'unknown:'||wallet_id)`,
  never INNER. On the `lab` mirror `wallet_dict` is non-destructively merged each sync. (An
  INNER join once hid ~58% of the lab's trades — looked like an ingest miss, wasn't.)
- **Flow hash SSOT:** `hunter_engine::metrics::flow_split::{ix_hash,wallet_hash,ix_hash_opt}`
  are the only hashers for volume/organic classification. Live producer, lake replay, and
  event-log adapters must call them — never roll a private FNV/string join. Patterns compile
  to a hash set at `RulesReloaded`. See
  [docs/plans/strategies/metrics-reference.md](docs/plans/strategies/metrics-reference.md).
