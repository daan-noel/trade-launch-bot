# Backend Improvement Plan

Audit date: 2026-06-12. Scope: full `backend/` + `pump-trader/` crate.
Themes: low-latency trade path, ingest/runtime performance, reliability, code quality.

Severity legend: 🔴 correctness / data-loss / money bug · 🟠 latency or hot-path waste · 🟡 quality / reliability hardening.

Each item links the file and notes the fix. Check off as completed.

> **Execution status (2026-06-12 pass).** Phases 1–4 implemented and committed
> (one commit per item where files allowed). Phase 5: 5.2 / 5.3 / 5.4 done
> (5.4 partial — see its note). **Deferred for safety:** 2.4 (cap
> `TokenState.trades`) — the exit memoization's absolute `consumed_len` index,
> `recompute_token_state`'s full replay, and the incremental aggregates all
> assume the append-only full history; a hard cap needs those reworked first.
> **Deferred:** 5.1 (tpsl unification) — large money-path refactor needing
> dedicated focus + live validation. 1.4 buy-path ATA RPC kept intentionally
> (see its note). Pre-existing uncommitted Phase 0 work in `token_sync.rs`,
> `sync.rs`, `app_state.rs` was left untouched and out of these commits.

---

## Phase 0 — Critical (correctness, data-loss, money)

These are bugs, not just slowness. Do these first.

- [x] 🔴 **Runner blocks the whole select loop on real sells.** ✅ FIXED — `trigger_real_exit` now marks `ExitPending` inline (keeps the holding-index/select-loop serialization) then `tokio::spawn`s `sell_and_close_position`; a `selling: Arc<DashSet<Uuid>>` in-flight set gates against double-sells across the spawn boundary (survives a failed ExitPending DB write too). Services now hold `Arc<TokenCache>` so the spawn owns its cache handle. Applied to both tpsl1 and tpsl2.
  `src/strategies/tpsl_sniper_1/service.rs:306-314` (and tpsl2 `:329-337`).
  `on_trade_executed → trigger_real_exit → sell_and_close_position` runs inline + awaited inside the single `select!` task. A real sell awaits tens of seconds (`SELL_MAX_ATTEMPTS × SELL_POLL_MAX_ATTEMPTS × 1s`); during it the runner handles no pings and never ticks the 1s sweep — for both strategies. Root cause of dropped pings below.
  **Fix:** mark `ExitPending` inline, then `tokio::spawn` the sell (as `stop_and_close_rule` does at `lifecycle.rs:171`). Gate with a `DashSet<position_id>` in-flight set so the next sweep/trade skips a position already selling (preserves the no-double-sell invariant).

- [x] 🔴 **Ingest backpressure inversion → silent data loss.** ✅ FIXED — `enqueue_db` and `ping_strategy` now use `send().await` (real backpressure) instead of `try_send`; a queue-full burst stalls the ingest loop (propagating up to the gRPC `send().await` in `client.rs`) rather than dropping persisted trades/metrics or strategy pings. `on_creator_activity` became `async`; the two metrics blocks were restructured so the DashMap `get_mut` guard is dropped before the `await` (no guard held across the new await point).

- [x] 🔴 **token_sync backfill: per-row inserts + swallowed errors + watermark advances past failures.** ✅ FIXED — both the curve loop (`run_token_sync`) and `sync_amm_trades` now accumulate decoded txs/trades/wallets and bulk-insert via a shared `persist_backfill` helper (`tx_repo.insert_many` / `trade_repo.insert_many` / `wallet_repo.touch_last_seen_many`). A bulk-insert failure returns `SyncError::Internal`, which propagates **before** the watermark stamp — so a failed persist no longer advances the watermark and the next incremental sync re-pulls those rows. The previously-swallowed `update_migration_status` error is now logged.

- [x] 🔴 **Integer overflow on a money path.** ✅ FIXED — `manual_sell` now computes `((body.token_amount as u128 * 90) / 100) as u64`, exact and overflow-free. Decoder reserve roll-forward at `trade.rs:267,285` switched from unchecked `+` to `saturating_add`.

