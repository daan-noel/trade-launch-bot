# CLAUDE.md — hunter

Meme-coin trading bot — **massive token + trade volume; performance outranks everything.**
Read [../CLAUDE.md](../CLAUDE.md) first (SSOT, latency, Helius budget, EC2, docs
discipline). This file is hunter-specific only.

## Architecture — 5 crates, 2 bins + the SPA

| Crate | Kind | Role |
| --- | --- | --- |
| `trading_core` | lib | config, models, storage, core services/state (`CoreState`), api framework + auth + SSE bridge, strategy domain, **ingest contract** (`trading_core::ingest`) |
| `hunter-engine` | lib | the pure decision fold (`reduce`) — no clock, no I/O |
| `pump-trader` (pkg `executor-pumpfun`) | lib | buy/sell executor; standalone drop-in in `shared/`, signs via `Arc<dyn Signer>` |
| `ingest-laserstream` (pkg `ingest-pumpfun`) | lib | Helius LaserStream gRPC transport + watchdog; standalone drop-in, bridged onto `trading_core::ingest` by `live`'s host adapter |
| `live` | **bin** | LIVE box: strategies, trader, `DeployState`, `probe`. Ships to EC2 |
| `lab` | **bin** | ANALYSIS box: sweep/backtest, replay/simulate, `LocalState`. NO keys / NO gRPC; never depends on the executor |

Each bin is its own composition root (`tokio::select!`). Helius LaserStream is the **sole**
live transport; the `trades` table *is* that feed. The frontend is two apps over a shared
core mirroring the split (`src/shared` · `src/live` · `src/lab`, `@live`/`@lab` aliases) —
mode is **build-time**, never a runtime capability check.

**Read `docs/arch/` instead of re-exploring source; deep dives live in `docs/plans/`.**

| Doc | Covers |
| --- | --- |
| [docs/arch/architecture.md](docs/arch/architecture.md) | crate map, both `main.rs` wirings, the three state structs |
| [docs/arch/strategies.md](docs/arch/strategies.md) | the engine: pure fold + live adapters, **the ONE-kernel contract** |
| [docs/arch/position-lifecycle.md](docs/arch/position-lifecycle.md) | arm → entry → fill → exit, retries, slot accounting |
| [docs/arch/ingest.md](docs/arch/ingest.md) | client→pipeline→db_writer + host adapter |
| [docs/arch/trade-execution.md](docs/arch/trade-execution.md) | executor module map, revert self-heal |
| [docs/arch/database.md](docs/arch/database.md) | Postgres schema, pools, every repo→table→fns, unit rules |
| [docs/arch/frontend.md](docs/arch/frontend.md) | pages, components, hooks, RTK Query/SSE |
| [docs/arch/sweep.md](docs/arch/sweep.md) | param-sweep engine, grouping, persistence, API |

Read before trusting any backtest number:
[execution-costs.md](docs/plans/strategies/execution-costs.md) (what a round trip costs) and
[fill-and-cost-models.md](docs/plans/strategies/fill-and-cost-models.md). Open work lives in
[`docs/roadmap/`](docs/roadmap/), never in `docs/plans/`.

**Searching for a new trading rule? Start at
[convexity-search-workflow.md](docs/plans/strategies/convexity-search-workflow.md)** - a
self-contained guide to the purpose, the method, the measurement rules, the cost model, the
gate checklist and the pitfalls. It needs no other file. The islands it has produced so far
are in [island-map.md](docs/plans/strategies/island-map.md).

## Commands

```powershell
cargo check -p hunter-live             # or -p hunter-lab / -p hunter-core
cargo test  -p hunter-live             # add -- --ignored for integration (needs DATABASE_URL + HELIUS_RPC_URL)
cargo run   -p hunter-live             # live box: .env + Postgres + Helius gRPC   (LIVE_PORT :8130)
cargo run   -p hunter-lab              # analysis box: Postgres only, no keys/gRPC (LAB_PORT  :8140)
cargo run   -p hunter-lab  -- lake-export         # sealed days: local PG -> Parquet lake ($SWEEP_LAKE_DIR)
cargo run   -p hunter-live -- probe <ladder|fanout|pin-senders|simulate-*|sim-matrix|holdings>
cd frontend; npm run dev               # both apps: live :5173, lab :5174
npm run build:live                     # tsc (BOTH trees) + vite → lab-free dist/index.html (the EC2 artifact)
npm run lint                           # import-boundary gate ONLY: shared⊬@live/@lab, live⊬@lab, lab⊬@live
npm test                               # tsc -p tsconfig.test.json + vitest — the ONLY typecheck covering the tests
```

