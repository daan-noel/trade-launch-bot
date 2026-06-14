# Backend Performance Audit

Multi-agent review of the Rust `backend` + `pump-trader` crates focused on **performance, low-latency buy/sell, data management, caching, and API/SSE handlers**. Six subsystem reviewers fanned out (ingest, strategies, execution, api/db, sse/state, services); every finding was then adversarially re-checked against the actual source before inclusion.

- Severity scale: **high** = per-event waste on ingest/strategy hot path or unbounded growth · **medium** = per-request API/DB waste · **low** = cold-path inefficiency.
- The buy/sell send path and the sell-confirm loop are clean (no critical findings). The remaining weight is in **ingest per-event allocation** and assorted **bounded-query / cold-path** cleanups.

---

## Confirmed findings (open)

### HIGH — hot-path / unbounded

#### H1. Protobuf is transcoded to `serde_json::Value`, then re-parsed by the decoder (double work per event)
- **File:** [backend/src/ingest_laserstream/adapter.rs:24-137](backend/src/ingest_laserstream/adapter.rs#L24-L137) (decode side: `decoder/parse.rs`, `decoder/instructions.rs:199`)
- **Category:** throughput · **Hot path:** yes
- Every passing pump.fun/PumpSwap tx is converted from the already-typed protobuf `SubscribeUpdateTransaction` into a `serde_json::Value`: account keys base58-encoded into `Value::String`, instructions turned into `json!` maps, instruction data base58-**encoded**, balances boxed into `Value`. The decoder then immediately re-parses it.
- **Note:** the upstream pre-filter (only build the `Value` for pump/PumpSwap txs), the decode-once-per-tx of instruction data, and the account-key `String` re-allocation (decoder now threads `&[&str]` borrowed from the `Value` instead of cloning every key per tx — `h1-typed-decode-plan.md` Tier A) are now in place — so the *ignored-tx* build, the repeated re-decode, and the per-tx account-key alloc are gone. What remains is the adapter still base58-**encoding** data that the decoder then base58-**decodes** once — removable only by a protobuf-native decode (Tier B), which forks the shared decoder from token_sync's RPC `Value` path and is gated on a profile.
- **Caveat:** the `Value` is genuinely needed as the persisted `raw_tx` blob, so it can't be fully removed.
- **Profiled (2026-06-14, live, ~45 tx/s):** Value build **~2.7 ms** avg vs decode **~0.83 ms** (`INGEST_PROFILE` span). `built_ratio = 1.0` — the gRPC subscription already scopes server-side, so the Value is built for *every* received tx (the pre-filter saves nothing). Build dominates (~76% of hot-path CPU) and scales on a single task → bottleneck at high volume. **Tier B is justified.**
- **Fix:** **Tier B** — decode directly from typed protobuf into the event structs; build the `Value` only for the raw blob, off-thread in the DbWriter. (A mimalloc global allocator was tested first on the allocator-bound hypothesis — **no measurable change**, so the cost is CPU-bound in base58 + `Value` construction; mimalloc reverted.) See `h1-typed-decode-plan.md`.

### MEDIUM — per-request API/DB waste

#### M3. `find_scalp_entry` rebuilds cohort + rescans the prefix for every candidate (O(n²) per evaluation)
- **File:** [backend/src/strategies/tpsl_sniper_2/entry/scalp.rs:70-106](backend/src/strategies/tpsl_sniper_2/entry/scalp.rs#L70-L106)
- Walks every index `i` calling `scalp_features(&trades[..=i])`; each candidate rebuilds the cohort `HashSet<String>` and does multiple full prefix passes → O(n²) total, re-paid every poll tick.
- **Fix:** compute cohort once for the full slice, carry running `cohort_flow`/`outside_net_sol`/alive-window accumulators forward → O(n).

#### M4. `find_by_mint_all` — unbounded `SELECT` of a mint's entire trade history (shared root cause across paths)
- **File:** [backend/src/storage/repositories/trade_repo.rs:297-309](backend/src/storage/repositories/trade_repo.rs#L297-L309) — no LIMIT, no window, `fetch_all` into `Vec<Trade>`
- The swing endpoints now use the bounded `find_by_mint_paged` variant, but `find_by_mint_all` still exists and is still called by the token-sync metrics rebuild ([token_sync.rs:523-533](backend/src/services/token_sync.rs#L523-L533), see L11).
- **Fix:** switch the remaining callers to the paged variant / a time window; bound the read.

#### M5. Whole 50K-trade buffer cloned while holding the DashMap shard lock (swing endpoints)
- **File:** [backend/src/api/handlers/tokens/swing.rs:125-126, 196-198](backend/src/api/handlers/tokens/swing.rs#L125-L126)
- The guard is now dropped before `detect_swings` runs (heavy work no longer under the lock), but `entry.trades.clone()` — the multi-MB deep copy of up to 50K `Trade`s — still happens **while the shard read-guard is held**. The ingest pipeline's `get_mut` on any mint in the same shard blocks for that copy; the batch endpoint amplifies this across 16 concurrent mints.
- **Fix:** store trades as `Arc<[Trade]>` in `TokenState` so readers clone a refcount instead of deep-copying under the guard.

### LOW — cold path / micro-optimizations

| # | Finding | File | Status |
|---|---------|------|--------|
| L1 | `extract_logs` allocates a fresh `Vec<&str>` of all log lines multiple times per tx (first call is a pure gate that could be lazy) | [decoder/parse.rs](backend/src/ingest_laserstream/decoder/parse.rs) | **DONE** — added borrowed `log_lines()` iterator; the `decode_result` relevance gate now `.any()`s over it instead of collecting a throwaway `Vec` |
| L2 | Pool-subscription refresh clones all `pool_index` values into a HashSet and full-scans the whole token cache every 120s tick (O(all tokens)) | [pipeline.rs](backend/src/ingest_laserstream/pipeline.rs) | **WONTFIX** — a 120 s *cold* revival sweep; making it incremental requires tracking active-migrated tokens from the hot path, adding latency/coupling there to save a background tick. The full scan is also the robustness net that catches missed `Migrate` events. Net negative. |
| L3 | Full `TokenMetricsWrite` (2 String allocs) built + channel-sent on **every** trade, though DbWriter dedups to one-per-mint per 25ms flush | [pipeline.rs](backend/src/ingest_laserstream/pipeline.rs) | **WONTFIX** — DbWriter *already* coalesces per-mint per 25 ms flush. "Coalesce before enqueue" would duplicate that dedup and add a timer+map to the hot path; the only real waste is 2 short String allocs into a cheap bounded mpsc. Adds more hot-path work than it removes. |
| L4 | SSE event struct built + `sse_tx.send()` unconditionally on every trade, no `receiver_count()==0` guard | [pipeline.rs](backend/src/ingest_laserstream/pipeline.rs) | **DONE** — `emit_sse` now early-returns on `receiver_count()==0` (atomic load) so the steady state with no dashboard open does zero broadcast-ring work |
| L5 | Sender fan-out re-serializes the JSON-RPC body to bytes once **per endpoint** | [pump-trader/src/trader/tx.rs](pump-trader/src/trader/tx.rs) | **ALREADY DONE** — `send_transaction` serializes once into `Arc<Vec<u8>>`, each task clones the pointer |
| L6 | `sell_token_once` calls `get_creator_from_mint_pda` only to warm the cache but discards the allocated creator `String` | [pump-trader/src/trader/sell.rs](pump-trader/src/trader/sell.rs) | **ALREADY DONE** — String-free `ensure_token_pdas` exists and is used by the sell path |
| L7 | Cold first AMM swap gates on `serde_json::Value` RPC round-trips to recover the fee-share marker | [pump-trader/src/trader/amm.rs](pump-trader/src/trader/amm.rs) | **ALREADY DONE** — `prewarm_amm_pool` is wired into the pipeline on a token's first AMM trade (pipeline.rs); residual is only a sell racing the in-flight prewarm |
| L8 | Token global-search clones every string field (+ rfc3339 formats) of every scanned row per request | [api/handlers/tokens/tokens.rs](backend/src/api/handlers/tokens/tokens.rs) | **ALREADY DONE** — `search_match`/`field_contains` match against `&str` borrows; only the candidate is lowercased, and dates/numerics format lazily after the string fields miss |
| L9 | `HeliusRpc::new()` builds a fresh `reqwest::Client` per construction | [services/helius_rpc.rs](backend/src/services/helius_rpc.rs) | **ALREADY DONE** — `rpc_client()` memoizes the client in a shared `OnceLock`; `new()` clones the Arc-backed handle |
| L10 | Token-sync materializes the full signature history into a Vec before filtering against `saved_signatures` (extra DISTINCT scan) | [services/token_sync.rs](backend/src/services/token_sync.rs) | **WONTFIX** — already watermark-bounded (the `until` cursor usually yields a single page), and the `saved_signatures` DISTINCT *is* the dedup correctness mechanism. Cold sync path. |
| L11 | Token-sync `find_by_mint_all` + `trades.clone()` reloads a mint's entire history every incremental sync to rebuild metrics | [services/token_sync.rs](backend/src/services/token_sync.rs) | **WONTFIX** — the second copy is **not** redundant: `recompute_token_state` rebuilds `state.trades` through the 50K retention ring, so for high-volume mints it is a truncated view, whereas `SyncOutput.trades` must carry the complete synced list. Confirmed in-code with a comment. |
| L12 | CoinGecko SOL price: text + `from_str` + dynamic key nav instead of `resp.json::<Typed>()` | [services/clients/coingecko.rs](backend/src/services/clients/coingecko.rs) | **ALREADY DONE** — uses `resp.json::<SimplePrice>()` with a typed struct |
| L13 | Jupiter prices parsed into `serde_json::Value` + string-key indexing instead of a typed `Deserialize` map | [services/clients/jupiter.rs](backend/src/services/clients/jupiter.rs) | **ALREADY DONE** — deserializes into `HashMap<String, RawPriceEntry>` via `.json()` |

---

## Suggested order of attack

1. **H1** — the remaining per-event ingest cost (protobuf → Value reparse / base58 round-trip). Biggest steady-state win left.
2. **M4 / M5 / M3** — bounded queries + `Arc<[Trade]>` snapshot (kills the under-guard deep copy) + O(n²)→O(n) scalp scan.
3. **Low tier** — opportunistic; L3 (metrics coalescing) and L5 (fan-out byte reuse) are the highest-value of these.

---

## Appendix — rejected findings (verified false / intentional)

| Claim | File | Why rejected |
|-------|------|--------------|
| DbWriter rebuilds 5 repos + clones `PgPool` per flush | `ingest_laserstream/db_writer.rs` | `Repo::new` is a zero-cost move; `PgPool::clone` is an Arc bump. The per-metric owned-pool pattern is a **deliberate** HRTB workaround (documented in-code). No measurable impact. |
| StrategyRunner evaluates tpsl1 then tpsl2 sequentially | `strategies/runner.rs` | Hot common case bails before any `.await`; the slow case is synchronous, so the proposed `tokio::join!` provides **no** parallelism. Real exit awaits already offload to spawned tasks. |
| Sell-confirm loop runs an unbounded `SUM` over all trades per poll tick | `tpsl_sniper_1/execution/real.rs` | The composite index `idx_trades_wallet_mint` (migration 0002) makes it an index range scan over one position's rows — not a full-table scan. The fix requested an index that already exists. |
| `acquire_nonce` holds the slots Mutex across an O(n) scan + atomics per buy/sell | `pump-trader/src/trader/nonce.rs` | No `.await` in the critical section; scan is over a tiny fixed nonce set (µs-scale); diagnostic atomics only fire after a spin-wait. Standard practice, not a latency source. |
| Every trade broadcast to every client; mint filter is client-side only | `frontend-react/src/services/sse.ts` | Premise false — **all three** trade-stream consumers are global views (Tokens/Transactions/Dashboard); token-detail pages use RTK Query REST, not SSE. No single-mint subscriber exists to scope. Single shared connection is intentional (browser connection cap). |
| `get_wallet_tokens` does 2 full account scans + Jupiter call per request, no caching | `services/wallet_tokens.rs` | Premise false — the expensive `/wallet/tokens` read is **not** polled; it's RTK-Query cached and refreshed surgically. The 20s poll hits the lightweight `/prices` endpoint. Decoupling is **intentional**. |
| SOL price poller logs + `watch.send` every 60s even when unchanged | `services/sol_price.rs` | The watch channel has **zero** `.changed()` subscribers (only pull-on-demand `borrow()`); the send wakes nobody. One log line per minute on a cold path — below the bar. |

---

*Generated by a 6-reviewer + adversarial-verifier workflow. Each confirmed finding was read and re-verified against the cited source; line numbers reflect the state of the tree at audit time (branch `master`).*
