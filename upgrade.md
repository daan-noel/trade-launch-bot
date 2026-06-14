## 🟠 Tier 2 — Strategy & trade-execution latency

5. The two strategies run serially on the event loop. strategies/runner.rs:83-90 — tpsl2 can't start until tpsl1's awaited DB write finishes. They share no mutable state. tokio::join!(...) the two calls — roughly halves per-event wall time and keeps each strategy's internal ordering intact. (Respects separation — they stay separate, just concurrent.) NOTE: a parallel audit argued the hot common case bails before any `.await` and the slow path is synchronous, so `join!` may yield no real parallelism — verify before investing.

6. Buy path — one avoidable network round-trip remains on the manual (non-snipe) path. buy.rs:109-113 — get_account(&ata) is a full RPC RTT before send; the snipe path already skips it (skip_ata_check). Use the cached resolve_cached_token_account or join! it with template acquisition. (The sender reqwest client warmup + tcp_nodelay is already done in the pump-trader init path.)

7. Fixed 250ms sleeps on the sell path. sell.rs:82-84 — between sell retries, when a lost Jito auction "costs nothing." That's dead latency while the token dumps. Escalate tip + fresh nonce and resend immediately; only back off on genuine network errors.

## 🟡 Tier 3 — API / storage workflow

8. Swing batch clones + serializes per mint. swing.rs — entry.trades.clone() (full deep copy) just to read. detect_swings only needs &[Trade]. The N+1 miss path is already fixed (paged DB read + buffer_unordered concurrency) and detection runs off the shard guard; the remaining cost is the per-mint deep clone still taken under the shard guard (store Arc<[Trade]> to avoid it).

9. Trade SELECTs pull all 19 columns (incl. JSONB ix_labels) when swing/seed use ~7. trade_repo.rs:297 — per-row JSONB decode across the entire table at seed time. Add a slim projection row for the swing + seed paths.

10. Missing index for the buy/sell confirm query. trade_repo.rs:263 — find_latest_by_wallet_mint_type filters (wallet, mint, trade_type) + ORDER BY block_time DESC but the index is only (wallet, mint), forcing a sort on a latency-critical path. Add idx_trades_wallet_mint_type_time(wallet_address, mint_address, trade_type, block_time DESC).

#### If you fix only two things

- Warm/tune already done for the sender hop; focus next on the manual buy get_account (#6) and the sell-retry 250ms sleeps (#7) — both shave latency off the money path.
- Add the (wallet, mint, trade_type, block_time DESC) index (#10) — removes a sort from the buy/sell confirm query.