`tsconfig.json` (what `build:*` runs) **excludes every test file**: the UI image's build context is
`hunter/frontend/` alone, so a test reading a sibling crate's fixture — the `flow_ix_parity.json`
the Rust suite shares — cannot resolve on the server. Tests typecheck via `tsconfig.test.json`,
which `vitest.config.ts` also feeds to `vite-tsconfig-paths` for the `@shared`/`components` aliases.

Clippy `too_many_arguments` is `#[allow]`-ed on trade-path fns by design.

## Locked rules

- **The token-data key is `mint_address`** — one name across DB columns, Rust/TS DTOs, the
  filter/sort grammar, and frontend column keys. A bare `mint` on a token-data path is a
  bug. Reuse the existing anchors, never re-derive: `TradeRow::chart_spot_price`,
  `MARKET_CAP_SQL`, `market_cap_sol`,
  `config::constants::{sol_to_lamports,lamports_to_sol}`, the lake `schema.rs` column
  names, `token_enrichment::ENRICH_SELECT`, the TS `TokenEnrichmentFields` base. The
  executor, ingest and `lake/` crates keep their own decoupled `mint` vocabulary.
- **What a trade is DISPLAYED at comes from `TradeRow::chart_spot_price`** — reserve-pair
  spot, execution price only as the last rung. The raw `price_per_token` column is the
  *execution* price and is NOT that: for a buy it is the average along the curve, below
  the post-trade spot the chart plots, so anything shown next to a bar and derived from
  it (ATH, current price, market cap) silently disagrees with that bar. The frontend twin
  is `tradeSpotPriceSol` (`chartBars.ts`).
