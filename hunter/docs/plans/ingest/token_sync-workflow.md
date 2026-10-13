# token_sync Workflow

`token_sync` is the **historical backfill** counterpart to live LaserStream ingest. It downloads
past Pump.fun trades (bonding curve + post-migration PumpSwap AMM) for a single mint and writes them
through the same decoder and the same `TokenMetricsWrite` path as ingest, so live and backfilled rows
are byte-for-byte consistent.

See also: [laserstream-workflow.md](./laserstream-workflow.md).

## Module layout

| File | Purpose |
| ------ | --------- |
| `trading_core/src/services/token_sync.rs` | Orchestration: `run_token_sync`, `preview_sync`, `sync_amm_trades`, `persist_backfill` |
| `live/src/api/handlers/tokens/sync.rs` | HTTP handlers for `POST /api/token/sync` and `/sync/preview` |
| `trading_core/src/services/helius_rpc.rs` | `getSignaturesForAddress`, `getTransaction`, `getTransactionsForAddress` (gTFA) |
| `trading_core/src/services/laserstream_replay.rs` | gRPC replay path for free incremental syncs |
| `trading_core/src/storage/repositories/token_info_repo.rs` | Sync watermarks, `upsert_metrics`, migration status |
| `trading_core/src/storage/repositories/trade_repo.rs` | Trade inserts, `saved_signatures()`, `existing_legs()` |
| `shared/ingest/pumpfun/src/decode/mod.rs` | **Shared** `Decoder` (alias `HeliusDecoder`) |
| `live/src/ingest/db_writer.rs` | **Shared** `TokenMetricsWrite` struct |

## Two modes

### Incremental ("Fetch New") — `incremental = true`

Downloads only transactions newer than the stored watermark.

1. Read `last_synced_curve_sig` / `last_synced_curve_slot` from `tokens_info` via
   `TokenInfoRepo::get_sync_watermark()` (falls back to the latest saved trade if absent).
2. **LaserStream replay first** — if the watermark is fresh (`REPLAY_WINDOW_SECS = 72000`, 20 h),
   replay from the stored slot over Yellowstone gRPC for **zero Helius credits**.
3. **RPC fallback** — `getSignaturesForAddress(bonding_curve, until = prev_sig)` then batched
   `getTransaction`.
4. **Dedup** — skip any signature already in `TradeRepo::saved_signatures(mint, "curve")` so a
   trade already stored for that mint and venue is not downloaded again. The lookup has no slot
   cutoff: a live row can sit below the watermark.

### Full backfill ("Fetch All") — `incremental = false`

Downloads the entire history via archival `getTransactionsForAddress` (gTFA), cursor-paginated at
`GTFA_PAGE_LIMIT = 1000` txs/page (full txs in each page, ≈0.1 credit/tx vs 1 credit/tx for per-sig
`getTransaction`).

gTFA has no signature filter, so Fetch All still downloads every page. `persist_backfill` then
drops any leg whose `(mint, venue, tx_signature, leg_index)` is already in `trades`. The primary
key does not catch that leg: live `block_time` is the receive clock and sync writes the chain
second, so `ON CONFLICT DO NOTHING` inserts a second row. A stored leg is left as written.

`saved_signatures()` returns the candidates already stored for the mint and venue, with no slot
cutoff. Returns a `HashSet<String>` for O(1) membership. Incremental uses it to skip the download.
The insert-time check is per leg, so a full refetch can still add a leg that is not stored yet.

## Data flow

```
POST /api/token/sync ─▶ preflight (validate mint, bonding-curve check)
                        │
       incremental? ────┴──────────────┐
        true                           false
        │                              │
  read watermark               gTFA full backfill
        │                       (~0.1 credit/tx)
  try LaserStream replay (0 credits)   │
        │ miss → RPC:                   │
        │   getSignaturesForAddress     │
        │   dedup vs saved_signatures   │
        │   batch getTransaction        │
        └──────────────┬────────────────┘
                       ▼
        decode (shared HeliusDecoder)
        TokenCreated / Trade / Migration / Liquidity
                       ▼
        persist_backfill: bulk raw_txs + trades (skip a stored leg, else ON CONFLICT DO NOTHING) + wallet touch
                       ▼
        migrated? ─▶ sync_amm_trades  (separate watermark: last_synced_amm_*)
                       ▼
        recompute metrics → TokenMetricsWrite → TokenInfoRepo::upsert_metrics
        (ATH, volume, market_cap, trade_count, current_price, is_dead)
                       ▼
        update watermarks (last_synced_at / *_sig / *_slot)  ← only AFTER persist succeeds
                       ▼
        register AMM pool (pool_index, wake resubscribe) ─▶ stream "complete"
```

