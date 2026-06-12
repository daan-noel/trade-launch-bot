## 🔴 (DONE) - Tier 1 — Per-event hot path (runs on every trade, costs the most)

1. Full JSON tree is rebuilt for every gRPC update — even the majority you throw away. adapter.rs:18-116
update_tx_to_value allocates a fresh serde_json::Value tree (dozens of heap allocs + base58 encodes) for every message, including txs the decoder Ignores one call later (wrong program, untracked pool). Fix: cheap pre-filter on meta.log_messages for the pump/PumpSwap program id before building the Value. Biggest single win — kills the build for most messages.

2. The whole JSON blob is then deep-cloned per kept tx. decoder/mod.rs:121 result.clone() exists only to keep result borrowable. Pass the Value by value into decode_result and move it into the Arc<RawTransaction>. Removes a second full deep copy per tx.

3. The strategy path deep-clones the entire trade history per event — twice. tpsl_sniper_1/service.rs:272 and tpsl_sniper_2/service.rs:295
state.trades.clone() copies up to 50,000 Trade structs (each 4+ heap allocs) on every trade ping for a held mint — and it fires independently in each strategy, so one ingested trade = two full-vec deep clones. And it fires precisely on the hot, high-volume tokens you're actually holding. Fix (per-strategy, no merge): store Arc<Vec<Trade>> in TokenState so the snapshot is a pointer bump, or compute exit decisions under the read guard and drop it before the awaits — no clone at all.

4. Instruction data is base58-decoded 3-5× per tx. decoder/parse.rs, instructions.rs — collect_instruction_kinds, build_instruction_labels, the create check, the migrate check, and trade-event decode each re-decode the same strings the adapter just encoded. Decode once per tx into Vec<(programId, Vec<u8>)> and pass it through.

## 🟠 Tier 2 — Strategy & trade-execution latency

5. The two strategies run serially on the event loop. strategies/runner.rs:83-90 — tpsl2 can't start until tpsl1's awaited DB write finishes. They share no mutable state. tokio::join!(...) the two calls — roughly halves per-event wall time and keeps each strategy's internal ordering intact. (Respects separation — they stay separate, just concurrent.)

2. Exit ladder re-walks full post-entry history every trade. tpsl_sniper_1/exit/mod.rs:225, worse in tpsl_sniper_2/exit/mod.rs:236 (E5 adds 3 extra full passes + a HashSet alloc per position per trade). You already maintain an incremental CachedExitState for the clock sweep — feed it into the trade path so it walks only new trades, not the whole history.

3. Two avoidable network round-trips on the manual buy path.

buy.rs:109-113 — get_account(&ata) is a full RPC RTT before send; the snipe path already skips it. Use the cached resolve_cached_token_account or join! it with template acquisition.
mod.rs:287 — reqwest::Client::new() for the landing hop has no tcp_nodelay (Nagle ≈40ms), no warmed keep-alive pool. First send pays full TCP+TLS handshake inline. Build with tcp_nodelay(true) + warm one connection per sender endpoint at init.
8. Fixed 250ms sleeps on the sell path. sell.rs:82-84 — between sell retries, when a lost Jito auction "costs nothing." That's dead latency while the token dumps. Escalate tip + fresh nonce and resend immediately; only back off on genuine network errors.

## 🟡 Tier 3 — API / storage workflow

9. GET /api/tokens materializes and sorts the entire cache per request. tokens.rs:218-248 — builds a TokenSummary for every token (cap 50,000), each calling active_lifetime_secs() which walks that token's whole trade vec — before paginating. O(N·M) per list call to return one page. Sort/filter on cheap keys first, build summaries only for the page slice, and memoize active_lifetime_secs on add_trade.

2. Swing batch clones + serializes per mint. swing.rs:183-209 — entry.trades.clone() (full deep copy) just to read, and on cache-miss does up to 200 serial DB round-trips (N+1). Frontend then runs those chunks sequentially (SwingDetectionPage.tsx:521). detect_swings only needs &[Trade] — borrow instead of clone; batch the miss path with WHERE mint_address = ANY($1); run detection on the blocking pool.

3. Trade SELECTs pull all 19 columns (incl. JSONB ix_labels) when swing/seed use ~7. trade_repo.rs:297 — per-row JSONB decode across the entire table at seed time. Add a slim projection row for the swing + seed paths.

4. Missing index for the buy/sell confirm query. trade_repo.rs:263 — find_latest_by_wallet_mint_type filters (wallet, mint, trade_type) + ORDER BY block_time DESC but the index is only (wallet, mint), forcing a sort on a latency-critical path. Add idx_trades_wallet_mint_type_time(wallet_address, mint_address, trade_type, block_time DESC).

#### If you fix only three things

- Pre-filter in the adapter (#1) — stop building JSON for ignored txs. Contained change, biggest ingest win.
- Arc<Vec<Trade>> in TokenState (#3) — kills the 50k-element deep clone that fires twice per held-token trade.
- Warm + tune the sender reqwest::Client (#7) — removes a TCP/TLS handshake from your money-making landing hop.
