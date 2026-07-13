# Forge efficiency audit vs hunter — 2026-07-13

A subsystem-by-subsystem comparison of **forge** against the more mature **hunter**
reference (same stack: actix-web + sqlx + Postgres + React/RTK-Query). Scope: find
places where forge is doing more work than it needs to, or has skipped an efficiency
pattern hunter already adopted. Produced from a 6-agent parallel audit
(real-time/SSE, backend handlers, ingest, background tasks, frontend, DB/storage).

## TL;DR — the through-line

Forge is essentially the **"scaffold" version of hunter**: correct, but it re-does on
every request/tick what hunter computes once and reuses. Three root patterns recur:

1. **Poll instead of push, everywhere.** Forge has *zero* SSE. The frontend polls REST
   every 3–5s; each handler re-queries Postgres; and a *second* set of backend tasks
   *also* polls the same DB rows every 3s to advance state. The same rows are polled
   from both directions with no in-process event bus. Hunter is push-first (SSE + a
   feed `ping` channel), with polling kept only as a slow gap-heal safety net.
2. **Inline heavy work on the hot path.** The ingest consumer does all DB I/O inline
   (no decoupled writer); the `/positions` handler does N sequential RPC calls + 3N
   DB queries *inside the request*. Hunter pushes both off the critical path.
3. **Recompute per tick what's already materialized.** Frontend rebuilds column
   arrays / re-renders every row on every poll; backend recounts full trade history
   (`count(*) OVER()`) when a denormalized counter exists; TimescaleDB continuous
   aggregates are refreshed forever but never read.

None of this is broken — it's overhead, and it matters specifically because the
deploy target is a 2vCPU/4GB EC2 box where CLAUDE.md calls IO/RAM "load-bearing."

## Fix-first shortlist (highest value / lowest effort)

