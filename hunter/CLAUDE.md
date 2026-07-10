# CLAUDE.md

Guidance for Claude Code working in this repo.

## Priorities

Meme-coin trading bot — **massive token + trade volume**. Performance outranks everything. On every change:

- **Backend latency first.** Hot paths (ingest, strategy eval, sell-confirm): no blocking runtime, redundant RPC/DB round-trips, per-event alloc, or lock contention. Notify over poll.
- **Modular.** handler→service→repo; page→component+hook. One responsibility per module.
- **Single source of truth.** Before adding a constant, formula, SQL fragment, type, or column list, search for an existing one and reuse it — never copy-paste a fact that must stay consistent. A value/rule/shape lives in exactly ONE place (e.g. `MARKET_CAP_SQL`, `market_cap_sol`, `config::constants::{sol_to_lamports,lamports_to_sol}`, the lake `schema.rs` column names, `token_enrichment::ENRICH_SELECT`, the TS `TokenEnrichmentFields` base). **The mint of any token-data row/request/response names its field `mint_address` — the ONE token-data key across DB columns, Rust/TS DTOs, the table filter/sort grammar, and the frontend column keys** (`tokens.mint_address`, `trades.mint_address`, `strategy_positions.mint_address`); a bare `mint` field/key on a token-data path is a bug (the frontend `TokenTable` mint accessor and the `in`-op mint-set filter both rely on this uniformity — no `mintOf` prop). The `pump-trader` crate, `ingest-laserstream` raw events, and the `lake/` schema keep their own decoupled `mint` vocab. On every change, actively watch for **SSOT violations** — the same fact defined twice that can silently drift. When duplication is genuinely unavoidable (deliberate crate decoupling like `pump-trader`'s constants, or the intentional `tpsl_sniper_{1,2}` clones), it is NOT license to drift: add a guard test that asserts the copies stay equal (e.g. `live/tests/protocol_constants_ssot.rs`), and prefer a no-DB guard so it runs on every `cargo test`.
- **Efficient frontend state.** RTK Query / SSE cache; memoize high-freq ticks (SOL/USD, live trades).
- **Reusable UI.** Build from `components/ui/`, `components/table/DataTable`, shared hooks.
- **Concise.** Short answers; non-trivial plans to a `*-plan.md` file.

## Architecture

**Monorepo:** `meme-trading/` is now one product folder inside the `Bot/` monorepo (single Cargo `[workspace]`, `resolver = "1"`, root `Bot/Cargo.toml`) alongside `solana-launch-platform/`. The two **standalone drop-in** crates (`pump-trader`, `ingest-laserstream`) moved to a neutral **`shared/`** home (`Bot/shared/…`) so both products consume them as intra-workspace deps — meme-trading's `live` links them via `path = "../../shared/…"`. A bare `cargo build` at the root builds only meme-trading's bins (`default-members`); target SLP crates with `-p`. See `../monorepo-trader-plan.md` (Part 1).

Six Rust crates + `frontend-react` SPA. The old single `backend` crate was split into two bins over a shared core, then renamed to the `live`/`lab` topology (see [docs/plans/modes/crate-split.md](docs/plans/modes/crate-split.md) and [live-lab-remake-plan.md](live-lab-remake-plan.md)):

| Crate | Kind | Role |
| --- | --- | --- |
| `trading_core` | lib | config, models, storage, core services/state (`CoreState`), api framework + auth + SSE bridge, core handlers, strategy domain (`tpsl_rules_core`), **ingest contract** (`trading_core::ingest`) |
| `pump-trader` (`shared/pump-trader`) | lib | buy/sell executor; **standalone drop-in library** (no workspace deps). Three tiers: `protocol` (Tier-1 `const Pubkey` invariants), `config` (Tier-2 `TraderConfig` + 7 `Default` sub-structs), per-call args. Signs via `Arc<dyn Signer>` (HSM/remote-ready); typed `error::TradeError` (no `anyhow`). `probe`/`claim` are off-by-default cargo features; `constants.rs` is a thin back-compat shim |
| `ingest-laserstream` (`shared/ingest-laserstream`) | lib | Helius LaserStream gRPC transport (client→pipeline→db_writer) + watchdog. **Standalone drop-in — no workspace deps** (NOT `trading_core`); exposes its own raw transport API (`Ingest`/`IngestHandle`/`IngestEvent`/`Protocol`), which `live`'s host adapter bridges onto the `trading_core::ingest` contract |
| `ingest-websocket` | lib | **empty scaffold** (this one *does* depend on `trading_core`) — a stub transport so `live` can swap ingest backends later (not yet implemented) |
| `live` | **bin** | LIVE box: strategies, trader, deploy services/state (`DeployState`), live/trading handlers, `probe`. Ships to EC2 |
| `lab` | **bin** | ANALYSIS box: sweep/backtest, swing analyzer, local state (`LocalState`), rule-authoring + sweep handlers. Runs with NO keys / NO gRPC; never depends on `pump-trader` |

The transport-agnostic ingest contract (`IngestHandles`, `TraderHook`, re-exports of `StrategyPing`/`TradeSignals`) lives in `trading_core::ingest`. `ingest-laserstream` is **standalone** (no workspace deps) and exposes only its raw transport (`Ingest`/`IngestHandle`/`IngestEvent`); the host adapter `live/src/ingest/` (`spawn_ingest`) is what builds that transport and bridges its events onto the `trading_core` contract, returning `IngestHandles`. `ingest-websocket` (the not-yet-implemented sibling) does depend on `trading_core`.

Each bin is its own composition root (`tokio::select!` over long-lived tasks). `live/main.rs` starts ingest via `live::ingest::spawn_ingest` (host adapter over `ingest_laserstream::Ingest`) + trading + strategy + HTTP; `lab/main.rs` is thin (SOL-price poller + token-cache seed + HTTP, no ingest/trader). Helius LaserStream (gRPC) is the **sole** live transport; the `trades` table *is* that feed. Both serve `configure_core_routes` plus their own route config. The frontend split is build-time, so there is no runtime capability advertisement — each bin builds into its own SPA with a static nav. **The frontend uses `live`/`lab` vocabulary throughout** (`@live`/`@lab` aliases, `src/live`/`src/lab` trees, `liveApi`/`labApi`).

**Read `docs/arch/` docs instead of re-exploring source. Deep-dive detail lives in `docs/plans/`.**

| Doc | Covers |
| --- | --- |
| [docs/arch/architecture.md](docs/arch/architecture.md) | crate map, two bins' `main.rs` wiring, three state structs, ingest interface |
| [docs/arch/ingest.md](docs/arch/ingest.md) | `ingest_laserstream/`: client→pipeline→db_writer, file map |
| [docs/arch/strategies.md](docs/arch/strategies.md) | `strategies/`: StrategyRunner, tpsl1/tpsl2 module map, exit ladder |
| [docs/arch/trade-execution.md](docs/arch/trade-execution.md) | `pump-trader/`: module map, key behaviors |
| [docs/arch/database.md](docs/arch/database.md) | Postgres schema, pools, every repo→table→fns |
| [docs/arch/frontend.md](docs/arch/frontend.md) | `frontend-react/src/`: pages, components, hooks, RTK Query/SSE |
| [docs/arch/sweep.md](docs/arch/sweep.md) | `sweep/`: param-sweep engine, grouping, persistence, API |

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
npm run dev:live                       # live app only (:5173, proxies /api -> live bin :8130)
npm run dev:lab                        # lab app only  (:5174, proxies /api -> lab bin :8140)
npm run build:live                     # tsc (checks BOTH trees) && vite build (live config) → LIVE-ONLY dist/index.html
npm run build:lab                      # tsc (checks BOTH trees) && vite build (lab config)  → workstation lab.html (never deployed)
```

**Frontend is two apps over a shared core** (mirrors the backend two-bin split): `src/shared` ·
`src/live` (`@live/*`) · `src/lab` (`@lab/*`), two Vite entries + **two dev servers**
(`index.html`→live `vite.live.config.ts` :5173, `lab.html`→lab `vite.lab.config.ts` :5174;
`lab.html` is dev-only — never built for prod). Each app runs independently (`dev:live`/`dev:lab`)
or both at once (`dev`). Mode is build-time, not runtime — no `useCapabilities` gating. Ship the
**live** build to EC2 (`npm run build:live` emits lab-free `dist/index.html`). One split `createApi`:
`baseApi` shell + per-mode `injectEndpoints`; import mode hooks from `@live|@lab/store/*Endpoints`,
never the shared `store/apiSlice` barrel, so a mode's side effect never leaks across builds. See
[docs/arch/frontend.md](docs/arch/frontend.md).

Stay in the owning crate (`trading_core` / `pump-trader` / `ingest-laserstream` / `ingest-websocket` / `live` / `lab`). Use `--target-dir target-check` if a bin `.exe` is running (locks `target/`). Clippy `too_many_arguments` is `#[allow]`-ed on trade-path fns by design.

## Performance budgets (hot path — violation = bug)

- **Sell-confirm:** no new RPC call — confirm via the `trades` gRPC feed. An RPC poll reintroduces latency + double-sell risk.
- **Ingest pipeline:** no blocking I/O, `.await`-on-lock, or unbounded per-event alloc. DB/SSE writes through channels only.
- **Strategy eval:** read from `runtime_cache.rs` (in-memory), never DB-per-event.

## Data-scale guardrails

- Bound every query — paginate/time-window/stream. Never `SELECT *` the full `trades`/`raw_txs`. **Deliberate carve-out:** `GET /api/tokens/:mint/trades` (`get_trades` → `TradeRepo::find_by_mint_paged`, `limit <= 0` ⇒ no `LIMIT`) returns a token's **full** trade history on purpose — the inspect charts (Positions / Sim / grouped-sweep) resolve their entry/exit markers + swing legs against this exact trade set, so a first-N cap mis-snapped the exit / later swing legs off a high-volume token. It's still mint-scoped (never the whole table) and a cold, deliberately-opened path; don't re-add a row cap. A positive `limit` still pages.
- **Single-rule simulate reads the Parquet lake, not PG** (tpsl1/tpsl2/swing1 `.../simulate` → `strategies::sim_fetch::fetch_sim_histories` → `LakeSource::load` with `Selection::with_signatures = true`, the **same corpus + same `SweepTrade`** the grouped sweep uses — no separate `SimTrade`; the flag only populates `tx_signature` for Solscan links; the grouped sweep loads it `false` and instead resolves its token-results table's entry/exit signatures via a narrow indexed `(mint, slot, side)` PG lookup (`grouped_sweep.rs`'s `resolve_fill_signatures` → `TradeRepo`) rather than carrying the extra bytes through every sweep row; that lookup plus the candidate-token scan are the **only** remaining PG reads of the `trades` table anywhere in `lab` — every other trade-history path (grouped sweep, simulate, swing1-detect, the generic `swing.rs` analyzer, backtests) is lake-only). The lake is **sealed-days-only**, so keep `cargo run -p lab -- lake-export --include-today` on a cadence or simulate on recent (today's) tokens returns truncated histories (the loader logs a stale-lake warn). Parity with the sweep is guarded by `lake::duck::parity_tests` (no longer `--ignored`: auto-runs when `$SWEEP_LAKE_DIR` points at a populated lake, self-skips otherwise), and the writer/reader lake column names are single-sourced in `lab/src/lake/schema.rs`. The per-token **`swing1-detect`** endpoint reads the token's **full history = sealed lake ∪ PG fresh tail** (`fetch_full_history_one`: lake for the deep past, plus every PG row on a slot the lake never reached) via the shared `swing_1::funnel::build_swing1_funnel`. Lake-only blanked the overlay for a token created after the last `lake-export` (entry/exit markers still showed — they come from the PG fills, not the lake); the PG-tail union closes that, and the lake still covers tokens older than PG's 30-day `trades` retention. The swing1 backtest still **carries its legs** in the result row — so the *sim/sweep* inspect chart's legs are the sim's own, no re-detect; only the *position* overlay re-detects (and now sees the fresh tail). `MAX_TRADES_RETAINED` is the **live in-RAM cache trim, never an analysis read bound** — analysis reads full history. The **generic** `swing.rs` endpoints (`detect_token_swings`/`detect_tokens_swings_batch` — a separate analyzer from the swing1 strategy) also read the same uncapped lake now; the batch path resolves its whole mint list in **one** `fetch_sim_histories` call instead of per-mint PG round trips.
- New high-volume tables are **TimescaleDB hypertables** with declarative `add_compression_policy` + `add_retention_policy` (defined in `0001_init.sql`); continuous aggregates are set up at boot by `trading_core::storage::timescale`. The old hand-rolled `maintenance.rs` partition loop is gone.
- **Token-list backend differs by bin** (same `/api/tokens` wire contract: **`POST` `TableRequest`** — the unified strategy-table body; the global filter panel + per-column filters fold into ONE `filters:{col→FilterSpec}` map, lowered onto the internal engines by `TokenQuery::from_table_request`). `live` pages the list **straight from Postgres** — filter/sort/search are compiled to SQL by `trading_core::api::handlers::tokens::sql` (`build_where_and_order` → `TokenRepo::find_list_page`/`count_list`), so the full token universe (100K+) is pageable with NO in-RAM cap; the live in-RAM `token_list` snapshot holds **only tracking tokens** (4 GB EC2 guardrail — live does NOT run `run_token_list_db_refresh`). `lab` runs the in-RAM `build_tokens_list` engine over a **full snapshot** (`LAB_TOKEN_LIST_LIMIT`/`LAB_TOKEN_LIST_WINDOW_DAYS`, workstation RAM, analysis speed). `SEED_TRACKING_LIMIT` (formerly `SEED_TOKEN_LIMIT`) is now the **live tracking-cache seed** cap only, never the list cap. The two engines are held at parity by `token_repo::parity_tests` (no longer `--ignored`: auto-runs when `DATABASE_URL` is set, self-skips otherwise) plus a **no-DB** column-key guard `handlers::tokens::grammar_parity_tests` that runs on every `cargo test`.

## Deployed server (EC2: 2vCPU / 4GB RAM — IO-bound, RAM-constrained)

- **Ship `live` + `ingest-laserstream` to EC2 only.** `lab` (sweep/arrow/parquet/rayon + bundled `duckdb`, and the `lab/src/lake/` Parquet-lake pipeline) stays on the workstation — never deploy it.
- Sweeps/backtests: **local only** (server = 7-day rolling ingest buffer)
- Analysis: server→local DB sync (`scripts/db-incremental-sync.ps1` — incremental DB→DB over an SSH tunnel)
- No new infra spend (box stays fixed)
- Every new write path must justify IO cost; follow partition+retention pattern
- Connection counts are load-bearing; new pools require shrinking something else
- Don't raise `MAX_TRADES_RETAINED`, `SEED_TRACKING_LIMIT`, or cache TTLs on server

## Definition of done

- **Backend:** `cargo check -p hunter-live` + `cargo check -p hunter-lab` clean; clippy on touched code; test when logic changed
- **Frontend:** `npm run build:live` clean; no extra re-render on SOL/USD tick or live-trade stream
- **Docs — update ALL affected tiers:**
  - Rules/commands/constraints changed → **CLAUDE.md**
  - Module structure/data flow/behavior changed → **docs/arch/[subsystem].md** *(high-level map: crates, files, data flow)*
  - Implementation detail/algorithm/decision changed → **docs/plans/[subsystem]/[topic].md** *(deep-dive: column rationale, invariants, tuning constants, design decisions — permanent reference docs, not temporary plans)*
- Stayed in the owning crate, no new warnings, no secrets in code

## .env management

`.env` is gitignored; keep in sync with `.env.example`. When `.env.example` updates: backup first, then apply every new key with real values.

```powershell
Copy-Item .env .env.backup -Force   # always do this first
```

## SOL vs lamports naming (locked, no exceptions)

Every field/column/variable that denotes an amount of SOL names its unit. `_lamports` = exact integer (`BIGINT`/`i64`/`u64`); `_sol` = human `f64`. Same base concept keeps the same base name across layers, unit-only suffix differs — the DB stores `entry_lamports`, the model exposes `entry_sol` (converted at the repo boundary via the **one shared** `config::constants::{sol_to_lamports, lamports_to_sol}` pair — every repo imports it, no private copies). If a name held lamports but read like SOL, drop the `sol` (`reserve_sol` → `reserve_lamports`, **not** `reserve_sol_lamports`). Ratio/rate fields are **not** amounts and keep `_price`/`_pct` (`entry_price`, `price_per_token`, `pnl_pct`, `cu_price`). JSONB keys follow the same rule (`initial_buy_instruction->>'max_cost_lamports'`/`'spendable_lamports_in'`). A new SOL column that skips the suffix is a bug (it caused the `find_tx_by_fill` lamports-vs-SOL mismatch). Codified in migration `0009_sol_lamports_naming.sql`; the `pump-trader` crate + the `lake/` Parquet schema keep their own decoupled vocab.

## Gotchas

- **Deferred entry fingerprint gates:** a criterion whose source data isn't settled at `TokenCreated` (today: `p_token_first_slot_buy_sol` / `p_token_first_slot_sell_sol`) must defer via `StrategyRuntimeCache::pending_first_slot` + the existing 1s runner sweep — never block the hot path with sleep/poll. Instant fingerprint axes still match synchronously in `on_token_created`; only rules that opt into the deferred fields wait for window-close (or the 5s backstop).
- **Sell-confirm timing:** the exit loop polls the **full** window before retrying — buffers the gRPC feed's index lag. Without it, duplicate sells fire. Preserve when editing `execution/real.rs` or the sell retry path.
- **Stale-creator `ConstraintSeeds` (2006) self-heal is unified**, not sell-only: `pump-trader::trader::swap_retry::classify_swap_revert` is the one SSOT decision (route × direction × error code) both crates use. `pump-trader` self-heals in-call on every `confirm=true` swap (`sell_token`/`buy_token`/`amm_sell`/`amm_buy` — manual + AMM buy/sell are now covered, not just curve sell), retrying once only when the refresh reports the creator/`coin_creator` actually changed. `live`'s feed-confirmed bot sell loop and curve-buy snipe retry import the same classifier instead of keeping a local copy — see `docs/arch/trade-execution.md` and `stale-creator-2006-unify-plan.md`.
- `tpsl_sniper_1`/`tpsl_sniper_2` **decision** modules (`entry`/`exit`, in `trading_core`) are intentional clones — a fix in one usually belongs in both. (The live *orchestration* is no longer cloned: Phase 3 unified it into one registry-dispatched `live/src/strategies/{service,runner,execution}`.)
- **Analysis-only death-close (`ExitReason::Dead`):** sim/grouped-sweep/detect mislabeled silent-death tokens as `Open` at a stale price — the analysis ladder only fires on an observed trade, and a token that dies by going quiet has none. The **analysis** `find_trade_driven_exit` (all 3 strategies) appends `.or_else(|| strategies::death::find_death_point(trades, entry_time, Utc::now()))`, closing the bag at the **death point** (last meaningful post-entry trade, `DEAD_MEANINGFUL_TRADE_SOL`). Deadness is the shared `token_cache::is_dead_verdict` SSOT — the SAME verdict live's `TokenState::is_dead` uses, so they can't drift. **Live paper reuses the SAME `find_death_point`**: `live`'s `StrategyService::sweep_dead_paper_exits` (runner 1s sweep, AFTER `sweep_time_exits`) closes **paper** `Holding` positions the ladder/time sweep left open (rules with no TimeStop/Stall) at the death point with reason `Dead`, so paper matches sim; the paper *poll* stays strict (`find_trade_driven_exit_live`, waits for a real fill). **Real is untouched** — force-closing a real bag on a deadness verdict is out of scope (a dead pool has no liquidity to sell into); real silent tokens still close only via the Stall/TimeStop clock sweep. Booked closed, not Open: `RunMetrics.n_exit_dead`, grouped-sweep `n_exit_dead` column (folded into `lab/migrations/0001_grouped_sweep.sql`), `ExitCode::Dead=9`.
- **Truncated logs drop trade legs:** the validator truncates a tx's logs past a byte limit, so the curve decoder's primary "Program data:" TradeEvent scan under-counts legs on multi-buy **bundle** txs (a 4-buy launch bundle logged only 3 events + "Log truncated"). `decode_curve_pb` (`ingest-laserstream`) recovers them from the **inner-instruction self-CPI events** (complete, not log-limited) when logs are empty OR truncated — never revert this to an `is_empty()`-only fallback (partial truncation slips through). The **AMM** path (`decode_pump_swap_trades_from_logs`) is still log-only and has the same latent gap.
- **`trades`↔`wallet_dict` resolution:** the address is stored only in `wallet_dict` (interned `wallet_id`), and there is **no FK** on `trades.wallet_id`, so a missing dict row must never hide a trade. All `trades` read paths **LEFT JOIN** `wallet_dict` with a `COALESCE(w.address, 'unknown:'||wallet_id)` fallback (`trade_repo.rs`) — never an INNER join. On the `lab` mirror, `wallet_dict` is **non-destructively merged** each sync (`db-incremental-sync.ps1`): server wins by id, but local-only ids the server has aged out are **preserved** (the lab retains trade history longer than the server's 7-day window, so a `TRUNCATE`+replace would re-orphan old days). An in-txn guard asserts every server id landed locally; residual orphans (ids the server re-minted/aged out) render as `unknown:<id>` and age out. (An INNER join + the old watermark-incremental dict sync once hid ~58% of the lab's trades — looked like "ingest missed transactions" but the rows were all present in `trades`.)
- `.env` required (see `.env.example`); secrets/keys there only, never in code.
