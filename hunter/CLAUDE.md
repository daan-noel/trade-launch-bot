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
[docs/plans/strategies/fill-and-cost-models.md](docs/plans/strategies/fill-and-cost-models.md)
(the Fill / Cost dropdowns explained with worked numbers — what each mode picks or charges),
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

## PnL % is money over capital (locked)

**A percent shown beside a SOL figure is that SOL over the capital that produced it** —
`weighted_return_pct(Σ pnl_sol, Σ entry_sol)` (`strategies::kernel`, the ONE formula, at
every grain: position / rule / run / window). Never a **price ratio** (it charges no
execution cost — break-even is ~4% of price move — so it renders green beside a red ◎)
and never a **mean of other percents** (`buy_amount_lamports` is editable mid-run, so
notionals vary; count-weighting lets a 0.05 ◎ rule outvote a 1.0 ◎ one). The denominator
is `closed_entry_sol`, **not** `total_entry_sol` — the latter includes open positions.
An aggregate re-weights by carrying both sums, never by averaging children's percents,
which is why the counters + summary wire ships `closed_entry_sol`. Sign-lock is the
invariant to protect: percent and ◎ can never point opposite ways. The deliberate
exception is `RunMetrics::mean_pnl_pct` (equal-weighted; exact under a backtest's fixed
notional). Full rationale, the four fixed defects, and the residual gap:
[docs/plans/strategies/pnl-percent-definition.md](docs/plans/strategies/pnl-percent-definition.md).

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
- **Both migration chains are squashed to a single `0001_init.sql`** (core +
  lab). A new migration is a new `00NN_*.sql`; re-squashing later invalidates every
  already-migrated database's ledger and needs a one-off
  `scripts/consolidate-migration-ledgers.ps1` run per DB (**EC2 before the next
  `db-incremental-sync.ps1`**, which copies the server ledger into the local
  mirror). Rules + the verification step: `docs/plans/database/db-patterns.md`.
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
storage.

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
check — today: the rule editor's **Max total** and Trader Analysis **Max tokens**
(blank/`∞`).

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
- **A warm start may rebuild state, never re-decide the past.** The producer's trade
  cursor is RAM-only and the cache seed backfills up to 500 historical trades per mint,
  so after a restart a token's whole past reads as new. Deciding on it is not cosmetic:
  `reduce` evaluates a trade at `trade.at`, so the decision uses the old price and the
  fill uses the live one — that sold a real position's last leg on a `stop_loss` that
  had been true five minutes earlier, at **+79%** (2026-08-06). Cached trades older than
  the loop's `started_at` go through `hunter_engine::prime_trade` (fold the track, emit
  nothing, don't log); only newer ones become `Event::Trade`. The 200 ms tick re-decides,
  so priming defers a decision rather than dropping it. The mirror-image failure is just
  as real: an adopted position on a token that never prints again keeps a `NaN` price, and
  `NaN` satisfies no condition — no TP/SL, no trail, **no dead-close** — so `prime_tracked`
  must keep retrying from the tick until the async seed lands. Full audit:
  [docs/plans/strategies/restart-state-restoration.md](docs/plans/strategies/restart-state-restoration.md).
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
- **A retry is a NEW trade decision, so it must re-clear the gate that authorized the
  first one.** `reduce`'s `FillFailed` → `EntryPending` branch re-checks
  `entry_enabled && can_enter` at the failure's `at` before re-submitting; without it a
  buy is authorized once and re-fired blind for as long as the attempt ladder lasts.
  Confirming a revert is *slow* (12.3 s measured), which is exactly when the market has
  moved most: on 2026-08-07 an `entry liquidity > 10` rule decided at 14.65 SOL, reverted
  6042 (slippage), and the blind retry filled at **0.276 SOL** — 36x under its own floor.
  `can_enter` does **not** cover `entry_enabled`, so both are checked (`decide_arm` gates
  them separately, and a stopped rule stays loaded as a drain rule). This is why
  `Event::FillFailed` carries `at` — it was the only event with no clock. Failing the
  re-check re-arms rather than terminating (only an exhausted ladder / `Fatal` is
  `Done`), so the ONE gate re-decides and the gates a retry cannot express —
  `exclusive` (which means *wait*), `dead`, `entry_unsatisfiable` — still apply. Full
  rules: [docs/arch/position-lifecycle.md](docs/arch/position-lifecycle.md).
- **`peak`/`trough` are position-scoped — only prices the position lived through may
  move them.** `fold_entered_extremes` skips any arm whose `entered_at` is newer than
  the trade being folded. This is a no-op live (every event postdates the fill) and
  exists for the **restart** path: priming replays the token-cache seed, which reaches
  back `SEED_TRADES_MAX_AGE_HOURS` and therefore spans trades from before an adopted
  position entered. Without the guard a dip-entry bag inherits the run-up it
  deliberately did not buy and its trailing stop fires on a peak it never held — the
  exact inverse of the re-anchoring bug the priming rail was built to fix. Detail:
  [docs/plans/strategies/restart-state-restoration.md](docs/plans/strategies/restart-state-restoration.md).