| # | Fix | Effort | Why first |
|---|-----|--------|-----------|
| 1 | Call `setupListeners(store.dispatch)` + add `skipPollingIfUnfocused` | 1 line + N flags | Backgrounded tabs currently poll forever; this is the single cheapest cut |
| 2 | Add SSE stream (port hunter's `/api/stream` + `sse.ts`) | Medium | Eliminates the whole frontend↔backend double-poll class |
| 3 | Decouple ingest DB writer + port watchdog | Medium | Removes hot-path blocking + silent-stall risk (2× HIGH) |
| 4 | Move `/positions` reconcile off the request path (1 RPC + grouped SQL) | Small-Med | Turns an N+1 RPC request into O(1) |
| 5 | Add 2 indexes + page `/api/wallet_pool`; drop dead CAggs | Small | Cheap DB wins on the hot poll endpoints |

---

## 1. Real-time layer — no SSE, double-polling (CROSS-CUTTING)

Hunter: actix-web + a two-stage `tokio::sync::broadcast` bus → single
`GET /api/stream` SSE endpoint (**12 event types**, optional `?mint=` server filter),
consumed by **one shared browser `EventSource`** (`hunter/frontend/src/shared/services/sse.ts`)
that multiplexes to every page. Polling retained only as a 30s resync safety net.

Forge: **no SSE**. All real-time is RTK Query `pollingInterval`. Ten poll sites:

| Location | Endpoint | Interval | Stops? |
|---|---|---|---|
| `LaunchConsolePage.tsx:113` | `/api/launches/{id}/status` | 3s (dynamic) | ✅ only one that stops on terminal state |
| `DashboardPage.tsx:28`, `IngestToggle.tsx:10` | `/api/ingest` | 5s **×2** | ❌ duplicated pollers, identical data |
| `WalletPoolPage.tsx:136` | `/api/wallet_pool` | 5s | ❌ comment claims it stops; no gate — polls forever |
| `DashboardPage.tsx:26,27` | `/api/wallet_pool`, `/api/launches` | 15s | ❌ |
| `LaunchesPage.tsx:23` | `/api/launches` | 15s | ❌ |
| `TokenDetailPage.tsx:31` | `/api/tokens/{mint}/trades` | 10s | ❌ (see Frontend C3 — unbounded) |
| `VolumePanel.tsx:23` | `/api/tokens/{mint}/manage/volume` | 10s | ❌ |
| `AppShell.tsx:16` | `/api/quote_assets` | 60s | ❌ (fine — slow ref data) |

The backend has **no broadcast/watch channel** modeling status today (confirmed: only
an mpsc ingest pipe + control `watch<bool>` in the transport). Every status surface is a
DB row advanced by a background **polling** loop and read by a REST **poll**. An SSE
stream would need to be built from scratch — but the design ports near-verbatim from
hunter, and the ingest consumer + confirm watcher + balance poller are the natural
publish sites.

**Recommendation:** port hunter's SSE (backend bus + `/api/stream`, frontend `sse.ts`),
emit `ingest_status`, `launch/bundle status`, `trade_executed`, and wallet-funding
events. Then demote the pollers to slow gap-heal fallbacks (hunter's model). Also
collapse the dashboard's 3 pollers and the duplicated `/api/ingest` poll.

---

## 2. Frontend (React + RTK Query)

**CRITICAL**

- **C1 — `setupListeners` is never called.** `forge/frontend/src/app/store.ts:6-11`
  builds the store but never calls `setupListeners(store.dispatch)`. Hunter calls it
  (`hunter/frontend/src/live/store/index.ts:2,28`). Without it, `skipPollingIfUnfocused` /
  `refetchOnFocus` / `refetchOnReconnect` are all **inert app-wide**. *Fix: one import + one call.*
- **C2 — No `skipPollingIfUnfocused` anywhere.** Every poller keeps hitting the API in a
  hidden tab. Hunter sets it on every polling query. *Fix: add the flag (and land C1 so it takes effect).*
- **C3 — Full trade history polled every 10s, unmemoized + unvirtualized.**
  `TokenDetailPage.tsx:29-32` polls `useTokenTradesQuery({mint})` with **no `limit`** →
  backend returns the token's *entire* history, re-rendered through a non-paginated
  `DataTable` and re-fed to `PriceChart` every 10s. Hunter's equivalent full-history
  endpoint is a cold, unpolled path. *Fix: cap the query (`limit`) or make it on-demand;
  paginate/virtualize; memoize the chart series.*

**HIGH**

- **H1 — `DataTable` re-renders every row on every poll tick** (`shared/components/ui/DataTable.tsx:63-76`;
  inline closures, no row `memo`). `LaunchesPage` renders up to 100 rows at 15s. Hunter
  extracts a `memo`'d row relying on RTK structural sharing.
- **H2 — Column arrays + derivations rebuilt inline every render**
  (`DashboardPage.tsx:42-69`, `LaunchesPage.tsx:30-69`, `TokenDetailPage.tsx:143-181`, …).
  New array identity defeats any row memoization. Note forge *does* use `useMemo` in
  `WalletPoolPage.tsx:205` — pattern known, applied inconsistently. Hunter keeps columns
  module-scope + referentially stable.
- **H3 — No shared `useNow` ticker.** `formatAge` calls `Date.now()` in render
  (`shared/lib/format.ts:39-49`); ages freeze between polls and only advance on a full
  poll re-render. Hunter has one app-wide adaptive `useNow` + memoized `AgeCell`.

**MEDIUM**

- **M1 — Over-broad `Bootstrap` invalidation.** Every template/metadata/wallet mutation
  invalidates both the narrow tag *and* `Bootstrap` (`endpoints.ts:75-132`), refetching the
  whole composite on a narrow edit. Same facts also fetched twice (bootstrap vs standalone
  queries) with no cache sharing.
- **M2 — Missing `providesTags` on `tokenOverview`/`launch`/`launchStatus`/`tokenTrades`**
  (`endpoints.ts:168-185`) → the token header stays stale after `manageExecute` (which only
  invalidates `Positions`/`ManageActions`) until a remount.
- **M3 — SOL/USD price piggybacks the full `quoteAssets` dimensions list**
  (`AppShell.tsx:15-22`) just to read one `usd_rate`. Hunter has a dedicated `getSolPrice`.

**LOW**

- **L1** — no `shared/hooks/` dir at all; forge re-implements polling/visibility/now ad hoc.
- **L2** — `PriceChart.tsx:77-81` rebuilds the whole series (Map/sort/array) on every 10s poll.

---

## 3. Backend HTTP handlers

Mostly good — forge already adopted hunter's query-folding (`try_join!`, `count(*) OVER()`
page+count, clamped limits, `UNNEST` batch inserts). The one hotspot is the **position
reconcile path**, the single place forge did *not* adopt hunter's batch-one-RPC/off-critical-path pattern.

- **HIGH — N+1 RPC + per-row writes, inline in the request.**
  `forge/launcher/src/manage/positions.rs:119-139` loops positions and per wallet does:
  one `getTokenAccountsByOwner` RPC (10s timeout), one per-wallet `SUM` query, then **two**
  separate `UPDATE`s — all sequential, blocking the HTTP response, invoked from
  `GET /api/tokens/{mint}/positions` (`http.rs:830`) and `manage/preview` (`http.rs:848`).
  A slow/failing wallet `?`-aborts the whole pass. Hunter reconciles with **one** RPC scan +
  **one** query, off the request path at boot (`hunter/live/src/services/wallet_reconcile.rs:11-72`),
  and serves reads from a cached snapshot (`portfolio.rs:137`). *Fix: single
  `get_all_token_accounts_for_mint` RPC + `GROUP BY wallet` sum + `UNNEST` batch update, run in a background task.*
- **MED** — per-position `sum_side_quote_by_address` should be one `GROUP BY w.address` query (`positions.rs:135`, `feed.rs:146`).
- **MED** — two `UPDATE`s to the same row should be one (`positions.rs:130,138`).
- **LOW** — `manage/preview` pays the full reconcile on every click (`http.rs:846`).
- **LOW** — `seed_positions` does INSERT+SELECT per leg on every read (`positions.rs:70-90`).

---

## 4. Ingest pipeline

Forge and hunter link the **same** shared transport + decoder (`shared/ingest/*`), so the
reconnect/backoff/gap-replay loop is common. All divergence is in forge's host adapter,
which does everything inline in one task where hunter uses a decoupled writer + watchdog.

- **HIGH — Consumer does all DB I/O inline on the hot path**
  (`forge/live/src/ingest/consumer.rs:65-136`): wallet intern round-trip, per-row
  `TokenRepo::insert` + `MarketRepo::upsert`, and `flush()` are all awaited *in* the recv
  loop, so any DB stall stops draining the transport. The module doc itself flags this as
  scaffold. Hunter pushes `DbWriteOp`s into an `mpsc` (cap 16384) drained by a separate
  `DbWriter` task (`hunter/live/src/ingest/db_writer.rs`). *Fix: decouple the writer via a channel; batch/defer wallet interns.*
- **HIGH — No watchdog; a wedged DB silently stalls ingest.** No watchdog anywhere in
  `forge/live/src/ingest/`. Hunter runs an OS-thread watchdog on a `DbHeartbeat` that
  `process::exit(1)`s for the supervisor when writes stop (`hunter/live/src/ingest/watchdog.rs:95-101`).
  *Fix: port the heartbeat + watchdog (pairs with the decoupled writer above).*
- **MED — Overflow guard sheds *durable* trades/raws.** `MAX_PENDING_ROWS=50_000`
  drains the oldest `NewTrade`/`RawTx` on sustained flush failure (`consumer.rs:158,196-202`)
  — real, non-recomputable data dropped with a `warn!`. Hunter only drops *recomputable*
  metrics; durable ops use awaiting backpressure. *Fix: backpressure durable writes, shed only metrics.*
- **MED — Missing instrumentation:** only `{configured, live}` exposed (`http.rs:159-165`);
  no shed counters, commit heartbeat, buffer depth, or event count. Hunter has `ShedCounters` + `DbHeartbeat`.
- **LOW/MED** — redundant `f64` lamport round-trip per trade (`map.rs:61,63`): decoder had
  the exact `u64`, forge multiplies the `f64` back. *Fix: carry raw `u64` lamports through the event.*

**Cleared (stale memory note):** the log-truncation dropped-legs bug is **fixed on both
curve *and* AMM** in the shared decoder forge links (`shared/ingest/pumpfun/src/decode/grpc.rs:148-156,295-305`,
with parity tests). The `[[log-truncation-dropped-legs]]` memory's "AMM path still exposed"
is out of date for forge's current dependency.

---

## 5. Background / async tasks

Forge spawns **8 long-lived tasks** from `main.rs:124-160`, **6 of them fixed-interval
Postgres pollers**. Hunter consolidates its trading/exit path into **one** event-driven
`StrategyRunner` (`hunter/live/src/strategies/runner.rs:33-77`) fed by an ingest `ping`
channel + a single 1s sweep, and runs reconcile **once at boot**.

- **MED — Redundant on-chain balance reads across 3 tasks.** Balance poller, wallet
  funding (`wallet_funding.rs:177`), and dust sweep (`dust_sweep.rs:99`) each RPC-read
  balances independently; the treasury balance is fetched by ≥2 loops that never share the
  result, and a DB balance cache (`managed_wallets.balance_lamports`) already exists. *Fix:
  read the cached balance; fresh `get_balance` only when stale or pre-send.*
- **MED — Funding pass pays treasury RPC before the cheap warm-pool check.** `fund_once`
  calls `build_treasury_pool` (RPC per treasury) *before* computing the `target - funded_count`
  shortfall (`wallet_funding.rs:292` vs `:315`), so a fully warm pool still pays N RPC reads
  every 60s to do nothing. *Fix: gate on the indexed `funded_count` first.*
- **MED (architectural) — 6 independent fixed-interval pollers** with separate cadences +
  pool acquisitions; the balance poller and reservation sweep touch the *same*
  `managed_wallets` table on overlapping cadences without sharing a wake-up. *Fix: fold the
  same-table wallet-lifecycle loops into one tick (poll→promote→sweep→top-up).*
- **LOW-MED — Confirm watcher blind-polls every 3s forever** (`confirm.rs:31,48`) with no
  idle backoff, to notice a leg signature that the *in-process* ingest consumer wrote. This
  is the exact "notify over poll" case. *Fix: `tokio::Notify` from the consumer + slow fallback tick.*
- **LOW — N+1 in the confirm pass:** per-bundle `LaunchRepo::get` each tick (`confirm.rs:74`). *Fix: batch-load by launch-id set.*

**Cleared:** no missing `WHERE` filters (all pollers are partial-index-bounded), no
per-request spawns/leaks, and the balance poller already does adaptive idle backoff
(`wallet_pool.rs:104-108`) — a good model the others don't follow.

---

## 6. DB / storage layer

Pool setup is a near-verbatim copy of hunter (3 workload-isolated pools, session guards,
statement timeouts), sized down for the small box — **not a bottleneck**. Batch UNNEST
inserts, CAS upserts, and partial indexes are all present. Real findings:

- **MED-HIGH — Dead continuous aggregates.** `timescale.rs:25-126` creates + refreshes
  `trades_ohlcv_1m/5m/1h` (1m/5m/1h policies), but **zero readers** exist in the tree — all
  trade reads use the raw `trades_priced` view. On the RAM-constrained box the 1m CAgg
  re-scans recent chunks every minute and materializes 3 hypertables for nothing. *Fix:
  wire the candle endpoint to the CAgg, or drop CAgg creation until a consumer exists.*
- **MED — `/api/launches` sorts on an unindexed column every poll.** `list_page` does
  `ORDER BY l.created_at DESC` (`own_launch.rs:508-531`) but `0001_init.sql:385` indexes
  `(status, created_at DESC)`, `(mint_address)`, `(template_id)` — **not** `(created_at DESC)`.
  Forge even copies hunter's `(created_at DESC, …)` index for `tokens` but not `launches`.
  *Fix: `CREATE INDEX idx_launches_created ON launches (created_at DESC, id)`.*
- **MED — `/api/wallet_pool` fetches the whole table, unbounded + unindexed, every poll.**
  `list_all` = `SELECT * FROM managed_wallets ORDER BY created_at DESC` with no LIMIT and no
  `created_at` index (`own_launch.rs:68-82`); the set grows monotonically (retired wallets
  never deleted). *Fix: paginate + `CREATE INDEX ... (role, created_at DESC)`.*
- **MED — Launch-status poll recounts full mint history.** `find_priced_page_with_count`
  uses `count(*) OVER()` (`feed.rs:195-219`), reading every trade of the mint before
  `LIMIT 50`, every ~3s during a launch — when the total is already denormalized in
  `token_market_state.trade_count`. *Fix: plain `LIMIT` + read the count from the counter row.*
- **LOW** — `sum_side_quote_by_address` scans all of a mint's trades to sum one wallet
  (`feed.rs:146`); a `(mint_address, wallet_ref)` index would help if it ever goes hot.
- **LOW/informational** — repo comments cite non-existent migration numbers (0008/0012/0013);
  those indexes actually live in `0001_init.sql`. Harmless but misleading.

---

## Suggested phasing

- **Phase 0 (hours):** C1 `setupListeners` + `skipPollingIfUnfocused`; the 3 index/pagination
  DB fixes; drop dead CAggs; dedup the `/api/ingest` double-poll. Cheap, no architecture change.
- **Phase 1 (frontend render):** memoize `DataTable` rows + column arrays, port `useNow`,
  cap the trade-history query. Kills the per-tick render churn.
- **Phase 2 (ingest hardening):** decoupled `DbWriter` + watchdog + durable-write
  backpressure + shed/heartbeat instrumentation (ports directly from hunter).
- **Phase 3 (positions):** move reconcile off the request path with 1 RPC + grouped SQL + batch update.
- **Phase 4 (push):** port SSE (`/api/stream` + `sse.ts`); emit ingest/launch/trade/funding
  events; demote pollers to gap-heal fallbacks; consolidate the wallet-lifecycle pollers +
  make confirm feed-notified.

## Coverage note

Checked-and-cleared (not issues): connection-pool sizing, query folding in read handlers,
batch inserts / CAS upserts, partial-index-bounded pollers, per-request spawn leaks, the
`trades` DESC read path (backward index scan), and the log-truncation decoder path (fixed
on both venues). See per-subsystem "cleared" notes above.