- [x] 🔴 **Unbounded fan-out (3 sites) — caps + dedup.** ✅ FIXED
  - `POST /api/token/sync` (`handlers/tokens/sync.rs`): added `AppState::sync_semaphore` (`Semaphore`, cap 4) + `sync_in_flight` (`DashSet<mint>`). A duplicate mint sync returns `409 Conflict`; the spawned task holds a permit for the backfill and clears the in-flight guard on completion.
  - Paper fill-polls (`tpsl1+tpsl2 execution/paper.rs`, entry + exit): each spawned poll task now acquires a permit from a shared `paper_poll_sem` (`Semaphore`, cap 64) on the runtime cache before doing DB work.
  - `db_writer.rs` metrics flush: per-mint `compute_is_rugged`+upsert switched from `join_all` to `stream::iter(...).buffer_unordered(8)`; each task owns a cloned `PgPool`-built repo set.

---

## Phase 1 — Trade hot-path latency (`pump-trader`)

- [ ] 🟠 **Acquire nonce after account/PDA resolution, not before.**
  `pump-trader/src/trader/sell.rs:118-133` holds the nonce slot across RPC-hitting account resolution. Move `acquire_nonce()` to just before `build_nonce_tx`. Same for the slippage `curve_reserves` read at `sell.rs:221` / `buy.rs:166`.

- [ ] 🟠 **Parallelize cold-pool AMM RPCs.**
  `pump-trader/src/trader/amm.rs:199-203` awaits pool-info → config → reserves serially; `amm_config` is independent — `tokio::join!`. And `fetch_fee_share_marker` (`amm.rs:527-554`) loops up to 15 sequential `getTransaction` calls while gating an exit sell — batch it, or always prewarm off the hot path.

- [ ] 🟠 **Give `amm_buy` a `confirm=false` opt-out.**
  `pump-trader/src/trader/amm.rs:104` always pays the full 1s-poll `confirm_transaction` (~4s) unlike live sells. Add the flag; consider a 200ms first poll in `tx.rs:187-207`.

- [ ] 🟠 **Avoid redundant RPC reads on the buy path.**
  `buy.rs:110` does a synchronous ATA-exists RPC; `query.rs:357` re-reads curve routing via RPC for pre-migration tokens even though the WS-fed `TokenCache` has migration state. Consult the cache first.

- [ ] 🟠 **Hot-path micro-allocations.**
  Fan-out clones the full serialized tx JSON per endpoint (`tx.rs:164-174`) → wrap body in `Arc`. `Pubkey::from_str(TOKEN_PROGRAM_ID)` parsed per AMM trade (`amm.rs:217,288`) → cache in `new()`. Timestamp syscall for JSON-RPC `id` per send (`tx.rs:112`) → static / atomic counter.

---

## Phase 2 — Ingest & strategy hot-path allocations

- [ ] 🟠 **Stop cloning full-tx JSON before the relevance decision.**
  `src/ingest_laserstream/decoder/mod.rs:117-122` clones logs/balances/inner-ixs for txs later filtered out. Defer until `save_raw` is decided, or `Arc` the source `Value`. Also `find_pump_ixs_anywhere` re-walks all ixs up to 3× per tx (`mod.rs:229-309`) — call once and reuse.

- [ ] 🟠 **Collapse the double trade clone.**
  `src/ingest_laserstream/pipeline.rs:372,376` clones `e.trade` twice (each incl. `instruction_labels` JSON). Move once / `Arc` the labels.

- [ ] 🟠 **Empty-check before cloning trade history.**
  `src/strategies/tpsl_sniper_1/service.rs:245` does `state.trades.clone()` per trade ping before the `holding_by_mint` empty-check. Move the check first; share one per-mint snapshot across tpsl1/tpsl2.

- [ ] 🟠 **Cap `TokenState.trades` (unbounded memory growth).**
  `src/state/token_cache.rs:19-20` only ever pushes → memory leak proportional to total volume; `unique_wallets()` rescans the whole vec. Keep a ring of last-N, or store only the aggregates actually read.

- [ ] 🟠 **Index the 1s time-exit sweep.**
  `src/strategies/tpsl_sniper_1/service.rs:344-425` clones every holding `Arc<Position>` into a fresh Vec every tick for both strategies even when no rule has a time exit. Maintain a secondary index of only time-exit positions.

- [ ] 🟠 **`TradeSignals::notify` per-trade String allocs.**
  `trade_signals.rs:66-72` allocs two `String`s per committed trade for a lookup that nearly always misses. Use a borrowed-key lookup.

