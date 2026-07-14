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

### Phase A — Finish the SSE push surface (highest value) ✅ DONE 2026-07-14

The bus exists; two of the plan's event categories are still missing, so the Launch Console
and wallet pages still poll for status the backend could push.

Decoupling seam: the confirm watcher + wallet funder live in the `launcher` crate, which
can't see the live bin's `SseHub`. Added a crate-neutral `EventSink` trait
(`launcher/src/events.rs`, `LaunchStatusEvent`) that `SseHub` implements (`sse.rs`), passed
as `Option<Arc<dyn EventSink>>` into `spawn_bundle_confirm_watcher` / `spawn_wallet_funding`
(+ threaded through `fund_once`/`fund_for_launch` so the manual + JIT HTTP paths push too).

- [x] **A1 — Emit `launch`/`bundle` status over SSE.** `SseHub::launch_status` +
  `launch_status` frame emitted from every confirm-watcher terminal transition
  (`finalize_landed`/`_dropped`/`_partial`). Frontend `connectLaunchStatusStream` refetches
  the Launch Console status query on a matching `launch_id`.
- [x] **A2 — Emit wallet-funding events over SSE.** `SseHub::wallet_pool_changed` + bare
  `wallet_pool` frame emitted after any funding pass that touched wallets (`notify_pool_changed`
  in `wallet_funding.rs`). Frontend `connectWalletPoolStream` refetches `GET /api/wallet_pool`.
- [x] **A3 — Demote the now-redundant pollers to gap-heal.** Launch Console status poll
  3s→30s (`LaunchConsolePage.tsx`), Wallet Pool poll 5s→30s (`WalletPoolPage.tsx`); SSE is
  now the primary trigger, the poll a fallback.

### Phase B — Ingest observability ✅ DONE 2026-07-14

The heartbeat exists internally but nothing new is exposed, so a stall is invisible from
outside until the watchdog force-exits.

- [x] **B1 — Surface ingest health on the status endpoint.** New `IngestMetrics`
  (`forge/live/src/ingest/metrics.rs`) bundles the shared `DbHeartbeat`, a per-flush
  committed-event `AtomicU64` (bumped in `DbWriter::flush`), and a `WeakSender` peek at the
  consumer→writer channel depth. `spawn_ingest` returns it; it's wired into `app_data` and
  `GET /api/ingest` now carries a `health { commit_age_ms, buffer_depth, buffer_capacity,
  events_total }` block (`http.rs` `IngestHealth`). Shed counters N/A (nothing is shed).
  Frontend `IngestStatus.health?` type kept in sync. Additive — the toggle response omits it.

### Phase C — Task & RPC consolidation ✅ DONE 2026-07-14

> ⚠️ Autonomous real-SOL machinery (funding + dust). Refactored to reuse the existing
> per-step helpers verbatim (C1) + a staleness-gated cache read (C2), cargo/clippy clean,
> but NOT runtime-verified here — needs a mainnet funding + dust smoke before deploy.

- [x] **C1 — Fold the same-table wallet-lifecycle pollers into one tick.** New
  `spawn_wallet_lifecycle` (`launcher/src/wallet_lifecycle.rs`) folds the balance poll,
  reservation/funding TTL sweep, warm-pool funder, and hourly dust sweep into ONE
  `poll→promote→sweep→top-up→dust` tick; each step keeps its cadence via a wall-clock gate,
  reusing the existing bodies (`poll_balances_once`, `sweep_reservations_once`,
  `run_background_funding_pass`, `run_dust_sweep_pass`) — scheduling change only. `main.rs`
  drops from 6 select arms to 3 (ladder + volume stay separate — trading, not lifecycle).
- [x] **C2 — Read the cached balance instead of re-RPCing.** `fresh_cached_balance`
  (`wallet_pool.rs`, 15s window > the tick's 5s active cadence) returns the poller's
  just-written `balance_lamports` when fresh, else `None`. Funding's treasury-pool build +
  per-target shortfall (`wallet_funding.rs`) and the dust sweep's above-floor pre-check
  (`dust_sweep.rs`) reuse it, so a warm lifecycle tick skips the redundant `get_balance`;
  a stale/absent cache falls back to a live read. Pre-send correctness unchanged (exact
  sends; reserve/cap rails apply to whichever figure is used). The balance poller itself
  stays the RPC source-of-truth writer.

### Phase D — Micro ✅ DONE 2026-07-14

- [x] **D1 — Carry raw `u64` lamports through the ingest event.** Added exact-lamport
  mirrors alongside the human-SOL `f64`s: `Trade.sol_lamports` +
  `Reserves.{virtual,real}_sol_lamports` (`shared/ingest/core/src/event.rs`), populated at all
  three decoder construction sites (`trade.rs` curve `DecodedTradeEvent`/AMM `DecodedAmmTrade`
  + new `compute_sol_change_lamports` for the balance-delta fallback in `grpc.rs`). Forge's
  mapper now persists `t.sol_lamports` / `virtual_sol_lamports` directly
  (`forge/live/src/ingest/map.rs`), dropping the `sol_to_lamports(t.sol)` f64 round-trip. The
  `f64` fields stay for non-lamport hosts (hunter is a pure consumer — unaffected).
  ingest-pumpfun unit tests green; forge-live + hunter-live compile.

---

## Cleared (not issues)

Connection-pool sizing, query folding in read handlers, batch UNNEST inserts / CAS upserts,
partial-index-bounded pollers, per-request spawn leaks, the `trades` DESC read path, and the
log-truncation decoder path (fixed on both curve + AMM in the shared decoder forge links).
