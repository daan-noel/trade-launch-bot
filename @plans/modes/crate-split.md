# Crate split — `backend` → `backend-deploy` + `backend-local`

Design + decision record for the workspace split that replaced the single binary
`backend` crate. Navigation map lives in [@arch/architecture.md](@arch/architecture.md);
this doc is the *why* and the patterns to follow when extending it.

## Why split

The one `backend` bin carried two disjoint workloads on the same box:

- **Live trading** — Helius LaserStream gRPC ingest + strategy eval + on-chain
  execution. RAM/latency-sensitive, runs on the 2vCPU/4GB EC2 box.
- **Analysis** — param sweeps + backtests + swing detection. CPU/RAM-heavy
  (`rayon`/`arrow`/`parquet`), runs on the workstation against a DB snapshot.

Shipping both meant the EC2 image dragged in `arrow`/`parquet`/`rayon` it never
ran, and the analysis box linked `tonic`/`prost`/`pump-trader` it never used. The
split makes each bin link **only** its half; the dep partition is enforced by the
verification matrix (`cargo tree` shows no cross-contamination).

## Final shape (five crates)

```
pump-constants ──┐
                 ├─► backend-core ──┬─► ingest-laserstream ──► backend-deploy (bin)
pump-trader ─────┘                  ├──────────────────────────► backend-deploy
                                    └─► backend-local (bin)
```

- **`pump-constants`** — leaf, zero deps. The tunable `COMPUTE_UNIT_*` + program-ID
  literals. Single-sourced so the backtest CU cost-model can't drift from the live
  trader. `pump-trader` re-exports it as `pump_trader::constants` (no caller churn);
  `backend-core` and `backend-local` read it **without** depending on `pump-trader`.
- **`backend-core`** — everything both bins share: `config`, `models`, `storage`,
  core services/state (`CoreState`), the actix api framework + auth + SSE bridge,
  the **core handlers**, and the **strategy domain** (`tpsl_rules_core`: rule repos +
  validation + DTOs). Depends on `pump-constants`, **not** `pump-trader`.
- **`ingest-laserstream`** — the gRPC live transport, behind `spawn(…)`. See below.
- **`backend-deploy`** (bin) — strategies, trader shim, deploy services/state
  (`DeployState`), live/trading handlers, `probe`. The only crate that links
  `pump-trader` + `ingest-laserstream`.
- **`backend-local`** (bin) — sweep/backtest, swing analyzer, local state
  (`LocalState`), rule-authoring + simulate + sweep handlers. Links `rayon`/`arrow`/
  `parquet`; **no** `pump-trader`, **no** `ingest-laserstream`.

## State: one fat struct → three narrow ones

`AppState` (every field an `Arc`/`PgPool`/`watch::Sender`) was split into:

- `CoreState` (in core) — pools, caches, sse channels, settings, sol-price, repo
  accessors. Shared, read by every handler.
- `DeployState` = `core: Arc<CoreState>` + trader, tpsl{1,2} caches, pool index,
  `trade_signals`, `sync_gate`, `live_mode`.
- `LocalState` = `core: Arc<CoreState>` + backtest/sweep/sim/swing caches, progress,
  cancels, `sweep_corpus_cache`.

Handlers take the **narrow** state they need (`web::Data<Arc<CoreState>>` /
`web::Data<DeployState>` / `web::Data<LocalState>`). The split was done **in place**
first (all four states cloned from the same handles, identical runtime) so each
handler could migrate one group at a time before the crates were physically separated.

## Strategy layering (apply when adding a strategy)

Three layers, so a new strategy is written once and cloned only where the runtime
side effect differs:

1. **Domain → `backend-core`** (`tpsl_rules_core`): rule repo + validation + DTO.
   The CRUD write+DTO body is a core helper, written once.
2. **Runtime edge (deploy)**: live runner, `cache.reload_rules`, live-count
   enrichment, lifecycle, positions. The deploy CRUD handler = core helper + a
   ~5-line wrapper whose **only** addition is `cache.reload_rules`.
3. **Runtime edge (local)**: simulate + paper-result. The local CRUD wrapper = core
   helper + nothing (no live cache to reload; the `is_live` guard is dropped locally).

Clones carry **only runtime wiring**, never business logic. `tpsl_sniper_1` /
`tpsl_sniper_2` stay intentional clones — a fix in one usually belongs in both.

## Ingest crate pattern (apply when adding an ingest source)

Each ingest source is its own workspace lib crate (`ingest-laserstream`, future
`ingest-websocket`, …) with:

- Deps: `backend-core` + its own transport crate(s) (here tonic/prost/tokio-stream).
- Public surface: `pub fn spawn(helius_laserstream_url, helius_api_key,
  pump_program_id, db, token_cache, sse_tx, settings_rx, live_rx,
  trader: Arc<dyn TraderHook>, trade_signals) -> IngestHandles` — wraps the
  client→pipeline→db_writer chain. `IngestHeartbeat` + the OS-thread watchdog live
  **inside** the crate (`ingest_health.rs`, started by `spawn`); `maintenance.rs`
  (partition upkeep) lives here too.
- `backend-deploy` links whichever ingest crate(s) it needs; unused ones never
  compile. No shared trait until runtime switching between sources is required.

## Frontend capability gating

Both bins serve `GET /api/system/capabilities` → `{has_live_trading, has_analysis}`
(deploy `true/false`, local `false/true`). The SPA fetches it once at boot
(`useCapabilities`) and gates nav + lazy routes so one build serves either backend:
live-trading routes (sync, transactions, wallet holdings, live-mode toggle) mount
only against deploy; analysis routes (analysis pages, strategy rule editor + sweeps)
only against local. Until the fetch resolves the gate holds everything back behind a
fallback; on error it fails **open** (both `true`).

## TimescaleDB coordination

Orthogonal refactor; adds no Rust deps. Sequence was **crate split first, Timescale
second**. Overlap files: `maintenance.rs` (now in `ingest-laserstream`; Timescale
deletes it), the two bins' `main.rs` `select!`, `trade_repo.rs` (core), and the
watchdog. Synergy once boxes are separate: per-box retention (deploy 7-day +
compression / local long corpora) is natural, and an OHLC continuous-aggregate candle
endpoint would land in core. See [timescaledb-plan.md](../../timescaledb-plan.md).