- **The copycat guard is a global switch with per-mode memory, and it records
  attempts, not fills.** `strategy.skip_duplicate_identity` (an `app_settings` bool,
  enforced inside `decide_arm` — not at submit time like `max_committed_sol`, so
  live-real / live-paper / simulate share the one decision path) refuses an entry when a
  **different** mint with the same normalized `(name, symbol)` was traded inside
  `strategy.duplicate_identity_window_hours` (default 168). Four invariants, each of
  which is a bug if inverted: **(a)** the memory is written by every rule and every
  manual buy, so the switch is global — a per-rule flag would let one rule silently
  change another's gate; **(b)** the record is written at the entry *attempt*
  (`rollback_entry` does not undo it), because a copycat that reverts our buy is the
  trap worth remembering and confirming a revert takes ~12 s; **(c)** entries carry
  their mint and only a *different* mint is blocked — recording otherwise poisons the
  token's own retry ladder; **(d)** paper and real keep separate memories, so a paper
  experiment can never narrow what the real rules may buy. Blocking is a **`Disarm`**
  (`DisarmReason::DuplicateIdentity`), not `exclusive`'s wait — the block outlives any
  curve token. `hunter_engine::token_identity_hash` is the ONE hasher (live producer,
  lake exporter, replay); a private copy on any side is the bug the module exists to
  prevent. Every skip is a `warn!` with the mint: a silently skipped entry is
  indistinguishable from a rule that never fired. The grouped sweep **cannot** honor it
  (no cross-token state — divergence D7 in
  [docs/plans/sweep/sim-parity.md](docs/plans/sweep/sim-parity.md)). **Simulate does**:
  it inherits the same `app_settings` value unless the request overrides it
  (`EngineSimRequest.skip_duplicate_identity`), and stamps the resolved policy on the
  run (`SimMeta::dupe_guard_window_hours`) — a backtest whose result depends on
  ambient state it does not record is how the pre-2026-07-28 cost runs became
  incomparable. Note `app_settings` is **per database**: co-hosted live+lab share one
  row (one `DATABASE_URL` in `deploy/hunter.compose.yml`), but EC2 and the workstation
  do not — `db-incremental-sync.ps1` copies data tables, never settings.
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
- **Flow hash SSOT:** `hunter_engine::metrics::flow_split::{ix_hash,wallet_hash,ix_hash_opt,ix_hash_from_labels_json}`
  are the only hashers for volume/organic classification. Live producer, lake replay, and
  event-log adapters must call them — never roll a private FNV/string join. Patterns compile
  to a hash set at `RulesReloaded`. The offline paths hash **at load**
  (`projection::FlowKeys`, resolved in `lake/duck.rs` + `project_pg_tail`), not per fold —
  `to_trade_lite` must stay a pure field move. The raw label/wallet **text** rides only on a
  `Selection::with_flow_text` load, which is flow *discovery* and nothing else. See
  [docs/plans/strategies/metrics-reference.md](docs/plans/strategies/metrics-reference.md).
- **A `Tick` may skip a token; it may never skip a decision.** `reduce` skips tokens marked
  `Settled` — evaluated at or past every clock their rules read (`arm::ClockHorizons`) with
  no cross-token input moved since. This is what stops a token that can never go dead (real
  reserves ≥ 30, or no reserve reading at all ⇒ `NaN` liquidity ⇒ "alive") from being swept
  5x/second for the rest of a run; it was the dominant cost of a multi-day simulate
  (~180x measured, `engine/tests/tick_bench.rs`). Three rules when editing the fold:
  a **new metric that moves on a bare tick** needs a `ClockHorizons` field; a **new
  cross-token input** a decision reads must bump `cross_epoch` (go through
  `with_counters` / `record_identity`, the ONE write paths); anything mutating a tracked
  token **outside** the evaluate sweep must `unsettle()` (in-fold) or `touch_token`
  (live boot adoption). `dense_ticks` turns it all off for bisecting, and
  `engine/tests/settled_ticks.rs` is the differential guard. Full rationale:
  [docs/plans/strategies/tick-cost-and-settled-tokens.md](docs/plans/strategies/tick-cost-and-settled-tokens.md).
- **A trailing-window read is O(1) — keep it that way.** `flow_window` / `flow_split`
  maintain running sums over a **time-sorted** deque and correct only the two
  out-of-window ends on read. They used to rescan the whole buffer *and* re-derive the
  window width per element, which a flow-split rule paid once per metric per rule per
  event. Never reintroduce a full-buffer scan in a `value()`, and never assume the caller
  evicted at `now` (`TokenCreated`/`FirstSlotSettled` do not, and a skipped tick leaves
  entries un-evicted by design).
