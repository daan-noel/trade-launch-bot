# Forge efficiency audit vs hunter — remaining work

Origin: a 6-agent parallel audit (2026-07-13) comparing **forge** against the more mature
**hunter** reference (same stack: actix-web + sqlx + Postgres + React/RTK-Query). This file
has been **pruned to only the not-yet-done work** and re-verified against the working tree on
**2026-07-14**. Completed items are collapsed into the ledger below — do not redo them.

Deploy target is a 2vCPU/4GB EC2 box where CLAUDE.md calls IO/RAM "load-bearing", so the
remaining items are all overhead cuts, not correctness fixes.

---

## Status ledger — DONE (do not redo)

Verified present in the current tree at the cited `file:line`.

- ✅ **Phase 0 — cheap cuts.** `setupListeners` + `skipPollingIfUnfocused` (`f903a79`),
  deduped `/api/ingest` double-poll (`0942644`), dropped dead OHLCV CAggs
  (`timescale.rs:25-31` now teardown-only, wired at `postgres.rs:90`), added
  `idx_launches_created (created_at DESC, id)` + `idx_managed_wallets_role_created
  (role, created_at DESC)` (`0001_init.sql:392,321`).
- ✅ **Phase 1 — frontend render.** Memoized `DataTable` rows + hoisted columns
  (`4c83844`), shared `useNow` + memoized `AgeCell` (`c8bdccf`), capped TokenDetail
  trades poll (`caf9dbb`).
- ✅ **Phase 2 (core) — ingest decoupling.** DB writer moved off the recv loop via a
  bounded `mpsc` (cap 16384) drained by `DbWriter` (`2037b8d`, `db_writer.rs:39`),
  OS-thread watchdog on a `DbHeartbeat` (`watchdog.rs:24-42`), and — better than the
  original plan — the `MAX_PENDING_ROWS` shed guard is **gone**: durable writes now
  await backpressure instead of dropping (`consumer.rs:105-107`).
- ✅ **Phase 3 — positions reconcile.** Off the request path with one grouped `GROUP BY`
  sum (`sum_sells_by_address_for_mint`, `positions.rs:220,311`) + one `UNNEST` batch
  update (`reconcile_batch`, `positions.rs:264,397`) (`3be9317`, plus Phase 1/2/3
  holdings follow-ups `cabce8c`/`322eb0a`/`331bfc9`).
- ✅ **Phase 4 (core) — SSE.** `SseHub` + `GET /api/stream` (`sse.rs`), frontend `sse.ts`
  multiplexer, pollers demoted to gap-heal (`d4d5ab2` + `9fadff2`). Emits
  `trade_executed` / `token_created` / `ingest_status` today.
- ✅ **§5 fees / confirm / funding-gate.** Tip/fee waste cut ~40-100× (`da9397b`),
  confirm watcher feed-notified over the 3s blind poll (`77dea07`), funding pass gated on
  the indexed `funded_count` before the treasury RPC snapshot (`d1d156b`,
  `wallet_funding.rs:292-307`).

---

## Remaining work (re-verified 2026-07-14, evidence = current `file:line`)

Four tranches, ordered by value/effort. Phase A is the last real double-poll class; B–D are
piecemeal overhead cuts.

### Phase A — Finish the SSE push surface (highest value)

The bus exists; two of the plan's event categories are still missing, so the Launch Console
and wallet pages still poll for status the backend could push.

- [ ] **A1 — Emit `launch`/`bundle` status over SSE.** `SseHub` publishes only
  `trade_executed` / `token_created` / `ingest_status` (`forge/live/src/sse.rs:83-121`;
  frontend `forge/frontend/src/shared/services/sse.ts:2-8`). Publish launch + bundle status
  transitions from the confirm watcher / launch state writes.
- [ ] **A2 — Emit wallet-funding events over SSE.** No funding event type exists. Publish
  from the funding pass (`forge/live/src/wallet_funding.rs`) so the wallet pool reflects
  fund progress without polling.
- [ ] **A3 — Demote the now-redundant pollers to gap-heal.** Once A1/A2 land, drop the
  Launch Console `/api/launches/{id}/status` 3s poll (`LaunchConsolePage.tsx:113`) and the
  `WalletPoolPage.tsx:136` 5s poll to slow (~30s) fallbacks, matching hunter's model.

### Phase B — Ingest observability

The heartbeat exists internally but nothing new is exposed, so a stall is invisible from
outside until the watchdog force-exits.

- [ ] **B1 — Surface ingest health on the status endpoint.** `IngestStatusResponse` is still
  only `{configured, live}` (`forge/live/src/http.rs:170-183`). Add commit heartbeat age,
  DB-writer buffer depth, and an event counter (hunter exposes `DbHeartbeat` + buffer/event
  metrics). Shed counters are N/A now that nothing is shed.

### Phase C — Task & RPC consolidation

- [ ] **C1 — Fold the same-table wallet-lifecycle pollers into one tick.** Still 6
  independent fixed-interval tasks, each its own `select!` arm
  (`forge/live/src/main.rs:165-186,313-318`): balance poller (30s), reservation sweep (30s),
  funding (60s), dust sweep (3600s), ladder evaluator, volume scheduler. The balance poller
  and reservation sweep touch the same `managed_wallets` table on overlapping cadences with
  no shared wake-up. Collapse the wallet-lifecycle loops into one `poll→promote→sweep→top-up`
  tick.
- [ ] **C2 — Read the cached balance instead of re-RPCing across 3 tasks.** Balance poller
  (`wallet_pool.rs:141`), funding (`wallet_funding.rs:177,693`), and dust sweep
  (`dust_sweep.rs:149`) each independently RPC-read balances; a DB cache
  (`managed_wallets.balance_lamports`) already exists. Read the cache first; issue a fresh
  `get_balance` only when stale or immediately pre-send.

### Phase D — Micro

- [ ] **D1 — Carry raw `u64` lamports through the ingest event.** The decoder divides the
  exact `u64` to an `f64` (`shared/ingest/pumpfun/src/decode/trade.rs:52`;
  `event.rs:26` `pub sol: f64`) and forge multiplies it back with `sol_to_lamports`
  (`forge/live/src/ingest/map.rs:61,63`). Add a raw-`u64` lamport field to the event and drop
  the round-trip.

---

## Cleared (not issues)

Connection-pool sizing, query folding in read handlers, batch UNNEST inserts / CAS upserts,
partial-index-bounded pollers, per-request spawn leaks, the `trades` DESC read path, and the
log-truncation decoder path (fixed on both curve + AMM in the shared decoder forge links).
