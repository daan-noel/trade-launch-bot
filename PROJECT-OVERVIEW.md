# Project Overview — big picture + implementation status

A meme-coin trading bot. It ingests every pump.fun trade in real time, runs
take-profit/stop-loss strategies that auto buy & sell, and gives you a web UI to watch
tokens, manage rules, and backtest. Built for huge token + trade volume — performance is
the top priority everywhere.

This doc is the map **plus** what is actually built. Status legend:
**✅ done** · **🟡 partial** · **🟥 stub/scaffold**.

---

## The one idea: **two boxes over a shared core**

The project is **two independent apps** that share one library.

| Box | Crate (bin) | Runs where | Keys? | Job | Status |
| --- | --- | --- | --- | --- | --- |
| **LIVE** | `live` | EC2 (24/7) | ✅ wallet + Helius gRPC | Ingest trades, run strategies, **execute real buys/sells** | ✅ done |
| **LAB** | `lab` | Workstation | ❌ none | **Analyze & backtest** — sweeps, simulations, swing analysis | ✅ done |

**LIVE makes money; LAB figures out how.** Shared code (config, DB, models, API
framework, SSE, strategy *decision logic*) lives in `trading_core`.

```
                         ┌────────────────────┐
                         │    trading_core     │  shared library (✅ ~21k LOC)
                         │ config·models·DB·API │
                         │ SSE · strategy logic │
                         └─────────┬──────────┘
            ┌────────────────────┘└────────────────────┐
            ▼                                            ▼
   ┌──────────────────┐                       ┌──────────────────┐
   │   LIVE  (✅ bin)  │                       │   LAB  (✅ bin)   │
   │ + pump-trader     │                       │ + rayon/arrow/    │
   │ + ingest-laserstream                      │   parquet/duckdb  │
   └──────────────────┘                       └──────────────────┘
```

---

## Crates — what's built (verified against source, not docs)

| Crate | Kind | LOC | Status | Reality |
| --- | --- | --- | --- | --- |
| `trading_core` | lib | ~21k | ✅ done | 13 repos with real SQL · 3 workload-isolated PG pools · actix API + auth + SSE bridge · TokenCache/TradeSignals · strategy decision logic. Zero `todo!`. |
| `pump-trader` | lib | ~6.7k | ✅ done | Real buy/sell executor: snipe-buy, sell-with-retries, durable nonces, prebuilt tx pool, Jito tip escalation, AMM (post-migration) routing, simulate. `probe`/`claim` are genuine off-by-default cargo features. |
| `ingest-laserstream` | lib | ~4.1k | ✅ done | Helius gRPC transport w/ auto-reconnect → decode (create/trade/migrate) → pipeline → DB writer. Watchdog/health with atomic counters. RPC backfill path included. |
| `ingest-websocket` | lib | ~40 | 🟥 **scaffold** | **Only real stub in the repo.** `spawn()` matches the contract but calls `unimplemented!()`. Exists so LIVE *could* swap transports later. Not wired anywhere. |
| `live` | **bin** | ~9.7k | ✅ done | StrategyRunner + unified strategy service, real execution (real + paper), pump-trader bridge, deploy routes, 10-subcommand `probe` CLI. Zero `todo!`. |
| `lab` | **bin** | ~13k | ✅ done | Param-sweep engine (rayon, grouped two-phase), backtest harness (both strategies), swing analyzer, Parquet "lake" pipeline, 15 API routes. Zero `todo!`. |

Dependency rules that hold in code: `trading_core` does **not** depend on `pump-trader`;
`lab` depends on **neither** `pump-trader` nor `ingest-laserstream` (no keys / no live feed
on the analysis box).

---

## Folder structure

```
meme-trading/
├─ trading_core/src/        # ✅ shared library
│  ├─ config/  models/  storage/   # settings/consts · row types · PG pools + 13 repos
│  ├─ services/  state/            # core services · CoreState, TokenCache, TradeSignals
│  ├─ strategies/                  # strategy DECISION logic (shared by live + lab)
│  ├─ api/                         # actix framework, auth, SSE, core handlers
│  └─ ingest.rs                    # transport-agnostic ingest CONTRACT
│
├─ pump-trader/src/         # ✅ buy/sell executor          (LIVE only)
├─ ingest-laserstream/src/  # ✅ gRPC live feed             (LIVE only)
├─ ingest-websocket/src/    # 🟥 scaffold — unimplemented!()
│
├─ live/src/                # ✅ LIVE bin
│  ├─ main.rs               #   composition root: ingest + strategies + trader + HTTP
│  ├─ strategies/           #   runner + unified service + execution (real/paper/scalp)
│  ├─ trader/               #   bridges pump-trader into the ingest contract
│  ├─ services/ state/ api/ #   deploy services · DeployState · deploy routes
│  └─ (probe subcommand in main.rs)
│
├─ lab/src/                 # ✅ LAB bin
│  ├─ main.rs               #   thin: SOL price + HTTP + lake-export CLI (no ingest/trader)
│  ├─ sweep/                #   param-sweep engine
│  ├─ analyzers/            #   swing detection, token analysis
│  ├─ lake/                 #   Parquet lake: export (PG→Parquet) + query (DuckDB)
│  └─ strategies/ state/ api/ storage/
│
├─ frontend-react/src/      # ✅ ONE repo, TWO apps
│  ├─ shared/ (@shared)     #   ~100 reusable components, SSE multiplexer, RTK base
│  ├─ live/   (@live)       #   LIVE app  → dev :5173
│  ├─ lab/    (@lab)        #   LAB app   → dev :5174
│  └─ pages/
│
├─ @arch/   @plans/         # subsystem deep-dives (reference only)
└─ CLAUDE.md                # working rules for this repo
```