---

## Phase 3 — Database / query layer

- [ ] 🟡 **Fix N+1s.** Runtime cache `find_all_runs()` + per-run `count_by_run` loop (`runtime_cache.rs:80-84`, both strategies) → one `GROUP BY run_id`. Same in `wallet_profile_repo.list_with_wallets:152`.
- [ ] 🟡 **Add missing indexes** (`migrations/0001_init.sql`): `(wallet_address, mint_address)` for hot balance checks; `(mint, slot, leg)` ordering; `tokens_analysis(computed_at)` and `(suspiciousness_score)`.
- [ ] 🟡 **Stream/page `load_all_chronological`** (`trade_repo.rs:491`) — currently loads the entire `trades` table into memory at startup.
- [ ] 🟡 **Bound `saved_signatures` by `slot >= watermark`** (`trade_repo.rs:224`).
- [ ] 🟡 **Collapse `early_buyer_cohort_net`** (`trade_repo.rs:359`, scans `trades` 4× per mint) to one CTE pass; replace pervasive `SELECT *` with explicit columns.
- [ ] 🟡 **Pool config** (`storage/postgres.rs:9`): raise `max_connections`, set `min_connections` + `acquire_timeout`, source from `Settings`.

---

## Phase 4 — API & external-service reliability

- [ ] 🟡 **Helius retry/backoff/rate-limit** (`services/helius_rpc.rs:28-59`): bounded exponential backoff honoring 429 `Retry-After`; check HTTP status before JSON parse; drop the 120s timeout to something interactive.
- [ ] 🟡 **Serve cached SOL price** (`api/handlers/system/system.rs:25`) instead of a synchronous CoinGecko fetch per request.
- [ ] 🟡 **Paginate / cap / offload request-thread work**: `list_tokens` deep-clones+sorts whole cache (`tokens.rs:206`); `detect_tokens_swings_batch` unbounded `mints`, serial DB loads (`swing.rs:160`); `get_trades` unpaginated. Cap inputs, paginate, move CPU to `web::block` (only `http_workers=2`).
- [ ] 🟡 **Lock down trading routes** (`main.rs:24`): CORS `*` + no auth means any browser page can `POST /solana/wallet/buy`. Restrict origin via config + auth token on mutating routes.

---

## Phase 5 — Code quality / maintainability

- [ ] 🟡 **Unify tpsl1 / tpsl2** — biggest maintainability liability. `service.rs`, `runtime_cache.rs`, `lifecycle.rs`, `exit/mod.rs`, `execution/real.rs`, `execution/paper.rs`, and the position/paper repos are near-byte-identical clones differing only by the `Tpsl1`/`Tpsl2` prefix (+ tpsl2 scalp arming). Every fix above must be applied twice and will drift. Extract a `trait Strategy` (associated `Rule`/`PositionRepo`/`PaperRepo` types); keep table-level DB isolation, unify the *logic*; per-strategy code only for the entry policy.
- [ ] 🟡 **Reconnect hardening** (`client.rs:161`): exponential backoff + jitter; track highest slot from all update types (not just transactions) to avoid silent gaps; debounce pool resubscribes.
- [ ] 🟡 **Confirmation timeouts** (`execution/real.rs:534`): wrap trader send/confirm in `tokio::time::timeout` so a wedged RPC doesn't hang a position until the 5-min cleanup.
- [ ] 🟡 **Misc**: move scattered magic constants (`token_sync.rs`, `helius_rpc.rs`) into `config`; fix `received_at = block_time` mislabel (`models/trade.rs:126`); replace per-item-hiding `#![allow(dead_code)]` blanket in `config/constants.rs`.

---

## Already good — do not regress

Pre-built CU/tip instructions, precomputed AMM PDAs, WS-fed reserve cache, event-driven nonce `Notify`, sender fan-out (returns on first accept), `confirm=false` feed-confirm on live sells, parameterized bulk inserts (no SQL injection), COALESCE-preserving upserts.

---

## Suggested order

1. Phase 0 (#1 spawn-sells + in-flight guard, #2 backpressure) — correctness/data-loss.
2. Phase 5 tpsl unification — so every later fix is applied once, not twice.
3. Phases 1–2 — latency wins.
4. Phases 3–4 — DB indexes + external reliability.