- **What a trade is TRADED at is `price_per_token`, and that is what every metric folds.**
  `TradeLite::price` and `Fill::price` are execution prices on all three adapters (live
  `producers`, the lab's `to_trade_lite`, the readout), so `m_price_lifetime`,
  `m_price_window` and `m_position` read one series — the one a rule can actually
  transact at, and the one a position is marked against. The two series differ by the
  trade's own impact (`B/vsol`), so **never mix them**: a gate derived against chart spot
  does not price the same in the engine, and vice versa. Deriving offline:
  [island-search.md](docs/plans/strategies/island-search.md).
- **Every SOL amount names its unit.** `_lamports` = exact integer (`BIGINT`/`i64`/`u64`),
  `_sol` = human `f64`; same base name across layers, converted only at the repo boundary
  through the one shared pair. Ratios keep `_price`/`_pct`. Rules + rationale:
  [docs/arch/database.md](docs/arch/database.md).
- **PnL % is money over capital** — `weighted_return_pct(Σ pnl_sol, Σ entry_sol)` at every
  grain, denominator `closed_entry_sol`. Never a price ratio (charges no execution cost)
  and never a mean of percents. Aggregates re-weight by carrying both sums. Percent and ◎
  can never point opposite ways.
  [pnl-percent-definition.md](docs/plans/strategies/pnl-percent-definition.md).
- **A raw on-chain `u64` arg is never narrowed** — `max_sol_cost = u64::MAX` is a *sentinel*
  ("fill at any price"), not an amount. One decode seam (`extract_lamports`), a ceiling is
  its own group, `float8` is banned where the value can pass 2^53, and the wire form is a
  JSON string. [u64-instruction-args.md](docs/plans/database/u64-instruction-args.md).
- **`0` means "off" only where 0 is not a valid value** — the two governance caps, decoded
  through the one `hunter_engine::Cap` reader. Anything *measured* uses `Option`/NULL, and
  an empty collection is the same sentinel as absent. One sentinel, ONE reader: two readers
  fail in opposite directions.
  [sentinel-and-zero-encoding.md](docs/plans/strategies/sentinel-and-zero-encoding.md).
- **The query string is shared state — patch it, never rebuild it.** Delete/set only the
  keys you own and use the functional `setParams(prev => …, { replace: true })`; building
  from empty is correct only in an href builder.
  [frontend-patterns.md](docs/plans/frontend/frontend-patterns.md).

## ONE decision kernel (root rule)

Live-real, live-paper and single-rule simulate are **literally the same code** —
`hunter-engine::reduce`. Real vs paper fork only at the fill layer, simulate only at who
feeds events. Never add a second decision path or a per-strategy clone. The grouped sweep
is the ONE sanctioned approximation, and it is a **ranking screener, not a backtest** —
re-run a promoted combo through simulate before trusting its PnL. Full contract + the
recorded divergences: [docs/arch/strategies.md](docs/arch/strategies.md).

## Performance budgets (hot path — violation = bug)

- **Sell-confirm:** no new RPC call — confirm via the `trades` feed. The exit loop polls the
  **full** window before retrying, buffering the feed's index lag (`exec_real.rs`). Once
  that window is spent and the signature is known landed, one batched `getTransaction`
  heals the missing leg into `trades` rather than losing the fill — the degraded branch
  only, never per attempt.
- **Ingest:** no blocking I/O, `.await`-on-lock, or unbounded per-event alloc; DB/SSE writes
  go through channels.
- **Strategy eval:** read the in-memory `TokenCache` (`token_cache.rs`) and engine state,
  never DB-per-event.

## Data-scale guardrails

- Bound every query — paginate/time-window/stream; never `SELECT *` on `trades`/`raw_txs`.
  Hot tables are TimescaleDB hypertables with declarative compression + retention. There is
  no partition-maintenance loop, and adding one fights the retention policies. <!-- ref-ok: absence is the rule -->
- **Both migration chains are squashed to `0001_init.sql`.** A new migration is a new
  `00NN_*.sql`. Re-squashing invalidates every migrated DB's ledger; the recovery rewrites
  the **ledger only**, so run `scripts/squash-catchup.sql` → reconcile → redeploy, per DB,
  **EC2 before the next `db-incremental-sync.ps1`**.
  [db-patterns.md](docs/plans/database/db-patterns.md).
- **Trade history: lake vs PG.** Simulate and all `lab` analysis read the sealed Parquet
  lake; two indexed lookups plus one deliberate full-history carve-out
  (`GET /api/tokens/:mint/trades`, `limit<=0` ⇒ no LIMIT) stay on PG — don't re-add a row
  cap. `MAX_TRADES_RETAINED` is the live in-RAM trim, never an analysis bound.
  [lake-pg-read-paths.md](docs/plans/database/lake-pg-read-paths.md).
- **`/api/tokens` differs by bin** (same wire contract): `live` pages from Postgres, `lab`
  runs the in-RAM engine over a snapshot. `SEED_TRACKING_LIMIT` is the tracking-cache seed
  cap, not the list cap. [token-list-backend.md](docs/plans/frontend/token-list-backend.md).

## Hot-path landmines

The rule is here; the linked doc carries the mechanism. Read the doc before editing the code it names.

| Landmine | The rule |
| --- | --- |
| Backtest honesty | Default `CostModelKind::pumpfun_impact` — the only kind that charges our own price impact, so the only one whose cost responds to buy size. Fee **125 bps/leg**; cost is U-shaped, so the optimal fixed buy is `sqrt(fixed_per_leg × vsol)`. Runs stored before 2026-07-28 are priced at 100 bps with no impact charge and do not compare. <!-- pt-ok: cutoff, those runs are still in the DB --> [costs](docs/plans/strategies/execution-costs.md) |
| Exit conditions that lie | `m_price_lifetime.stall` is *seconds since the last all-time high* — it caps every hold and doubles as an entry filter. Use `m_position.held` for a time stop, and `arm_above_pct` to arm a trail (an unarmed `retrace` is a hard stop from entry). [traps](docs/plans/strategies/flow-scalper-findings.md) |
| Restart rebuilds state, never re-decides it | Cached trades older than the loop's `started_at` prime the fold and emit nothing; deciding on them prices at the old trade and fills at the live one. Inversely, an adopted position with no new prints holds `NaN` — which satisfies no exit at all — so priming retries until the seed lands. Only prices the position lived through may move `peak`/`trough`. [restart](docs/plans/strategies/restart-state-restoration.md) |
| A cleared bag is a landed sell | "Bag gone, feed shows no sell" means the feed MISSED it — never price that as zero. Heal `trades` from the row's `exit_tx_signatures` first; still unresolvable ⇒ **park**, never a `−100%` `End`. An RPC failure in the bag check returns `Unchanged`, never `Cleared`. A row keeps its OWN `exit_reason`; `"Manual"` is only the no-reason fallback. [lifecycle](docs/arch/position-lifecycle.md) |
| A leaked slot is permanent | Every exit from the buy path MUST emit `FillConfirmed`/`FillFailed` — a bare `return` strands the arm in `EntryPending`, boot re-adopts it, and the concurrency slot is lost across restarts. Keep the send bounded. A retry is a NEW decision: re-check `entry_enabled && can_enter` before re-firing. [lifecycle](docs/arch/position-lifecycle.md) |
| Copycat guard | `skip_duplicate_identity` blocks a *different* mint sharing a normalized `(name, symbol)`. Global, records at the entry **attempt**, exempts the recording mint, separate paper/real memories, and `app_settings` is **per database**. [engine](docs/arch/strategies.md) |
| A tick may skip a token, never a decision | `reduce` skips `Settled` tokens (~180x on a multi-day simulate). New tick-moving metric ⇒ a `ClockHorizons` field; new cross-token input ⇒ bump `cross_epoch`; mutating a tracked token outside the sweep ⇒ `unsettle()`/`touch_token`. [ticks](docs/plans/strategies/tick-cost-and-settled-tokens.md) |
| Trailing windows are O(1) | Never rescan the buffer inside a `value()`, and never assume the caller evicted at `now` — and give every windowed group an arm in **both** `TokenTrack::on_trade` and `TokenTrack::on_tick`, or its buffers only ever shrink on their own next match. Reads are corrected at both window ends, so a missing tick arm is invisible in the numbers and shows only as retained state. Flow classification hashes come from the ONE `flow_ix` hasher set, applied at load offline. [metrics](docs/plans/strategies/metrics-reference.md) |
| A loaded fingerprint is a cost on EVERY token | Anything `TokenTrack` keys by `FingerprintId` (`flow`, `dump`) is opened once per loaded fingerprint per token and folded on every trade of that token, so the working set — not the `fingerprints` table — is what may reach `EngineState`. `reload` narrows `fps` to the ids the rules name and compiles each `metric_config` ONCE into `metrics::FingerprintPatterns`; deriving patterns where they are *used* re-walks the JSON and re-hashes every label sequence on the create fast lane, which is the serialized decision loop. Measured at 115 rows / 59 configured: 461 us per `TokenCreated`, of which 415 us was paid with **zero active rules**. |
| A registered metric with no compute arm is always-false | `TokenTrack::value` routes by group and is exhaustive, but each group's own `value(id)` ends in `_ => f64::NAN`, and `NaN` satisfies nothing — so a metric in `REGISTRY` whose group arm was never written is a gate that silently never fires. `engine/tests/every_metric_is_live_reachable.rs` walks `REGISTRY` and reads every metric through `reduce` + `read_state` on EVERY window basis (seconds, slots, prints); a new metric is covered without touching it, and a new **required** strict param fails it until taught a value. |
| A DashMap guard at an `.await` wedges the process | dashmap 4's shard lock is an **unbounded spinlock**, so a guard alive across `.await` lets the worker pick up another task, hit the same shard, and spin at 100% CPU in a non-async loop — it never returns to the scheduler to poll the future that would release the guard. Both workers wedge, every task stops, the watchdog force-exits at 90 s, and the hole in `trades` is never replayed. Copy the fields out, drop the guard, then await (`DeadFlush` in `token_cache.rs`). `scripts/check-async-guards.sh` gates it. [history](docs/history/2026-08-28-token-cache-eviction-spinlock.md) |
| A silent shed hides an outage | A wedged engine sheds 100% of pings while ingest keeps writing, so every external signal looks healthy while no rule is evaluated. Any `try_send`-and-drop on a trade-deciding path must be **loud**. [backpressure](docs/plans/ingest/backpressure-watchdog.md) |
| Boot must be bounded | An unbounded recovery scan starves the runtime and the watchdog then kills a booting process. Diagnostic: **`strategy engine loop running` absent from the log = the engine never started**, however healthy ingest looks. [engine](docs/arch/strategies.md) |
| Dead pools book a Dead exit | Sim and sweep close silent-death tokens from the shared deadness verdict rather than booking them `Open` at a stale price; a dead **real** pool has no liquidity to sell into. [engine](docs/arch/strategies.md) |
| Truncated logs drop legs | The validator truncates logs past a byte limit, so leg recovery from inner-instruction self-CPI events must fire when logs are empty **or** truncated — never an `is_empty()`-only fallback. [ingest](docs/arch/ingest.md) |
| `trades` ↔ `wallet_dict` | The address lives only in `wallet_dict` (no FK), so every read path **LEFT JOIN**s with a `COALESCE` fallback. An INNER join hides trades wholesale and reads exactly like an ingest miss. [database](docs/arch/database.md) |
| Revert self-heal is one SSOT | `classify_swap_revert` decides route × direction × error code for both crates — sell loop and snipe retry import it, never a local copy. [execution](docs/arch/trade-execution.md) |

## Deployed server (EC2: 2vCPU / 4GB)

Ship `live` + ingest only — `lab` (sweep/arrow/parquet/rayon + bundled `duckdb` + the lake
pipeline) stays on the workstation. Sweeps and backtests are local-only; the server keeps a
7-day rolling ingest buffer and analysis pulls it down with
`scripts/db-incremental-sync.ps1`. Never raise `MAX_TRADES_RETAINED`,
`SEED_TRACKING_LIMIT`, or cache TTLs there.

## Definition of done

- **Backend:** `cargo check -p hunter-live` + `-p hunter-lab` clean; clippy on touched code;
  test when logic changed.
- **Frontend:** `npm run build:live` and `npm run lint` clean (cross a boundary and you
  relocate the code, not the import); no extra re-render on SOL/USD tick or trade stream.
- **Docs:** update the tier that changed, present-tense (see [../CLAUDE.md](../CLAUDE.md)).
  A `docs/history/` entry only when the past left a live consequence — never per bug fix.
