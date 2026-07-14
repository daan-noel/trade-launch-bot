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
| `trading_core` | lib | config, models, storage, core services/state (`CoreState`), api framework + auth + SSE bridge, core handlers, strategy domain (`tpsl_rules_core`), **ingest contract** (`trading_core::ingest`) |
| `pump-trader` (dep key; pkg `executor-pumpfun` at `shared/executor/pumpfun`, lib `pump_trader`; + `executor-core`) | lib | buy/sell executor; **standalone drop-in** (no workspace deps). Signs via `Arc<dyn Signer>`; typed `error::TradeError`. `probe`/`claim` off-by-default features |
| `ingest-laserstream` (dep key; pkg `ingest-pumpfun` at `shared/ingest/pumpfun`, lib `ingest_laserstream`; + `ingest-core`) | lib | Helius LaserStream gRPC transport (client→pipeline→db_writer) + watchdog. **Standalone drop-in** (NOT `trading_core`); exposes raw transport API, bridged onto `trading_core::ingest` by `live`'s host adapter |
| `live` | **bin** | LIVE box: strategies, trader, deploy services/state (`DeployState`), live/trading handlers, `probe`. Ships to EC2 |
| `lab` | **bin** | ANALYSIS box: sweep/backtest, swing analyzer, local state (`LocalState`), rule-authoring + sweep handlers. NO keys / NO gRPC; never depends on the executor |

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
| [docs/arch/strategies.md](docs/arch/strategies.md) | `strategies/`: StrategyRunner, tpsl1/tpsl2 module map, exit ladder |
| [docs/arch/trade-execution.md](docs/arch/trade-execution.md) | executor crate: module map, key behaviors |
| [docs/arch/database.md](docs/arch/database.md) | Postgres schema, pools, every repo→table→fns |
| [docs/arch/frontend.md](docs/arch/frontend.md) | `frontend-react/src/`: pages, components, hooks, RTK Query/SSE |
| [docs/arch/sweep.md](docs/arch/sweep.md) | `sweep/`: param-sweep engine, grouping, persistence, API |

Deep-dive references: [docs/plans/database/lake-pg-read-paths.md](docs/plans/database/lake-pg-read-paths.md)
(which trade reads hit the lake vs PG), [docs/plans/frontend/token-list-backend.md](docs/plans/frontend/token-list-backend.md)
(`/api/tokens` differs by bin), plus the per-subsystem docs under `docs/plans/`.

## Commands

```powershell
cargo check -p hunter-live             # typecheck the live bin
cargo check -p hunter-lab              # typecheck the analysis bin
cargo check -p hunter-core             # typecheck the shared lib
cargo test  -p hunter-live             # live unit tests (strategies, trader edge)
cargo test  -p hunter-lab              # lab unit tests (sweep, swing)
cargo test  -p hunter-live -- --ignored  # integration; needs DATABASE_URL + HELIUS_RPC_URL
cargo test  -p executor-pumpfun        # trader crate tests
cargo run   -p hunter-live             # live box: loads .env; needs Postgres + Helius gRPC (binds LIVE_PORT :8130)
cargo run   -p hunter-lab              # analysis box: needs Postgres; NO keys / NO gRPC (binds LAB_PORT :8140)
cargo run   -p hunter-lab -- lake-export # batch: export sealed days local-PG -> Parquet lake ($SWEEP_LAKE_DIR)
cargo run   -p hunter-live -- probe <ladder|fanout|simulate-sell|holdings> [args]
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

## Gotchas (hot-path landmines)

- **Deferred entry fingerprint gates:** a criterion whose source data isn't settled at
  `TokenCreated` (`p_token_first_slot_{buy,sell}_sol`) must defer via
  `StrategyRuntimeCache::pending_first_slot` + the 1s runner sweep — never block the hot path
  with sleep/poll. Instant fingerprint axes still match synchronously in `on_token_created`.
- **Stale-creator `ConstraintSeeds` (2006) self-heal is unified**, not sell-only:
  `pump-trader::trader::swap_retry::classify_swap_revert` is the one SSOT decision (route ×
  direction × error code) both crates use — `live`'s sell loop + curve-buy snipe retry import
  it, no local copy. See [docs/arch/trade-execution.md](docs/arch/trade-execution.md).
- `tpsl_sniper_1`/`tpsl_sniper_2` **decision** modules (`entry`/`exit`, in `trading_core`)
  are intentional clones — a fix in one usually belongs in both. (Live *orchestration* is no
  longer cloned: one registry-dispatched `live/src/strategies/{service,runner,execution}`.)
- **Analysis-only death-close (`ExitReason::Dead`):** sim/grouped-sweep/detect no longer
  mislabel silent-death tokens as `Open` at a stale price — the analysis exits append
  `strategies::death::find_death_point` (shared `token_cache::is_dead_verdict` SSOT). Live
  **paper** reuses the same `find_death_point` (`sweep_dead_paper_exits`); **real is
  untouched** (a dead pool has no liquidity to sell into). See
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
