# Backend Performance Audit

Multi-agent review of the Rust `backend` + `pump-trader` crates focused on **performance, low-latency buy/sell, data management, caching, and API/SSE handlers**. Six subsystem reviewers fanned out (ingest, strategies, execution, api/db, sse/state, services); every finding was then adversarially re-checked against the actual source before inclusion.

- **29 findings confirmed** (verified against code), **7 rejected** (false positives / intentional design — see appendix).
- Severity scale: **critical** = adds buy/sell execution latency or can stall ingest · **high** = per-event waste on ingest/strategy hot path or unbounded growth · **medium** = per-request API/DB waste · **low** = cold-path inefficiency.
- No **critical** findings — the buy/sell send path and the sell-confirm loop are clean. The real weight is in **ingest per-event allocation**, **strategy exit re-walks**, **paper-path DB polling**, and **SSE per-subscriber re-serialization**.

> Several reviewers independently flagged the same root causes (SSE re-serialization, `find_by_mint_all` unbounded, exit re-walk in both sniper clones). These are **merged** below with all call sites listed.

---

## Confirmed findings

### HIGH — hot-path / unbounded

#### H1. Protobuf is transcoded to `serde_json::Value`, then re-parsed by the decoder (double work per event)
- **File:** [backend/src/ingest_laserstream/adapter.rs:24-137](backend/src/ingest_laserstream/adapter.rs#L24-L137) (decode side: `decoder/parse.rs`, `decoder/instructions.rs:199`)
- **Category:** throughput · **Hot path:** yes
- Every passing pump.fun/PumpSwap tx is converted from the already-typed protobuf `SubscribeUpdateTransaction` into a `serde_json::Value`: account keys base58-encoded into `Value::String`, instructions turned into `json!` maps, instruction data base58-**encoded**, balances boxed into `Value`. The decoder then immediately re-parses it — `extract_account_keys` re-allocates Strings, `instruction_data_bytes` base58-**decodes** the data the adapter just encoded, `extract_balances` re-reads numbers. The protobuf already holds `Vec<bytes>`/`Vec<CompiledInstruction>`/`Vec<u64>`.
- **Caveat:** the `Value` is genuinely needed as the persisted `raw_tx` blob, so it can't be fully removed — but the decode-side reparse and the base58 encode→decode round-trip are pure waste.
- **Fix:** decode directly from the typed protobuf into the event structs; build the `Value` only for the raw blob. At minimum pass instruction-data bytes through instead of base58 encode-then-decode.

#### H2. Per-leg `instruction_labels` JSON `Value` cloned and retained in the 50K in-memory trade ring
- **File:** [backend/src/ingest_laserstream/decoder/mod.rs:221-231](backend/src/ingest_laserstream/decoder/mod.rs#L221-L231) · pipeline clone [pipeline.rs:394](backend/src/ingest_laserstream/pipeline.rs#L394) · retention [state/token_cache.rs:24](backend/src/state/token_cache.rs#L24)
- **Category:** memory · **Hot path:** yes
- `Trade.instruction_labels: serde_json::Value` is deep-cloned once per trade leg, then deep-cloned **again** into the DB channel, and the `Trade` is also moved into `TokenState.trades` (capped at `MAX_TRADES_RETAINED = 50_000` per token). So every retained trade permanently holds a heap JSON array that the in-memory exit/metric logic **never reads** (strategy only reads the `Token`-level labels; per-trade labels are used only by the trades-API display filter).
- **Fix:** strip labels before pushing into the in-memory ring (keep the full `Trade` only for the DB write), and/or wrap labels in `Arc<str>`/`Arc<Value>` so the DB clone is a pointer bump.

#### H3. Trade-driven exit re-walks the full post-entry history on every trade ping (memoization bypassed) — both sniper clones
- **Files:** [backend/src/strategies/tpsl_sniper_1/exit/mod.rs:225-231](backend/src/strategies/tpsl_sniper_1/exit/mod.rs#L225-L231) and [tpsl_sniper_2/exit/mod.rs:215-343](backend/src/strategies/tpsl_sniper_2/exit/mod.rs#L215-L343) · caller `service.rs on_trade_executed`
- **Category:** redundant per-event compute · **Hot path:** yes
- For every held position on every trade ping, `find_trade_driven_exit` builds `trades.iter().filter(|t| t.block_time > entry_time).collect::<Vec<&Trade>>()` and re-folds the **entire** post-entry window from a fresh `ExitWalkState`, recomputing peaks/stall/reserves — O(retained trades) per position per ping, climbing toward the 50K cap on a hot token. The incremental `CachedExitState`/`exit_state_advance` memo (built on this same path) is consumed **only by the clock sweep**, never by the trade gate.
- **Fix:** have the trade gate evaluate the ladder against only the newly-appended trades using the already-memoized running peaks; drop the `collect()` into `Vec<&Trade>`. (Apply to both clones.)

#### H4. TPSL2 E5 cohort exit recomputes the launch cohort (HashSet of wallet clones + 3 full passes) on every trade ping
- **File:** [backend/src/strategies/tpsl_sniper_2/exit/mod.rs:236-251](backend/src/strategies/tpsl_sniper_2/exit/mod.rs#L236-L251) (`cohort.rs`)
- **Category:** memory / per-event compute · **Hot path:** yes (gated behind `p_exit_cohort_ratio`)
- When E5 is enabled, each per-position per-ping call rebuilds the cohort from scratch: `early_cohort_wallets` allocates a `HashSet<String>` cloning every early-buyer wallet, `cohort_flow` does a full pass, plus a third inline pass for net-at-entry. Cohort membership is fixed once the early-slot window closes.
- **Fix:** memoize cohort set + `cohort_bought` + net-at-entry alongside `CachedExitState` (compute once when the window closes), advance `cohort_net` incrementally — mirroring the existing peak-memo pattern.

#### H5. Paper exit-fill poll re-fetches **all** trades for a mint (unbounded `SELECT`, no LIMIT) every 500ms for 10s
- **File:** [backend/src/strategies/tpsl_sniper_2/execution/paper.rs:128-174](backend/src/strategies/tpsl_sniper_2/execution/paper.rs#L128-L174) → `trade_repo.find_by_mint_all` (and tpsl1 clone)
- **Category:** db · **Hot path:** no (paper path), but violates "bound every query" + "notify over poll"
- The loop calls `find_by_mint_all(&mint)` (full history, all columns, no LIMIT) up to ~20× per exiting position and re-walks the whole vec each time. Task **count** is bounded (64-permit semaphore) but each task issues unbounded queries.
- **Fix:** confirm the fill from the in-memory `token_cache` window and wake on `TradeSignals` like the real path; if DB is required, fetch only rows after `entry_block_time` with a LIMIT / since-last-seen.

#### H6. Paper entry-fill poll re-fetches **all** trades per tick across the whole arming window, then runs O(n²) scalp scan
- **File:** [backend/src/strategies/tpsl_sniper_2/execution/paper.rs:53-83](backend/src/strategies/tpsl_sniper_2/execution/paper.rs#L53-L83) · same pattern in `execution/real.rs:144-156` (`await_scalp_entry_signal`)
- **Category:** db + compute · **Hot path:** no (arming, before buy)
- Loops `scalp_arming_attempts` times (≥60 ticks at 1s cadence), each tick `find_by_mint_all` (unbounded) then `find_scalp_entry` over the full growing vec.
- **Fix:** drive arming off the `token_cache` window + `TradeSignals` wakeup; bound the DB read if unavoidable. (See M3 for the O(n²) inner cost.)

#### H7. SSE event re-serialized (JSON build + `to_string`) once **per subscriber** instead of once per event
- **File:** [backend/src/api/handlers/system/stream.rs:33-205](backend/src/api/handlers/system/stream.rs#L33-L205) (`to_sse_frame` invoked inside each connection's `stream::unfold`)
- **Category:** api-sse · **Hot path:** no (SSE delivery, decoupled from ingest)
- The `broadcast` channel hands each subscriber the **same** typed `SseEvent`; each one independently runs `json!{...}` + `data.to_string()` + `format!` into a fresh buffer. N dashboard tabs ⇒ a single trade serialized N times → O(events × clients). CLAUDE.md explicitly calls this pattern out as a bug.
- **Fix:** serialize each event to frame bytes **once** at publish (or memoize per `(event, mint_filter)`), broadcast `Arc<Bytes>`/`Bytes`, and have subscribers clone the ref-counted buffer.
- **Related (same root cause):** `live_stats(state, mint)` embedded in each frame does a `token_cache.get` (DashMap shard read-lock) + `json!` build **per subscriber per event** ([stream.rs:14-31, 65, 93](backend/src/api/handlers/system/stream.rs#L14-L31)), re-reading data the pipeline held microseconds earlier and contending the same shards the ingest writer (`get_mut`) uses. **Fix:** carry the small stat set (price/mcap/volume/trade_count/ath) in the `SseEvent` payload itself so the SSE path touches the cache zero times.

### MEDIUM — per-request API/DB waste

#### M1. List endpoints issue a separate full-table `COUNT(*)` per request
- **File:** [backend/src/storage/repositories/analysis_repo.rs:162-202](backend/src/storage/repositories/analysis_repo.rs#L162-L202) (`list_results`, `list_creator_profiles`) → handlers `/api/analysis`, `/api/creators`
- Unfiltered `SELECT COUNT(*)` (cannot use a narrow index, grows with table) plus the page query = two round-trips per request, only to feed the pager.
- **Fix:** `COUNT(*) OVER()` as a window column (one round-trip), or `limit+1` `has_more` flag, or a briefly-cached total.

#### M2. Position list/holding queries have no LIMIT (full result set per call)
- **File:** [backend/src/storage/repositories/tpsl1_position_repo.rs:202-318](backend/src/storage/repositories/tpsl1_position_repo.rs#L202-L318) (`find_holding_by_mint/_wallet`, `find_by_rule`, `find_by_strategy`) + tpsl2 mirror → `tpsl{1,2}_positions.rs` handlers
- All `fetch_all` with `ORDER BY created_at DESC`, no LIMIT/offset; handlers return verbatim. `find_by_strategy` filters only on strategy (all statuses) → returns every position ever recorded, grows unbounded.
- **Fix:** add limit/offset (or time window) defaulting to a sane page size.

#### M3. `find_scalp_entry` rebuilds cohort + rescans the prefix for every candidate (O(n²) per evaluation)
- **File:** [backend/src/strategies/tpsl_sniper_2/entry/scalp.rs:70-106](backend/src/strategies/tpsl_sniper_2/entry/scalp.rs#L70-L106)
- Walks every index `i` calling `scalp_features(&trades[..=i])`; each candidate rebuilds the cohort `HashSet<String>` and does multiple full prefix passes → O(n²) total, re-paid every poll tick (see H6).
- **Fix:** compute cohort once for the full slice, carry running `cohort_flow`/`outside_net_sol`/alive-window accumulators forward → O(n).

#### M4. `find_by_mint_all` — unbounded `SELECT` of a mint's entire trade history (shared root cause across paths)
- **File:** [backend/src/storage/repositories/trade_repo.rs:297-309](backend/src/storage/repositories/trade_repo.rs#L297-L309) — no LIMIT, no window, `fetch_all` into `Vec<Trade>`
- Callers: single-token swing fallback [swing.rs:125-138](backend/src/api/handlers/tokens/swing.rs#L125-L138) (also runs `detect_swings` **inline** on the HTTP worker, unlike the batch path which uses `web::block`); swing batch (up to 200 mints, 16 concurrent); token-sync metrics rebuild [token_sync.rs:523-533](backend/src/services/token_sync.rs#L523-L533); paper polls (H5/H6). A bounded sibling `find_by_mint_paged` already exists and its doc comment acknowledges this exact risk.
- **Fix:** bound the swing fallback (same `MAX_TRADES_RETAINED` cap or time window) and offload its scan to `web::block`; switch callers to the paged variant.

#### M5. Whole 50K-trade buffer cloned while holding the DashMap shard lock (swing endpoints)
- **File:** [backend/src/api/handlers/tokens/swing.rs:125-126, 196-198](backend/src/api/handlers/tokens/swing.rs#L125-L126)
- `state.token_cache.get(&mint)` then `entry.trades.clone()` — the `Ref` read-guard holds the shard lock for the entire multi-MB deep copy of up to 50K `Trade`s. The ingest pipeline's `get_mut` on any mint in the same shard blocks meanwhile; the batch endpoint amplifies this across 16 concurrent mints.
- **Fix:** store trades as `Arc<[Trade]>` in `TokenState` so readers clone a refcount, or copy out only the needed window then release the guard before heavy work.

### LOW — cold path / micro-optimizations

| # | Finding | File | Note |
|---|---------|------|------|
| L1 | `extract_logs` allocates a fresh `Vec<&str>` of all log lines multiple times per tx (first call is a pure gate that could be lazy) | [decoder/parse.rs:16-21](backend/src/ingest_laserstream/decoder/parse.rs#L16-L21) | borrowed refs, not deep clones; minor |
| L2 | Pool-subscription refresh clones all `pool_index` values into a HashSet and full-scans the whole token cache every 120s tick (O(all tokens)) | [pipeline.rs:636-654](backend/src/ingest_laserstream/pipeline.rs#L636-L654) | maintain migrated+live set incrementally |
| L3 | Full `TokenMetricsWrite` (2 String allocs) built + channel-sent on **every** trade, though DbWriter dedups to one-per-mint per 25ms flush | [pipeline.rs:397-403](backend/src/ingest_laserstream/pipeline.rs#L397-L403) | **borderline medium**; coalesce per-mint before enqueue |
| L4 | SSE event struct built + `sse_tx.send()` unconditionally on every trade, no `receiver_count()==0` guard | [pipeline.rs:315-317](backend/src/ingest_laserstream/pipeline.rs#L315-L317) | no heap alloc added; tiny constant waste with no dashboard open |
| L5 | Sender fan-out re-serializes the JSON-RPC body to bytes once **per endpoint** (the costly tx base64 is shared; only the envelope walk repeats) | [pump-trader/src/trader/tx.rs:172-193](pump-trader/src/trader/tx.rs#L172-L193) | `serde_json::to_vec` once into `Arc<Vec<u8>>`, `.body(bytes.clone())` per task |
| L6 | `sell_token_once` calls `get_creator_from_mint_pda` only to warm the cache but discards the allocated creator `String` | [pump-trader/src/trader/sell.rs:116-119](pump-trader/src/trader/sell.rs#L116-L119) | cold (first sell of a mint); add a String-free `ensure_token_pdas` |
| L7 | Cold first AMM swap gates on `serde_json::Value` RPC round-trips (`getSignaturesForAddress` + `getTransaction jsonParsed`) to recover the fee-share marker | [pump-trader/src/trader/amm.rs:539-586](pump-trader/src/trader/amm.rs#L539-L586) | mostly mitigated — `prewarm_amm_pool` is already wired into the pipeline on first AMM trade; residual is a sell racing the in-flight prewarm |
| L8 | Token global-search clones every string field (+ rfc3339 formats) of every scanned row per request | [api/handlers/tokens/tokens.rs:1034-1052](backend/src/api/handlers/tokens/tokens.rs#L1034-L1052) | runs on blocking pool; match against `&str` borrows instead |
| L9 | `HeliusRpc::new()` builds a fresh `reqwest::Client` (new pool + TLS) per construction, bypassing the shared `OnceLock` client | [services/helius_rpc.rs:24-32](backend/src/services/helius_rpc.rs#L24-L32) | manual-sync cold path; build once at startup / memoize per URL |
| L10 | Token-sync materializes the full signature history into a Vec before filtering against `saved_signatures` (extra DISTINCT scan) | [services/token_sync.rs:341-364](backend/src/services/token_sync.rs#L341-L364) | already watermark-bounded; cold sync path |
| L11 | Token-sync `find_by_mint_all` + `trades.clone()` reloads a mint's entire history every incremental sync to rebuild metrics | [services/token_sync.rs:523-533](backend/src/services/token_sync.rs#L523-L533) | see M4; clone has 2 real consumers so one copy is unavoidable; fold metrics incrementally |
| L12 | CoinGecko SOL price: `resp.text()` then `serde_json::from_str` + dynamic key nav, instead of `resp.json::<Typed>()` | [services/clients/coingecko.rs:18-25](backend/src/services/clients/coingecko.rs#L18-L25) | once/60s; typed struct avoids String + Value round-trip |
| L13 | Jupiter prices parsed into `serde_json::Value` + string-key indexing instead of a typed `Deserialize` map | [services/clients/jupiter.rs:15-38](backend/src/services/clients/jupiter.rs#L15-L38) | per-request (many mints) but dwarfed by the HTTP RTT |

---

## Suggested order of attack

1. **H1 / H2 / H3 / H4** — these are the per-event ingest + strategy-eval costs that scale with trade volume. Biggest steady-state wins. H3/H4 also clean up the serialized `StrategyRunner` loop.
2. **H5 / H6 + M4** — move the paper/arming polls off unbounded DB queries onto the in-memory `token_cache` + `TradeSignals` (the documented "notify over poll" pattern); fix `find_by_mint_all` callers at the same time.
3. **H7 (+ live_stats)** — serialize SSE frames once and carry stats in the payload; removes O(events × clients) work and DashMap contention with the ingest writer.
4. **M1 / M2 / M5 / M3** — bounded queries + `Arc<[Trade]>` snapshot + O(n²)→O(n) scalp scan.
5. **Low tier** — opportunistic; L3 (metrics coalescing) and L5 (fan-out byte reuse) are the highest-value of these.

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