## Shared with live ingest

- **Decoder** — same `HeliusDecoder::decode_protobuf` as the live hot path. Token_sync lowers each
  RPC result (`encoding=base64`) to a `SubscribeUpdateTransaction` via `adapter_rpc::rpc_to_protobuf`
  (replay supplies the protobuf natively), then decodes it. It seeds the decoder's `pool_index` with
  the token's `{pool → mint}` up front, so post-migration AMM swaps resolve through the same
  `decode_protobuf` call (no separate explicit-pool entry point). The persisted payload is the
  verbatim protobuf wire bytes via `raw_tx::encode_payload` (inline, since token_sync has no
  DbWriter), identical to the live path. Backfill rows land with `source=1` on both `raw_txs` and
  `trades` (live ingest writes `source=0`). Sync `block_time` stays the chain second.
- **`TokenMetricsWrite`** — token_sync builds it via `metrics_from_state` and writes through the same
  `TokenInfoRepo::upsert_metrics` the DbWriter uses, so metrics are computed identically. This is the
  shared surface noted in [[laserstream-ingest-migration]].

## Entry points

- **`POST /api/token/sync`** → `sync_token()` (`live/src/api/handlers/tokens/sync.rs`).
  Body `SyncTokenBody { mint_address, include_post_migrate, incremental }`. Streams NDJSON progress
  events, ending in `complete` or `error`.
  - **Dedup gate** `state.sync_gate.try_begin()` rejects a concurrent sync of the same mint (409),
    preventing a watermark write race.
  - Runs in a background task holding a global sync-semaphore permit.
- **`POST /api/token/sync/preview`** → `preview_sync()`. Counts signatures only (no tx downloads)
  for both modes; returns `SyncPreview { new_count, new_capped, total_count, total_capped, is_migrated }`
  (capped at `PREVIEW_MAX_PAGES = 10`). No CLI command exists — HTTP only.

## Watermarks & robustness

Stored in `tokens_info`: `last_synced_at`, `last_synced_curve_sig/slot`, `last_synced_amm_sig/slot`
(bonding curve and AMM tracked separately).

- **Replay safety** — replay returns `None` if the watermark is too old → RPC fallback. The caller
  only stamps the watermark to the **max slot actually returned**, never to chain tip, so a partial
  drain can't permanently skip data.
- **Persist-before-watermark** — `persist_backfill()` is `await?`-ed *before* `update_sync_watermark()`;
  the watermark never advances past unpersisted rows.
- **Batch fallback** — a failed 100-tx JSON-RPC batch falls back to per-signature fetch (one transient
  error doesn't drop up to 100 txs).

## Tuning constants (`trading_core/src/services/token_sync.rs`)

| Constant | Value | Purpose |
| ---------- | ------- | --------- |
| `TX_BATCH_SIZE` | 100 | Signatures per JSON-RPC batch |
| `TX_BATCH_CONCURRENCY` | 5 | Concurrent in-flight batches |
| `GTFA_PAGE_LIMIT` | 1000 | gTFA full-mode page size |
| `REPLAY_WINDOW_SECS` | 72000 (20 h) | Max watermark age to trust replay |
| `PREVIEW_MAX_PAGES` | 10 | Preview count cap (~10k txs) |

## Related

- [[token-sync-credit-reduction]] — incremental dedup vs `saved_signatures`. Fetch All still downloads every page; a stored leg is not inserted again.
- [[laserstream-ingest-migration]] — ingest keeps only decoder + `TokenMetricsWrite`, shared here.
- [[amm-reserves-preswap-bug]] — a full re-sync does not overwrite a leg already stored.