---

## How a trade flows (LIVE hot path — ✅ live & running)

```
 Helius gRPC ──▶ ingest-laserstream ──▶ trades table (the feed) ──▶ TokenCache (in-memory)
   (every          (decode + pipeline)          │                          │
   pump.fun                                      │                          ▼
    trade)                                       │                  StrategyRunner
                                                 │            (reads cache, evals rules/event)
                                                 ▼                          │
                                          SSE bridge ──▶ frontend           ▼ buy/sell decision
                                        (live UI ticks)            pump-trader executes swap
                                                                            │
                                              sell-confirm reads the SAME gRPC feed
                                                       (NO extra RPC call)
```

Three hot-path rules that are **bugs if violated**:
- Sell-confirm reads the gRPC `trades` feed — never a fresh RPC poll (latency + double-sell).
- Ingest pipeline does no blocking I/O; DB/SSE writes go through channels only.
- Strategy eval reads the in-memory cache, never DB-per-event.

The `trades` table **is** the feed — single source of truth for confirmation, strategy
input, and the UI.

---

## How analysis flows (LAB path — ✅ built)

LAB has no live feed. It works off dumped data through a local Parquet "lake":

```
EC2 live DB ──(dump/restore script)──▶ local Postgres
                                            │
                              cargo run -p lab -- lake-export
                                            ▼
                                   Parquet "lake" (sealed days, immutable)
                                            │  ← DuckDB reads it (LakeSource)
   Sweep / Backtest / Swing  ◀─────────────┘     this is now the SOLE corpus source;
            │                                     the old Postgres corpus path was removed
            ▼
   "which tpsl params would've been most profitable over the last N days?"
```

Research loop: **dump live → restore locally → sweep in LAB → take winning rule → create
it in LIVE.**

---

## The strategies — ⚠️ not actually clones

Two strategies, **`tpsl_sniper_1`** and **`tpsl_sniper_2`**. Each is split into 3 layers so
live and backtest can't drift:

| Layer | Lives in | Status |
| --- | --- | --- |
| Decision logic (when to buy/sell) | `trading_core/strategies/` | ✅ written once, used by both edges |
| Live edge (real-time exec) | `live/strategies/` | ✅ real + paper modes |
| Backtest edge (replay) | `lab/strategies/` | ✅ both strategies |

> Reality check vs CLAUDE.md: CLAUDE.md calls them "intentional clones." In the code,
> **tpsl1 (~1.4k LOC) and tpsl2 (~2.8k LOC) are NOT clones** — tpsl2 adds a **scalp-entry
> gate** (arms and waits for a continuation signal before buying) and **cohort** feature
> logic on top of tpsl1's immediate-buy + exit ladder. They share *structure* and the exit
> ladder, so many fixes still apply to both — but tpsl2 is a superset, not a copy.

---

## The frontend — ✅ two apps, one repo

Mirrors the backend split; **mode is build-time, not runtime**.

| App | Alias | Dev | Proxies to | Shipped? | Status |
| --- | --- | --- | --- | --- | --- |
| LIVE | `@live/*` | `:5173` | live bin `:8081` | ✅ `npm run build` → `dist/` (lab-free) → EC2 | ✅ done |
| LAB | `@lab/*` | `:5174` | lab bin `:8082` | ❌ dev-only (`lab.html`) | ✅ done |

Real pages (all fetch data, no placeholders):

- **LIVE:** All Tokens (SSE token-created + live prices), Sync Token, TP/SL Sniper 1 & 2
  (rule CRUD + live positions over SSE), My/Other Wallets (holdings, buy/sell, cashback),
  Settings, Transactions (500-row live trade stream).
- **LAB:** Creation Stats (heatmap/trend), All Tokens (+ swing overlay), Swing Detection,
  TP/SL Sniper 1 & 2 (+ sim/paper-result analysis), Grouped Sweep TPSL1 & TPSL2, Settings.

SSE is one multiplexed `EventSource` → `/api/stream`, fanned out per event type
(`trade_executed`, `token_created`, `tpsl_*_changed`, `sweep_finished`, …). High-frequency
ticks (SOL/USD, live trades) are memoized so they don't trigger re-renders.

---

## Common commands

```powershell
cargo check -p live          # typecheck LIVE bin
cargo check -p lab           # typecheck LAB bin
cargo run   -p live          # LIVE: needs Postgres + Helius gRPC + keys
cargo run   -p lab           # LAB:  needs Postgres only
cargo run   -p lab -- lake-export                 # seal days → Parquet lake
cargo run   -p live -- probe <ladder|holdings|simulate-buy|cashback-status|...>

cd frontend-react
npm run dev                  # both apps (live :5173, lab :5174)
npm run build                # production LIVE build → dist/
```

---

## TL;DR

1. **`trading_core`** = shared brain (config, DB, models, API, SSE, strategy decisions). ✅
2. **`live`** = trading box on EC2 — gRPC ingest, strategies, real swaps via `pump-trader`. ✅
3. **`lab`** = analysis box on your laptop — sweeps/backtests over a local Parquet lake, no
   keys, no live feed. ✅
4. **frontend** = one React repo, two build-time apps (`@live`/`@lab`) over `@shared`. ✅
5. **Only unfinished piece:** `ingest-websocket` (🟥 scaffold, `unimplemented!()`) — a
   placeholder for a future alternate transport; nothing depends on it.
6. **tpsl_sniper_2 is a superset of tpsl_sniper_1** (scalp-entry + cohort), not a clone.
7. Workflow: **LAB finds the best params → you create that rule in LIVE → LIVE trades it.**
```
